//! The original port of `Sources/Markdown/Parser/CommonMarkConverter.swift`:
//! runs cmark-gfm (the C sources swift-markdown links) with the extensions
//! swift-markdown attaches and converts its node tree into markup, event by
//! event, exactly as `MarkupParser` does.
//!
//! Upleft parses with pulldown-cmark ([`super::common_mark_converter`]).
//! This converter is kept only as the oracle the adapter is checked against
//! (the unit tests and `examples/markup_diff.rs`); it is compiled for tests
//! and under the `cmark-oracle` feature, which no shipped binary enables.

use std::ffi::{CStr, c_char};
use std::ptr;

use upleft_cmark_gfm_sys as cmark;

use crate::base::document::Document;
use crate::base::raw_markup::{Checkbox, NodeId, RawMarkupArena, RawMarkupData, StrRef};
use crate::infrastructure::source_location::{SourceLocation, SourceRange};
use crate::nodes::tables::ColumnAlignment;
use crate::parser::parse_options::ParseOptions;
use crate::utility::swift_string::swift_contains_character;

/// String-based CommonMark node type identifiers (`CommonMarkNodeType`).
///
/// In light of extensions the raw `cmark_node_type` is not reliable on its
/// own (a task list item is a `CMARK_NODE_ITEM` whose type string is
/// `"tasklist"`), so types are identified by `cmark_node_get_type_string`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommonMarkNodeType {
    Document,
    BlockQuote,
    List,
    Item,
    CodeBlock,
    HtmlBlock,
    CustomBlock,
    Paragraph,
    Heading,
    ThematicBreak,
    Text,
    SoftBreak,
    LineBreak,
    Code,
    Html,
    CustomInline,
    Emphasis,
    Strong,
    Link,
    Image,
    InlineAttributes,
    None,
    Unknown,

    // Extensions
    Strikethrough,

    Table,
    TableHead,
    TableRow,
    TableCell,

    TaskListItem,
}

impl CommonMarkNodeType {
    /// `CommonMarkNodeType(rawValue:)`.
    fn from_raw_value(raw_value: &[u8]) -> Option<CommonMarkNodeType> {
        Some(match raw_value {
            b"document" => CommonMarkNodeType::Document,
            b"block_quote" => CommonMarkNodeType::BlockQuote,
            b"list" => CommonMarkNodeType::List,
            b"item" => CommonMarkNodeType::Item,
            b"code_block" => CommonMarkNodeType::CodeBlock,
            b"html_block" => CommonMarkNodeType::HtmlBlock,
            b"custom_block" => CommonMarkNodeType::CustomBlock,
            b"paragraph" => CommonMarkNodeType::Paragraph,
            b"heading" => CommonMarkNodeType::Heading,
            b"thematic_break" => CommonMarkNodeType::ThematicBreak,
            b"text" => CommonMarkNodeType::Text,
            b"softbreak" => CommonMarkNodeType::SoftBreak,
            b"linebreak" => CommonMarkNodeType::LineBreak,
            b"code" => CommonMarkNodeType::Code,
            b"html_inline" => CommonMarkNodeType::Html,
            b"custom_inline" => CommonMarkNodeType::CustomInline,
            b"emph" => CommonMarkNodeType::Emphasis,
            b"strong" => CommonMarkNodeType::Strong,
            b"link" => CommonMarkNodeType::Link,
            b"image" => CommonMarkNodeType::Image,
            b"attribute" => CommonMarkNodeType::InlineAttributes,
            b"NONE" => CommonMarkNodeType::None,
            b"<unknown>" => CommonMarkNodeType::Unknown,
            b"strikethrough" => CommonMarkNodeType::Strikethrough,
            b"table" => CommonMarkNodeType::Table,
            b"table_header" => CommonMarkNodeType::TableHead,
            b"table_row" => CommonMarkNodeType::TableRow,
            b"table_cell" => CommonMarkNodeType::TableCell,
            b"tasklist" => CommonMarkNodeType::TaskListItem,
            _ => return None,
        })
    }

