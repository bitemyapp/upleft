//! Mirrors `oracle/Sources/downright-oracle/CoreTextDump.swift` (`core-text`)
//! and `CoreTextBench.swift` (`bench-core-text`): MarkdownCore's text-level
//! functions over one file, with no parse tree involved.

use std::path::Path;
use std::time::Instant;

use serde_json::Value;
use upleft_core::contracts::ChangeHunk;
use upleft_core::derived::{Slug, SourceScanner};
use upleft_core::document_io::{DocumentIO, DocumentIOError};
use upleft_core::extensions::callout_scanner::CalloutScanner;
use upleft_core::extensions::fence_language::{FenceLanguage, FenceLanguageKind};
use upleft_core::extensions::front_matter_scanner::FrontMatterScanner;
use upleft_core::extensions::math_scanner::{MathMatch, MathScanner};
use upleft_core::extensions::path_token_scanner::{Match as PathTokenMatch, PathTokenScanner};
use upleft_core::extensions::wikilink_scanner::{WikilinkMatch, WikilinkScanner};
use upleft_core::hashing::FNV;
use upleft_core::metrics::Metrics;
use upleft_core::model::{ByteFidelity, LineEnding};
use upleft_core::myers::{Myers, Step};
use upleft_core::safe_html::SafeHTMLParser;
use upleft_core::source_positions::SourceMap;
use upleft_core::swift_text::{self, ns::NSStringExt};
use upleft_core::text_diff::TextDiff;
use upleft_core::NSRange;

use super::json::{self, Object};
use super::parse::{front_matter, int, path_token, range, safe_html, string};
use super::Failure;

pub fn run(input: &Path, output: &Path) -> Result<(), Failure> {
    let data = std::fs::read(input)?;
    json::write(&document(&data, input), output)?;
    Ok(())
}

pub fn document(data: &[u8], url: &Path) -> Value {
    let mut object = Object::new().with("byteCount", data.len() as i64).with(
        "readHead",
        Value::Array(
            [1usize, 2, 3, 4, 5, 7, 64, 4096]
                .iter()
                .map(|&limit| string(DocumentIO::read_head(url, limit as isize).as_deref()))
                .collect(),
        ),
    );
    let (text, fidelity_value) = match DocumentIO::decode_snapshot(data, url) {
        Ok(decoded) => decoded,
        Err(error) => {
            return object.with("decode", Object::new().with("error", error_kind(&error)).build()).build();
        }
    };
    object = object.with("decode", Object::new().with("text", text.as_str()).with("fidelity", fidelity(fidelity_value)).build());
    object = match DocumentIO::encoded_data(&text, fidelity_value) {
        Ok(encoded) => object.with(
            "encode",
            Object::new()
                .with("roundTrip", encoded.as_slice() == data)
                .with("sha256", DocumentIO::content_hash_data(&encoded))
                .build(),
        ),
        Err(error) => object.with("encode", Object::new().with("error", error_kind(&error)).build()),
    };
    object = object
        .with("contentHash", DocumentIO::content_hash(&text))
        .with("dominantLineEnding", line_ending(DocumentIO::dominant_line_ending(&text)));
    for (key, value) in analyses(&text) {
        object = object.with(key, value);
    }
    object.build()
}

