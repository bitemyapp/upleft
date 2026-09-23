//! The Swift standard library and Foundation behaviours the ported code relies
//! on, reproduced exactly (each one was probed against the macOS 26 runtime).
//!
//! - String, Character and CharacterSet behaviour comes from
//!   `upleft-swift-text` (tables generated from the Swift runtime, re-checked
//!   by the `unicode` suite); the wrappers below keep this crate's call sites.
//! - `FixedWidthInteger(_:radix:)` accepts one leading `+` or `-`; `-0` parses
//!   as zero for an unsigned type.
//! - `Swift.min` / `Swift.max` are `y < x ? y : x` and `y >= x ? y : x`, which
//!   differ from `f64::min`/`max` for NaN and signed zeros.

use upleft_swift_text::{self as swift_text, CharSet};

/// `CharacterSet.whitespaces` (Zs, tab and U+200B).
#[inline]
pub fn is_whitespace(c: char) -> bool {
    CharSet::Whitespaces.contains(c)
}

/// `CharacterSet.whitespacesAndNewlines`.
#[inline]
pub fn is_whitespace_or_newline(c: char) -> bool {
    CharSet::WhitespacesAndNewlines.contains(c)
}

/// `trimmingCharacters(in: .whitespaces)`.
#[inline]
pub fn trim_whitespaces(s: &str) -> &str {
    swift_text::trim_whitespaces(s)
}

/// `trimmingCharacters(in: .whitespacesAndNewlines)`.
#[inline]
pub fn trim_whitespaces_and_newlines(s: &str) -> &str {
    swift_text::trim_whitespaces_and_newlines(s)
}

/// `String.lowercased()`: per-scalar full lowercase mapping, no context.
#[inline]
pub fn lowercased(s: &str) -> String {
    swift_text::lowercased(s)
}

/// `UInt64(text, radix: 16)`.
pub fn parse_u64_hex(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    let (negative, digits) = match bytes.first() {
        Some(b'+') => (false, &bytes[1..]),
        Some(b'-') => (true, &bytes[1..]),
        _ => (false, bytes),
    };
    if digits.is_empty() {
        return None;
    }
    let mut value: u64 = 0;
    for &byte in digits {
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return None,
        };
        value = value.checked_mul(16)?.checked_add(digit as u64)?;
    }
    if negative && value != 0 {
        return None;
    }
    Some(value)
}

/// `Swift.min(x, y)` for floating point: `y < x ? y : x`.
#[inline(always)]
pub fn smin(x: f64, y: f64) -> f64 {
    if y < x { y } else { x }
}

/// `Swift.max(x, y)` for floating point: `y >= x ? y : x`.
#[inline(always)]
pub fn smax(x: f64, y: f64) -> f64 {
    if y >= x { y } else { x }
}

/// `Swift.pow` on `CGFloat`: the C library's `pow`. The exponent goes through
/// `black_box` so LLVM cannot rewrite a constant exponent (`pow(x, 0.5)` into
/// `sqrt`, `pow(x, 2)` into `x * x`) where the Swift call reaches libm.
#[inline(always)]
pub fn pow(x: f64, y: f64) -> f64 {
    x.powf(std::hint::black_box(y))
}

/// `String.count`: extended grapheme clusters.
#[inline]
pub fn character_count(s: &str) -> usize {
    swift_text::count(s)
}

/// `String.hasPrefix(_:)` for an ASCII `prefix`: `Character` by `Character`
/// under canonical equivalence (so `"\u{212A}x"` has the prefix `"K"`).
#[inline]
pub fn has_ascii_prefix(s: &str, prefix: &str) -> bool {
    debug_assert!(prefix.is_ascii());
    swift_text::has_prefix(s, prefix)
}

/// `String.hasPrefix(_:)`.
#[inline]
pub fn has_prefix(s: &str, prefix: &str) -> bool {
    swift_text::has_prefix(s, prefix)
}

/// `s.split(separator: character)` with `omittingEmptySubsequences: true`.
#[inline]
pub fn split_on_character(s: &str, separator: char) -> Vec<&str> {
    swift_text::split_default(s, separator)
}

/// Swift `String ==`: Unicode canonical equivalence.
#[inline]
pub fn string_eq(a: &str, b: &str) -> bool {
    swift_text::str_eq(a, b)
}

/// A key under which canonically equivalent strings collide, for maps that
/// stand in for a Swift `Dictionary<String, _>` or `Set<String>`.
#[inline]
pub fn string_key(s: &str) -> String {
    swift_text::string_key(s)
}

