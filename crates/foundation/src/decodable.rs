//! What Swift's `JSONDecoder` does with a scanned value ([`json_decoder`]):
//! the keyed-container rules synthesized `init(from:)` follows, the number
//! conversions, the `.iso8601` date strategy and `UUID`. Recorded from Swift
//! 6.4 on macOS 26 by the probes quoted in the tests.
//!
//! * `Int` from a literal with no fraction or exponent is parsed exactly and
//!   fails on overflow. Any other literal is read as a `Double`, which must be
//!   integral and strictly inside `(Double(Int.min), Double(Int.max))`:
//!   `1.0`, `1e2`, `100e-2` and even `1e-400` decode (as 1, 100, 1 and 0);
//!   `1.5`, `1e19` and `-9223372036854775808.0` do not. The scanner is lax
//!   ([`json_decoder`]), so the literal's grammar is checked here.
//! * `Double` rejects a literal that overflows to infinity, and one that
//!   parses as zero unless swift-foundation's `isTrueZero` scan accepts it
//!   (which, through its loop unrolling, rejects `0e1` and `0.00e-5` but
//!   accepts `0e+1` and `0.0e1`; see [`is_true_zero`]).
//! * `null` is never a value of a non-optional type; `decodeIfPresent` reads
//!   it as absent.
//! * `.iso8601` dates follow swift-foundation's lenient ISO 8601 parser
//!   ([`parse_iso8601`]); `UUID` follows `UUID(uuidString:)` ([`parse_uuid`]).

use crate::date::Date;
pub use crate::json_decoder::{DecodingError, Value, parse};
use crate::json_decoder::{self};

fn corrupt<T>(message: impl Into<String>) -> Result<T, DecodingError> {
    Err(DecodingError::DataCorrupted(message.into()))
}

fn null_found<T>(expected: &str) -> Result<T, DecodingError> {
    Err(DecodingError::ValueNotFound(format!("Expected {expected} value but found null instead.")))
}

fn mismatch<T>(expected: &str, found: &Value) -> Result<T, DecodingError> {
    let kind = match found {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "a dictionary",
    };
    Err(DecodingError::TypeMismatch(format!("Expected to decode {expected} but found {kind} instead.")))
}

/// Typed reads over a scanned [`Value`], as a synthesized `init(from:)`
/// makes them.
pub trait DecodableValue {
    /// `decoder.container(keyedBy:)`.
    fn keyed_container(&self) -> Result<Keyed<'_>, DecodingError>;
    /// `decoder.unkeyedContainer()` / `[T]`.
    fn array_value(&self) -> Result<&[Value], DecodingError>;
    /// `decode(Int.self)`.
    fn int_value(&self) -> Result<i64, DecodingError>;
    /// `decode(Double.self)` (and `CGFloat`).
    fn double_value(&self) -> Result<f64, DecodingError>;
    /// `decode(Bool.self)`.
    fn bool_value(&self) -> Result<bool, DecodingError>;
    /// `decode(String.self)`.
    fn string_value(&self) -> Result<String, DecodingError>;
    /// `decode(Date.self)` under `.dateDecodingStrategy = .iso8601`.
    fn date_iso8601(&self) -> Result<Date, DecodingError>;
    /// `decode(UUID.self)`: the 16 bytes.
    fn uuid_value(&self) -> Result<[u8; 16], DecodingError>;
    /// A raw-value enum: `T(rawValue:)` over a decoded `String`.
    fn raw_string_enum<T>(&self, from_raw: impl Fn(&str) -> Option<T>) -> Result<T, DecodingError>;
    /// A raw-value enum: `T(rawValue:)` over a decoded `Int`.
    fn raw_int_enum<T>(&self, from_raw: impl Fn(i64) -> Option<T>) -> Result<T, DecodingError>;
    /// `[T]`, element by element.
    fn array_of<T>(&self, element: impl Fn(&Value) -> Result<T, DecodingError>) -> Result<Vec<T>, DecodingError>;
}

