//! Parser.swift — `file bytes → swift-markdown parse → extension pass →
//! extended AST`.
//!
//! Front matter is stripped before cmark runs and the body is parsed on its
//! own, with line numbers shifted back. `.disableSmartOpts` is mandatory: the
//! bytes are the truth.
//!
//! Blocks are built as `Arc<MDBlock>` and mutated in place (callout trimming,
//! footnote retagging, identities, hashes) while the builder still owns them
//! uniquely, which is where Swift mutates its class instances too.

use std::collections::HashMap;
use std::sync::Arc;

use upleft_markup::{ColumnAlignment, Document, Markup, MarkupData, ParseOptions as MarkupParseOptions};

use crate::contracts::ParseOptions;
use crate::derived::{DerivedStructures, FootnoteDefinition, SourceScanner};
use crate::extensions::callout_scanner::CalloutScanner;
use crate::extensions::fence_language::{FenceLanguage, FenceLanguageKind};
use crate::extensions::front_matter_scanner::FrontMatterScanner;
use crate::extensions::math_scanner::MathScanner;
use crate::hashing::FNV;
use crate::inlines::{InlineBuilder, StringSet};
use crate::model::{
    BlockContent, BlockIdentity, BlockRef, Checkbox, FrontMatter, InlineSpan, ListMarkerStyle, MDBlock, ParsedDocument,
    TableAlignment, TableCell, TableData, TableRow,
};
use crate::ns_range::NSRange;
use crate::safe_html::{SafeHTMLDocument, SafeHTMLParser};
use crate::source_positions::SourceMap;
use crate::swift_text::{self, ns::NSStringExt};

pub struct MarkdownParser;

impl MarkdownParser {
    /// `MarkdownParser.parse(_:)`.
    pub fn parse(text: &str) -> Arc<ParsedDocument> {
        Self::parse_with(text, ParseOptions::DEFAULT)
    }

    /// `MarkdownParser.parse(_:options:)`.
    pub fn parse_with(text: &str, options: ParseOptions) -> Arc<ParsedDocument> {
        let map = SourceMap::new(text);
        if !(map.length > 0) {
            return ParsedDocument::empty();
        }
        let run_extensions = map.length <= options.extension_pass_limit;

        let front_matter = if options.detect_front_matter { FrontMatterScanner::scan(&map) } else { None };
        let body_start = front_matter.as_ref().map_or(0, |front| front.range.upper_bound());
        let line_offset = if front_matter.is_none() { 0 } else { map.line_containing(body_start) };
        let body_owned;
        let body: &str = if body_start == 0 {
            text
        } else {
            body_owned = map.text.as_slice().substring_from(body_start);
            &body_owned
        };

        let document = Document::parse(body, MarkupParseOptions::DISABLE_SMART_OPTS);
        let scan = SourceScanner::new(&map);

        let footnote_identifiers: StringSet = scan.footnote_definitions.iter().map(|d| d.identifier.clone()).collect();
        let mut builder = BlockBuilder::new(&map, line_offset, options, run_extensions, footnote_identifiers);

        let mut children: Vec<BlockRef> =
            document.children().filter_map(|child| builder.block(child, 1, 0)).map(Arc::new).collect();
        if let Some(front) = &front_matter {
            children.insert(0, Arc::new(builder.front_matter_block(front)));
        }
        builder.attach_footnote_definitions(&mut children, &scan.footnote_definitions);

        let mut root = Arc::new(
            MDBlock::new(BlockContent::Document, NSRange::new(0, map.length), NSRange::new(0, map.length))
                .with_children(children)
                .with_depth(0),
        );
        BlockIdentifier::assign(&mut root);
        SubtreeHasher::hash(&mut root, &map.text);

        let derived = DerivedStructures::new(&root, &map);
        let path_tokens = std::mem::take(&mut builder.inlines.path_tokens);
        drop(builder);
        let length = map.length;
        let line_starts = map.line_starts.clone();
        let utf16 = map.text;
        Arc::new(ParsedDocument::with_utf16(
            text.to_owned(),
            utf16,
            length,
            root,
            front_matter,
            derived.headings,
            derived.tasks,
            path_tokens,
            derived.footnotes,
            scan.link_references,
            line_starts,
        ))
    }
}

