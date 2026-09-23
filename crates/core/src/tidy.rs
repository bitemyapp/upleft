//! Tidy.swift — Tidy Document (§9.1).
//!
//! Every rule produces `TextEdit`s rather than mutating text, because §9.1
//! shows a rendered diff with per-change accept/reject first. Two invariants:
//!
//! * **Idempotence.** Applying the plan and re-planning produces nothing.
//! * **No overlaps.** Rules that could contend for the same characters skip
//!   ranges owned by another rule, rather than relying on `applied(to:)` to
//!   drop the loser — that would silently break idempotence.

use std::sync::Arc;

use crate::contracts::{TextEdit, TidyRule};
use crate::extensions::fence_language::FenceLanguage;
use crate::model::{BlockContent, MDBlock, ParsedDocument, TableData};
use crate::ns_range::NSRange;
use crate::source_positions::SourceMap;
use crate::swift_text::{self, ns::NSStringExt};
use crate::table_formatter::TableFormatter;

pub struct TidyDocument;

/// `String(s.prefix { … })` over Characters.
fn prefix_while(s: &str, mut predicate: impl FnMut(&str) -> bool) -> &str {
    let end = swift_text::first_index_where(s, |g| !predicate(g)).unwrap_or(s.len());
    &s[..end]
}

impl TidyDocument {
    /// `plan(_:)` with every rule.
    pub fn plan(doc: &ParsedDocument) -> Vec<TextEdit> {
        Self::plan_with(doc, &TidyRule::ALL_CASES)
    }

    /// `plan(_:rules:)`. `rules` is Swift's `Set<TidyRule>`; only membership
    /// is ever asked of it.
    pub fn plan_with(doc: &ParsedDocument, rules: &[TidyRule]) -> Vec<TextEdit> {
        if doc.length <= 0 {
            return Vec::new();
        }
        let context = TidyContext::new(doc, rules);
        let mut edits: Vec<TextEdit> = Vec::new();

        if rules.contains(&TidyRule::HeadingLevels) {
            edits.extend(Self::heading_levels(&context));
        }
        if rules.contains(&TidyRule::TablePipes) {
            edits.extend(Self::table_pipes(&context));
        }
        if rules.contains(&TidyRule::CodeFenceLanguages) {
            edits.extend(Self::code_fence_languages(&context));
        }
        if rules.contains(&TidyRule::OrderedListNumbers) {
            edits.extend(Self::ordered_list_numbers(&context));
        }
        if rules.contains(&TidyRule::ListMarkers) {
            edits.extend(Self::list_markers(&context));
        }
        if rules.contains(&TidyRule::TrailingWhitespace) {
            edits.extend(Self::trailing_whitespace(&context));
        }
        if rules.contains(&TidyRule::BlankLines) {
            edits.extend(Self::blank_lines(&context));
        }

        let mut edits: Vec<TextEdit> =
            edits.into_iter().filter(|edit| edit.range.length > 0 || !edit.replacement.is_empty()).collect();
        // Swift's `sorted(by:)` is stable.
        edits.sort_by_key(|edit| edit.range.location);
        edits
    }

    // MARK: Rule: heading levels

    /// Agents jump H2 → H4 constantly. H1 is never touched — it is the
    /// document title — and a collapsed jump carries down, so H2 → H4 → H5
    /// becomes H2 → H3 → H4 rather than H2 → H3 → H5.
    fn heading_levels(context: &TidyContext) -> Vec<TextEdit> {
        let mut edits: Vec<TextEdit> = Vec::new();
        // (original, corrected)
        let mut stack: Vec<(isize, isize)> = Vec::new();

        for heading in &context.doc.headings {
            while let Some(top) = stack.last()
                && top.0 >= heading.level
            {
                stack.pop();
            }
            let corrected = if heading.level == 1 {
                1
            } else if let Some(parent) = stack.last() {
                heading.level.min(6.min(parent.1 + 1))
            } else {
                heading.level
            };
            stack.push((heading.level, corrected));
            if corrected == heading.level {
                continue;
            }

            // Only ATX headings carry a `#` run to rewrite; a setext heading
            // cannot express a jump in the first place.
            let line = context.doc.range_of_line(context.doc.line_at(heading.range.location));
            let source = context.doc.substring(line);
            let indent = swift_text::leading_indent(&source);
            let hashes = prefix_while(swift_text::drop_first(&source, swift_text::count(indent)), |g| swift_text::char_is(g, '#'));
            let hash_count = swift_text::count(hashes) as isize;
            if hash_count != heading.level {
                continue;
            }

            edits.push(TextEdit::new(
                NSRange::new(line.location + swift_text::utf16_count(indent), hash_count),
                swift_text::repeating("#", corrected),
                format!("H{} → H{}: {}", heading.level, corrected, heading.title),
                Some(TidyRule::HeadingLevels),
            ));
        }
        edits
    }

