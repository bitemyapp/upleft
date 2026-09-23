//! Port of `Sources/Markdown/Base/Document.swift`.

use crate::base::markup::Markup;
use crate::base::markup_children::MarkupChildren;
use crate::base::raw_markup::{NodeId, RawMarkupArena};
use crate::infrastructure::source_location::SourceRange;
use crate::parser::common_mark_converter::MarkupParser;
use crate::parser::parse_options::ParseOptions;

/// A parsed Markdown document: owns the whole markup tree. Its root element
/// ([`Document::root`]) is swift-markdown's `Document` markup.
#[derive(Clone, Debug)]
pub struct Document {
    pub(crate) arena: RawMarkupArena,
    pub(crate) root: NodeId,
}

impl Document {
    /// `Document(parsing:options:)`: parses `string` with cmark-gfm and
    /// converts the result the way swift-markdown's `MarkupParser` does.
    ///
    /// Downright calls this as
    /// `Document::parse(body, ParseOptions::DISABLE_SMART_OPTS)`.
    pub fn parse(string: &str, options: ParseOptions) -> Document {
        MarkupParser::parse_string(string, options)
    }

    /// The root element (a `MarkupData::Document`).
    pub fn root(&self) -> Markup<'_> {
        Markup::new(self, self.root)
    }

    /// The root's `children`.
    pub fn children(&self) -> MarkupChildren<'_> {
        self.root().children()
    }

    /// The root's `childCount`.
    pub fn child_count(&self) -> usize {
        self.root().child_count()
    }

    /// The root's `child(at:)`.
    pub fn child(&self, position: usize) -> Option<Markup<'_>> {
        self.root().child(position)
    }

    /// The root's `range`.
    pub fn range(&self) -> Option<SourceRange> {
        self.root().range()
    }

    /// The element with the given arena id, as returned by [`Markup::id`].
    pub fn markup(&self, id: NodeId) -> Markup<'_> {
        assert!((id as usize) < self.arena.nodes.len(), "no element {id} in this document");
        Markup::new(self, id)
    }

    /// The number of elements in the tree, including the root.
    pub fn element_count(&self) -> usize {
        self.arena.nodes.len()
    }
}