// MARK: - Block construction

pub struct BlockBuilder<'m> {
    map: &'m SourceMap,
    line_offset: isize,
    options: ParseOptions,
    run_extensions: bool,
    pub inlines: InlineBuilder<'m>,
}

impl<'m> BlockBuilder<'m> {
    pub fn new(
        map: &'m SourceMap,
        line_offset: isize,
        options: ParseOptions,
        run_extensions: bool,
        footnote_identifiers: StringSet,
    ) -> BlockBuilder<'m> {
        BlockBuilder {
            map,
            line_offset,
            options,
            run_extensions,
            inlines: InlineBuilder::new(map, line_offset, options, run_extensions, footnote_identifiers),
        }
    }

    #[inline]
    fn text(&self) -> &'m [u16] {
        self.map.text.as_slice()
    }

    #[inline]
    fn range_of(&self, markup: Markup<'_>) -> Option<NSRange> {
        self.map.range(markup.range(), self.line_offset)
    }

    /// Source-addressed HTML annotations for `range`, or `None` when the
    /// document holds no `<` at all.
    fn safe_html(&self, range: NSRange) -> Option<SafeHTMLDocument> {
        if !self.map.may_contain_html {
            return None;
        }
        SafeHTMLParser::parse_ns(self.text(), Some(range))
    }

    pub fn block(&mut self, markup: Markup<'_>, depth: isize, quote_depth: isize) -> Option<MDBlock> {
        let range = self.range_of(markup)?;

        match markup.data() {
            MarkupData::Heading { level } => Some(self.heading_block(markup, level as isize, range, depth, quote_depth)),
            MarkupData::Paragraph => Some(self.paragraph_block(markup, range, depth, quote_depth)),
            MarkupData::BlockQuote => Some(self.quote_block(markup, range, depth, quote_depth)),
            MarkupData::UnorderedList => Some(self.list_block(markup, false, 1, range, depth, quote_depth)),
            MarkupData::OrderedList { start_index } => {
                // `Int(list.startIndex)` traps above Int.max.
                let start = isize::try_from(start_index).expect("Int(UInt) overflow");
                Some(self.list_block(markup, true, start, range, depth, quote_depth))
            }
            MarkupData::ListItem { checkbox } => Some(self.list_item_block(markup, checkbox.is_some(), range, depth, quote_depth)),
            MarkupData::CodeBlock { language, .. } => Some(self.code_block(language, range, depth, quote_depth)),
            MarkupData::Table { column_alignments } => Some(self.table_block(markup, column_alignments, range, depth, quote_depth)),
            MarkupData::ThematicBreak => Some(
                MDBlock::new(BlockContent::ThematicBreak, range, NSRange::new(range.upper_bound(), 0))
                    .with_marker_range(Some(range))
                    .with_depth(depth)
                    .with_quote_depth(quote_depth),
            ),
            MarkupData::HtmlBlock { .. } => {
                let mut block = MDBlock::new(BlockContent::HtmlBlock, range, range).with_depth(depth).with_quote_depth(quote_depth);
                block.safe_html = self.safe_html(range);
                Some(block)
            }
            _ => {
                // Block directives, doxygen commands and custom blocks: keep
                // their characters covered by *something*.
                let children: Vec<BlockRef> =
                    markup.children().filter_map(|child| self.block(child, depth + 1, quote_depth)).map(Arc::new).collect();
                if children.is_empty() {
                    let inlines = self.inlines.spans(markup, range);
                    return Some(
                        MDBlock::new(BlockContent::Paragraph, range, range)
                            .with_inlines(inlines)
                            .with_depth(depth)
                            .with_quote_depth(quote_depth),
                    );
                }
                Some(
                    MDBlock::new(BlockContent::BlockQuote, range, range)
                        .with_children(children)
                        .with_depth(depth)
                        .with_quote_depth(quote_depth),
                )
            }
        }
    }

    // MARK: Leaf text blocks

    fn heading_block(&mut self, heading: Markup<'_>, level: isize, range: NSRange, depth: isize, quote_depth: isize) -> MDBlock {
        let mut range = range;
        let child_ranges: Vec<NSRange> = heading.children().filter_map(|child| self.range_of(child)).collect();
        let first_child = child_ranges.first().copied();
        let last_child = child_ranges.last().copied();

        let start_line = self.map.line_containing(range.location);
        let end_line = self.map.line_containing(range.location.max(range.upper_bound() - 1));
        let is_setext = end_line > start_line;

        if is_setext {
            let underline = self.map.content_range_of_line(end_line);
            let content = NSRange::new(
                range.location,
                0.max(last_child.map_or(underline.location, |last| last.upper_bound()) - range.location),
            );
            let inlines = self.inlines.spans(heading, content);
            return MDBlock::new(BlockContent::Heading { level }, range, content)
                .with_trailing_marker_range(Some(underline))
                .with_inlines(inlines)
                .with_depth(depth)
                .with_quote_depth(quote_depth);
        }

        // cmark ends an ATX heading's range before a closing `#` sequence;
        // extend to the end of the line so the closing run is covered.
        let line_end = self.map.content_range_of_line(start_line).upper_bound();
        let mut trailing: Option<NSRange> = None;
        if line_end > range.upper_bound() {
            trailing = Some(NSRange::new(range.upper_bound(), line_end - range.upper_bound()));
            range = NSRange::new(range.location, line_end - range.location);
        }
        let marker = first_child.map_or(range, |first| NSRange::new(range.location, 0.max(first.location - range.location)));
        let content = match first_child {
            Some(first) => {
                let upper = last_child.map_or(first.upper_bound(), |last| last.upper_bound());
                NSRange::new(first.location, 0.max(upper - first.location))
            }
            None => NSRange::new(range.upper_bound(), 0),
        };

        let inlines = self.inlines.spans(heading, content);
        MDBlock::new(BlockContent::Heading { level }, range, content)
            .with_marker_range(if marker.length > 0 { Some(marker) } else { None })
            .with_trailing_marker_range(trailing)
            .with_inlines(inlines)
            .with_depth(depth)
            .with_quote_depth(quote_depth)
    }

    fn paragraph_block(&mut self, markup: Markup<'_>, range: NSRange, depth: isize, quote_depth: isize) -> MDBlock {
        // A paragraph that is nothing but `$$…$$` or `\[…\]` is a display
        // formula, not prose (§4.1).
        if self.run_extensions
            && self.options.detect_math
            && let Some(block) = MathScanner::whole_block_bridged(self.text(), range, Some(!self.map.is_ascii))
        {
            return MDBlock::new(BlockContent::MathBlock { latex_range: block.content_range }, range, block.content_range)
                .with_marker_range(Some(NSRange::new(block.range.location, block.content_range.location - block.range.location)))
                .with_trailing_marker_range(Some(NSRange::new(
                    block.content_range.upper_bound(),
                    block.range.upper_bound() - block.content_range.upper_bound(),
                )))
                .with_depth(depth)
                .with_quote_depth(quote_depth);
        }
        let inlines = self.inlines.spans(markup, range);
        let mut block = MDBlock::new(BlockContent::Paragraph, range, range)
            .with_inlines(inlines)
            .with_depth(depth)
            .with_quote_depth(quote_depth);
        block.safe_html = self.safe_html(range);
        block
    }

    // MARK: Containers

    fn quote_block(&mut self, quote: Markup<'_>, range: NSRange, depth: isize, quote_depth: isize) -> MDBlock {
        let mut children: Vec<BlockRef> =
            quote.children().filter_map(|child| self.block(child, depth + 1, quote_depth + 1)).map(Arc::new).collect();
        let first_child = children.first().map(|child| child.range);
        let mut marker = first_child.map(|first| NSRange::new(range.location, 0.max(first.location - range.location)));

        let mut content = BlockContent::BlockQuote;
        if self.run_extensions
            && self.options.detect_callouts
            && let Some(callout) = CalloutScanner::scan(self.map, range)
        {
            content = BlockContent::Callout { kind: callout.kind, title: callout.title };
            marker = Some(callout.marker_range);
            Self::trim_leading(&mut children, callout.marker_range.upper_bound());
        }

        let content_start = children.first().map_or(range.upper_bound(), |child| child.range.location);
        MDBlock::new(content, range, NSRange::new(content_start, 0.max(range.upper_bound() - content_start)))
            .with_marker_range(marker.filter(|&m| m.length > 0))
            .with_children(children)
            .with_depth(depth)
            .with_quote_depth(quote_depth)
    }

    /// Moves a callout's first child past the `> [!NOTE] Title` marker.
    fn trim_leading(children: &mut Vec<BlockRef>, offset: isize) {
        let Some(first) = children.first_mut() else { return };
        if !(first.range.location < offset) {
            return;
        }
        if first.range.upper_bound() <= offset {
            children.remove(0);
            return;
        }
        let first = Arc::get_mut(first).expect("block uniquely owned while building");
        first.range = NSRange::new(offset, first.range.upper_bound() - offset);
        let lower = offset.max(first.content_range.location);
        first.content_range = NSRange::new(lower, 0.max(first.content_range.upper_bound() - lower));
        let inlines = std::mem::take(&mut first.inlines);
        first.inlines = InlineSpan::clip_leading(inlines, offset);
    }

    fn list_block(
        &mut self,
        list: Markup<'_>,
        ordered: bool,
        start: isize,
        range: NSRange,
        depth: isize,
        quote_depth: isize,
    ) -> MDBlock {
        let children: Vec<BlockRef> =
            list.children().filter_map(|child| self.block(child, depth + 1, quote_depth)).map(Arc::new).collect();
        let marker = self.marker_style(range.location, ordered);
        let tight = self.is_tight(&children);
        MDBlock::new(BlockContent::List { ordered, start, tight, marker }, range, range)
            .with_children(children)
            .with_depth(depth)
            .with_quote_depth(quote_depth)
    }

    fn list_item_block(&mut self, item: Markup<'_>, has_checkbox: bool, range: NSRange, depth: isize, quote_depth: isize) -> MDBlock {
        let children: Vec<BlockRef> =
            item.children().filter_map(|child| self.block(child, depth + 1, quote_depth)).map(Arc::new).collect();
        // The marker is everything between the item's start and its content.
        let content_start = children.first().map_or(range.upper_bound(), |child| child.range.location);
        let marker = NSRange::new(range.location, 0.max(content_start - range.location));

        let mut checkbox: Option<Checkbox> = None;
        if has_checkbox && let Some(mark_range) = self.checkbox_mark_range(marker) {
            let mark = self.text().substring(mark_range);
            checkbox = Some(Checkbox::new(!swift_text::trim_whitespaces(&mark).is_empty(), mark_range));
        }
        MDBlock::new(
            BlockContent::ListItem { ordinal: self.ordinal(range.location), checkbox },
            range,
            NSRange::new(content_start, 0.max(range.upper_bound() - content_start)),
        )
        .with_marker_range(if marker.length > 0 { Some(marker) } else { None })
        .with_children(children)
        .with_depth(depth)
        .with_quote_depth(quote_depth)
    }

    /// A list is tight when no blank line separates its items and no item
    /// holds more than one block.
    fn is_tight(&self, children: &[BlockRef]) -> bool {
        let text = self.text();
        for index in 1..children.len() {
            let start = children[index - 1].range.location;
            let span = NSRange::new(start, 0.max(children[index].range.location - start));
            let mut end = span.upper_bound();
            let mut terminators = 0;
            while terminators < 2 {
                if end >= 2 && text.character_at(end - 2) == 0x0D && text.character_at(end - 1) == 0x0A {
                    end -= 2;
                } else if end >= 1 && (text.character_at(end - 1) == 0x0A || text.character_at(end - 1) == 0x0D) {
                    end -= 1;
                } else {
                    break;
                }
                terminators += 1;
            }
            if terminators >= 2 {
                return false;
            }
        }
        if children.iter().any(|child| child.children.len() > 1) {
            return false;
        }
        true
    }

    fn marker_style(&self, offset: isize, ordered: bool) -> ListMarkerStyle {
        let text = self.text();
        let mut i = offset;
        while i < text.length() {
            let ch = text.character_at(i);
            if ordered {
                if ch == 0x2E {
                    return ListMarkerStyle::Period;
                }
                if ch == 0x29 {
                    return ListMarkerStyle::Paren;
                }
                if !(0x30..=0x39).contains(&ch) {
                    break;
                }
            } else {
                match ch {
                    0x2D => return ListMarkerStyle::Dash,
                    0x2A => return ListMarkerStyle::Asterisk,
                    0x2B => return ListMarkerStyle::Plus,
                    _ => {}
                }
                if ch != 0x20 && ch != 0x09 {
                    break;
                }
            }
            i += 1;
        }
        if ordered { ListMarkerStyle::Period } else { ListMarkerStyle::Dash }
    }

    fn ordinal(&self, offset: isize) -> Option<isize> {
        let text = self.text();
        let mut i = offset;
        let mut value: isize = 0;
        let mut saw_digit = false;
        while i < text.length() {
            let ch = text.character_at(i);
            if !(0x30..=0x39).contains(&ch) {
                break;
            }
            saw_digit = true;
            let digit = (ch - 0x30) as isize;
            if value > (isize::MAX - digit) / 10 {
                return None;
            }
            value = value * 10 + digit;
            i += 1;
        }
        if saw_digit { Some(value) } else { None }
    }

    /// Range of the single character between the brackets of `- [x] `.
    fn checkbox_mark_range(&self, marker: NSRange) -> Option<NSRange> {
        let text = self.text();
        let mut i = marker.location;
        while i + 2 < marker.upper_bound() {
            if text.character_at(i) == 0x5B && text.character_at(i + 2) == 0x5D {
                return Some(NSRange::new(i + 1, 1));
            }
            i += 1;
        }
        None
    }

    // MARK: Code, math and diagrams

    fn code_block(&mut self, language: Option<&str>, range: NSRange, depth: isize, quote_depth: isize) -> MDBlock {
        let map = self.map;
        let start_line = map.line_containing(range.location);
        let first_line = map.string_of_line(start_line);
        let fence_characters = Self::fence_prefix(&first_line);
        let is_fenced = swift_text::has_prefix(fence_characters, "```") || swift_text::has_prefix(fence_characters, "~~~");

        let mut content = range;
        let mut marker: Option<NSRange> = None;
        let mut trailing: Option<NSRange> = None;
        if is_fenced {
            let open_end = map.full_range_of_line(start_line).upper_bound();
            marker = Some(NSRange::new(range.location, 0.max(open_end - range.location)));
            let end_line = map.line_containing(range.location.max(range.upper_bound() - 1));
            let close_start = if end_line > start_line { map.line_starts[end_line as usize] } else { range.upper_bound() };
            let fence_char = if swift_text::has_prefix(fence_characters, "~~~") { '~' } else { '`' };
            let open_length = Self::prefix_run_count(fence_characters, fence_char);
            let close_line_string = map.string_of_line(end_line);
            let close_line = Self::fence_prefix(&close_line_string);
            let close_run = Self::prefix_run_count(close_line, fence_char);
            let close_tail = swift_text::trim_whitespaces(swift_text::drop_first(close_line, close_run));
            let has_closing_fence = end_line > start_line && close_run >= 3.max(open_length) && close_tail.is_empty();
            let content_end = if has_closing_fence { close_start } else { range.upper_bound() };
            content = NSRange::new(open_end, 0.max(content_end - open_end));
            if has_closing_fence {
                trailing = Some(NSRange::new(content_end, 0.max(range.upper_bound() - content_end)));
            }
        }

        let language: Option<String> = language.map(|l| swift_text::trim_whitespaces(l).to_owned());
        let kind = FenceLanguage::kind(language.as_deref());
        if kind == FenceLanguageKind::Mermaid && self.run_extensions && self.options.detect_mermaid {
            return MDBlock::new(BlockContent::Mermaid { source_range: content }, range, content)
                .with_marker_range(marker)
                .with_trailing_marker_range(trailing)
                .with_depth(depth)
                .with_quote_depth(quote_depth);
        }
        if kind == FenceLanguageKind::Math && self.run_extensions && self.options.detect_math {
            return MDBlock::new(BlockContent::MathBlock { latex_range: content }, range, content)
                .with_marker_range(marker)
                .with_trailing_marker_range(trailing)
                .with_depth(depth)
                .with_quote_depth(quote_depth);
        }
        MDBlock::new(
            BlockContent::CodeBlock { language: language.filter(|l| !l.is_empty()), is_fenced, content_range: content },
            range,
            content,
        )
        .with_marker_range(marker)
        .with_trailing_marker_range(trailing)
        .with_depth(depth)
        .with_quote_depth(quote_depth)
    }

    /// `s.prefix { $0 == ch }.count`: leading Characters equal to `ch`.
    fn prefix_run_count(s: &str, ch: char) -> usize {
        swift_text::graphemes(s).take_while(|g| swift_text::char_is(g, ch)).count()
    }

    /// The fence-visible portion of a code line: leading whitespace and any
    /// blockquote `>` markers stripped.
    fn fence_prefix(line: &str) -> &str {
        let start = swift_text::first_index_where(line, |c| !(c == " " || c == "\t" || c == ">")).unwrap_or(line.len());
        &line[start..]
    }

    // MARK: Tables

    fn table_block(
        &mut self,
        table: Markup<'_>,
        column_alignments: &[Option<ColumnAlignment>],
        range: NSRange,
        depth: isize,
        quote_depth: isize,
    ) -> MDBlock {
        let mut rows: Vec<TableRow> = Vec::new();
        let mut head_end = range.location;

        for child in table.children() {
            match child.data() {
                MarkupData::TableHead => {
                    let Some(head_range) = self.range_of(child) else { continue };
                    head_end = head_range.upper_bound();
                    let cells = self.cells(child);
                    rows.push(TableRow::new(head_range, cells, true));
                }
                MarkupData::TableBody => {
                    for row in child.children() {
                        if !matches!(row.data(), MarkupData::TableRow) {
                            continue;
                        }
                        let Some(row_range) = self.range_of(row) else { continue };
                        let cells = self.cells(row);
                        rows.push(TableRow::new(row_range, cells, false));
                    }
                }
                _ => continue,
            }
        }

        // The delimiter row is alignment metadata to cmark; recover its range
        // from the line after the header.
        let delimiter_line = self.map.line_containing(range.location.max(head_end - 1)) + 1;
        let delimiter_range = if delimiter_line < self.map.line_count() {
            self.map.content_range_of_line(delimiter_line)
        } else {
            NSRange::new(head_end, 0)
        };

        let alignments: Vec<TableAlignment> = column_alignments
            .iter()
            .map(|alignment| match alignment {
                Some(ColumnAlignment::Left) => TableAlignment::Left,
                Some(ColumnAlignment::Center) => TableAlignment::Center,
                Some(ColumnAlignment::Right) => TableAlignment::Right,
                None => TableAlignment::None,
            })
            .collect();
        MDBlock::new(BlockContent::Table(TableData::new(rows, alignments, delimiter_range)), range, range)
            .with_depth(depth)
            .with_quote_depth(quote_depth)
    }

    fn cells(&mut self, container: Markup<'_>) -> Vec<TableCell> {
        let mut out = Vec::new();
        for child in container.children() {
            if !matches!(child.data(), MarkupData::TableCell { .. }) {
                continue;
            }
            let Some(range) = self.range_of(child) else { continue };
            let spans = self.inlines.spans(child, range);
            let content = if spans.is_empty() {
                self.empty_cell_content_range(range)
            } else {
                let first = spans[0].range.location;
                NSRange::new(first, 0.max(spans[spans.len() - 1].range.upper_bound() - first))
            };
            out.push(TableCell::new(range, content, spans));
        }
        out
    }

    /// Content bounds for a cell with no inline children.
    fn empty_cell_content_range(&self, range: NSRange) -> NSRange {
        let text = self.text();
        let mut start = range.location;
        let mut end = range.upper_bound().min(text.length());
        while start < end && Self::is_cell_padding(text.character_at(start)) {
            start += 1;
        }
        while end > start && Self::is_cell_padding(text.character_at(end - 1)) {
            end -= 1;
        }
        NSRange::new(start, end - start)
    }

    fn is_cell_padding(character: u16) -> bool {
        character == 0x7C || character == 0x20 || character == 0x09
    }

    // MARK: Synthetic blocks

    pub fn front_matter_block(&self, front: &FrontMatter) -> MDBlock {
        MDBlock::new(BlockContent::FrontMatter(front.clone()), front.range, front.body_range)
            .with_marker_range(Some(NSRange::new(front.range.location, 0.max(front.body_range.location - front.range.location))))
            .with_trailing_marker_range(Some(NSRange::new(
                front.body_range.upper_bound(),
                0.max(front.range.upper_bound() - front.body_range.upper_bound()),
            )))
            .with_depth(1)
    }

    /// Retypes the block a footnote definition survived as, or synthesises
    /// one when cmark swallowed it as a link reference definition.
    pub fn attach_footnote_definitions(&mut self, children: &mut Vec<BlockRef>, definitions: &[FootnoteDefinition]) {
        for definition in definitions {
            if let Some(existing) = children.iter_mut().find(|child| child.range.contains(definition.range.location)) {
                let existing = Arc::get_mut(existing).expect("block uniquely owned while building");
                existing.content = BlockContent::FootnoteDefinition { identifier: definition.identifier.clone() };
                existing.marker_range = Some(definition.marker_range);
                existing.content_range = NSRange::new(
                    definition.marker_range.upper_bound(),
                    0.max(existing.range.upper_bound() - definition.marker_range.upper_bound()),
                );
                let inlines = std::mem::take(&mut existing.inlines);
                existing.inlines = InlineSpan::clip_leading(inlines, definition.marker_range.upper_bound());
                continue;
            }
            let synthesized = Self::clamped_synthesis_range(definition.range, children);
            let block = MDBlock::new(
                BlockContent::FootnoteDefinition { identifier: definition.identifier.clone() },
                synthesized,
                NSRange::new(
                    definition.marker_range.upper_bound(),
                    0.max(synthesized.upper_bound() - definition.marker_range.upper_bound()),
                ),
            )
            .with_marker_range(Some(definition.marker_range))
            .with_depth(1);
            let insertion = children.iter().position(|child| child.range.location > definition.range.location);
            children.insert(insertion.unwrap_or(children.len()), Arc::new(block));
        }
    }

    fn clamped_synthesis_range(range: NSRange, children: &[BlockRef]) -> NSRange {
        let mut clamped = range;
        if let Some(next) = children.iter().find(|child| child.range.location > range.location)
            && next.range.location < clamped.upper_bound()
        {
            clamped.length = 0.max(next.range.location - clamped.location);
        }
        clamped
    }
}

