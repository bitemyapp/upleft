//! Editing/FrontMatterEditing.swift — the small, source-preserving front
//! matter editor.
//!
//! Intentionally not a YAML value tree: complex YAML stays in Source mode,
//! where an incomplete model cannot change it.

use std::collections::HashSet;

use unicode_normalization::UnicodeNormalization;

use crate::contracts::TextEdit;
use crate::model::{FrontMatter, FrontMatterField, ParsedDocument};
use crate::ns_range::NSRange;
use crate::swift_text::{
    self, CharSet,
    ns::{NSStringExt, string_from_utf16, utf16},
};

/// Values the editor can write.
#[derive(Clone, Debug, PartialEq)]
pub enum FrontMatterValue {
    Text(String),
    Boolean(bool),
    Number(f64),
    List(Vec<String>),
}

impl FrontMatterValue {
    pub fn string(value: impl Into<String>) -> FrontMatterValue {
        FrontMatterValue::Text(value.into())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum FrontMatterEditOperation {
    Set { key: String, value: FrontMatterValue },
    Add { key: String, value: FrontMatterValue },
    Remove { key: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FrontMatterSourceFallback {
    MissingFrontMatter,
    MalformedFence,
    NestedYAML,
    CommentsNotSupported,
    AnchorsOrAliasesNotSupported,
    BlockScalarNotSupported,
    AmbiguousField,
    UnsupportedValue,
    InvalidSourceRange,
    SourceChanged,
}

impl FrontMatterSourceFallback {
    pub fn raw_value(&self) -> &'static str {
        match self {
            FrontMatterSourceFallback::MissingFrontMatter => "missingFrontMatter",
            FrontMatterSourceFallback::MalformedFence => "malformedFence",
            FrontMatterSourceFallback::NestedYAML => "nestedYAML",
            FrontMatterSourceFallback::CommentsNotSupported => "commentsNotSupported",
            FrontMatterSourceFallback::AnchorsOrAliasesNotSupported => "anchorsOrAliasesNotSupported",
            FrontMatterSourceFallback::BlockScalarNotSupported => "blockScalarNotSupported",
            FrontMatterSourceFallback::AmbiguousField => "ambiguousField",
            FrontMatterSourceFallback::UnsupportedValue => "unsupportedValue",
            FrontMatterSourceFallback::InvalidSourceRange => "invalidSourceRange",
            FrontMatterSourceFallback::SourceChanged => "sourceChanged",
        }
    }
}

impl std::fmt::Display for FrontMatterSourceFallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.raw_value())
    }
}

impl std::error::Error for FrontMatterSourceFallback {}

/// One source-local front matter edit. `expected` prevents an old card from
/// changing a newer buffer.
#[derive(Clone, Debug, PartialEq)]
pub struct FrontMatterEditProposal {
    pub range: NSRange,
    pub replacement: String,
    pub summary: String,
    pub expected: String,
}

impl FrontMatterEditProposal {
    pub fn new(range: NSRange, replacement: impl Into<String>, summary: impl Into<String>, expected: impl Into<String>) -> Self {
        FrontMatterEditProposal { range, replacement: replacement.into(), summary: summary.into(), expected: expected.into() }
    }

    pub fn edit(&self) -> TextEdit {
        TextEdit::new(self.range, self.replacement.clone(), self.summary.clone(), None)
    }