impl DecodableValue for Value {
    fn keyed_container(&self) -> Result<Keyed<'_>, DecodingError> {
        json_decoder::keyed(self).map(|members| Keyed { members })
    }

    fn array_value(&self) -> Result<&[Value], DecodingError> {
        json_decoder::unkeyed(self)
    }

    fn int_value(&self) -> Result<i64, DecodingError> {
        match self {
            Value::Number(text) => number_to_int(text),
            Value::Null => null_found("Int"),
            other => mismatch("Int", other),
        }
    }

    fn double_value(&self) -> Result<f64, DecodingError> {
        match self {
            Value::Number(text) => number_to_double(text),
            Value::Null => null_found("Double"),
            other => mismatch("Double", other),
        }
    }

    fn bool_value(&self) -> Result<bool, DecodingError> {
        json_decoder::decode_bool(self)
    }

    fn string_value(&self) -> Result<String, DecodingError> {
        json_decoder::decode_string(self)
    }

    fn date_iso8601(&self) -> Result<Date, DecodingError> {
        let text = self.string_value()?;
        parse_iso8601(&text)
            .ok_or_else(|| DecodingError::DataCorrupted("Expected date string to be ISO8601-formatted.".to_owned()))
    }

    fn uuid_value(&self) -> Result<[u8; 16], DecodingError> {
        let text = self.string_value()?;
        parse_uuid(&text).ok_or_else(|| {
            DecodingError::DataCorrupted(format!("Attempted to decode UUID from invalid UUID string: {text}"))
        })
    }

    fn raw_string_enum<T>(&self, from_raw: impl Fn(&str) -> Option<T>) -> Result<T, DecodingError> {
        let raw = self.string_value()?;
        from_raw(&raw).map_or_else(|| corrupt(format!("Cannot initialize from invalid String value {raw}")), Ok)
    }

    fn raw_int_enum<T>(&self, from_raw: impl Fn(i64) -> Option<T>) -> Result<T, DecodingError> {
        let raw = self.int_value()?;
        from_raw(raw).map_or_else(|| corrupt(format!("Cannot initialize from invalid Int value {raw}")), Ok)
    }

    fn array_of<T>(&self, element: impl Fn(&Value) -> Result<T, DecodingError>) -> Result<Vec<T>, DecodingError> {
        self.array_value()?.iter().map(element).collect()
    }
}

/// A keyed decoding container over an object's members.
#[derive(Clone, Copy, Debug)]
pub struct Keyed<'a> {
    members: &'a [(String, Value)],
}