    fn raw_value(self) -> &'static str {
        match self {
            CommonMarkNodeType::Document => "document",
            CommonMarkNodeType::BlockQuote => "block_quote",
            CommonMarkNodeType::List => "list",
            CommonMarkNodeType::Item => "item",
            CommonMarkNodeType::CodeBlock => "code_block",
            CommonMarkNodeType::HtmlBlock => "html_block",
            CommonMarkNodeType::CustomBlock => "custom_block",
            CommonMarkNodeType::Paragraph => "paragraph",
            CommonMarkNodeType::Heading => "heading",
            CommonMarkNodeType::ThematicBreak => "thematic_break",
            CommonMarkNodeType::Text => "text",
            CommonMarkNodeType::SoftBreak => "softbreak",
            CommonMarkNodeType::LineBreak => "linebreak",
            CommonMarkNodeType::Code => "code",
            CommonMarkNodeType::Html => "html_inline",
            CommonMarkNodeType::CustomInline => "custom_inline",
            CommonMarkNodeType::Emphasis => "emph",
            CommonMarkNodeType::Strong => "strong",
            CommonMarkNodeType::Link => "link",
            CommonMarkNodeType::Image => "image",
            CommonMarkNodeType::InlineAttributes => "attribute",
            CommonMarkNodeType::None => "NONE",
            CommonMarkNodeType::Unknown => "<unknown>",
            CommonMarkNodeType::Strikethrough => "strikethrough",
            CommonMarkNodeType::Table => "table",
            CommonMarkNodeType::TableHead => "table_header",
            CommonMarkNodeType::TableRow => "table_row",
            CommonMarkNodeType::TableCell => "table_cell",
            CommonMarkNodeType::TaskListItem => "tasklist",
        }
    }

    /// Returns true when a node kind cannot have structural children.
    ///
    /// Leaf nodes are converted immediately when their ENTER event is
    /// encountered and therefore never need a `ParsingFrame`.
    fn is_leaf(self) -> bool {
        matches!(
            self,
            CommonMarkNodeType::CodeBlock
                | CommonMarkNodeType::HtmlBlock
                | CommonMarkNodeType::ThematicBreak
                | CommonMarkNodeType::Text
                | CommonMarkNodeType::SoftBreak
                | CommonMarkNodeType::LineBreak
                | CommonMarkNodeType::Code
                | CommonMarkNodeType::Html
                | CommonMarkNodeType::CustomInline
        )
    }
}

/// `MarkupConverterState.PendingTableBody`.
#[derive(Clone, Copy, Debug)]
struct PendingTableBody {
    range: Option<SourceRange>,
}

/// Represents the current state of cmark → markup conversion
/// (`MarkupConverterState`).
struct MarkupConverterState {
    /// An opaque pointer to a `cmark_iter` used during parsing.
    iterator: *mut cmark::cmark_iter,
    /// The last `cmark_event_type` during parsing.
    event: cmark::cmark_event_type,
    /// An opaque pointer to the last parsed `cmark_node`.
    node: *mut cmark::cmark_node,
    /// The type of `node`. Swift recomputes this from the type string on
    /// every access; it cannot change while the state exists.
    node_type: CommonMarkNodeType,
    /// Options to consider when converting to markup elements.
    options: ParseOptions,
    header_seen: bool,
    pending_table_body: Option<PendingTableBody>,
}

impl MarkupConverterState {
    /// `init(source:iterator:event:node:options:headerSeen:pendingTableBody:)`.
    unsafe fn new(
        iterator: *mut cmark::cmark_iter,
        event: cmark::cmark_event_type,
        node: *mut cmark::cmark_node,
        options: ParseOptions,
        header_seen: bool,
        pending_table_body: Option<PendingTableBody>,
    ) -> MarkupConverterState {
        let node_type = unsafe { node_type(node) };
        let mut state = MarkupConverterState {
            iterator,
            event,
            node,
            node_type,
            options,
            header_seen,
            pending_table_body,
        };

        match (event, node_type) {
            (cmark::CMARK_EVENT_EXIT, CommonMarkNodeType::TableHead) => {
                state.header_seen = true;
            }
            (cmark::CMARK_EVENT_ENTER, CommonMarkNodeType::TableRow) if state.header_seen => {
                if state.pending_table_body.is_none() {
                    state.pending_table_body = Some(PendingTableBody {
                        range: unsafe { range(state.node) },
                    });
                    assert!(state.pending_table_body.is_some());
                }
            }
            (cmark::CMARK_EVENT_EXIT, CommonMarkNodeType::Table) => {
                if let Some(end_of_table) =
                    unsafe { range(state.node) }.map(|range| range.upper_bound)
                    && let Some(pending_table_range) =
                        state.pending_table_body.and_then(|pending| pending.range)
                    && let Some(pending) = state.pending_table_body.as_mut()
                {
                    pending.range = Some(SourceRange::new(
                        pending_table_range.lower_bound,
                        end_of_table,
                    ));
                }
            }
            _ => {}
        }
        state
    }

