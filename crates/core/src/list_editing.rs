//! ListEditing.swift — list editing (§6.4).
//!
//! List continuation on ⏎, outdent-and-exit on an empty item, ⇥/⇧⇥ indent
//! and outdent. Continuation is a replacement rather than an insertion so the
//! empty-item case — where ⏎ *removes* the marker — takes the same path and
//! the same undo grouping.

use std::sync::Arc;

use crate::contracts::TextEdit;
use crate::model::{BlockContent, MDBlock, ParsedDocument};
use crate::ns_range::NSRange;
use crate::swift_text::{self, CharSet, ns::NSStringExt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListContinuation {
    pub replace_range: NSRange,
    pub insertion: String,
}

impl ListContinuation {
    pub fn new(replace_range: NSRange, insertion: impl Into<String>) -> ListContinuation {
        ListContinuation { replace_range, insertion: insertion.into() }
    }
}

pub struct ListEditing;

/// `String(s.prefix { … })` over Characters.
fn prefix_while(s: &str, mut predicate: impl FnMut(&str) -> bool) -> &str {
    let end = swift_text::first_index_where(s, |g| !predicate(g)).unwrap_or(s.len());
    &s[..end]
}

impl ListEditing {
    /// Result of pressing Return at `offset` inside a list item (§6.4), or
    /// `None` when the caret is not in a list.
    pub fn continuation(doc: &ParsedDocument, offset: isize) -> Option<ListContinuation> {
        let item = Self::enclosing_item(doc, offset)?;
        let marker = item.marker_range?;
        let BlockContent::ListItem { ordinal, checkbox } = &item.content else { return None };

        let text = doc.utf16.as_slice();
        let line_number = doc.line_at(marker.location);
        let line_range = doc.range_of_line(line_number);
        let source = doc.substring(line_range);
        let indent = swift_text::leading_indent(&source);
        let marker_text = text.substring(marker);
        let body = text.substring(NSRange::new(marker.upper_bound(), 0.max(item.range.upper_bound() - marker.upper_bound())));

        // An empty item ends the list: outdent one level if it is nested,
        // otherwise clear the marker and leave a blank line behind.
        if swift_text::trim_whitespaces_and_newlines(&body).is_empty() {
            if indent.is_empty() {
                return Some(ListContinuation::new(line_range, ""));
            }
            let bullet = swift_text::trimming(&marker_text, CharSet::Whitespaces);
            let outdented = swift_text::drop_last(indent, swift_text::count(indent).min(2));
            return Some(ListContinuation::new(line_range, format!("{outdented}{bullet} ")));
        }

        let mut next = indent.to_owned();
        if let Some(ordinal) = ordinal {
            let punctuation = if swift_text::contains_char(&marker_text, ')') { ")" } else { "." };
            next.push_str(&format!("{}{punctuation} ", ordinal + 1));
        } else {
            // `markerRange` excludes the container indentation, so the bullet
            // is just the marker's first non-whitespace character.
            let bullet = swift_text::first(swift_text::trimming(&marker_text, CharSet::Whitespaces)).unwrap_or("-");
            next.push_str(bullet);
            next.push(' ');
        }
        if checkbox.is_some() {
            next.push_str("[ ] ");
        }
        Some(ListContinuation::new(NSRange::new(offset, 0), format!("\n{next}")))
    }

    /// Indents or outdents every list line touched by `line_range`.
    ///
    /// Returns *indentation* edits only: the app applies them, reparses, then
    /// runs `TidyDocument.plan(rules: [.orderedListNumbers])` in the same undo
    /// group to renumber.
    pub fn indent(doc: &ParsedDocument, line_range: NSRange, outdent: bool) -> Vec<TextEdit> {
        let first_line = doc.line_at(0.max(line_range.location));
        let last_line = doc.line_at(line_range.location.max(line_range.upper_bound() - 1));
        if first_line > last_line {
            return Vec::new();
        }

        let mut edits: Vec<TextEdit> = Vec::new();
        for line in first_line..=last_line {
            let range = doc.range_of_line(line);
            let source = doc.substring(range);
            if swift_text::is_blank_line(&source) {
                continue;
            }
            let indent = swift_text::leading_indent(&source);
            let content_offset = range.upper_bound().min(range.location + swift_text::utf16_count(indent));
            if Self::enclosing_item(doc, content_offset).is_none() {
                continue;
            }
            let width = Self::indent_width(swift_text::drop_first(&source, swift_text::count(indent)));

            if outdent {
                if indent.is_empty() {
                    continue;
                }
                let removal = if swift_text::has_suffix(indent, "\t") { 1 } else { width.min(swift_text::count(indent) as isize) };
                if removal <= 0 {
                    continue;
                }
                edits.push(TextEdit::new(
                    NSRange::new(range.location + swift_text::utf16_count(indent) - removal, removal),
                    "",
                    format!("Outdent line {line}"),
                    None,
                ));
            } else {
                edits.push(TextEdit::new(
                    NSRange::new(range.location, 0),
                    swift_text::repeating(" ", width),
                    format!("Indent line {line}"),
                    None,
                ));
            }
        }
        edits
    }

    /// One indent step is the width of the line's own marker, so a nested
    /// item lines up under its parent's text.
    fn indent_width(line: &str) -> isize {
        let digits = prefix_while(line, swift_text::is_number);
        if !digits.is_empty()
            && swift_text::first(swift_text::drop_first(line, swift_text::count(digits)))
                .is_some_and(|g| swift_text::char_is(g, '.') || swift_text::char_is(g, ')'))
        {
            return swift_text::count(digits) as isize + 2;
        }
        if let Some(bullet) = swift_text::first(line)
            && (swift_text::char_is(bullet, '-') || swift_text::char_is(bullet, '*') || swift_text::char_is(bullet, '+'))
        {
            return 2;
        }
        2
    }

    /// Innermost list item at `offset`, matched against the item's *lines*
    /// rather than its range: cmark ends an empty item at its marker, so a
    /// caret after the trailing space of `"  - "` is outside the range but
    /// unmistakably inside the item.
    fn enclosing_item(doc: &ParsedDocument, offset: isize) -> Option<Arc<MDBlock>> {
        let text = doc.utf16.as_slice();
        let mut found: Option<Arc<MDBlock>> = None;
        doc.root.walk_pruning(&mut |candidate| {
            let last_character = candidate.range.location.max(candidate.range.upper_bound() - 1);
            let end = text.line_end_after(last_character);
            let lines = NSRange::new(candidate.range.location, candidate.range.length.max(end - candidate.range.location));
            if !lines.touches(offset) {
                return false;
            }
            if let BlockContent::ListItem { .. } = candidate.content {
                found = Some(candidate.clone());
            }
            true
        });
        found
    }
}
