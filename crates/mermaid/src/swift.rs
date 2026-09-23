//! The Swift standard library and Foundation behaviours beautiful-mermaid-swift
//! relies on, reproduced exactly. Not a port of a library file.
//!
//! - Regular expressions are ICU's, through `NSRegularExpression`, exactly as
//!   the Swift calls them (`NSRegularExpression.firstMatch`, NSString's
//!   `.regularExpression` search and replace). Compiled expressions are cached
//!   per thread; the Swift recompiles most of them per call.
//! - `String.count`, `dropFirst(_:)`, `hasPrefix`, `hasSuffix`, `==` and
//!   `Set<Character>` membership walk extended grapheme clusters under
//!   canonical equivalence.
//! - `Swift.min`/`max`, `Sequence.min()`/`max()` and `sort(by:)` keep Swift's
//!   comparison order, which matters for NaN and for inconsistent orderings.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CString;

use objc2::rc::Retained;
use objc2_foundation::{
    NSMatchingOptions, NSRange, NSRegularExpression, NSRegularExpressionOptions, NSString,
    NSTextCheckingResult,
};
pub use upleft_render::swift_compat::{smax, smin};

// MARK: - Numbers

/// `Swift.min(x, y)`: `y < x ? y : x`.
#[inline(always)]
pub fn min<T: PartialOrd>(x: T, y: T) -> T {
    if y < x { y } else { x }
}

/// `Swift.max(x, y)`: `y >= x ? y : x`.
#[inline(always)]
pub fn max<T: PartialOrd>(x: T, y: T) -> T {
    if y >= x { y } else { x }
}

/// `Swift.max(x, y, z, rest...)`: `max(max(x, y), z)`, then each `rest`
/// element `where value >= maxValue`.
pub fn max_n(values: &[f64]) -> f64 {
    let mut m = max(max(values[0], values[1]), values[2]);
    for &v in &values[3..] {
        if v >= m {
            m = v;
        }
    }
    m
}

/// `Swift.min(x, y, z, rest...)`: `min(min(x, y), z)`, then each `rest`
/// element `where value < minValue`.
pub fn min_n(values: &[f64]) -> f64 {
    let mut m = min(min(values[0], values[1]), values[2]);
    for &v in &values[3..] {
        if v < m {
            m = v;
        }
    }
    m
}

/// `Sequence.min()` on `Comparable` elements.
pub fn seq_min<T: PartialOrd + Copy>(items: impl IntoIterator<Item = T>) -> Option<T> {
    let mut it = items.into_iter();
    let mut result = it.next()?;
    for e in it {
        if e < result {
            result = e;
        }
    }
    Some(result)
}

/// `Sequence.max()` on `Comparable` elements.
pub fn seq_max<T: PartialOrd + Copy>(items: impl IntoIterator<Item = T>) -> Option<T> {
    let mut it = items.into_iter();
    let mut result = it.next()?;
    for e in it {
        if result < e {
            result = e;
        }
    }
    Some(result)
}

/// `Int(x)` on a `Double`: truncation. Swift traps on NaN and out-of-range
/// values; so does this.
#[inline]
pub fn int(x: f64) -> i64 {
    assert!(
        x.is_finite() && x > -9.223372036854777e18 && x < 9.223372036854776e18,
        "Int({x}) traps"
    );
    x as i64
}

/// `Double(text)`: `strtod` in the C locale, rejecting a leading ASCII space
/// or control whitespace and requiring the whole string to be consumed.
pub fn parse_double(text: &str) -> Option<f64> {
    let first = *text.as_bytes().first()?;
    if matches!(first, 9..=13 | 32) {
        return None;
    }
    if text.as_bytes().contains(&0) {
        // `withCString` stops at the first NUL: strtod sees a prefix, and the
        // end pointer lands on that NUL.
        let prefix = &text[..text.find('\0').unwrap()];
        if prefix.is_empty() {
            return None;
        }
        return parse_double(prefix);
    }
    let c = CString::new(text).ok()?;
    unsafe extern "C" {
        fn strtod_l(nptr: *const libc_char, endptr: *mut *mut libc_char, loc: *mut std::ffi::c_void) -> f64;
    }
    #[allow(non_camel_case_types)]
    type libc_char = std::ffi::c_char;
    let mut end: *mut libc_char = std::ptr::null_mut();
    let value = unsafe { strtod_l(c.as_ptr(), &mut end, std::ptr::null_mut()) };
    if end.is_null() {
        return None;
    }
    let consumed = unsafe { end.offset_from(c.as_ptr()) } as usize;
    if consumed != text.len() {
        return None;
    }
    Some(value)
}

