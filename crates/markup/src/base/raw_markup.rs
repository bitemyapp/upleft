//! Port of `Sources/Markdown/Base/RawMarkup.swift`.
//!
//! swift-markdown keeps each element in a reference-counted `ManagedBuffer`
//! with its children tail-allocated. The port keeps the whole tree of one
//! parse in flat arrays owned by [`Document`](crate::Document): one
//! [`RawNode`] per element, the child lists back to back in `child_ids`, and
//! every string the tree holds in one buffer. The observable data, the child
//! order and the parsed ranges are the ones `RawMarkup` holds.

use crate::infrastructure::source_location::SourceRange;
use crate::nodes::tables::ColumnAlignment;

/// Index of an element in its document's arena.
pub type NodeId = u32;

pub(crate) const NO_PARENT: NodeId = NodeId::MAX;

/// The checkbox state of a list item (`Checkbox`, from `ListItem.swift`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Checkbox {
    Checked,
    Unchecked,
}

/// A string stored in the document's string buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StrRef {
    start: u32,
    len: u32,
}

/// A run of table column alignments stored in the document's alignment buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AlignmentsRef {
    start: u32,
    len: u32,
}

/// `RawMarkupData`: the data specific to a kind of markup element, with
/// strings held by reference into the document.
///
/// `blockDirective` and the `doxygen*` cases come only from
/// `BlockDirectiveParser`, which is not ported (Downright never enables it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RawMarkupData {
    BlockQuote,
    CodeBlock {
        code: StrRef,
        language: Option<StrRef>,
    },
    CustomBlock,
    Document,
    Heading {
        level: i64,
    },
    ThematicBreak,
    HtmlBlock(StrRef),
    ListItem {
        checkbox: Option<Checkbox>,
    },
    OrderedList {
        start_index: u64,
    },
    UnorderedList,
    Paragraph,

    InlineCode(StrRef),
    CustomInline(StrRef),
    Emphasis,
    Image {
        source: Option<StrRef>,
        title: Option<StrRef>,
    },
    InlineHtml(StrRef),
    LineBreak,
    Link {
        destination: Option<StrRef>,
        title: Option<StrRef>,
    },
    SoftBreak,
    Strong,
    Text(StrRef),
    SymbolLink {
        destination: Option<StrRef>,
    },
    InlineAttributes {
        attributes: StrRef,
    },

    // Extensions
    Strikethrough,

    Table {
        column_alignments: AlignmentsRef,
    },
    TableHead,
    TableBody,
    TableRow,
    TableCell {
        colspan: u64,
        rowspan: u64,
    },
}

impl RawMarkupData {
    /// `isTableCell()`.
    pub(crate) fn is_table_cell(&self) -> bool {
        matches!(self, RawMarkupData::TableCell { .. })
    }
}

/// The data of one markup element, as the public API reads it: the Swift
/// element type plus the stored properties that type exposes.
///
/// Each variant is one swift-markdown `Markup` type; the fields are its
/// properties under their Swift names in snake case (`CodeBlock.code`,
/// `CodeBlock.language`, `Heading.level`, `ListItem.checkbox`,
/// `OrderedList.startIndex`, `Table.columnAlignments`, `Table.Cell.colspan`,
/// `Link.destination`, `Image.source`, `Text.string`, …). Swift `Int` is
/// `i64` and `UInt` is `u64`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkupData<'a> {
    BlockQuote,
    CodeBlock {
        code: &'a str,
        language: Option<&'a str>,
    },
    CustomBlock,
    Document,
    Heading {
        level: i64,
    },
    ThematicBreak,
    /// `HTMLBlock.rawHTML`.
    HtmlBlock {
        raw_html: &'a str,
    },
    ListItem {
        checkbox: Option<Checkbox>,
    },
    OrderedList {
        start_index: u64,
    },
    UnorderedList,
    Paragraph,

    InlineCode {
        code: &'a str,
    },
    CustomInline {
        text: &'a str,
    },
    Emphasis,
    Image {
        source: Option<&'a str>,
        title: Option<&'a str>,
    },
    /// `InlineHTML.rawHTML`.
    InlineHtml {
        raw_html: &'a str,
    },
    LineBreak,
    Link {
        destination: Option<&'a str>,
        title: Option<&'a str>,
    },
    SoftBreak,
    Strong,
    Text {
        string: &'a str,
    },
    SymbolLink {
        destination: Option<&'a str>,
    },
    InlineAttributes {
        attributes: &'a str,
    },

    Strikethrough,

    Table {
        column_alignments: &'a [Option<ColumnAlignment>],
    },
    /// `Table.Head`.
    TableHead,
    /// `Table.Body`.
    TableBody,
    /// `Table.Row`.
    TableRow,
    /// `Table.Cell`.
    TableCell {
        colspan: u64,
        rowspan: u64,
    },
}