pub fn analyses(text: &str) -> Vec<(&'static str, Value)> {
    let map = SourceMap::new(text);
    let ns = map.text.as_slice();
    let whole = NSRange::new(0, ns.length());
    let lines: Vec<NSRange> = (0..map.line_count()).map(|line| map.content_range_of_line(line)).collect();
    let mut out: Vec<(&'static str, Value)> = Vec::new();

    out.push(("length", int(ns.length())));
    out.push(("fnvUTF8", json::hex(FNV::hash_str(text))));
    out.push(("fnvUTF16", json::hex(FNV::hash_range(ns, whole))));
    let step = 1.max(ns.length() / 97);
    out.push((
        "sourceMap",
        Object::new()
            .with("lineStarts", Value::Array(map.line_starts.iter().map(|&v| int(v)).collect()))
            .with("lineEnds", Value::Array(map.line_ends.iter().map(|&v| int(v)).collect()))
            .with("mayContainHTML", map.may_contain_html)
            .with(
                "offsets",
                Value::Array(
                    (0..(map.line_count() + 1).min(200))
                        .map(|line| {
                            Value::Array([1, 2, 3, 5, 8, 13, 21, 34, 55, 89].iter().map(|&column| int(map.offset(line + 1, column))).collect())
                        })
                        .collect(),
                ),
            )
            .with(
                "lineContaining",
                Value::Array((0..=ns.length()).step_by(step as usize).map(|offset| int(map.line_containing(offset))).collect()),
            )
            .build(),
    ));

    let scan = SourceScanner::new(&map);
    let mut keys: Vec<&String> = scan.link_references.keys().collect();
    keys.sort_by(|a, b| swift_text::str_cmp(a, b));
    out.push((
        "sourceScanner",
        Object::new()
            .with(
                "footnoteDefinitions",
                Value::Array(
                    scan.footnote_definitions
                        .iter()
                        .map(|definition| {
                            Object::new()
                                .with("identifier", definition.identifier.as_str())
                                .with("markerRange", range(definition.marker_range))
                                .with("range", range(definition.range))
                                .build()
                        })
                        .collect(),
                ),
            )
            .with(
                "linkReferences",
                Value::Array(
                    keys.into_iter()
                        .map(|key| {
                            let reference = &scan.link_references[key];
                            Object::new()
                                .with("key", key.as_str())
                                .with("identifier", reference.identifier.as_str())
                                .with("destination", reference.destination.as_str())
                                .with("title", string(reference.title.as_deref()))
                                .with("range", range(reference.range))
                                .build()
                        })
                        .collect(),
                ),
            )
            .build(),
    ));

    out.push(("frontMatter", FrontMatterScanner::scan(&map).as_ref().map_or(Value::Null, front_matter)));

    out.push((
        "callouts",
        Value::Array(
            lines
                .iter()
                .enumerate()
                .filter(|(_, line)| first_non_blank(ns, **line).is_some_and(|i| ns.character_at(i) == 0x3E))
                .map(|(index, line)| {
                    let found = CalloutScanner::scan(&map, *line);
                    Object::new()
                        .with("line", index as i64)
                        .with(
                            "match",
                            found.map_or(Value::Null, |found| {
                                Object::new()
                                    .with("kind", found.kind.raw_value())
                                    .with("title", string(found.title.as_deref()))
                                    .with("markerRange", range(found.marker_range))
                                    .build()
                            }),
                        )
                        .build()
                })
                .collect(),
        ),
    ));

    out.push(("math", Value::Array(MathScanner::matches(ns, whole).iter().map(math).collect())));
    out.push((
        "mathPerLine",
        Value::Array(lines.iter().map(|&line| Value::Array(MathScanner::matches(ns, line).iter().map(math).collect())).collect()),
    ));
    out.push(("wikilinks", Value::Array(WikilinkScanner::matches(ns, whole).iter().map(wikilink).collect())));
    out.push((
        "wikilinksPerLine",
        Value::Array(lines.iter().map(|&line| Value::Array(WikilinkScanner::matches(ns, line).iter().map(wikilink).collect())).collect()),
    ));
    out.push(("pathTokens", Value::Array(PathTokenScanner::matches(ns, whole).iter().map(path_match).collect())));
    out.push((
        "pathTokensPerLine",
        Value::Array(lines.iter().map(|&line| Value::Array(PathTokenScanner::matches(ns, line).iter().map(path_match).collect())).collect()),
    ));
    out.push((
        "codeSpans",
        Value::Array(
            code_spans(ns, &lines)
                .into_iter()
                .map(|span| {
                    Object::new()
                        .with("range", range(span))
                        .with("pathToken", PathTokenScanner::code_span_match(ns, span).as_ref().map_or(Value::Null, path_match))
                        .build()
                })
                .collect(),
        ),
    ));

    let paragraphs = paragraph_ranges(ns, &lines);
    out.push(("safeHTML", SafeHTMLParser::parse(text, None).as_ref().map_or(Value::Null, safe_html)));
    out.push((
        "paragraphs",
        Value::Array(
            paragraphs
                .iter()
                .map(|&paragraph| {
                    Object::new()
                        .with("range", range(paragraph))
                        .with("safeHTML", SafeHTMLParser::parse(text, Some(paragraph)).as_ref().map_or(Value::Null, safe_html))
                        .with("mathBlock", MathScanner::whole_block(ns, paragraph).as_ref().map_or(Value::Null, math))
                        .build()
                })
                .collect(),
        ),
    ));

    out.push((
        "fences",
        Value::Array(
            fences(ns, &lines)
                .into_iter()
                .map(|fence| {
                    let info = ns.substring(fence.info);
                    let body = ns.substring(fence.body);
                    Object::new()
                        .with("line", fence.line as i64)
                        .with("info", info.as_str())
                        .with("kind", fence_kind(FenceLanguage::kind(Some(&info))))
                        .with("guess", string(FenceLanguage::guess_bridged(&body, !map.is_ascii).as_deref()))
                        .build()
                })
                .collect(),
        ),
    ));

    out.push((
        "slugs",
        Value::Array(
            lines
                .iter()
                .filter(|line| line.length > 0 && ns.character_at(line.location) == 0x23)
                .map(|line| {
                    let mut start = line.location;
                    while start < line.upper_bound() && ns.character_at(start) == 0x23 {
                        start += 1;
                    }
                    while start < line.upper_bound() && ns.character_at(start) == 0x20 {
                        start += 1;
                    }
                    let title = ns.substring(NSRange::new(start, line.upper_bound() - start));
                    let slug = Slug::make(&title);
                    Value::Array(vec![title.into(), slug.into()])
                })
                .collect(),
        ),
    ));

    let metrics = Metrics::metrics_of(text);
    out.push((
        "metrics",
        Object::new()
            .with("words", int(metrics.words))
            .with("characters", int(metrics.characters))
            .with("sentences", int(metrics.sentences))
            .with("readMinutes", json::double(metrics.read_minutes))
            .build(),
    ));

    let mutated = mutate(text);
    let edited = insert_x(text);
    let mutated_length = swift_text::utf16_count(&mutated);
    let edited_length = swift_text::utf16_count(&edited);
    out.push((
        "diff",
        Object::new()
            .with("mutatedLength", int(mutated_length))
            .with("forward", hunks(&TextDiff::hunks(text, &mutated), mutated_length))
            .with("backward", hunks(&TextDiff::hunks(&mutated, text), ns.length()))
            .with("oneCharacter", hunks(&TextDiff::hunks(text, &edited), edited_length))
            .with("identical", hunks(&TextDiff::hunks(text, text), ns.length()))
            .with("myers", myers(text, &mutated, 4096))
            .with("myersCapped", myers(text, &mutated, 8))
            .build(),
    ));
    out
}

// MARK: Segmentation (mirrors the Swift exactly)

fn is_blank(unit: u16) -> bool {
    unit == 0x20 || unit == 0x09
}

pub fn first_non_blank(ns: &[u16], range: NSRange) -> Option<isize> {
    let mut index = range.location;
    while index < range.upper_bound() && is_blank(ns.character_at(index)) {
        index += 1;
    }
    if index < range.upper_bound() { Some(index) } else { None }
}

pub fn paragraph_ranges(ns: &[u16], lines: &[NSRange]) -> Vec<NSRange> {
    let mut out = Vec::new();
    let mut start: Option<isize> = None;
    let mut end = 0;
    for &line in lines {
        if first_non_blank(ns, line).is_none() {
            if let Some(s) = start.take() {
                out.push(NSRange::new(s, end - s));
            }
        } else {
            if start.is_none() {
                start = Some(line.location);
            }
            end = line.upper_bound();
        }
    }
    if let Some(s) = start {
        out.push(NSRange::new(s, end - s));
    }
    out
}

fn code_spans(ns: &[u16], lines: &[NSRange]) -> Vec<NSRange> {
    let mut out = Vec::new();
    for &line in lines {
        let mut open: Option<isize> = None;
        for index in line.indices() {
            if ns.character_at(index) == 0x60 {
                match open.take() {
                    Some(o) => out.push(NSRange::new(o + 1, index - o - 1)),
                    None => open = Some(index),
                }
            }
        }
    }
    out
}

struct Fence {
    line: usize,
    info: NSRange,
    body: NSRange,
}

fn fences(ns: &[u16], lines: &[NSRange]) -> Vec<Fence> {
    let run = |range: NSRange| -> Option<(u16, isize, isize)> {
        let first = first_non_blank(ns, range)?;
        let unit = ns.character_at(first);
        if !(unit == 0x60 || unit == 0x7E) {
            return None;
        }
        let mut index = first;
        while index < range.upper_bound() && ns.character_at(index) == unit {
            index += 1;
        }
        if index - first >= 3 { Some((unit, first, index - first)) } else { None }
    };
    let mut out = Vec::new();
    let mut line = 0usize;
    while line < lines.len() {
        let Some(open) = run(lines[line]) else {
            line += 1;
            continue;
        };
        let info_start = open.1 + open.2;
        let info = NSRange::new(info_start, lines[line].upper_bound() - info_start);
        let body_start = if line + 1 < lines.len() { lines[line + 1].location } else { lines[line].upper_bound() };
        let mut closing = line + 1;
        while closing < lines.len() {
            if let Some(close) = run(lines[closing])
                && close.0 == open.0
                && close.2 >= open.2
            {
                break;
            }
            closing += 1;
        }
        let body_end = if closing < lines.len() { lines[closing].location } else { ns.length() };
        out.push(Fence { line, info, body: NSRange::new(body_start, 0.max(body_end - body_start)) });
        line = closing + 1;
    }
    out
}

/// Deletes every 7th line and inserts a marker line before every 11th,
/// splitting on LF code units.
pub fn mutate(text: &str) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut lines: Vec<&[u16]> = Vec::new();
    let mut start = 0;
    for (index, &unit) in units.iter().enumerate() {
        if unit == 0x0A {
            lines.push(&units[start..=index]);
            start = index + 1;
        }
    }
    if start < units.len() {
        lines.push(&units[start..]);
    }
    let mut out: Vec<u16> = Vec::with_capacity(units.len());
    for (index, line) in lines.iter().enumerate() {
        if index % 11 == 10 {
            out.extend(format!("<<inserted {index}>>\n").encode_utf16());
        }
        if index % 7 == 6 {
            continue;
        }
        out.extend_from_slice(line);
    }
    String::from_utf16_lossy(&out)
}

