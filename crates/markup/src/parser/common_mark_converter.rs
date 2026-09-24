//! Port of `Sources/Markdown/Parser/CommonMarkConverter.swift` on
//! pulldown-cmark.
//!
//! swift-markdown converts the tree cmark-gfm builds (with
//! `CMARK_OPT_TABLE_SPANS | CMARK_OPT_SOURCEPOS` and the `table`,
//! `strikethrough` and `tasklist` extensions). Upleft parses with its fork of
//! pulldown-cmark instead (`vendor/pulldown-cmark`), whose
//! `ENABLE_CMARK_GFM_COMPAT` option parses as cmark-gfm does wherever
//! CommonMark 0.31 and cmark-gfm disagree (the fork's `UPLEFT.md` lists
//! them), and builds the same markup tree from its events: the same element
//! kinds and children, the same strings, and the source ranges cmark would
//! have reported, including cmark's quirks:
//!
//! - Tight list items keep their paragraphs.
//! - Adjacent text nodes are one node (`cmark_consolidate_text_nodes`); a text
//!   node that is a partly used emphasis delimiter run keeps the whole run's
//!   extent, and emphasis spans from its opener run's start to its closer
//!   run's end. A text node ending in an unmatched `~` has no valid range
//!   (the strikethrough extension never sets its end column). Whitespace
//!   before a line break belongs to the text before it, or to an empty text
//!   node after any other inline.
//! - Inline columns are measured in the paragraph's text after container
//!   prefixes are stripped (so lazy continuation lines and multi-line code
//!   spans report cmark's columns), line numbers count from the paragraph's
//!   first line (so link reference definitions at its start shift them), and
//!   a backslash hard break does not advance the line.
//! - Blocks end where cmark's `finalize()` puts them: containers that stay
//!   open across blank lines end at the line before the next block (often
//!   column 0 of a blank line), fenced code and setext headings end on the
//!   line that closes them, multi-line HTML blocks of kinds 1–5 on the line
//!   before their end marker.
//! - Tables use the GFM extension's cell offsets, column and row spans, and
//!   its positions when a table interrupts a paragraph (the text before the
//!   table has no position, and its inlines count lines and columns from
//!   0 with `\|` unescaped).
//! - Inline attributes (`^[text](attributes)`) come from the fork as links
//!   of type `LinkType::InlineAttributes`; cmark puts them on the line of
//!   their closing bracket.

use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};

use crate::base::document::Document;
use crate::base::raw_markup::{Checkbox, NodeId, RawMarkupArena, RawMarkupData};
use crate::infrastructure::source_location::{SourceLocation, SourceRange};
use crate::nodes::tables::ColumnAlignment;
use crate::parser::cmark_lines::{self, Line, Prefix, SourceLines};
use crate::parser::cmark_table::{self, Row};
use crate::parser::parse_options::ParseOptions;
use crate::utility::swift_string::swift_contains_character;

/// Parses markup source and returns the document it represents
/// (`MarkupParser`).
pub(crate) struct MarkupParser;

impl MarkupParser {
    /// `parseString(_:source:options:)`.
    pub(crate) fn parse_string(string: &str, options: ParseOptions) -> Document {
        let mut text = std::borrow::Cow::Borrowed(string);
        if text.contains('\0') {
            // cmark replaces each NUL with U+FFFD as it reads lines, so its
            // strings and columns are those of the replaced text.
            text = std::borrow::Cow::Owned(text.replace('\0', "\u{FFFD}"));
        }
        if text.bytes().any(|byte| byte == b'\r') {
            // cmark ends a line at a lone `\r` and never passes the line
            // ending on; pulldown-cmark misreads lone `\r`s, so they become
            // `\n`s (the same length, so offsets and lines are unchanged).
            let bytes = text.as_bytes();
            let lone = (0..bytes.len())
                .any(|index| bytes[index] == b'\r' && bytes.get(index + 1) != Some(&b'\n'));
            if lone {
                let mut normalized = String::with_capacity(text.len());
                let mut characters = text.chars().peekable();
                while let Some(character) = characters.next() {
                    if character == '\r' && characters.peek() != Some(&'\n') {
                        normalized.push('\n');
                    } else {
                        normalized.push(character);
                    }
                }
                text = std::borrow::Cow::Owned(normalized);
            }
        }
        let mut converter = Converter::new(&text, options);
        converter.input_length = string.len();
        converter.run()
    }
}

/// `MarkupConverterState.range(_:)`: a cmark position (1-based lines and
/// byte columns, end inclusive) as a half-open swift-markdown range, widened
/// by the backtick count of a code span. `None` where cmark reports no
/// position, or an inverted one (rdar://73376719).
fn cmark_range(
    start_line: i64,
    start_column: i64,
    end_line: i64,
    end_column: i64,
    backtick_count: i64,
) -> Option<SourceRange> {
    if !(start_line > 0 && start_column > 0) {
        return None;
    }
    let end_column = end_column + 1;
    if !(end_line > 0 && end_column > 0) {
        return None;
    }
    let start = SourceLocation::new(start_line, start_column - backtick_count);
    let end = SourceLocation::new(end_line, end_column + backtick_count);
    if !(start <= end) {
        return None;
    }
    Some(SourceRange::new(start, end))
}

fn is_space_or_tab(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

/// Where a block is finalized: while cmark processes a line (0-based), or at
/// the end of the input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Finalize {
    Line(usize),
    Eof,
}

/// How a block's end is found.
#[derive(Clone, Copy, Debug)]
enum EndRule {
    /// The end is known; `n` is where the block is finalized.
    Done { n: Finalize },
    /// The block stays open across blank lines (lists, items, indented and
    /// unclosed fenced code, unclosed HTML blocks of kinds 1–5): it is
    /// finalized by the next block or by its parent.
    Absorbing { fenced: bool },
}

/// A block, for the pass that finds where open blocks end.
#[derive(Clone, Copy, Debug)]
struct BlockRecord {
    node: NodeId,
    /// The 0-based line cmark creates the block on.
    first_line: usize,
    /// cmark's start position, or `None` when it reports none.
    start: Option<(i64, i64)>,
    /// Where pulldown-cmark's source range for the block ends.
    range_end: usize,
    /// The container (document, block quote or item) the block is in, whose
    /// link reference definitions can end it.
    container: u32,
    rule: EndRule,
}

/// A text node being assembled from consecutive pulldown-cmark text events.
#[derive(Clone, Copy, Debug)]
struct PendingText {
    /// Where the node's string starts in the arena's string buffer.
    string_start: usize,
    /// Source offset where cmark's first text piece starts.
    start: usize,
    /// Source offset where the last piece ends.
    end: usize,
}

/// One line of an inline container's text, as cmark's inline parser sees it.
#[derive(Clone, Copy, Debug)]
struct SubjectLine {
    /// Source offset where the line's text starts (after container prefixes
    /// and, unless the line is a lazy continuation, leading whitespace).
    source_start: usize,
    /// Position of the line's start in the concatenated text.
    subject_start: i64,
    /// Spaces cmark's `add_line` puts first for a tab that a container's
    /// prefix partly consumed on a lazy line (`source_start` is then just
    /// after the tab).
    lead: i64,
    /// The inline parser's line and column base when it went on to this
    /// line, if it did (a backslash hard break keeps both).
    entered: Option<(i64, i64)>,
}

impl SubjectLine {
    /// The position of `offset`, on this line, in the concatenated text.
    fn subject(&self, offset: usize) -> i64 {
        if self.lead > 0 && offset < self.source_start {
            // The partly consumed tab: where its spaces start.
            return self.subject_start;
        }
        self.subject_start + self.lead + offset.saturating_sub(self.source_start) as i64
    }
}

/// The inline parser's position state for one paragraph, heading or cell
/// (cmark's `subject`: `line`, `block_offset`, `column_offset`).
#[derive(Debug)]
struct InlineContext {
    first_source_line: usize,
    lines: Vec<SubjectLine>,
    /// Line reported for inlines (`subj->line`).
    line: i64,
    block_offset: i64,
    /// Subject position of the current line's start (`-column_offset`).
    base: i64,
    /// Source offsets of backslashes cmark removed before `|` (table cells).
    removed: Vec<usize>,
    /// Source offset where the previous inline sibling ended.
    previous_end: usize,
    /// Only one line (headings, table cells).
    single_line: bool,
    /// Where whitespace that cmark keeps as text starts: at the start of a
    /// lazy continuation line that no `handle_newline` skipped (after a
    /// backslash hard break, or first once definitions are removed).
    kept_whitespace: Option<usize>,
    /// The source offsets of the line last looked up, and its index.
    cached: (usize, usize, usize),
}

#[derive(Debug)]
enum FrameKind<'a> {
    Document,
    BlockQuote,
    List {
        start: Option<u64>,
    },
    Item {
        checkbox: Option<Checkbox>,
        /// 0-based line of the item's marker.
        first_line: usize,
    },
    Paragraph {
        synthetic: bool,
    },
    Heading {
        level: i64,
        setext: bool,
    },
    CodeBlock {
        fenced: Option<(u8, usize)>,
        info: Option<CowStr<'a>>,
    },
    HtmlBlock,
    Table(Box<TableState>),
    TableHead,
    TableRow,
    /// A cell and cmark's end column for it.
    TableCell {
        end_column: i64,
    },
    Emphasis,
    Strong,
    Strikethrough,
    Link {
        destination: Option<CowStr<'a>>,
        title: CowStr<'a>,
        /// `<scheme:…>` or `<address@…>`, whose text cmark decodes.
        autolink: bool,
    },
    /// swift-cmark's `^[text](attributes)`.
    Attributes {
        attributes: CowStr<'a>,
    },
    Image {
        source: CowStr<'a>,
        title: CowStr<'a>,
    },
}

#[derive(Debug)]
struct Frame<'a> {
    kind: FrameKind<'a>,
    /// The enclosing container's number, restored when this frame closes.
    outer_container: u32,
    children_start: usize,
    /// cmark's start line and column (raw, 1-based).
    start: (i64, i64),
    range: Range<usize>,
    /// The first arena id created inside this frame.
    first_node: NodeId,
}

/// A table being converted: the extension's rows, and each cell's node and
/// row span so later rows can extend them.
#[derive(Debug)]
struct TableState {
    start: (i64, i64),
    first_line: usize,
    columns: usize,
    alignments: Vec<Option<ColumnAlignment>>,
    rows: Vec<TableRowState>,
    header: Option<NodeId>,
    body_rows: Vec<NodeId>,
    /// cmark's range of the first body row.
    first_body_range: Option<SourceRange>,
    /// The header row's string: the header line, after the text of the
    /// paragraph the table interrupted, if any.
    header_string: Vec<u8>,
    /// Where the header line starts in `header_string`.
    header_line_start: usize,
}

#[derive(Debug, Default)]
struct TableRowState {
    row: Row,
    /// Source offset of the row line's first non-space character.
    source_base: usize,
    /// Where that character is in the row string.
    string_base: usize,
    line: usize,
    cells: Vec<CellState>,
    next_cell: usize,
}

#[derive(Clone, Copy, Debug)]
struct CellState {
    node: Option<NodeId>,
    rowspan: u64,
    colspan: u64,
    /// Filled in by cmark for a short row; has no span data.
    autocompleted: bool,
    /// A `^` marker whose content cmark cleared.
    cleared: bool,
}

/// A paragraph just closed, in case a table interrupts it.
#[derive(Debug)]
struct ClosedParagraph {
    node: NodeId,
    first_node: NodeId,
    last_line: usize,
    start: (i64, i64),
    first_line: usize,
    /// The paragraph's text lines, to rebuild cmark's paragraph content.
    context: InlineContext,
}