    // MARK: Rule: table pipes

    fn table_pipes(context: &TidyContext) -> Vec<TextEdit> {
        context
            .tables
            .iter()
            .filter_map(|(block, table)| {
                let range = TableFormatter::source_range(table, block.range);
                let rendered = TableFormatter::render(&TableFormatter::model(table, context.text));
                if rendered.is_empty() || swift_text::str_eq(&rendered, &context.text.substring(range)) {
                    return None;
                }
                Some(TextEdit::new(
                    range,
                    rendered,
                    format!("Align {}-column table", table.column_count()),
                    Some(TidyRule::TablePipes),
                ))
            })
            .collect()
    }

    // MARK: Rule: code fence languages

    fn code_fence_languages(context: &TidyContext) -> Vec<TextEdit> {
        let mut edits: Vec<TextEdit> = Vec::new();
        // `code` below is an `NSString` substring, bridged (Foundation
        // `contains` semantics) exactly when the document is not all ASCII.
        let bridged = !context.map.is_ascii;
        context.doc.root.walk(&mut |block| {
            let BlockContent::CodeBlock { language, is_fenced, content_range } = &block.content else { return };
            if !*is_fenced || language.is_some() {
                return;
            }
            let Some(marker) = block.marker_range else { return };
            let code = context.text.substring(*content_range);
            let Some(guess) = FenceLanguage::guess_bridged(&code, bridged) else { return };

            // Insert straight after the fence characters, leaving the rest of
            // the marker (the newline) alone.
            let fence = context.text.substring(marker);
            let leading = swift_text::leading_indent(&fence);
            let indent = swift_text::utf16_count(leading);
            let ticks = prefix_while(swift_text::drop_first(&fence, swift_text::count(leading)), |g| {
                swift_text::char_is(g, '`') || swift_text::char_is(g, '~')
            });
            let summary = format!("Add language hint `{guess}`");
            edits.push(TextEdit::new(
                NSRange::new(marker.location + indent + swift_text::count(ticks) as isize, 0),
                guess,
                summary,
                Some(TidyRule::CodeFenceLanguages),
            ));
        });
        edits
    }

    // MARK: Rule: ordered list numbers

    fn ordered_list_numbers(context: &TidyContext) -> Vec<TextEdit> {
        let mut edits: Vec<TextEdit> = Vec::new();
        context.doc.root.walk(&mut |block| {
            let BlockContent::List { ordered, start, .. } = block.content else { return };
            if !ordered {
                return;
            }
            for (offset, item) in block.children.iter().enumerate() {
                let BlockContent::ListItem { .. } = item.content else { continue };
                let Some(marker) = item.marker_range else { continue };
                let source = context.text.substring(marker);
                let indent = swift_text::leading_indent(&source);
                let digits = prefix_while(swift_text::drop_first(&source, swift_text::count(indent)), swift_text::is_number);
                if digits.is_empty() {
                    continue;
                }
                let expected = (start + offset as isize).to_string();
                if swift_text::str_eq(digits, &expected) {
                    continue;
                }
                let summary = format!("Renumber list item {digits} → {expected}");
                edits.push(TextEdit::new(
                    // `digits.count`: Characters, as Swift measures it.
                    NSRange::new(marker.location + swift_text::utf16_count(indent), swift_text::count(digits) as isize),
                    expected,
                    summary,
                    Some(TidyRule::OrderedListNumbers),
                ));
            }
        });
        edits
    }

    // MARK: Rule: list markers

    /// Normalises `*` and `+` bullets to `-`. A mixed document is the tell
    /// that two different generators wrote it.
    fn list_markers(context: &TidyContext) -> Vec<TextEdit> {
        let mut edits: Vec<TextEdit> = Vec::new();
        context.doc.root.walk(&mut |block| {
            let BlockContent::List { ordered, .. } = block.content else { return };
            if ordered {
                return;
            }
            for item in &block.children {
                let BlockContent::ListItem { .. } = item.content else { continue };
                let Some(marker) = item.marker_range else { continue };
                let source = context.text.substring(marker);
                let indent = swift_text::leading_indent(&source);
                let Some(bullet) = swift_text::first(swift_text::drop_first(&source, swift_text::count(indent))) else { continue };
                if swift_text::char_is(bullet, '-') || !(swift_text::char_is(bullet, '*') || swift_text::char_is(bullet, '+')) {
                    continue;
                }
                edits.push(TextEdit::new(
                    NSRange::new(marker.location + swift_text::utf16_count(indent), 1),
                    "-",
                    format!("Normalise bullet {bullet} → -"),
                    Some(TidyRule::ListMarkers),
                ));
            }
        });
        edits
    }