impl MarkupData<'_> {
    /// The Swift type name, with the nested table types qualified
    /// (`Table.Head`, `Table.Body`, `Table.Row`, `Table.Cell`).
    pub fn type_name(&self) -> &'static str {
        match self {
            MarkupData::BlockQuote => "BlockQuote",
            MarkupData::CodeBlock { .. } => "CodeBlock",
            MarkupData::CustomBlock => "CustomBlock",
            MarkupData::Document => "Document",
            MarkupData::Heading { .. } => "Heading",
            MarkupData::ThematicBreak => "ThematicBreak",
            MarkupData::HtmlBlock { .. } => "HTMLBlock",
            MarkupData::ListItem { .. } => "ListItem",
            MarkupData::OrderedList { .. } => "OrderedList",
            MarkupData::UnorderedList => "UnorderedList",
            MarkupData::Paragraph => "Paragraph",
            MarkupData::InlineCode { .. } => "InlineCode",
            MarkupData::CustomInline { .. } => "CustomInline",
            MarkupData::Emphasis => "Emphasis",
            MarkupData::Image { .. } => "Image",
            MarkupData::InlineHtml { .. } => "InlineHTML",
            MarkupData::LineBreak => "LineBreak",
            MarkupData::Link { .. } => "Link",
            MarkupData::SoftBreak => "SoftBreak",
            MarkupData::Strong => "Strong",
            MarkupData::Text { .. } => "Text",
            MarkupData::SymbolLink { .. } => "SymbolLink",
            MarkupData::InlineAttributes { .. } => "InlineAttributes",
            MarkupData::Strikethrough => "Strikethrough",
            MarkupData::Table { .. } => "Table",
            MarkupData::TableHead => "Table.Head",
            MarkupData::TableBody => "Table.Body",
            MarkupData::TableRow => "Table.Row",
            MarkupData::TableCell { .. } => "Table.Cell",
        }
    }
}

/// `RawMarkupHeader` plus the tree links `_MarkupData` would compute: one
/// element of the arena.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RawNode {
    pub(crate) data: RawMarkupData,
    /// The range of the element as parsed from source, if cmark tracked one.
    pub(crate) parsed_range: Option<SourceRange>,
    pub(crate) parent: NodeId,
    pub(crate) index_in_parent: u32,
    children_start: u32,
    pub(crate) child_count: u32,
}

/// The arrays one parse fills: the storage behind a [`Document`](crate::Document).
#[derive(Clone, Debug, Default)]
pub(crate) struct RawMarkupArena {
    pub(crate) nodes: Vec<RawNode>,
    pub(crate) child_ids: Vec<NodeId>,
    pub(crate) strings: String,
    pub(crate) alignments: Vec<Option<ColumnAlignment>>,
}

fn to_u32(value: usize) -> u32 {
    u32::try_from(value).expect("markup tree exceeds u32 addressing")
}