struct Converter<'a> {
    text: &'a str,
    /// The length of the text as cmark receives it (before NUL replacement
    /// and without a byte order mark removed): its reference expansion
    /// limit depends on it.
    input_length: usize,
    bytes: &'a [u8],
    options: ParseOptions,
    lines: SourceLines,
    arena: RawMarkupArena,
    children: Vec<NodeId>,
    frames: Vec<Frame<'a>>,
    prefixes: Vec<Prefix>,
    records: Vec<BlockRecord>,
    inline: Option<InlineContext>,
    pending: Option<PendingText>,
    /// Literal of the code or HTML block being read.
    literal: String,
    /// Inline events inside a cleared rowspan marker cell are dropped.
    skip_inlines: bool,
    /// Where the last inline event of a synthetic paragraph ended.
    last_inline_end: usize,
    closed_paragraph: Option<ClosedParagraph>,
    /// Lines holding link reference definitions, in order: (0-based line,
    /// offset of its first non-space character, the container it is in).
    /// pulldown-cmark reports no event for a definition, so these are the
    /// non-blank lines no block covers.
    definitions: Vec<(usize, usize, u32)>,
    /// Identifies the innermost open container (document, block quote,
    /// item): each gets its own number.
    container: u32,
    containers_opened: u32,
    /// Where the source not yet covered by a block starts: after the last
    /// block in the current container, or after the container's prefix.
    gap_from: usize,
    /// Code spans and inline HTML whose end column cmark measured without
    /// the block offset (they span lines).
    unshifted_ends: Vec<NodeId>,
    /// Text nodes whose end the strikethrough extension left unset, with
    /// their line and start column (a table's shift can make them valid).
    tilde_texts: Vec<(NodeId, i64, i64)>,
    /// Lazy lines where cmark's tasklist extension skipped 3 bytes.
    task_skips: Vec<Range<usize>>,
    /// Every code span and its backtick count, which its range includes.
    code_ticks: Vec<(NodeId, i64)>,
    /// A subject line buffer to reuse for the next inline context.
    spare_lines: Vec<SubjectLine>,
    /// Where the code block's last text event ended.
    code_text_end: usize,
    /// The first line's indentation of the HTML block being read, used
    /// unless pulldown-cmark reports it.
    html_indentation: Option<String>,
}

impl<'a> Converter<'a> {
    fn new(text: &'a str, options: ParseOptions) -> Converter<'a> {
        Converter {
            text,
            input_length: text.len(),
            bytes: text.as_bytes(),
            options,
            lines: SourceLines::new(text.as_bytes()),
            // Sized from the source: agent-shaped Markdown averages about one
            // element per 12 bytes, and text is at most the source's length.
            arena: RawMarkupArena::with_capacity(text.len() / 12 + 16, text.len()),
            children: Vec::with_capacity(64),
            frames: Vec::with_capacity(32),
            prefixes: Vec::with_capacity(8),
            records: Vec::with_capacity(text.len() / 64 + 8),
            inline: None,
            pending: None,
            literal: String::new(),
            skip_inlines: false,
            last_inline_end: 0,
            closed_paragraph: None,
            definitions: Vec::new(),
            container: 0,
            containers_opened: 0,
            gap_from: 0,
            unshifted_ends: Vec::new(),
            tilde_texts: Vec::new(),
            task_skips: Vec::new(),
            code_ticks: Vec::new(),
            spare_lines: Vec::new(),
            code_text_end: 0,
            html_indentation: None,
        }
    }

    fn run(mut self) -> Document {
        let mut pulldown_options = Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_CMARK_GFM_COMPAT;
        if !self.options.contains(ParseOptions::DISABLE_SMART_OPTS) {
            pulldown_options |= Options::ENABLE_SMART_PUNCTUATION;
        }
        // cmark skips a UTF-8 byte order mark at the start of the first line;
        // pulldown-cmark would read it as text.
        let bom = if self.text.starts_with('\u{FEFF}') {
            3
        } else {
            0
        };
        let parser = Parser::new_ext(&self.text[bom..], pulldown_options)
            .cmark_input_length(self.input_length)
            .into_offset_iter();

        self.frames.push(Frame {
            kind: FrameKind::Document,
            outer_container: 0,
            children_start: 0,
            start: (1, 1),
            range: 0..self.text.len(),
            first_node: 0,
        });

        for (event, range) in parser {
            let range = range.start + bom..range.end + bom;
            match event {
                Event::Start(tag) => self.start(tag, range),
                Event::End(tag) => self.end(tag, range),
                Event::Text(text) => self.text_event(text, range),
                Event::Code(code) => self.code(code, range),
                Event::Html(html) => self.html(html, range),
                Event::InlineHtml(html) => self.inline_html(html, range),
                Event::SoftBreak => self.line_break(range, false),
                Event::HardBreak => self.line_break(range, true),
                Event::Rule => self.rule(range),
                Event::TaskListMarker(checked) => self.task_list_marker(checked, range),
                Event::FootnoteReference(_) | Event::InlineMath(_) | Event::DisplayMath(_) => {
                    unreachable!("pulldown-cmark option not enabled")
                }
            }
        }
        self.close_synthetic_paragraph();
        self.scan_gap(self.lines.count());

        let frame = self.frames.pop().expect("the document frame");
        assert!(matches!(frame.kind, FrameKind::Document));
        let count = self.lines.count();
        let end = if count == 0 {
            (0, 0)
        } else {
            (count as i64, self.lines.len(count - 1) as i64)
        };
        let range = cmark_range(1, 1, end.0, end.1, 0);
        let root = self.container(&frame, RawMarkupData::Document, range);
        self.record(
            root,
            0,
            Some((1, 1)),
            0..self.text.len(),
            EndRule::Done { n: Finalize::Eof },
        );
        self.resolve_open_blocks(root);
        Document {
            arena: self.arena,
            root,
        }
    }

    // MARK: Positions

    /// The source line index of `offset` (0-based).
    fn line_of(&self, offset: usize) -> usize {
        self.lines.line_of(offset)
    }

    fn line(&self, line: usize) -> Line<'a> {
        Line::new(self.bytes, &self.lines, line)
    }

    /// The raw cmark start of a block beginning at `offset`.
    fn block_start(&self, offset: usize) -> (i64, i64) {
        let line = self.line_of(offset);
        (
            line as i64 + 1,
            (offset - self.lines.start(line)) as i64 + 1,
        )
    }

    /// The last line of `range` that is not blank once the current
    /// containers' prefixes are stripped.
    fn last_content_line(&self, range: &Range<usize>) -> usize {
        let first = self.line_of(range.start);
        let mut last = self.line_of(range.end.saturating_sub(1).max(range.start));
        while last > first {
            let start = self.lines.start(last);
            let end = self.lines.end(last);
            let blank = self.bytes[start..end]
                .iter()
                .all(|&byte| is_space_or_tab(byte))
                || {
                    let line = self.line(last);
                    let (scan, all_matched) = line.match_prefixes(&self.prefixes);
                    all_matched && line.first_nonspace(&scan).blank
                };
            if blank {
                last -= 1;
            } else {
                break;
            }
        }
        last
    }

    /// cmark's end for a block finalized at `n` (`finalize()`): the line
    /// before, unless the block is a fenced code block, a setext heading, or
    /// was opened on line `n`.
    fn end_position(&self, n: Finalize, first_line: usize, same_line_rule: bool) -> (i64, i64) {
        match n {
            Finalize::Eof => {
                let count = self.lines.count();
                if count == 0 {
                    (0, 0)
                } else {
                    (count as i64, self.lines.len(count - 1) as i64)
                }
            }
            Finalize::Line(n) => {
                if same_line_rule || n == first_line || n == 0 {
                    // The line being processed, before any trimming.
                    (n as i64 + 1, self.lines.raw_len(n) as i64)
                } else {
                    (n as i64, self.lines.len(n - 1) as i64)
                }
            }
        }
    }

    /// Where a block whose last line is `last` is finalized: on the next
    /// line, or at the end of the input.
    fn after(&self, last: usize) -> Finalize {
        if last + 1 < self.lines.count() {
            Finalize::Line(last + 1)
        } else {
            Finalize::Eof
        }
    }

    fn record(
        &mut self,
        node: NodeId,
        first_line: usize,
        start: Option<(i64, i64)>,
        range: Range<usize>,
        rule: EndRule,
    ) {
        self.records.push(BlockRecord {
            node,
            first_line,
            start,
            range_end: range.end,
            container: self.container,
            rule,
        });
    }

    /// Sets a block's range from its start and a finalize point.
    fn finish_block(
        &mut self,
        node: NodeId,
        start: Option<(i64, i64)>,
        first_line: usize,
        n: Finalize,
        same_line_rule: bool,
    ) {
        let range = start.and_then(|(line, column)| {
            let (end_line, end_column) = self.end_position(n, first_line, same_line_rule);
            cmark_range(line, column, end_line, end_column, 0)
        });
        self.arena.nodes[node as usize].parsed_range = range;
    }

    /// Finds where every block that stays open across blank lines ends: at
    /// the next block's first line (or a link reference definition's), or
    /// where its parent ends.
    fn resolve_open_blocks(&mut self, root: NodeId) {
        let mut record_of = vec![u32::MAX; self.arena.nodes.len()];
        for (index, record) in self.records.iter().enumerate() {
            record_of[record.node as usize] = index as u32;
        }
        let mut stack = vec![(root, Finalize::Eof, self.text.len())];
        let mut children = Vec::new();
        while let Some((container, container_n, container_end)) = stack.pop() {
            children.clear();
            children.extend_from_slice(self.arena.children(container));
            for index in 0..children.len() {
                let record_index = record_of[children[index] as usize];
                if record_index == u32::MAX {
                    continue;
                }
                let record = self.records[record_index as usize];
                let next = children
                    .get(index + 1)
                    .map(|&next| record_of[next as usize])
                    .filter(|&next| next != u32::MAX)
                    .map(|next| self.records[next as usize]);
                let n = match record.rule {
                    EndRule::Done { n } => n,
                    EndRule::Absorbing { fenced } => {
                        let bound = next.map_or_else(
                            || self.line_of(container_end.saturating_sub(1)) + 1,
                            |next| next.first_line,
                        );
                        let definition = self.definition_line(
                            self.line_of(record.range_end),
                            bound,
                            record.container,
                        );
                        let candidate = match (next.map(|next| next.first_line), definition) {
                            (Some(a), Some(b)) => Some(a.min(b)),
                            (a, b) => a.or(b),
                        };
                        let n = candidate.map_or(container_n, Finalize::Line);
                        self.finish_block(record.node, record.start, record.first_line, n, fenced);
                        n
                    }
                };
                if matches!(
                    self.arena.nodes[record.node as usize].data,
                    RawMarkupData::BlockQuote
                        | RawMarkupData::UnorderedList
                        | RawMarkupData::OrderedList { .. }
                        | RawMarkupData::ListItem { .. }
                ) {
                    stack.push((record.node, n, record.range_end));
                }
            }
        }
    }

    /// The first line of a link reference definition in lines `from..to`
    /// that is directly in `container`.
    fn definition_line(&self, from: usize, to: usize, container: u32) -> Option<usize> {
        let index = self
            .definitions
            .partition_point(|&(line, _, _)| line < from);
        self.definitions[index..]
            .iter()
            .take_while(|&&(line, _, _)| line < to)
            .find(|&&(_, _, definition_container)| definition_container == container)
            .map(|&(line, _, _)| line)
    }

    /// Records the link reference definitions in the lines between the
    /// uncovered source and line `to` (exclusive): the lines there that are
    /// not blank once the containers' prefixes are stripped.
    fn scan_gap(&mut self, to: usize) {
        let from_line = self.line_of(self.gap_from);
        if self.gap_from >= self.bytes.len() || from_line >= to {
            return;
        }
        for line_index in from_line..to.min(self.lines.count()) {
            let line = self.line(line_index);
            // A line that misses a container's prefix is a lazy continuation
            // of the definitions.
            let (scan, _) = line.match_prefixes(&self.prefixes);
            let mut scan = scan;
            if scan.offset < self.gap_from {
                let distance = self.gap_from - scan.offset;
                line.advance(&mut scan, distance, false);
            }
            let first = line.first_nonspace(&scan);
            if !first.blank && first.offset < self.lines.end(line_index) {
                self.definitions
                    .push((line_index, first.offset, self.container));
            }
        }
        self.gap_from = self
            .lines
            .start(to.min(self.lines.count() - 1))
            .max(self.gap_from);
        if to >= self.lines.count() {
            self.gap_from = self.bytes.len();
        }
    }