/// `Int(text)` (radix 10): an optional sign then ASCII digits, no overflow.
pub fn parse_int(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    let (negative, digits) = match bytes.first()? {
        b'+' => (false, &bytes[1..]),
        b'-' => (true, &bytes[1..]),
        _ => (false, bytes),
    };
    if digits.is_empty() {
        return None;
    }
    let mut value: i64 = 0;
    for &b in digits {
        if !b.is_ascii_digit() {
            return None;
        }
        let d = (b - b'0') as i64;
        value = if negative {
            value.checked_mul(10)?.checked_sub(d)?
        } else {
            value.checked_mul(10)?.checked_add(d)?
        };
    }
    Some(value)
}

/// `Int(text, radix: 16)`.
pub fn parse_int_hex(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    let (negative, digits) = match bytes.first()? {
        b'+' => (false, &bytes[1..]),
        b'-' => (true, &bytes[1..]),
        _ => (false, bytes),
    };
    if digits.is_empty() {
        return None;
    }
    let mut value: i64 = 0;
    for &b in digits {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return None,
        } as i64;
        value = if negative {
            value.checked_mul(16)?.checked_sub(d)?
        } else {
            value.checked_mul(16)?.checked_add(d)?
        };
    }
    Some(value)
}

/// `String(format:)` with one numeric argument, through the C library.
pub fn format_f64(format: &str, value: f64) -> String {
    let c = CString::new(format).unwrap();
    let mut buffer = [0u8; 512];
    let n = unsafe {
        snprintf(buffer.as_mut_ptr().cast(), buffer.len(), c.as_ptr(), value)
    };
    String::from_utf8_lossy(&buffer[..(n as usize).min(buffer.len() - 1)]).into_owned()
}

/// `String(format:)` with one `Int` argument.
pub fn format_i64(format: &str, value: i64) -> String {
    let c = CString::new(format).unwrap();
    let mut buffer = [0u8; 128];
    let n = unsafe {
        snprintf(buffer.as_mut_ptr().cast(), buffer.len(), c.as_ptr(), value)
    };
    String::from_utf8_lossy(&buffer[..(n as usize).min(buffer.len() - 1)]).into_owned()
}

unsafe extern "C" {
    fn snprintf(buf: *mut std::ffi::c_char, size: usize, format: *const std::ffi::c_char, ...) -> i32;
}

/// `Double.description`: the shortest representation that round-trips, in
/// Swift's spelling (`40.0`, `1e+16`, `5e-324`, `inf`, `nan`).
pub fn double_description(value: f64) -> String {
    if value.is_nan() {
        return "nan".into();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf".into() } else { "inf".into() };
    }
    let abs = value.abs();
    if value == 0.0 || (abs >= 1e-4 && abs < 2f64.powi(53)) {
        let text = format!("{value:?}");
        return text;
    }
    // Exponent form: Rust's `{:e}` gives the shortest digits.
    let text = format!("{value:e}");
    let (mantissa, exponent) = text.split_once('e').unwrap();
    let exponent: i32 = exponent.parse().unwrap();
    let sign = if exponent < 0 { '-' } else { '+' };
    format!("{mantissa}e{sign}{:02}", exponent.abs())
}

// MARK: - Strings
//
// Character-level operations delegate to `upleft-swift-text`, whose grapheme
// breaking and canonical equivalence come from the Swift runtime's tables.

pub use upleft_swift_text::{str_eq as string_eq, string_key};

/// `String.count`.
pub fn character_count(text: &str) -> usize {
    upleft_swift_text::count(text)
}

