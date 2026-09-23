//! StructuralZoom.swift — structural zoom (§5.2).
//!
//! "The document changes resolution in place." Levels 1–3 keep headings down
//! to a depth; level 4 keeps every heading, the first sentence of each
//! section, and every concrete artifact (code block, table, math, task list).
//!
//! Ranges are whole lines: eliding a partial line would leave the renderer to
//! stitch a fragment back together mid-paragraph.

use crate::contracts::{ZoomLevel, ZoomPlan};
use crate::metrics::{Metrics, SentenceTokenizer};
use crate::model::{BlockContent, InlineKind, InlineSpan, MDBlock, ParsedDocument};
use crate::ns_range::NSRange;
use crate::source_positions::SourceMap;
use crate::swift_text::{self, ns::NSStringExt};

pub struct StructuralZoom;

impl StructuralZoom {
    /// Semantic navigation summary for a section. Markdown markers are
    /// removed, inline math becomes a readable placeholder, and two sentences
    /// provide enough context to judge a jump.
    pub fn section_preview(doc: &ParsedDocument, heading_index: isize) -> Option<String> {
        if !(heading_index >= 0 && (heading_index as usize) < doc.headings.len()) {
            return None;
        }
        let index = heading_index as usize;
        let heading = &doc.headings[index];
        let end = if index + 1 < doc.headings.len() { doc.headings[index + 1].range.location } else { doc.length };
        let start = heading.range.upper_bound();
        if !(end > start) {
            return None;
        }
        let range = NSRange::new(start, end - start);
        let prose = split_where(&Self::preview_prose(doc, range), swift_text::is_whitespace).join(" ");
        if prose.is_empty() {
            return Self::artifact_summary(doc, range);
        }

        let tokenizer = SentenceTokenizer::new();
        let units = swift_text::ns::utf16(&prose);
        let mut sentences: Vec<String> = Vec::new();
        tokenizer.enumerate(&prose, |token| {
            let sentence = units.as_slice().substring(token);
            sentences.push(swift_text::trim_whitespaces_and_newlines(&sentence).to_owned());
            sentences.len() < 2 && swift_text::count(&sentences.join(" ")) < 220
        });
        let summary = sentences.join(" ");
        if swift_text::count(&summary) > 260 {
            Some(format!("{}\u{2026}", swift_text::prefix(&summary, 257)))
        } else {
            Some(summary)
        }
    }

    fn artifact_summary(doc: &ParsedDocument, range: NSRange) -> Option<String> {
        let mut code_languages: Vec<String> = Vec::new();
        let mut code_count = 0isize;
        let mut table_count = 0isize;
        let mut math_count = 0isize;
        let mut diagram_count = 0isize;
        let mut task_list_count = 0isize;
        doc.root.walk_pruning(&mut |block| {
            if !(block.range.upper_bound() > range.location && block.range.location < range.upper_bound()) {
                return false;
            }
            match &block.content {
                BlockContent::Document | BlockContent::BlockQuote | BlockContent::Callout { .. } | BlockContent::ListItem { .. } => {
                    true
                }
                BlockContent::List { .. } => {
                    if Self::contains_task(block) {
                        task_list_count += 1;
                        return false;
                    }
                    true
                }
                BlockContent::CodeBlock { language, .. } => {
                    code_count += 1;
                    if let Some(language) = language
                        && !language.is_empty()
                        && !code_languages.iter().any(|known| swift_text::case_insensitive_equal(known, language))
                    {
                        code_languages.push(swift_text::capitalized(language));
                    }
                    false
                }
                BlockContent::Table(_) => {
                    table_count += 1;
                    false
                }
                BlockContent::MathBlock { .. } => {
                    math_count += 1;
                    false
                }
                BlockContent::Mermaid { .. } => {
                    diagram_count += 1;
                    false
                }
                _ => false,
            }
        });

        let mut parts: Vec<String> = Vec::new();
        if code_count > 0 {
            let languages = code_languages.iter().take(3).map(String::as_str).collect::<Vec<_>>().join(", ");
            parts.push(
                format!("{} code {}", code_count, if code_count == 1 { "block" } else { "blocks" })
                    + &(if languages.is_empty() { String::new() } else { format!(" \u{B7} {languages}") }),
            );
        }
        if table_count > 0 {
            parts.push(format!("{} {}", table_count, if table_count == 1 { "table" } else { "tables" }));
        }
        if math_count > 0 {
            parts.push(format!("{} math {}", math_count, if math_count == 1 { "block" } else { "blocks" }));
        }
        if diagram_count > 0 {
            parts.push(format!("{} {}", diagram_count, if diagram_count == 1 { "diagram" } else { "diagrams" }));
        }
        if task_list_count > 0 {
            parts.push(format!("{} task {}", task_list_count, if task_list_count == 1 { "list" } else { "lists" }));
        }
        if parts.is_empty() { None } else { Some(parts.join(" \u{B7} ")) }
    }