    /// Get the next cmark iterator and node, returning a new state.
    unsafe fn next(&self, clear_pending_table_body: bool) -> MarkupConverterState {
        let new_event = unsafe { cmark::cmark_iter_next(self.iterator) };
        let new_node = unsafe { cmark::cmark_iter_get_node(self.iterator) };
        unsafe {
            MarkupConverterState::new(
                self.iterator,
                new_event,
                new_node,
                self.options,
                if clear_pending_table_body {
                    false
                } else {
                    self.header_seen
                },
                if clear_pending_table_body {
                    None
                } else {
                    self.pending_table_body
                },
            )
        }
    }
}

/// `MarkupConverterState.nodeType`: the type of a cmark node, `.none` for a
/// null node.
unsafe fn node_type(node: *mut cmark::cmark_node) -> CommonMarkNodeType {
    if node.is_null() {
        return CommonMarkNodeType::None;
    }
    let type_string = unsafe { CStr::from_ptr(cmark::cmark_node_get_type_string(node)) }.to_bytes();
    match CommonMarkNodeType::from_raw_value(type_string) {
        Some(node_type) => node_type,
        None => panic!(
            "Unknown cmark node type '{}' encountered during conversion",
            String::from_utf8_lossy(type_string)
        ),
    }
}

/// `MarkupConverterState.range(_:)`: the source range where a node occurred,
/// according to cmark.
unsafe fn range(node: *mut cmark::cmark_node) -> Option<SourceRange> {
    let start_line = unsafe { cmark::cmark_node_get_start_line(node) } as i64;
    let start_column = unsafe { cmark::cmark_node_get_start_column(node) } as i64;
    if !(start_line > 0 && start_column > 0) {
        // cmark doesn't track the positions for this node.
        return None;
    }

    let end_line = unsafe { cmark::cmark_node_get_end_line(node) } as i64;
    let end_column = unsafe { cmark::cmark_node_get_end_column(node) } as i64 + 1;

    if !(end_line > 0 && end_column > 0) {
        // cmark doesn't track the positions for this node.
        return None;
    }

    // If this is a symbol link / code span, set the locations to include the ticks.
    let backtick_count = unsafe { cmark::cmark_node_get_backtick_count(node) } as i64;

    let start = SourceLocation::new(start_line, start_column - backtick_count);
    let end = SourceLocation::new(end_line, end_column + backtick_count);

    // Sometimes the cmark range is invalid (rdar://73376719)
    if !(start <= end) {
        return None;
    }
    Some(SourceRange::new(start, end))
}

/// Represents an active container node while iteratively converting the
/// cmark AST (`ParsingFrame`).
struct ParsingFrame {
    /// The underlying cmark node associated with this frame.
    node: *mut cmark::cmark_node,
    /// Cached node type to avoid repeated string conversions while the frame
    /// remains on the work stack.
    node_type: CommonMarkNodeType,
    /// Source range reported by cmark for this node.
    parsed_range: Option<SourceRange>,
    /// Where this frame's converted children start in the shared child stack.
    /// Swift gives every frame its own `children` array; the children of the
    /// innermost open frame are always the top of one shared stack.
    children_start: usize,
}

/// The tree being built plus the shared stack of converted children awaiting
/// their parent's EXIT event.
struct Converter {
    arena: RawMarkupArena,
    children: Vec<NodeId>,
}

impl Converter {
    /// `String(cString:)` into the document's string buffer. A null pointer
    /// traps, as it does in Swift; invalid UTF-8 is repaired with U+FFFD.
    unsafe fn string(&mut self, pointer: *const c_char) -> StrRef {
        assert!(
            !pointer.is_null(),
            "unexpectedly found nil while unwrapping a C string"
        );
        let bytes = unsafe { CStr::from_ptr(pointer) }.to_bytes();
        match std::str::from_utf8(bytes) {
            Ok(string) => self.arena.push_str(string),
            Err(_) => self.arena.push_str(&String::from_utf8_lossy(bytes)),
        }
    }