    // MARK: Rule: trailing whitespace

    /// Strips trailing spaces and tabs, except an exact two-space run on a
    /// non-empty line, which is a deliberate hard line break (§6.4).
    fn trailing_whitespace(context: &TidyContext) -> Vec<TextEdit> {
        let mut edits: Vec<TextEdit> = Vec::new();
        for line in 0..context.map.line_count() {
            let range = context.map.content_range_of_line(line);
            if !(range.length > 0 && !context.is_protected(range)) {
                continue;
            }
            let source = context.map.string_of_line(line);
            // `source.reversed().prefix { $0 == " " || $0 == "\t" }`
            let trailing: Vec<&str> = swift_text::graphemes(&source)
                .rev()
                .take_while(|g| swift_text::char_is(g, ' ') || swift_text::char_is(g, '\t'))
                .collect();
            if trailing.is_empty() {
                continue;
            }
            // A whitespace-only blank line inside a run `.blankLines` collapses
            // is that rule's region.
            if context.trailing_whitespace_owned_by_blank_lines(line) {
                continue;
            }
            let trailing_count = trailing.len() as isize;
            let kept_break = trailing_count == 2
                && trailing.iter().all(|g| swift_text::char_is(g, ' '))
                && trailing_count < swift_text::count(&source) as isize;
            if kept_break {
                continue;
            }
            edits.push(TextEdit::new(
                NSRange::new(range.upper_bound() - trailing_count, trailing_count),
                "",
                format!("Trim trailing whitespace on line {}", line + 1),
                Some(TidyRule::TrailingWhitespace),
            ));
        }
        edits
    }

    // MARK: Rule: blank lines

    /// Collapses a run of two or more blank lines to one.
    fn blank_lines(context: &TidyContext) -> Vec<TextEdit> {
        let mut edits: Vec<TextEdit> = Vec::new();
        let map = &context.map;
        let mut line: isize = 0;
        while line < map.line_count() {
            if !(swift_text::is_blank_line(&map.string_of_line(line)) && !context.is_protected(map.full_range_of_line(line))) {
                line += 1;
                continue;
            }

            let mut last = line;
            while last + 1 < map.line_count()
                && swift_text::is_blank_line(&map.string_of_line(last + 1))
                && !context.is_protected(map.full_range_of_line(last + 1))
            {
                last += 1;
            }
            // The final "line" of a file ending in a newline is an artefact of
            // the index, not a blank line the user typed.
            let last_is_virtual = last == map.line_count() - 1 && map.content_range_of_line(last).length == 0;
            let effective_last = if last_is_virtual { last - 1 } else { last };

            if effective_last > line {
                let start = map.line_starts[line as usize];
                let end = map.full_range_of_line(effective_last).upper_bound();
                edits.push(TextEdit::new(
                    NSRange::new(start, 0.max(end - start)),
                    "\n",
                    format!("Collapse {} blank lines", effective_last - line + 1),
                    Some(TidyRule::BlankLines),
                ));
            }
            line = last + 1;
        }
        edits
    }
}

// MARK: - Shared analysis

/// Precomputes what every rule needs: the tables, and the ranges other rules
/// must not touch.
pub struct TidyContext<'a> {
    pub doc: &'a ParsedDocument,
    pub map: SourceMap,
    /// `doc.text as NSString`.
    pub text: &'a [u16],
    /// The rules currently being planned, so a rule that would contend with
    /// another can defer to the one that owns the region.
    pub rules: &'a [TidyRule],
    pub tables: Vec<(&'a Arc<MDBlock>, &'a TableData)>,
    /// Code, math, HTML, front matter and table source — regions where
    /// whitespace and blank lines are content, not formatting.
    protected_ranges: Vec<NSRange>,
}