impl RawMarkupArena {
    pub(crate) fn with_capacity(nodes: usize, strings: usize) -> RawMarkupArena {
        RawMarkupArena {
            nodes: Vec::with_capacity(nodes),
            child_ids: Vec::with_capacity(nodes),
            strings: String::with_capacity(strings),
            alignments: Vec::new(),
        }
    }

    // MARK: Storage

    pub(crate) fn push_str(&mut self, string: &str) -> StrRef {
        let start = to_u32(self.strings.len());
        self.strings.push_str(string);
        StrRef {
            start,
            len: to_u32(string.len()),
        }
    }

    pub(crate) fn str(&self, reference: StrRef) -> &str {
        let start = reference.start as usize;
        &self.strings[start..start + reference.len as usize]
    }

    pub(crate) fn alignments(&self, reference: AlignmentsRef) -> &[Option<ColumnAlignment>] {
        let start = reference.start as usize;
        &self.alignments[start..start + reference.len as usize]
    }

    pub(crate) fn node(&self, id: NodeId) -> &RawNode {
        &self.nodes[id as usize]
    }

    pub(crate) fn children(&self, id: NodeId) -> &[NodeId] {
        let node = self.node(id);
        let start = node.children_start as usize;
        &self.child_ids[start..start + node.child_count as usize]
    }

    /// `child(at:)` — `precondition(index < header.childCount)`.
    pub(crate) fn child(&self, id: NodeId, index: usize) -> NodeId {
        let children = self.children(id);
        assert!(index < children.len());
        children[index]
    }