    /// Scans the gap before a block starting at `offset`.
    fn scan_gap_before(&mut self, offset: usize) {
        let line = self.line_of(offset);
        self.scan_gap(line);
    }

    // MARK: Tree building

    fn push_frame(&mut self, kind: FrameKind<'a>, start: (i64, i64), range: Range<usize>) {
        let outer_container = self.container;
        if matches!(kind, FrameKind::BlockQuote | FrameKind::Item { .. }) {
            self.containers_opened += 1;
            self.container = self.containers_opened;
        }
        self.frames.push(Frame {
            kind,
            outer_container,
            children_start: self.children.len(),
            start,
            range,
            first_node: self.arena.nodes.len() as NodeId,
        });
    }

    fn container(
        &mut self,
        frame: &Frame<'a>,
        data: RawMarkupData,
        range: Option<SourceRange>,
    ) -> NodeId {
        let id = self
            .arena
            .create(data, range, &self.children[frame.children_start..]);
        self.children.truncate(frame.children_start);
        id
    }

    fn top_is(&self, predicate: impl Fn(&FrameKind<'a>) -> bool) -> bool {
        self.frames
            .last()
            .is_some_and(|frame| predicate(&frame.kind))
    }

    // MARK: Inline contexts

    fn new_inline_context(
        &mut self,
        first_offset: usize,
        start_line: i64,
        block_offset: i64,
        single_line: bool,
    ) -> InlineContext {
        let mut lines = std::mem::take(&mut self.spare_lines);
        lines.clear();
        lines.push(SubjectLine {
            source_start: first_offset,
            subject_start: 0,
            lead: 0,
            entered: Some((start_line, 0)),
        });
        InlineContext {
            first_source_line: self.line_of(first_offset),
            lines,
            line: start_line,
            block_offset,
            base: 0,
            removed: Vec::new(),
            previous_end: first_offset,
            single_line,
            kept_whitespace: None,
            cached: (0, 0, 0),
        }
    }

    /// Ends the current inline context, keeping its line buffer for reuse.
    fn drop_inline(&mut self) {
        if let Some(context) = self.inline.take() {
            self.spare_lines = context.lines;
        }
    }

    /// Forgets the paragraph a table could have interrupted.
    fn discard_closed_paragraph(&mut self) {
        if let Some(paragraph) = self.closed_paragraph.take() {
            self.spare_lines = paragraph.context.lines;
        }
    }

    /// The subject position of a source offset in the current inline
    /// container.
    fn subject(&mut self, offset: usize) -> i64 {
        let context = self.inline.as_ref().expect("an inline context");
        if context.single_line {
            let first = context.lines[0];
            let removed = context
                .removed
                .iter()
                .filter(|&&position| position < offset)
                .count() as i64;
            return first.subject_start + offset as i64 - first.source_start as i64 - removed;
        }
        // Most lookups fall on the line of the previous one.
        if (context.cached.0..context.cached.1).contains(&offset) {
            let subject_line = context.lines[context.cached.2];
            return subject_line.subject(offset);
        }
        let line = self.lines.line_of(offset);
        if line < context.first_source_line {
            let first = context.lines[0];
            return first.subject_start + offset as i64 - first.source_start as i64;
        }
        let index = line - context.first_source_line;
        while self.inline.as_ref().unwrap().lines.len() <= index {
            self.extend_subject();
        }
        let next_start = if line + 1 < self.lines.count() {
            self.lines.start(line + 1)
        } else {
            usize::MAX
        };
        let line_start = self.lines.start(line);
        let context = self.inline.as_mut().unwrap();
        context.cached = (line_start, next_start, index);
        let subject_line = context.lines[index];
        subject_line.subject(offset)
    }

    /// Computes the next line of the current inline container's text:
    /// where cmark's `add_line` starts it after matching the containers'
    /// prefixes (from the first non-space character, or from where matching
    /// stopped on a lazy continuation line).
    fn extend_subject(&mut self) {
        let context = self.inline.as_ref().unwrap();
        let previous_index = context.lines.len() - 1;
        let previous = context.lines[previous_index];
        let previous_line = context.first_source_line + previous_index;
        let next_line = previous_line + 1;
        let subject_start = previous.subject(self.lines.end(previous_line)) + 1;
        let mut lead = 0;
        let source_start = if next_line < self.lines.count() {
            let line = self.line(next_line);
            let (scan, all_matched) = line.match_prefixes(&self.prefixes);
            if all_matched {
                line.first_nonspace(&scan).offset
            } else if let Some(skip) = self
                .task_skips
                .iter()
                .find(|skip| skip.start == scan.offset)
            {
                skip.end
            } else if scan.partially_consumed_tab {
                lead = cmark_lines::tab_remainder(&scan);
                scan.offset + 1
            } else {
                scan.offset
            }
        } else {
            self.bytes.len()
        };
        self.inline.as_mut().unwrap().lines.push(SubjectLine {
            source_start,
            subject_start,
            lead,
            entered: None,
        });
    }

    /// The spaces a partly consumed tab at `offset` stands for, if it is one
    /// (see `SubjectLine::lead`); 0 otherwise.
    fn tab_lead(&self, offset: usize) -> i64 {
        let Some(context) = self.inline.as_ref() else {
            return 0;
        };
        let line = self.line_of(offset);
        line.checked_sub(context.first_source_line)
            .and_then(|index| context.lines.get(index))
            .filter(|subject_line| subject_line.lead > 0 && subject_line.source_start == offset + 1)
            .map_or(0, |subject_line| subject_line.lead)
    }

    /// The raw cmark column of the character at `offset`.
    fn column(&mut self, offset: usize) -> i64 {
        let subject = self.subject(offset);
        let context = self.inline.as_ref().unwrap();
        subject - context.base + 1 + context.block_offset
    }

    fn inline_line(&self) -> i64 {
        self.inline.as_ref().map_or(0, |context| context.line)
    }

    /// Processes a newline the inline parser counts (`handle_newline`): the
    /// next line's text starts a new column base.
    fn count_newline(&mut self, newline_line: usize) {
        let index = newline_line + 1 - self.inline.as_ref().unwrap().first_source_line;
        while self.inline.as_ref().unwrap().lines.len() <= index {
            self.extend_subject();
        }
        let context = self.inline.as_mut().unwrap();
        context.base = context.lines[index].subject_start;
        context.line += 1;
        context.lines[index].entered = Some((context.line, context.base));
    }

    /// `adjust_subj_node_newlines`: after a code span or inline HTML that
    /// spans lines, cmark moves the line on and measures the node's end from
    /// the last line's start, without the block offset. Returns the new end
    /// line and column, if the node spanned lines.
    fn adjust_newlines(&mut self, window_start: usize, window_end: usize) -> Option<(i64, i64)> {
        if self.options.contains(ParseOptions::DISABLE_SOURCE_POS_OPTS) {
            return None;
        }
        let first = self.line_of(window_start);
        let last = self.line_of(window_end);
        if last == first {
            return None;
        }
        let newlines = (last - first) as i64;
        let end_subject = self.subject(window_end);
        let context = self.inline.as_ref().unwrap();
        let index = last - context.first_source_line;
        let last_start = context.lines[index].subject_start;
        let since_newline = end_subject - last_start;
        let context = self.inline.as_mut().unwrap();
        let end_line = context.line + newlines;
        context.line += newlines;
        context.base = last_start;
        context.lines[index].entered = Some((context.line, context.base));
        Some((end_line, since_newline))
    }

    // MARK: Text

    fn is_escaped(&self, offset: usize) -> bool {
        let mut backslashes = 0;
        let mut position = offset;
        while position > 0 && self.bytes[position - 1] == b'\\' {
            backslashes += 1;
            position -= 1;
        }
        backslashes % 2 == 1
    }

    /// The start of the delimiter run of `*`, `_` or `~` containing the
    /// delimiter at `offset`.
    fn run_start(&self, offset: usize) -> usize {
        let delimiter = self.bytes[offset];
        let mut start = offset;
        while start > 0 && self.bytes[start - 1] == delimiter {
            start -= 1;
        }
        while start < offset && self.is_escaped(start) {
            start += 1;
        }
        start
    }

    /// The end (exclusive) of the delimiter run containing the delimiter at
    /// `offset`.
    fn run_end(&self, offset: usize) -> usize {
        let delimiter = self.bytes[offset];
        let mut end = offset + 1;
        while end < self.bytes.len() && self.bytes[end] == delimiter {
            end += 1;
        }
        end
    }

    fn is_emphasis_delimiter(byte: u8) -> bool {
        byte == b'*' || byte == b'_'
    }

    /// Adds a piece of text: consecutive pieces are one text node, as after
    /// `cmark_consolidate_text_nodes`.
    fn add_text_piece(&mut self, text: &str, range: Range<usize>) {
        if self.pending.is_none() {
            let mut start = range.start;
            if start >= 2
                && self.bytes[start] == b'|'
                && self.bytes[start - 1] == b'\\'
                && self.bytes[start - 2] == b'\\'
            {
                // `\\|` where cmark unescapes pipes (a table cell, or text
                // before a table): it removed the second backslash, and the
                // first escapes the pipe.
                start -= 2;
            } else if start > 0
                && start < self.bytes.len()
                && self.bytes[start - 1] == b'\\'
                && self.bytes[start].is_ascii_punctuation()
            {
                // A backslash escape: cmark's piece starts at the backslash.
                start -= 1;
            } else if start < range.end && Self::is_emphasis_delimiter(self.bytes[start]) {
                // The rest of a delimiter run: cmark's text node keeps the
                // whole run's position.
                start = self.run_start(start);
            }
            self.pending = Some(PendingText {
                string_start: self.arena.strings.len(),
                start,
                end: range.end,
            });
        }
        self.arena.strings.push_str(text);
        let pending = self.pending.as_mut().unwrap();
        pending.end = pending.end.max(range.end);
    }

    /// Creates the pending text node. `line_end` is the offset of the line
    /// ending when a counted line break follows: cmark's text then runs up
    /// to it, trailing whitespace included.
    fn flush_text(&mut self, line_end: Option<usize>) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        let start_column = self.column(pending.start);
        let line = self.inline_line();
        let end = pending.end;
        let end_column = if let Some(line_end) = line_end.filter(|&line_end| {
            line_end > end
                && self.bytes[end..line_end]
                    .iter()
                    .all(|&byte| is_space_or_tab(byte))
        }) {
            self.column(line_end - 1)
        } else if end <= pending.start {
            self.column(pending.start) - 1
        } else {
            let last = end - 1;
            let byte = self.bytes[last];
            if Self::is_emphasis_delimiter(byte) && !self.is_escaped(last) {
                let run_end = self.run_end(last);
                self.column(run_end - 1)
            } else if byte == b'~'
                && !self.is_escaped(self.run_start(last))
                && !self.top_is(|kind| matches!(kind, FrameKind::Link { autolink: true, .. }))
            {
                // The strikethrough extension never sets a text node's end
                // (an autolink's text is not its).
                0
            } else {
                self.column(last) + (self.tab_lead(last) - 1).max(0)
            }
        };
        let string = self.arena.str_since(pending.string_start);
        let range = cmark_range(line, start_column, line, end_column, 0);
        let node = self.arena.create(RawMarkupData::Text(string), range, &[]);
        if end_column == 0 && end > pending.start {
            self.tilde_texts.push((node, line, start_column));
        }
        self.children.push(node);
        if let Some(context) = self.inline.as_mut() {
            context.previous_end = end;
        }
    }

    fn text_event(&mut self, text: CowStr<'a>, range: Range<usize>) {
        if self.top_is(|kind| matches!(kind, FrameKind::CodeBlock { .. })) {
            self.literal.push_str(&text);
            self.code_text_end = range.end;
            return;
        }
        if self.top_is(|kind| matches!(kind, FrameKind::HtmlBlock { .. })) {
            // pulldown-cmark reports indentation left over after a list
            // item's prefix as text; the first line's is already in the
            // literal.
            // pulldown-cmark reports a first line's indentation as text; the
            // literal already has cmark's.
            if !(self.html_indentation.is_some() && range.is_empty()) {
                self.flush_html_indentation();
                self.literal.push_str(&text);
            }
            return;
        }
        if self.skip_inlines {
            return;
        }
        if text.is_empty() && range.is_empty() {
            // What pulldown-cmark leaves of trimmed heading text: nothing
            // for cmark.
            return;
        }
        self.ensure_inline_container(range.start);
        if text.contains('&')
            && self.top_is(|kind| matches!(kind, FrameKind::Link { autolink: true, .. }))
        {
            // `make_str_with_entities`.
            let decoded = decode_entities(&text);
            self.add_text_piece(&decoded, range.clone());
        } else {
            self.add_text_piece(&text, range.clone());
        }
        self.note_inline_end(range.end);
    }

