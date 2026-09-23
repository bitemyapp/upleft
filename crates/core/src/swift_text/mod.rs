//! Swift `String` / `Character` and Foundation `CharacterSet` semantics.
//!
//! MarkdownCore mixes three levels of text:
//!
//! * **Characters** — `String.count`, `hasPrefix`, `contains`, `split`,
//!   `firstIndex(of:)`, `==` walk extended grapheme clusters and compare them
//!   up to canonical equivalence (`"\u{212A}" == "K"`, and `"a\r\nb"` does
//!   not contain `"\n"`).
//! * **Unicode scalars** — `trimmingCharacters(in:)`, `lowercased()`,
//!   `unicodeScalars`.
//! * **UTF-16 code units** — everything done through `NSString` (see [`ns`]).
//!
//! Each function here names the Swift API it reproduces. The property tables
//! in [`tables`] are generated from the Swift runtime itself
//! (`scripts/gen-unicode-tables.swift`), so `Character.isLetter` and friends
//! agree with Downright by construction.
//!
//! Grapheme segmentation ([`graphemes`]) reimplements the stdlib's breaking
//! state machine over break classes recovered from the Swift runtime. Stock
//! Unicode segmenters differ from Swift: its GB9c only treats the six Indic
//! viramas (U+094D, U+09CD, U+0ACD, U+0B4D, U+0C4D, U+0D4D) as linkers, and
//! it does not give the Kirat Rai vowel signs (U+16D63, U+16D67–U+16D6A)
//! Grapheme_Cluster_Break=V.

pub mod grapheme_tables;
pub mod graphemes;
pub mod ns;
pub mod tables;

use unicode_normalization::UnicodeNormalization;

// MARK: - Property tables

#[inline]
fn in_table(table: &[(u32, u32)], value: u32) -> bool {
    table
        .binary_search_by(|&(lo, hi)| {
            if hi < value {
                std::cmp::Ordering::Less
            } else if lo > value {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// `Unicode.Scalar.Properties.isAlphabetic`.
#[inline]
pub fn scalar_is_alphabetic(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_alphabetic();
    }
    in_table(tables::ALPHABETIC, c as u32)
}

/// `Unicode.Scalar.Properties.numericType != nil`.
#[inline]
pub fn scalar_is_numeric(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_digit();
    }
    in_table(tables::NUMERIC, c as u32)
}

/// `Unicode.Scalar.Properties.isWhitespace`.
#[inline]
pub fn scalar_is_white_space(c: char) -> bool {
    if c.is_ascii() {
        return matches!(c, '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ');
    }
    in_table(tables::WHITE_SPACE, c as u32)
}

/// `Unicode.Scalar.Properties.isGraphemeExtend`.
#[inline]
pub fn scalar_is_grapheme_extend(c: char) -> bool {
    !c.is_ascii() && in_table(tables::GRAPHEME_EXTEND, c as u32)
}

// MARK: - CharacterSet

/// The `CharacterSet`s MarkdownCore uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharSet {
    /// `.whitespaces`: Zs, U+0009 and U+200B.
    Whitespaces,
    /// `.whitespacesAndNewlines`.
    WhitespacesAndNewlines,
    /// `.newlines`.
    Newlines,
    /// `CharacterSet(charactersIn:)`.
    Chars(&'static str),
}

impl CharSet {
    #[inline]
    pub fn contains(&self, c: char) -> bool {
        match self {
            CharSet::Whitespaces => {
                if c.is_ascii() {
                    c == ' ' || c == '\t'
                } else {
                    in_table(tables::CS_WHITESPACES, c as u32)
                }
            }
            CharSet::WhitespacesAndNewlines => {
                if c.is_ascii() {
                    matches!(c, ' ' | '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r')
                } else {
                    in_table(tables::CS_WHITESPACES_AND_NEWLINES, c as u32)
                }
            }
            CharSet::Newlines => {
                if c.is_ascii() {
                    matches!(c, '\n' | '\u{0B}' | '\u{0C}' | '\r')
                } else {
                    in_table(tables::CS_NEWLINES, c as u32)
                }
            }
            CharSet::Chars(chars) => chars.contains(c),
        }
    }

    /// Membership of a UTF-16 code unit, as `NSString` asks it: a surrogate
    /// half is never a member of these sets.
    #[inline]
    pub fn contains_unit(&self, unit: u16) -> bool {
        char::from_u32(unit as u32).is_some_and(|c| self.contains(c))
    }
}

/// `trimmingCharacters(in:)`: strips leading and trailing *scalars* in `set`.
pub fn trimming(s: &str, set: CharSet) -> &str {
    let start = s.char_indices().find(|&(_, c)| !set.contains(c)).map_or(s.len(), |(i, _)| i);
    let rest = &s[start..];
    let end = rest.char_indices().rev().find(|&(_, c)| !set.contains(c)).map_or(0, |(i, c)| i + c.len_utf8());
    &rest[..end]
}

/// `trimmingCharacters(in: .whitespaces)`.
#[inline]
pub fn trim_whitespaces(s: &str) -> &str {
    trimming(s, CharSet::Whitespaces)
}

/// `trimmingCharacters(in: .whitespacesAndNewlines)`.
#[inline]
pub fn trim_whitespaces_and_newlines(s: &str) -> &str {
    trimming(s, CharSet::WhitespacesAndNewlines)
}

// MARK: - Case

/// `String.lowercased()`: each scalar's full `lowercaseMapping`, with no
/// context (a final sigma stays σ, unlike `NSString.lowercased`).
pub fn lowercased(s: &str) -> String {
    if s.is_ascii() {
        return s.to_ascii_lowercase();
    }
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c.to_ascii_lowercase());
        } else {
            match tables::LOWERCASE.binary_search_by_key(&(c as u32), |&(k, _)| k) {
                Ok(index) => out.push_str(tables::LOWERCASE[index].1),
                Err(_) => out.push(c),
            }
        }
    }
    out
}