/// drbench's one-character edit: an `x` at the middle Character.
pub fn insert_x(text: &str) -> String {
    let middle = swift_text::count(text) / 2;
    let offset = text.len() - swift_text::drop_first(text, middle).len();
    let mut edited = String::with_capacity(text.len() + 1);
    edited.push_str(&text[..offset]);
    edited.push('x');
    edited.push_str(&text[offset..]);
    edited
}

// MARK: Values

fn error_kind(error: &DocumentIOError) -> String {
    match error {
        DocumentIOError::Undecodable(..) => "undecodable".into(),
        DocumentIOError::Unencodable(encoding) => format!("unencodable:{}", encoding.raw_value()),
        _ => "other".into(),
    }
}

fn fidelity(value: ByteFidelity) -> Value {
    Object::new()
        .with("encoding", value.encoding.raw_value())
        .with("hasBOM", value.has_bom)
        .with("lineEnding", line_ending(value.line_ending))
        .with("hasTrailingNewline", value.has_trailing_newline)
        .build()
}

fn line_ending(value: LineEnding) -> &'static str {
    match value {
        LineEnding::Lf => "lf",
        LineEnding::Crlf => "crlf",
        LineEnding::Cr => "cr",
    }
}

fn math(found: &MathMatch) -> Value {
    Object::new()
        .with("range", range(found.range))
        .with("contentRange", range(found.content_range))
        .with("isDisplay", found.is_display)
        .build()
}