    fn note_inline_end(&mut self, end: usize) {
        self.last_inline_end = self.last_inline_end.max(end);
    }

    // MARK: Inline leaves

    fn code(&mut self, code: CowStr<'a>, range: Range<usize>) {
        if self.skip_inlines {
            return;
        }
        self.ensure_inline_container(range.start);
        self.flush_text(None);
        let ticks = self.bytes[range.start..range.end]
            .iter()
            .take_while(|&&byte| byte == b'`')
            .count();
        let content_start = range.start + ticks;
        let content_end = range.end - ticks;
        let line = self.inline_line();
        let start_column = self.column(content_start);
        let mut end_line = line;
        let mut end_column = self.column(content_end.max(content_start + 1) - 1);
        let mut unshifted = false;
        if let Some((line, column)) = self.adjust_newlines(range.start, content_end) {
            end_line = line;
            end_column = column;
            unshifted = true;
        }
        let parsed_range = cmark_range(line, start_column, end_line, end_column, ticks as i64);
        // cmark reads the span from the paragraph's text, where continuation
        // lines have lost their indentation.
        let code = if unshifted {
            let mut text = self.subject_text(content_start, content_end);
            if text.matches("\\|").count() > code.matches("\\|").count() {
                // pulldown-cmark unescaped the pipes, as cmark does in the
                // text before a table.
                text = text.replace("\\|", "|");
            }
            CowStr::from(normalize_code(&text))
        } else {
            code
        };
        let literal = self.arena.push_str(&code);
        let data = if self.options.contains(ParseOptions::PARSE_SYMBOL_LINKS)
            && ticks > 1
            && !swift_contains_character(&code, '`')
        {
            RawMarkupData::SymbolLink {
                destination: Some(literal),
            }
        } else {
            RawMarkupData::InlineCode(literal)
        };
        let node = self.arena.create(data, parsed_range, &[]);
        if unshifted {
            self.unshifted_ends.push(node);
        }
        self.code_ticks.push((node, ticks as i64));
        self.children.push(node);
        self.inline.as_mut().unwrap().previous_end = range.end;
        self.note_inline_end(range.end);
    }

    /// The inline container's text between two source offsets, as cmark's
    /// subject holds it: each line from its subject start, joined by `\n`.
    fn subject_text(&mut self, from: usize, to: usize) -> String {
        let first = self.line_of(from);
        let last = self.line_of(to);
        // Make sure the subject lines are known.
        self.subject(to);
        let context = self.inline.as_ref().unwrap();
        let mut text = String::new();
        for line in first..=last {
            let index = line - context.first_source_line;
            let line_start = if line == first {
                from
            } else {
                for _ in 0..context.lines[index].lead {
                    text.push(' ');
                }
                context.lines[index].source_start
            };
            let line_end = if line == last {
                to
            } else {
                self.lines.end(line)
            };
            text.push_str(&self.text[line_start.min(line_end)..line_end]);
            if line != last {
                text.push('\n');
            }
        }
        text
    }

    fn inline_html(&mut self, html: CowStr<'a>, range: Range<usize>) {
        if self.skip_inlines {
            return;
        }
        self.ensure_inline_container(range.start);
        self.flush_text(None);
        let last = range.end - 1;
        let line = self.inline_line();
        let start_column = self.column(range.start);
        let mut end_line = line;
        let mut end_column = self.column(last);
        let mut unshifted = false;
        if let Some((line, column)) = self.adjust_newlines(range.start, last) {
            end_line = line;
            end_column = column;
            unshifted = true;
        }
        let parsed_range = cmark_range(line, start_column, end_line, end_column, 0);
        let literal = if unshifted {
            let mut text = self.subject_text(range.start, range.end);
            if text.matches("\\|").count() > html.matches("\\|").count() {
                // pulldown-cmark unescaped the pipes, as cmark does in the
                // text before a table.
                text = text.replace("\\|", "|");
            }
            self.arena.push_str(&text)
        } else {
            self.arena.push_str(&html)
        };
        let node = self
            .arena
            .create(RawMarkupData::InlineHtml(literal), parsed_range, &[]);
        if unshifted {
            self.unshifted_ends.push(node);
        }
        self.children.push(node);
        self.inline.as_mut().unwrap().previous_end = range.end;
        self.note_inline_end(range.end);
    }

    fn line_break(&mut self, range: Range<usize>, hard: bool) {
        if self.skip_inlines {
            return;
        }
        self.ensure_inline_container(range.start);
        let backslash = hard && self.bytes[range.start] == b'\\';
        let newline_line = self.line_of(range.end - 1);
        let line_end = self.lines.end(newline_line);
        // `handle_newline`: a hard break needs two spaces right before the
        // line ending; other trailing whitespace makes a soft break.
        let hard = hard
            && (backslash
                || (line_end >= 2
                    && self.bytes[line_end - 1] == b' '
                    && self.bytes[line_end - 2] == b' '));
        if backslash {
            self.flush_text(None);
        } else if self.pending.is_some() {
            self.flush_text(Some(line_end));
        } else {
            // Whitespace between another inline and the line ending is a text
            // node of its own, with an empty string.
            let previous_end = self.inline.as_ref().unwrap().previous_end;
            if previous_end < line_end
                && self.bytes[previous_end..line_end]
                    .iter()
                    .all(|&byte| is_space_or_tab(byte))
            {
                let line = self.inline_line();
                let start_column = self.column(previous_end);
                let end_column = self.column(line_end - 1);
                let string = self.arena.push_str("");
                let range = cmark_range(line, start_column, line, end_column, 0);
                let node = self.arena.create(RawMarkupData::Text(string), range, &[]);
                self.children.push(node);
            }
        }
        let data = if hard {
            RawMarkupData::LineBreak
        } else {
            RawMarkupData::SoftBreak
        };
        let node = self.arena.create(data, None, &[]);
        self.children.push(node);
        if backslash {
            // `handle_backslash` skips the line ending but not the next
            // line's leading whitespace, which a lazy line still has.
            let index = newline_line + 1 - self.inline.as_ref().unwrap().first_source_line;
            if newline_line + 1 < self.lines.count() {
                while self.inline.as_ref().unwrap().lines.len() <= index {
                    self.extend_subject();
                }
                let context = self.inline.as_mut().unwrap();
                let subject_line = context.lines[index];
                // A partly consumed tab's spaces start at the tab.
                context.kept_whitespace =
                    Some(subject_line.source_start - usize::from(subject_line.lead > 0));
                context.lines[index].entered = Some((context.line, context.base));
            }
        } else {
            self.count_newline(newline_line);
        }
        self.inline.as_mut().unwrap().previous_end = range.end;
        self.note_inline_end(range.end);
    }

    // MARK: Synthetic paragraphs

    /// pulldown-cmark leaves out the paragraphs of tight list items; cmark
    /// keeps them. Opens one when inline content arrives directly in an
    /// item.
    fn ensure_inline_container(&mut self, offset: usize) {
        if !self.top_is(|kind| matches!(kind, FrameKind::Item { .. })) {
            self.keep_whitespace(offset);
            return;
        }
        let mut start = offset;
        if start > 0
            && start < self.bytes.len()
            && self.bytes[start - 1] == b'\\'
            && self.bytes[start].is_ascii_punctuation()
        {
            start -= 1;
        }
        self.open_paragraph(start, true);
        self.keep_whitespace(offset);
    }

    /// Adds the whitespace cmark keeps at the start of a line (see
    /// `kept_whitespace`) before the inline starting at `offset`.
    fn keep_whitespace(&mut self, offset: usize) {
        let Some(context) = self.inline.as_mut() else {
            return;
        };
        let Some(start) = context.kept_whitespace.take() else {
            return;
        };
        // A backslash escape starts at its backslash.
        let offset = if offset > start
            && self.bytes[offset - 1] == b'\\'
            && self.bytes.get(offset).is_some_and(u8::is_ascii_punctuation)
        {
            offset - 1
        } else {
            offset
        };
        // After a `]` that closed no attribute span, cmark dropped the label
        // that followed it and put the `]` where that label ended.
        let whitespace_end = start
            + self.bytes[start..offset]
                .iter()
                .take_while(|&&b| is_space_or_tab(b))
                .count();
        let offset = if whitespace_end + 1 < offset
            && self.bytes[whitespace_end] == b']'
            && self.bytes[whitespace_end + 1] == b'['
            && self.bytes[offset] == b']'
            && !self.bytes[whitespace_end + 2..offset]
                .iter()
                .any(|&b| b == b'[' || b == b']')
        {
            whitespace_end
        } else {
            offset
        };
        if start < offset
            && self.bytes[start..offset]
                .iter()
                .all(|&byte| is_space_or_tab(byte))
        {
            // A lazy line's partly consumed tab is spaces in cmark's text.
            let lead = context
                .lines
                .iter()
                .find(|line| line.lead > 0 && line.source_start == start + 1)
                .map(|line| line.lead as usize);
            match lead {
                Some(lead) => {
                    let whitespace = " ".repeat(lead) + &self.text[start + 1..offset];
                    self.add_text_piece(&whitespace, start..offset);
                }
                None => {
                    let whitespace = &self.text[start..offset];
                    self.add_text_piece(whitespace, start..offset);
                }
            }
        }
    }

    fn close_synthetic_paragraph(&mut self) {
        if self.top_is(|kind| matches!(kind, FrameKind::Paragraph { synthetic: true })) {
            let end = self.last_inline_end;
            self.close_paragraph(end);
        }
    }

    /// Opens a paragraph whose text starts at `offset`. Its start moves to
    /// the link reference definitions directly above it: cmark keeps them in
    /// the paragraph until it finalizes it.
    fn open_paragraph(&mut self, offset: usize, synthetic: bool) {
        self.scan_gap_before(offset);
        let start = self.definitions_before(offset);
        let block_start = self.block_start(start);
        self.push_frame(
            FrameKind::Paragraph { synthetic },
            block_start,
            start..offset,
        );
        let mut context = self.new_inline_context(offset, block_start.0, block_start.1 - 1, false);
        if start != offset {
            self.lazy_first_line(&mut context, offset);
        }
        self.inline = Some(context);
        self.last_inline_end = offset;
    }

    /// Definitions came first: the text's first line is where the
    /// paragraph's content now starts, which on a lazy continuation line
    /// includes its indentation (and the rest of a partly consumed tab as
    /// spaces), unless cmark's tasklist extension skipped 3 bytes there.
    fn lazy_first_line(&self, context: &mut InlineContext, offset: usize) {
        let line = self.line(self.line_of(offset));
        let (scan, all_matched) = line.match_prefixes(&self.prefixes);
        if all_matched || scan.offset >= offset {
            return;
        }
        if let Some(skip) = self
            .task_skips
            .iter()
            .find(|skip| skip.start == scan.offset)
        {
            context.lines[0].source_start = skip.end;
            context.kept_whitespace = Some(skip.end);
        } else if scan.partially_consumed_tab {
            context.lines[0].source_start = scan.offset + 1;
            context.lines[0].lead = cmark_lines::tab_remainder(&scan);
            context.kept_whitespace = Some(scan.offset);
        } else {
            context.lines[0].source_start = scan.offset;
            context.kept_whitespace = Some(scan.offset);
        }
    }

    /// Where cmark's paragraph starts when its text starts at `offset`: at
    /// the first of the link reference definitions directly above it, which
    /// cmark keeps in the paragraph until it finalizes it.
    fn definitions_before(&self, offset: usize) -> usize {
        let mut line = self.line_of(offset);
        let mut start = offset;
        for &(definition_line, definition_start, container) in self.definitions.iter().rev() {
            if definition_line + 1 != line || container != self.container {
                break;
            }
            line = definition_line;
            start = definition_start;
        }
        start
    }