    pub fn applying(&self, source: &str) -> Option<String> {
        let mut ns = utf16(source);
        if !(self.range.location >= 0
            && self.range.upper_bound() <= ns.as_slice().length()
            && swift_text::str_eq(&ns.as_slice().substring(self.range), &self.expected))
        {
            return None;
        }
        ns.splice(self.range.as_usize_range(), self.replacement.encode_utf16());
        Some(string_from_utf16(&ns))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrontMatterEditResult {
    pub proposal: Option<FrontMatterEditProposal>,
    pub fallback: Option<FrontMatterSourceFallback>,
}

impl FrontMatterEditResult {
    pub fn new(proposal: Option<FrontMatterEditProposal>, fallback: Option<FrontMatterSourceFallback>) -> FrontMatterEditResult {
        FrontMatterEditResult { proposal, fallback }
    }

    fn fail(reason: FrontMatterSourceFallback) -> FrontMatterEditResult {
        FrontMatterEditResult::new(None, Some(reason))
    }
}

pub struct FrontMatterEditing;

impl FrontMatterEditing {
    pub fn propose(document: &ParsedDocument, operation: &FrontMatterEditOperation) -> FrontMatterEditResult {
        let Some(front) = &document.front_matter else {
            let opening = document.range_of_line(1);
            return FrontMatterEditResult::fail(
                if opening.length > 0 && swift_text::str_eq(swift_text::trim_whitespaces(&document.substring(opening)), "---") {
                    FrontMatterSourceFallback::MalformedFence
                } else {
                    FrontMatterSourceFallback::MissingFrontMatter
                },
            );
        };
        match Self::validate(document, front) {
            None => Self::make(document, front, operation),
            Some(failure) => FrontMatterEditResult::fail(failure),
        }
    }

    pub fn proposal(document: &ParsedDocument, operation: &FrontMatterEditOperation) -> Option<FrontMatterEditProposal> {
        Self::propose(document, operation).proposal
    }

    pub fn set(document: &ParsedDocument, key: impl Into<String>, value: FrontMatterValue) -> FrontMatterEditResult {
        Self::propose(document, &FrontMatterEditOperation::Set { key: key.into(), value })
    }

    pub fn add(document: &ParsedDocument, key: impl Into<String>, value: FrontMatterValue) -> FrontMatterEditResult {
        Self::propose(document, &FrontMatterEditOperation::Add { key: key.into(), value })
    }

    pub fn remove(document: &ParsedDocument, key: impl Into<String>) -> FrontMatterEditResult {
        Self::propose(document, &FrontMatterEditOperation::Remove { key: key.into() })
    }

    fn make(document: &ParsedDocument, front_matter: &FrontMatter, operation: &FrontMatterEditOperation) -> FrontMatterEditResult {
        let key = match operation {
            FrontMatterEditOperation::Set { key, .. }
            | FrontMatterEditOperation::Add { key, .. }
            | FrontMatterEditOperation::Remove { key } => key.as_str(),
        };
        if !Self::is_simple_key(key) {
            return FrontMatterEditResult::fail(FrontMatterSourceFallback::UnsupportedValue);
        }
        let mut matches: Vec<FrontMatterField> =
            front_matter.fields.iter().filter(|field| swift_text::case_insensitive_equal(&field.key, key)).cloned().collect();
        if matches.is_empty()
            && let Some(empty) = Self::empty_field(key, document, front_matter)
        {
            matches = vec![empty];
        }
        if matches.len() > 1 {
            return FrontMatterEditResult::fail(FrontMatterSourceFallback::AmbiguousField);
        }

        match operation {
            FrontMatterEditOperation::Set { value, .. } => {
                let Some(field) = matches.first() else {
                    return Self::make_add(document, front_matter, key, value);
                };
                let raw = document.substring(field.value_range);
                let Some(rendered) = Self::render(value, &raw) else {
                    return FrontMatterEditResult::fail(FrontMatterSourceFallback::UnsupportedValue);
                };
                Self::result(document, field.value_range, rendered, format!("Set {}", field.key))
            }
            FrontMatterEditOperation::Add { value, .. } => {
                if !matches.is_empty() {
                    return FrontMatterEditResult::fail(FrontMatterSourceFallback::AmbiguousField);
                }
                Self::make_add(document, front_matter, key, value)
            }
            FrontMatterEditOperation::Remove { .. } => {
                let Some(field) = matches.first() else {
                    return FrontMatterEditResult::fail(FrontMatterSourceFallback::AmbiguousField);
                };
                let ns = document.utf16.as_slice();
                let start = ns.line_start_before(field.key_range.location);
                let end = ns.line_end_after(field.value_range.upper_bound());
                let range = NSRange::new(start, end - start);
                Self::result(document, range, String::new(), format!("Remove {}", field.key))
            }
        }
    }

    fn make_add(document: &ParsedDocument, front_matter: &FrontMatter, key: &str, value: &FrontMatterValue) -> FrontMatterEditResult {
        let Some(rendered) = Self::render(value, "") else {
            return FrontMatterEditResult::fail(FrontMatterSourceFallback::UnsupportedValue);
        };
        let newline = Self::line_ending(&document.substring(front_matter.range));
        let insertion = format!("{key}: {rendered}{newline}");
        let range = NSRange::new(front_matter.body_range.upper_bound(), 0);
        Self::result(document, range, insertion, format!("Add {key}"))
    }

    fn result(document: &ParsedDocument, range: NSRange, replacement: String, summary: String) -> FrontMatterEditResult {
        let ns = document.utf16.as_slice();
        if !(range.location >= 0 && range.upper_bound() <= ns.length()) {
            return FrontMatterEditResult::fail(FrontMatterSourceFallback::InvalidSourceRange);
        }
        FrontMatterEditResult::new(Some(FrontMatterEditProposal::new(range, replacement, summary, ns.substring(range))), None)
    }

    fn validate(document: &ParsedDocument, front_matter: &FrontMatter) -> Option<FrontMatterSourceFallback> {
        let source = document.substring(front_matter.body_range);
        let lines = swift_text::components_separated_by_set(&source, CharSet::Newlines);
        // `Set<String>`: canonical-equivalence keys, i.e. NFC forms.
        let mut keys: HashSet<String> = HashSet::new();
        for line in &lines {
            if line.is_empty() {
                continue;
            }
            if swift_text::first(line).is_some_and(swift_text::is_whitespace) {
                return Some(FrontMatterSourceFallback::NestedYAML);
            }
            let trimmed = swift_text::trim_whitespaces(line);
            if swift_text::has_prefix(trimmed, "#") {
                return Some(FrontMatterSourceFallback::CommentsNotSupported);
            }
            if swift_text::contains(trimmed, "&") || swift_text::contains(trimmed, "*") {
                return Some(FrontMatterSourceFallback::AnchorsOrAliasesNotSupported);
            }
            if let Some(colon) = swift_text::first_index_of(trimmed, ':') {
                let raw = swift_text::trim_whitespaces(after_character(trimmed, colon));
                if swift_text::str_eq(raw, "|")
                    || swift_text::str_eq(raw, ">")
                    || swift_text::has_prefix(raw, "| ")
                    || swift_text::has_prefix(raw, "> ")
                {
                    return Some(FrontMatterSourceFallback::BlockScalarNotSupported);
                }
                let name = swift_text::trim_whitespaces(&trimmed[..colon]);
                if !Self::is_simple_key(name) {
                    return Some(FrontMatterSourceFallback::UnsupportedValue);
                }
                let folded = swift_text::lowercased(name);
                if !keys.insert(swift_key(&folded)) {
                    return Some(FrontMatterSourceFallback::AmbiguousField);
                }
            } else {
                return Some(FrontMatterSourceFallback::UnsupportedValue);
            }
        }
        None
    }

    fn is_simple_key(key: &str) -> bool {
        !key.is_empty()
            && swift_text::all_satisfy(key, |c| {
                swift_text::is_letter(c)
                    || swift_text::is_number(c)
                    || swift_text::char_is(c, '_')
                    || swift_text::char_is(c, '-')
                    || swift_text::char_is(c, '.')
                    || swift_text::char_is(c, ' ')
            })
    }

    fn render(value: &FrontMatterValue, raw: &str) -> Option<String> {
        let characters: Vec<&str> = swift_text::graphemes(raw).collect();
        let left_count = characters.iter().take_while(|c| swift_text::is_whitespace(c)).count();
        let right_count = characters[left_count..].iter().rev().take_while(|c| swift_text::is_whitespace(c)).count();
        let safe_right_count = right_count.min(characters.len().saturating_sub(left_count));
        let leading: String = characters[..left_count].concat();
        let trailing: String =
            if safe_right_count == 0 { String::new() } else { characters[characters.len() - safe_right_count..].concat() };
        let token_end = left_count.max(characters.len() - safe_right_count);
        let token: &[&str] = &characters[left_count..token_end];
        let rendered = match value {
            FrontMatterValue::Text(text) => {
                let first = token.first().copied();
                let last = token.last().copied();
                if first.is_some_and(|c| swift_text::char_is(c, '"')) && last.is_some_and(|c| swift_text::char_is(c, '"')) {
                    Self::double_quoted(text)
                } else if first.is_some_and(|c| swift_text::char_is(c, '\'')) && last.is_some_and(|c| swift_text::char_is(c, '\'')) {
                    format!("'{}'", swift_text::replacing_occurrences(text, "'", "''"))
                } else if Self::needs_quote(text) {
                    Self::double_quoted(text)
                } else {
                    text.clone()
                }
            }
            FrontMatterValue::Boolean(value) => (if *value { "true" } else { "false" }).to_owned(),
            FrontMatterValue::Number(value) => {
                if !value.is_finite() {
                    return None;
                }
                if value.round() == *value && value.abs() <= i64::MAX as f64 {
                    // `Int64(value)` traps at exactly 2^63, which passes the
                    // `<= Double(Int64.max)` check.
                    assert!(*value < 9_223_372_036_854_775_808.0, "Double value cannot be converted to Int64 because it is outside the representable range");
                    (*value as i64).to_string()
                } else {
                    swift_double_description(*value)
                }
            }
            FrontMatterValue::List(items) => {
                let rendered: Vec<String> =
                    items.iter().map(|item| if Self::needs_quote(item) { Self::double_quoted(item) } else { item.clone() }).collect();
                format!("[{}]", rendered.join(", "))
            }
        };
        Some(format!("{leading}{rendered}{trailing}"))
    }

    fn double_quoted(text: &str) -> String {
        let escaped = swift_text::replacing_occurrences(&swift_text::replacing_occurrences(text, "\\", "\\\\"), "\"", "\\\"");
        format!("\"{escaped}\"")
    }

    fn needs_quote(text: &str) -> bool {
        if text.is_empty() {
            return true;
        }
        if swift_text::first(text).is_some_and(swift_text::is_whitespace) || swift_text::last(text).is_some_and(swift_text::is_whitespace) {
            return true;
        }
        let lower = swift_text::lowercased(text);
        if ["true", "false", "yes", "no", "null", "~"].iter().any(|word| swift_text::str_eq(word, &lower)) {
            return true;
        }
        if swift_double_parses(text) {
            return true;
        }
        // `text.contains(where: { ":#[]{}&,*!%@`".contains($0) })`.
        if swift_text::graphemes(text).any(|c| ":#[]{}&,*!%@`".chars().any(|special| swift_text::char_is(c, special))) {
            return true;
        }
        swift_text::has_prefix(text, "-") || swift_text::has_prefix(text, "?")
    }

    fn line_ending(source: &str) -> &'static str {
        // Character-wise: a lone "\r" is not found inside a CR LF.
        if swift_text::contains(source, "\r\n") {
            return "\r\n";
        }
        if swift_text::contains(source, "\r") {
            return "\r";
        }
        "\n"
    }

    fn empty_field(key: &str, document: &ParsedDocument, front_matter: &FrontMatter) -> Option<FrontMatterField> {
        let ns = document.utf16.as_slice();
        let body = front_matter.body_range;
        let mut line = document.line_at(body.location);
        while line <= document.line_at(body.location.max(body.upper_bound() - 1)) {
            let range = document.range_of_line(line);
            if !(range.location >= body.location && range.location < body.upper_bound()) {
                break;
            }
            let text = ns.substring(range);
            let Some(colon) = swift_text::first_index_of(&text, ':') else {
                line += 1;
                continue;
            };
            let name = swift_text::trim_whitespaces(&text[..colon]);
            let raw = after_character(&text, colon);
            if swift_text::case_insensitive_equal(name, key) && swift_text::trim_whitespaces(raw).is_empty() {
                let colon_offset = swift_text::utf16_count(&text[..colon]);
                let value_range = NSRange::new(range.location + colon_offset + 1, swift_text::utf16_count(raw));
                return Some(FrontMatterField::new(
                    name,
                    "",
                    NSRange::new(range.location, swift_text::utf16_count(name)),
                    value_range,
                ));
            }
            line += 1;
        }
        None
    }
}

/// `s[s.index(after: at)...]` for the Character starting at byte `at`.
fn after_character(s: &str, at: usize) -> &str {
    let width = swift_text::first(&s[at..]).map_or(0, str::len);
    &s[at + width..]
}

/// A Swift `String` set/dictionary key: canonical equivalence is equality of
/// the NFC forms.
fn swift_key(s: &str) -> String {
    if s.is_ascii() { s.to_owned() } else { s.nfc().collect() }
}

/// `Double(text) != nil` (Swift 6.4's `LosslessStringConvertible` parse):
/// no leading whitespace, the whole string (up to a NUL) consumed; decimal
/// and hexadecimal floats, `inf`/`infinity`, `nan`, `nan(…)` and `snan`, all
/// case-insensitive, with an optional sign. Recorded from Swift, see tests.
fn swift_double_parses(text: &str) -> bool {
    let bytes = text.as_bytes();
    let bytes = match bytes.iter().position(|&b| b == 0) {
        Some(nul) => &bytes[..nul],
        None => bytes,
    };
    match bytes.first() {
        None | Some(9..=13 | 32) => return false,
        _ => {}
    }
    let rest = match bytes[0] {
        b'+' | b'-' => &bytes[1..],
        _ => bytes,
    };
    if rest.eq_ignore_ascii_case(b"inf") || rest.eq_ignore_ascii_case(b"infinity") || rest.eq_ignore_ascii_case(b"snan") {
        return true;
    }
    if rest.len() >= 3 && rest[..3].eq_ignore_ascii_case(b"nan") {
        let payload = &rest[3..];
        if payload.is_empty() {
            return true;
        }
        if payload[0] != b'(' {
            return false;
        }
        // The first ")" after "nan(" must end the string.
        return payload[1..].iter().position(|&b| b == b')').is_some_and(|close| close + 2 == payload.len());
    }
    let (digits, exponent_markers, is_digit): (&[u8], &[u8], fn(&u8) -> bool) =
        if rest.len() >= 2 && rest[0] == b'0' && (rest[1] == b'x' || rest[1] == b'X') {
            (&rest[2..], b"pP", u8::is_ascii_hexdigit)
        } else {
            (rest, b"eE", u8::is_ascii_digit)
        };
    let mut i = 0;
    let mut mantissa_digits = 0;
    while i < digits.len() && is_digit(&digits[i]) {
        i += 1;
        mantissa_digits += 1;
    }
    if i < digits.len() && digits[i] == b'.' {
        i += 1;
        while i < digits.len() && is_digit(&digits[i]) {
            i += 1;
            mantissa_digits += 1;
        }
    }
    if mantissa_digits == 0 {
        return false;
    }
    if i < digits.len() && exponent_markers.contains(&digits[i]) {
        i += 1;
        if i < digits.len() && (digits[i] == b'+' || digits[i] == b'-') {
            i += 1;
        }
        let exponent_start = i;
        while i < digits.len() && digits[i].is_ascii_digit() {
            i += 1;
        }
        if i == exponent_start {
            return false;
        }
    }
    i == digits.len()
}

/// `String(describing: Double)` / `"\(value)"`: the shortest round-trip
/// digits, in exponential form when the magnitude is above 2^53 or below
/// 1e-4 (`1e-05`, `1e+16`), otherwise decimal with at least one fraction
/// digit (`0.5`, `2.0`).
fn swift_double_description(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf".to_owned() } else { "inf".to_owned() };
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0.0".to_owned() } else { "0.0".to_owned() };
    }
    // Rust's `{:e}` is the shortest round-trip digits: `d.ddde±x`.
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific.split_once('e').expect("`{:e}` has an exponent");
    let exponent: i32 = exponent.parse().expect("`{:e}` exponent is an integer");
    let sign = if value < 0.0 { "-" } else { "" };
    if value.abs() > 9_007_199_254_740_992.0 || exponent < -4 {
        let exponent_sign = if exponent < 0 { '-' } else { '+' };
        return format!("{sign}{mantissa}e{exponent_sign}{:02}", exponent.abs());
    }
    let decimal = format!("{}", value.abs());
    if decimal.contains('.') { format!("{sign}{decimal}") } else { format!("{sign}{decimal}.0") }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Recorded from Swift 6.4: `Double(s) != nil`.
    #[test]
    fn double_parse_matches_swift() {
        let accepted = [
            "1", "inf", "Inf", "INFINITY", "infinity", "nan", "NaN", "nan(1)", "nan()", "snan", "-nan", "0x1p3", "0x1.8", "1e5", "1E5",
            ".5", "5.", "+.5", "+1", "-0", "1e400", "1e-400", "0e400", "00012", "1.5e3", "0x1P-2", "1\0x", "SNAN", "-snan", "+snan",
            "+nan", "+inf", "-inf", "nan(abc_1)", "nan(-1)", "0x.8p1", "0X1P3", "1.e5", "1e05", "0x1p+3", "1e+5", "0x1.", "0.", "sNaN",
            "nAn", "iNf", "0xAbC", "nan(ab c)", "nan(é)", "nan(()", "NAN(1)", "-nan(5)", "Infinity", "-INFINITY", "nan(\n)", "0x1e3",
            "9999999999999999999999999999",
        ];
        let rejected = [
            " 1", "1 ", "infinit", "0x", "-", "", "1_0", "1,5", "\u{661}", "1e", "1e+", " ", "\t1", "1\n", "\u{FF11}", "1d", "1f", "e5",
            ".", "+", "-.e1", "true", "~", "snan(1)", "nan(", "nan(1", "infinityx", "0x1.8p", "0x.p1", ".e5", "+-1", "--1", "0x1p", "0xg",
            "infinite", "inf(", "1e5x", "1.2.3", "1e5.5", "nan(1)x", "é", "1é", "0x1.8p3a", "1e-", "0b1", "0o7", "1__0", "nan(a)b)",
            "nan())", "nan()()", "nan)", "infin", "in", "n", "s", "sn", "sna", "snan()", "0x1p3.5", "0xp1", "0x.", "00x1", "0x1p-",
            "0x1p+", "1e+-5",
        ];
        for s in accepted {
            assert!(swift_double_parses(s), "{s:?} should parse");
        }
        for s in rejected {
            assert!(!swift_double_parses(s), "{s:?} should not parse");
        }
    }