/// Swift `String <`: the NFC-normalised scalars, lexicographically.
#[inline]
pub fn string_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    swift_text::str_cmp(a, b)
}

/// `Character.isLetter`: the first scalar's `Alphabetic` property.
#[inline]
pub fn is_letter(c: char) -> bool {
    swift_text::scalar_is_alphabetic(c)
}

/// `Character.isNumber`: the first scalar has a numeric type (including the
/// Unihan numerics such as 一, which `char::is_numeric` misses).
#[inline]
pub fn is_number(c: char) -> bool {
    swift_text::scalar_is_numeric(c)
}

/// Swift `(x).rounded()` on a floating-point value: schoolbook rounding.
#[inline(always)]
pub fn rounded(x: f64) -> f64 {
    x.round()
}

/// `Int(x)`: truncation toward zero; Swift traps on NaN and out-of-range.
#[inline(always)]
pub fn int_truncating(x: f64) -> i64 {
    assert!(
        x.is_finite() && x > -9.223372036854777e18 && x < 9.223372036854776e18,
        "Int({x}) traps"
    );
    x as i64
}

pub mod json {
    //! A JSON reader with the acceptance rules of Swift's `JSONDecoder` on
    //! macOS 26 (swift-foundation), as far as the theme decoders can observe:
    //!
    //! - a UTF-8 byte-order mark is skipped;
    //! - space, tab, LF and CR are whitespace, nothing else;
    //! - one trailing comma before `}` or `]` is accepted;
    //! - a duplicated object key keeps its *first* value;
    //! - strings reject raw control characters, unknown escapes and unpaired
    //!   surrogate escapes;
    //! - numbers follow the JSON grammar, are parsed correctly rounded, and are
    //!   rejected when they overflow to infinity or underflow to zero while
    //!   carrying a non-zero digit.

    #[derive(Debug, Clone, PartialEq)]
    pub enum Value {
        Null,
        Bool(bool),
        /// The literal text of the number, validated; converted on demand.
        Number(String),
        String(String),
        Array(Vec<Value>),
        /// Keys in document order, first occurrence wins.
        Object(Vec<(String, Value)>),
    }

    impl Value {
        pub fn get(&self, key: &str) -> Option<&Value> {
            match self {
                Value::Object(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
                _ => None,
            }
        }

        /// `decode(Double.self)`.
        pub fn as_f64(&self) -> Result<f64, String> {
            match self {
                Value::Number(text) => number_to_f64(text),
                other => Err(format!("expected a number, found {}", other.kind())),
            }
        }

        pub fn as_str(&self) -> Option<&str> {
            match self {
                Value::String(s) => Some(s),
                _ => None,
            }
        }

        pub fn kind(&self) -> &'static str {
            match self {
                Value::Null => "null",
                Value::Bool(_) => "bool",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
            }
        }
    }

    fn number_to_f64(text: &str) -> Result<f64, String> {
        let value: f64 = text.parse().map_err(|_| format!("bad number {text}"))?;
        if !value.is_finite() {
            return Err(format!("number {text} is not representable"));
        }
        if value == 0.0 {
            // Zero is only accepted when the significand has no non-zero digit.
            let significand = text.split(['e', 'E']).next().unwrap_or("");
            if significand.bytes().any(|b| (b'1'..=b'9').contains(&b)) {
                return Err(format!("number {text} is not representable"));
            }
        }
        Ok(value)
    }

    pub fn parse(bytes: &[u8]) -> Result<Value, String> {
        let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
        let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
        let mut parser = Parser {
            bytes: text.as_bytes(),
            text,
            i: 0,
        };
        parser.skip_whitespace();
        let value = parser.value(0)?;
        parser.skip_whitespace();
        if parser.i != parser.bytes.len() {
            return Err(format!("unexpected trailing data at byte {}", parser.i));
        }
        Ok(value)
    }

