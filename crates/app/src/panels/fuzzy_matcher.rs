//! Port of `Sources/DownrightApp/Panels/FuzzyMatcher.swift`.
//!
//! Subsequence fuzzy matching for the outline quick-open panel (§7.2).
//!
//! A `contains` filter is not enough: typing `dpl` should find "Document
//! **p**ipe**l**ine", and it should rank it above a heading where the same
//! letters happen to fall mid-word. So this is a real alignment — a small
//! dynamic program over (needle × haystack) that scores word-boundary,
//! prefix, and consecutive-run matches highest, and reports the positions it
//! chose so the panel can highlight exactly the characters that matched.
//!
//! Characters are Swift `Character`s (extended grapheme clusters) compared by
//! canonical equivalence, as the Swift `Array(needle)` / `==` make them.

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_foundation::{NSAttributedStringKey, NSDictionary, NSMutableAttributedString, NSRange as FoundationRange};
use upleft_swift_text::{self as swift_text, NSRange};

pub struct FuzzyMatcher;

/// `FuzzyMatcher.Match`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub score: isize,
    /// Character (not UTF-16) offsets into the haystack.
    pub positions: Vec<isize>,
}

// Weights. Tuned so that, for a two-character query, a prefix match beats a
// word-boundary match beats a consecutive mid-word run beats a scattered
// match — which is the ordering people actually expect.
const MATCH_BASE: isize = 16;
const BONUS_BOUNDARY: isize = 10;
const BONUS_CAMEL: isize = 8;
const BONUS_CONSECUTIVE: isize = 8;
const BONUS_EXACT_CASE: isize = 2;
const GAP_START: isize = -5;
const GAP_EXTENSION: isize = -1;
const LEADING_GAP_PENALTY: isize = -1;
const LEADING_GAP_FLOOR: isize = -12;
const UNREACHABLE: isize = isize::MIN / 4;

/// `"-_/.,:;()[]{}<>#*`\"'"`.
const SEPARATORS: &str = "-_/.,:;()[]{}<>#*`\"'";

/// `Character(c.lowercased())`, which traps when lowercasing yields more than
/// one `Character`.
fn lowercased_character(character: &str) -> String {
    let lowered = swift_text::lowercased(character);
    assert!(
        swift_text::count(&lowered) == 1,
        "Can't form a Character from a String containing more than one extended grapheme cluster"
    );
    lowered
}

impl FuzzyMatcher {
    /// `FuzzyMatcher.match(needle:in:)`. `None` when `needle` is not a
    /// subsequence of `haystack`. An empty needle matches everything with
    /// score 0, so an empty query lists the document.
    pub fn r#match(needle: &str, haystack: &str) -> Option<Match> {
        let needle_chars: Vec<&str> = swift_text::graphemes(needle).collect();
        if needle_chars.is_empty() {
            return Some(Match { score: 0, positions: Vec::new() });
        }
        let hay_chars: Vec<&str> = swift_text::graphemes(haystack).collect();
        if needle_chars.len() > hay_chars.len() {
            return None;
        }

        let lower_needle: Vec<String> = needle_chars.iter().map(|c| lowercased_character(c)).collect();
        let lower_hay: Vec<String> = hay_chars.iter().map(|c| lowercased_character(c)).collect();
        if !is_subsequence(&lower_needle, &lower_hay) {
            return None;
        }

        let n = lower_needle.len();
        let m = lower_hay.len();
        let bonuses = position_bonuses(&lower_hay, &hay_chars);

        // score[i][j] — best total with needle[i] aligned to haystack[j].
        let mut score = vec![vec![UNREACHABLE; m]; n];

        for j in 0..m {
            if !swift_text::char_eq(&lower_hay[j], &lower_needle[0]) {
                continue;
            }
            let leading = LEADING_GAP_FLOOR.max(LEADING_GAP_PENALTY * j as isize);
            score[0][j] = MATCH_BASE + bonuses[j] * 2 + case_bonus(needle_chars[0], hay_chars[j]) + leading;
        }

        for i in 1..n {
            for j in i..m {
                if !swift_text::char_eq(&lower_hay[j], &lower_needle[i]) {
                    continue;
                }
                let mut best = UNREACHABLE;
                for k in (i - 1)..j {
                    if score[i - 1][k] == UNREACHABLE {
                        continue;
                    }
                    let candidate = score[i - 1][k] + transition(j - k - 1);
                    if candidate > best {
                        best = candidate;
                    }
                }
                if best == UNREACHABLE {
                    continue;
                }
                score[i][j] = best + MATCH_BASE + bonuses[j] + case_bonus(needle_chars[i], hay_chars[j]);
            }
        }