impl<'a> Keyed<'a> {
    /// The member's value, if the key is present (a `null` included).
    pub fn value(&self, key: &str) -> Option<&'a Value> {
        self.members.iter().find(|(existing, _)| existing == key).map(|(_, value)| value)
    }

    /// `decode(_:forKey:)`: a missing key throws `keyNotFound`; a `null`
    /// throws as the type's own decode does.
    pub fn decode<T>(&self, key: &str, decode: impl Fn(&'a Value) -> Result<T, DecodingError>) -> Result<T, DecodingError> {
        decode(json_decoder::member(self.members, key)?)
    }

    /// `decodeIfPresent(_:forKey:)`: a missing key or a `null` is `nil`; any
    /// other value must decode.
    pub fn decode_if_present<T>(
        &self,
        key: &str,
        decode: impl Fn(&'a Value) -> Result<T, DecodingError>,
    ) -> Result<Option<T>, DecodingError> {
        json_decoder::member_if_present(self.members, key).map(decode).transpose()
    }
}

// MARK: - Numbers

/// The JSON number grammar, which the lazy scanner leaves to the decode.
fn is_valid_number(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    if bytes.get(i) == Some(&b'-') {
        i += 1;
    }
    match bytes.get(i) {
        Some(b'0') => i += 1,
        Some(b'1'..=b'9') => {
            while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
        }
        _ => return false,
    }
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    if matches!(bytes.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let start = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    i == bytes.len()
}

fn is_plain_integer(text: &str) -> bool {
    !text.bytes().any(|byte| matches!(byte, b'.' | b'e' | b'E'))
}

fn number_to_int(text: &str) -> Result<i64, DecodingError> {
    if !is_valid_number(text) {
        return corrupt(format!("Invalid number {text}"));
    }
    if is_plain_integer(text) {
        return text.parse::<i64>().or_else(|_| corrupt(format!("Parsed JSON number <{text}> does not fit in Int.")));
    }
    // swift-foundation's slow path: the `Double` when its magnitude is below
    // 2^53 (it must then be integral), otherwise the literal read as a
    // `Decimal`, whose conversion keeps the integer part (probed:
    // `9007199254740992.5` decodes as 9007199254740992) and fails when the
    // value as a `Double` reaches ±2^63.
    let value: f64 = text.parse().or_else(|_| corrupt(format!("Invalid number {text}")))?;
    let not_representable = || corrupt(format!("Parsed JSON number <{text}> does not fit in Int."));
    if !value.is_finite() {
        return not_representable();
    }
    if value.abs() < 9_007_199_254_740_992.0 {
        return if value.trunc() == value { Ok(value as i64) } else { not_representable() };
    }
    let bound = 9_223_372_036_854_775_808.0;
    if value <= -bound || value >= bound {
        return not_representable();
    }
    integer_part(text).map_or_else(not_representable, Ok)
}

/// The integer part of a valid JSON number literal, computed exactly.
fn integer_part(text: &str) -> Option<i64> {
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(index) => (&unsigned[..index], unsigned[index + 1..].parse::<i64>().ok()?),
        None => (unsigned, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{whole}{fraction}");
    let shift = exponent - fraction.len() as i64;
    let integer_digits: String = if shift >= 0 {
        format!("{digits}{}", "0".repeat(shift.min(40) as usize))
    } else {
        let keep = digits.len() as i64 + shift;
        if keep <= 0 { "0".to_owned() } else { digits[..keep as usize].to_owned() }
    };
    let magnitude: i128 = integer_digits.trim_start_matches('0').parse::<i128>().or_else(|_| {
        if integer_digits.trim_start_matches('0').is_empty() { Ok(0) } else { Err(()) }
    }).ok()?;
    i64::try_from(if negative { -magnitude } else { magnitude }).ok()
}

/// swift-foundation's `isTrueZero`: does a literal that parsed as zero really
/// spell zero? It scans four bytes at a time, stopping at the first non-zero
/// digit (not zero) or `e` (zero), but checks the last partial block of one to
/// three bytes from its end: `0e1` reads its `1` before its `e` and is
/// rejected, `0e+1` is accepted.
fn is_true_zero(text: &str) -> bool {
    let bytes = text.as_bytes();
    let check = |byte: u8| -> Option<bool> {
        match byte {
            b'1'..=b'9' => Some(false),
            b'e' | b'E' => Some(true),
            _ => None,
        }
    };
    let full = bytes.len() / 4 * 4;
    for &byte in &bytes[..full] {
        if let Some(result) = check(byte) {
            return result;
        }
    }
    for &byte in bytes[full..].iter().rev() {
        if let Some(result) = check(byte) {
            return result;
        }
    }
    true
}

fn number_to_double(text: &str) -> Result<f64, DecodingError> {
    if !is_valid_number(text) {
        return corrupt(format!("Invalid number {text}"));
    }
    let value: f64 = text.parse().or_else(|_| corrupt(format!("Invalid number {text}")))?;
    if !value.is_finite() {
        return corrupt(format!("Number {text} is not representable in Swift."));
    }
    if value == 0.0 && !is_true_zero(text) {
        return corrupt(format!("Number {text} is not representable in Swift."));
    }
    Ok(value)
}

// MARK: - UUID

/// `UUID(uuidString:)`: exactly `8-4-4-4-12` hexadecimal digits, either case.
pub fn parse_uuid(text: &str) -> Option<[u8; 16]> {
    let bytes = text.as_bytes();
    if bytes.len() != 36 {
        return None;
    }
    let mut out = [0u8; 16];
    let mut nibbles = 0usize;
    for (index, &byte) in bytes.iter().enumerate() {
        if matches!(index, 8 | 13 | 18 | 23) {
            if byte != b'-' {
                return None;
            }
            continue;
        }
        let value = (byte as char).to_digit(16)? as u8;
        out[nibbles / 2] |= if nibbles % 2 == 0 { value << 4 } else { value };
        nibbles += 1;
    }
    Some(out)
}

/// Swift's `UUID.uuidString` (and its `Codable` form): upper-case, hyphenated.
pub fn uuid_string(bytes: &[u8; 16]) -> String {
    let mut out = String::with_capacity(36);
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        out.push_str(&format!("{byte:02X}"));
    }
    out
}

// MARK: - ISO 8601

/// The largest year swift-foundation's `.iso8601` strategy accepts (probed:
/// `506714-12-31T23:59:59Z` decodes, `506715-01-01T00:00:00Z` does not).
const MAXIMUM_YEAR: i64 = 506_714;

struct Scanner<'a> {
    bytes: &'a [u8],
    i: usize,
}

impl Scanner<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.i).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.i += 1;
        Some(byte)
    }

    fn expect(&mut self, byte: u8) -> Option<()> {
        (self.next()? == byte).then_some(())
    }

    /// At least one ASCII digit, at most `max` when given.
    fn digits(&mut self, max: Option<usize>) -> Option<(i64, usize)> {
        let start = self.i;
        let mut value: i64 = 0;
        while let Some(byte) = self.peek() {
            if !byte.is_ascii_digit() || max.is_some_and(|max| self.i - start >= max) {
                break;
            }
            value = value.checked_mul(10)?.checked_add((byte - b'0') as i64)?;
            self.i += 1;
        }
        let count = self.i - start;
        (count > 0).then_some((value, count))
    }
}

