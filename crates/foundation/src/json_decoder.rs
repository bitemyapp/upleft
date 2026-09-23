//! The input side of Swift's `JSONDecoder` (swift-foundation, macOS 26): the
//! scanner that turns bytes into a value tree, and the error each typed
//! `decode` raises when the tree does not have the expected shape.
//!
//! A `Decodable` port reads the tree the way its `init(from:)` does, with the
//! helpers at the bottom, so that the first error it meets is the one Swift
//! would throw, with the same `NSError` code.
//!
//! Recorded from Swift 6.4 on macOS 26 (probes quoted in the tests):
//!
//! * **Encoding.** A UTF-8 byte-order mark is skipped. `FF FE` means UTF-16LE
//!   and `FE FF` UTF-16BE (their marks are skipped), `00 00 FE FF` UTF-32BE;
//!   `FF FE 00 00` is read as UTF-16LE, so a UTF-32LE mark is not recognised.
//!   Without a mark, zero bytes in the first four decide: `00 00 00 xx`
//!   UTF-32BE, `xx 00 00 00` UTF-32LE, `00 xx 00 xx` UTF-16BE, `xx 00 xx 00`
//!   UTF-16LE (with two or three bytes, `00 xx` and `xx 00`). Trailing bytes
//!   that do not fill a code unit are ignored; an unpaired surrogate is an
//!   error.
//! * **Syntax.** Whitespace is space, tab, LF and CR only. One trailing comma
//!   before `}` or `]` is accepted. Containers nest at most 512 deep.
//! * **Lazy values.** A number is only *scanned* (a `-` or digit, then any
//!   run of digits, `.`, `e`, `E`, `+`, `-`): `01`, `1.` and `--1` pass as
//!   long as nothing decodes them. A string value is scanned for its closing
//!   quote only (a backslash skips one byte, `\u` skips four more), so raw
//!   control characters, unknown escapes, unpaired surrogate escapes and
//!   invalid UTF-8 are errors only when the string is decoded. Object keys
//!   are decoded, and so validated, while scanning.
//! * **Duplicate keys.** The first occurrence wins.
//! * **Errors.** Every syntax error is `dataCorrupted` (4864). A missing key
//!   is `keyNotFound` (4865); `null` where a `String`, an array, a dictionary
//!   or a keyed type is expected is `valueNotFound` (4865), but `null` for a
//!   `Bool` is `typeMismatch` (4864); every other wrong type is 4864.
//! * **Dictionaries.** Swift decodes a `[String: T]`'s entries in hash order,
//!   which changes from process to process, so when two entries would throw
//!   different errors Swift reports either. [`entries`] walks document order.

/// A scanned JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    /// The number's text as scanned; not validated until something decodes it.
    Number(String),
    /// The bytes between the quotes, escapes unresolved; validated and
    /// unescaped by [`decode_string`].
    String(Vec<u8>),
    Array(Vec<Value>),
    /// Members in document order, first occurrence of each key only.
    Object(Vec<(String, Value)>),
}

/// A `DecodingError`, reduced to what can be observed through `NSError`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodingError {
    /// Not valid JSON, or a custom `dataCorrupted` from an `init(from:)`.
    DataCorrupted(String),
    TypeMismatch(String),
    ValueNotFound(String),
    KeyNotFound(String),
}

impl DecodingError {
    /// `(error as NSError).code`: `NSCoderReadCorruptError` or
    /// `NSCoderValueNotFoundError`.
    pub fn code(&self) -> isize {
        match self {
            DecodingError::DataCorrupted(_) | DecodingError::TypeMismatch(_) => 4864,
            DecodingError::ValueNotFound(_) | DecodingError::KeyNotFound(_) => 4865,
        }
    }

    /// `error.localizedDescription`.
    pub fn localized_description(&self) -> &'static str {
        match self.code() {
            4865 => "The data couldn\u{2019}t be read because it is missing.",
            _ => "The data couldn\u{2019}t be read because it isn\u{2019}t in the correct format.",
        }
    }

    /// The debug text; never shown by Downright, kept for diagnostics.
    pub fn debug_description(&self) -> &str {
        match self {
            DecodingError::DataCorrupted(text)
            | DecodingError::TypeMismatch(text)
            | DecodingError::ValueNotFound(text)
            | DecodingError::KeyNotFound(text) => text,
        }
    }
}

impl std::fmt::Display for DecodingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.localized_description(), self.debug_description())
    }
}

impl std::error::Error for DecodingError {}

