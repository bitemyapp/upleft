//! Port of `alg/common/nodespacing/cellsystem/StripContainerCell.swift`.
//!
//! A container cell that lays its (up to three) children out along a strip.

use std::cell::RefCell;
use std::rc::Rc;

use super::cell::{Cell, CellRef};
use super::container_area::ContainerArea;
use super::container_cell::ContainerCell;
use crate::org::eclipse::elk::core::math::elk_padding::ElkPadding;
use crate::swift;

/// A shared `StripContainerCell` reference.
pub type StripContainerCellRef = Rc<RefCell<StripContainerCell>>;

/// `StripContainerCell.Strip`: whether children are laid out in rows or columns.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Strip {
    VERTICAL = 0,
    HORIZONTAL = 1,
}

#[derive(Clone, Debug)]
pub struct StripContainerCell {
    pub cell: Cell,
    /// Whether we lay children out in rows or columns.
    pub container_mode: Strip,
    /// Whether the outer cells should be the same width or height.
    pub symmetrical: bool,
    /// A container cell can include gaps between its children when calculating its preferred size.
    pub gap: f64,
    /// The cells that make up the container's content.
    pub cells: [Option<CellRef>; 3],
}

impl StripContainerCell {
    pub fn new(mode: Strip, symmetrical: bool, gap: f64) -> StripContainerCell {
        StripContainerCell { cell: Cell::default(), container_mode: mode, symmetrical, gap, cells: [None, None, None] }
    }

    pub fn new_ref(mode: Strip, symmetrical: bool, gap: f64) -> StripContainerCellRef {
        Rc::new(RefCell::new(StripContainerCell::new(mode, symmetrical, gap)))
    }

    pub fn get_container_mode(&self) -> Strip {
        self.container_mode
    }

    pub fn get_gap(&self) -> f64 {
        self.gap
    }

    pub fn get_cell(&self, area: ContainerArea) -> Option<&CellRef> {
        self.cells[area.ordinal()].as_ref()
    }

    pub fn set_cell(&mut self, area: ContainerArea, cell: Option<CellRef>) {
        self.cells[area.ordinal()] = cell;
    }

    /// `setPadding(_:)` (Swift stores the caller's object; not called by reachable code).
    pub fn set_padding(&mut self, new_padding: ElkPadding) {
        self.cell.padding = new_padding;
    }

    // MARK: - (Container) Cell Methods

    pub fn get_minimum_width(&self) -> f64 {
        let mut width: f64 = 0.0;

        if self.container_mode == Strip::VERTICAL {
            width = swift::seq_max(
                self.cells.iter().flatten().filter(|c| c.is_contributing_to_minimum_width()).map(|c| c.get_minimum_width()),
            )
            .unwrap_or(0.0);
        } else {
            let cell_widths = self.min_cell_widths(true);

            let mut active_cells = 0;
            for cell_width in cell_widths {
                if cell_width > 0.0 {
                    width += cell_width;
                    active_cells += 1;
                }
            }

            if active_cells > 1 {
                width += self.gap * (active_cells - 1) as f64;
            }
        }

        if width > 0.0 {
            width + self.cell.padding.left + self.cell.padding.right
        } else {
            0.0
        }
    }

    pub fn get_minimum_height(&self) -> f64 {
        let mut height: f64 = 0.0;

        if self.container_mode == Strip::VERTICAL {
            let cell_heights = self.min_cell_heights(true);

            let mut active_cells = 0;
            for cell_height in cell_heights {
                if cell_height > 0.0 {
                    height += cell_height;
                    active_cells += 1;
                }
            }

            if active_cells > 1 {
                height += self.gap * (active_cells - 1) as f64;
            }
        } else {
            height = swift::seq_max(
                self.cells.iter().flatten().filter(|c| c.is_contributing_to_minimum_height()).map(|c| c.get_minimum_height()),
            )
            .unwrap_or(0.0);
        }

        if height > 0.0 {
            height + self.cell.padding.top + self.cell.padding.bottom
        } else {
            0.0
        }
    }

