//! Port of `alg/common/nodespacing/cellsystem/ContainerCell.swift`.
//!
//! A container cell contains other cells. How it contains them depends on the
//! actual container cell ([`StripContainerCell`](super::strip_container_cell::StripContainerCell),
//! [`GridContainerCell`](super::grid_container_cell::GridContainerCell)); each
//! implements `layoutChildrenHorizontally()`/`layoutChildrenVertically()`.
//! This module holds the shared utility methods.

use super::cell::CellRef;

/// `ContainerCell`'s utility methods.
pub struct ContainerCell;

impl ContainerCell {
    /// `minWidthOfCell(_:respectContributionFlag:)`.
    pub fn min_width_of_cell(cell: Option<&CellRef>, respect_contribution_flag: bool) -> f64 {
        // If there's no cell, there's no minimum width
        let Some(cell) = cell else { return 0.0 };

        // If the cell doesn't have its contribution flag activated, there's no minimum width
        if respect_contribution_flag && !cell.is_contributing_to_minimum_width() {
            return 0.0;
        }

        // If the cell is an atomic cell with a content area of no width, there's no minimum width
        if let Some(atomic_cell) = cell.as_atomic() {
            if atomic_cell.borrow().minimum_content_area_size.x == 0.0 {
                return 0.0;
            }
        }

        cell.get_minimum_width()
    }

    /// `minHeightOfCell(_:respectContributionFlag:)`.
    pub fn min_height_of_cell(cell: Option<&CellRef>, respect_contribution_flag: bool) -> f64 {
        // If there's no cell, there's no minimum height
        let Some(cell) = cell else { return 0.0 };

        // If the cell doesn't have its contribution flag activated, there's no minimum height
        if respect_contribution_flag && !cell.is_contributing_to_minimum_height() {
            return 0.0;
        }

        // If the cell is an atomic cell with a content area of no height, there's no minimum height
        if let Some(atomic_cell) = cell.as_atomic() {
            if atomic_cell.borrow().minimum_content_area_size.y == 0.0 {
                return 0.0;
            }
        }

        cell.get_minimum_height()
    }

    /// `applyHorizontalLayout(_:x:width:)`.
    pub fn apply_horizontal_layout(cell: Option<&CellRef>, x: f64, width: f64) {
        let Some(cell) = cell else { return };
        cell.with_cell_mut(|c| {
            c.cell_rectangle.x = x;
            c.cell_rectangle.width = width;
        });
    }

    /// `applyVerticalLayout(_:y:height:)`.
    pub fn apply_vertical_layout(cell: Option<&CellRef>, y: f64, height: f64) {
        let Some(cell) = cell else { return };
        cell.with_cell_mut(|c| {
            c.cell_rectangle.y = y;
            c.cell_rectangle.height = height;
        });
    }
}
