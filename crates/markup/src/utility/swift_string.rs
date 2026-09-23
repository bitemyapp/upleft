//! The two Swift `String` semantics swift-markdown relies on that differ from
//! byte comparison: `==` is canonical equivalence, and `contains(_:)` with a
//! `Character` compares whole extended grapheme clusters under canonical
//! equivalence. Both come from `upleft-swift-text`, whose tables are
//! generated from the Swift runtime and re-checked by the `unicode` suite.

/// Swift `String ==`: equal when canonically equivalent.
#[inline]
pub(crate) fn swift_string_eq(a: &str, b: &str) -> bool {
    upleft_swift_text::str_eq(a, b)
}

/// Swift `string.contains(character)`: true when some `Character` of `string`
/// is canonically equivalent to `character`. A backtick followed by a
/// combining mark is a different `Character`; U+1FEF GREEK VARIA is the same
/// `Character` as a backtick.
#[inline]
pub(crate) fn swift_contains_character(string: &str, character: char) -> bool {
    upleft_swift_text::contains_char(string, character)
}