    fn close_paragraph(&mut self, end: usize) {
        self.flush_text(None);
        self.gap_from = self.gap_from.max(end);
        let frame = self.frames.pop().expect("a paragraph frame");
        let context = self.inline.take().expect("a paragraph's inline context");
        let first_line = self.line_of(frame.range.start);
        let last_line =
            self.last_content_line(&(frame.range.start..end.max(frame.range.start + 1)));
        let n = self.after(last_line);
        let node = self.container(&frame, RawMarkupData::Paragraph, None);
        self.finish_block(node, Some(frame.start), first_line, n, false);
        self.record(
            node,
            first_line,
            Some(frame.start),
            frame.range.start..end,
            EndRule::Done { n },
        );
        self.closed_paragraph = Some(ClosedParagraph {
            node,
            first_node: frame.first_node,
            last_line,
            start: frame.start,
            first_line,
            context,
        });
        self.children.push(node);
    }

    /// Where cmark's content of a closed paragraph's `line` starts.
    fn paragraph_line_start(&self, paragraph: &ClosedParagraph, line: usize) -> usize {
        let context = &paragraph.context;
        let index = line.wrapping_sub(context.first_source_line);
        let start = match (line >= context.first_source_line)
            .then(|| context.lines.get(index))
            .flatten()
        {
            Some(subject_line) => subject_line.source_start,
            None => {
                let text_line = self.line(line);
                let (scan, all_matched) = text_line.match_prefixes(&self.prefixes);
                if all_matched {
                    text_line.first_nonspace(&scan).offset
                } else {
                    scan.offset
                }
            }
        };
        start.min(self.lines.end(line))
    }

    /// cmark's content of a closed paragraph: each line's text plus `\n`.
    /// It starts where the paragraph's text does: the definitions that can
    /// come before it were resolved (at a setext underline), and cmark
    /// dropped them from the content.
    fn paragraph_content(&self, paragraph: &ClosedParagraph) -> Vec<u8> {
        let mut content = Vec::new();
        let context = &paragraph.context;
        for line in context.first_source_line.max(paragraph.first_line)..=paragraph.last_line {
            let lead = (line >= context.first_source_line)
                .then(|| context.lines.get(line - context.first_source_line))
                .flatten()
                .map_or(0, |subject_line| subject_line.lead);
            content.extend(std::iter::repeat_n(b' ', lead as usize));
            let start = self.paragraph_line_start(paragraph, line);
            content.extend_from_slice(&self.bytes[start..self.lines.end(line)]);
            content.push(b'\n');
        }
        content
    }

    /// The backslashes cmark's `unescape_pipes` removes from a closed
    /// paragraph before the table that interrupts it (each one right before
    /// a `|`), as cmark lines and 0-based positions in them.
    fn unescaped_pipes(&self, paragraph: &ClosedParagraph) -> Vec<(i64, i64)> {
        let context = &paragraph.context;
        let first = self.lines.start(paragraph.first_line);
        let last = self.lines.end(paragraph.last_line);
        if !contains(&self.bytes[first..last], b"\\|") {
            return Vec::new();
        }
        let mut removed = Vec::new();
        for line in context.first_source_line..=paragraph.last_line {
            let Some(subject_line) = context.lines.get(line - context.first_source_line) else {
                continue;
            };
            let Some((cmark_line, base)) = subject_line.entered else {
                continue;
            };
            let start = subject_line.source_start.min(self.lines.end(line));
            for position in start..self.lines.end(line).saturating_sub(1) {
                if self.bytes[position] == b'\\' && self.bytes[position + 1] == b'|' {
                    removed.push((cmark_line, subject_line.subject(position) - base));
                }
            }
        }
        removed
    }

    // MARK: Start events

