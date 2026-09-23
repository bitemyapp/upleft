//! The two Swift `String` semantics swift-markdown relies on that differ from
//! byte comparison: `==` is canonical equivalence, and `contains(_:)` with a
//! `Character` compares whole extended grapheme clusters.

use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

/// Swift `String ==`: equal when canonically equivalent.
pub(crate) fn swift_string_eq(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if a.is_ascii() && b.is_ascii() {
        return false;
    }
    a.nfc().eq(b.nfc())
}

/// Swift `string.contains(character)` for a single-scalar `character`: true
/// when some extended grapheme cluster of `string` is exactly that character.
/// (A backtick followed by a combining mark is a different `Character`.)
pub(crate) fn swift_contains_character(string: &str, character: char) -> bool {
    let mut buffer = [0; 4];
    let character = &*character.encode_utf8(&mut buffer);
    if !string.contains(character) {
        return false;
    }
    string.graphemes(true).any(|grapheme| swift_string_eq(grapheme, character))
}