/// `Unicode.Scalar.Properties.lowercaseMapping`.
pub fn scalar_lowercase_mapping(c: char) -> String {
    let mut out = String::new();
    let mut buffer = [0u8; 4];
    out.push_str(&lowercased(c.encode_utf8(&mut buffer)));
    out
}

// MARK: - Grapheme clusters

/// Swift `Character`s of `s`, as string slices.
pub fn graphemes(s: &str) -> Graphemes<'_> {
    if s.is_ascii() {
        Graphemes::Ascii { s, front: 0, back: s.len() }
    } else {
        Graphemes::Unicode { s, front: 0, back: s.len() }
    }
}

pub enum Graphemes<'a> {
    Ascii { s: &'a str, front: usize, back: usize },
    Unicode { s: &'a str, front: usize, back: usize },
}

impl<'a> Iterator for Graphemes<'a> {
    type Item = &'a str;

    #[inline]
    fn next(&mut self) -> Option<&'a str> {
        match self {
            Graphemes::Ascii { s, front, back } => {
                if *front >= *back {
                    return None;
                }
                let bytes = s.as_bytes();
                let start = *front;
                let end = if bytes[start] == b'\r' && start + 1 < *back && bytes[start + 1] == b'\n' {
                    start + 2
                } else {
                    start + 1
                };
                *front = end;
                Some(&s[start..end])
            }
            Graphemes::Unicode { s, front, back } => {
                if *front >= *back {
                    return None;
                }
                let start = *front;
                let end = graphemes::next_boundary(s, start).min(*back);
                *front = end;
                Some(&s[start..end])
            }
        }
    }
}

impl<'a> DoubleEndedIterator for Graphemes<'a> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a str> {
        match self {
            Graphemes::Ascii { s, front, back } => {
                if *front >= *back {
                    return None;
                }
                let bytes = s.as_bytes();
                let end = *back;
                let start = if bytes[end - 1] == b'\n' && end - 1 > *front && bytes[end - 2] == b'\r' {
                    end - 2
                } else {
                    end - 1
                };
                *back = start;
                Some(&s[start..end])
            }
            Graphemes::Unicode { s, front, back } => {
                if *front >= *back {
                    return None;
                }
                let end = *back;
                let start = graphemes::previous_boundary(s, end).max(*front);
                *back = start;
                Some(&s[start..end])
            }
        }
    }
}