    fn start(&mut self, tag: Tag<'a>, range: Range<usize>) {
        let is_inline = matches!(
            tag,
            Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. } | Tag::Image { .. }
        );
        if is_inline {
            self.start_inline(tag, range);
            return;
        }
        self.close_synthetic_paragraph();
        if !matches!(tag, Tag::Table(_)) {
            self.discard_closed_paragraph();
        }
        if !matches!(
            tag,
            Tag::Paragraph | Tag::TableHead | Tag::TableRow | Tag::TableCell
        ) {
            self.scan_gap_before(range.start);
        }
        match tag {
            Tag::Paragraph => {
                self.open_paragraph(range.start, false);
                if let Some(frame) = self.frames.last_mut() {
                    frame.range.end = range.end;
                }
            }
            Tag::Heading { level, .. } => {
                let level = level as i64;
                // A setext heading spans its text's lines and the underline;
                // its text may itself look like an ATX heading.
                let setext = self.line_of(range.end.saturating_sub(1).max(range.start))
                    > self.line_of(range.start)
                    || !self.is_atx_heading(range.start);
                let start = if setext {
                    self.block_start(self.definitions_before(range.start))
                } else {
                    // A heading with text has its line trimmed in place; an
                    // empty one takes cmark's blank-line path, untrimmed.
                    let line = self.line_of(range.start);
                    let content = range.start + self.atx_internal_offset(range.start);
                    let end = self.lines.end(line);
                    let blank = content <= end
                        && self.bytes[content.min(end)..end]
                            .iter()
                            .all(|&byte| is_space_or_tab(byte));
                    if !blank {
                        let length = self.chopped_length(line);
                        self.lines.set_len(line, length);
                    }
                    self.block_start(range.start)
                };
                let (content_start, internal_offset) = if setext {
                    (range.start, 0)
                } else {
                    let internal = self.atx_internal_offset(range.start);
                    (range.start + internal, internal)
                };
                self.push_frame(FrameKind::Heading { level, setext }, start, range.clone());
                let line = self.line_of(range.start);
                let mut context = self.new_inline_context(
                    content_start.min(self.lines.end(line)),
                    start.0,
                    start.1 - 1 + internal_offset as i64,
                    !setext,
                );
                if setext && self.definitions_before(range.start) != range.start {
                    self.lazy_first_line(&mut context, range.start);
                }
                self.inline = Some(context);
            }
            Tag::BlockQuote(_) => {
                let start = self.block_start(range.start);
                self.push_frame(FrameKind::BlockQuote, start, range.clone());
                self.prefixes.push(Prefix::BlockQuote);
                let line = self.line(self.line_of(range.start));
                self.gap_from = line.match_prefixes(&self.prefixes).0.offset;
            }
            Tag::List(first) => {
                let marker = self.skip_whitespace(range.start);
                let start = self.block_start(marker);
                self.push_frame(FrameKind::List { start: first }, start, range);
            }
            Tag::Item => {
                let marker = self.skip_whitespace(range.start);
                let start = self.block_start(marker);
                let first_line = self.line_of(marker);
                let line = self.line(first_line);
                let (scan, _) = line.match_prefixes(&self.prefixes);
                let (width, content) = line.item_width(scan);
                self.gap_from = content.offset;
                self.push_frame(
                    FrameKind::Item {
                        checkbox: None,
                        first_line,
                    },
                    start,
                    range,
                );
                self.prefixes.push(Prefix::Item { width, first_line });
            }
            Tag::CodeBlock(kind) => {
                let start = match kind {
                    CodeBlockKind::Fenced(_) => self.block_start(range.start),
                    CodeBlockKind::Indented => {
                        // `S_advance_offset(CODE_INDENT, columns)`: a partly
                        // consumed tab leaves the offset on the tab.
                        let line_index = self.line_of(range.start);
                        let line = self.line(line_index);
                        let (mut scan, _) = line.match_prefixes(&self.prefixes);
                        line.advance(&mut scan, 4, true);
                        self.block_start(scan.offset)
                    }
                };
                let (fenced, info) = match kind {
                    CodeBlockKind::Fenced(info) => {
                        let fence = self.bytes[range.start];
                        let length = self.bytes[range.start..]
                            .iter()
                            .take_while(|&&byte| byte == fence)
                            .count();
                        (Some((fence, length)), Some(info))
                    }
                    CodeBlockKind::Indented => (None, None),
                };
                self.literal.clear();
                self.code_text_end = range.start;
                self.push_frame(FrameKind::CodeBlock { fenced, info }, start, range);
            }
            Tag::HtmlBlock => {
                let start = self.block_start(range.start);
                self.literal.clear();
                // cmark keeps the first line's indentation in the literal.
                // pulldown-cmark reports it as text when a tab is split.
                // `add_line` turns the rest of a tab the prefix partly
                // consumed into spaces.
                let line = self.line(self.line_of(range.start));
                let (scan, _) = line.match_prefixes(&self.prefixes);
                let mut indentation = String::new();
                let mut from = scan.offset;
                if scan.partially_consumed_tab {
                    indentation.push_str(&" ".repeat(4 - scan.column % 4));
                    from += 1;
                }
                if from < range.start
                    && self.bytes[from..range.start]
                        .iter()
                        .all(|&byte| is_space_or_tab(byte))
                {
                    indentation.push_str(&self.text[from..range.start]);
                }
                self.html_indentation = Some(indentation);
                self.push_frame(FrameKind::HtmlBlock, start, range);
            }
            Tag::Table(_) => self.start_table(range),
            Tag::TableHead => self.start_table_row(range, true),
            Tag::TableRow => self.start_table_row(range, false),
            Tag::TableCell => self.start_table_cell(range),
            Tag::FootnoteDefinition(_)
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::MetadataBlock(_)
            | Tag::Superscript
            | Tag::Subscript => unreachable!("pulldown-cmark option not enabled"),
            Tag::Emphasis
            | Tag::Strong
            | Tag::Strikethrough
            | Tag::Link { .. }
            | Tag::Image { .. } => {
                unreachable!()
            }
        }
    }

    /// The first byte at or after `offset` that is not whitespace (a list's
    /// range can start at the line ending before its first marker).
    fn skip_whitespace(&self, mut offset: usize) -> usize {
        while offset < self.bytes.len()
            && matches!(self.bytes[offset], b' ' | b'\t' | b'\n' | b'\r')
        {
            offset += 1;
        }
        offset
    }

    fn is_atx_heading(&self, offset: usize) -> bool {
        let hashes = self.bytes[offset..]
            .iter()
            .take_while(|&&byte| byte == b'#')
            .count();
        (1..=6).contains(&hashes)
            && matches!(
                self.bytes.get(offset + hashes),
                None | Some(b' ' | b'\t' | b'\n' | b'\r')
            )
    }

    /// The length of an ATX heading's line after `chop_trailing_hashtags`
    /// trims it in place: trailing whitespace, then a closing sequence of
    /// `#`s after a space, then whitespace again. cmark's `last_line_length`
    /// for that line is this length.
    fn chopped_length(&self, line: usize) -> usize {
        fn is_cmark_space(byte: u8) -> bool {
            matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C)
        }
        let text = &self.bytes[self.lines.start(line)..self.lines.end(line)];
        let mut length = text.len();
        while length > 0 && is_cmark_space(text[length - 1]) {
            length -= 1;
        }
        let trimmed = length;
        let mut position = length;
        while position > 0 && text[position - 1] == b'#' {
            position -= 1;
        }
        if position != trimmed && position > 0 && is_space_or_tab(text[position - 1]) {
            length = position - 1;
            while length > 0 && is_cmark_space(text[length - 1]) {
                length -= 1;
            }
        }
        length
    }

    /// `scan_atx_heading_start`'s match length: the `#`s and the whitespace
    /// after them (or the line ending).
    fn atx_internal_offset(&self, offset: usize) -> usize {
        let hashes = self.bytes[offset..]
            .iter()
            .take_while(|&&byte| byte == b'#')
            .count();
        let after = offset + hashes;
        match self.bytes.get(after) {
            Some(b' ' | b'\t') => {
                let spaces = self.bytes[after..]
                    .iter()
                    .take_while(|&&byte| is_space_or_tab(byte))
                    .count();
                hashes + spaces
            }
            _ => hashes + 1,
        }
    }

    fn start_inline(&mut self, tag: Tag<'a>, range: Range<usize>) {
        if self.skip_inlines {
            return;
        }
        self.ensure_inline_container(range.start);
        self.flush_text(None);
        let line = self.inline_line();
        let (kind, start_offset, content_start) = match tag {
            Tag::Emphasis => (
                FrameKind::Emphasis,
                self.run_start(range.start),
                range.start + 1,
            ),
            Tag::Strong => (
                FrameKind::Strong,
                self.run_start(range.start),
                range.start + 2,
            ),
            Tag::Strikethrough => (FrameKind::Strikethrough, range.start, range.start + 1),
            Tag::Link {
                link_type: LinkType::InlineAttributes,
                dest_url,
                ..
            } => (
                FrameKind::Attributes {
                    attributes: dest_url,
                },
                range.start,
                range.start + 2,
            ),
            Tag::Link {
                link_type,
                dest_url,
                title,
                ..
            } => {
                let destination = match link_type {
                    LinkType::Email => Some(CowStr::from(format!(
                        "mailto:{}",
                        decode_entities(trim_cmark_space(&dest_url))
                    ))),
                    LinkType::Autolink => {
                        Some(CowStr::from(decode_entities(trim_cmark_space(&dest_url))))
                    }
                    _ => Some(dest_url),
                };
                (
                    FrameKind::Link {
                        destination,
                        title,
                        autolink: matches!(link_type, LinkType::Autolink | LinkType::Email),
                    },
                    range.start,
                    range.start + 1,
                )
            }
            Tag::Image {
                dest_url, title, ..
            } => (
                FrameKind::Image {
                    source: dest_url,
                    title,
                },
                range.start,
                range.start + 2,
            ),
            _ => unreachable!(),
        };
        let column = self.column(start_offset);
        self.push_frame(kind, (line, column), range);
        self.inline.as_mut().unwrap().previous_end = content_start;
    }

    // MARK: End events

    fn end(&mut self, tag: TagEnd, range: Range<usize>) {
        if matches!(tag, TagEnd::BlockQuote(_) | TagEnd::Item | TagEnd::List(_)) {
            // Definitions after the container's last block, inside it: the
            // whole lines before the one pulldown-cmark's range ends in (a
            // list's range can end inside the next line's indentation).
            self.close_synthetic_paragraph();
            let to = if range.end >= self.bytes.len() {
                self.lines.count()
            } else {
                self.line_of(range.end)
            };
            self.scan_gap(to);
        }
        let block = !matches!(
            tag,
            TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Strikethrough
                | TagEnd::Link
                | TagEnd::Image
                | TagEnd::TableHead
                | TagEnd::TableRow
                | TagEnd::TableCell
        );
        match tag {
            TagEnd::Emphasis
            | TagEnd::Strong
            | TagEnd::Strikethrough
            | TagEnd::Link
            | TagEnd::Image => self.end_inline(tag, range.clone()),
            TagEnd::Paragraph => self.close_paragraph(range.end),
            TagEnd::Heading(_) => self.end_heading(range.clone()),
            TagEnd::BlockQuote(_) => {
                self.close_synthetic_paragraph();
                self.discard_closed_paragraph();
                self.prefixes.pop();
                let frame = self.frames.pop().expect("a block quote frame");
                self.container = frame.outer_container;
                let first_line = self.line_of(frame.range.start);
                let last_line = self.last_content_line(&frame.range);
                let n = self.after(last_line);
                let node = self.container(&frame, RawMarkupData::BlockQuote, None);
                self.finish_block(node, Some(frame.start), first_line, n, false);
                self.record(
                    node,
                    first_line,
                    Some(frame.start),
                    frame.range,
                    EndRule::Done { n },
                );
                self.children.push(node);
            }
            TagEnd::List(_) => {
                self.close_synthetic_paragraph();
                self.discard_closed_paragraph();
                let frame = self.frames.pop().expect("a list frame");
                let FrameKind::List { start } = frame.kind else {
                    unreachable!()
                };
                let data = match start {
                    Some(start_index) => RawMarkupData::OrderedList { start_index },
                    None => RawMarkupData::UnorderedList,
                };
                let first_line = self.line_of(self.skip_whitespace(frame.range.start));
                // pulldown-cmark's list can run over definitions after its
                // last item; the list's content ends with that item.
                let content_end = self
                    .children
                    .last()
                    .filter(|_| self.children.len() > frame.children_start)
                    .and_then(|&last| self.records.iter().rev().find(|record| record.node == last))
                    .map_or(frame.range.end, |record| record.range_end);
                let node = self.container(&frame, data, None);
                self.record(
                    node,
                    first_line,
                    Some(frame.start),
                    frame.range.start..content_end,
                    EndRule::Absorbing { fenced: false },
                );
                self.children.push(node);
            }
            TagEnd::Item => {
                self.close_synthetic_paragraph();
                self.discard_closed_paragraph();
                let width = match self.prefixes.pop() {
                    Some(Prefix::Item { width, .. }) => width,
                    _ => 0,
                };
                let frame = self.frames.pop().expect("an item frame");
                let item_container = self.container;
                self.container = frame.outer_container;
                let FrameKind::Item {
                    checkbox,
                    first_line,
                } = frame.kind
                else {
                    unreachable!()
                };
                // A definition is a paragraph child until cmark finalizes it,
                // at the blank line after it: from then on the item is empty.
                let no_children = self.children.len() == frame.children_start;
                let last_definition = self
                    .definitions
                    .last()
                    .filter(|definition| definition.2 == item_container)
                    .map(|definition| definition.0);
                let empty = no_children && last_definition.is_none();
                let node = self.container(&frame, RawMarkupData::ListItem { checkbox }, None);
                // An item with nothing after its marker continues only over
                // blank lines indented at least as far as its content
                // (`parse_node_item_prefix` needs a child to accept a less
                // indented blank line).
                let after_definitions = last_definition
                    .filter(|_| no_children)
                    .map(|line| line + 1)
                    .filter(|&line| {
                        line < self.lines.count() && {
                            let text_line = self.line(line);
                            let (scan, all_matched) = text_line.match_prefixes(&self.prefixes);
                            all_matched && text_line.first_nonspace(&scan).blank
                        }
                    });
                let rule = if empty {
                    let n = self.empty_item_end(first_line, width);
                    self.finish_block(node, Some(frame.start), first_line, n, false);
                    EndRule::Done { n }
                } else if let Some(blank) = after_definitions {
                    let n = self.empty_item_end(blank, width);
                    self.finish_block(node, Some(frame.start), first_line, n, false);
                    EndRule::Done { n }
                } else {
                    EndRule::Absorbing { fenced: false }
                };
                self.record(node, first_line, Some(frame.start), frame.range, rule);
                self.children.push(node);
            }
            TagEnd::CodeBlock => self.end_code_block(range.clone()),
            TagEnd::HtmlBlock => self.end_html_block(),
            TagEnd::Table => self.end_table(),
            TagEnd::TableHead | TagEnd::TableRow => self.end_table_row(),
            TagEnd::TableCell => self.end_table_cell(),
            TagEnd::FootnoteDefinition
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::MetadataBlock(_)
            | TagEnd::Superscript
            | TagEnd::Subscript => unreachable!("pulldown-cmark option not enabled"),
        }
        if block {
            self.gap_from = self.gap_from.max(range.end);
        }
    }

    /// Where an item with no content is finalized: at the first line after
    /// its marker that is not blank with at least `width` columns of
    /// indentation.
    fn empty_item_end(&self, first_line: usize, width: usize) -> Finalize {
        let mut line_index = first_line + 1;
        while line_index < self.lines.count() {
            let line = self.line(line_index);
            let (scan, all_matched) = line.match_prefixes(&self.prefixes);
            let first = line.first_nonspace(&scan);
            if !(all_matched && first.blank && first.indent >= width) {
                return Finalize::Line(line_index);
            }
            line_index += 1;
        }
        Finalize::Eof
    }

    fn end_inline(&mut self, tag: TagEnd, range: Range<usize>) {
        if self.skip_inlines {
            return;
        }
        self.flush_text(None);
        let frame = self.frames.pop().expect("an inline frame");
        let (start_line, start_column) = frame.start;
        let line = self.inline_line();
        let (end_line, end_column) = match tag {
            TagEnd::Emphasis | TagEnd::Strong => {
                let end = self.run_end(range.end - 1);
                (line, self.column(end - 1))
            }
            // The strikethrough node is the opener's text node: it keeps the
            // opener's line.
            TagEnd::Strikethrough => (start_line, self.column(range.end - 1)),
            TagEnd::Link if matches!(frame.kind, FrameKind::Attributes { .. }) => {
                let subject = self.subject(range.end);
                let context = self.inline.as_ref().unwrap();
                (line, subject - context.base + context.block_offset)
            }
            _ => {
                let end = range.end;
                // `subj->pos + subj->column_offset + subj->block_offset`.
                let subject = self.subject(end);
                let context = self.inline.as_ref().unwrap();
                (line, subject - context.base + context.block_offset)
            }
        };
        // `handle_close_bracket_attribute` puts an attribute span on the line
        // of its closing bracket.
        let start_line = if matches!(frame.kind, FrameKind::Attributes { .. }) {
            end_line
        } else {
            start_line
        };
        let parsed_range = cmark_range(start_line, start_column, end_line, end_column, 0);
        let data = match frame.kind {
            FrameKind::Attributes { ref attributes } => RawMarkupData::InlineAttributes {
                attributes: self.arena.push_str(attributes),
            },
            FrameKind::Emphasis => RawMarkupData::Emphasis,
            FrameKind::Strong => RawMarkupData::Strong,
            FrameKind::Strikethrough => RawMarkupData::Strikethrough,
            FrameKind::Link {
                ref destination,
                ref title,
                ..
            } => {
                // pulldown-cmark cleans destinations as `cmark_clean_url`
                // does (trimmed before entities are decoded).
                let destination = destination
                    .as_deref()
                    .filter(|destination| !destination.is_empty())
                    .map(|destination| self.arena.push_str(destination));
                let title = (!title.is_empty()).then(|| self.arena.push_str(title));
                RawMarkupData::Link { destination, title }
            }
            FrameKind::Image {
                ref source,
                ref title,
                ..
            } => {
                let source = Some(source.as_ref())
                    .filter(|source| !source.is_empty())
                    .map(|source| self.arena.push_str(source));
                let title = (!title.is_empty()).then(|| self.arena.push_str(title));
                RawMarkupData::Image { source, title }
            }
            _ => unreachable!(),
        };
        let node = self.container(&frame, data, parsed_range);
        self.children.push(node);
        self.inline.as_mut().unwrap().previous_end = range.end;
        self.note_inline_end(range.end);
    }

    fn end_heading(&mut self, range: Range<usize>) {
        self.flush_text(None);
        self.drop_inline();
        let frame = self.frames.pop().expect("a heading frame");
        let FrameKind::Heading { level, setext } = frame.kind else {
            unreachable!()
        };
        let first_line = self.line_of(frame.range.start);
        let node = self.container(&frame, RawMarkupData::Heading { level }, None);
        let n = if setext {
            let underline = self.last_content_line(&range);
            let n = self.after(underline);
            self.finish_block(node, Some(frame.start), first_line, n, true);
            n
        } else {
            let n = self.after(first_line);
            self.finish_block(node, Some(frame.start), first_line, n, false);
            n
        };
        self.record(
            node,
            first_line,
            Some(frame.start),
            frame.range,
            EndRule::Done { n },
        );
        self.children.push(node);
    }

    /// A thematic break. `check_open_blocks` has no case for it, so like a
    /// list it stays open across blank lines until the next block.
    fn rule(&mut self, range: Range<usize>) {
        self.close_synthetic_paragraph();
        self.discard_closed_paragraph();
        self.scan_gap_before(range.start);
        self.gap_from = range.end;
        let start = self.block_start(range.start);
        let first_line = self.line_of(range.start);
        let node = self.arena.create(RawMarkupData::ThematicBreak, None, &[]);
        self.record(
            node,
            first_line,
            Some(start),
            range,
            EndRule::Absorbing { fenced: false },
        );
        self.children.push(node);
    }

    fn end_code_block(&mut self, range: Range<usize>) {
        let first_line = self.line_of(range.start);
        let unclosed_fence_at_end = self.frames.last().is_some_and(|frame| {
            matches!(
                frame.kind,
                FrameKind::CodeBlock {
                    fenced: Some(_),
                    ..
                }
            )
        }) && range.end == self.bytes.len()
            && !self.bytes.ends_with(b"\n")
            && !self.bytes.ends_with(b"\r")
            && self.lines.count() - 1 > first_line
            && self.code_text_end <= self.lines.start(self.lines.count() - 1);
        if unclosed_fence_at_end {
            // A last line without a line ending that is blank once the
            // fence's indentation is stripped: pulldown-cmark leaves it out,
            // cmark keeps it as a line of the block.
            let last = self.lines.count() - 1;
            let line = self.line(last);
            let (mut scan, all_matched) = line.match_prefixes(&self.prefixes);
            let opening = self.line(first_line);
            let (opening_scan, _) = opening.match_prefixes(&self.prefixes);
            let mut fence_offset = range.start.saturating_sub(opening_scan.offset);
            while fence_offset > 0 && is_space_or_tab(line.peek(scan.offset)) {
                line.advance(&mut scan, 1, true);
                fence_offset -= 1;
            }
            let end = self.lines.end(last);
            // `add_line` turns the rest of a partly consumed tab into spaces.
            let (lead, content_start) = if scan.partially_consumed_tab {
                (cmark_lines::tab_remainder(&scan) as usize, scan.offset + 1)
            } else {
                (0, scan.offset)
            };
            let content = &self.text[content_start.min(end)..end];
            if all_matched && content.bytes().all(is_space_or_tab) {
                if !self.literal.is_empty() && !self.literal.ends_with('\n') {
                    self.literal.push('\n');
                }
                self.literal.extend(std::iter::repeat_n(' ', lead));
                self.literal.push_str(content);
                self.literal.push('\n');
            }
        }
        let frame = self.frames.pop().expect("a code block frame");
        let FrameKind::CodeBlock { fenced, ref info } = frame.kind else {
            unreachable!()
        };
        let code = self.push_literal();
        // pulldown-cmark cleans the info string as cmark's `finalize` does.
        let language = info
            .as_deref()
            .filter(|info| !info.is_empty())
            .map(|language| self.arena.push_str(language));
        let node = self
            .arena
            .create(RawMarkupData::CodeBlock { code, language }, None, &[]);
        let rule = match fenced {
            Some((fence, length)) => {
                let last = self.line_of(range.end.saturating_sub(1).max(range.start));
                if last > first_line && self.is_closing_fence(last, fence, length) {
                    let n = Finalize::Line(last);
                    self.finish_block(node, Some(frame.start), first_line, n, true);
                    EndRule::Done { n }
                } else {
                    EndRule::Absorbing { fenced: true }
                }
            }
            None => EndRule::Absorbing { fenced: false },
        };
        self.record(node, first_line, Some(frame.start), frame.range, rule);
        self.children.push(node);
    }

    /// Stores the code or HTML block literal read so far: line endings as
    /// `\n`, and a final one (cmark gives every line one).
    fn push_literal(&mut self) -> crate::base::raw_markup::StrRef {
        let start = self.arena.strings.len();
        if self.literal.contains('\r') {
            let normalized = self.literal.replace("\r\n", "\n").replace('\r', "\n");
            self.arena.strings.push_str(&normalized);
        } else {
            self.arena.strings.push_str(&self.literal);
        }
        if self.arena.strings.len() > start && !self.arena.strings.ends_with('\n') {
            self.arena.strings.push('\n');
        }
        self.arena.str_since(start)
    }

    fn is_closing_fence(&self, line: usize, fence: u8, length: usize) -> bool {
        let text_line = self.line(line);
        let (scan, all_matched) = text_line.match_prefixes(&self.prefixes);
        if !all_matched {
            return false;
        }
        let first = text_line.first_nonspace(&scan);
        if first.indent > 3 {
            return false;
        }
        let mut position = first.offset;
        let mut count = 0;
        while text_line.peek(position) == fence {
            position += 1;
            count += 1;
        }
        if count < 3 || count < length {
            return false;
        }
        while is_space_or_tab(text_line.peek(position)) {
            position += 1;
        }
        matches!(text_line.peek(position), b'\n' | b'\r')
    }

    fn html(&mut self, html: CowStr<'a>, _range: Range<usize>) {
        self.flush_html_indentation();
        self.literal.push_str(&html);
    }

    fn flush_html_indentation(&mut self) {
        if let Some(indentation) = self.html_indentation.take() {
            self.literal.push_str(&indentation);
        }
    }

    fn end_html_block(&mut self) {
        self.flush_html_indentation();
        let frame = self.frames.pop().expect("an HTML block frame");
        let first_line = self.line_of(frame.range.start);
        let literal = self.push_literal();
        let node = self
            .arena
            .create(RawMarkupData::HtmlBlock(literal), None, &[]);
        let kind = html_block_kind(&self.bytes[frame.range.start..]);
        let rule = if (1..=5).contains(&kind) {
            let last = self.line_of(frame.range.end.saturating_sub(1).max(frame.range.start));
            let closed = (first_line..=last).find(|&line| {
                let from = if line == first_line {
                    frame.range.start
                } else {
                    let text_line = self.line(line);
                    let (scan, _) = text_line.match_prefixes(&self.prefixes);
                    text_line.first_nonspace(&scan).offset
                };
                let to = self.lines.end(line);
                html_block_ends(kind, &self.bytes[from.min(to)..to])
            });
            match closed {
                Some(line) => {
                    let n = Finalize::Line(line);
                    self.finish_block(node, Some(frame.start), first_line, n, false);
                    EndRule::Done { n }
                }
                None => EndRule::Absorbing { fenced: false },
            }
        } else {
            let last = self.last_content_line(&frame.range);
            let n = self.after(last);
            self.finish_block(node, Some(frame.start), first_line, n, false);
            EndRule::Done { n }
        };
        self.record(node, first_line, Some(frame.start), frame.range, rule);
        self.children.push(node);
    }

    // MARK: Task list items

    /// pulldown-cmark reports a task only where cmark-gfm's tasklist
    /// extension finds one, checked as cmark checks it.
    fn task_list_marker(&mut self, checked: bool, range: Range<usize>) {
        let item_index = self
            .frames
            .iter()
            .rposition(|frame| matches!(frame.kind, FrameKind::Item { .. }))
            .expect("a task list marker inside an item");
        let FrameKind::Item { first_line, .. } = self.frames[item_index].kind else {
            unreachable!()
        };
        if self.line_of(range.start) == first_line {
            self.gap_from = self.gap_from.max(range.end);
        } else {
            // A lazy line made the item a task, and cmark skipped the 3 bytes
            // in `range` before taking that line as text.
            self.task_skips.push(range);
        }
        if let FrameKind::Item { checkbox, .. } = &mut self.frames[item_index].kind {
            *checkbox = Some(if checked {
                Checkbox::Checked
            } else {
                Checkbox::Unchecked
            });
        }
    }

    // MARK: Tables

    fn row_string(&self, start: usize) -> Vec<u8> {
        let line = self.line_of(start);
        let end = self.lines.end(line);
        let mut string = Vec::with_capacity(end - start + 1);
        string.extend_from_slice(&self.bytes[start..end]);
        string.push(b'\n');
        string
    }

    fn start_table(&mut self, range: Range<usize>) {
        let header_line = self.line_of(range.start);
        // A table right after a paragraph line is cmark's table interrupting
        // that paragraph: the header row is found in the paragraph's text.
        let interrupted = self
            .closed_paragraph
            .take()
            .filter(|paragraph| paragraph.last_line + 1 == header_line)
            .filter(|paragraph| self.children.last() == Some(&paragraph.node));
        let mut header_string = Vec::new();
        let start;
        let first_line;
        if let Some(paragraph) = &interrupted {
            header_string.extend_from_slice(&self.paragraph_content(paragraph));
            start = paragraph.start;
            first_line = paragraph.first_line;
            // cmark inserts a new paragraph without a position for the text
            // before the header, and parses its inlines as if the paragraph
            // started at line 0, column 0: their lines count from 0 and their
            // columns lose the paragraph's offset.
            self.arena.nodes[paragraph.node as usize].parsed_range = None;
            // cmark also unescapes `\|` in that text, shifting what follows
            // on the line: where each removed backslash was, in raw cmark
            // coordinates.
            let removed = self.unescaped_pipes(paragraph);
            let block_offset = paragraph.context.block_offset;
            // Removed backslashes before a raw column; an unshifted end is
            // measured without the block offset.
            let before = |line: i64, column: i64, unshifted: bool| {
                let position = if unshifted {
                    column
                } else {
                    column - 1 - block_offset
                };
                removed
                    .iter()
                    .filter(|&&(removed_line, removed_position)| {
                        removed_line == line && removed_position < position
                    })
                    .count() as i64
            };
            for id in paragraph.first_node..paragraph.node {
                let unshifted_end = self.unshifted_ends.contains(&id);
                let ticks = self
                    .code_ticks
                    .binary_search_by_key(&id, |&(node, _)| node)
                    .map_or(0, |index| self.code_ticks[index].1);
                let tilde = self
                    .tilde_texts
                    .iter()
                    .find(|&&(node, _, _)| node == id)
                    .map(|&(_, line, column)| (line, column));
                if let Some((text_line, text_column)) = tilde {
                    // The unset end stays 0, but the shifted start may now
                    // come before it.
                    let (line, column) = paragraph.start;
                    self.arena.nodes[id as usize].parsed_range = cmark_range(
                        text_line - line,
                        text_column - column - before(text_line, text_column, false),
                        text_line - line,
                        0,
                        0,
                    );
                    continue;
                }
                let shift = |range: SourceRange| {
                    let start = before(
                        range.lower_bound.line,
                        range.lower_bound.column + ticks,
                        false,
                    );
                    let end = before(
                        range.upper_bound.line,
                        range.upper_bound.column - 1 - ticks,
                        unshifted_end,
                    );
                    (start, end)
                };
                let shifts = self.arena.nodes[id as usize].parsed_range.map(shift);
                let node = &mut self.arena.nodes[id as usize];
                node.parsed_range = node.parsed_range.and_then(|range| {
                    let (line, column) = paragraph.start;
                    let (start_shift, end_shift) = shifts.unwrap_or((0, 0));
                    // Back to cmark's raw positions, shifted.
                    let start_column = range.lower_bound.column + ticks - column - start_shift;
                    let mut end_column = range.upper_bound.column - 1 - ticks - end_shift;
                    if !unshifted_end {
                        end_column -= column;
                    }
                    cmark_range(
                        range.lower_bound.line - line,
                        start_column,
                        range.upper_bound.line - line,
                        end_column,
                        ticks,
                    )
                });
            }
            if let Some(record) = self
                .records
                .iter_mut()
                .rev()
                .find(|record| record.node == paragraph.node)
            {
                record.start = None;
            }
        } else {
            // The table is the paragraph node cmark started, which keeps its
            // start when definitions at its start were resolved (at a setext
            // underline that then became the header).
            let paragraph_start = self.definitions_before(range.start);
            start = self.block_start(paragraph_start);
            first_line = self.line_of(paragraph_start);
        }
        // A lazy header line whose tab the containers partly consumed starts
        // with the rest of the tab as spaces.
        let (scan, all_matched) = self.line(header_line).match_prefixes(&self.prefixes);
        if !all_matched && scan.partially_consumed_tab && scan.offset + 1 == range.start {
            let lead = cmark_lines::tab_remainder(&scan) as usize;
            header_string.extend(std::iter::repeat_n(b' ', lead));
        }
        let header_line_start = header_string.len();
        header_string.extend_from_slice(&self.row_string(range.start));

        // The delimiter row: the next line, from its first non-space
        // character after the containers' prefixes.
        let delimiter_line = (header_line + 1).min(self.lines.count().saturating_sub(1));
        let line = self.line(delimiter_line);
        let (scan, _) = line.match_prefixes(&self.prefixes);
        let delimiter_start = line.first_nonspace(&scan).offset;
        let delimiter_string = self.row_string(delimiter_start.min(self.lines.end(delimiter_line)));
        let delimiter = cmark_table::row_from_string(&delimiter_string).unwrap_or_default();
        let alignments = cmark_table::alignments(&delimiter_string, &delimiter);
        let header = cmark_table::row_from_string(&header_string).unwrap_or_default();
        let columns = header.cells.len();

        let state = TableState {
            start,
            first_line,
            columns,
            alignments,
            rows: Vec::new(),
            header: None,
            body_rows: Vec::new(),
            first_body_range: None,
            header_string,
            header_line_start,
        };
        self.push_frame(FrameKind::Table(Box::new(state)), start, range);
    }

    fn table_state(&mut self) -> &mut TableState {
        let index = self
            .frames
            .iter()
            .rposition(|frame| matches!(frame.kind, FrameKind::Table(_)))
            .expect("a table frame");
        match &mut self.frames[index].kind {
            FrameKind::Table(state) => state,
            _ => unreachable!(),
        }
    }

    fn start_table_row(&mut self, range: Range<usize>, header: bool) {
        let line = self.line_of(range.start);
        let row_string = if header {
            None
        } else {
            Some(self.row_string(range.start))
        };
        let state = self.table_state();
        let (row, string_base) = match &row_string {
            None => (
                cmark_table::row_from_string(&state.header_string).unwrap_or_default(),
                state.header_line_start,
            ),
            Some(string) => (cmark_table::row_from_string(string).unwrap_or_default(), 0),
        };
        let columns = state.columns;
        let mut cells: Vec<CellState> = (0..columns)
            .map(|index| match row.cells.get(index) {
                Some(cell) => CellState {
                    node: None,
                    rowspan: cell.rowspan,
                    colspan: cell.colspan,
                    autocompleted: false,
                    cleared: false,
                },
                None => CellState {
                    node: None,
                    rowspan: 1,
                    colspan: 1,
                    autocompleted: true,
                    cleared: false,
                },
            })
            .collect();
        let mut updates = Vec::new();
        if !header {
            // A row-span marker extends the nearest cell above that is not a
            // marker itself (`try_opening_table_row`), and loses its content.
            for index in 0..row.cells.len().min(columns) {
                if row.cells[index].rowspan != 0 {
                    continue;
                }
                if let Some(previous) = state
                    .rows
                    .iter_mut()
                    .rev()
                    .find(|previous| previous.cells[index].rowspan != 0)
                {
                    let spanning = &mut previous.cells[index];
                    if !spanning.autocompleted {
                        spanning.rowspan += 1;
                        if let Some(node) = spanning.node {
                            updates.push((node, spanning.colspan, spanning.rowspan));
                        }
                    }
                    cells[index].cleared = true;
                }
            }
        }
        state.rows.push(TableRowState {
            row,
            source_base: range.start,
            string_base,
            line,
            cells,
            next_cell: 0,
        });
        let start = if header {
            state.start
        } else {
            (line as i64 + 1, state.start.1)
        };
        for (node, colspan, rowspan) in updates {
            self.arena.nodes[node as usize].data = RawMarkupData::TableCell { colspan, rowspan };
        }
        let kind = if header {
            FrameKind::TableHead
        } else {
            FrameKind::TableRow
        };
        self.push_frame(kind, start, range);
    }

    fn start_table_cell(&mut self, range: Range<usize>) {
        let header_row = self.top_is(|kind| matches!(kind, FrameKind::TableHead));
        let state = self.table_state();
        let table_start = state.start;
        let row = state.rows.last_mut().expect("a table row");
        let index = row.next_cell;
        row.next_cell += 1;
        let cell_state = row.cells.get(index).copied();
        let cell = row.row.cells.get(index).cloned();
        let source_base = row.source_base;
        let string_base = row.string_base;
        let row_line = row.line;

        let mut start = (0, 0);
        let mut end_column = 0;
        self.drop_inline();
        self.skip_inlines = true;
        if let (Some(cell), Some(cell_state)) = (cell, cell_state)
            && !cell_state.autocompleted
        {
            let line = if header_row {
                table_start.0
            } else {
                row_line as i64 + 1
            };
            let start_column = table_start.1 + cell.start_offset as i64;
            start = (line, start_column);
            end_column = table_start.1 + cell.end_offset as i64;
            self.skip_inlines = cell_state.cleared;
            // The source offset of a row-string offset.
            let to_source = |offset: usize| source_base + offset.saturating_sub(string_base);
            let content_end = to_source(cell.content_end).min(self.bytes.len());
            // cmark parses the trimmed content (the first cell of a lazy
            // header line can start with its indentation).
            let mut content_start = to_source(cell.content_start);
            while content_start < content_end
                && matches!(self.bytes[content_start], b' ' | b'\t' | 0x0B | 0x0C)
            {
                content_start += 1;
            }
            let mut context = self.new_inline_context(
                content_start,
                line,
                start_column - 1 + cell.internal_offset as i64,
                true,
            );
            for position in content_start..content_end {
                if self.bytes[position] == b'\\' && self.bytes.get(position + 1) == Some(&b'|') {
                    context.removed.push(position);
                }
            }
            self.inline = Some(context);
        }
        self.push_frame(FrameKind::TableCell { end_column }, start, range);
    }

    fn end_table_cell(&mut self) {
        self.flush_text(None);
        self.drop_inline();
        self.skip_inlines = false;
        let frame = self.frames.pop().expect("a table cell frame");
        let FrameKind::TableCell { end_column } = frame.kind else {
            unreachable!()
        };
        let state = self.table_state();
        let row = state.rows.last().expect("a table row");
        let index = row.next_cell - 1;
        let columns = state.columns;
        let (colspan, rowspan) = row
            .cells
            .get(index)
            .map_or((1, 1), |cell| (cell.colspan, cell.rowspan));
        let range = cmark_range(frame.start.0, frame.start.1, frame.start.0, end_column, 0);
        let node = self.container(&frame, RawMarkupData::TableCell { colspan, rowspan }, range);
        let state = self.table_state();
        let row = state.rows.last_mut().unwrap();
        if let Some(cell) = row.cells.get_mut(index) {
            cell.node = Some(node);
        }
        if index < columns {
            self.children.push(node);
        }
    }

    fn end_table_row(&mut self) {
        let frame = self.frames.pop().expect("a table row frame");
        let header = matches!(frame.kind, FrameKind::TableHead);
        let state = self.table_state();
        let row_line = state.rows.last().map_or(0, |row| row.line);
        let range = if header {
            let end_column = state.start.1 + state.header_string.len() as i64 - 2;
            cmark_range(state.start.0, state.start.1, state.start.0, end_column, 0)
        } else {
            let length = self.lines.len(row_line) as i64;
            cmark_range(frame.start.0, frame.start.1, row_line as i64 + 1, length, 0)
        };
        let node = if header {
            let id = self
                .arena
                .table_head(range, &self.children[frame.children_start..]);
            self.children.truncate(frame.children_start);
            id
        } else {
            let id = self
                .arena
                .table_row(range, &self.children[frame.children_start..]);
            self.children.truncate(frame.children_start);
            id
        };
        let state = self.table_state();
        if header {
            state.header = Some(node);
        } else {
            if state.body_rows.is_empty() {
                state.first_body_range = range;
            }
            state.body_rows.push(node);
        }
    }

    fn end_table(&mut self) {
        let frame = self.frames.pop().expect("a table frame");
        let FrameKind::Table(state) = frame.kind else {
            unreachable!()
        };
        let last_line = self.last_content_line(&frame.range);
        let n = self.after(last_line);
        let (end_line, end_column) = self.end_position(n, state.first_line, false);
        let table_range = cmark_range(state.start.0, state.start.1, end_line, end_column, 0);
        let header = match state.header {
            Some(header) => header,
            None => self.arena.table_head(None, &[]),
        };
        let body_range = state.first_body_range.map(|first| match table_range {
            Some(table_range) => SourceRange::new(first.lower_bound, table_range.upper_bound),
            None => first,
        });
        let body = self.arena.table_body(body_range, &state.body_rows);
        self.children.truncate(frame.children_start);
        let node = self.arena.table(
            &state.alignments[..state.alignments.len().min(state.columns)],
            table_range,
            header,
            body,
        );
        self.record(
            node,
            self.line_of(frame.range.start).min(state.first_line),
            Some(state.start),
            frame.range,
            EndRule::Done { n },
        );
        self.children.push(node);
    }
}

