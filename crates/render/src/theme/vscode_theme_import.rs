//! Port of `Theme/VSCodeThemeImport.swift`: VS Code / Shiki theme import
//! (§11.2). Importing a VS Code theme is what makes code blocks and mermaid
//! diagrams share a single palette.

use std::collections::HashMap;

use super::theme_store::ThemeStoreError;
use crate::render_contracts::{CodeTheme, Theme, ThemeAppearance, ThemeColor, ThemePalette, TypographyConfig};
use crate::swift_compat::{self, json, smax, smin};
use crate::syntax::syntax_contracts::SyntaxToken;

pub struct VSCodeThemeImporter;

/// TextMate scopes to ask for, per token, most specific first. The first
/// query that any of the theme's selectors answers wins.
const SCOPE_QUERIES: [(SyntaxToken, &[&str]); 14] = [
    (SyntaxToken::Keyword, &["keyword.control", "keyword", "storage.type", "storage.modifier", "storage"]),
    (SyntaxToken::String, &["string.quoted", "string"]),
    (SyntaxToken::Number, &["constant.numeric"]),
    (SyntaxToken::Comment, &["comment"]),
    (SyntaxToken::Type, &["entity.name.type", "support.type", "entity.name.class", "support.class"]),
    (SyntaxToken::Function, &["entity.name.function", "support.function", "meta.function-call"]),
    (SyntaxToken::Variable, &["variable.other", "variable"]),
    (SyntaxToken::Constant, &["constant.language", "constant.character", "constant"]),
    (SyntaxToken::Operator, &["keyword.operator"]),
    (SyntaxToken::Punctuation, &["punctuation", "meta.brace"]),
    (SyntaxToken::Attribute, &["entity.other.attribute-name", "meta.decorator", "storage.type.annotation"]),
    (SyntaxToken::DiffAdded, &["markup.inserted"]),
    (SyntaxToken::DiffRemoved, &["markup.deleted"]),
    (SyntaxToken::DiffHeader, &["meta.diff.header", "markup.changed", "meta.diff.range"]),
];

#[derive(Debug, Clone, PartialEq)]
pub struct ScopeEntry {
    pub selector: String,
    pub color: Rgba,
}

impl VSCodeThemeImporter {
    pub fn theme(data: &[u8], fallback_name: &str) -> Result<Theme, ThemeStoreError> {
        // Published themes are JSONC far more often than JSON.
        let raw = VSCodeThemeFile::decode(&JsoncSanitizer::strip(data)).ok_or(ThemeStoreError::NotAVSCodeTheme)?;
        if raw.colors.is_none() && raw.token_colors.is_none() {
            return Err(ThemeStoreError::NotAVSCodeTheme);
        }

        let empty = HashMap::new();
        let colors = raw.colors.as_ref().unwrap_or(&empty);
        let scopes = raw.scope_entries();

        let background = Rgba::parse_optional(colors.get("editor.background").map(String::as_str))
            .unwrap_or_else(|| Rgba::parse(if raw.is_dark() { "#1e1e1e" } else { "#ffffff" }).unwrap());
        let text = Rgba::parse_optional(colors.get("editor.foreground").map(String::as_str))
            .or_else(|| raw.default_foreground())
            .unwrap_or_else(|| Rgba::parse(if raw.is_dark() { "#d4d4d4" } else { "#1f1f1f" }).unwrap());

        let code = VSCodeThemeImporter::code_theme(&scopes, colors, text, background);
        let palette = VSCodeThemeImporter::palette(colors, text, background, &code);

        let declared_name =
            raw.name.as_deref().map(swift_compat::trim_whitespaces_and_newlines).unwrap_or("").to_owned();
        Ok(Theme {
            name: if declared_name.is_empty() { fallback_name.to_owned() } else { declared_name },
            appearance: raw.appearance(background),
            palette,
            code,
            typography: TypographyConfig::default_config(),
        })
    }

    // MARK: - Code theme

