//! Port of the accessors in `Block Nodes/Tables/Table.swift`,
//! `TableBody.swift`, `TableCell.swift` and `TableCellContainer.swift`.

use crate::base::markup::Markup;
use crate::base::raw_markup::MarkupData;

/// `Table.ColumnAlignment`. A column without an explicit alignment is `None`
/// in `columnAlignments`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColumnAlignment {
    Left,
    Center,
    Right,
}

impl<'a> Markup<'a> {
    /// `Table.head`: `child(at: 0)`. `None` when this is not a table.
    pub fn table_head(&self) -> Option<Markup<'a>> {
        match self.data() {
            MarkupData::Table { .. } => self.child(0),
            _ => None,
        }
    }

    /// `Table.body`: `child(at: 1)`. `None` when this is not a table.
    pub fn table_body(&self) -> Option<Markup<'a>> {
        match self.data() {
            MarkupData::Table { .. } => self.child(1),
            _ => None,
        }
    }

    /// `Table.maxColumnCount` (`max(head.childCount, body.maxColumnCount)`)
    /// or `Table.Body.maxColumnCount` (the widest row). `None` for any other
    /// element.
    pub fn max_column_count(&self) -> Option<usize> {
        match self.data() {
            MarkupData::Table { .. } => {
                let head = self.child(0)?.child_count();
                let body = self.child(1)?.max_column_count()?;
                Some(usize::max(head, body))
            }
            MarkupData::TableBody => {
                Some(self.children().fold(0, |result, row| usize::max(result, row.child_count())))
            }
            _ => None,
        }
    }
}