    pub(crate) fn data(&self, id: NodeId) -> MarkupData<'_> {
        let string = |reference: StrRef| self.str(reference);
        match self.node(id).data {
            RawMarkupData::BlockQuote => MarkupData::BlockQuote,
            RawMarkupData::CodeBlock { code, language } => MarkupData::CodeBlock {
                code: string(code),
                language: language.map(string),
            },
            RawMarkupData::CustomBlock => MarkupData::CustomBlock,
            RawMarkupData::Document => MarkupData::Document,
            RawMarkupData::Heading { level } => MarkupData::Heading { level },
            RawMarkupData::ThematicBreak => MarkupData::ThematicBreak,
            RawMarkupData::HtmlBlock(html) => MarkupData::HtmlBlock {
                raw_html: string(html),
            },
            RawMarkupData::ListItem { checkbox } => MarkupData::ListItem { checkbox },
            RawMarkupData::OrderedList { start_index } => MarkupData::OrderedList { start_index },
            RawMarkupData::UnorderedList => MarkupData::UnorderedList,
            RawMarkupData::Paragraph => MarkupData::Paragraph,
            RawMarkupData::InlineCode(code) => MarkupData::InlineCode { code: string(code) },
            RawMarkupData::CustomInline(text) => MarkupData::CustomInline { text: string(text) },
            RawMarkupData::Emphasis => MarkupData::Emphasis,
            RawMarkupData::Image { source, title } => MarkupData::Image {
                source: source.map(string),
                title: title.map(string),
            },
            RawMarkupData::InlineHtml(html) => MarkupData::InlineHtml {
                raw_html: string(html),
            },
            RawMarkupData::LineBreak => MarkupData::LineBreak,
            RawMarkupData::Link { destination, title } => MarkupData::Link {
                destination: destination.map(string),
                title: title.map(string),
            },
            RawMarkupData::SoftBreak => MarkupData::SoftBreak,
            RawMarkupData::Strong => MarkupData::Strong,
            RawMarkupData::Text(text) => MarkupData::Text {
                string: string(text),
            },
            RawMarkupData::SymbolLink { destination } => MarkupData::SymbolLink {
                destination: destination.map(string),
            },
            RawMarkupData::InlineAttributes { attributes } => MarkupData::InlineAttributes {
                attributes: string(attributes),
            },
            RawMarkupData::Strikethrough => MarkupData::Strikethrough,
            RawMarkupData::Table { column_alignments } => MarkupData::Table {
                column_alignments: self.alignments(column_alignments),
            },
            RawMarkupData::TableHead => MarkupData::TableHead,
            RawMarkupData::TableBody => MarkupData::TableBody,
            RawMarkupData::TableRow => MarkupData::TableRow,
            RawMarkupData::TableCell { colspan, rowspan } => {
                MarkupData::TableCell { colspan, rowspan }
            }
        }
    }

    // MARK: Creation

    /// `RawMarkup.create(data:parsedRange:children:)`: a new element owning
    /// `children`, in order. Each child's parent link and index are set here,
    /// which is what `MarkupChildren` would compute when walking down.
    pub(crate) fn create(
        &mut self,
        data: RawMarkupData,
        parsed_range: Option<SourceRange>,
        children: &[NodeId],
    ) -> NodeId {
        let id = to_u32(self.nodes.len());
        let children_start = to_u32(self.child_ids.len());
        self.child_ids.extend_from_slice(children);
        for (index, &child) in children.iter().enumerate() {
            let child = &mut self.nodes[child as usize];
            child.parent = id;
            child.index_in_parent = index as u32;
        }
        self.nodes.push(RawNode {
            data,
            parsed_range,
            parent: NO_PARENT,
            index_in_parent: 0,
            children_start,
            child_count: to_u32(children.len()),
        });
        id
    }

    /// `RawMarkup.tableRow(parsedRange:_:)`.
    pub(crate) fn table_row(
        &mut self,
        parsed_range: Option<SourceRange>,
        columns: &[NodeId],
    ) -> NodeId {
        assert!(
            columns
                .iter()
                .all(|&column| self.node(column).data.is_table_cell())
        );
        self.create(RawMarkupData::TableRow, parsed_range, columns)
    }

    /// `RawMarkup.tableHead(parsedRange:columns:)`.
    pub(crate) fn table_head(
        &mut self,
        parsed_range: Option<SourceRange>,
        columns: &[NodeId],
    ) -> NodeId {
        assert!(
            columns
                .iter()
                .all(|&column| self.node(column).data.is_table_cell())
        );
        self.create(RawMarkupData::TableHead, parsed_range, columns)
    }

    /// `RawMarkup.tableBody(parsedRange:rows:)`.
    pub(crate) fn table_body(
        &mut self,
        parsed_range: Option<SourceRange>,
        rows: &[NodeId],
    ) -> NodeId {
        assert!(
            rows.iter()
                .all(|&row| self.node(row).data == RawMarkupData::TableRow)
        );
        self.create(RawMarkupData::TableBody, parsed_range, rows)
    }

    /// `RawMarkup.table(columnAlignments:parsedRange:header:body:)`.
    ///
    /// The alignments are padded with `nil` up to the widest row. swift-markdown
    /// measures the body through `RawMarkup.children`, whose lazy map returns
    /// `child(at: 0)` for every index, so every body row counts as the first
    /// row's width. Kept as is.
    pub(crate) fn table(
        &mut self,
        column_alignments: &[Option<ColumnAlignment>],
        parsed_range: Option<SourceRange>,
        header: NodeId,
        body: NodeId,
    ) -> NodeId {
        let body_count = self.node(body).child_count as usize;
        let body_width = (0..body_count)
            .map(|_| self.node(self.child(body, 0)).child_count as usize)
            .fold(0, usize::max);
        let max_column_count = usize::max(self.node(header).child_count as usize, body_width);
        let start = to_u32(self.alignments.len());
        self.alignments.extend_from_slice(column_alignments);
        let padding =
            usize::max(column_alignments.len(), max_column_count) - column_alignments.len();
        self.alignments.extend(std::iter::repeat_n(None, padding));
        let len = to_u32(self.alignments.len()) - start;
        self.create(
            RawMarkupData::Table {
                column_alignments: AlignmentsRef { start, len },
            },
            parsed_range,
            &[header, body],
        )
    }
}
