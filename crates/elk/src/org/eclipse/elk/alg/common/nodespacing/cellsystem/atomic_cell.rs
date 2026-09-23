//! Port of `alg/common/nodespacing/cellsystem/AtomicCell.swift`.
//!
//! An atomic cell is a simple cell that simply holds a size. Its minimum size
//! can be directly manipulated. This is basically a placeholder for other
//! things to be placed in the cell system later.

use std::cell::RefCell;
use std::rc::Rc;

use super::cell::Cell;
use crate::org::eclipse::elk::core::math::k_vector::KVector;

/// A shared `AtomicCell` reference.
pub type AtomicCellRef = Rc<RefCell<AtomicCell>>;

#[derive(Clone, Debug, Default)]
pub struct AtomicCell {
    pub cell: Cell,
    /// The minimum size of a cell's content area (that is, this excludes the padding).
    pub minimum_content_area_size: KVector,
}

impl AtomicCell {
    pub fn new() -> AtomicCell {
        AtomicCell::default()
    }

    pub fn new_ref() -> AtomicCellRef {
        Rc::new(RefCell::new(AtomicCell::new()))
    }

    /// `getMinimumContentAreaSize()`: to be modified by clients (excludes padding).
    pub fn get_minimum_content_area_size(&mut self) -> &mut KVector {
        &mut self.minimum_content_area_size
    }

    /// `setMinimumContentAreaSize(_:includesPadding:)`. Without
    /// `includesPadding` Swift stores the caller's vector object itself; no
    /// reachable code calls this method.
    pub fn set_minimum_content_area_size(&mut self, new_minimum_content_area_size: KVector, includes_padding: bool) {
        if includes_padding {
            let padding = self.cell.padding;
            self.minimum_content_area_size.x = new_minimum_content_area_size.x - padding.left - padding.right;
            self.minimum_content_area_size.y = new_minimum_content_area_size.y - padding.top - padding.bottom;
        } else {
            self.minimum_content_area_size = new_minimum_content_area_size;
        }
    }

    pub fn get_minimum_width(&self) -> f64 {
        let padding = &self.cell.padding;
        self.minimum_content_area_size.x + padding.left + padding.right
    }

    pub fn get_minimum_height(&self) -> f64 {
        let padding = &self.cell.padding;
        self.minimum_content_area_size.y + padding.top + padding.bottom
    }
}