    fn preview_prose(doc: &ParsedDocument, range: NSRange) -> String {
        let source = doc.utf16.as_slice();
        let mut pieces: Vec<String> = Vec::new();
        doc.root.walk_pruning(&mut |block| {
            if !(block.range.upper_bound() > range.location && block.range.location < range.upper_bound()) {
                return false;
            }
            match block.content {
                BlockContent::Document
                | BlockContent::BlockQuote
                | BlockContent::Callout { .. }
                | BlockContent::List { .. }
                | BlockContent::ListItem { .. } => true,
                BlockContent::Paragraph => {
                    let text = Self::preview_text(&block.inlines, source);
                    if !text.is_empty() {
                        pieces.push(text);
                    }
                    false
                }
                _ => false,
            }
        });
        pieces.join(" ")
    }

    fn preview_text(spans: &[InlineSpan], source: &[u16]) -> String {
        let mut out = String::new();
        for span in spans {
            match &span.kind {
                InlineKind::Text | InlineKind::PathToken(_) => out.push_str(&source.substring(span.range)),
                InlineKind::InlineCode => out.push_str(&source.substring(span.content_range)),
                InlineKind::SoftBreak | InlineKind::LineBreak => out.push(' '),
                InlineKind::InlineMath { .. } => out.push_str("a formula"),
                InlineKind::Wikilink { target, label } => out.push_str(label.as_deref().unwrap_or(target)),
                InlineKind::Image { alt, .. } => out.push_str(if alt.is_empty() { "image" } else { alt }),
                InlineKind::FootnoteReference { .. } | InlineKind::InlineHTML => {}
                InlineKind::Autolink { destination } => out.push_str(destination),
                _ => out.push_str(&Self::preview_text(&span.children, source)),
            }
        }
        out
    }

    pub fn plan(doc: &ParsedDocument, level: ZoomLevel) -> ZoomPlan {
        if !(level != ZoomLevel::Everything && doc.length > 0) {
            return ZoomPlan::all();
        }
        let map = SourceMap::new(&doc.text);
        let mut keep: Vec<NSRange> = Vec::new();

        for heading in &doc.headings {
            if heading.level <= level.max_heading_level() {
                keep.push(Self::lines(heading.range, &map));
            }
        }

        if level == ZoomLevel::Skeleton {
            keep.extend(Self::skeleton_extras(doc, &map));
        }
        if let Some(front) = &doc.front_matter {
            keep.push(Self::lines(front.range, &map));
        }

        let visible = Self::normalise(&keep, map.length);
        let elided = Self::complement(&visible, map.length);
        ZoomPlan::new(visible, elided)
    }

