//! Swift's `JSONEncoder` output, byte for byte (swift-foundation's writer,
//! macOS 14 and later).
//!
//! A `Codable` value is first turned into a [`JsonValue`] tree whose object
//! members are in the order the synthesized `encode(to:)` visits them (the
//! `CodingKeys` declaration order), then written by [`encode`].
//!
//! Facts reproduced here, each recorded from Swift 6.4 on macOS 26 by the
//! probes quoted in the tests:
//!
//! * Without `.sortedKeys` swift-foundation keeps a keyed container's members
//!   in a hash table, so their order changes from process to process
//!   (`SWIFT_DETERMINISTIC_HASHING` aside). There is no Swift order to match;
//!   Upleft writes the declaration order. Readers of such files must not
//!   depend on key order, and the conformance suites compare them as JSON.
//! * `.sortedKeys` sorts by Swift's `String <` (Unicode scalar order after
//!   NFC), not by `NSString.compare` as `JSONSerialization` does.
//! * `.prettyPrinted` indents by two spaces, separates keys with `" : "`, and
//!   writes an empty container as `[` newline newline indent `]`.
//! * `/` is escaped as `\/` unless `.withoutEscapingSlashes`; `\b \t \n \f \r`
//!   use their short escapes; other C0 controls are `\u00xx` with lower-case
//!   hex; DEL, U+2028 and U+2029 are written raw.
//! * Numbers: an integer prints as an integer; a `Double` or `Float` prints as
//!   its Swift `description` with a trailing `.0` removed (`1`, `1e+16`,
//!   `1e-05`, `-0`).

use upleft_swift_text::str_less;

/// A value as `JSONEncoder` sees it after `encode(to:)` has run.
#[derive(Clone, Debug, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    /// `Int`, `Int64` and the smaller signed integers.
    Int(i64),
    /// `UInt` and `UInt64`.
    UInt(u64),
    /// `Double` (and `CGFloat`, `TimeInterval`).
    Double(f64),
    Float(f32),
    String(String),
    Array(Vec<JsonValue>),
    /// Members in `encode(to:)` order.
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    /// An object from `(key, value)` pairs in encoding order.
    pub fn object<K: Into<String>>(members: impl IntoIterator<Item = (K, JsonValue)>) -> JsonValue {
        JsonValue::Object(members.into_iter().map(|(key, value)| (key.into(), value)).collect())
    }

    /// `encodeIfPresent`: appends the member only when `value` is `Some`.
    pub fn push_if_present(members: &mut Vec<(String, JsonValue)>, key: &str, value: Option<JsonValue>) {
        if let Some(value) = value {
            members.push((key.to_owned(), value));
        }
    }
}

impl From<&str> for JsonValue {
    fn from(value: &str) -> Self {
        JsonValue::String(value.to_owned())
    }
}

impl From<String> for JsonValue {
    fn from(value: String) -> Self {
        JsonValue::String(value)
    }
}

impl From<bool> for JsonValue {
    fn from(value: bool) -> Self {
        JsonValue::Bool(value)
    }
}

impl From<i64> for JsonValue {
    fn from(value: i64) -> Self {
        JsonValue::Int(value)
    }
}

impl From<isize> for JsonValue {
    fn from(value: isize) -> Self {
        JsonValue::Int(value as i64)
    }
}

impl From<i32> for JsonValue {
    fn from(value: i32) -> Self {
        JsonValue::Int(value as i64)
    }
}

impl From<f64> for JsonValue {
    fn from(value: f64) -> Self {
        JsonValue::Double(value)
    }
}

impl<T: Into<JsonValue>> From<Vec<T>> for JsonValue {
    fn from(values: Vec<T>) -> Self {
        JsonValue::Array(values.into_iter().map(Into::into).collect())
    }
}

/// `JSONEncoder.OutputFormatting`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutputFormatting {
    pub pretty_printed: bool,
    pub sorted_keys: bool,
    pub without_escaping_slashes: bool,
}

impl OutputFormatting {
    /// `[]`, the default.
    pub const DEFAULT: OutputFormatting =
        OutputFormatting { pretty_printed: false, sorted_keys: false, without_escaping_slashes: false };
    /// `[.prettyPrinted, .sortedKeys]`.
    pub const PRETTY_SORTED: OutputFormatting =
        OutputFormatting { pretty_printed: true, sorted_keys: true, without_escaping_slashes: false };
    /// `[.sortedKeys]`.
    pub const SORTED: OutputFormatting =
        OutputFormatting { pretty_printed: false, sorted_keys: true, without_escaping_slashes: false };
}

/// `JSONEncoder().encode(value)` with `outputFormatting` set.
pub fn encode(value: &JsonValue, formatting: OutputFormatting) -> Vec<u8> {
    let mut out = String::new();
    write_value(value, formatting, 0, &mut out);
    out.into_bytes()
}

