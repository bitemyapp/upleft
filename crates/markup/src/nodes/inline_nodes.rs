//! Computed accessors from `Inline Nodes/Inline Containers/Link.swift`. The
//! stored properties of every node are in [`MarkupData`].

use crate::base::markup::Markup;
use crate::base::raw_markup::MarkupData;
use crate::utility::swift_string::swift_string_eq;

impl Markup<'_> {
    /// `Link.isAutolink`: the link has a destination and exactly one child, a
    /// `Text` whose string equals the destination (Swift `==`, canonical
    /// equivalence). `None` for any other
    /// element.
    pub fn is_autolink(&self) -> Option<bool> {
        let MarkupData::Link { destination, .. } = self.data() else {
            return None;
        };
        let Some(destination) = destination else {
            return Some(false);
        };
        if self.child_count() != 1 {
            return Some(false);
        }
        Some(matches!(
            self.child(0).map(|child| child.data()),
            Some(MarkupData::Text { string }) if swift_string_eq(destination, string)
        ))
    }
}