    /// Level 4's additions: the first sentence of each section, and every
    /// code block, table, math block, mermaid diagram and task list in full.
    fn skeleton_extras(doc: &ParsedDocument, map: &SourceMap) -> Vec<NSRange> {
        let mut keep: Vec<NSRange> = Vec::new();
        // Bridged and built once, then reused for every section below.
        let text = doc.utf16.as_slice();
        let tokenizer = SentenceTokenizer::new();

        doc.root.walk_pruning(&mut |block| match block.content {
            BlockContent::CodeBlock { .. } | BlockContent::Table(_) | BlockContent::Mermaid { .. } | BlockContent::MathBlock { .. } => {
                keep.push(Self::lines(block.range, map));
                false
            }
            BlockContent::List { .. } => {
                if Self::contains_task(block) {
                    keep.push(Self::lines(block.range, map));
                    return false;
                }
                true
            }
            BlockContent::Document | BlockContent::BlockQuote | BlockContent::Callout { .. } | BlockContent::ListItem { .. } => true,
            _ => false,
        });

        // One sentence per section, taken from the section's own prose so a
        // code block immediately under a heading is never mistaken for one.
        for (index, heading) in doc.headings.iter().enumerate() {
            let end = if index + 1 < doc.headings.len() { doc.headings[index + 1].range.location } else { doc.length };
            let start = heading.range.upper_bound();
            if !(end > start) {
                continue;
            }
            let body = NSRange::new(start, end - start);
            if let Some(sentence) = Metrics::first_sentence_range_with(doc, body, text, &tokenizer) {
                keep.push(Self::lines(sentence, map));
            }
        }

        // A document that opens without a heading still deserves its lede.
        if let Some(first) = doc.headings.first() {
            if first.range.location > 0 {
                let lede = NSRange::new(0, first.range.location);
                if let Some(sentence) = Metrics::first_sentence_range_with(doc, lede, text, &tokenizer) {
                    keep.push(Self::lines(sentence, map));
                }
            }
        } else {
            let all = NSRange::new(0, doc.length);
            if let Some(sentence) = Metrics::first_sentence_range_with(doc, all, text, &tokenizer) {
                keep.push(Self::lines(sentence, map));
            }
        }
        keep
    }

    fn contains_task(list: &MDBlock) -> bool {
        list.children.iter().any(|child| matches!(child.content, BlockContent::ListItem { checkbox: Some(_), .. }))
    }

    /// Expands a range to whole lines, terminators included.
    fn lines(range: NSRange, map: &SourceMap) -> NSRange {
        let first = map.line_containing(range.location);
        let last = map.line_containing(range.location.max(range.upper_bound() - 1));
        let start = map.line_starts[first as usize];
        let end = map.full_range_of_line(last).upper_bound();
        NSRange::new(start, 0.max(end - start))
    }

    /// Ascending, non-overlapping, clamped — the contract `ZoomPlan` states.
    fn normalise(ranges: &[NSRange], limit: isize) -> Vec<NSRange> {
        let mut sorted: Vec<NSRange> = ranges
            .iter()
            .filter(|r| r.length > 0 && r.location < limit)
            .map(|r| NSRange::new(r.location, r.length.min(limit - r.location)))
            .collect();
        // Swift's `sorted(by:)` is stable, as is `sort_by`.
        sorted.sort_by_key(|r| r.location);
        let Some(&first) = sorted.first() else {
            return Vec::new();
        };
        let mut current = first;
        let mut out: Vec<NSRange> = Vec::new();
        for &range in &sorted[1..] {
            if range.location <= current.upper_bound() {
                current = current.union(range);
            } else {
                out.push(current);
                current = range;
            }
        }
        out.push(current);
        out
    }

    fn complement(visible: &[NSRange], limit: isize) -> Vec<NSRange> {
        let mut out: Vec<NSRange> = Vec::new();
        let mut cursor = 0isize;
        for range in visible {
            if range.location > cursor {
                out.push(NSRange::new(cursor, range.location - cursor));
            }
            cursor = cursor.max(range.upper_bound());
        }
        if cursor < limit {
            out.push(NSRange::new(cursor, limit - cursor));
        }
        out
    }
}

/// `s.split(whereSeparator:)` over Characters with Swift's defaults
/// (unlimited, omitting empty subsequences).
fn split_where(s: &str, mut is_separator: impl FnMut(&str) -> bool) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    for g in swift_text::graphemes(s) {
        if is_separator(g) {
            if start != offset {
                out.push(&s[start..offset]);
            }
            start = offset + g.len();
        }
        offset += g.len();
    }
    if start != s.len() {
        out.push(&s[start..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_where_matches_swift() {
        // `"a \r\n b\u{2028}c  ".split(whereSeparator: \.isWhitespace)`
        assert_eq!(split_where(" a \r\n b\u{2028}c  ", swift_text::is_whitespace), vec!["a", "b", "c"]);
        // A combining mark after a space joins the space's Character, which
        // is still whitespace (its first scalar is).
        assert_eq!(split_where("a \u{301}b", swift_text::is_whitespace), vec!["a", "b"]);
        assert!(split_where("   ", swift_text::is_whitespace).is_empty());
    }
}