    /// `String(cString:)` mapped to `nil` when empty, as the converter does
    /// for fence info, link destinations and titles.
    unsafe fn non_empty_string(&mut self, pointer: *const c_char) -> Option<StrRef> {
        assert!(
            !pointer.is_null(),
            "unexpectedly found nil while unwrapping a C string"
        );
        if unsafe { *pointer } == 0 {
            return None;
        }
        Some(unsafe { self.string(pointer) })
    }

    /// `getLiteralContent(node:)`: the raw literal text for a cmark node.
    unsafe fn literal_content(&mut self, node: *mut cmark::cmark_node) -> StrRef {
        let raw_text = unsafe { cmark::cmark_node_get_literal(node) };
        if raw_text.is_null() {
            panic!("Expected literal content for cmark node but got null pointer");
        }
        unsafe { self.string(raw_text) }
    }

    fn leaf(&mut self, data: RawMarkupData, parsed_range: Option<SourceRange>) -> NodeId {
        self.arena.create(data, parsed_range, &[])
    }

    // MARK: Leaves

    unsafe fn convert_code_block(
        &mut self,
        state: &MarkupConverterState,
        parsed_range: Option<SourceRange>,
    ) -> NodeId {
        assert!(state.event == cmark::CMARK_EVENT_ENTER);
        assert!(state.node_type == CommonMarkNodeType::CodeBlock);
        let language =
            unsafe { self.non_empty_string(cmark::cmark_node_get_fence_info(state.node)) };
        let code = unsafe { self.literal_content(state.node) };
        self.leaf(RawMarkupData::CodeBlock { code, language }, parsed_range)
    }

    unsafe fn convert_html_block(
        &mut self,
        state: &MarkupConverterState,
        parsed_range: Option<SourceRange>,
    ) -> NodeId {
        assert!(state.event == cmark::CMARK_EVENT_ENTER);
        assert!(state.node_type == CommonMarkNodeType::HtmlBlock);
        let html = unsafe { self.literal_content(state.node) };
        self.leaf(RawMarkupData::HtmlBlock(html), parsed_range)
    }

    fn convert_thematic_break(
        &mut self,
        state: &MarkupConverterState,
        parsed_range: Option<SourceRange>,
    ) -> NodeId {
        assert!(state.event == cmark::CMARK_EVENT_ENTER);
        assert!(state.node_type == CommonMarkNodeType::ThematicBreak);
        self.leaf(RawMarkupData::ThematicBreak, parsed_range)
    }

    unsafe fn convert_text(
        &mut self,
        state: &MarkupConverterState,
        parsed_range: Option<SourceRange>,
    ) -> NodeId {
        assert!(state.event == cmark::CMARK_EVENT_ENTER);
        assert!(state.node_type == CommonMarkNodeType::Text);
        let string = unsafe { self.literal_content(state.node) };
        self.leaf(RawMarkupData::Text(string), parsed_range)
    }

    fn convert_soft_break(
        &mut self,
        state: &MarkupConverterState,
        parsed_range: Option<SourceRange>,
    ) -> NodeId {
        assert!(state.event == cmark::CMARK_EVENT_ENTER);
        assert!(state.node_type == CommonMarkNodeType::SoftBreak);
        self.leaf(RawMarkupData::SoftBreak, parsed_range)
    }

    fn convert_line_break(
        &mut self,
        state: &MarkupConverterState,
        parsed_range: Option<SourceRange>,
    ) -> NodeId {
        assert!(state.event == cmark::CMARK_EVENT_ENTER);
        assert!(state.node_type == CommonMarkNodeType::LineBreak);
        self.leaf(RawMarkupData::LineBreak, parsed_range)
    }

    unsafe fn convert_inline_code(
        &mut self,
        state: &MarkupConverterState,
        parsed_range: Option<SourceRange>,
    ) -> NodeId {
        assert!(state.event == cmark::CMARK_EVENT_ENTER);
        assert!(state.node_type == CommonMarkNodeType::Code);
        let literal_content = unsafe { self.literal_content(state.node) };
        if state.options.contains(ParseOptions::PARSE_SYMBOL_LINKS)
            && unsafe { cmark::cmark_node_get_backtick_count(state.node) } > 1
            && !swift_contains_character(self.arena.str(literal_content), '`')
        {
            self.leaf(
                RawMarkupData::SymbolLink {
                    destination: Some(literal_content),
                },
                parsed_range,
            )
        } else {
            self.leaf(RawMarkupData::InlineCode(literal_content), parsed_range)
        }
    }