// MARK: - Identity and hashing

pub struct BlockIdentifier;

impl BlockIdentifier {
    /// `BlockIdentity` is (kind, ordinal-among-same-kind-siblings).
    pub fn assign(root: &mut BlockRef) {
        let root = Arc::get_mut(root).expect("block uniquely owned while building");
        Self::assign_children(&mut root.children);
    }

    fn assign_children(children: &mut [BlockRef]) {
        let mut counters: HashMap<isize, isize> = HashMap::new();
        for child in children.iter_mut() {
            let child = Arc::get_mut(child).expect("block uniquely owned while building");
            let kind = Self::discriminator(&child.content);
            let ordinal = *counters.get(&kind).unwrap_or(&0);
            counters.insert(kind, ordinal + 1);
            child.identity = BlockIdentity::new(kind, ordinal);
            Self::assign_children(&mut child.children);
        }
    }

    pub fn discriminator(content: &BlockContent) -> isize {
        match content {
            BlockContent::Document => 0,
            BlockContent::Heading { level } => 100 + level,
            BlockContent::Paragraph => 2,
            BlockContent::BlockQuote => 3,
            BlockContent::Callout { .. } => 4,
            BlockContent::List { .. } => 5,
            BlockContent::ListItem { .. } => 6,
            BlockContent::CodeBlock { .. } => 7,
            BlockContent::Mermaid { .. } => 8,
            BlockContent::MathBlock { .. } => 9,
            BlockContent::Table(_) => 10,
            BlockContent::ThematicBreak => 11,
            BlockContent::HtmlBlock => 12,
            BlockContent::FrontMatter(_) => 13,
            BlockContent::FootnoteDefinition { .. } => 14,
        }
    }
}