    fn code_theme(scopes: &[ScopeEntry], colors: &HashMap<String, String>, text: Rgba, background: Rgba) -> CodeTheme {
        let mut found: HashMap<SyntaxToken, Rgba> = HashMap::new();
        for (token, queries) in SCOPE_QUERIES {
            for query in queries {
                if let Some(color) = VSCodeThemeImporter::foreground(query, scopes) {
                    found.insert(token, color);
                    break;
                }
            }
        }
        let color = |token: SyntaxToken, fallback: &dyn Fn() -> Rgba| -> ThemeColor {
            ThemeColor { raw: found.get(&token).copied().unwrap_or_else(fallback).hex_string() }
        };
        let lookup = |key: &str| Rgba::parse_optional(colors.get(key).map(String::as_str));
        let added = lookup("gitDecoration.addedResourceForeground")
            .or_else(|| lookup("editorGutter.addedBackground"))
            .unwrap_or_else(|| Rgba::parse("#3f9c53").unwrap());
        let removed = lookup("gitDecoration.deletedResourceForeground")
            .or_else(|| lookup("editorGutter.deletedBackground"))
            .unwrap_or_else(|| Rgba::parse("#c1554d").unwrap());
        let found_or_text = |token: SyntaxToken| found.get(&token).copied().unwrap_or(text);
        CodeTheme {
            keyword: color(SyntaxToken::Keyword, &|| text),
            string: color(SyntaxToken::String, &|| text),
            number: color(SyntaxToken::Number, &|| text),
            comment: color(SyntaxToken::Comment, &|| text.blended(background, 0.45)),
            r#type: color(SyntaxToken::Type, &|| found_or_text(SyntaxToken::Keyword)),
            function: color(SyntaxToken::Function, &|| found_or_text(SyntaxToken::Type)),
            variable: color(SyntaxToken::Variable, &|| text),
            constant: color(SyntaxToken::Constant, &|| found_or_text(SyntaxToken::Number)),
            operator: color(SyntaxToken::Operator, &|| text.blended(background, 0.20)),
            punctuation: color(SyntaxToken::Punctuation, &|| text.blended(background, 0.30)),
            attribute: color(SyntaxToken::Attribute, &|| found_or_text(SyntaxToken::Function)),
            diff_added: color(SyntaxToken::DiffAdded, &|| added),
            diff_removed: color(SyntaxToken::DiffRemoved, &|| removed),
            diff_header: color(SyntaxToken::DiffHeader, &|| {
                found.get(&SyntaxToken::Comment).copied().unwrap_or_else(|| text.blended(background, 0.40))
            }),
        }
    }

    // MARK: - Palette

    fn palette(colors: &HashMap<String, String>, text: Rgba, background: Rgba, code: &CodeTheme) -> ThemePalette {
        let value = |keys: &[&str]| -> Option<Rgba> {
            keys.iter().find_map(|key| Rgba::parse_optional(colors.get(*key).map(String::as_str)))
        };
        let accent = value(&["focusBorder", "textLink.foreground", "button.background"])
            .or_else(|| Rgba::parse(&code.keyword.raw))
            .unwrap_or(text);
        let added = Rgba::parse(&code.diff_added.raw).unwrap_or(text);
        let removed = Rgba::parse(&code.diff_removed.raw).unwrap_or(text);
        let error = value(&["editorError.foreground", "errorForeground"]).unwrap_or(removed);
        let warning = value(&["editorWarning.foreground"]).unwrap_or_else(|| Rgba::parse("#c9a227").unwrap());
        let c = |rgba: Rgba| ThemeColor { raw: rgba.hex_string() };

        ThemePalette {
            background: c(background),
            surface: c(value(&["editorWidget.background", "sideBar.background"])
                .unwrap_or_else(|| background.blended(text, 0.05))),
            text: c(text),
            text_secondary: c(value(&["descriptionForeground"]).unwrap_or_else(|| text.blended(background, 0.30))),
            text_faint: c(text.blended(background, 0.55)),
            heading: c(text),
            marker: c(text.blended(background, 0.65)),
            accent: c(accent),
            link: c(value(&["textLink.foreground"]).unwrap_or(accent)),
            rule: c(value(&["panel.border", "editorGroup.border"]).unwrap_or_else(|| background.blended(text, 0.18))),
            selection: c(value(&["editor.selectionBackground"]).unwrap_or_else(|| accent.blended(background, 0.70))),
            // A code block is a tint plus a left rule (§11.3), so the editor
            // background is *derived* from the page background.
            code_background: c(background.blended(text, 0.05)),
            inline_code_background: c(background.blended(text, 0.08)),
            code_rule: c(background.blended(text, 0.20)),
            rail_tick: c(text.blended(background, 0.55)),
            rail_tick_current: c(text),
            quote_rule: c(background.blended(text, 0.30)),
            change_added: c(added),
            change_removed: c(removed),
            change_modified: c(value(&["gitDecoration.modifiedResourceForeground"]).unwrap_or(warning)),
            path_missing: c(error),
            search_hit: c(value(&["editor.findMatchHighlightBackground"])
                .unwrap_or_else(|| accent.blended(background, 0.65))),
            search_hit_current: c(value(&["editor.findMatchBackground"]).unwrap_or(accent)),
            callout_note: c(value(&["editorInfo.foreground"]).unwrap_or(accent)),
            callout_warning: c(warning),
            callout_success: c(added),
            callout_danger: c(error),
            callout_important: None,
        }
    }