    unsafe fn convert_inline_html(
        &mut self,
        state: &MarkupConverterState,
        parsed_range: Option<SourceRange>,
    ) -> NodeId {
        assert!(state.event == cmark::CMARK_EVENT_ENTER);
        assert!(state.node_type == CommonMarkNodeType::Html);
        let html = unsafe { self.literal_content(state.node) };
        self.leaf(RawMarkupData::InlineHtml(html), parsed_range)
    }

    unsafe fn convert_custom_inline(
        &mut self,
        state: &MarkupConverterState,
        parsed_range: Option<SourceRange>,
    ) -> NodeId {
        assert!(state.event == cmark::CMARK_EVENT_ENTER);
        assert!(state.node_type == CommonMarkNodeType::CustomInline);
        let text = unsafe { self.literal_content(state.node) };
        self.leaf(RawMarkupData::CustomInline(text), parsed_range)
    }

    /// Converts a leaf cmark node directly into its markup element
    /// (`createLeaf(state:)`).
    unsafe fn create_leaf(
        &mut self,
        state: &MarkupConverterState,
        parsed_range: Option<SourceRange>,
    ) -> NodeId {
        unsafe {
            match state.node_type {
                CommonMarkNodeType::CodeBlock => self.convert_code_block(state, parsed_range),
                CommonMarkNodeType::HtmlBlock => self.convert_html_block(state, parsed_range),
                CommonMarkNodeType::ThematicBreak => {
                    self.convert_thematic_break(state, parsed_range)
                }
                CommonMarkNodeType::Text => self.convert_text(state, parsed_range),
                CommonMarkNodeType::SoftBreak => self.convert_soft_break(state, parsed_range),
                CommonMarkNodeType::LineBreak => self.convert_line_break(state, parsed_range),
                CommonMarkNodeType::Code => self.convert_inline_code(state, parsed_range),
                CommonMarkNodeType::Html => self.convert_inline_html(state, parsed_range),
                CommonMarkNodeType::CustomInline => self.convert_custom_inline(state, parsed_range),
                other => panic!("Unhandled leaf node type: {other:?}"),
            }
        }
    }

    // MARK: Containers

    /// A container over the frame's accumulated children, which are popped
    /// off the shared child stack.
    fn container(&mut self, frame: &ParsingFrame, data: RawMarkupData) -> NodeId {
        let id = self.arena.create(
            data,
            frame.parsed_range,
            &self.children[frame.children_start..],
        );
        self.children.truncate(frame.children_start);
        id
    }