/// `trimmingCharacters(in: .whitespaces)`.
pub fn trim_whitespaces(text: &str) -> &str {
    upleft_swift_text::trim_whitespaces(text)
}

/// `trimmingCharacters(in: .whitespacesAndNewlines)`.
pub fn trim_whitespaces_and_newlines(text: &str) -> &str {
    upleft_swift_text::trim_whitespaces_and_newlines(text)
}

/// `String.lowercased()`.
pub fn lowercased(text: &str) -> String {
    upleft_swift_text::lowercased(text)
}

/// `String.uppercased()`.
pub fn uppercased(text: &str) -> String {
    upleft_swift_text::uppercased(text)
}

/// `text.components(separatedBy: CharacterSet(charactersIn: chars))`: split
/// on every scalar in the set, keeping empty pieces.
pub fn components_separated_by_set<'a>(text: &'a str, set: impl Fn(char) -> bool) -> Vec<&'a str> {
    text.split(set).collect()
}

/// `CharacterSet.newlines`.
pub fn is_newline(c: char) -> bool {
    matches!(c as u32, 0x000A..=0x000D | 0x0085 | 0x2028 | 0x2029)
}

/// `text.components(separatedBy: "\n")`: NSString's search, which splits
/// `\r\n` (probed); no other scalar is canonically equivalent to `\n`.
pub fn components_separated_by_newline(text: &str) -> Vec<&str> {
    text.split('\n').collect()
}

/// `String.dropFirst(n)`: drops `n` Characters.
pub fn drop_first(text: &str, n: usize) -> &str {
    upleft_swift_text::drop_first(text, n)
}

/// `String.dropLast(n)`.
pub fn drop_last(text: &str, n: usize) -> &str {
    upleft_swift_text::drop_last(text, n)
}

/// `String.hasPrefix(_:)`.
pub fn has_prefix(text: &str, prefix: &str) -> bool {
    upleft_swift_text::has_prefix(text, prefix)
}

/// `String.hasSuffix(_:)`.
pub fn has_suffix(text: &str, suffix: &str) -> bool {
    upleft_swift_text::has_suffix(text, suffix)
}

/// `Character == c` for an ASCII `c`.
pub fn grapheme_is(grapheme: &str, c: char) -> bool {
    upleft_swift_text::char_is(grapheme, c)
}

/// `String.first`.
pub fn first_grapheme(text: &str) -> Option<&str> {
    upleft_swift_text::first(text)
}

/// `text.split(separator: c)` (omitting empty pieces).
pub fn split_character(text: &str, separator: char) -> Vec<&str> {
    upleft_swift_text::split_default(text, separator)
}

/// `text.split(separator: c, omittingEmptySubsequences: false)`.
pub fn split_character_keeping_empty(text: &str, separator: char) -> Vec<&str> {
    upleft_swift_text::split(text, separator, usize::MAX, false)
}

/// `text.firstIndex(of: c)` as a byte offset.
pub fn first_index_of(text: &str, c: char) -> Option<usize> {
    upleft_swift_text::first_index_of(text, c)
}

/// `text.lastIndex(of: c)` as a byte offset.
pub fn last_index_of(text: &str, c: char) -> Option<usize> {
    upleft_swift_text::last_index_of(text, c)
}

/// The byte offset just past the Character starting at `offset`
/// (`index(after:)`).
pub fn index_after(text: &str, offset: usize) -> usize {
    upleft_swift_text::graphemes::next_boundary(text, offset)
}

/// `Character.isWhitespace`.
pub fn character_is_whitespace(grapheme: &str) -> bool {
    upleft_swift_text::is_whitespace(grapheme)
}

/// `text.split(whereSeparator: \.isWhitespace)`.
pub fn split_whitespace_characters(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut start: Option<usize> = None;
    let mut offset = 0;
    for grapheme in upleft_swift_text::graphemes(text) {
        if character_is_whitespace(grapheme) {
            if let Some(s) = start.take() {
                pieces.push(&text[s..offset]);
            }
        } else if start.is_none() {
            start = Some(offset);
        }
        offset += grapheme.len();
    }
    if let Some(s) = start {
        pieces.push(&text[s..]);
    }
    pieces
}

