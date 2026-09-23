//! Rust counterparts of `oracle/Sources/downright-oracle/HighlightDump.swift`:
//! `highlight` (fenced code through the built-in highlighter) and
//! `vscode-theme` (the VS Code importer and the theme decoder).

use serde_json::Value;
use upleft_render::engine::syntax_run_cache::SyntaxRunCache;
use upleft_render::render_contracts::Theme;
use upleft_render::syntax::builtin_syntax_highlighter::BuiltinSyntaxHighlighter;
use upleft_render::syntax::language_catalog;
use upleft_render::syntax::syntax_contracts::{SyntaxHighlighter, SyntaxRun};
use upleft_render::theme::theme_store::{ThemeStore, ThemeStoreError};
use upleft_render::theme::vscode_theme_import::{JsoncSanitizer, VSCodeThemeImporter};

use super::json::Object;
use super::style_sheet::theme_dump;

struct Fence {
    line: usize,
    info: Vec<u16>,
    code: Vec<u16>,
}

fn is_blank(unit: u16) -> bool {
    unit == 0x20 || unit == 0x09 || unit == 0x0D
}

/// (fence character, run length, index after the run), indices into `units`.
fn fence_run(units: &[u16], start: usize, end: usize) -> Option<(u16, usize, usize)> {
    let mut index = start;
    let mut spaces = 0;
    while index < end && units[index] == 0x20 && spaces < 3 {
        index += 1;
        spaces += 1;
    }
    if !(index < end && (units[index] == 0x60 || units[index] == 0x7E)) {
        return None;
    }
    let marker = units[index];
    let mut length = 0;
    while index < end && units[index] == marker {
        index += 1;
        length += 1;
    }
    if length >= 3 { Some((marker, length, index)) } else { None }
}

fn fences(units: &[u16]) -> Vec<Fence> {
    let mut lines: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    for (index, unit) in units.iter().enumerate() {
        if *unit == 0x0A {
            lines.push((start, index + 1));
            start = index + 1;
        }
    }
    if start < units.len() {
        lines.push((start, units.len()));
    }

    let mut result = Vec::new();
    let mut line_index = 0;
    while line_index < lines.len() {
        let (line_start, line_end) = lines[line_index];
        let Some((marker, length, after)) = fence_run(units, line_start, line_end) else {
            line_index += 1;
            continue;
        };
        let mut info: Vec<u16> = units[after..line_end].to_vec();
        while info.last().is_some_and(|&unit| is_blank(unit) || unit == 0x0A) {
            info.pop();
        }
        let leading = info.iter().take_while(|&&unit| is_blank(unit)).count();
        info.drain(..leading);
        let body_start = line_end;
        let mut body_end = units.len();
        let mut closing = lines.len();
        for (probe, &(candidate_start, candidate_end)) in lines.iter().enumerate().skip(line_index + 1) {
            if let Some((close_marker, close_length, close_after)) = fence_run(units, candidate_start, candidate_end)
                && close_marker == marker
                && close_length >= length
                && units[close_after..candidate_end].iter().all(|&unit| is_blank(unit) || unit == 0x0A)
            {
                body_end = candidate_start;
                closing = probe;
                break;
            }
        }
        result.push(Fence { line: line_index, info, code: units[body_start..body_start.max(body_end)].to_vec() });
        line_index = closing + 1;
    }
    result
}

fn language(info: &[u16]) -> Option<String> {
    let word: Vec<u16> = info.iter().copied().take_while(|&unit| unit != 0x20 && unit != 0x09).collect();
    if word.is_empty() { None } else { Some(String::from_utf16_lossy(&word)) }
}

fn runs_json(runs: &[SyntaxRun]) -> Value {
    Value::Array(
        runs.iter()
            .map(|run| {
                Value::Array(vec![run.range.location.into(), run.range.length.into(), run.token.raw_value().into()])
            })
            .collect(),
    )
}

fn optional(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |text| Value::String(text.to_owned()))
}