/// The scanner's view of `JSONDecoder().decode(_:from:)`'s input.
pub fn parse(bytes: &[u8]) -> Result<Value, DecodingError> {
    let text = transcode(bytes).ok_or_else(|| corrupt("The given data was not valid JSON."))?;
    let mut scanner = Scanner { bytes: &text, i: 0 };
    scanner.skip_whitespace();
    let value = scanner.value(0)?;
    scanner.skip_whitespace();
    if scanner.i != scanner.bytes.len() {
        return scanner.error("unexpected trailing data");
    }
    Ok(value)
}

fn corrupt(text: &str) -> DecodingError {
    DecodingError::DataCorrupted(text.to_owned())
}

#[derive(Clone, Copy)]
enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    Utf32Le,
    Utf32Be,
}

fn detect_encoding(bytes: &[u8]) -> (Encoding, usize) {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return (Encoding::Utf8, 3);
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return (Encoding::Utf16Le, 2);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return (Encoding::Utf16Be, 2);
    }
    if bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        return (Encoding::Utf32Be, 4);
    }
    if bytes.len() >= 4 {
        match (bytes[0], bytes[1], bytes[2], bytes[3]) {
            (0, 0, 0, _) => return (Encoding::Utf32Be, 0),
            (_, 0, 0, 0) => return (Encoding::Utf32Le, 0),
            (0, _, 0, _) => return (Encoding::Utf16Be, 0),
            (_, 0, _, 0) => return (Encoding::Utf16Le, 0),
            _ => {}
        }
    } else if bytes.len() >= 2 {
        match (bytes[0], bytes[1]) {
            (0, _) => return (Encoding::Utf16Be, 0),
            (_, 0) => return (Encoding::Utf16Le, 0),
            _ => {}
        }
    }
    (Encoding::Utf8, 0)
}

/// The input as UTF-8 bytes, or `None` when it cannot be converted. UTF-8
/// input is not validated here: Swift only checks the strings it decodes.
fn transcode(bytes: &[u8]) -> Option<Vec<u8>> {
    let (encoding, mark) = detect_encoding(bytes);
    let body = &bytes[mark..];
    match encoding {
        Encoding::Utf8 => Some(body.to_vec()),
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let units: Vec<u16> = body
                .chunks_exact(2)
                .map(|pair| match encoding {
                    Encoding::Utf16Le => u16::from_le_bytes([pair[0], pair[1]]),
                    _ => u16::from_be_bytes([pair[0], pair[1]]),
                })
                .collect();
            String::from_utf16(&units).ok().map(String::into_bytes)
        }
        Encoding::Utf32Le | Encoding::Utf32Be => body
            .chunks_exact(4)
            .map(|quad| {
                let value = match encoding {
                    Encoding::Utf32Le => u32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]),
                    _ => u32::from_be_bytes([quad[0], quad[1], quad[2], quad[3]]),
                };
                char::from_u32(value)
            })
            .collect::<Option<String>>()
            .map(String::into_bytes),
    }
}

struct Scanner<'a> {
    bytes: &'a [u8],
    i: usize,
}

/// Containers may nest this deep, the top-level one included.
const MAX_DEPTH: usize = 512;