/// `text.replacingOccurrences(of: target, with: replacement)`: Foundation's
/// search (it will not split a composed sequence, but matches `\n` inside
/// `\r\n`).
pub fn replacing_occurrences(text: &str, target: &str, replacement: &str) -> String {
    upleft_swift_text::replacing_occurrences(text, target, replacement)
}

/// `String.contains(_: String)` on a native Swift string: Character-wise,
/// so `"a\r\nb"` does not contain `"\n"`.
pub fn contains(text: &str, needle: &str) -> bool {
    upleft_swift_text::contains(text, needle)
}

/// `Set<Character>(chars).contains(grapheme)`, `chars` ASCII.
pub fn character_in(grapheme: &str, chars: &str) -> bool {
    if grapheme.len() == 1 {
        return chars.as_bytes().contains(&grapheme.as_bytes()[0]);
    }
    chars.chars().any(|c| upleft_swift_text::char_is(grapheme, c))
}

// MARK: - Sorting

/// `MutableCollection.sort(by:)`: Swift's stable merge sort, comparator calls
/// in Swift's order.
pub fn sort_by<T: Clone, F: FnMut(&T, &T) -> bool>(v: &mut [T], mut less: F) {
    let count = v.len();
    let minimum_run_length = minimum_merge_run_length(count);
    if count <= minimum_run_length {
        if count > 0 {
            insertion_sort(v, 0, count, 1, &mut less);
        }
        return;
    }
    let mut buffer: Vec<T> = Vec::with_capacity(count / 2 + 1);
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut start = 0;
    while start < count {
        let (mut end, descending) = find_next_run(v, start, &mut less);
        if descending {
            v[start..end].reverse();
        }
        if end < count && end - start < minimum_run_length {
            let new_end = count.min(start + minimum_run_length);
            insertion_sort(v, start, new_end, end, &mut less);
            end = new_end;
        }
        runs.push((start, end));
        merge_top_runs(v, &mut runs, &mut buffer, &mut less);
        start = end;
    }
    while runs.len() > 1 {
        let i = runs.len() - 1;
        merge_runs(v, &mut runs, i, &mut buffer, &mut less);
    }
}

/// `Sequence.sorted(by:)`.
pub fn sorted_by<T: Clone, F: FnMut(&T, &T) -> bool>(items: impl IntoIterator<Item = T>, less: F) -> Vec<T> {
    let mut v: Vec<T> = items.into_iter().collect();
    sort_by(&mut v, less);
    v
}

fn minimum_merge_run_length(c: usize) -> usize {
    let bits_to_use = 6;
    if c < 1 << bits_to_use {
        return c;
    }
    let c = c as i64;
    let offset = (64 - bits_to_use) - c.leading_zeros() as i64;
    let mask = (1i64 << offset) - 1;
    ((c >> offset) + if c & mask == 0 { 0 } else { 1 }) as usize
}

fn insertion_sort<T, F: FnMut(&T, &T) -> bool>(v: &mut [T], lower: usize, upper: usize, sorted_end: usize, less: &mut F) {
    let mut sorted_end = sorted_end;
    while sorted_end != upper {
        let mut i = sorted_end;
        loop {
            let j = i - 1;
            if !less(&v[i], &v[j]) {
                break;
            }
            v.swap(i, j);
            i = j;
            if i == lower {
                break;
            }
        }
        sorted_end += 1;
    }
}

fn find_next_run<T, F: FnMut(&T, &T) -> bool>(v: &[T], start: usize, less: &mut F) -> (usize, bool) {
    let mut previous = start;
    let mut current = start + 1;
    if current >= v.len() {
        return (current, false);
    }
    let is_descending = less(&v[current], &v[previous]);
    loop {
        previous = current;
        current += 1;
        if !(current < v.len() && is_descending == less(&v[current], &v[previous])) {
            break;
        }
    }
    (current, is_descending)
}