/// `cmark_chunk_trim`: leading and trailing `cmark_isspace` bytes.
fn trim_cmark_space(text: &str) -> &str {
    text.trim_matches(|character| {
        matches!(character, ' ' | '\t' | '\n' | '\r' | '\u{0B}' | '\u{0C}')
    })
}

/// Decodes HTML entities the way `houdini_unescape_html_f` does for an info
/// string or an autolink, using pulldown-cmark's own entity table (by
/// parsing the string's entities as Markdown text).
fn decode_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut decoded = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(ampersand) = rest.find('&') {
        decoded.push_str(&rest[..ampersand]);
        rest = &rest[ampersand..];
        let end = rest.find(';').filter(|&end| {
            end <= 33
                && rest[1..end]
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'#')
        });
        match end {
            Some(end) => {
                let entity = &rest[..=end];
                let mut replacement = None;
                for event in Parser::new_ext(entity, Options::ENABLE_CMARK_GFM_COMPAT) {
                    if let Event::Text(text) = event {
                        replacement.get_or_insert_with(String::new).push_str(&text);
                    }
                }
                decoded.push_str(replacement.as_deref().unwrap_or(entity));
                rest = &rest[end + 1..];
            }
            None => {
                decoded.push('&');
                rest = &rest[1..];
            }
        }
    }
    decoded.push_str(rest);
    decoded
}