fn wikilink(found: &WikilinkMatch) -> Value {
    Object::new()
        .with("range", range(found.range))
        .with("targetRange", range(found.target_range))
        .with("target", found.target.as_str())
        .with("label", string(found.label.as_deref()))
        .build()
}

fn path_match(found: &PathTokenMatch) -> Value {
    Object::new().with("range", range(found.range)).with("token", path_token(&found.token)).build()
}

fn fence_kind(kind: FenceLanguageKind) -> &'static str {
    match kind {
        FenceLanguageKind::Mermaid => "mermaid",
        FenceLanguageKind::Math => "math",
        FenceLanguageKind::Code => "code",
    }
}

fn hunks(hunks: &[ChangeHunk], new_length: isize) -> Value {
    Value::Array(
        hunks
            .iter()
            .map(|hunk| {
                Object::new()
                    .with("kind", hunk.kind.raw_value())
                    .with("newRange", range(hunk.new_range))
                    .with("oldRange", range(hunk.old_range))
                    .with("wordRanges", Value::Array(hunk.word_ranges.iter().map(|&r| range(r)).collect()))
                    .with("anchor", range(TextDiff::anchor_range(hunk, new_length)))
                    .build()
            })
            .collect(),
    )
}

/// Myers over line hashes, run-length encoded as
/// `[kind, firstOldIndex, firstNewIndex, count]` (`-1` for an absent side).
fn myers(old: &str, new: &str, max_distance: isize) -> Value {
    let old_ns: Vec<u16> = old.encode_utf16().collect();
    let new_ns: Vec<u16> = new.encode_utf16().collect();
    let old_hashes: Vec<u64> = TextDiff::lines(&old_ns).iter().map(|&line| FNV::hash_range(&old_ns, line)).collect();
    let new_hashes: Vec<u64> = TextDiff::lines(&new_ns).iter().map(|&line| FNV::hash_range(&new_ns, line)).collect();
    let Some(script) = Myers::diff(&old_hashes, &new_hashes, max_distance) else { return Value::Null };
    let mut runs: Vec<(&str, isize, isize, isize)> = Vec::new();
    for step in script {
        let (kind, o, n) = match step {
            Step::Equal { old_index, new_index } => ("equal", old_index, new_index),
            Step::Delete { old_index } => ("delete", old_index, -1),
            Step::Insert { new_index } => ("insert", -1, new_index),
        };
        if let Some(last) = runs.last_mut()
            && last.0 == kind
            && o == (if last.1 < 0 { -1 } else { last.1 + last.3 })
            && n == (if last.2 < 0 { -1 } else { last.2 + last.3 })
        {
            last.3 += 1;
        } else {
            runs.push((kind, o, n, 1));
        }
    }
    Value::Array(runs.into_iter().map(|(k, o, n, c)| Value::Array(vec![k.into(), int(o), int(n), int(c)])).collect())
}