fn merge<T: Clone, F: FnMut(&T, &T) -> bool>(v: &mut [T], low: usize, mid: usize, high: usize, buffer: &mut Vec<T>, less: &mut F) {
    let low_count = mid - low;
    let high_count = high - mid;
    buffer.clear();
    if low_count < high_count {
        buffer.extend_from_slice(&v[low..mid]);
        let mut buffer_low = 0;
        let buffer_high = low_count;
        let mut src_low = mid;
        let mut dest_low = low;
        while buffer_low < buffer_high && src_low < high {
            if less(&v[src_low], &buffer[buffer_low]) {
                v[dest_low] = v[src_low].clone();
                src_low += 1;
            } else {
                v[dest_low] = buffer[buffer_low].clone();
                buffer_low += 1;
            }
            dest_low += 1;
        }
        for k in buffer_low..buffer_high {
            v[dest_low] = buffer[k].clone();
            dest_low += 1;
        }
    } else {
        buffer.extend_from_slice(&v[mid..high]);
        let buffer_low = 0;
        let mut buffer_high = high_count;
        let mut dest_high = high;
        let mut src_high = mid;
        let mut dest_low = mid;
        while buffer_high > buffer_low && src_high > low {
            dest_high -= 1;
            if less(&buffer[buffer_high - 1], &v[src_high - 1]) {
                src_high -= 1;
                v[dest_high] = v[src_high].clone();
                dest_low -= 1;
            } else {
                buffer_high -= 1;
                v[dest_high] = buffer[buffer_high].clone();
            }
        }
        for k in buffer_low..buffer_high {
            v[dest_low] = buffer[k].clone();
            dest_low += 1;
        }
    }
}

fn merge_runs<T: Clone, F: FnMut(&T, &T) -> bool>(v: &mut [T], runs: &mut Vec<(usize, usize)>, i: usize, buffer: &mut Vec<T>, less: &mut F) {
    let low = runs[i - 1].0;
    let middle = runs[i].0;
    let high = runs[i].1;
    merge(v, low, middle, high, buffer, less);
    runs[i - 1] = (low, high);
    runs.remove(i);
}

fn merge_top_runs<T: Clone, F: FnMut(&T, &T) -> bool>(v: &mut [T], runs: &mut Vec<(usize, usize)>, buffer: &mut Vec<T>, less: &mut F) {
    let count = |r: (usize, usize)| r.1 - r.0;
    while runs.len() > 1 {
        let mut last_index = runs.len() - 1;
        if last_index >= 3 && count(runs[last_index - 3]) <= count(runs[last_index - 2]) + count(runs[last_index - 1]) {
            if count(runs[last_index - 2]) < count(runs[last_index]) {
                last_index -= 1;
            }
        } else if last_index >= 2 && count(runs[last_index - 2]) <= count(runs[last_index - 1]) + count(runs[last_index]) {
            if count(runs[last_index - 2]) < count(runs[last_index]) {
                last_index -= 1;
            }
        } else if count(runs[last_index - 1]) <= count(runs[last_index]) {
            // merge Y and Z below
        } else {
            break;
        }
        merge_runs(v, runs, last_index, buffer, less);
    }
}

// MARK: - Regular expressions (ICU, through NSRegularExpression)

/// A string prepared for regular-expression matching: the `NSString` and a
/// map from UTF-16 offsets back to byte offsets.
pub struct Text<'a> {
    pub text: &'a str,
    ns: Retained<NSString>,
    utf16_len: usize,
    /// For non-ASCII text: the byte offset of each UTF-16 offset (plus the end).
    offsets: Option<Vec<usize>>,
}