/// `S_normalize_code`: line endings become spaces, then one leading and one
/// trailing space go if both are there and the span is not all spaces.
fn normalize_code(text: &str) -> String {
    let mut code = String::with_capacity(text.len());
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '\r' => {
                if characters.peek() != Some(&'\n') {
                    code.push(' ');
                }
            }
            '\n' => code.push(' '),
            other => code.push(other),
        }
    }
    let has_non_space = code.bytes().any(|byte| byte != b' ');
    if has_non_space && code.len() >= 2 && code.starts_with(' ') && code.ends_with(' ') {
        code[1..code.len() - 1].to_owned()
    } else {
        code
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// The HTML block kind (`scan_html_block_start`, kinds 1–6; 7 for anything
/// else pulldown-cmark accepted) of the block starting at `text`.
fn html_block_kind(text: &[u8]) -> u8 {
    fn starts_with_ignoring_case(text: &[u8], prefix: &[u8]) -> bool {
        text.len() >= prefix.len() && text[..prefix.len()].eq_ignore_ascii_case(prefix)
    }
    if text.first() != Some(&b'<') {
        return 7;
    }
    for tag in [&b"script"[..], b"pre", b"textarea", b"style"] {
        if starts_with_ignoring_case(&text[1..], tag) {
            let next = text.get(1 + tag.len()).copied();
            if matches!(
                next,
                Some(b' ' | b'\t' | 0x0B | 0x0C | b'\r' | b'\n' | b'>') | None
            ) {
                return 1;
            }
        }
    }
    if text.starts_with(b"<!--") {
        return 2;
    }
    if text.starts_with(b"<?") {
        return 3;
    }
    if text.len() >= 3 && text[1] == b'!' && text[2].is_ascii_uppercase() {
        return 4;
    }
    if starts_with_ignoring_case(text, b"<![CDATA[") {
        return 5;
    }
    6
}

/// `scan_html_block_end_1` … `_5`: whether a line contains the end marker.
fn html_block_ends(kind: u8, line: &[u8]) -> bool {
    match kind {
        1 => {
            let lower = line.to_ascii_lowercase();
            [&b"</script>"[..], b"</pre>", b"</textarea>", b"</style>"]
                .iter()
                .any(|end| contains(&lower, end))
        }
        2 => contains(line, b"-->"),
        3 => contains(line, b"?>"),
        4 => line.contains(&b'>'),
        5 => contains(line, b"]]>"),
        _ => false,
    }
}