/// Byte offsets where each Character of `s` starts, plus `s.len()`.
pub fn grapheme_boundaries(s: &str) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::with_capacity(s.len() + 1);
    let mut offset = 0;
    for g in graphemes(s) {
        out.push(offset);
        offset += g.len();
    }
    out.push(s.len());
    out
}

/// Whether byte offset `index` of `s` falls between two Characters.
pub fn is_grapheme_boundary(s: &str, index: usize) -> bool {
    if index == 0 || index == s.len() {
        return true;
    }
    if !s.is_char_boundary(index) {
        return false;
    }
    let bytes = s.as_bytes();
    if bytes[index - 1].is_ascii() && bytes[index].is_ascii() {
        // Only CR LF joins two ASCII scalars (GB3); anything else is split by
        // GB999 unless the next scalar extends, and ASCII never does.
        return !(bytes[index - 1] == b'\r' && bytes[index] == b'\n');
    }
    graphemes::is_boundary(s, index)
}

/// `String.count`.
pub fn count(s: &str) -> usize {
    if s.is_ascii() {
        // CR LF is one Character.
        let bytes = s.as_bytes();
        let pairs = bytes.windows(2).filter(|w| w[0] == b'\r' && w[1] == b'\n').count();
        return bytes.len() - pairs;
    }
    let mut count = 0;
    let mut position = 0;
    while position < s.len() {
        position = graphemes::next_boundary(s, position);
        count += 1;
    }
    count
}

/// `String.utf16.count`.
#[inline]
pub fn utf16_count(s: &str) -> isize {
    if s.is_ascii() {
        return s.len() as isize;
    }
    s.chars().map(|c| c.len_utf16() as isize).sum()
}

/// `String.first`.
#[inline]
pub fn first(s: &str) -> Option<&str> {
    graphemes(s).next()
}

/// `String.last`.
#[inline]
pub fn last(s: &str) -> Option<&str> {
    graphemes(s).next_back()
}

/// `String(s.dropFirst(n))`.
pub fn drop_first(s: &str, n: usize) -> &str {
    let mut it = graphemes(s);
    let mut offset = 0;
    for _ in 0..n {
        match it.next() {
            Some(g) => offset += g.len(),
            None => return "",
        }
    }
    &s[offset..]
}

/// `String(s.dropLast(n))`.
pub fn drop_last(s: &str, n: usize) -> &str {
    let mut it = graphemes(s);
    let mut end = s.len();
    for _ in 0..n {
        match it.next_back() {
            Some(g) => end -= g.len(),
            None => return "",
        }
    }
    &s[..end]
}

/// `String(s.prefix(n))`.
pub fn prefix(s: &str, n: usize) -> &str {
    let mut it = graphemes(s);
    let mut end = 0;
    for _ in 0..n {
        match it.next() {
            Some(g) => end += g.len(),
            None => break,
        }
    }
    &s[..end]
}

/// `String(s.suffix(n))`.
pub fn suffix(s: &str, n: usize) -> &str {
    let mut it = graphemes(s);
    let mut start = s.len();
    for _ in 0..n {
        match it.next_back() {
            Some(g) => start -= g.len(),
            None => break,
        }
    }
    &s[start..]
}

// MARK: - Character properties (first scalar, per the stdlib)

#[inline]
fn first_scalar(g: &str) -> char {
    g.chars().next().unwrap_or('\0')
}

/// `Character.isLetter`.
#[inline]
pub fn is_letter(g: &str) -> bool {
    scalar_is_alphabetic(first_scalar(g))
}

/// `Character.isNumber`.
#[inline]
pub fn is_number(g: &str) -> bool {
    scalar_is_numeric(first_scalar(g))
}

/// `Character.isWhitespace`.
#[inline]
pub fn is_whitespace(g: &str) -> bool {
    scalar_is_white_space(first_scalar(g))
}