impl<'a> TidyContext<'a> {
    pub fn new(doc: &'a ParsedDocument, rules: &'a [TidyRule]) -> TidyContext<'a> {
        let map = SourceMap::new(&doc.text);
        let mut tables: Vec<(&'a Arc<MDBlock>, &'a TableData)> = Vec::new();
        let mut protected: Vec<NSRange> = Vec::new();
        doc.root.walk(&mut |block| match &block.content {
            BlockContent::Table(table) => {
                tables.push((block, table));
                protected.push(TableFormatter::source_range(table, block.range));
            }
            BlockContent::CodeBlock { .. }
            | BlockContent::Mermaid { .. }
            | BlockContent::MathBlock { .. }
            | BlockContent::HtmlBlock
            | BlockContent::FrontMatter(_) => protected.push(block.range),
            _ => {}
        });
        protected.sort_by_key(|range| range.location);
        TidyContext { doc, map, text: &doc.utf16, rules, tables, protected_ranges: protected }
    }

    pub fn is_protected(&self, range: NSRange) -> bool {
        self.protected_ranges.iter().any(|p| p.location < range.upper_bound() && range.location < p.upper_bound())
    }

    /// True when a whitespace-only line belongs to a run of two or more blank
    /// lines that `.blankLines` will collapse away. A lone blank line is not
    /// owned by `.blankLines` (it is never collapsed), so trailing whitespace
    /// there stays the trailing-whitespace rule's job.
    pub fn trailing_whitespace_owned_by_blank_lines(&self, line: isize) -> bool {
        if !(self.rules.contains(&TidyRule::BlankLines) && swift_text::is_blank_line(&self.map.string_of_line(line))) {
            return false;
        }
        let previous = line - 1;
        let next = line + 1;
        let previous_blank = previous >= 0
            && swift_text::is_blank_line(&self.map.string_of_line(previous))
            && !self.is_protected(self.map.full_range_of_line(previous));
        let next_blank = next < self.map.line_count()
            && swift_text::is_blank_line(&self.map.string_of_line(next))
            && !self.is_protected(self.map.full_range_of_line(next));
        previous_blank || next_blank
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::applied;
    use std::collections::HashMap;

    /// A document with no blocks: the line-based rules (trailing whitespace,
    /// blank lines) see exactly what they would for all-paragraph text.
    fn bare(text: &str) -> ParsedDocument {
        let map = SourceMap::new(text);
        ParsedDocument::new(
            text.to_owned(),
            map.length,
            MDBlock::new(BlockContent::Document, NSRange::new(0, map.length), NSRange::new(0, map.length)).into_ref(),
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            HashMap::new(),
            HashMap::new(),
            map.line_starts.clone(),
        )
    }

    fn tidied(text: &str, rules: &[TidyRule]) -> String {
        applied(&TidyDocument::plan_with(&bare(text), rules), text)
    }

    #[test]
    fn line_rules_on_blockless_text() {
        use TidyRule::*;
        assert_eq!(tidied("a\n\n\n\n\nb\n", &[BlankLines]), "a\n\nb\n");
        assert_eq!(tidied("a\n\nb\n", &[BlankLines]), "a\n\nb\n");
        assert_eq!(tidied("line one  \nline two\n", &[TrailingWhitespace]), "line one  \nline two\n");
        assert_eq!(tidied("line one   \nline two\t\n", &[TrailingWhitespace]), "line one\nline two\n");
        assert_eq!(tidied("   \nx\n", &[TrailingWhitespace]), "\nx\n");
        assert_eq!(tidied("   \nx\n", &[TrailingWhitespace, BlankLines]), "\nx\n");
        let edits = TidyDocument::plan_with(&bare("a\n   \n\t \nb\n"), &[TrailingWhitespace, BlankLines]);
        for pair in edits.windows(2) {
            assert!(pair[0].range.upper_bound() <= pair[1].range.location);
        }
        assert_eq!(applied(&edits, "a\n   \n\t \nb\n"), "a\n\nb\n");
        assert!(TidyDocument::plan(&bare("")).is_empty());
    }

    #[test]
    fn trailing_whitespace_counts_characters() {
        // " \u{301}" is one Character that is not a space, so nothing trails;
        // a CR LF inside a line index line cannot occur, but a lone space
        // after an emoji is one Character and one UTF-16 unit.
        assert_eq!(tidied("a \u{301}\n", &[TidyRule::TrailingWhitespace]), "a \u{301}\n");
        assert_eq!(tidied("😀   \n", &[TidyRule::TrailingWhitespace]), "😀\n");
        // Exactly two spaces after a single Character is still a hard break.
        assert_eq!(tidied("😀  \n", &[TidyRule::TrailingWhitespace]), "😀  \n");
    }
}