    // Recorded from Swift 6.4: `String(d)`.
    #[test]
    fn double_description_matches_swift() {
        let cases: [(f64, &str); 22] = [
            (0.5, "0.5"),
            (1.5, "1.5"),
            (-2.25, "-2.25"),
            (0.1, "0.1"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1e-7, "1e-07"),
            (123456.789, "123456.789"),
            (1e15 + 0.5, "1000000000000000.5"),
            (4503599627370495.5, "4503599627370495.5"),
            (1e16, "1e+16"),
            (1e19, "1e+19"),
            (9.3e18, "9.3e+18"),
            (1.7976931348623157e308, "1.7976931348623157e+308"),
            (5e-324, "5e-324"),
            (2.2250738585072014e-308, "2.2250738585072014e-308"),
            (1.0 / 3.0, "0.3333333333333333"),
            (9007199254740992.0, "9007199254740992.0"),
            (9007199254740994.0, "9.007199254740994e+15"),
            (9.223372036854775807e18, "9.223372036854776e+18"),
            (1.2e-5, "1.2e-05"),
            (-0.00012, "-0.00012"),
        ];
        for (value, expected) in cases {
            assert_eq!(swift_double_description(value), expected, "{value:e}");
        }
    }

    #[test]
    fn render_numbers() {
        assert_eq!(FrontMatterEditing::render(&FrontMatterValue::Number(42.0), " 1 ").as_deref(), Some(" 42 "));
        assert_eq!(FrontMatterEditing::render(&FrontMatterValue::Number(-0.0), "").as_deref(), Some("0"));
        assert_eq!(FrontMatterEditing::render(&FrontMatterValue::Number(2.5), "").as_deref(), Some("2.5"));
        assert_eq!(FrontMatterEditing::render(&FrontMatterValue::Number(1e19), "").as_deref(), Some("1e+19"));
        assert_eq!(FrontMatterEditing::render(&FrontMatterValue::Number(f64::NAN), ""), None);
    }

    #[test]
    fn render_keeps_quotes_and_padding() {
        let text = |s: &str| FrontMatterValue::Text(s.to_owned());
        assert_eq!(FrontMatterEditing::render(&text("New"), "  \"Old\"  ").as_deref(), Some("  \"New\"  "));
        assert_eq!(FrontMatterEditing::render(&text("it's"), "'x'").as_deref(), Some("'it''s'"));
        assert_eq!(FrontMatterEditing::render(&text("a: b"), "").as_deref(), Some("\"a: b\""));
        assert_eq!(FrontMatterEditing::render(&text("12"), "").as_deref(), Some("\"12\""));
        assert_eq!(FrontMatterEditing::render(&text("plain"), "   ").as_deref(), Some("   plain"));
        // A lone `"` is both first and last Character of the token.
        assert_eq!(FrontMatterEditing::render(&text("x"), "\"").as_deref(), Some("\"x\""));
    }
}
