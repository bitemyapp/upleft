//! Inlines.swift — inline span construction.
//!
//! Marker ranges come from the gap between a span's own range and its
//! children's, which gets `*em*`, `**strong**`, `~~strike~~`, `[text](url)`,
//! `![alt](src)` and `<autolink>` right without a table of delimiters.

use upleft_markup::{Markup, MarkupData};

use crate::contracts::ParseOptions;
use crate::extensions::math_scanner::MathScanner;
use crate::extensions::path_token_scanner::PathTokenScanner;
use crate::extensions::wikilink_scanner::WikilinkScanner;
use crate::model::{InlineKind, InlineSpan, ResolvableToken};
use crate::ns_range::{NS_NOT_FOUND, NSRange};
use crate::source_positions::SourceMap;
use crate::swift_text::{self, CharSet, ns::NSStringExt};

/// A Swift `Set<String>`: membership by canonical equivalence.
#[derive(Clone, Debug, Default)]
pub struct StringSet {
    set: std::collections::HashSet<String>,
    has_non_ascii: bool,
}

impl StringSet {
    pub fn insert(&mut self, value: String) {
        if !value.is_ascii() {
            if self.set.iter().any(|existing| swift_text::str_eq(existing, &value)) {
                return;
            }
            self.has_non_ascii = true;
        }
        self.set.insert(value);
    }

    pub fn contains(&self, value: &str) -> bool {
        if self.set.contains(value) {
            return true;
        }
        if value.is_ascii() && !self.has_non_ascii {
            return false;
        }
        self.set.iter().any(|existing| swift_text::str_eq(existing, value))
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }
}

impl FromIterator<String> for StringSet {
    fn from_iter<I: IntoIterator<Item = String>>(iter: I) -> Self {
        let mut set = StringSet::default();
        for value in iter {
            set.insert(value);
        }
        set
    }
}

#[derive(Clone, Copy)]
enum Pass {
    Math,
    Footnote,
    Wikilink,
    Path,
}

pub struct InlineBuilder<'m> {
    pub map: &'m SourceMap,
    pub line_offset: isize,
    pub options: ParseOptions,
    pub run_extensions: bool,
    /// Footnote identifiers found by the block scan.
    pub footnote_identifiers: StringSet,
    pub path_tokens: Vec<ResolvableToken>,
}