fn language_probes() -> Vec<String> {
    let mut probes: Vec<String> = language_catalog::CANONICAL_NAMES.iter().map(|name| name.to_string()).collect();
    let mut aliases: Vec<&str> = language_catalog::ALIASES.iter().map(|(alias, _)| *alias).collect();
    aliases.sort();
    probes.extend(aliases.into_iter().map(str::to_owned));
    probes.extend(
        [
            "SH",
            " Rust ",
            "\tPython3\n",
            "C++",
            "OBJECTIVE-C",
            "m\u{212A}d",
            "\u{200B}py\u{200B}",
            "",
            "   ",
            "unknown",
            "ts ",
            "İ",
            "ΣΑΣ",
            "rust,ignore",
            "{.python}",
            "json5",
            "Diff",
            "HTML",
        ]
        .into_iter()
        .map(str::to_owned),
    );
    probes
}

pub fn document(text: &str) -> Value {
    let units: Vec<u16> = text.encode_utf16().collect();
    let highlighter = BuiltinSyntaxHighlighter::shared();
    let cache = SyntaxRunCache::new(4);
    let fences: Vec<Value> = fences(&units)
        .into_iter()
        .map(|fence| {
            let info = String::from_utf16_lossy(&fence.info);
            let language = language(&fence.info);
            let direct = highlighter.highlight(&fence.code, language.as_deref());
            let cached_first = cache.runs(&fence.code, language.as_deref(), highlighter);
            let cached_second = cache.runs(&fence.code, language.as_deref(), highlighter);
            Object::new()
                .with("line", fence.line)
                .with("info", info.clone())
                .with("language", optional(language.as_deref()))
                .with("canonical", optional(language.as_deref().and_then(BuiltinSyntaxHighlighter::canonical_language)))
                .with("canonicalInfo", optional(BuiltinSyntaxHighlighter::canonical_language(&info)))
                .with("supports", language.as_deref().is_some_and(|language| highlighter.supports(language)))
                .with("codeLength", fence.code.len())
                .with("runs", runs_json(&direct))
                .with("cacheAgrees", *cached_first == *direct && *cached_second == *direct)
                .with("infoRuns", runs_json(&highlighter.highlight(&fence.code, Some(&info))))
                .build()
        })
        .collect();
    Object::new()
        .with("fences", Value::Array(fences))
        .with(
            "languages",
            Value::Array(
                language_probes()
                    .iter()
                    .map(|probe| {
                        Value::Array(vec![
                            Value::String(probe.clone()),
                            optional(BuiltinSyntaxHighlighter::canonical_language(probe)),
                        ])
                    })
                    .collect(),
            ),
        )
        .with(
            "supported",
            Value::Array(BuiltinSyntaxHighlighter::supported_languages().iter().map(|name| (*name).into()).collect()),
        )
        .build()
}

/// `vscode-theme`.
pub fn vscode_theme(data: &[u8], path: &std::path::Path) -> Value {
    // `url.deletingPathExtension().lastPathComponent`.
    let fallback_name = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
    let stripped = JsoncSanitizer::strip(data);
    let imported = match VSCodeThemeImporter::theme(data, &fallback_name) {
        Ok(theme) => Object::new()
            .with("theme", theme_dump::theme(&theme))
            .with("validation", theme_dump::validation(&theme))
            .with("export", theme.encode_pretty_sorted())
            .with("slug", ThemeStore::slug(&theme.name))
            .build(),
        Err(error) => Object::new()
            .with(
                "error",
                match error {
                    ThemeStoreError::NotAVSCodeTheme => "notAVSCodeTheme".to_owned(),
                    other => other.error_description(),
                },
            )
            .build(),
    };
    let decoded = match Theme::decode_json(data) {
        Ok(theme) => Object::new()
            .with("theme", theme_dump::theme(&theme))
            .with("export", theme.encode_pretty_sorted())
            .build(),
        Err(_) => Value::Null,
    };
    Object::new()
        .with("fallbackName", fallback_name)
        .with("stripped", String::from_utf8_lossy(&stripped).into_owned())
        .with("imported", imported)
        .with("decoded", decoded)
        .build()
}