/// `Character.isNewline`.
#[inline]
pub fn is_newline(g: &str) -> bool {
    matches!(first_scalar(g) as u32, 0x0A..=0x0D | 0x85 | 0x2028 | 0x2029)
}

/// `Character.isASCII` / `asciiValue != nil`: a single ASCII scalar, or CR LF.
#[inline]
pub fn is_ascii_character(g: &str) -> bool {
    g == "\r\n" || (g.len() == 1 && g.as_bytes()[0].is_ascii())
}

/// `Character.asciiValue` (CR LF reads as LF).
#[inline]
pub fn ascii_value(g: &str) -> Option<u8> {
    if g == "\r\n" {
        return Some(b'\n');
    }
    if g.len() == 1 && g.as_bytes()[0].is_ascii() { Some(g.as_bytes()[0]) } else { None }
}

// MARK: - Equality (canonical equivalence)

/// `Character == Character` (and `String == String`): canonical equivalence.
pub fn str_eq(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (x, y) = (a.as_bytes(), b.as_bytes());
    let limit = x.len().min(y.len());
    let mut first_difference = 0;
    while first_difference < limit && x[first_difference] == y[first_difference] {
        first_difference += 1;
    }
    // Normalise from the last shared ASCII byte before the first difference:
    // an ASCII scalar is a starter nothing before it composes with, and the
    // identical prefix normalises identically.
    let start = x[..first_difference].iter().rposition(u8::is_ascii).unwrap_or(0);
    let (a, b) = (&a[start..], &b[start..]);
    if a.is_ascii() && b.is_ascii() {
        return false;
    }
    a.nfc().eq(b.nfc())
}

/// `Character == Character`.
#[inline]
pub fn char_eq(a: &str, b: &str) -> bool {
    str_eq(a, b)
}

/// Whether the Character `g` equals the ASCII character `c`.
#[inline]
pub fn char_is(g: &str, c: char) -> bool {
    debug_assert!(c.is_ascii());
    if g.len() == 1 {
        return g.as_bytes()[0] == c as u8;
    }
    // A single scalar canonically equivalent to an ASCII one (KELVIN SIGN,
    // GREEK QUESTION MARK, GREEK VARIA).
    let mut chars = g.chars();
    match (chars.next(), chars.next()) {
        (Some(only), None) => ascii_singleton(only) == Some(c as u8),
        _ => false,
    }
}

/// The ASCII scalar a non-ASCII scalar is canonically equivalent to, if any.
#[inline]
pub fn ascii_singleton(c: char) -> Option<u8> {
    let value = c as u32;
    tables::ASCII_SINGLETONS.iter().find(|&&(k, _)| k == value).map(|&(_, v)| v)
}

#[inline]
fn has_ascii_singleton(s: &str) -> bool {
    !s.is_ascii() && s.chars().any(|c| ascii_singleton(c).is_some())
}

// MARK: - Character-wise search

/// `String.hasPrefix(_:)`: Character-wise `starts(with:)`.
pub fn has_prefix(s: &str, p: &str) -> bool {
    if p.is_empty() {
        return true;
    }
    if s.as_bytes().starts_with(p.as_bytes()) && is_grapheme_boundary(s, p.len()) {
        // The prefix ends on a Character boundary of `s`, so `s`'s first
        // Characters are exactly `p`'s.
        if p.is_ascii() || s.len() == p.len() {
            return true;
        }
        return characters_start_with(s, p);
    }
    if s.is_ascii() && p.is_ascii() {
        return false;
    }
    characters_start_with(s, p)
}

fn characters_start_with(s: &str, p: &str) -> bool {
    let mut a = graphemes(s);
    for g in graphemes(p) {
        match a.next() {
            Some(h) if char_eq(g, h) => {}
            _ => return false,
        }
    }
    true
}