// MARK: - bench-core-text

/// drbench's stages that need a parse tree, over the same documents.
fn parse_stages(document5k: &str, edited_text: &str, sink: &mut usize) -> Vec<(&'static str, Value)> {
    use upleft_core::ast_diff::ASTDiff;
    use upleft_core::parser::MarkdownParser;
    use upleft_core::structural_zoom::StructuralZoom;
    use upleft_core::tidy::TidyDocument;
    use upleft_core::{ParseOptions, ZoomLevel};

    let mut results = Vec::new();
    results.push(measure("MarkdownParser.parse, all passes", 15, || {
        *sink = sink.wrapping_add(MarkdownParser::parse(document5k).length as usize)
    }));
    let off = ParseOptions {
        detect_front_matter: false,
        detect_math: false,
        detect_callouts: false,
        detect_wikilinks: false,
        detect_path_tokens: false,
        detect_mermaid: false,
        ..ParseOptions::DEFAULT
    };
    results.push(measure("  … extension passes off", 15, || {
        *sink = sink.wrapping_add(MarkdownParser::parse_with(document5k, off).length as usize)
    }));
    let variants: [(&'static str, ParseOptions); 4] = [
        ("  … without path tokens", ParseOptions { detect_path_tokens: false, ..ParseOptions::DEFAULT }),
        ("  … without math", ParseOptions { detect_math: false, ..ParseOptions::DEFAULT }),
        ("  … without wikilinks", ParseOptions { detect_wikilinks: false, ..ParseOptions::DEFAULT }),
        ("  … without callouts", ParseOptions { detect_callouts: false, ..ParseOptions::DEFAULT }),
    ];
    for (name, options) in variants {
        results.push(measure(name, 15, || *sink = sink.wrapping_add(MarkdownParser::parse_with(document5k, options).length as usize)));
    }
    let baseline = MarkdownParser::parse(document5k);
    let edited = MarkdownParser::parse(edited_text);
    results.push(measure("ASTDiff.dirtySet, one-character edit", 25, || {
        *sink = sink.wrapping_add(ASTDiff::dirty_set(Some(&baseline), &edited).ranges.len())
    }));
    let long = agent_document(6_000);
    let document100k = swift_text::prefix(&long, 100_000).to_owned();
    results.push(measure("parse 100 KB", 30, || *sink = sink.wrapping_add(MarkdownParser::parse(&document100k).length as usize)));
    results.push(measure("StructuralZoom.plan, skeleton", 10, || {
        *sink = sink.wrapping_add(StructuralZoom::plan(&baseline, ZoomLevel::Skeleton).visible_ranges.len())
    }));
    results.push(measure("Metrics.metrics", 10, || *sink = sink.wrapping_add(Metrics::metrics_for(document5k).words as usize)));
    results.push(measure("TidyDocument.plan", 10, || *sink = sink.wrapping_add(TidyDocument::plan(&baseline).len())));
    results
}

/// drbench's `agentDocument(lines:)`, verbatim.
pub fn agent_document(target_lines: usize) -> String {
    let mut out = String::new();
    let mut line_count = 0;
    let mut index = 0;
    while line_count < target_lines {
        index += 1;
        let block = format!(
            "## Section {index}\n\nA paragraph with **bold**, `code`, a [link](https://example.com), and a\npath reference `src/module{index}/file.ts:{index}` that resolves.\n\n- [ ] first task for section {index}\n- [x] second task\n- a plain item\n"
        );
        line_count += block.matches('\n').count();
        out.push_str(&block);
        if index % 7 == 0 {
            out.push_str(&format!("```swift\nlet value{index} = {index}\nfunc compute{index}() -> Int {{ value{index} * 2 }}\n```\n\n"));
            line_count += 6;
        }
        if index % 11 == 0 {
            out.push_str(&format!("| column | value |\n|---|--:|\n| a | {index} |\n| b | {} |\n\n", index * 2));
            line_count += 6;
        }
        if index % 13 == 0 {
            out.push_str("> [!NOTE]\n> A callout, because agents emit these constantly.\n\n");
            line_count += 3;
        }
    }
    out
}

fn percentile(ascending: &[f64], p: f64) -> f64 {
    let rank = (p * ascending.len() as f64).ceil() as isize;
    ascending[((ascending.len() as isize - 1).min(0.max(rank - 1))) as usize]
}

fn measure(label: &'static str, runs: usize, mut body: impl FnMut()) -> (&'static str, Value) {
    body();
    let mut samples: Vec<f64> = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        body();
        samples.push(start.elapsed().as_nanos() as f64 / 1_000_000.0);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let (p50, p95) = (percentile(&samples, 0.50), percentile(&samples, 0.95));
    let max = *samples.last().unwrap();
    println!("  {label:<44}  p50 {p50:8.3} ms   p95 {p95:8.3} ms   max {max:8.3} ms (n={runs})");
    (
        label,
        Object::new()
            .with("p50", json::double(p50))
            .with("p95", json::double(p95))
            .with("max", json::double(max))
            .with("runs", runs as i64)
            .build(),
    )
}

pub fn bench(input: &Path, output: &Path) -> Result<(), Failure> {
    let text = super::markup::read_text(input)?;
    let edited = insert_x(&text);
    let map = SourceMap::new(&text);
    let ns = map.text.as_slice();
    let whole = NSRange::new(0, ns.length());
    let data = text.as_bytes().to_vec();
    let lines: Vec<NSRange> = (0..map.line_count()).map(|line| map.content_range_of_line(line)).collect();
    let quote_lines: Vec<NSRange> =
        lines.iter().copied().filter(|&line| first_non_blank(ns, line).is_some_and(|i| ns.character_at(i) == 0x3E)).collect();
    let paragraphs = paragraph_ranges(ns, &lines);
    let dev_null = Path::new("/dev/null");
    let mut sink = 0usize;

    let mut results = Vec::new();
    results.push(measure("TextDiff.hunks, external rewrite", 10, || sink = sink.wrapping_add(TextDiff::hunks(&text, &edited).len())));
    results.push(measure("TextDiff.hunks, identical", 25, || sink = sink.wrapping_add(TextDiff::hunks(&text, &text).len())));
    results.push(measure("Metrics.metrics(of:) whole text", 10, || sink = sink.wrapping_add(Metrics::metrics_of(&text).words as usize)));
    results.push(measure("Metrics.wordCount", 25, || sink = sink.wrapping_add(Metrics::word_count(&text) as usize)));
    results.push(measure("SourceMap", 25, || sink = sink.wrapping_add(SourceMap::new(&text).line_count() as usize)));
    results.push(measure("SourceScanner", 25, || sink = sink.wrapping_add(SourceScanner::new(&map).footnote_definitions.len())));
    results.push(measure("FrontMatterScanner.scan", 25, || {
        sink = sink.wrapping_add(FrontMatterScanner::scan(&map).map_or(0, |f| f.fields.len()))
    }));
    results.push(measure("MathScanner.matches, whole text", 25, || sink = sink.wrapping_add(MathScanner::matches(ns, whole).len())));
    results.push(measure("WikilinkScanner.matches, whole text", 25, || {
        sink = sink.wrapping_add(WikilinkScanner::matches(ns, whole).len())
    }));
    results.push(measure("PathTokenScanner.matches, whole text", 25, || {
        sink = sink.wrapping_add(PathTokenScanner::matches(ns, whole).len())
    }));
    results.push(measure("PathTokenScanner.matches, per line", 25, || {
        for &line in &lines {
            sink = sink.wrapping_add(PathTokenScanner::matches(ns, line).len());
        }
    }));
    results.push(measure("CalloutScanner.scan, quote lines", 25, || {
        for &line in &quote_lines {
            sink = sink.wrapping_add(CalloutScanner::scan(&map, line).is_some() as usize);
        }
    }));
    results.push(measure("SafeHTMLParser.parse, paragraphs", 25, || {
        for &paragraph in &paragraphs {
            sink = sink.wrapping_add(SafeHTMLParser::parse_ns(ns, Some(paragraph)).is_some() as usize);
        }
    }));
    results.push(measure("FNV.hash, UTF-16 whole text", 25, || sink = sink.wrapping_add(FNV::hash_range(ns, whole) as usize)));
    results.push(measure("DocumentIO.decodeSnapshot", 25, || {
        sink = sink.wrapping_add(DocumentIO::decode_snapshot(&data, dev_null).map_or(0, |(text, _)| text.len()))
    }));
    results.extend(parse_stages(&text, &edited, &mut sink));
    if sink == 42 {
        println!();
    }
    let mut object = Object::new();
    for (label, value) in results {
        object = object.with(label, value);
    }
    json::write(&object.build(), output)?;
    Ok(())
}