/// [`encode`] as a `String`.
pub fn encode_string(value: &JsonValue, formatting: OutputFormatting) -> String {
    let mut out = String::new();
    write_value(value, formatting, 0, &mut out);
    out
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_value(value: &JsonValue, formatting: OutputFormatting, depth: usize, out: &mut String) {
    match value {
        JsonValue::Null => out.push_str("null"),
        JsonValue::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        JsonValue::Int(number) => out.push_str(&number.to_string()),
        JsonValue::UInt(number) => out.push_str(&number.to_string()),
        JsonValue::Double(number) => out.push_str(&double_text(*number)),
        JsonValue::Float(number) => out.push_str(&float_text(*number)),
        JsonValue::String(text) => write_string(text, formatting.without_escaping_slashes, out),
        JsonValue::Array(values) => {
            out.push('[');
            if formatting.pretty_printed {
                out.push('\n');
                if values.is_empty() {
                    out.push('\n');
                }
                for (index, element) in values.iter().enumerate() {
                    indent(out, depth + 1);
                    write_value(element, formatting, depth + 1, out);
                    if index + 1 < values.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                indent(out, depth);
            } else {
                for (index, element) in values.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_value(element, formatting, depth + 1, out);
                }
            }
            out.push(']');
        }
        JsonValue::Object(members) => {
            let mut ordered: Vec<&(String, JsonValue)> = members.iter().collect();
            if formatting.sorted_keys {
                // A stable sort by Swift `String <`; keys are unique.
                ordered.sort_by(|a, b| {
                    if str_less(&a.0, &b.0) {
                        std::cmp::Ordering::Less
                    } else if str_less(&b.0, &a.0) {
                        std::cmp::Ordering::Greater
                    } else {
                        std::cmp::Ordering::Equal
                    }
                });
            }
            out.push('{');
            if formatting.pretty_printed {
                out.push('\n');
                if ordered.is_empty() {
                    out.push('\n');
                }
                for (index, (key, element)) in ordered.iter().enumerate() {
                    indent(out, depth + 1);
                    write_string(key, formatting.without_escaping_slashes, out);
                    out.push_str(" : ");
                    write_value(element, formatting, depth + 1, out);
                    if index + 1 < ordered.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                indent(out, depth);
            } else {
                for (index, (key, element)) in ordered.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_string(key, formatting.without_escaping_slashes, out);
                    out.push(':');
                    write_value(element, formatting, depth + 1, out);
                }
            }
            out.push('}');
        }
    }
}

/// `JSONEncoder` string escaping.
pub fn write_string(text: &str, without_escaping_slashes: bool, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' if !without_escaping_slashes => out.push_str("\\/"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Swift's `Double.description`: the shortest digits that round-trip, in
/// decimal notation for magnitudes in [1e-4, 2^53], exponential (`1e+16`,
/// `1e-05`) otherwise.
pub fn double_description(value: f64) -> String {
    float_description(value, 9_007_199_254_740_992.0, format!("{value:e}"))
}

/// Swift's `Float.description`: as [`double_description`], decimal up to 2^24.
pub fn float32_description(value: f32) -> String {
    float_description(value as f64, 16_777_216.0, format!("{value:e}"))
}

fn float_description(value: f64, decimal_limit: f64, scientific: String) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf".into() } else { "inf".into() };
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    // `{:e}` prints the shortest round-trip digits: "1.25e16", "-5e-324".
    let (mantissa, exponent) = scientific.split_once('e').expect("exponent");
    let exponent: i32 = exponent.parse().expect("exponent digits");
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let magnitude = value.abs();
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if magnitude <= decimal_limit && exponent >= -4 {
        if exponent < 0 {
            out.push_str("0.");
            for _ in 0..(-exponent - 1) {
                out.push('0');
            }
            out.push_str(&digits);
        } else {
            let integer_digits = exponent as usize + 1;
            if digits.len() <= integer_digits {
                out.push_str(&digits);
                for _ in digits.len()..integer_digits {
                    out.push('0');
                }
                out.push_str(".0");
            } else {
                out.push_str(&digits[..integer_digits]);
                out.push('.');
                out.push_str(&digits[integer_digits..]);
            }
        }
    } else {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if exponent < 0 { '-' } else { '+' });
        out.push_str(&format!("{:02}", exponent.abs()));
    }
    out
}

/// `JSONEncoder`'s text for a `Double`: its `description` without a trailing
/// `.0`. Non-finite values throw in Swift (`invalidValue`) under the default
/// `nonConformingFloatEncodingStrategy`; callers must not pass them.
pub fn double_text(value: f64) -> String {
    let text = double_description(value);
    match text.strip_suffix(".0") {
        Some(stripped) => stripped.to_owned(),
        None => text,
    }
}

