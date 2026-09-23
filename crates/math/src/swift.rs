//! The Swift `String`, `Character` and numeric semantics SwiftMath leans on.
//!
//! SwiftMath walks its input by `Character` (extended grapheme clusters),
//! compares characters with `<`/`==` (canonical equivalence, ordered by the
//! NFC-normalised scalars), and sums a character's scalars into a
//! `UTF32Char`. These helpers reproduce each of those on `&str` slices that
//! hold exactly one grapheme cluster.

use std::cmp::Ordering;

use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

/// The characters (extended grapheme clusters) of `s`, as Swift iterates them.
pub fn characters(s: &str) -> impl DoubleEndedIterator<Item = &str> {
    s.graphemes(true)
}

/// `String.count`.
pub fn count(s: &str) -> usize {
    if s.is_ascii() && !s.contains('\r') {
        return s.len();
    }
    s.graphemes(true).count()
}

/// The first character of `s` (`s[s.startIndex]`).
pub fn first_character(s: &str) -> Option<&str> {
    s.graphemes(true).next()
}

/// The last character of `s` (`s[s.index(before: s.endIndex)]`).
pub fn last_character(s: &str) -> Option<&str> {
    s.graphemes(true).next_back()
}

/// SwiftMath's `Character.utf32Char`: the *sum* of the character's scalars.
pub fn utf32_char(ch: &str) -> u32 {
    ch.chars().map(|c| c as u32).sum()
}

/// `Character ==`: canonical equivalence.
pub fn equal(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if a.is_ascii() && b.is_ascii() {
        return false;
    }
    a.nfc().eq(b.nfc())
}

/// `Character <` / `String <`: lexicographic order of the NFC-normalised
/// Unicode scalars.
pub fn compare(a: &str, b: &str) -> Ordering {
    if a.is_ascii() && b.is_ascii() {
        return a.as_bytes().cmp(b.as_bytes());
    }
    a.nfc().cmp(b.nfc())
}

/// `lower...upper ~= ch` for a `ClosedRange<String>` or `ClosedRange<Character>`.
pub fn in_closed_range(ch: &str, lower: &str, upper: &str) -> bool {
    compare(lower, ch) != Ordering::Greater && compare(ch, upper) != Ordering::Greater
}

/// NFC form, for looking a character up in a table keyed by `Character`.
pub fn nfc(ch: &str) -> std::borrow::Cow<'_, str> {
    if ch.is_ascii() {
        std::borrow::Cow::Borrowed(ch)
    } else {
        std::borrow::Cow::Owned(ch.nfc().collect())
    }
}

fn single_scalar(ch: &str) -> Option<char> {
    let mut chars = ch.chars();
    let first = chars.next()?;
    chars.next().is_none().then_some(first)
}

fn first_scalar(ch: &str) -> char {
    ch.chars().next().unwrap_or('\0')
}

/// Unicode `Lt` (titlecase letter): the part of `Cased` that is neither
/// `Lowercase` nor `Uppercase`.
fn is_titlecase(c: char) -> bool {
    matches!(
        c as u32,
        0x01C5 | 0x01C8 | 0x01CB | 0x01F2 | 0x1F88..=0x1F8F | 0x1F98..=0x1F9F | 0x1FA8..=0x1FAF | 0x1FBC | 0x1FCC | 0x1FFC
    )
}

fn is_cased_scalar(c: char) -> bool {
    c.is_lowercase() || c.is_uppercase() || is_titlecase(c)
}

fn is_uppercased(ch: &str) -> bool {
    ch.to_uppercase() == ch
}

fn is_lowercased(ch: &str) -> bool {
    ch.to_lowercase() == ch
}

/// `Character.isCased`.
pub fn is_cased(ch: &str) -> bool {
    if single_scalar(ch).is_some_and(is_cased_scalar) {
        return true;
    }
    !is_uppercased(ch) || !is_lowercased(ch)
}

/// `Character.isLowercase`.
pub fn is_lowercase(ch: &str) -> bool {
    if single_scalar(ch).is_some_and(char::is_lowercase) {
        return true;
    }
    is_lowercased(ch) && is_cased(ch)
}

/// `Character.isUppercase`.
pub fn is_uppercase(ch: &str) -> bool {
    if single_scalar(ch).is_some_and(char::is_uppercase) {
        return true;
    }
    is_uppercased(ch) && is_cased(ch)
}

/// `Character.isLetter`: the first scalar's `Alphabetic` property.
pub fn is_letter(ch: &str) -> bool {
    first_scalar(ch).is_alphabetic()
}

/// A string's characters as byte ranges, so a parser can hold an index and
/// step back (`index(before:)`) the way SwiftMath's builder does.
pub struct CharacterIndex {
    bounds: Vec<(usize, usize)>,
}

impl CharacterIndex {
    pub fn new(s: &str) -> Self {
        let bounds = if s.is_ascii() && !s.contains('\r') {
            (0..s.len()).map(|i| (i, i + 1)).collect()
        } else {
            s.grapheme_indices(true)
                .map(|(start, g)| (start, start + g.len()))
                .collect()
        };
        CharacterIndex { bounds }
    }

    pub fn len(&self) -> usize {
        self.bounds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bounds.is_empty()
    }

    pub fn get<'a>(&self, s: &'a str, index: usize) -> &'a str {
        let (start, end) = self.bounds[index];
        &s[start..end]
    }
}

/// `Swift.max(x, y)`: `y >= x ? y : x` (so `max(0, -0.0)` is `-0.0`).
#[inline]
pub fn max(x: f64, y: f64) -> f64 {
    if y >= x { y } else { x }
}

/// `Swift.min(x, y)`: `y < x ? y : x`.
#[inline]
pub fn min(x: f64, y: f64) -> f64 {
    if y < x { y } else { x }
}

unsafe extern "C" {
    #[link_name = "fmax"]
    fn c_fmax(x: f64, y: f64) -> f64;
}

/// C's `fmax`, which SwiftMath calls by name.
#[inline]
pub fn fmax(x: f64, y: f64) -> f64 {
    unsafe { c_fmax(x, y) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_is_nfc_scalar_order() {
        assert_eq!(compare("e\u{301}", "f"), Ordering::Greater);
        assert!(equal("e\u{301}", "é"));
        assert!(in_closed_range("a\u{20DD}", "a", "z"));
        assert!(in_closed_range("x\u{302}", "a", "z"));
        assert!(in_closed_range("\u{0438}\u{0306}", "\u{0410}", "\u{044F}"));
    }

    #[test]
    fn character_properties_match_swift() {
        assert!(is_lowercase("e\u{301}"));
        assert!(is_letter("e\u{301}"));
        assert!(!is_lowercase("ǅ") && !is_uppercase("ǅ"));
        assert!(!is_uppercase("ß") && is_lowercase("ß"));
        assert_eq!(count("a\r\nb"), 3);
    }

    #[test]
    fn swift_max_min_keep_signed_zero_order() {
        assert!(max(0.0, -0.0).is_sign_negative());
        assert!(min(0.0, -0.0).is_sign_positive());
    }
}