/// `String.hasSuffix(_:)`.
pub fn has_suffix(s: &str, p: &str) -> bool {
    if p.is_empty() {
        return true;
    }
    if s.as_bytes().ends_with(p.as_bytes()) && is_grapheme_boundary(s, s.len() - p.len()) {
        if p.is_ascii() || s.len() == p.len() {
            return true;
        }
        return characters_end_with(s, p);
    }
    if s.is_ascii() && p.is_ascii() {
        return false;
    }
    characters_end_with(s, p)
}

fn characters_end_with(s: &str, p: &str) -> bool {
    let mut a = graphemes(s);
    for g in graphemes(p).rev() {
        match a.next_back() {
            Some(h) if char_eq(g, h) => {}
            _ => return false,
        }
    }
    true
}

/// `String.contains(_: String)` on a string whose provenance is known only as
/// "bridged from an `NSString` or not" (see [`contains_bridged`]).
#[inline]
pub fn contains_with(s: &str, needle: &str, bridged: bool) -> bool {
    if bridged { contains_bridged(s, needle) } else { contains(s, needle) }
}

/// `String.contains(_: String)` on an `NSString`-backed string.
///
/// Swift's `contains` answers differently depending on how the string was
/// made. On a native Swift string it is Character-wise ([`contains`]). On a
/// string bridged from an `NSString` — in Downright, any
/// `(text as NSString).substring(with:)` (and anything Foundation derives
/// from it: `trimmingCharacters`, `replacingOccurrences`, `components`) when
/// the document holds a non-ASCII character — it is Foundation's non-literal
/// search: `"é\r\nb"`-derived text contains `"\n"`, and `"<\u{200D}"`
/// contains `"<"`. `lowercased()` always returns a native string.
pub fn contains_bridged(s: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    if s.is_ascii() && needle.is_ascii() {
        return memfind(s.as_bytes(), needle.as_bytes()).is_some();
    }
    if needle.is_ascii() && !s.contains(needle) && !has_ascii_singleton(s) {
        return false;
    }
    ns::foundation::contains(s, needle)
}

/// Whether a document's `NSString` hands out bridged substrings: true when
/// the document holds any non-ASCII character (a native ASCII string's
/// substrings come back native).
#[inline]
pub fn bridges_substrings(document: &[u16]) -> bool {
    !document.iter().all(|&unit| unit < 0x80)
}

/// `String.contains(_: String)` — a Character-wise substring search.
pub fn contains(s: &str, needle: &str) -> bool {
    find(s, needle).is_some()
}

/// `String.contains(_: Character)`.
#[inline]
pub fn contains_char(s: &str, c: char) -> bool {
    let mut buffer = [0u8; 4];
    contains(s, c.encode_utf8(&mut buffer))
}

/// Byte range of the first Character-wise occurrence of `needle` in `s`
/// (`firstRange(of:)`), or `None`.
pub fn find(s: &str, needle: &str) -> Option<std::ops::Range<usize>> {
    if needle.is_empty() {
        return Some(0..0);
    }
    if needle.is_ascii() && !has_ascii_singleton(s) {
        // Byte candidates, kept only when both ends fall on Character
        // boundaries.
        let mut from = 0;
        while let Some(found) = memfind(&s.as_bytes()[from..], needle.as_bytes()) {
            let start = from + found;
            let end = start + needle.len();
            if is_grapheme_boundary(s, start) && is_grapheme_boundary(s, end) {
                return Some(start..end);
            }
            from = start + 1;
        }
        return None;
    }
    // General case: compare Character sequences.
    let hay: Vec<(usize, &str)> = {
        let mut offset = 0;
        graphemes(s)
            .map(|g| {
                let item = (offset, g);
                offset += g.len();
                item
            })
            .collect()
    };
    let pattern: Vec<&str> = graphemes(needle).collect();
    if pattern.len() > hay.len() {
        return None;
    }
    'outer: for start in 0..=hay.len() - pattern.len() {
        for (k, g) in pattern.iter().enumerate() {
            if !char_eq(hay[start + k].1, g) {
                continue 'outer;
            }
        }
        let last = &hay[start + pattern.len() - 1];
        return Some(hay[start].0..last.0 + last.1.len());
    }
    None
}