impl<'a> Text<'a> {
    pub fn new(text: &'a str) -> Text<'a> {
        let ns = NSString::from_str(text);
        if text.is_ascii() {
            return Text { text, ns, utf16_len: text.len(), offsets: None };
        }
        let mut offsets = Vec::with_capacity(text.len() + 1);
        for (byte, c) in text.char_indices() {
            offsets.push(byte);
            if c.len_utf16() == 2 {
                // An offset inside a surrogate pair maps to nothing valid.
                offsets.push(usize::MAX);
            }
        }
        offsets.push(text.len());
        let utf16_len = offsets.len() - 1;
        Text { text, ns, utf16_len, offsets: Some(offsets) }
    }

    fn byte(&self, utf16: usize) -> Option<usize> {
        match &self.offsets {
            None => Some(utf16),
            Some(offsets) => offsets.get(utf16).copied().filter(|&b| b != usize::MAX),
        }
    }

    /// `Range(nsRange, in: text).map { String(text[$0]) }`.
    pub fn slice(&self, range: NSRange) -> Option<&'a str> {
        if range.location == usize::MAX >> 1 || range.location == isize::MAX as usize {
            return None;
        }
        let start = self.byte(range.location)?;
        let end = self.byte(range.location + range.length)?;
        Some(&self.text[start..end])
    }

    pub fn full_range(&self) -> NSRange {
        NSRange { location: 0, length: self.utf16_len }
    }
}

/// `NSNotFound`.
pub const NS_NOT_FOUND: usize = isize::MAX as usize;

/// One match: the range of every group (`None` for `NSNotFound`).
pub struct Match {
    pub ranges: Vec<Option<NSRange>>,
}

impl Match {
    fn from(result: &NSTextCheckingResult) -> Match {
        let count = result.numberOfRanges();
        let ranges = (0..count)
            .map(|i| {
                let r = result.rangeAtIndex(i);
                (r.location != NS_NOT_FOUND).then_some(r)
            })
            .collect();
        Match { ranges }
    }

    pub fn range(&self) -> NSRange {
        self.ranges[0].unwrap()
    }

    /// The Swift idiom `Range(match.range(at: i), in: text).map { String(text[$0]) } ?? ""`.
    pub fn group<'a>(&self, text: &Text<'a>, i: usize) -> Option<&'a str> {
        self.ranges.get(i).copied().flatten().and_then(|r| text.slice(r))
    }

    pub fn count(&self) -> usize {
        self.ranges.len()
    }
}

/// A compiled ICU expression.
#[derive(Clone)]
pub struct Regex(Retained<NSRegularExpression>);