impl Scanner<'_> {
    fn skip_whitespace(&mut self) {
        while self.i < self.bytes.len() && matches!(self.bytes[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn error<T>(&self, what: &str) -> Result<T, DecodingError> {
        Err(corrupt(&format!("The given data was not valid JSON ({what} at byte {}).", self.i)))
    }

    fn value(&mut self, depth: usize) -> Result<Value, DecodingError> {
        match self.bytes.get(self.i) {
            None => self.error("unexpected end of file"),
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Value::String(self.raw_string()?)),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') => self.literal("null", Value::Null),
            Some(b'-' | b'0'..=b'9') => Ok(self.number()),
            Some(_) => self.error("unexpected character"),
        }
    }

    fn literal(&mut self, word: &str, value: Value) -> Result<Value, DecodingError> {
        if self.bytes[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(value)
        } else {
            self.error("unexpected character")
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, DecodingError> {
        if depth >= MAX_DEPTH {
            return self.error("too many nested arrays or dictionaries");
        }
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
            let raw = self.raw_string()?;
            let key = match unescape(&raw) {
                Ok(key) => key,
                Err(what) => return self.error(what),
            };
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

    fn array(&mut self, depth: usize) -> Result<Value, DecodingError> {
        if depth >= MAX_DEPTH {
            return self.error("too many nested arrays or dictionaries");
        }
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

    /// Finds the closing quote, as the scanner does: a backslash skips the
    /// next byte, and `\u` four more, without looking at them.
    fn raw_string(&mut self) -> Result<Vec<u8>, DecodingError> {
        self.i += 1;
        let start = self.i;
        loop {
            match self.bytes.get(self.i) {
                None => return self.error("unexpected end of file"),
                Some(b'"') => {
                    let raw = self.bytes[start..self.i].to_vec();
                    self.i += 1;
                    return Ok(raw);
                }
                Some(b'\\') => {
                    let skip = if self.bytes.get(self.i + 1) == Some(&b'u') { 6 } else { 2 };
                    self.i += skip;
                }
                Some(_) => self.i += 1,
            }
        }
    }

    /// Scans a number without validating it (see the module notes).
    fn number(&mut self) -> Value {
        let start = self.i;
        self.i += 1;
        while self.i < self.bytes.len() && matches!(self.bytes[self.i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-') {
            self.i += 1;
        }
        Value::Number(String::from_utf8_lossy(&self.bytes[start..self.i]).into_owned())
    }
}

/// A string's text from its raw bytes: valid UTF-8, no raw control
/// characters, known escapes, paired surrogates.
fn unescape(raw: &[u8]) -> Result<String, &'static str> {
    let text = std::str::from_utf8(raw).map_err(|_| "invalid UTF-8 in string")?;
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let hex4 = |at: usize| -> Result<u32, &'static str> {
        match bytes.get(at..at + 4) {
            Some(digits) if digits.iter().all(u8::is_ascii_hexdigit) => {
                Ok(u32::from_str_radix(&text[at..at + 4], 16).unwrap_or(0))
            }
            _ => Err("invalid unicode escape"),
        }
    };
    while i < bytes.len() {
        let start = i;
        while i < bytes.len() && !matches!(bytes[i], b'\\' | 0x00..=0x1F) {
            i += 1;
        }
        out.push_str(&text[start..i]);
        let Some(&byte) = bytes.get(i) else { break };
        if byte != b'\\' {
            return Err("unescaped control character");
        }
        let Some(&escape) = bytes.get(i + 1) else { return Err("invalid escape") };
        i += 2;
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
                let first = hex4(i)?;
                i += 4;
                let scalar = if (0xD800..0xDC00).contains(&first) {
                    if bytes.get(i..i + 2) != Some(b"\\u") {
                        return Err("unpaired surrogate");
                    }
                    let second = hex4(i + 2)?;
                    if !(0xDC00..0xE000).contains(&second) {
                        return Err("unpaired surrogate");
                    }
                    i += 6;
                    0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                } else if (0xDC00..0xE000).contains(&first) {
                    return Err("unpaired surrogate");
                } else {
                    first
                };
                out.push(char::from_u32(scalar).ok_or("invalid unicode scalar")?);
            }
            _ => return Err("invalid escape"),
        }
    }
    Ok(out)
}

// MARK: - Typed reads, as the synthesized `init(from:)` makes them

impl Value {
    fn kind(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "a boolean",
            Value::Number(_) => "a number",
            Value::String(_) => "a string",
            Value::Array(_) => "an array",
            Value::Object(_) => "a dictionary",
        }
    }
}

fn mismatch(expected: &str, found: &Value) -> DecodingError {
    DecodingError::TypeMismatch(format!("Expected to decode {expected} but found {} instead.", found.kind()))
}

/// `decode(Bool.self)`. `null` is a type mismatch here, not a missing value.
pub fn decode_bool(value: &Value) -> Result<bool, DecodingError> {
    match value {
        Value::Bool(flag) => Ok(*flag),
        other => Err(mismatch("Bool", other)),
    }
}

/// `decode(String.self)`: validates and unescapes the scanned bytes.
pub fn decode_string(value: &Value) -> Result<String, DecodingError> {
    match value {
        Value::String(raw) => unescape(raw).map_err(|what| corrupt(&format!("The given data was not valid JSON ({what})."))),
        Value::Null => Err(DecodingError::ValueNotFound("Expected String value but found null instead.".into())),
        other => Err(mismatch("String", other)),
    }
}

/// `container(keyedBy:)`, or a `[String: T]`'s container.
pub fn keyed(value: &Value) -> Result<&[(String, Value)], DecodingError> {
    match value {
        Value::Object(members) => Ok(members),
        Value::Null => Err(DecodingError::ValueNotFound("Cannot get keyed decoding container -- found null value instead".into())),
        other => Err(mismatch("Dictionary<String, Any>", other)),
    }
}

/// `unkeyedContainer()`, or an array's container.
pub fn unkeyed(value: &Value) -> Result<&[Value], DecodingError> {
    match value {
        Value::Array(values) => Ok(values),
        Value::Null => Err(DecodingError::ValueNotFound("Cannot get unkeyed decoding container -- found null value instead".into())),
        other => Err(mismatch("Array<Any>", other)),
    }
}

/// `container.decode(_:forKey:)`'s lookup: a missing key is `keyNotFound`.
pub fn member<'a>(members: &'a [(String, Value)], key: &str) -> Result<&'a Value, DecodingError> {
    members
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
        .ok_or_else(|| DecodingError::KeyNotFound(format!("No value associated with key \"{key}\".")))
}