    /// Converts a completed parsing frame into its markup element
    /// (`createContainer(frame:state:)`). Called on the matching EXIT event,
    /// when all descendants have been converted onto the child stack.
    unsafe fn create_container(
        &mut self,
        frame: &ParsingFrame,
        state: &MarkupConverterState,
    ) -> NodeId {
        assert!(
            state.event == cmark::CMARK_EVENT_EXIT,
            "Expected EXIT event when closing a container node."
        );
        assert!(
            state.node_type == frame.node_type,
            "MarkupConverterState nodeType does not match the frame being closed."
        );
        let node = frame.node;

        match frame.node_type {
            CommonMarkNodeType::Document => self.container(frame, RawMarkupData::Document),
            CommonMarkNodeType::BlockQuote => self.container(frame, RawMarkupData::BlockQuote),
            CommonMarkNodeType::List => {
                for &child in &self.children[frame.children_start..] {
                    if !matches!(self.arena.node(child).data, RawMarkupData::ListItem { .. }) {
                        panic!("Converted cmark list had a non-listItem node");
                    }
                }
                match unsafe { cmark::cmark_node_get_list_type(node) } {
                    cmark::CMARK_BULLET_LIST => self.container(frame, RawMarkupData::UnorderedList),
                    cmark::CMARK_ORDERED_LIST => {
                        let cmark_start = unsafe { cmark::cmark_node_get_list_start(node) };
                        let start_index = u64::try_from(cmark_start)
                            .expect("Negative value is not representable");
                        self.container(frame, RawMarkupData::OrderedList { start_index })
                    }
                    _ => panic!(
                        "cmark reported a list node but said its list type is CMARK_NO_LIST?"
                    ),
                }
            }
            CommonMarkNodeType::Item => {
                self.container(frame, RawMarkupData::ListItem { checkbox: None })
            }
            CommonMarkNodeType::CustomBlock => self.container(frame, RawMarkupData::CustomBlock),
            CommonMarkNodeType::Paragraph => self.container(frame, RawMarkupData::Paragraph),
            CommonMarkNodeType::Heading => {
                let heading_level = unsafe { cmark::cmark_node_get_heading_level(node) } as i64;
                self.container(
                    frame,
                    RawMarkupData::Heading {
                        level: heading_level,
                    },
                )
            }
            CommonMarkNodeType::Emphasis => self.container(frame, RawMarkupData::Emphasis),
            CommonMarkNodeType::Strong => self.container(frame, RawMarkupData::Strong),
            CommonMarkNodeType::Link => {
                let destination = unsafe { self.non_empty_string(cmark::cmark_node_get_url(node)) };
                let title = unsafe { self.non_empty_string(cmark::cmark_node_get_title(node)) };
                self.container(frame, RawMarkupData::Link { destination, title })
            }
            CommonMarkNodeType::Image => {
                let source = unsafe { self.non_empty_string(cmark::cmark_node_get_url(node)) };
                let title = unsafe { self.non_empty_string(cmark::cmark_node_get_title(node)) };
                self.container(frame, RawMarkupData::Image { source, title })
            }
            CommonMarkNodeType::Strikethrough => {
                self.container(frame, RawMarkupData::Strikethrough)
            }
            CommonMarkNodeType::TaskListItem => {
                let checkbox =
                    if unsafe { cmark::cmark_gfm_extensions_get_tasklist_item_checked(node) } {
                        Checkbox::Checked
                    } else {
                        Checkbox::Unchecked
                    };
                self.container(
                    frame,
                    RawMarkupData::ListItem {
                        checkbox: Some(checkbox),
                    },
                )
            }
            CommonMarkNodeType::Table => unsafe { self.create_table(frame, state) },
            CommonMarkNodeType::TableHead => {
                let id = self
                    .arena
                    .table_head(frame.parsed_range, &self.children[frame.children_start..]);
                self.children.truncate(frame.children_start);
                id
            }
            CommonMarkNodeType::TableRow => {
                let id = self
                    .arena
                    .table_row(frame.parsed_range, &self.children[frame.children_start..]);
                self.children.truncate(frame.children_start);
                id
            }
            CommonMarkNodeType::TableCell => {
                let colspan =
                    unsafe { cmark::cmark_gfm_extensions_get_table_cell_colspan(node) } as u64;
                let rowspan =
                    unsafe { cmark::cmark_gfm_extensions_get_table_cell_rowspan(node) } as u64;
                self.container(frame, RawMarkupData::TableCell { colspan, rowspan })
            }
            CommonMarkNodeType::InlineAttributes => {
                let attributes = unsafe { self.string(cmark::cmark_node_get_attributes(node)) };
                self.container(frame, RawMarkupData::InlineAttributes { attributes })
            }
            other => panic!("Unknown container node type '{}'", other.raw_value()),
        }
    }

    /// The `.table` case of `createContainer`: GFM tables are a header row
    /// followed by body rows; the body gets the range the converter state
    /// collected from its first row to the end of the table.
    unsafe fn create_table(
        &mut self,
        frame: &ParsingFrame,
        state: &MarkupConverterState,
    ) -> NodeId {
        let node = frame.node;
        let column_count = unsafe { cmark::cmark_gfm_extensions_get_table_columns(node) } as usize;
        let alignments = unsafe { cmark::cmark_gfm_extensions_get_table_alignments(node) };
        let column_alignments: Vec<Option<ColumnAlignment>> = (0..column_count)
            .map(|column| {
                let ascii = unsafe { *alignments.add(column) };
                match ascii {
                    b'l' => Some(ColumnAlignment::Left),
                    b'r' => Some(ColumnAlignment::Right),
                    b'c' => Some(ColumnAlignment::Center),
                    0 => None,
                    _ => panic!("Unexpected table column character"),
                }
            })
            .collect();

        let start = frame.children_start;
        let header;
        let mut rows_start = start;
        // GFM tables are represented as a header followed by body rows.
        if let Some(&first_child) = self.children.get(start)
            && self.arena.node(first_child).data == RawMarkupData::TableHead
        {
            header = first_child;
            rows_start += 1;
        } else {
            header = self.arena.table_head(None, &[]);
        }

        if rows_start == self.children.len() {
            assert!(state.pending_table_body.is_none());
        }
        let body_range = state.pending_table_body.and_then(|pending| pending.range);
        let body = self
            .arena
            .table_body(body_range, &self.children[rows_start..]);
        self.children.truncate(start);
        self.arena
            .table(&column_alignments, frame.parsed_range, header, body)
    }