#[inline]
fn memfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() == 1 {
        return hay.iter().position(|&b| b == needle[0]);
    }
    if needle.len() > hay.len() {
        return None;
    }
    let first = needle[0];
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        match hay[i..hay.len() - needle.len() + 1].iter().position(|&b| b == first) {
            None => return None,
            Some(p) => {
                let at = i + p;
                if &hay[at..at + needle.len()] == needle {
                    return Some(at);
                }
                i = at + 1;
            }
        }
    }
    None
}

/// `String.firstIndex(of: Character)` as a byte offset.
pub fn first_index_of(s: &str, c: char) -> Option<usize> {
    let mut buffer = [0u8; 4];
    let needle = c.encode_utf8(&mut buffer);
    find(s, needle).map(|r| r.start)
}

/// `String.lastIndex(of: Character)` as a byte offset.
pub fn last_index_of(s: &str, c: char) -> Option<usize> {
    let mut end = s.len();
    for g in graphemes(s).rev() {
        end -= g.len();
        if c.is_ascii() {
            if char_is(g, c) {
                return Some(end);
            }
        } else {
            let mut buffer = [0u8; 4];
            if char_eq(g, c.encode_utf8(&mut buffer)) {
                return Some(end);
            }
        }
    }
    None
}

/// `String.firstIndex(where:)` over Characters, as a byte offset.
pub fn first_index_where(s: &str, mut predicate: impl FnMut(&str) -> bool) -> Option<usize> {
    let mut offset = 0;
    for g in graphemes(s) {
        if predicate(g) {
            return Some(offset);
        }
        offset += g.len();
    }
    None
}

/// `s.allSatisfy { … }` over Characters.
pub fn all_satisfy(s: &str, mut predicate: impl FnMut(&str) -> bool) -> bool {
    graphemes(s).all(|g| predicate(g))
}

/// `String(s.filter { … })` over Characters.
pub fn filter(s: &str, mut predicate: impl FnMut(&str) -> bool) -> String {
    let mut out = String::with_capacity(s.len());
    for g in graphemes(s) {
        if predicate(g) {
            out.push_str(g);
        }
    }
    out
}

/// `s.split(separator: Character, maxSplits:, omittingEmptySubsequences:)`.
pub fn split(s: &str, separator: char, max_splits: usize, omitting_empty: bool) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    let mut splits = 0;
    let mut buffer = [0u8; 4];
    let separator_str: &str = separator.encode_utf8(&mut buffer);
    for g in graphemes(s) {
        let is_separator = if separator.is_ascii() { char_is(g, separator) } else { char_eq(g, separator_str) };
        if is_separator && splits < max_splits {
            if !(omitting_empty && start == offset) {
                out.push(&s[start..offset]);
                splits += 1;
            }
            start = offset + g.len();
        }
        offset += g.len();
    }
    if !(omitting_empty && start == s.len()) {
        out.push(&s[start..]);
    }
    out
}

/// `s.split(separator:)` with Swift's defaults (unlimited, omitting empties).
#[inline]
pub fn split_default(s: &str, separator: char) -> Vec<&str> {
    split(s, separator, usize::MAX, true)
}

/// `Int(_: String)`: optional sign, ASCII digits only, `nil` on overflow.
pub fn parse_int(s: &str) -> Option<isize> {
    let bytes = s.as_bytes();
    let (negative, digits) = match bytes.first()? {
        b'+' => (false, &bytes[1..]),
        b'-' => (true, &bytes[1..]),
        _ => (false, bytes),
    };
    if digits.is_empty() {
        return None;
    }
    let mut value: isize = 0;
    for &b in digits {
        if !b.is_ascii_digit() {
            return None;
        }
        let digit = (b - b'0') as isize;
        value = value.checked_mul(10)?;
        value = if negative { value.checked_sub(digit)? } else { value.checked_add(digit)? };
    }
    Some(value)
}

/// `String(repeating:count:)`.
#[inline]
pub fn repeating(s: &str, count: isize) -> String {
    s.repeat(count.max(0) as usize)
}