pub struct SubtreeHasher;

impl SubtreeHasher {
    /// Hashes kind, source bytes and children (§3.5).
    pub fn hash(block: &mut BlockRef, text: &[u16]) {
        let block = Arc::get_mut(block).expect("block uniquely owned while building");
        for child in block.children.iter_mut() {
            Self::hash(child, text);
        }
        let length = text.length();
        let mut h = FNV::combine_u64(FNV::OFFSET_BASIS, BlockIdentifier::discriminator(&block.content) as u64);
        if block.children.is_empty() {
            h = FNV::combine_range(h, text, Self::clamp(block.range, length));
        } else {
            // Hash every gap between children plus the trailing gap.
            let full = Self::clamp(block.range, length);
            let mut scan = full.location;
            for child in &block.children {
                let child_range = Self::clamp(child.range, length);
                if child_range.location > scan {
                    h = FNV::combine_range(h, text, NSRange::new(scan, child_range.location - scan));
                }
                h = FNV::combine_u64(h, child.subtree_hash);
                if child_range.upper_bound() > scan {
                    scan = child_range.upper_bound();
                }
            }
            if full.upper_bound() > scan {
                h = FNV::combine_range(h, text, NSRange::new(scan, full.upper_bound() - scan));
            }
        }
        block.subtree_hash = h;
    }

    /// Hashes only a container's own bytes (kind + gaps between children).
    pub fn framework_hash(block: &MDBlock, text: &[u16]) -> u64 {
        let length = text.length();
        let mut h = FNV::combine_u64(FNV::OFFSET_BASIS, BlockIdentifier::discriminator(&block.content) as u64);
        let full = Self::clamp(block.range, length);
        let mut scan = full.location;
        for child in &block.children {
            let child_range = Self::clamp(child.range, length);
            if child_range.location > scan {
                h = FNV::combine_range(h, text, NSRange::new(scan, child_range.location - scan));
            }
            if child_range.upper_bound() > scan {
                scan = child_range.upper_bound();
            }
        }
        if full.upper_bound() > scan {
            h = FNV::combine_range(h, text, NSRange::new(scan, full.upper_bound() - scan));
        }
        h
    }

    fn clamp(range: NSRange, length: isize) -> NSRange {
        let location = 0.max(range.location.min(length));
        NSRange::new(location, 0.max(range.length.min(length - location)))
    }
}
