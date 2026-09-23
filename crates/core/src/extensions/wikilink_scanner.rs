//! Extensions/WikilinkScanner.swift — `[[Name]]` and `[[Name|Label]]` (§4.1).
//!
//! Produces a span and nothing else: no registry, no backlink table. cmark
//! leaves `[[Name]]` as literal text, so this only ever sees `.text` spans and
//! cannot match inside code.

use crate::ns_range::NSRange;
use crate::swift_text::{self, ns::NSStringExt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WikilinkMatch {
    pub range: NSRange,
    /// Text between the brackets, before any `|`.
    pub target_range: NSRange,
    pub target: String,
    pub label: Option<String>,
}

pub struct WikilinkScanner;

impl WikilinkScanner {
    pub fn matches(text: &[u16], range: NSRange) -> Vec<WikilinkMatch> {
        let mut out = Vec::new();
        let mut i = range.location;
        let end = range.upper_bound();
        while i + 3 < end {
            if !(text.character_at(i) == 0x5B && text.character_at(i + 1) == 0x5B) {
                i += 1;
                continue;
            }
            let Some(close) = Self::closing(text, i + 2, end) else {
                i += 1;
                continue;
            };
            let inner = NSRange::new(i + 2, close - (i + 2));
            // Reject empty or multi-line bodies on the raw UTF-16 buffer before
            // paying for a substring and a split.
            if inner.length > 0 && !Self::contains_newline(text, inner) {
                let body = text.substring(inner);
                // Character-wise: a `|` carrying a combining mark does not split.
                let parts = swift_text::split(&body, '|', 1, false);
                let target = swift_text::trim_whitespaces(parts[0]);
                let label = if parts.len() > 1 { Some(swift_text::trim_whitespaces(parts[1])) } else { None };
                if !target.is_empty() {
                    out.push(WikilinkMatch {
                        range: NSRange::new(i, close + 2 - i),
                        target_range: NSRange::new(inner.location, swift_text::utf16_count(parts[0])),
                        target: target.to_owned(),
                        label: label.and_then(|label| if label.is_empty() { None } else { Some(label.to_owned()) }),
                    });
                    i = close + 2;
                    continue;
                }
            }
            i += 2;
        }
        out
    }

    fn closing(text: &[u16], start: isize, end: isize) -> Option<isize> {
        let mut i = start;
        while i + 1 < end {
            let ch = text.character_at(i);
            if ch == 0x5D && text.character_at(i + 1) == 0x5D {
                return Some(i);
            }
            if ch == 0x5B {
                return None; // a nested `[` means this is not a wikilink
            }
            i += 1;
        }
        None
    }

    fn contains_newline(text: &[u16], range: NSRange) -> bool {
        text[range.as_usize_range()].iter().any(|&ch| ch == 0x0A || ch == 0x0D)
    }
}