/// `JSONDecoder.DateDecodingStrategy.iso8601` on macOS 26, which is
/// swift-foundation's `Date.ISO8601FormatStyle` parser in its lenient form.
/// Probed rules:
///
/// * `year-month-dayThour:minute:second`, each field one or more digits with
///   no digit limit (`2024-1-01`, `0:0:0` and `100:00:00` parse). Month must
///   be 1…12 and day 1…31; hours, minutes and seconds are unchecked and roll
///   over (`24:30:00`, `00:99:00`, `00:00:60` is a minute later). The `T` is
///   case-sensitive.
/// * optional `.` and one to nine fraction digits;
/// * a time zone: `Z`/`z`; `GMT` or `UTC` (any case), optionally followed by
///   an offset; or `±hh[[:]mm[[:]ss]]` with one or two digits per field. The
///   offset may not exceed 18 hours. Anything after the time zone is ignored;
///   anything before the year is not.
/// * The fields are resolved by the ISO 8601 calendar, which is Julian before
///   1582-10-15, leniently (`2023-02-29` is 1 March).
pub fn parse_iso8601(text: &str) -> Option<Date> {
    let mut scanner = Scanner { bytes: text.as_bytes(), i: 0 };
    let (year, _) = scanner.digits(None)?;
    if year > MAXIMUM_YEAR {
        return None;
    }
    scanner.expect(b'-')?;
    let (month, _) = scanner.digits(None)?;
    if !(1..=12).contains(&month) {
        return None;
    }
    scanner.expect(b'-')?;
    let (day, _) = scanner.digits(None)?;
    if !(1..=31).contains(&day) {
        return None;
    }
    scanner.expect(b'T')?;
    let (hour, _) = scanner.digits(None)?;
    scanner.expect(b':')?;
    let (minute, _) = scanner.digits(None)?;
    scanner.expect(b':')?;
    let (second, _) = scanner.digits(None)?;
    let mut nanosecond = 0i64;
    if scanner.peek() == Some(b'.') {
        scanner.next();
        let (value, count) = scanner.digits(Some(9))?;
        nanosecond = value * 10i64.pow(9 - count as u32);
    }
    let offset = time_zone_offset(&mut scanner)?;
    let days = iso_calendar_days(year, month, day);
    let seconds = days
        .checked_mul(86_400)?
        .checked_add(hour.checked_mul(3600)?)?
        .checked_add(minute.checked_mul(60)?)?
        .checked_add(second)?
        .checked_sub(offset)?;
    Some(Date::from_1970(seconds as f64 + nanosecond as f64 / 1_000_000_000.0))
}