impl<'m> InlineBuilder<'m> {
    pub fn new(
        map: &'m SourceMap,
        line_offset: isize,
        options: ParseOptions,
        run_extensions: bool,
        footnote_identifiers: StringSet,
    ) -> InlineBuilder<'m> {
        InlineBuilder { map, line_offset, options, run_extensions, footnote_identifiers, path_tokens: Vec::new() }
    }

    #[inline]
    fn text(&self) -> &'m [u16] {
        self.map.text.as_slice()
    }

    // MARK: Entry point

    /// Inline spans for the children of a leaf text block, covering `bounds`.
    pub fn spans(&mut self, markup: Markup<'_>, bounds: NSRange) -> Vec<InlineSpan> {
        let mut built: Vec<InlineSpan> = markup.children().filter_map(|child| self.span(child)).collect();
        self.fill_breaks(&mut built, bounds);
        self.apply_extensions(built)
    }

    // MARK: swift-markdown → InlineSpan

    fn span(&mut self, markup: Markup<'_>) -> Option<InlineSpan> {
        let range = self.map.range(markup.range(), self.line_offset)?;
        let mut children: Vec<InlineSpan> = markup.children().filter_map(|child| self.span(child)).collect();
        self.fill_breaks(&mut children, range);
        let content = Self::content_range(range, &children);

        match markup.data() {
            MarkupData::Text { .. } => Some(InlineSpan::new(InlineKind::Text, range, range)),
            MarkupData::SoftBreak => Some(InlineSpan::new(InlineKind::SoftBreak, range, range)),
            MarkupData::LineBreak => Some(InlineSpan::new(InlineKind::LineBreak, range, range)),
            MarkupData::InlineHtml { .. } => Some(InlineSpan::new(InlineKind::InlineHTML, range, range)),
            MarkupData::InlineCode { .. } => {
                let ticks = self.backtick_run(range);
                let inner = NSRange::new(range.location + ticks, 0.max(range.length - 2 * ticks));
                let span = InlineSpan::new(InlineKind::InlineCode, range, inner)
                    .with_markers(
                        if ticks > 0 { Some(NSRange::new(range.location, ticks)) } else { None },
                        if ticks > 0 { Some(NSRange::new(inner.upper_bound(), ticks)) } else { None },
                    )
                    .with_children(self.code_span_children(inner));
                Some(span)
            }
            MarkupData::Emphasis => Some(Self::wrapped(InlineKind::Emphasis, range, content, children)),
            MarkupData::Strong => Some(Self::wrapped(InlineKind::Strong, range, content, children)),
            MarkupData::Strikethrough => Some(Self::wrapped(InlineKind::Strikethrough, range, content, children)),
            MarkupData::Image { source, .. } => {
                let alt = markup.plain_text().unwrap_or_default();
                Some(Self::wrapped(
                    InlineKind::Image { source: source.unwrap_or("").to_owned(), alt },
                    range,
                    content,
                    children,
                ))
            }
            MarkupData::Link { destination, title } => Some(self.link_span(destination, title, range, content, children)),
            _ => {
                // Symbol links, custom inlines and inline attributes have no
                // decoration policy of their own; treat them as text.
                if children.is_empty() {
                    return Some(InlineSpan::new(InlineKind::Text, range, range));
                }
                Some(Self::wrapped(InlineKind::Text, range, content, children))
            }
        }
    }

    fn link_span(
        &self,
        destination: Option<&str>,
        title: Option<&str>,
        range: NSRange,
        content: NSRange,
        children: Vec<InlineSpan>,
    ) -> InlineSpan {
        let source = self.text().substring(range);
        if swift_text::has_prefix(&source, "<") && swift_text::has_suffix(&source, ">") {
            return Self::wrapped(InlineKind::Autolink { destination: destination.unwrap_or("").to_owned() }, range, content, children);
        }
        // A footnote reference reaches us as a link whose text is `^id`.
        if swift_text::has_prefix(&source, "[^")
            && let Some(close) = swift_text::first_index_of(&source, ']')
        {
            // `source.index(source.startIndex, offsetBy: 2)`: the two
            // Characters `[` and `^`, which `hasPrefix` just matched.
            let start = source.len() - swift_text::drop_first(&source, 2).len();
            let identifier = if close >= start { &source[start..close] } else { "" };
            if !identifier.is_empty() && self.footnote_identifiers.contains(identifier) {
                return InlineSpan::new(InlineKind::FootnoteReference { identifier: identifier.to_owned() }, range, content)
                    .with_children(children);
            }
        }
        Self::wrapped(
            InlineKind::Link { destination: destination.unwrap_or("").to_owned(), title: title.map(str::to_owned) },
            range,
            content,
            children,
        )
    }

    fn wrapped(kind: InlineKind, range: NSRange, content: NSRange, children: Vec<InlineSpan>) -> InlineSpan {
        let leading = if content.location > range.location {
            Some(NSRange::new(range.location, content.location - range.location))
        } else {
            None
        };
        let trailing = if content.upper_bound() < range.upper_bound() {
            Some(NSRange::new(content.upper_bound(), range.upper_bound() - content.upper_bound()))
        } else {
            None
        };
        InlineSpan::new(kind, range, content).with_markers(leading, trailing).with_children(children)
    }

    fn content_range(range: NSRange, children: &[InlineSpan]) -> NSRange {
        let (Some(first), Some(last)) = (children.first(), children.last()) else { return range };
        let lower = range.location.max(first.range.location);
        let upper = range.upper_bound().min(last.range.upper_bound());
        if !(upper >= lower) {
            return range;
        }
        NSRange::new(lower, upper - lower)
    }

    fn backtick_run(&self, range: NSRange) -> isize {
        let text = self.text();
        let mut count = 0;
        while count < range.length && text.character_at(range.location + count) == 0x60 {
            count += 1;
        }
        count.min(range.length / 2)
    }

    /// Reconstructs `SoftBreak`/`LineBreak` spans from the newline gaps
    /// between siblings (they carry no source range of their own).
    fn fill_breaks(&self, spans: &mut Vec<InlineSpan>, bounds: NSRange) {
        if spans.len() <= 1 {
            return;
        }
        let mut out: Vec<InlineSpan> = Vec::with_capacity(spans.len() * 2);
        let mut cursor = bounds.location;
        let count = spans.len();
        for index in 0..count {
            let span = &spans[index];
            if span.range.location > cursor {
                let gap = NSRange::new(cursor, span.range.location - cursor);
                if let Some(kind) = self.break_kind(gap) {
                    out.push(InlineSpan::new(kind, gap, gap));
                }
            }
            let next_location = if index + 1 < count { spans[index + 1].range.location } else { NS_NOT_FOUND };
            if let Some((break_range, fixed)) = self.reanchored_text_span(span, next_location, bounds) {
                out.push(InlineSpan::new(InlineKind::LineBreak, break_range, break_range));
                cursor = fixed.range.upper_bound();
                out.push(fixed);
                continue;
            }
            cursor = span.range.upper_bound();
            out.push(span.clone());
        }
        *spans = out;
    }

    /// Re-anchors a degenerate zero-length `.text` span onto the line its
    /// content actually lives on.
    fn reanchored_text_span(&self, span: &InlineSpan, next_location: isize, bounds: NSRange) -> Option<(NSRange, InlineSpan)> {
        if !(matches!(span.kind, InlineKind::Text) && span.range.length == 0) {
            return None;
        }
        let text = self.text();
        let anchor = span.range.location;
        if !(anchor >= bounds.location && anchor < bounds.upper_bound() && self.is_newline(anchor)) {
            return None;
        }
        let mut break_start = anchor;
        if anchor > bounds.location && text.character_at(anchor - 1) == 0x5C {
            break_start = anchor - 1;
        }
        let mut break_end = anchor + 1;
        while break_end < bounds.upper_bound() && self.is_newline(break_end) {
            break_end += 1;
        }
        let mut line_end = break_end;
        while line_end < bounds.upper_bound() && !self.is_newline(line_end) {
            line_end += 1;
        }
        let stop = (if next_location > break_end { line_end.min(next_location) } else { line_end }).min(bounds.upper_bound());
        if !(stop > break_end) {
            return None;
        }
        let mut fixed = span.clone();
        fixed.range = NSRange::new(break_end, stop - break_end);
        fixed.content_range = fixed.range;
        Some((NSRange::new(break_start, break_end - break_start), fixed))
    }

    fn is_newline(&self, location: isize) -> bool {
        let unit = self.text().character_at(location);
        unit == 0x0A || unit == 0x0D || unit == 0x2028 || unit == 0x2029
    }

    /// The kind of line break a span gap represents, or `None` if the gap
    /// holds no terminator.
    fn break_kind(&self, gap: NSRange) -> Option<InlineKind> {
        if !(gap.length > 0) {
            return None;
        }
        let text = self.text();
        let units = &text[gap.as_usize_range()];
        // `source.rangeOfCharacter(from: .newlines)`: the first UTF-16 unit
        // in the set.
        let terminator = units.iter().position(|&u| CharSet::Newlines.contains_unit(u))?;
        let offset = gap.location;
        let two_spaces_before = offset >= 2 && text.character_at(offset - 1) == 0x20 && text.character_at(offset - 2) == 0x20;
        let backslash_before = offset >= 1 && text.character_at(offset - 1) == 0x5C;
        let before = swift_text::ns::string_from_utf16(&units[..terminator]);
        if swift_text::has_suffix(&before, "  ") || swift_text::has_suffix(&before, "\\") || two_spaces_before || backslash_before {
            return Some(InlineKind::LineBreak);
        }
        Some(InlineKind::SoftBreak)
    }

    // MARK: Extension passes

    fn code_span_children(&mut self, content: NSRange) -> Vec<InlineSpan> {
        if !(self.run_extensions && self.options.detect_path_tokens && content.length > 0) {
            return Vec::new();
        }
        let Some(found) = PathTokenScanner::code_span_match(self.text(), content) else { return Vec::new() };
        self.path_tokens.push(ResolvableToken::new(found.token.clone(), found.range, true));
        vec![InlineSpan::new(InlineKind::PathToken(found.token), found.range, found.range)]
    }

    /// Runs the text-level extension passes in a fixed order.
    fn apply_extensions(&mut self, spans: Vec<InlineSpan>) -> Vec<InlineSpan> {
        if !self.run_extensions {
            return spans;
        }
        let mut result = spans;
        if self.options.detect_math {
            result = self.split(result, Pass::Math);
        }
        if !self.footnote_identifiers.is_empty() {
            result = self.split(result, Pass::Footnote);
        }
        if self.options.detect_wikilinks {
            result = self.split(result, Pass::Wikilink);
        }
        if self.options.detect_path_tokens {
            result = self.split(result, Pass::Path);
        }
        result
    }

    fn split(&mut self, spans: Vec<InlineSpan>, pass: Pass) -> Vec<InlineSpan> {
        let mut out: Vec<InlineSpan> = Vec::with_capacity(spans.len());
        for mut span in spans {
            if matches!(span.kind, InlineKind::Text) {
                let replacements = self.produce(pass, span.range);
                if replacements.is_empty() {
                    out.push(span);
                } else {
                    out.extend(replacements);
                }
            } else {
                let children = std::mem::take(&mut span.children);
                span.children = self.split(children, pass);
                out.push(span);
            }
        }
        out
    }

    fn produce(&mut self, pass: Pass, range: NSRange) -> Vec<InlineSpan> {
        let text = self.text();
        match pass {
            Pass::Math => {
                let matches = MathScanner::matches_bridged(text, range, Some(!self.map.is_ascii));
                if matches.is_empty() {
                    return Vec::new();
                }
                let ranges: Vec<NSRange> = matches.iter().map(|m| m.range).collect();
                Self::interleave(range, &ranges, |index| {
                    let found = &matches[index];
                    InlineSpan::new(InlineKind::InlineMath { latex_range: found.content_range }, found.range, found.content_range)
                        .with_markers(
                            Some(NSRange::new(found.range.location, found.content_range.location - found.range.location)),
                            Some(NSRange::new(
                                found.content_range.upper_bound(),
                                found.range.upper_bound() - found.content_range.upper_bound(),
                            )),
                        )
                })
            }
            Pass::Footnote => {
                let matches = self.footnote_references(range);
                if matches.is_empty() {
                    return Vec::new();
                }
                let ranges: Vec<NSRange> = matches.iter().map(|m| m.0).collect();
                Self::interleave(range, &ranges, |index| {
                    let (span, identifier) = &matches[index];
                    let inner = NSRange::new(span.location + 1, 0.max(span.length - 2));
                    InlineSpan::new(InlineKind::FootnoteReference { identifier: identifier.clone() }, *span, inner).with_markers(
                        Some(NSRange::new(span.location, 1)),
                        Some(NSRange::new(inner.upper_bound(), 1)),
                    )
                })
            }
            Pass::Wikilink => {
                let matches = WikilinkScanner::matches(text, range);
                if matches.is_empty() {
                    return Vec::new();
                }
                let ranges: Vec<NSRange> = matches.iter().map(|m| m.range).collect();
                Self::interleave(range, &ranges, |index| {
                    let found = &matches[index];
                    let inner = NSRange::new(found.range.location + 2, 0.max(found.range.length - 4));
                    let target_length = found.target_range.length;
                    let has_label = found.label.is_some();
                    let leading_length = if has_label { 2 + target_length + 1 } else { 2 };
                    let content = if has_label {
                        NSRange::new(found.range.location + leading_length, 0.max(found.range.length - leading_length - 2))
                    } else {
                        inner
                    };
                    InlineSpan::new(
                        InlineKind::Wikilink { target: found.target.clone(), label: found.label.clone() },
                        found.range,
                        content,
                    )
                    .with_markers(
                        Some(NSRange::new(found.range.location, leading_length)),
                        Some(NSRange::new(inner.upper_bound(), 2)),
                    )
                })
            }
            Pass::Path => {
                let matches = PathTokenScanner::matches(text, range);
                if matches.is_empty() {
                    return Vec::new();
                }
                for found in &matches {
                    self.path_tokens.push(ResolvableToken::new(found.token.clone(), found.range, false));
                }
                let ranges: Vec<NSRange> = matches.iter().map(|m| m.range).collect();
                Self::interleave(range, &ranges, |index| {
                    let found = &matches[index];
                    InlineSpan::new(InlineKind::PathToken(found.token.clone()), found.range, found.range)
                })
            }
        }
    }

    fn footnote_references(&self, range: NSRange) -> Vec<(NSRange, String)> {
        let text = self.text();
        let mut out = Vec::new();
        let mut i = range.location;
        while i + 3 < range.upper_bound() {
            if !(text.character_at(i) == 0x5B && text.character_at(i + 1) == 0x5E) {
                i += 1;
                continue;
            }
            let mut j = i + 2;
            while j < range.upper_bound() && text.character_at(j) != 0x5D {
                j += 1;
            }
            if !(j < range.upper_bound()) {
                break;
            }
            let identifier = text.substring(NSRange::new(i + 2, j - i - 2));
            if self.footnote_identifiers.contains(&identifier) {
                out.push((NSRange::new(i, j + 1 - i), identifier));
                i = j + 1;
            } else {
                i += 1;
            }
        }
        out
    }

    /// Rebuilds `range` as alternating plain text and matched spans.
    fn interleave(range: NSRange, matched: &[NSRange], mut make: impl FnMut(usize) -> InlineSpan) -> Vec<InlineSpan> {
        let mut out = Vec::with_capacity(matched.len() * 2 + 1);
        let mut cursor = range.location;
        for (index, hit) in matched.iter().enumerate() {
            if hit.location > cursor {
                let gap = NSRange::new(cursor, hit.location - cursor);
                out.push(InlineSpan::new(InlineKind::Text, gap, gap));
            }
            out.push(make(index));
            cursor = hit.upper_bound();
        }
        if cursor < range.upper_bound() {
            let tail = NSRange::new(cursor, range.upper_bound() - cursor);
            out.push(InlineSpan::new(InlineKind::Text, tail, tail));
        }
        out
    }
}

impl InlineSpan {
    /// Drops everything at or before `offset` and clips a span that
    /// straddles it (a callout marker lifted out of the first paragraph).
    pub fn clip_leading(spans: Vec<InlineSpan>, offset: isize) -> Vec<InlineSpan> {
        let mut out = Vec::with_capacity(spans.len());
        for mut span in spans {
            if span.range.upper_bound() <= offset {
                continue;
            }
            if span.range.location < offset {
                let length = span.range.upper_bound() - offset;
                span.range = NSRange::new(offset, length);
                let lower = offset.max(span.content_range.location);
                span.content_range = NSRange::new(lower, 0.max(span.content_range.upper_bound() - lower));
                let children = std::mem::take(&mut span.children);
                span.children = InlineSpan::clip_leading(children, offset);
            }
            out.push(span);
        }
        out
    }
}
