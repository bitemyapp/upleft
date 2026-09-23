//! The Swift `String`, `Character` and numeric semantics SwiftMath leans on.
//!
//! SwiftMath walks its input by `Character` (extended grapheme clusters),
//! compares characters with `<`/`==` (canonical equivalence, ordered by the
//! NFC-normalised scalars), and sums a character's scalars into a
//! `UTF32Char`. These helpers reproduce each of those on `&str` slices that
//! hold exactly one grapheme cluster; the String and Character semantics come
//! from `upleft-swift-text`, whose tables are generated from the Swift
//! runtime and re-checked by the `unicode` suite.

use std::cmp::Ordering;

use upleft_swift_text as swift_text;

/// The characters (extended grapheme clusters) of `s`, as Swift iterates them.
#[inline]
pub fn characters(s: &str) -> impl DoubleEndedIterator<Item = &str> {
    swift_text::graphemes(s)
}

/// `String.count`.
#[inline]
pub fn count(s: &str) -> usize {
    swift_text::count(s)
}

/// The first character of `s` (`s[s.startIndex]`).
#[inline]
pub fn first_character(s: &str) -> Option<&str> {
    swift_text::first(s)
}

/// The last character of `s` (`s[s.index(before: s.endIndex)]`).
#[inline]
pub fn last_character(s: &str) -> Option<&str> {
    swift_text::last(s)
}

/// SwiftMath's `Character.utf32Char`: the *sum* of the character's scalars.
pub fn utf32_char(ch: &str) -> u32 {
    ch.chars().map(|c| c as u32).sum()
}

/// `Character ==`: canonical equivalence.
#[inline]
pub fn equal(a: &str, b: &str) -> bool {
    swift_text::str_eq(a, b)
}

/// `Character <` / `String <`: lexicographic order of the NFC-normalised
/// Unicode scalars.
#[inline]
pub fn compare(a: &str, b: &str) -> Ordering {
    swift_text::str_cmp(a, b)
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
        std::borrow::Cow::Owned(swift_text::nfc(ch))
    }
}

/// `Character.isCased`.
#[inline]
pub fn is_cased(ch: &str) -> bool {
    swift_text::is_cased(ch)
}

/// `Character.isLowercase`.
#[inline]
pub fn is_lowercase(ch: &str) -> bool {
    swift_text::is_lowercase(ch)
}

/// `Character.isUppercase`.
#[inline]
pub fn is_uppercase(ch: &str) -> bool {
    swift_text::is_uppercase(ch)
}

/// `Character.isLetter`: the first scalar's `Alphabetic` property.
#[inline]
pub fn is_letter(ch: &str) -> bool {
    swift_text::is_letter(ch)
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
            let mut start = 0;
            swift_text::graphemes(s)
                .map(|g| {
                    let bound = (start, start + g.len());
                    start += g.len();
                    bound
                })
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