    // MARK: Conversion

    /// `parseString(_:source:options:)`.
    unsafe fn parse_string(string: &str, options: ParseOptions) -> Document {
        unsafe {
            cmark::cmark_gfm_core_extensions_ensure_registered();

            let mut cmark_options = cmark::CMARK_OPT_TABLE_SPANS;
            if !options.contains(ParseOptions::DISABLE_SMART_OPTS) {
                cmark_options |= cmark::CMARK_OPT_SMART;
            }
            if !options.contains(ParseOptions::DISABLE_SOURCE_POS_OPTS) {
                cmark_options |= cmark::CMARK_OPT_SOURCEPOS;
            }

            let parser = cmark::cmark_parser_new(cmark_options);

            cmark::cmark_parser_attach_syntax_extension(
                parser,
                cmark::cmark_find_syntax_extension(c"table".as_ptr()),
            );
            cmark::cmark_parser_attach_syntax_extension(
                parser,
                cmark::cmark_find_syntax_extension(c"strikethrough".as_ptr()),
            );
            cmark::cmark_parser_attach_syntax_extension(
                parser,
                cmark::cmark_find_syntax_extension(c"tasklist".as_ptr()),
            );
            cmark::cmark_parser_feed(parser, string.as_ptr().cast(), string.len());
            let raw_document = cmark::cmark_parser_finish(parser);
            let mut state = MarkupConverterState::new(
                cmark::cmark_iter_new(raw_document),
                cmark::CMARK_EVENT_NONE,
                ptr::null_mut(),
                options,
                false,
                None,
            )
            .next(false);

            assert!(state.event == cmark::CMARK_EVENT_ENTER);
            assert!(state.node_type == CommonMarkNodeType::Document);

            // Sized from the source: agent-shaped Markdown averages about one
            // element per 12 bytes, and text is at most the source's length.
            let mut converter = Converter {
                arena: RawMarkupArena::with_capacity(string.len() / 12 + 16, string.len()),
                children: Vec::with_capacity(64),
            };
            let mut stack: Vec<ParsingFrame> = Vec::with_capacity(32);

            while state.event != cmark::CMARK_EVENT_DONE {
                let node = state.node;
                if node.is_null() {
                    state = state.next(false);
                    continue;
                }

                let node_type = state.node_type;
                let parsed_range = range(node);

                if state.event == cmark::CMARK_EVENT_ENTER {
                    if node_type.is_leaf() {
                        let leaf = converter.create_leaf(&state, parsed_range);
                        assert!(
                            !stack.is_empty(),
                            "Leaf node encountered without a parent document on the stack."
                        );
                        converter.children.push(leaf);
                    } else {
                        stack.push(ParsingFrame {
                            node,
                            node_type,
                            parsed_range,
                            children_start: converter.children.len(),
                        });
                    }
                    state = state.next(false);
                } else if state.event == cmark::CMARK_EVENT_EXIT {
                    assert!(
                        !node_type.is_leaf(),
                        "cmark iterators should never return EXIT events for leaf nodes."
                    );

                    let frame = stack.pop().expect("EXIT event without an open frame");
                    assert!(frame.node == node);

                    let container = converter.create_container(&frame, &state);

                    if stack.is_empty() {
                        assert!(frame.node_type == CommonMarkNodeType::Document);
                        let iterator = state.iterator;

                        cmark::cmark_iter_free(iterator);
                        cmark::cmark_node_free(raw_document);
                        cmark::cmark_parser_free(parser);

                        return Document {
                            arena: converter.arena,
                            root: container,
                        };
                    } else {
                        converter.children.push(container);
                        state = state.next(node_type == CommonMarkNodeType::Table);
                    }
                }
            }

            panic!(
                "cmark iteration terminated prematurely without cleanly exiting the document root."
            );
        }
    }
}

/// `Document(parsing:options:)` through cmark-gfm: the tree swift-markdown
/// builds, to compare the pulldown-cmark adapter with.
pub fn parse(string: &str, options: ParseOptions) -> Document {
    // SAFETY: the cmark objects are created, used and freed within the
    // call; every node pointer comes from the live iterator.
    unsafe { Converter::parse_string(string, options) }
}