/// `NSString.replacingOccurrences(of:with:)` on a Swift `String`: Foundation's
/// non-literal search. Plain byte replacement is exact when the text is ASCII;
/// anything else goes through Foundation itself, whose composed-character and
/// canonical-equivalence rules are not worth re-deriving.
pub fn replacing_occurrences(s: &str, target: &str, replacement: &str) -> String {
    if target.is_empty() {
        return s.to_owned();
    }
    if s.is_ascii() && target.is_ascii() {
        return s.replace(target, replacement);
    }
    if !s.contains(target) && target.is_ascii() && !has_ascii_singleton(s) {
        return s.to_owned();
    }
    ns::foundation::replacing_occurrences(s, target, replacement)
}

/// `String.components(separatedBy: String)` (Foundation, non-literal).
pub fn components_separated_by(s: &str, separator: &str) -> Vec<String> {
    if s.is_ascii() && separator.is_ascii() && !separator.is_empty() {
        return s.split(separator).map(str::to_owned).collect();
    }
    if !separator.is_empty() && separator.is_ascii() && !s.contains(separator) && !has_ascii_singleton(s) {
        return vec![s.to_owned()];
    }
    ns::foundation::components_separated_by(s, separator)
}

/// `String.components(separatedBy: CharacterSet)`: splits at every UTF-16
/// unit in the set (so CR LF yields an empty component between them).
pub fn components_separated_by_set(s: &str, set: CharSet) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in s.chars() {
        if set.contains(c) {
            out.push(std::mem::take(&mut current));
        } else {
            current.push(c);
        }
    }
    out.push(current);
    out
}

/// `caseInsensitiveCompare(_:) == .orderedSame`.
pub fn case_insensitive_equal(a: &str, b: &str) -> bool {
    if a.is_ascii() && b.is_ascii() {
        return a.eq_ignore_ascii_case(b);
    }
    ns::foundation::case_insensitive_compare(a, b) == std::cmp::Ordering::Equal
}

/// `caseInsensitiveCompare(_:)`.
pub fn case_insensitive_compare(a: &str, b: &str) -> std::cmp::Ordering {
    ns::foundation::case_insensitive_compare(a, b)
}

/// `String.capitalized` (Foundation).
pub fn capitalized(s: &str) -> String {
    ns::foundation::capitalized(s)
}

// MARK: - Leading indent helpers (SourcePositions.swift's `extension String`)

/// `leadingIndent`: the leading run of spaces and tabs.
pub fn leading_indent(s: &str) -> &str {
    let end = first_index_where(s, |g| g != " " && g != "\t").unwrap_or(s.len());
    &s[..end]
}

/// `indentColumns`: visual width of the leading indent, tabs to four.
pub fn indent_columns(s: &str) -> isize {
    let mut columns = 0isize;
    for g in graphemes(s) {
        if g == " " {
            columns += 1;
        } else if g == "\t" {
            columns += 4 - (columns % 4);
        } else {
            break;
        }
    }
    columns
}