/// `container.decodeIfPresent(_:forKey:)`'s lookup: absent and `null` are
/// both `nil`.
pub fn member_if_present<'a>(members: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    members.iter().find(|(name, _)| name == key).map(|(_, value)| value).filter(|value| **value != Value::Null)
}

/// A `[String: T]`'s entries, in document order (see the module notes).
pub fn entries(value: &Value) -> Result<&[(String, Value)], DecodingError> {
    keyed(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(bytes: &[u8]) -> bool {
        parse(bytes).is_ok()
    }

    /// `JSONDecoder().decode(Stored.self, …)` acceptance, from the probe.
    #[test]
    fn acceptance_matches_the_decoder_probe() {
        let stored = |extra: &str| format!(r#"{{"vimKeysEnabled":true,"overrides":{{}}{extra}}}"#).into_bytes();
        assert!(ok(&stored(",")));
        assert!(!ok(&stored(",,")));
        assert!(ok(&stored(r#","x":[1,]"#)));
        assert!(!ok(&stored(r#","x":[,]"#)));
        assert!(!ok(&stored(r#","x":[1,,]"#)));
        for lax in ["01", "1.", "-", "1e", "1.2.3", "--1", "1e5e5", "-01", "00", "1-2", "1ee", "1.e5", "0e", "1E+", "0.0.0", "1+1", "1e999"] {
            assert!(ok(&stored(&format!(r#","x":{lax}"#))), "{lax}");
        }
        for bad in [".5", "+1", "0x1", "Infinity", "12a", "1_0", "-a", "NaN", "True", "tru", "truex", "nullx"] {
            assert!(!ok(&stored(&format!(r#","x":{bad}"#))), "{bad}");
        }
        assert!(!ok(b""));
        assert!(!ok(b"  \n"));
        assert!(ok(b"\r{}"));
        assert!(!ok(b"\x0C{}"));
        assert!(!ok("\u{A0}{}".as_bytes()));
        // `\u` escapes, built at run time so the source stays ASCII.
        let u = |hex: &str| format!("{}u{hex}", '\\');
        let backslash = '\\';
        // Keys are validated while scanning, values only when decoded.
        assert!(!ok(format!(r#"{{"{}":1}}"#, u("d800")).as_bytes()));
        assert!(ok(format!(r#"{{"{}{}":1}}"#, u("d83d"), u("de00")).as_bytes()));
        assert!(!ok(b"{\"a\tb\":1}"));
        assert!(!ok(format!(r#"{{"{backslash}x":1}}"#).as_bytes()));
        assert!(!ok(b"{\"\xFF\":1}"));
        let lazy_values: Vec<Vec<u8>> = vec![
            format!(r#""{}""#, u("d800")).into_bytes(),
            format!(r#""{backslash}x""#).into_bytes(),
            b"\"a\tb\"".to_vec(),
            b"\"a\nb\"".to_vec(),
            b"\"\xFF\"".to_vec(),
            b"\"\xC3\"".to_vec(),
            format!(r#""{}""#, u("00zz")).into_bytes(),
            format!(r#""{backslash}U0041""#).into_bytes(),
        ];
        for lazy in lazy_values {
            let mut json = br#"{"v":true,"x":"#.to_vec();
            json.extend_from_slice(&lazy);
            json.push(b'}');
            let value = parse(&json).unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&lazy)));
            let Value::Object(members) = value else { unreachable!() };
            assert_eq!(decode_string(&members[1].1).unwrap_err().code(), 4864);
        }
        assert!(!ok(format!(r#"{{"v":true,"x":"{backslash}"}}"#).as_bytes()));
        assert!(!ok(format!(r#"{{"v":true,"x":"{}"}}"#, u("12")).as_bytes()));
        let pair = parse(format!(r#"["{}{}","{}"]"#, u("d83d"), u("de00"), u("0000")).as_bytes()).unwrap();
        let Value::Array(values) = pair else { unreachable!() };
        assert_eq!(decode_string(&values[0]).unwrap(), "\u{1F600}");
        assert_eq!(decode_string(&values[1]).unwrap(), "\u{0}");
        assert!(!ok(b"{} x"));
        assert!(!ok(b"{'a':1}"));
        assert!(!ok(b"{a:1}"));
        assert!(!ok(b"{\"a\":1, /* c */ \"b\":2}"));
    }

    #[test]
    fn nesting_limit_is_512_containers() {
        let nested = |n: usize| format!(r#"{{"x":{}{}}}"#, "[".repeat(n), "]".repeat(n)).into_bytes();
        assert!(ok(&nested(511)));
        assert!(!ok(&nested(512)));
    }

    #[test]
    fn encodings_match_the_probe() {
        assert!(!ok(&[0xEF, 0xBB, 0xBF]));
        assert!(ok(&[0xEF, 0xBB, 0xBF, b'{', b'}']));
        assert!(!ok(&[0xEF, 0xBB, 0xBF, 0xEF, 0xBB, 0xBF, b'{', b'}']));
        assert!(ok(&[0xFF, 0xFE, b'{', 0, b'}', 0]));
        assert!(ok(&[0xFE, 0xFF, 0, b'{', 0, b'}']));
        assert!(ok(&[b'{', 0, b'}', 0]));
        assert!(ok(&[0, b'{', 0, b'}']));
        assert!(!ok(&[0xFF, 0xFE, b'{']));
        assert!(ok(&[b'{', 0, b'}', 0, b' ']));
        assert!(ok(&[b'{', 0, b'}', 0, b'A']));
        assert!(!ok(&[0xFF, 0xFE, 0xFF, 0xFE, b'{', 0, b'}', 0]));
        assert!(!ok(&[0xFF, 0xFE, 0, 0, b'{', 0, 0, 0, b'}', 0, 0, 0]));
        assert!(ok(&[b'{', 0, 0, 0, b'}', 0, 0, 0, b'A']));
        assert!(ok(&[0, 0, 0xFE, 0xFF, 0, 0, 0, b'{', 0, 0, 0, b'}']));
        assert!(!ok(&[b'{', b'}', 0]));
        assert!(!ok(&[b'{', b'}', b' ', 0]));
        assert!(!ok(&[0, b'{']));
        // A lone surrogate in UTF-16 input.
        assert!(!ok(&[b'{', 0, b'"', 0, b'a', 0, b'"', 0, b':', 0, b'"', 0, 0x00, 0xD8, b'"', 0, b'}', 0]));
        let decoded = parse(&[b'{', 0, b'"', 0, b'a', 0, b'"', 0, b':', 0, b'"', 0, 0xE9, 0, b'"', 0, b'}', 0]).unwrap();
        assert_eq!(decoded, Value::Object(vec![("a".into(), Value::String("é".as_bytes().to_vec()))]));
    }

    #[test]
    fn first_duplicate_key_wins() {
        let value = parse(br#"{"a":1,"a":2}"#).unwrap();
        assert_eq!(value, Value::Object(vec![("a".into(), Value::Number("1".into()))]));
    }

    /// Error codes from the matrix probe (Bool/String/Dictionary/Array/keyed
    /// against every JSON kind).
    #[test]
    fn typed_reads_throw_the_probed_codes() {
        let null = Value::Null;
        let number = Value::Number("1".into());
        assert_eq!(decode_bool(&null).unwrap_err().code(), 4864);
        assert_eq!(decode_string(&null).unwrap_err().code(), 4865);
        assert_eq!(decode_string(&number).unwrap_err().code(), 4864);
        assert_eq!(keyed(&null).unwrap_err().code(), 4865);
        assert_eq!(keyed(&number).unwrap_err().code(), 4864);
        assert_eq!(unkeyed(&null).unwrap_err().code(), 4865);
        assert_eq!(unkeyed(&Value::Object(vec![])).unwrap_err().code(), 4864);
        assert_eq!(member(&[], "k").unwrap_err().code(), 4865);
        assert_eq!(
            DecodingError::KeyNotFound(String::new()).localized_description(),
            "The data couldn\u{2019}t be read because it is missing."
        );
    }
}