/// Seconds east of GMT, or `None` where Swift fails.
fn time_zone_offset(scanner: &mut Scanner<'_>) -> Option<i64> {
    let first = scanner.next()?;
    let positive;
    match first {
        b'Z' | b'z' => return Some(0),
        b'G' | b'g' | b'U' | b'u' => {
            let expected: [&[u8]; 2] = if matches!(first, b'G' | b'g') { [b"Mm", b"Tt"] } else { [b"Tt", b"Cc"] };
            for letters in expected {
                let byte = scanner.next()?;
                if !letters.contains(&byte) {
                    return None;
                }
            }
            match scanner.peek() {
                Some(b'+') => positive = true,
                Some(b'-') => positive = false,
                _ => return Some(0),
            }
            scanner.next();
        }
        b'+' => positive = true,
        b'-' => positive = false,
        _ => return None,
    }
    let (hours, _) = scanner.digits(Some(2))?;
    let expect_minutes = match scanner.peek() {
        Some(b':') => {
            scanner.next();
            true
        }
        Some(byte) if byte.is_ascii_digit() => true,
        _ => false,
    };
    let magnitude = if expect_minutes {
        let (minutes, _) = scanner.digits(Some(2))?;
        if scanner.peek() == Some(b':') {
            scanner.next();
        }
        if scanner.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            let (seconds, _) = scanner.digits(Some(2))?;
            hours * 3600 + minutes * 60 + seconds
        } else {
            hours * 3600 + minutes * 60
        }
    } else {
        hours * 3600
    };
    // `TimeZone(secondsFromGMT:)` accepts at most 18 hours either way.
    if magnitude > 18 * 3600 {
        return None;
    }
    Some(if positive { magnitude } else { -magnitude })
}

/// Days since 1970-01-01 for a lenient `year-month-day` in the ISO 8601
/// calendar (ICU's Gregorian calendar: Julian before 1582-10-15).
fn iso_calendar_days(year: i64, month: i64, day: i64) -> i64 {
    let cutover = gregorian_days(1582, 10, 1) + 14;
    if year >= 1582 {
        let gregorian = gregorian_days(year, month, 1) + day - 1;
        if gregorian >= cutover { gregorian } else { julian_days(year, month, 1) + day - 1 }
    } else {
        let julian = julian_days(year, month, 1) + day - 1;
        if julian < cutover { julian } else { gregorian_days(year, month, 1) + day - 1 }
    }
}