        // `max(by:)` keeps the first maximum.
        let mut last: Option<usize> = None;
        for j in 0..m {
            if score[n - 1][j] == UNREACHABLE {
                continue;
            }
            if last.is_none_or(|best| score[n - 1][best] < score[n - 1][j]) {
                last = Some(j);
            }
        }
        let last = last?;

        // Traceback inverts the recurrence rather than storing parents: at each
        // step the predecessor is whichever k maximises the same expression the
        // forward pass maximised.
        let mut positions = vec![0isize; n];
        positions[n - 1] = last as isize;
        let mut j = last;
        for i in (1..n).rev() {
            let mut best_k = i - 1;
            let mut best_value = UNREACHABLE;
            for k in (i - 1)..j {
                if score[i - 1][k] == UNREACHABLE {
                    continue;
                }
                let candidate = score[i - 1][k] + transition(j - k - 1);
                if candidate > best_value {
                    best_value = candidate;
                    best_k = k;
                }
            }
            positions[i - 1] = best_k as isize;
            j = best_k;
        }

        Some(Match { score: score[n - 1][last], positions })
    }

    /// The UTF-16 ranges [`highlighted`](Self::highlighted) adds the highlight
    /// attributes to, one per matched Character, in string order.
    pub fn highlight_ranges(haystack: &str, positions: &[isize]) -> Vec<NSRange> {
        if positions.is_empty() {
            return Vec::new();
        }
        let mut ranges = Vec::new();
        let mut utf16_offset = 0isize;
        for (character_index, character) in swift_text::graphemes(haystack).enumerate() {
            let width = swift_text::utf16_count(character);
            if positions.contains(&(character_index as isize)) {
                ranges.push(NSRange::new(utf16_offset, width));
            }
            utf16_offset += width;
        }
        ranges
    }

    /// `FuzzyMatcher.highlighted(_:positions:base:highlight:)`: highlights the
    /// matched characters. Kept here so the character-offset → UTF-16
    /// conversion exists in exactly one place.
    pub fn highlighted(
        haystack: &str,
        positions: &[isize],
        base: &NSDictionary<NSAttributedStringKey, objc2::runtime::AnyObject>,
        highlight: &NSDictionary<NSAttributedStringKey, objc2::runtime::AnyObject>,
    ) -> Retained<NSMutableAttributedString> {
        let string = swift_text::ns::foundation::ns_from_utf16(&swift_text::ns::utf16(haystack));
        let attributed = unsafe {
            NSMutableAttributedString::initWithString_attributes(NSMutableAttributedString::alloc(), &string, Some(base))
        };
        for range in FuzzyMatcher::highlight_ranges(haystack, positions) {
            unsafe {
                attributed.addAttributes_range(
                    highlight,
                    FoundationRange::new(range.location as usize, range.length as usize),
                );
            }
        }
        attributed
    }
}

// MARK: - Scoring pieces

fn transition(gap: usize) -> isize {
    if gap == 0 { BONUS_CONSECUTIVE } else { GAP_START + GAP_EXTENSION * (gap as isize - 1) }
}

fn case_bonus(needle: &str, hay: &str) -> isize {
    if swift_text::char_eq(needle, hay) { BONUS_EXACT_CASE } else { 0 }
}

/// Word-boundary and camelCase bonuses, precomputed per haystack position.
fn position_bonuses(lower: &[String], original: &[&str]) -> Vec<isize> {
    let mut bonuses = vec![0isize; lower.len()];
    for j in 0..lower.len() {
        if j == 0 || is_separator(original[j - 1]) {
            bonuses[j] = BONUS_BOUNDARY;
        } else if swift_text::is_lowercase(original[j - 1]) && swift_text::is_uppercase(original[j]) {
            bonuses[j] = BONUS_CAMEL;
        }
    }
    bonuses
}

fn is_separator(character: &str) -> bool {
    swift_text::is_whitespace(character) || SEPARATORS.chars().any(|separator| swift_text::char_is(character, separator))
}

fn is_subsequence(needle: &[String], hay: &[String]) -> bool {
    let mut index = 0;
    for character in hay {
        if swift_text::char_eq(character, &needle[index]) {
            index += 1;
            if index == needle.len() {
                return true;
            }
        }
    }
    false
}
