//! Model.swift — the document model.
//!
//! Every range is an [`NSRange`] of UTF-16 offsets into the document's text;
//! the render layer hands them straight to `NSTextStorage`.
//!
//! ## Representation
//!
//! `MDBlock` is a Swift class: the parser mutates blocks after building them
//! (identity, subtree hash, footnote retagging), `ParsedDocument.footnotes`
//! aliases blocks inside the tree, and the app keys caches on the root's
//! object identity. Upleft keeps that shape with [`BlockRef`] = `Arc<MDBlock>`:
//! children are `Vec<Arc<MDBlock>>`, the footnote index clones the `Arc`s, and
//! `Arc::as_ptr` stands in for `ObjectIdentifier`. The whole tree is `Send +
//! Sync` (parsing runs off the main thread) and sharing a subtree is a
//! reference-count bump. The parser mutates a block while it still owns it
//! uniquely (`Arc::get_mut` / `Arc::make_mut`); a published tree is never
//! mutated, which is how Downright uses it too. [`ParsedDocument`] is shared as
//! `Arc<ParsedDocument>` for the same reasons.

use std::collections::HashMap;
use std::sync::Arc;

use crate::ns_range::NSRange;
use crate::safe_html::SafeHTMLDocument;
use crate::swift_text::{self, ns::NSStringExt};

// MARK: - Blocks

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CalloutKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
    Info,
    Success,
    Question,
    Danger,
    Example,
    Quote,
    Abstract,
    Bug,
    Todo,
}

impl CalloutKind {
    pub const ALL_CASES: [CalloutKind; 14] = [
        CalloutKind::Note,
        CalloutKind::Tip,
        CalloutKind::Important,
        CalloutKind::Warning,
        CalloutKind::Caution,
        CalloutKind::Info,
        CalloutKind::Success,
        CalloutKind::Question,
        CalloutKind::Danger,
        CalloutKind::Example,
        CalloutKind::Quote,
        CalloutKind::Abstract,
        CalloutKind::Bug,
        CalloutKind::Todo,
    ];

