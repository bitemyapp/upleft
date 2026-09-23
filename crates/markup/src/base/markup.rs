//! Port of `Sources/Markdown/Base/Markup.swift` and `Base/MarkupData.swift`:
//! the element handle and its tree navigation.
//!
//! A swift-markdown `Markup` is a value holding its raw element plus the path
//! it was reached by (`_MarkupData`: parent, index in parent). Here a
//! [`Markup`] is a copyable handle, a document reference plus an element
//! index; parent links and indices are stored when the tree is built.

use crate::base::document::Document;
use crate::base::markup_children::MarkupChildren;
use crate::base::raw_markup::{MarkupData, NO_PARENT, NodeId};
use crate::infrastructure::source_location::SourceRange;

/// A markup element in a parsed [`Document`].
#[derive(Clone, Copy)]
pub struct Markup<'a> {
    pub(crate) document: &'a Document,
    pub(crate) id: NodeId,
}

impl<'a> Markup<'a> {
    pub(crate) fn new(document: &'a Document, id: NodeId) -> Markup<'a> {
        Markup { document, id }
    }

    /// The document this element belongs to.
    pub fn document(&self) -> &'a Document {
        self.document
    }

    /// This element's index in its document's arena. Stable for the life of
    /// the document; two handles are the same element iff their documents
    /// and ids are equal (`isIdentical(to:)`).
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// The element's type and stored properties.
    pub fn data(&self) -> MarkupData<'a> {
        self.document.arena.data(self.id)
    }

    /// `range`: the text range where this element was parsed, or `None` when
    /// cmark reported no position for it.
    pub fn range(&self) -> Option<SourceRange> {
        self.document.arena.node(self.id).parsed_range
    }

    /// `parent`: the parent of this element, or `None` for the root.
    pub fn parent(&self) -> Option<Markup<'a>> {
        let parent = self.document.arena.node(self.id).parent;
        (parent != NO_PARENT).then(|| Markup::new(self.document, parent))
    }

    /// `root`: the root of the tree, or the element itself if it is the root.
    pub fn root(&self) -> Markup<'a> {
        let mut element = *self;
        while let Some(parent) = element.parent() {
            element = parent;
        }
        element
    }

    /// `indexInParent`: the index of the element in its parent, else `0`.
    pub fn index_in_parent(&self) -> usize {
        self.document.arena.node(self.id).index_in_parent as usize
    }

    /// `childCount`.
    pub fn child_count(&self) -> usize {
        self.document.arena.node(self.id).child_count as usize
    }

    /// `isEmpty`: `true` if this element has no children.
    pub fn is_empty(&self) -> bool {
        self.child_count() == 0
    }

    /// `children`.
    pub fn children(&self) -> MarkupChildren<'a> {
        MarkupChildren::new(self.document, self.document.arena.children(self.id))
    }

    /// `child(at:)`: the child at `position`, if it is within bounds.
    pub fn child(&self, position: usize) -> Option<Markup<'a>> {
        self.document
            .arena
            .children(self.id)
            .get(position)
            .map(|&child| Markup::new(self.document, child))
    }

    /// `child(through:)`: follows a path of child indices.
    pub fn child_through(&self, path: impl IntoIterator<Item = usize>) -> Option<Markup<'a>> {
        let mut element = *self;
        for index in path {
            element = element.child(index)?;
        }
        Some(element)
    }
}

impl PartialEq for Markup<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.document, other.document) && self.id == other.id
    }
}

impl Eq for Markup<'_> {}

impl std::fmt::Debug for Markup<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Markup")
            .field("id", &self.id)
            .field("data", &self.data())
            .field("range", &self.range())
            .finish()
    }
}
