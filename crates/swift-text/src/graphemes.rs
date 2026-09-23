//! Swift's extended grapheme cluster breaking (the stdlib's
//! `_GraphemeBreakingState.shouldBreak(between:and:)` and
//! `_hasGraphemeBreakBetween`), over the break classes recovered from the
//! Swift runtime by `scripts/gen-grapheme-tables.swift`.
//!
//! Swift segments forward with a fresh state per Character; boundaries found
//! backward are, by design, the same. Here a backward query scans back to a
//! position that is always a boundary and segments forward from there.

use super::grapheme_tables::{CLASSES, INCB_EXTEND, LINKING_CONSONANTS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphemeClass {
    Any,
    CR,
    LF,
    Control,
    Extend,
    ZWJ,
    RegionalIndicator,
    Prepend,
    SpacingMark,
    L,
    V,
    T,
    LV,
    LVT,
    ExtendedPictographic,
}

use GraphemeClass::*;

#[inline]
fn in_ranges(table: &[(u32, u32)], value: u32) -> bool {
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

#[inline]
pub fn class_of(c: char) -> GraphemeClass {
    let value = c as u32;
    if value < 0x7F {
        return match value {
            0x0D => CR,
            0x0A => LF,
            0x00..=0x1F => Control,
            _ => Any,
        };
    }
    match CLASSES.binary_search_by(|&(lo, hi, _)| {
        if hi < value {
            std::cmp::Ordering::Less
        } else if lo > value {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    }) {
        Ok(index) => CLASSES[index].2,
        Err(_) => Any,
    }
}

#[inline]
fn is_linking_consonant(c: char) -> bool {
    (c as u32) >= 0x900 && in_ranges(LINKING_CONSONANTS, c as u32)
}

#[inline]
fn is_virama(c: char) -> bool {
    matches!(c as u32, 0x94D | 0x9CD | 0xACD | 0xB4D | 0xC4D | 0xD4D)
}

/// `_hasGraphemeBreakBetween`: the stdlib's fast path for pairs that always
/// break.
#[inline]
fn has_break_when_paired(c: char) -> bool {
    matches!(c as u32,
        0x3400..=0xA4CF
        | 0x0000..=0x02FF
        | 0x3041..=0x3096
        | 0x30A1..=0x30FC
        | 0x0400..=0x0482
        | 0x061D..=0x064A
        | 0xAC00..=0xD7AF
        | 0x2010..=0x2029
        | 0x3000..=0x3029
        | 0xFF01..=0xFF9D)
}

#[derive(Default)]
struct State {
    is_in_emoji_sequence: bool,
    is_in_indic_sequence: bool,
    has_seen_virama: bool,
    should_break_ri: bool,
}

impl State {
    fn should_break(&mut self, scalar1: char, scalar2: char) -> bool {
        // GB3
        if scalar1 == '\r' && scalar2 == '\n' {
            return false;
        }
        if has_break_when_paired(scalar1) && has_break_when_paired(scalar2) {
            return true;
        }
        let x = class_of(scalar1);
        let y = class_of(scalar2);

        let mut enter_emoji_sequence = false;
        let mut enter_indic_sequence = false;
        let result = match (x, y) {
            (Any, Any) => true,
            // GB4, GB5
            (Control | CR | LF, _) => true,
            (_, Control | CR | LF) => true,
            // GB6
            (L, L) | (L, V) | (L, LV) | (L, LVT) => false,
            // GB7
            (LV, V) | (V, V) | (LV, T) | (V, T) => false,
            // GB8
            (LVT, T) | (T, T) => false,
            // GB9 (partial GB11, GB9c)
            (_, Extend) | (_, ZWJ) => {
                if x == ExtendedPictographic || (self.is_in_emoji_sequence && x == Extend) {
                    enter_emoji_sequence = true;
                }
                if self.is_in_indic_sequence || is_linking_consonant(scalar1) {
                    if y == Extend && !in_ranges(INCB_EXTEND, scalar2 as u32) {
                        self.is_in_emoji_sequence = enter_emoji_sequence;
                        self.is_in_indic_sequence = false;
                        return false;
                    }
                    enter_indic_sequence = true;
                    if is_virama(scalar2) {
                        self.has_seen_virama = true;
                    }
                }
                false
            }
            // GB9a
            (_, SpacingMark) => false,
            // GB9b
            (Prepend, _) => false,
            // GB11
            (ZWJ, ExtendedPictographic) => !self.is_in_emoji_sequence,
            // GB12, GB13
            (RegionalIndicator, RegionalIndicator) => {
                let result = self.should_break_ri;
                self.should_break_ri = !self.should_break_ri;
                result
            }
            // GB999, with GB9c
            _ => {
                if self.is_in_indic_sequence && self.has_seen_virama && is_linking_consonant(scalar2) {
                    self.has_seen_virama = false;
                    false
                } else {
                    true
                }
            }
        };
        self.is_in_emoji_sequence = enter_emoji_sequence;
        self.is_in_indic_sequence = enter_indic_sequence;
        result
    }
}

/// Byte offset of the Character boundary after the Character starting at
/// `start` (which must be a boundary).
pub fn next_boundary(s: &str, start: usize) -> usize {
    let mut chars = s[start..].char_indices();
    let Some((_, first)) = chars.next() else { return start };
    let mut state = State::default();
    let mut scalar1 = first;
    let mut end = start + first.len_utf8();
    for (offset, scalar2) in chars {
        // Two ASCII scalars other than CR LF always break.
        if (scalar1 as u32) < 0x80 && (scalar2 as u32) < 0x80 && !(scalar1 == '\r' && scalar2 == '\n') {
            return start + offset;
        }
        if state.should_break(scalar1, scalar2) {
            return start + offset;
        }
        scalar1 = scalar2;
        end = start + offset + scalar2.len_utf8();
    }
    end
}

/// A byte offset at or before `index` that is certainly a Character
/// boundary: the string start, or a position before an ASCII scalar other
/// than a LF after CR, whose predecessor is not `Prepend`.
pub fn safe_boundary_before(s: &str, index: usize) -> usize {
    let bytes = s.as_bytes();
    let mut i = index.min(s.len());
    while i > 0 {
        if i < bytes.len() && bytes[i] < 0x80 && !(bytes[i] == b'\n' && bytes[i - 1] == b'\r') {
            // The scalar before `i`.
            let previous = s[..i].chars().next_back().unwrap();
            if class_of(previous) != Prepend {
                return i;
            }
        }
        i -= 1;
        while i > 0 && !s.is_char_boundary(i) {
            i -= 1;
        }
    }
    0
}

/// Whether byte offset `index` of `s` is a Character boundary.
pub fn is_boundary(s: &str, index: usize) -> bool {
    if index == 0 || index >= s.len() {
        return index == 0 || index == s.len();
    }
    if !s.is_char_boundary(index) {
        return false;
    }
    let mut position = safe_boundary_before(s, index);
    while position < index {
        position = next_boundary(s, position);
    }
    position == index
}

/// Byte offset where the last Character ending at `end` begins.
pub fn previous_boundary(s: &str, end: usize) -> usize {
    if end == 0 {
        return 0;
    }
    let mut position = safe_boundary_before(s, end - 1);
    loop {
        let next = next_boundary(s, position);
        if next >= end {
            return position;
        }
        position = next;
    }
}