/// `isBlankLine`: every Character a space or tab.
pub fn is_blank_line(s: &str) -> bool {
    all_satisfy(s, |g| g == " " || g == "\t")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expectations recorded from Swift 6.4 (see the probes in the port notes).
    #[test]
    fn character_semantics_match_swift() {
        assert!(!has_prefix("!\u{301}abc", "!"));
        assert!(!has_prefix("\r\nabc", "\r"));
        assert!(!has_suffix("abc\r\n", "\n"));
        assert!(!contains("a<\u{338}b", "<"));
        assert!(!contains("a\r\nb", "\n"));
        assert!(contains("a\u{212A}b", "K"));
        assert!(has_prefix("\u{212A}b", "K"));
        assert!(str_eq("é", "e\u{301}"));
        assert_eq!(split_default("a|\u{301}b", '|'), vec!["a|\u{301}b"]);
        assert_eq!(first_index_of("a:\u{301}b:c", ':'), Some(5));
        assert_eq!(count("क्षि"), 1);
        assert_eq!(count("🇺🇸🇬🇧🇫"), 3);
        assert_eq!(count("\r\n\r\n"), 2);
        assert!(!has_prefix("é", "e"));
        assert!(!contains("e\u{301}", "e"));
        assert!(is_whitespace("\r\n"));
        assert!(is_whitespace("\u{85}"));
        assert!(!is_whitespace("\u{200B}"));
        assert!(is_number("\u{4E00}"));
        assert!(is_letter("\u{4E00}"));
        assert_eq!(drop_first("\r\nab", 1), "ab");
    }

    #[test]
    fn scalar_semantics_match_swift() {
        assert_eq!(trim_whitespaces(" \u{301}a "), "\u{301}a");
        assert_eq!(trim_whitespaces_and_newlines("\u{A0}a\u{2028}"), "a");
        assert_eq!(trim_whitespaces("\u{85}a"), "\u{85}a");
        assert_eq!(lowercased("ΑΣ ΑΣ"), "ασ ασ");
        assert_eq!(lowercased("İ"), "i\u{307}");
        assert_eq!(lowercased("ẞ"), "ß");
    }

    #[test]
    fn foundation_semantics_match_swift() {
        assert_eq!(replacing_occurrences("a*\u{301}b", "*", ""), "a*\u{301}b");
        assert_eq!(replacing_occurrences("a\r\nb", "\r", "X"), "aX\nb");
        assert_eq!(replacing_occurrences("a\r\nb\r", "\r\n", "\n"), "a\nb\r");
        assert_eq!(replacing_occurrences("a\u{212A}b", "K", ""), "ab");
        assert_eq!(replacing_occurrences("a*\u{200D}b", "*", ""), "a\u{200D}b");
        assert_eq!(replacing_occurrences("a***b", "**", "X"), "aX*b");
        // A leading U+FEFF survives the trip through NSString.
        assert_eq!(replacing_occurrences("\u{FEFF}a\u{E9}", "a", "b"), "\u{FEFF}b\u{E9}");
        assert!(str_eq("xx\u{E9}!", "xxe\u{301}!"));
        assert!(!str_eq("xx\u{E9}!", "xxe\u{301}?"));
        assert_eq!(components_separated_by("a\r\nb", "\n"), vec!["a\r", "b"]);
        assert_eq!(components_separated_by_set("a\r\nb\u{2028}c", CharSet::Newlines), vec!["a", "", "b", "c"]);
        assert!(case_insensitive_equal("Title", "TITLE"));
        assert!(case_insensitive_equal("straße", "STRASSE"));
        assert!(case_insensitive_equal("\u{212A}", "k"));
    }
}

// MARK: - Dictionary keys

/// Swift `Dictionary<String, V>` keys compare by canonical equivalence. These
/// helpers keep a Rust `HashMap` keyed by the first-inserted spelling while
/// matching an equivalent spelling the way Swift would. ASCII keys (the
/// overwhelmingly common case) take the plain hash path.
pub fn dict_insert<V>(map: &mut std::collections::HashMap<String, V>, key: String, value: V) {
    if !key.is_ascii()
        && !map.contains_key(&key)
        && let Some(existing) = map.keys().find(|existing| str_eq(existing, &key)).cloned()
    {
        map.insert(existing, value);
        return;
    }
    map.insert(key, value);
}

/// Looks up `key` with Swift's canonical-equivalence key semantics.
pub fn dict_get<'a, V>(map: &'a std::collections::HashMap<String, V>, key: &str) -> Option<&'a V> {
    if let Some(value) = map.get(key) {
        return Some(value);
    }
    if key.is_ascii() && !map.keys().any(|k| !k.is_ascii()) {
        return None;
    }
    map.iter().find(|(existing, _)| str_eq(existing, key)).map(|(_, value)| value)
}

/// Swift's `String < String`: Unicode scalar order of the NFC forms.
pub fn str_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    if a.is_ascii() && b.is_ascii() {
        return a.cmp(b);
    }
    a.nfc().cmp(b.nfc())
}
