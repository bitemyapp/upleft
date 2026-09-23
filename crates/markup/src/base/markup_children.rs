//! Port of `Sources/Markdown/Base/MarkupChildren.swift`: an element's
//! children in order (and, through `DoubleEndedIterator`, the
//! `ReversedMarkupChildren` view).

use crate::base::document::Document;
use crate::base::markup::Markup;
use crate::base::raw_markup::NodeId;

/// The children of an element.
#[derive(Clone)]
pub struct MarkupChildren<'a> {
    document: &'a Document,
    ids: std::slice::Iter<'a, NodeId>,
}

impl<'a> MarkupChildren<'a> {
    pub(crate) fn new(document: &'a Document, ids: &'a [NodeId]) -> MarkupChildren<'a> {
        MarkupChildren { document, ids: ids.iter() }
    }
}

impl<'a> Iterator for MarkupChildren<'a> {
    type Item = Markup<'a>;

    fn next(&mut self) -> Option<Markup<'a>> {
        self.ids.next().map(|&id| Markup::new(self.document, id))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.ids.size_hint()
    }
}

impl DoubleEndedIterator for MarkupChildren<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.ids.next_back().map(|&id| Markup::new(self.document, id))
    }
}

impl ExactSizeIterator for MarkupChildren<'_> {}

impl std::iter::FusedIterator for MarkupChildren<'_> {}