    pub fn raw_value(&self) -> &'static str {
        match self {
            CalloutKind::Note => "note",
            CalloutKind::Tip => "tip",
            CalloutKind::Important => "important",
            CalloutKind::Warning => "warning",
            CalloutKind::Caution => "caution",
            CalloutKind::Info => "info",
            CalloutKind::Success => "success",
            CalloutKind::Question => "question",
            CalloutKind::Danger => "danger",
            CalloutKind::Example => "example",
            CalloutKind::Quote => "quote",
            CalloutKind::Abstract => "abstract",
            CalloutKind::Bug => "bug",
            CalloutKind::Todo => "todo",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<CalloutKind> {
        CalloutKind::ALL_CASES.into_iter().find(|kind| kind.raw_value() == raw)
    }

    /// `init?(token:)`: agents emit these in every casing imaginable.
    pub fn from_token(token: &str) -> Option<CalloutKind> {
        CalloutKind::from_raw_value(&swift_text::lowercased(token))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Checkbox {
    pub is_checked: bool,
    /// The single character between the brackets.
    pub mark_range: NSRange,
}

impl Checkbox {
    pub fn new(is_checked: bool, mark_range: NSRange) -> Checkbox {
        Checkbox { is_checked, mark_range }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TableAlignment {
    None,
    Left,
    Center,
    Right,
}

impl TableAlignment {
    pub fn raw_value(&self) -> &'static str {
        match self {
            TableAlignment::None => "none",
            TableAlignment::Left => "left",
            TableAlignment::Center => "center",
            TableAlignment::Right => "right",
        }
    }
}

#[derive(Clone, Debug)]
pub struct TableCell {
    pub range: NSRange,
    pub content_range: NSRange,
    pub inlines: Vec<InlineSpan>,
}

impl TableCell {
    pub fn new(range: NSRange, content_range: NSRange, inlines: Vec<InlineSpan>) -> TableCell {
        TableCell { range, content_range, inlines }
    }
}

#[derive(Clone, Debug)]
pub struct TableRow {
    pub range: NSRange,
    pub cells: Vec<TableCell>,
    pub is_header: bool,
}

impl TableRow {
    pub fn new(range: NSRange, cells: Vec<TableCell>, is_header: bool) -> TableRow {
        TableRow { range, cells, is_header }
    }
}

#[derive(Clone, Debug)]
pub struct TableData {
    pub rows: Vec<TableRow>,
    pub alignments: Vec<TableAlignment>,
    /// The `|---|:--:|` delimiter row.
    pub delimiter_range: NSRange,
}

impl TableData {
    pub fn new(rows: Vec<TableRow>, alignments: Vec<TableAlignment>, delimiter_range: NSRange) -> TableData {
        TableData { rows, alignments, delimiter_range }
    }

    pub fn header_row(&self) -> Option<&TableRow> {
        self.rows.iter().find(|row| row.is_header)
    }

    pub fn body_rows(&self) -> Vec<&TableRow> {
        self.rows.iter().filter(|row| !row.is_header).collect()
    }

    pub fn column_count(&self) -> isize {
        self.rows.iter().map(|row| row.cells.len() as isize).max().unwrap_or(0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrontMatterField {
    pub key: String,
    pub value: String,
    pub key_range: NSRange,
    pub value_range: NSRange,
}

impl FrontMatterField {
    pub fn new(key: impl Into<String>, value: impl Into<String>, key_range: NSRange, value_range: NSRange) -> Self {
        FrontMatterField { key: key.into(), value: value.into(), key_range, value_range }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrontMatter {
    pub fields: Vec<FrontMatterField>,
    /// Whole block including the `---` fences.
    pub range: NSRange,
    /// YAML body between the fences.
    pub body_range: NSRange,
}

impl FrontMatter {
    pub fn new(fields: Vec<FrontMatterField>, range: NSRange, body_range: NSRange) -> FrontMatter {
        FrontMatter { fields, range, body_range }
    }

    /// `subscript(_ key:)`: case-insensitive field lookup.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|field| swift_text::case_insensitive_equal(&field.key, key))
            .map(|field| field.value.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListMarkerStyle {
    Dash,
    Asterisk,
    Plus,
    Period,
    Paren,
}

impl ListMarkerStyle {
    pub fn raw_value(&self) -> &'static str {
        match self {
            ListMarkerStyle::Dash => "-",
            ListMarkerStyle::Asterisk => "*",
            ListMarkerStyle::Plus => "+",
            ListMarkerStyle::Period => ".",
            ListMarkerStyle::Paren => ")",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<ListMarkerStyle> {
        match raw {
            "-" => Some(ListMarkerStyle::Dash),
            "*" => Some(ListMarkerStyle::Asterisk),
            "+" => Some(ListMarkerStyle::Plus),
            "." => Some(ListMarkerStyle::Period),
            ")" => Some(ListMarkerStyle::Paren),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum BlockContent {
    Document,
    Heading { level: isize },
    Paragraph,
    BlockQuote,
    Callout { kind: CalloutKind, title: Option<String> },
    List { ordered: bool, start: isize, tight: bool, marker: ListMarkerStyle },
    ListItem { ordinal: Option<isize>, checkbox: Option<Checkbox> },
    /// `content_range` is the fence body; `language` the info string's first word.
    CodeBlock { language: Option<String>, is_fenced: bool, content_range: NSRange },
    Mermaid { source_range: NSRange },
    MathBlock { latex_range: NSRange },
    Table(TableData),
    ThematicBreak,
    HtmlBlock,
    FrontMatter(FrontMatter),
    FootnoteDefinition { identifier: String },
}

impl BlockContent {
    pub fn is_leaf_text(&self) -> bool {
        matches!(self, BlockContent::Paragraph | BlockContent::Heading { .. })
    }

    /// A nested list hanging off a list item.
    pub fn is_list(&self) -> bool {
        matches!(self, BlockContent::List { .. })
    }
}

/// A shared block reference (`MDBlock` is a class in Swift).
pub type BlockRef = Arc<MDBlock>;

/// A block-level node.
#[derive(Clone, Debug)]
pub struct MDBlock {
    pub content: BlockContent,
    /// Full source range, markers included.
    pub range: NSRange,
    /// The renderable content, markers excluded.
    pub content_range: NSRange,
    /// Leading block marker — `"## "`, `"> "`, `"- [ ] "`.
    pub marker_range: Option<NSRange>,
    /// Trailing marker, e.g. the closing fence of a code block.
    pub trailing_marker_range: Option<NSRange>,
    pub children: Vec<BlockRef>,
    /// Populated only for leaf text blocks.
    pub inlines: Vec<InlineSpan>,
    /// Nesting depth from the document root.
    pub depth: isize,
    /// Depth of blockquote nesting containing this block.
    pub quote_depth: isize,
    /// Subtree hash used by `ASTDiff` to find the dirty set (§3.5).
    pub subtree_hash: u64,
    /// Stable identity across reparses where possible.
    pub identity: BlockIdentity,
    /// Safe README-style HTML annotations for this block.
    pub safe_html: Option<SafeHTMLDocument>,
}

impl MDBlock {
    /// `init(content:range:contentRange:)` with every other field defaulted;
    /// set the rest with the `with_*` builders or directly.
    pub fn new(content: BlockContent, range: NSRange, content_range: NSRange) -> MDBlock {
        MDBlock {
            content,
            range,
            content_range,
            marker_range: None,
            trailing_marker_range: None,
            children: Vec::new(),
            inlines: Vec::new(),
            depth: 0,
            quote_depth: 0,
            subtree_hash: 0,
            identity: BlockIdentity::new(0, 0),
            safe_html: None,
        }
    }

    pub fn with_marker_range(mut self, range: Option<NSRange>) -> MDBlock {
        self.marker_range = range;
        self
    }

    pub fn with_trailing_marker_range(mut self, range: Option<NSRange>) -> MDBlock {
        self.trailing_marker_range = range;
        self
    }

    pub fn with_children(mut self, children: Vec<BlockRef>) -> MDBlock {
        self.children = children;
        self
    }

    pub fn with_inlines(mut self, inlines: Vec<InlineSpan>) -> MDBlock {
        self.inlines = inlines;
        self
    }

    pub fn with_depth(mut self, depth: isize) -> MDBlock {
        self.depth = depth;
        self
    }

    pub fn with_quote_depth(mut self, quote_depth: isize) -> MDBlock {
        self.quote_depth = quote_depth;
        self
    }

    pub fn with_identity(mut self, identity: BlockIdentity) -> MDBlock {
        self.identity = identity;
        self
    }

    pub fn into_ref(self) -> BlockRef {
        Arc::new(self)
    }

    /// Depth-first walk, self first.
    pub fn walk<'a>(self: &'a Arc<Self>, visit: &mut impl FnMut(&'a Arc<MDBlock>)) {
        visit(self);
        for child in &self.children {
            child.walk(visit);
        }
    }

    /// Depth-first walk that can prune subtrees by returning `false`.
    pub fn walk_pruning<'a>(self: &'a Arc<Self>, visit: &mut impl FnMut(&'a Arc<MDBlock>) -> bool) {
        if !visit(self) {
            return;
        }
        for child in &self.children {
            child.walk_pruning(visit);
        }
    }

    pub fn heading_level(&self) -> Option<isize> {
        match self.content {
            BlockContent::Heading { level } => Some(level),
            _ => None,
        }
    }

    /// Deepest block touching `offset`, preferring the leftmost sibling where
    /// two meet. Children are in source order and never overlap, so this
    /// binary searches for the leftmost child that can still reach `offset`.
    pub fn block_at(self: &Arc<Self>, offset: isize) -> Option<Arc<MDBlock>> {
        if !self.range.touches(offset) {
            return None;
        }
        let mut low = 0usize;
        let mut high = self.children.len();
        while low < high {
            let middle = (low + high) / 2;
            if self.children[middle].range.upper_bound() < offset {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        if low < self.children.len()
            && self.children[low].range.location <= offset
            && let Some(hit) = self.children[low].block_at(offset)
        {
            return Some(hit);
        }
        Some(self.clone())
    }

    pub fn flattened(self: &Arc<Self>) -> Vec<Arc<MDBlock>> {
        let mut out = Vec::new();
        self.walk(&mut |block| out.push(block.clone()));
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct BlockIdentity {
    /// Discriminator for the block's kind, so a heading never matches a table.
    pub kind: isize,
    /// Index among same-kind siblings.
    pub ordinal: isize,
}

impl BlockIdentity {
    pub const fn new(kind: isize, ordinal: isize) -> BlockIdentity {
        BlockIdentity { kind, ordinal }
    }
}

// MARK: - Inlines

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PathToken {
    pub raw_path: String,
    pub line: Option<isize>,
    pub column: Option<isize>,
}

impl PathToken {
    pub fn new(raw_path: impl Into<String>, line: Option<isize>, column: Option<isize>) -> PathToken {
        PathToken { raw_path: raw_path.into(), line, column }
    }
}

#[derive(Clone, Debug)]
pub enum InlineKind {
    Text,
    Emphasis,
    Strong,
    Strikethrough,
    InlineCode,
    Link { destination: String, title: Option<String> },
    Autolink { destination: String },
    Wikilink { target: String, label: Option<String> },
    Image { source: String, alt: String },
    InlineMath { latex_range: NSRange },
    PathToken(PathToken),
    FootnoteReference { identifier: String },
    SoftBreak,
    LineBreak,
    InlineHTML,
}

impl InlineKind {
    /// Whether the caret entering this span should reveal its markers (§6.1b).
    pub fn reveals_markers(&self) -> bool {
        !matches!(
            self,
            InlineKind::Text | InlineKind::SoftBreak | InlineKind::LineBreak | InlineKind::InlineHTML | InlineKind::PathToken(_)
        )
    }
}

#[derive(Clone, Debug)]
pub struct InlineSpan {
    pub kind: InlineKind,
    /// Whole span, markers included.
    pub range: NSRange,
    /// Inner content, markers excluded.
    pub content_range: NSRange,
    pub leading_marker_range: Option<NSRange>,
    pub trailing_marker_range: Option<NSRange>,
    pub children: Vec<InlineSpan>,
}

impl InlineSpan {
    pub fn new(kind: InlineKind, range: NSRange, content_range: NSRange) -> InlineSpan {
        InlineSpan { kind, range, content_range, leading_marker_range: None, trailing_marker_range: None, children: Vec::new() }
    }

    pub fn with_markers(mut self, leading: Option<NSRange>, trailing: Option<NSRange>) -> InlineSpan {
        self.leading_marker_range = leading;
        self.trailing_marker_range = trailing;
        self
    }

    pub fn with_children(mut self, children: Vec<InlineSpan>) -> InlineSpan {
        self.children = children;
        self
    }

    pub fn marker_ranges(&self) -> Vec<NSRange> {
        [self.leading_marker_range, self.trailing_marker_range].into_iter().flatten().collect()
    }

    pub fn walk<'a>(&'a self, visit: &mut impl FnMut(&'a InlineSpan)) {
        visit(self);
        for child in &self.children {
            child.walk(visit);
        }
    }

    /// Innermost span touching `offset`, used to decide what to reveal.
    pub fn span_touching(&self, offset: isize) -> Option<&InlineSpan> {
        if !self.range.touches(offset) {
            return None;
        }
        for child in &self.children {
            if let Some(hit) = child.span_touching(offset) {
                return Some(hit);
            }
        }
        Some(self)
    }
}

// MARK: - Document

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextEncodingKind {
    Utf8,
    Utf16LE,
    Utf16BE,
    Utf32LE,
    Utf32BE,
    Latin1,
}

impl TextEncodingKind {
    pub fn raw_value(&self) -> &'static str {
        match self {
            TextEncodingKind::Utf8 => "utf8",
            TextEncodingKind::Utf16LE => "utf16LE",
            TextEncodingKind::Utf16BE => "utf16BE",
            TextEncodingKind::Utf32LE => "utf32LE",
            TextEncodingKind::Utf32BE => "utf32BE",
            TextEncodingKind::Latin1 => "latin1",
        }
    }

    /// Width of one code unit in bytes, used to repair a file truncated
    /// mid-code-unit.
    pub fn code_unit_width(&self) -> usize {
        match self {
            TextEncodingKind::Utf16LE | TextEncodingKind::Utf16BE => 2,
            TextEncodingKind::Utf32LE | TextEncodingKind::Utf32BE => 4,
            _ => 1,
        }
    }
}

/// Byte-level facts about the file that must survive a round-trip untouched
/// (§3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ByteFidelity {
    pub encoding: TextEncodingKind,
    pub has_bom: bool,
    pub line_ending: LineEnding,
    pub has_trailing_newline: bool,
}

impl ByteFidelity {
    pub fn new(encoding: TextEncodingKind, has_bom: bool, line_ending: LineEnding, has_trailing_newline: bool) -> Self {
        ByteFidelity { encoding, has_bom, line_ending, has_trailing_newline }
    }

    pub const DEFAULT: ByteFidelity = ByteFidelity {
        encoding: TextEncodingKind::Utf8,
        has_bom: false,
        line_ending: LineEnding::Lf,
        has_trailing_newline: true,
    };
}

impl Default for ByteFidelity {
    fn default() -> Self {
        ByteFidelity::DEFAULT
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineEnding {
    Lf,
    Crlf,
    Cr,
}

impl LineEnding {
    pub fn raw_value(&self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
            LineEnding::Cr => "\r",
        }
    }
}

/// A parsed document: the text, the tree over it, and the extension-pass
/// results. Immutable once built; shared as `Arc<ParsedDocument>`.
#[derive(Debug)]
pub struct ParsedDocument {
    pub text: String,
    /// `text as NSString`: the UTF-16 code units every range indexes.
    pub utf16: Vec<u16>,
    /// UTF-16 length, cached.
    pub length: isize,
    pub root: BlockRef,
    pub front_matter: Option<FrontMatter>,
    pub headings: Vec<HeadingNode>,
    pub tasks: Vec<TaskItem>,
    pub path_tokens: Vec<ResolvableToken>,
    pub footnotes: HashMap<String, BlockRef>,
    pub link_references: HashMap<String, LinkReference>,
    /// Line start offsets, for `line:column` lookups.
    pub line_starts: Vec<isize>,
    /// Upleft extension: what the rest of a longer document gave this part
    /// of it (`MarkdownParser::parse_segment`); empty for a whole document.
    pub segment_context: SegmentContext,
}

/// Upleft extension, for hosts that show one long document as several
/// hosted views, each a part of it cut between top-level blocks: what the
/// rest of the document contributes to how a part parses, so the part parses
/// exactly as the same text does inside the whole document. Nothing of it is
/// in the part's text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SegmentContext {
    /// Link reference definitions from the rest of the document, each as its
    /// first line of source (`[label]: destination "title"`). References
    /// resolve against them as against definitions placed before the part,
    /// so one of them wins over the part's own definition of the same label.
    pub references: Vec<String>,
    /// Footnotes defined in the rest of the document: identifier and text.
    pub footnotes: Vec<(String, String)>,
    /// A real `<details>` opening tag, written as an HTML block, comes
    /// before the part: a closing tag in the part may pair with it.
    pub details_opened_before: bool,
    /// A `</details>` closing tag, written as an HTML block, comes after the
    /// part: an opening tag in the part may pair with it.
    pub details_closed_after: bool,
    /// Link reference labels (lowercased) and footnote identifiers the part
    /// defines that the rest defines again after it. A document keeps the
    /// last definition of a label in `link_references` and `footnotes`, so
    /// the part's own is not there: it is not hidden in Read mode.
    pub superseded_references: Vec<String>,
    pub superseded_footnotes: Vec<String>,
}

impl SegmentContext {
    pub fn is_empty(&self) -> bool {
        *self == SegmentContext::default()
    }
}

impl ParsedDocument {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        text: String,
        length: isize,
        root: BlockRef,
        front_matter: Option<FrontMatter>,
        headings: Vec<HeadingNode>,
        tasks: Vec<TaskItem>,
        path_tokens: Vec<ResolvableToken>,
        footnotes: HashMap<String, BlockRef>,
        link_references: HashMap<String, LinkReference>,
        line_starts: Vec<isize>,
    ) -> ParsedDocument {
        let utf16 = swift_text::ns::utf16(&text);
        Self::with_utf16(
            text,
            utf16,
            length,
            root,
            front_matter,
            headings,
            tasks,
            path_tokens,
            footnotes,
            link_references,
            line_starts,
        )
    }

    /// `init(…)` when the caller already holds the UTF-16 units of `text`.
    #[allow(clippy::too_many_arguments)]
    pub fn with_utf16(
        text: String,
        utf16: Vec<u16>,
        length: isize,
        root: BlockRef,
        front_matter: Option<FrontMatter>,
        headings: Vec<HeadingNode>,
        tasks: Vec<TaskItem>,
        path_tokens: Vec<ResolvableToken>,
        footnotes: HashMap<String, BlockRef>,
        link_references: HashMap<String, LinkReference>,
        line_starts: Vec<isize>,
    ) -> ParsedDocument {
        ParsedDocument {
            text,
            utf16,
            length,
            root,
            front_matter,
            headings,
            tasks,
            path_tokens,
            footnotes,
            link_references,
            line_starts,
            segment_context: SegmentContext::default(),
        }
    }

    /// `ParsedDocument.empty`: one shared instance, as Swift's `static let`.
    pub fn empty() -> Arc<ParsedDocument> {
        static EMPTY: std::sync::LazyLock<Arc<ParsedDocument>> = std::sync::LazyLock::new(ParsedDocument::make_empty);
        EMPTY.clone()
    }

    fn make_empty() -> Arc<ParsedDocument> {
        Arc::new(ParsedDocument::new(
            String::new(),
            0,
            MDBlock::new(BlockContent::Document, NSRange::new(0, 0), NSRange::new(0, 0)).into_ref(),
            None,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            HashMap::new(),
            HashMap::new(),
            vec![0],
        ))
    }

    /// 1-based line number containing `offset`.
    pub fn line_at(&self, offset: isize) -> isize {
        let (mut lo, mut hi, mut best) = (0isize, self.line_starts.len() as isize - 1, 0isize);
        while lo <= hi {
            let mid = (lo + hi) / 2;
            if self.line_starts[mid as usize] <= offset {
                best = mid;
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        }
        best + 1
    }

    /// Range of the 1-based line `line`, newline excluded.
    pub fn range_of_line(&self, line: isize) -> NSRange {
        let idx = line - 1;
        if !(idx >= 0 && (idx as usize) < self.line_starts.len()) {
            return NSRange::new(0, 0);
        }
        let start = self.line_starts[idx as usize];
        let end = if ((idx + 1) as usize) < self.line_starts.len() { self.line_starts[(idx + 1) as usize] } else { self.length };
        let ns = self.utf16.as_slice();
        let mut e = end;
        while e > start && e - 1 < ns.length() {
            let ch = ns.character_at(e - 1);
            if ch == 0x0A || ch == 0x0D {
                e -= 1;
            } else {
                break;
            }
        }
        NSRange::new(start, 0.max(e - start))
    }

    pub fn substring(&self, range: NSRange) -> String {
        if !(range.location >= 0 && range.upper_bound() <= self.length) {
            return String::new();
        }
        self.utf16.as_slice().substring(range)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinkReference {
    pub identifier: String,
    pub destination: String,
    pub title: Option<String>,
    pub range: NSRange,
}

impl LinkReference {
    pub fn new(identifier: String, destination: String, title: Option<String>, range: NSRange) -> LinkReference {
        LinkReference { identifier, destination, title, range }
    }
}

// MARK: - Derived structures

#[derive(Clone, Debug, PartialEq)]
pub struct HeadingNode {
    pub level: isize,
    pub title: String,
    /// Range of the heading line itself.
    pub range: NSRange,
    /// Range of the heading text without the `#` markers.
    pub content_range: NSRange,
    /// Heading plus every block beneath it until the next heading of the same
    /// or higher level (§9.2).
    pub section_range: NSRange,
    /// Index into `ParsedDocument.headings` of the parent heading.
    pub parent_index: Option<isize>,
    pub child_indices: Vec<isize>,
    /// GitHub-style anchor slug.
    pub slug: String,
    /// Words in this section excluding subsections (§9.6).
    pub word_count: isize,
}

impl HeadingNode {
    pub fn new(level: isize, title: impl Into<String>, range: NSRange, content_range: NSRange, section_range: NSRange) -> Self {
        HeadingNode {
            level,
            title: title.into(),
            range,
            content_range,
            section_range,
            parent_index: None,
            child_indices: Vec::new(),
            slug: String::new(),
            word_count: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TaskItem {
    pub is_checked: bool,
    /// Single character between the brackets.
    pub mark_range: NSRange,
    /// The task's text.
    pub content_range: NSRange,
    pub text: String,
    /// Index into `headings` of the nearest preceding heading.
    pub heading_index: Option<isize>,
    pub indent_level: isize,
}

impl TaskItem {
    pub fn new(
        is_checked: bool,
        mark_range: NSRange,
        content_range: NSRange,
        text: impl Into<String>,
        heading_index: Option<isize>,
        indent_level: isize,
    ) -> TaskItem {
        TaskItem { is_checked, mark_range, content_range, text: text.into(), heading_index, indent_level }
    }
}

/// A path-like token found by the extension pass, before resolution (§8.4).
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvableToken {
    pub token: PathToken,
    pub range: NSRange,
    /// True when the token came from an inline code span.
    pub from_code_span: bool,
}

impl ResolvableToken {
    pub fn new(token: PathToken, range: NSRange, from_code_span: bool) -> ResolvableToken {
        ResolvableToken { token, range, from_code_span }
    }
}