    struct Parser<'a> {
        bytes: &'a [u8],
        text: &'a str,
        i: usize,
    }

    const MAX_DEPTH: usize = 512;

    impl Parser<'_> {
        fn skip_whitespace(&mut self) {
            while self.i < self.bytes.len()
                && matches!(self.bytes[self.i], b' ' | b'\t' | b'\n' | b'\r')
            {
                self.i += 1;
            }
        }

        fn error<T>(&self, what: &str) -> Result<T, String> {
            Err(format!("{what} at byte {}", self.i))
        }

        fn value(&mut self, depth: usize) -> Result<Value, String> {
            if depth > MAX_DEPTH {
                return self.error("too deeply nested");
            }
            match self.bytes.get(self.i) {
                None => self.error("unexpected end"),
                Some(b'{') => self.object(depth),
                Some(b'[') => self.array(depth),
                Some(b'"') => Ok(Value::String(self.string()?)),
                Some(b't') => self.literal("true", Value::Bool(true)),
                Some(b'f') => self.literal("false", Value::Bool(false)),
                Some(b'n') => self.literal("null", Value::Null),
                Some(b'-' | b'0'..=b'9') => self.number(),
                Some(_) => self.error("unexpected character"),
            }
        }

        fn literal(&mut self, word: &str, value: Value) -> Result<Value, String> {
            if self.bytes[self.i..].starts_with(word.as_bytes()) {
                self.i += word.len();
                Ok(value)
            } else {
                self.error("bad literal")
            }
        }

        fn object(&mut self, depth: usize) -> Result<Value, String> {
            self.i += 1;
            let mut pairs: Vec<(String, Value)> = Vec::new();
            self.skip_whitespace();
            if self.bytes.get(self.i) == Some(&b'}') {
                self.i += 1;
                return Ok(Value::Object(pairs));
            }
            loop {
                self.skip_whitespace();
                if self.bytes.get(self.i) != Some(&b'"') {
                    return self.error("expected a key");
                }
                let key = self.string()?;
                self.skip_whitespace();
                if self.bytes.get(self.i) != Some(&b':') {
                    return self.error("expected ':'");
                }
                self.i += 1;
                self.skip_whitespace();
                let value = self.value(depth + 1)?;
                if !pairs.iter().any(|(existing, _)| *existing == key) {
                    pairs.push((key, value));
                }
                self.skip_whitespace();
                match self.bytes.get(self.i) {
                    Some(b',') => {
                        self.i += 1;
                        self.skip_whitespace();
                        if self.bytes.get(self.i) == Some(&b'}') {
                            self.i += 1;
                            return Ok(Value::Object(pairs));
                        }
                    }
                    Some(b'}') => {
                        self.i += 1;
                        return Ok(Value::Object(pairs));
                    }
                    _ => return self.error("expected ',' or '}'"),
                }
            }
        }

        fn array(&mut self, depth: usize) -> Result<Value, String> {
            self.i += 1;
            let mut values = Vec::new();
            self.skip_whitespace();
            if self.bytes.get(self.i) == Some(&b']') {
                self.i += 1;
                return Ok(Value::Array(values));
            }
            loop {
                self.skip_whitespace();
                values.push(self.value(depth + 1)?);
                self.skip_whitespace();
                match self.bytes.get(self.i) {
                    Some(b',') => {
                        self.i += 1;
                        self.skip_whitespace();
                        if self.bytes.get(self.i) == Some(&b']') {
                            self.i += 1;
                            return Ok(Value::Array(values));
                        }
                    }
                    Some(b']') => {
                        self.i += 1;
                        return Ok(Value::Array(values));
                    }
                    _ => return self.error("expected ',' or ']'"),
                }
            }
        }

        fn hex4(&mut self) -> Result<u32, String> {
            if self.i + 4 > self.bytes.len() {
                return self.error("short \\u escape");
            }
            let digits = &self.text[self.i..self.i + 4];
            let value = u32::from_str_radix(digits, 16)
                .map_err(|_| format!("bad \\u escape at byte {}", self.i))?;
            if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
                return self.error("bad \\u escape");
            }
            self.i += 4;
            Ok(value)
        }

        fn string(&mut self) -> Result<String, String> {
            self.i += 1;
            let mut out = String::new();
            loop {
                let start = self.i;
                while self.i < self.bytes.len()
                    && !matches!(self.bytes[self.i], b'"' | b'\\' | 0x00..=0x1F)
                {
                    self.i += 1;
                }
                out.push_str(&self.text[start..self.i]);
                match self.bytes.get(self.i) {
                    None => return self.error("unterminated string"),
                    Some(b'"') => {
                        self.i += 1;
                        return Ok(out);
                    }
                    Some(b'\\') => {
                        self.i += 1;
                        let Some(&escape) = self.bytes.get(self.i) else {
                            return self.error("unterminated escape");
                        };
                        self.i += 1;
                        match escape {
                            b'"' => out.push('"'),
                            b'\\' => out.push('\\'),
                            b'/' => out.push('/'),
                            b'b' => out.push('\u{8}'),
                            b'f' => out.push('\u{c}'),
                            b'n' => out.push('\n'),
                            b'r' => out.push('\r'),
                            b't' => out.push('\t'),
                            b'u' => {
                                let first = self.hex4()?;
                                let scalar = if (0xD800..0xDC00).contains(&first) {
                                    if !self.bytes[self.i..].starts_with(b"\\u") {
                                        return self.error("unpaired surrogate");
                                    }
                                    self.i += 2;
                                    let second = self.hex4()?;
                                    if !(0xDC00..0xE000).contains(&second) {
                                        return self.error("unpaired surrogate");
                                    }
                                    0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                                } else if (0xDC00..0xE000).contains(&first) {
                                    return self.error("unpaired surrogate");
                                } else {
                                    first
                                };
                                out.push(char::from_u32(scalar).ok_or("bad scalar")?);
                            }
                            _ => return self.error("unknown escape"),
                        }
                    }
                    Some(_) => return self.error("control character in string"),
                }
            }
        }

        fn number(&mut self) -> Result<Value, String> {
            let start = self.i;
            if self.bytes.get(self.i) == Some(&b'-') {
                self.i += 1;
            }
            let digits = |parser: &mut Self| {
                let from = parser.i;
                while parser.i < parser.bytes.len() && parser.bytes[parser.i].is_ascii_digit() {
                    parser.i += 1;
                }
                parser.i - from
            };
            match self.bytes.get(self.i) {
                Some(b'0') => {
                    self.i += 1;
                    if self.bytes.get(self.i).is_some_and(u8::is_ascii_digit) {
                        return self.error("leading zero");
                    }
                }
                Some(b'1'..=b'9') => {
                    digits(self);
                }
                _ => return self.error("bad number"),
            }
            if self.bytes.get(self.i) == Some(&b'.') {
                self.i += 1;
                if digits(self) == 0 {
                    return self.error("bad fraction");
                }
            }
            if matches!(self.bytes.get(self.i), Some(b'e' | b'E')) {
                self.i += 1;
                if matches!(self.bytes.get(self.i), Some(b'+' | b'-')) {
                    self.i += 1;
                }
                if digits(self) == 0 {
                    return self.error("bad exponent");
                }
            }
            let text = &self.text[start..self.i];
            // Representability is checked when the value is decoded
            // (`as_f64`), as swift-foundation converts numbers lazily.
            Ok(Value::Number(text.to_owned()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_sets_match_the_probe() {
        assert!(is_whitespace('\u{200B}'));
        assert!(!is_whitespace('\n'));
        assert!(is_whitespace_or_newline('\u{85}'));
        assert_eq!(trim_whitespaces("\u{200B} a \u{200B}"), "a");
    }

    #[test]
    fn hex_parsing_follows_fixed_width_integer_init() {
        assert_eq!(parse_u64_hex("+12345"), Some(0x12345));
        assert_eq!(parse_u64_hex("-00000"), Some(0));
        assert_eq!(parse_u64_hex("-1"), None);
        assert_eq!(parse_u64_hex("0x12"), None);
        assert_eq!(parse_u64_hex(" 12"), None);
        assert_eq!(parse_u64_hex("+"), None);
    }

    #[test]
    fn lowercasing_has_no_final_sigma() {
        assert_eq!(lowercased("M\u{212A}D"), "mkd");
        assert_eq!(lowercased("ΣΑΣ"), "σασ");
    }

    #[test]
    fn json_matches_the_decoder_probe() {
        use json::{Value, parse};
        let object = parse(br#"{"a": 1, "a": 2}"#).unwrap();
        assert_eq!(object.get("a").unwrap().as_f64().unwrap(), 1.0);
        assert!(parse(br#"{"a":1,}"#).is_ok());
        assert!(parse(br#"["x",]"#).is_ok());
        assert!(parse(br#"[,]"#).is_err());
        assert!(parse(b"\xEF\xBB\xBF{}").is_ok());
        assert!(parse(br#"{"s": "\ud800"}"#).is_err());
        assert!(parse(b"{\"s\": \"a\tb\"}").is_err());
        assert!(parse(br#"{"a": 01}"#).is_err());
        let number = |text: &str| Value::Number(text.into()).as_f64();
        assert!(number("1e999").is_err());
        assert!(number("1e-400").is_err());
        assert_eq!(number("-0").unwrap().to_bits(), (-0.0f64).to_bits());
        assert_eq!(number("5e-324").unwrap(), 5e-324);
        assert!(number("2e-324").is_err());
    }
}