    // MARK: - Scope matching

    /// TextMate selector semantics, reduced to what a colour lookup needs: a
    /// selector that generalises the query beats one that narrows it, and an
    /// exact match beats both.
    pub fn foreground(scope: &str, entries: &[ScopeEntry]) -> Option<Rgba> {
        let mut best: Option<(i64, Rgba)> = None;
        for entry in entries {
            let Some(score) = VSCodeThemeImporter::score(&entry.selector, scope) else { continue };
            if best.is_none_or(|(best_score, _)| score > best_score) {
                best = Some((score, entry.color));
            }
        }
        best.map(|(_, color)| color)
    }

    fn score(selector: &str, scope: &str) -> Option<i64> {
        if swift_compat::string_eq(selector, scope) {
            return Some(1000);
        }
        let depth = swift_compat::split_on_character(selector, '.').len() as i64;
        if swift_compat::has_prefix(scope, &format!("{selector}.")) {
            return Some(100 + depth);
        }
        if swift_compat::has_prefix(selector, &format!("{scope}.")) {
            return Some(50 - depth);
        }
        None
    }
}

// MARK: - File shape

/// `VSCodeThemeFile`, decoded with the synthesized `Decodable` rules.
struct VSCodeThemeFile {
    name: Option<String>,
    r#type: Option<String>,
    colors: Option<HashMap<String, String>>,
    token_colors: Option<Vec<TokenColor>>,
}

struct TokenColor {
    scope: Option<Vec<String>>,
    settings: Option<Settings>,
}

struct Settings {
    foreground: Option<String>,
}