thread_local! {
    static CACHE: RefCell<HashMap<(&'static str, bool), Option<Regex>>> = RefCell::new(HashMap::new());
}

/// `try? NSRegularExpression(pattern:options:)`, cached per thread.
pub fn regex(pattern: &'static str, case_insensitive: bool) -> Option<Regex> {
    CACHE.with(|cache| {
        cache
            .borrow_mut()
            .entry((pattern, case_insensitive))
            .or_insert_with(|| Regex::compile(pattern, case_insensitive))
            .clone()
    })
}

impl Regex {
    pub fn compile(pattern: &str, case_insensitive: bool) -> Option<Regex> {
        let options = if case_insensitive {
            NSRegularExpressionOptions::CaseInsensitive
        } else {
            NSRegularExpressionOptions::empty()
        };
        NSRegularExpression::regularExpressionWithPattern_options_error(&NSString::from_str(pattern), options)
            .ok()
            .map(Regex)
    }

    /// `firstMatch(in:options:[]:range: whole string)`.
    pub fn first_match(&self, text: &Text) -> Option<Match> {
        self.0
            .firstMatchInString_options_range(&text.ns, NSMatchingOptions::empty(), text.full_range())
            .map(|r| Match::from(&r))
    }

    pub fn is_match(&self, text: &Text) -> bool {
        let r = self
            .0
            .rangeOfFirstMatchInString_options_range(&text.ns, NSMatchingOptions::empty(), text.full_range());
        r.location != NS_NOT_FOUND
    }

    /// `matches(in:options:[]:range: whole string)`.
    pub fn matches(&self, text: &Text) -> Vec<Match> {
        let array = self.0.matchesInString_options_range(&text.ns, NSMatchingOptions::empty(), text.full_range());
        array.iter().map(|r| Match::from(&r)).collect()
    }

    /// `stringByReplacingMatches(in:options:[]:range: whole:withTemplate:)`.
    pub fn replace(&self, text: &str, template: &str) -> String {
        let t = Text::new(text);
        let matches = self.matches(&t);
        if matches.is_empty() {
            return text.to_owned();
        }
        let mut out = String::with_capacity(text.len());
        let mut last = 0usize;
        for m in &matches {
            let r = m.range();
            let start = t.byte(r.location).unwrap();
            let end = t.byte(r.location + r.length).unwrap();
            out.push_str(&text[last..start]);
            expand_template(template, m, &t, &mut out);
            last = end;
        }
        out.push_str(&text[last..]);
        out
    }
}

/// ICU template expansion: `$n` inserts group `n` (digits are consumed while
/// the number stays a valid group), `\` escapes the next character.
fn expand_template(template: &str, m: &Match, t: &Text, out: &mut String) {
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if i + 1 < bytes.len() => {
                let c = template[i + 1..].chars().next().unwrap();
                out.push(c);
                i += 1 + c.len_utf8();
            }
            b'$' if i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() => {
                let mut group = (bytes[i + 1] - b'0') as usize;
                i += 2;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    let next = group * 10 + (bytes[i] - b'0') as usize;
                    if next >= m.count() {
                        break;
                    }
                    group = next;
                    i += 1;
                }
                if let Some(s) = m.group(t, group) {
                    out.push_str(s);
                }
            }
            _ => {
                let c = template[i..].chars().next().unwrap();
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
}

/// `text.range(of: pattern, options: .regularExpression[, .caseInsensitive]) != nil`.
pub fn regex_test(pattern: &'static str, text: &str, case_insensitive: bool) -> bool {
    match regex(pattern, case_insensitive) {
        Some(r) => r.is_match(&Text::new(text)),
        None => false,
    }
}

/// `text.replacingOccurrences(of: pattern, with: template, options: .regularExpression[, .caseInsensitive])`.
pub fn regex_replace(text: &str, pattern: &'static str, template: &str, case_insensitive: bool) -> String {
    match regex(pattern, case_insensitive) {
        Some(r) => r.replace(text, template),
        None => text.to_owned(),
    }
}

/// `text.range(of: pattern, options: .regularExpression)`: the matched
/// substring.
pub fn regex_find<'a>(pattern: &'static str, text: &'a str, case_insensitive: bool) -> Option<&'a str> {
    let r = regex(pattern, case_insensitive)?;
    let t = Text::new(text);
    let m = r.first_match(&t)?;
    m.group(&t, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_groups_and_replace() {
        let r = regex(r"^([\w-]+)\[(.+?)\]", false).unwrap();
        let t = Text::new("A[héllo] --> B");
        let m = r.first_match(&t).unwrap();
        assert_eq!(m.group(&t, 0), Some("A[héllo]"));
        assert_eq!(m.group(&t, 2), Some("héllo"));
        assert_eq!(regex_replace("a<br/>b<BR>c", r"<br\s*/?>", "\n", true), "a\nb\nc");
        assert_eq!(regex_replace("**x** y", r"\*\*(.+?)\*\*", "<b>$1</b>", false), "<b>x</b> y");
    }

    #[test]
    fn doubles() {
        assert_eq!(parse_double("1.5"), Some(1.5));
        assert_eq!(parse_double(" 1"), None);
        assert_eq!(parse_double("1 "), None);
        assert_eq!(parse_double(""), None);
        assert_eq!(double_description(40.0), "40.0");
        assert_eq!(double_description(1e16), "1e+16");
        assert_eq!(double_description(0.1), "0.1");
        assert_eq!(format_f64("%.1f", 2.25), "2.2");
        assert_eq!(format_i64("%02x", 10), "0a");
    }

    #[test]
    fn graphemes() {
        assert_eq!(drop_first("héllo", 2), "llo");
        assert_eq!(drop_last("e3_out", 4), "e3");
        assert!(has_suffix("e3_out", "_out"));
        assert!(character_in(";", "iltfjI1!|.,:;'"));
        assert!(character_in("\u{037E}", "iltfjI1!|.,:;'"));
    }
}