/// `JSONEncoder`'s text for a `Float`.
pub fn float_text(value: f32) -> String {
    let text = float32_description(value);
    match text.strip_suffix(".0") {
        Some(stripped) => stripped.to_owned(),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &JsonValue, formatting: OutputFormatting) -> String {
        encode_string(value, formatting)
    }

    // Swift 6.4, macOS 26:
    //   let enc = JSONEncoder(); enc.outputFormatting = [.sortedKeys]
    //   enc.encode(["b": 0, "B": 1, …])
    #[test]
    fn sorted_keys_use_swift_string_order() {
        let keys = [
            "b", "B", "a", "A", "a10", "a2", "a_b", "aB", "Ab", "é", "e", "f", "z", "_x", "-y", "1", "10", "2", "ä",
            "ａ", "ab", "a b", "Z", "ß", "ss", "st",
        ];
        let value = JsonValue::object(keys.iter().enumerate().map(|(index, key)| (*key, JsonValue::Int(index as i64))));
        assert_eq!(
            text(&value, OutputFormatting::SORTED),
            r#"{"-y":14,"1":15,"10":16,"2":17,"A":3,"Ab":8,"B":1,"Z":22,"_x":13,"a":2,"a b":21,"a10":4,"a2":5,"aB":7,"a_b":6,"ab":20,"b":0,"e":10,"f":11,"ss":24,"st":25,"z":12,"ß":23,"ä":18,"é":9,"ａ":19}"#
        );
    }

    #[test]
    fn numbers_match_json_encoder() {
        let values = [
            1.0, 1e16, 1e-5, 123456.789, 0.30000000000000004, -0.0, 5e-324, 1.7976931348623157e308, 100.0, 1e15,
            12345678901234567890.0,
        ];
        let array = JsonValue::Array(values.iter().map(|value| JsonValue::Double(*value)).collect());
        assert_eq!(
            text(&array, OutputFormatting::DEFAULT),
            "[1,1e+16,1e-05,123456.789,0.30000000000000004,-0,5e-324,1.7976931348623157e+308,100,1000000000000000,1.2345678901234567e+19]"
        );
        let floats = JsonValue::Array(
            [0.1f32, 16777216.0, 1e-5, 3.4e38, 1.0].iter().map(|value| JsonValue::Float(*value)).collect(),
        );
        assert_eq!(text(&floats, OutputFormatting::DEFAULT), "[0.1,16777216,1e-05,3.4e+38,1]");
        assert_eq!(text(&JsonValue::Array(vec![JsonValue::Int(i64::MAX)]), OutputFormatting::DEFAULT), "[9223372036854775807]");
        assert_eq!(text(&JsonValue::Array(vec![JsonValue::UInt(u64::MAX)]), OutputFormatting::DEFAULT), "[18446744073709551615]");
    }

    #[test]
    fn escapes_match_json_encoder() {
        let mut s = String::new();
        for unit in 0u32..0x20 {
            s.push(char::from_u32(unit).unwrap());
        }
        s.push_str("\u{7f}\u{80}\u{a0}\u{2028}\u{2029}\u{feff}/");
        let value = JsonValue::Array(vec![JsonValue::String(s)]);
        assert_eq!(
            text(&value, OutputFormatting::SORTED),
            "[\"\\u0000\\u0001\\u0002\\u0003\\u0004\\u0005\\u0006\\u0007\\b\\t\\n\\u000b\\f\\r\\u000e\\u000f\\u0010\\u0011\\u0012\\u0013\\u0014\\u0015\\u0016\\u0017\\u0018\\u0019\\u001a\\u001b\\u001c\\u001d\\u001e\\u001f\u{7f}\u{80}\u{a0}\u{2028}\u{2029}\u{feff}\\/\"]"
        );
        let slash = JsonValue::Array(vec![JsonValue::from("a/b")]);
        let formatting = OutputFormatting { without_escaping_slashes: true, ..OutputFormatting::DEFAULT };
        assert_eq!(text(&slash, formatting), "[\"a/b\"]");
    }

    #[test]
    fn pretty_printing_matches_json_encoder() {
        // struct In: Codable { var a: [Int]; var b: [String: [Int]]; var c: [[Int]] }
        let value = JsonValue::object([
            ("a", JsonValue::Array(vec![])),
            (
                "b",
                JsonValue::object([
                    ("y", JsonValue::Array(vec![JsonValue::Int(1), JsonValue::Int(2)])),
                    ("x", JsonValue::Array(vec![])),
                ]),
            ),
            ("c", JsonValue::Array(vec![JsonValue::Array(vec![]), JsonValue::Array(vec![JsonValue::Int(3)])])),
        ]);
        assert_eq!(
            text(&value, OutputFormatting::PRETTY_SORTED),
            "{\n  \"a\" : [\n\n  ],\n  \"b\" : {\n    \"x\" : [\n\n    ],\n    \"y\" : [\n      1,\n      2\n    ]\n  },\n  \"c\" : [\n    [\n\n    ],\n    [\n      3\n    ]\n  ]\n}"
        );
        assert_eq!(text(&JsonValue::Array(vec![]), OutputFormatting::PRETTY_SORTED), "[\n\n]");
        assert_eq!(text(&JsonValue::Object(vec![]), OutputFormatting::PRETTY_SORTED), "{\n\n}");
    }
}
