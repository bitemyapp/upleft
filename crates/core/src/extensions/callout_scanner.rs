//! Extensions/CalloutScanner.swift — `> [!NOTE]`, `> [!WARNING] Optional title` (§4.1).
//!
//! A blockquote that opens with one becomes a `.callout` and the marker line
//! is lifted out of the body text. Matching is case-insensitive and tolerates
//! the Obsidian fold suffixes (`[!NOTE]+` / `[!NOTE]-`).

use crate::model::CalloutKind;
use crate::ns_range::NSRange;
use crate::source_positions::SourceMap;
use crate::swift_text::{self, ns::NSStringExt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalloutMatch {
    pub kind: CalloutKind,
    pub title: Option<String>,
    /// `> [!NOTE] Title` including the trailing space, if any — the gutter
    /// marker for §6.1a.
    pub marker_range: NSRange,
}

pub struct CalloutScanner;

impl CalloutScanner {
    /// Inspects the first line of a blockquote whose range starts at
    /// `quote_range.location`.
    pub fn scan(map: &SourceMap, quote_range: NSRange) -> Option<CalloutMatch> {
        let text = map.text.as_slice();
        let line_index = map.line_containing(quote_range.location);
        let line_range = map.content_range_of_line(line_index);
        let start = line_range.location;
        let end = line_range.upper_bound();
        let mut i = start;

        let is_space = |c: u16| c == 0x20 || c == 0x09;
        while i < end && is_space(text.character_at(i)) {
            i += 1;
        }

        // Consume the quote markers themselves; a nested `> >` callout is still
        // a callout for the innermost quote.
        let mut saw_marker = false;
        while i < end {
            while i < end && is_space(text.character_at(i)) {
                i += 1;
            }
            if !(i < end && text.character_at(i) == 0x3E) {
                break;
            }
            saw_marker = true;
            i += 1;
        }
        if !saw_marker {
            return None;
        }
        while i < end && is_space(text.character_at(i)) {
            i += 1;
        }

        if !(i + 2 < end && text.character_at(i) == 0x5B && text.character_at(i + 1) == 0x21) {
            return None;
        }
        let mut j = i + 2;
        while j < end && text.character_at(j) != 0x5D {
            j += 1;
        }
        if !(j < end) {
            return None;
        }
        let kind = CalloutKind::from_token(&text.substring(NSRange::new(i + 2, j - i - 2)))?;
        j += 1;
        if j < end && (text.character_at(j) == 0x2B || text.character_at(j) == 0x2D) {
            j += 1;
        }

        let mut title_start = j;
        while title_start < end && is_space(text.character_at(title_start)) {
            title_start += 1;
        }
        let raw_title = text.substring(NSRange::new(title_start, end - title_start));
        let raw_title = swift_text::trim_whitespaces(&raw_title);

        // The marker owns everything up to the body, which for a titled callout
        // means the title too — it renders in the panel header, not the body.
        let marker_length = if raw_title.is_empty() { j - start } else { end - start };
        Some(CalloutMatch {
            kind,
            title: if raw_title.is_empty() { None } else { Some(raw_title.to_owned()) },
            marker_range: NSRange::new(line_range.location, marker_length),
        })
    }
}