    pub fn layout_children_horizontally(&mut self) {
        let cell_rectangle = self.cell.cell_rectangle;
        let cell_padding = self.cell.padding;

        if self.container_mode == Strip::VERTICAL {
            let x_pos = cell_rectangle.x + cell_padding.left;
            let width = cell_rectangle.width - cell_padding.left - cell_padding.right;

            for child_cell in self.cells.iter().flatten() {
                ContainerCell::apply_horizontal_layout(Some(child_cell), x_pos, width);
            }
        } else {
            let cell_widths = self.min_cell_widths(false);

            if let Some(c0) = &self.cells[0] {
                ContainerCell::apply_horizontal_layout(Some(c0), cell_rectangle.x + cell_padding.left, cell_widths[0]);
            }
            if let Some(c2) = &self.cells[2] {
                ContainerCell::apply_horizontal_layout(
                    Some(c2),
                    cell_rectangle.x + cell_rectangle.width - cell_padding.right - cell_widths[2],
                    cell_widths[2],
                );
            }

            let mut free_content_area_width = cell_rectangle.width - cell_padding.left - cell_padding.right;

            let mut adjusted_cell_widths = cell_widths;

            if adjusted_cell_widths[0] > 0.0 {
                free_content_area_width -= adjusted_cell_widths[0] + self.gap;
                adjusted_cell_widths[0] += self.gap;
            }

            if adjusted_cell_widths[2] > 0.0 {
                free_content_area_width -= adjusted_cell_widths[2] + self.gap;
            }

            adjusted_cell_widths[1] = swift::max(adjusted_cell_widths[1], free_content_area_width);

            let x_offset = (adjusted_cell_widths[1] - free_content_area_width) / 2.0;
            if let Some(c1) = &self.cells[1] {
                ContainerCell::apply_horizontal_layout(
                    Some(c1),
                    cell_rectangle.x + cell_padding.left + adjusted_cell_widths[0] - x_offset,
                    adjusted_cell_widths[1],
                );
            }
        }

        // Layout container cells recursively
        for child_cell in self.cells.iter().flatten() {
            child_cell.layout_children_horizontally_if_container();
        }
    }

    pub fn layout_children_vertically(&mut self) {
        let cell_rectangle = self.cell.cell_rectangle;
        let cell_padding = self.cell.padding;

        if self.container_mode == Strip::VERTICAL {
            let cell_heights = self.min_cell_heights(false);

            if let Some(c0) = &self.cells[0] {
                ContainerCell::apply_vertical_layout(Some(c0), cell_rectangle.y + cell_padding.top, cell_heights[0]);
            }
            if let Some(c2) = &self.cells[2] {
                ContainerCell::apply_vertical_layout(
                    Some(c2),
                    cell_rectangle.y + cell_rectangle.height - cell_padding.bottom - cell_heights[2],
                    cell_heights[2],
                );
            }

            let content_area_height = cell_rectangle.height - cell_padding.top - cell_padding.bottom;
            let mut content_area_free_height = content_area_height;

            let mut adjusted_cell_heights = cell_heights;

            if adjusted_cell_heights[0] > 0.0 {
                adjusted_cell_heights[0] += self.gap;
                content_area_free_height -= adjusted_cell_heights[0];
            }

            if adjusted_cell_heights[2] > 0.0 {
                content_area_free_height -= adjusted_cell_heights[2] + self.gap;
            }

            adjusted_cell_heights[1] = swift::max(adjusted_cell_heights[1], content_area_free_height);

            let y_offset = (adjusted_cell_heights[1] - content_area_free_height) / 2.0;
            if let Some(c1) = &self.cells[1] {
                ContainerCell::apply_vertical_layout(
                    Some(c1),
                    cell_rectangle.y + cell_padding.top + adjusted_cell_heights[0] - y_offset,
                    adjusted_cell_heights[1],
                );
            }
        } else {
            let y_pos = cell_rectangle.y + cell_padding.top;
            let height = cell_rectangle.height - cell_padding.top - cell_padding.bottom;

            for child_cell in self.cells.iter().flatten() {
                ContainerCell::apply_vertical_layout(Some(child_cell), y_pos, height);
            }
        }

        // Layout container cells recursively
        for child_cell in self.cells.iter().flatten() {
            child_cell.layout_children_vertically_if_container();
        }
    }

    // MARK: - Utilities

    pub fn min_cell_widths(&self, respect_contribution_flag: bool) -> [f64; 3] {
        let mut cell_widths = [
            ContainerCell::min_width_of_cell(self.cells[0].as_ref(), respect_contribution_flag),
            ContainerCell::min_width_of_cell(self.cells[1].as_ref(), respect_contribution_flag),
            ContainerCell::min_width_of_cell(self.cells[2].as_ref(), respect_contribution_flag),
        ];

        if self.symmetrical {
            cell_widths[0] = swift::max(cell_widths[0], cell_widths[2]);
            cell_widths[2] = cell_widths[0];
        }

        cell_widths
    }

    pub fn min_cell_heights(&self, respect_contribution_flag: bool) -> [f64; 3] {
        let mut cell_heights = [
            ContainerCell::min_height_of_cell(self.cells[0].as_ref(), respect_contribution_flag),
            ContainerCell::min_height_of_cell(self.cells[1].as_ref(), respect_contribution_flag),
            ContainerCell::min_height_of_cell(self.cells[2].as_ref(), respect_contribution_flag),
        ];

        if self.symmetrical {
            cell_heights[0] = swift::max(cell_heights[0], cell_heights[2]);
            cell_heights[2] = cell_heights[0];
        }

        cell_heights
    }
}