/// `decodeIfPresent(String.self, forKey:)`: absent or null is `None`; any
/// other non-string is a type mismatch that fails the whole decode.
fn optional_string(object: &json::Value, key: &str) -> Result<Option<String>, ()> {
    match object.get(key) {
        None | Some(json::Value::Null) => Ok(None),
        Some(json::Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(()),
    }
}

impl VSCodeThemeFile {
    fn decode(data: &[u8]) -> Option<VSCodeThemeFile> {
        let root = json::parse(data).ok()?;
        let json::Value::Object(_) = root else { return None };
        let name = optional_string(&root, "name").ok()?;
        let r#type = optional_string(&root, "type").ok()?;
        let colors = match root.get("colors") {
            None | Some(json::Value::Null) => None,
            // `LenientStringMap`: non-string and null entries are skipped.
            Some(json::Value::Object(pairs)) => Some(
                pairs
                    .iter()
                    .filter_map(|(key, value)| value.as_str().map(|text| (key.clone(), text.to_owned())))
                    .collect(),
            ),
            Some(_) => return None,
        };
        let token_colors = match root.get("tokenColors") {
            None | Some(json::Value::Null) => None,
            Some(json::Value::Array(entries)) => {
                let mut decoded = Vec::with_capacity(entries.len());
                for entry in entries {
                    decoded.push(TokenColor::decode(entry)?);
                }
                Some(decoded)
            }
            Some(_) => return None,
        };
        Some(VSCodeThemeFile { name, r#type, colors, token_colors })
    }

    fn is_dark(&self) -> bool {
        !swift_compat::lowercased(self.r#type.as_deref().unwrap_or("dark")).contains("light")
    }

    /// A `tokenColors` entry with no scope carries the editor's default
    /// foreground.
    fn default_foreground(&self) -> Option<Rgba> {
        for entry in self.token_colors.iter().flatten() {
            if !entry.scope.as_ref().is_none_or(Vec::is_empty) {
                continue;
            }
            if let Some(color) = Rgba::parse_optional(entry.settings.as_ref().and_then(|s| s.foreground.as_deref())) {
                return Some(color);
            }
        }
        None
    }

    fn scope_entries(&self) -> Vec<ScopeEntry> {
        let mut entries = Vec::new();
        for token in self.token_colors.iter().flatten() {
            let Some(color) = Rgba::parse_optional(token.settings.as_ref().and_then(|s| s.foreground.as_deref()))
            else {
                continue;
            };
            for selector in token.scope.iter().flatten() {
                entries.push(ScopeEntry { selector: selector.clone(), color });
            }
        }
        entries
    }

    fn appearance(&self, background: Rgba) -> ThemeAppearance {
        match swift_compat::lowercased(self.r#type.as_deref().unwrap_or("")).as_str() {
            "light" | "hc-light" => ThemeAppearance::Light,
            "dark" | "hc-black" => ThemeAppearance::Dark,
            _ => {
                if background.relative_luminance() < 0.5 {
                    ThemeAppearance::Dark
                } else {
                    ThemeAppearance::Light
                }
            }
        }
    }
}

impl TokenColor {
    fn decode(value: &json::Value) -> Option<TokenColor> {
        let json::Value::Object(_) = value else { return None };
        let scope = match value.get("scope") {
            None | Some(json::Value::Null) => None,
            // `scope` is a single selector, a comma-separated list, or an
            // array of either; anything else decodes as no selectors.
            Some(scope) => {
                let raw: Vec<String> = match scope {
                    json::Value::String(single) => vec![single.clone()],
                    json::Value::Array(items) => {
                        let strings: Option<Vec<String>> = items.iter().map(|item| item.as_str().map(str::to_owned)).collect();
                        strings.unwrap_or_default()
                    }
                    _ => Vec::new(),
                };
                Some(
                    raw.iter()
                        .flat_map(|selector| swift_compat::split_on_character(selector, ','))
                        .map(|piece| swift_compat::trim_whitespaces_and_newlines(piece).to_owned())
                        .filter(|piece| !piece.is_empty())
                        .collect(),
                )
            }
        };
        let settings = match value.get("settings") {
            None | Some(json::Value::Null) => None,
            Some(settings @ json::Value::Object(_)) => {
                let foreground = optional_string(settings, "foreground").ok()?;
                optional_string(settings, "fontStyle").ok()?;
                Some(Settings { foreground })
            }
            Some(_) => return None,
        };
        Some(TokenColor { scope, settings })
    }
}

// MARK: - Colour arithmetic

/// Straight sRGB components, so the importer can derive colours without an
/// appearance to resolve against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Rgba {
    /// `RGBA(hex:)`: `#rrggbb`, `#rrggbbaa`, and the `#rgb`/`#rgba` shorthands.
    pub fn parse(hex: &str) -> Option<Rgba> {
        let mut text: String = swift_compat::trim_whitespaces(hex).to_owned();
        if swift_compat::has_ascii_prefix(&text, "#") {
            text.remove(0);
        }
        let mut count = swift_compat::character_count(&text);
        // `#rgb` and `#rgba` shorthands expand by doubling each Character.
        if count == 3 || count == 4 {
            use unicode_segmentation::UnicodeSegmentation;
            text = text.graphemes(true).map(|grapheme| format!("{grapheme}{grapheme}")).collect();
            count = swift_compat::character_count(&text);
        }
        if !(count == 6 || count == 8) {
            return None;
        }
        let value = swift_compat::parse_u64_hex(&text)?;
        let has_alpha = count == 8;
        Some(Rgba {
            r: ((value >> if has_alpha { 24 } else { 16 }) & 0xFF) as f64 / 255.0,
            g: ((value >> if has_alpha { 16 } else { 8 }) & 0xFF) as f64 / 255.0,
            b: ((value >> if has_alpha { 8 } else { 0 }) & 0xFF) as f64 / 255.0,
            a: if has_alpha { (value & 0xFF) as f64 / 255.0 } else { 1.0 },
        })
    }

    /// `RGBA(_ hex: String?)`.
    pub fn parse_optional(hex: Option<&str>) -> Option<Rgba> {
        Rgba::parse(hex?)
    }

    /// `t` of the way from `self` to `other`; alpha becomes 1.
    pub fn blended(self, other: Rgba, t: f64) -> Rgba {
        Rgba {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
            a: 1.0,
        }
    }

    pub fn relative_luminance(self) -> f64 {
        0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b
    }

    pub fn hex_string(self) -> String {
        let byte = |value: f64| -> i64 { swift_compat::int_truncating((smin(smax(value, 0.0), 1.0) * 255.0).round()) };
        let base = format!("#{:02x}{:02x}{:02x}", byte(self.r), byte(self.g), byte(self.b));
        if self.a >= 0.999 { base } else { format!("{base}{:02x}", byte(self.a)) }
    }
}

// MARK: - JSONC

/// Strips `//` and `/* */` comments and trailing commas, leaving byte offsets
/// intact by overwriting with spaces.
pub struct JsoncSanitizer;

impl JsoncSanitizer {
    pub fn strip(data: &[u8]) -> Vec<u8> {
        let mut bytes = data.to_vec();
        let n = bytes.len();

        // Pass 1: replace all comments outside of strings with spaces.
        let mut i = 0;
        let mut in_string = false;
        while i < n {
            let c = bytes[i];
            if in_string {
                if c == 0x5C {
                    i += 2;
                    continue;
                }
                if c == 0x22 {
                    in_string = false;
                }
                i += 1;
                continue;
            }
            if c == 0x22 {
                in_string = true;
                i += 1;
                continue;
            }
            if c == 0x2F && i + 1 < n && bytes[i + 1] == 0x2F {
                while i < n && bytes[i] != 0x0A && bytes[i] != 0x0D {
                    bytes[i] = 0x20;
                    i += 1;
                }
                continue;
            }
            if c == 0x2F && i + 1 < n && bytes[i + 1] == 0x2A {
                bytes[i] = 0x20;
                bytes[i + 1] = 0x20;
                i += 2;
                while i < n {
                    if bytes[i] == 0x2A && i + 1 < n && bytes[i + 1] == 0x2F {
                        bytes[i] = 0x20;
                        bytes[i + 1] = 0x20;
                        i += 2;
                        break;
                    }
                    if bytes[i] != 0x0A && bytes[i] != 0x0D {
                        bytes[i] = 0x20;
                    }
                    i += 1;
                }
                continue;
            }
            i += 1;
        }

        // Pass 2: replace trailing commas with spaces.
        i = 0;
        in_string = false;
        while i < n {
            let c = bytes[i];
            if in_string {
                if c == 0x5C {
                    i += 2;
                    continue;
                }
                if c == 0x22 {
                    in_string = false;
                }
                i += 1;
                continue;
            }
            if c == 0x22 {
                in_string = true;
                i += 1;
                continue;
            }
            if c == 0x2C {
                let mut j = i + 1;
                while j < n && (bytes[j] == 0x20 || bytes[j] == 0x09 || bytes[j] == 0x0A || bytes[j] == 0x0D) {
                    j += 1;
                }
                if j < n && (bytes[j] == 0x7D || bytes[j] == 0x5D) {
                    bytes[i] = 0x20;
                }
            }
            i += 1;
        }
        bytes
    }
}