fn gregorian_days(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn julian_days(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    // Days since Julian 0000-03-01, then shifted so that Julian 1582-10-05
    // is Gregorian 1582-10-15.
    let days = y * 365 + y.div_euclid(4) + doy;
    let anchor = 1582 * 365 + 1582 / 4 + (153 * 7 + 2) / 5 + 4;
    days - anchor + gregorian_days(1582, 10, 15)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seconds(text: &str) -> Option<f64> {
        parse_iso8601(text).map(Date::time_interval_since_1970)
    }

    // Swift 6.4, macOS 26:
    //   let d = JSONDecoder(); d.dateDecodingStrategy = .iso8601
    //   try d.decode(T.self, from: #"{"v":"<text>"}"#).v.timeIntervalSince1970
    #[test]
    fn iso8601_matches_the_probe() {
        let cases: &[(&str, Option<f64>)] = &[
            ("2024-01-01T00:00:00Z", Some(1704067200.0)),
            ("2024-01-01T00:00:00.5Z", Some(1704067200.5)),
            ("2024-01-01T00:00:00+01:00", Some(1704063600.0)),
            ("2024-01-01T00:00:00+0100", Some(1704063600.0)),
            ("2024-01-01T00:00:00z", Some(1704067200.0)),
            ("2024-01-01T00:00:00", None),
            ("2024-01-01 00:00:00Z", None),
            ("2024-1-01T00:00:00Z", Some(1704067200.0)),
            ("2024-02-30T00:00:00Z", Some(1709251200.0)),
            ("2024-01-01T24:00:00Z", Some(1704153600.0)),
            ("2024-01-01T23:59:60Z", Some(1704153600.0)),
            ("20240101T000000Z", None),
            ("2024-01-01T00:00:00+01", Some(1704063600.0)),
            ("2024-01-01T00:00:00-00:30", Some(1704069000.0)),
            ("0001-01-01T00:00:00Z", Some(-62135769600.0)),
            ("0000-01-01T00:00:00Z", Some(-62167392000.0)),
            ("12024-01-01T00:00:00Z", Some(317273587200.0)),
            (" 2024-01-01T00:00:00Z", None),
            ("2024-01-01T00:00:00Z ", Some(1704067200.0)),
            ("2024-01-01T00:00:00GMT", Some(1704067200.0)),
            ("2024-01-01T00:00:00+14:00", Some(1704016800.0)),
            ("2024-01-01T00:00:00+25:00", None),
            ("2024-13-01T00:00:00Z", None),
            ("2024-01-01T00:60:00Z", Some(1704070800.0)),
            ("2024-01-01t00:00:00Z", None),
            ("2024-01-01T00:00:00Zjunk", Some(1704067200.0)),
            ("2024-01-01T0:0:0Z", Some(1704067200.0)),
            ("2024-01-01T00:00Z", None),
            ("2024-01-01T00:00:00.Z", None),
            ("-2024-01-01T00:00:00Z", None),
            ("2024-01-01T00:00:00.9999999999Z", None),
            ("2024-00-01T00:00:00Z", None),
            ("2024-01-00T00:00:00Z", None),
            ("2024-01-32T00:00:00Z", None),
            ("2024-01-01T25:00:00Z", Some(1704157200.0)),
            ("99-01-01T00:00:00Z", Some(-59043168000.0)),
            ("2024-01-01T00:00:00+01:30:00", Some(1704061800.0)),
            ("2024-01-01T00:00:00 Z", None),
            ("2024-01-01T00:00:00+100", Some(1704031200.0)),
            ("2024-01-01T00:00:00+18:00", Some(1704002400.0)),
            ("2024-01-01T00:00:00+18:01", None),
            ("2024-01-01T00:00:00-18:00:01", None),
            ("2024-01-01T00:00:00+01:60", Some(1704060000.0)),
            ("2024-01-01T00:00:00+01:00:60", Some(1704063540.0)),
            ("2024-01-01T00:00:00+", None),
            ("2024-01-01T00:00:00+01:", None),
            ("2024-01-01T00:00:00GMT+1", Some(1704063600.0)),
            ("2024-01-01T00:00:00utc+2", Some(1704060000.0)),
            ("2024-01-01T00:00:00GMTx", Some(1704067200.0)),
            ("2024-01-01T00:00:00GM", None),
            ("2024-01-01T100:00:00Z", Some(1704427200.0)),
            ("2024-001-01T00:00:00Z", Some(1704067200.0)),
            ("2023-02-29T00:00:00Z", Some(1677628800.0)),
            ("1-1-1T1:1:1Z", Some(-62135765939.0)),
            ("1582-10-04T00:00:00Z", Some(-12219379200.0)),
            ("1582-10-05T00:00:00Z", Some(-12219292800.0)),
            ("1582-10-14T00:00:00Z", Some(-12218515200.0)),
            ("1582-10-15T00:00:00Z", Some(-12219292800.0)),
            ("1700-02-29T00:00:00Z", Some(-8515238400.0)),
            ("0004-02-29T00:00:00Z", Some(-62036064000.0)),
            ("0100-02-29T00:00:00Z", Some(-59006534400.0)),
            ("506714-12-31T23:59:59Z", Some(15928213679999.0)),
            ("506715-01-01T00:00:00Z", None),
        ];
        for (text, expected) in cases {
            assert_eq!(seconds(text), *expected, "{text}");
        }
    }

    #[test]
    fn integers_follow_json_decoder() {
        let int = |text: &str| parse(format!("[{text}]").as_bytes()).unwrap().array_value().unwrap()[0].int_value().ok();
        assert_eq!(int("1.0"), Some(1));
        assert_eq!(int("1e2"), Some(100));
        assert_eq!(int("100e-2"), Some(1));
        assert_eq!(int("1e-400"), Some(0));
        assert_eq!(int("-0.0"), Some(0));
        assert_eq!(int("1.5"), None);
        assert_eq!(int("1e19"), None);
        assert_eq!(int("9223372036854775807"), Some(i64::MAX));
        assert_eq!(int("9223372036854775808"), None);
        assert_eq!(int("-9223372036854775808"), Some(i64::MIN));
        assert_eq!(int("-9223372036854775808.0"), None);
        assert_eq!(int("1.00000000000000000001"), Some(1));
        assert_eq!(int("01"), None);
        assert_eq!(int("1."), None);
        // Beyond 2^53 the literal is read exactly and its integer part kept.
        assert_eq!(int("9007199254740993.0"), Some(9_007_199_254_740_993));
        assert_eq!(int("9007199254740992.5"), Some(9_007_199_254_740_992));
        assert_eq!(int("12345678901234567.8"), Some(12_345_678_901_234_567));
        assert_eq!(int("1.5e17"), Some(150_000_000_000_000_000));
        assert_eq!(int("-4611686018427387905.0"), Some(-4_611_686_018_427_387_905));
        assert_eq!(int("9223372036854775806.0"), None);
        assert_eq!(int("9223372036854775807e0"), None);
        let double = |text: &str| parse(format!("[{text}]").as_bytes()).unwrap().array_value().unwrap()[0].double_value().ok();
        assert_eq!(double("1e400"), None);
        assert_eq!(double("1e-400"), None);
        assert_eq!(double("-0").map(f64::to_bits), Some((-0.0f64).to_bits()));
        // Swift 6.4, macOS 26: which zero literals `JSONDecoder` rejects as a
        // `Double`, over significands × exponents.
        let rejected = "0e1 0e2 0e9 0E1 0e5 0.00e1 0.00e2 0.00e9 0.00e+1 0.00e-1 0.00e01 0.00e10 0.00E1 0.00e-5 0.00e5 0.00e-2 0.00e-9 0.00e+9 0.000e1 0.000e2 0.000e9 0.000E1 0.000e5 -0.0e1 -0.0e2 -0.0e9 -0.0e+1 -0.0e-1 -0.0e01 -0.0e10 -0.0E1 -0.0e-5 -0.0e5 -0.0e-2 -0.0e-9 -0.0e+9 -0.00e1 -0.00e2 -0.00e9 -0.00E1 -0.00e5";
        let accepted = "0 0e0 0e+1 0e-1 0e00 0e01 0e10 0e-5 0e+0 0e-0 0e-2 0e-9 0e+9 0e100 0e-100 0.0 0.0e0 0.0e1 0.0e2 0.0e9 0.0e+1 0.0e-1 0.0e00 0.0e01 0.0e10 0.0E1 0.0e-5 0.0e+0 0.0e-0 0.0e5 0.0e-2 0.0e-9 0.0e+9 0.0e100 0.0e-100 0.00 0.00e0 0.00e00 0.00e+0 0.00e-0 0.00e100 0.00e-100 0.000 0.000e0 0.000e+1 0.000e-1 0.000e00 0.000e01 0.000e10 0.000e-5 0.000e+0 0.000e-0 0.000e-2 0.000e-9 0.000e+9 0.000e100 0.000e-100 -0 -0e0 -0e1 -0e2 -0e9 -0e+1 -0e-1 -0e00 -0e01 -0e10 -0E1 -0e-5 -0e+0 -0e-0 -0e5 -0e-2 -0e-9 -0e+9 -0e100 -0e-100 -0.0 -0.0e0 -0.0e00 -0.0e+0 -0.0e-0 -0.0e100 -0.0e-100 -0.00 -0.00e0 -0.00e+1 -0.00e-1 -0.00e00 -0.00e01 -0.00e10 -0.00e-5 -0.00e+0 -0.00e-0 -0.00e-2 -0.00e-9 -0.00e+9 -0.00e100 -0.00e-100";
        for literal in rejected.split(' ') {
            assert_eq!(double(literal), None, "{literal}");
        }
        for literal in accepted.split(' ') {
            assert!(double(literal).is_some(), "{literal}");
        }
    }

    // Swift 6.4, macOS 26: `(error as NSError).code` of `JSONDecoder` over
    // `{"v": …}` into `Int`, `Double`, `UUID` and `.iso8601` `Date` fields:
    // null 4865; a string for a number, `1.5` or `01` for an `Int`, `1e999`,
    // a bad or numeric UUID or date 4864.
    #[test]
    fn error_codes_follow_json_decoder() {
        let value = |text: &str| parse(format!("[{text}]").as_bytes()).unwrap().array_value().unwrap()[0].clone();
        assert_eq!(value("null").int_value().unwrap_err().code(), 4865);
        assert_eq!(value("\"1\"").int_value().unwrap_err().code(), 4864);
        assert_eq!(value("1.5").int_value().unwrap_err().code(), 4864);
        assert_eq!(value("01").int_value().unwrap_err().code(), 4864);
        assert_eq!(value("null").double_value().unwrap_err().code(), 4865);
        assert_eq!(value("1e999").double_value().unwrap_err().code(), 4864);
        assert_eq!(value("null").uuid_value().unwrap_err().code(), 4865);
        assert_eq!(value("\"x\"").uuid_value().unwrap_err().code(), 4864);
        assert_eq!(value("1").uuid_value().unwrap_err().code(), 4864);
        assert_eq!(value("null").date_iso8601().unwrap_err().code(), 4865);
        assert_eq!(value("\"x\"").date_iso8601().unwrap_err().code(), 4864);
    }

    #[test]
    fn uuids_follow_uuid_string() {
        let bytes = parse_uuid("e621e1f8-c36c-495a-93fc-0c247a3e6e5f").unwrap();
        assert_eq!(uuid_string(&bytes), "E621E1F8-C36C-495A-93FC-0C247A3E6E5F");
        assert_eq!(parse_uuid("E621E1F8C36C495A93FC0C247A3E6E5F"), None);
        assert_eq!(parse_uuid("{E621E1F8-C36C-495A-93FC-0C247A3E6E5F}"), None);
    }

    #[test]
    fn keyed_containers_follow_synthesized_decoding() {
        let value = parse(br#"{"a":1,"a":2,"b":null}"#).unwrap();
        let keyed = value.keyed_container().unwrap();
        assert_eq!(keyed.decode("a", Value::int_value), Ok(1));
        assert!(keyed.decode("b", Value::int_value).is_err());
        assert!(keyed.decode("c", Value::int_value).is_err());
        assert_eq!(keyed.decode_if_present("b", Value::int_value), Ok(None));
        assert_eq!(keyed.decode_if_present("c", Value::int_value), Ok(None));
    }
}
