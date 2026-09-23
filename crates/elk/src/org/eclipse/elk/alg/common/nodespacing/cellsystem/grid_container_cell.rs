//! Port of `alg/common/nodespacing/cellsystem/GridContainerCell.swift`.
//!
//! A container that lays out its child cells in three rows and three columns.
//! Note (kept from elk-swift): `applyWidthToColumn` applies a column width to
//! the cells of *all* rows, so in non-tabular mode the widths computed for the
//! last row win.

use std::cell::RefCell;
use std::rc::Rc;

use super::cell::{Cell, CellRef};
use super::container_area::ContainerArea;
use super::container_cell::ContainerCell;
use crate::org::eclipse::elk::core::math::elk_rectangle::ElkRectangle;
use crate::org::eclipse::elk::core::math::k_vector::KVector;
use crate::swift;

/// A shared `GridContainerCell` reference.
pub type GridContainerCellRef = Rc<RefCell<GridContainerCell>>;

#[derive(Clone, Debug)]
pub struct GridContainerCell {
    pub cell: Cell,
    pub gap: f64,
    pub tabular: bool,
    pub symmetrical: bool,
    /// `cells[row][column]`.
    pub cells: [[Option<CellRef>; 3]; 3],
    pub center_cell_minimum_size: Option<KVector>,
    pub only_center_cell_contributes_to_minimum_size: bool,
    pub center_cell_rect: ElkRectangle,
}

impl GridContainerCell {
    pub const ROWS: usize = ContainerArea::COUNT;
    pub const COLUMNS: usize = Self::ROWS;

    pub fn new(tabular: bool, symmetrical: bool, gap: f64) -> GridContainerCell {
        GridContainerCell {
            cell: Cell::default(),
            gap,
            tabular,
            symmetrical,
            cells: Default::default(),
            center_cell_minimum_size: None,
            only_center_cell_contributes_to_minimum_size: false,
            center_cell_rect: ElkRectangle::default(),
        }
    }

    pub fn new_ref(tabular: bool, symmetrical: bool, gap: f64) -> GridContainerCellRef {
        Rc::new(RefCell::new(GridContainerCell::new(tabular, symmetrical, gap)))
    }

    pub fn get_gap(&self) -> f64 {
        self.gap
    }

    pub fn get_cell(&self, row: ContainerArea, col: ContainerArea) -> Option<&CellRef> {
        self.cells[row.ordinal()][col.ordinal()].as_ref()
    }

    pub fn set_cell(&mut self, row: ContainerArea, col: ContainerArea, cell: Option<CellRef>) {
        self.cells[row.ordinal()][col.ordinal()] = cell;
    }

    pub fn set_center_cell_minimum_size(&mut self, minimum_size: KVector) {
        self.center_cell_minimum_size = Some(KVector::new(minimum_size.x, minimum_size.y));
    }

    pub fn set_only_center_cell_contributes_to_minimum_size(&mut self, contribution: bool) {
        self.only_center_cell_contributes_to_minimum_size = contribution;
    }

    /// A copy of the center cell's rectangle.
    pub fn get_center_cell_rectangle(&self) -> ElkRectangle {
        ElkRectangle::new(self.center_cell_rect.x, self.center_cell_rect.y, self.center_cell_rect.width, self.center_cell_rect.height)
    }

    // MARK: - Cell Methods

    pub fn get_minimum_width(&self) -> f64 {
        let mut width = 0.0;

        if self.only_center_cell_contributes_to_minimum_size {
            if let Some(size) = self.center_cell_minimum_size {
                width = size.x;
            } else if let Some(center_cell) = &self.cells[1][1] {
                width = center_cell.get_minimum_width();
            }
        } else if self.tabular {
            width = self.sum_with_gaps(&self.min_column_widths(None, true));
        } else {
            for area in ContainerArea::ALL {
                width = swift::max(width, self.sum_with_gaps(&self.min_column_widths(Some(area), true)));
            }
        }

        if width > 0.0 {
            width + self.cell.padding.left + self.cell.padding.right
        } else {
            0.0
        }
    }

    pub fn get_minimum_height(&self) -> f64 {
        let mut height = 0.0;

        if self.only_center_cell_contributes_to_minimum_size {
            if let Some(size) = self.center_cell_minimum_size {
                height = size.y;
            } else if let Some(center_cell) = &self.cells[1][1] {
                height = center_cell.get_minimum_height();
            }
        } else {
            height = self.sum_with_gaps(&self.min_row_heights(true));
        }

        if height > 0.0 {
            height + self.cell.padding.top + self.cell.padding.bottom
        } else {
            0.0
        }
    }

    pub fn layout_children_horizontally(&mut self) {
        if self.tabular {
            let col_widths = self.min_column_widths(None, false);
            for area in ContainerArea::ALL {
                self.apply_widths_to_row(area, &col_widths);
            }
        } else {
            for area in ContainerArea::ALL {
                let col_widths = self.min_column_widths(Some(area), false);
                self.apply_widths_to_row(area, &col_widths);
            }
        }
    }

    pub fn layout_children_vertically(&mut self) {
        let cell_rectangle = self.cell.cell_rectangle;
        let cell_padding = self.cell.padding;

        let row_heights = self.min_row_heights(false);

        self.apply_height_to_row(ContainerArea::BEGIN, cell_rectangle.y + cell_padding.top, &row_heights);
        self.apply_height_to_row(
            ContainerArea::END,
            cell_rectangle.y + cell_rectangle.height - cell_padding.bottom - row_heights[2],
            &row_heights,
        );

        let mut free_content_area_height = cell_rectangle.height - cell_padding.top - cell_padding.bottom;

        let mut adjusted_row_heights = row_heights;
        if adjusted_row_heights[0] > 0.0 {
            adjusted_row_heights[0] += self.gap;
            free_content_area_height -= adjusted_row_heights[0];
        }

        if adjusted_row_heights[2] > 0.0 {
            adjusted_row_heights[2] += self.gap;
            free_content_area_height -= adjusted_row_heights[2];
        }

        self.center_cell_rect.height = swift::max(0.0, free_content_area_height);
        self.center_cell_rect.y =
            cell_rectangle.y + cell_padding.top + (self.center_cell_rect.height - free_content_area_height) / 2.0;

        adjusted_row_heights[1] = swift::max(adjusted_row_heights[1], free_content_area_height);

        self.apply_height_to_row(
            ContainerArea::CENTER,
            cell_rectangle.y + cell_padding.top + adjusted_row_heights[0]
                - (adjusted_row_heights[1] - free_content_area_height) / 2.0,
            &adjusted_row_heights,
        );
    }

    // MARK: - Width and Height Calculations

    pub fn min_column_widths(&self, row: Option<ContainerArea>, respect_contribution_flag: bool) -> [f64; 3] {
        let mut col_widths = [
            self.min_width_of_column(ContainerArea::BEGIN, row, respect_contribution_flag),
            self.min_width_of_column(ContainerArea::CENTER, row, respect_contribution_flag),
            self.min_width_of_column(ContainerArea::END, row, respect_contribution_flag),
        ];

        if self.symmetrical {
            col_widths[0] = swift::max(col_widths[0], col_widths[2]);
            col_widths[2] = col_widths[0];
        }

        col_widths
    }

    pub fn min_width_of_column(&self, column: ContainerArea, row: Option<ContainerArea>, respect_contribution_flag: bool) -> f64 {
        let mut max_min_width = 0.0;

        match row {
            None => {
                for row_index in 0..Self::ROWS {
                    max_min_width = swift::max(
                        max_min_width,
                        ContainerCell::min_width_of_cell(self.cells[row_index][column.ordinal()].as_ref(), respect_contribution_flag),
                    );
                }
            }
            Some(row) => {
                max_min_width =
                    ContainerCell::min_width_of_cell(self.cells[row.ordinal()][column.ordinal()].as_ref(), respect_contribution_flag);
            }
        }

        if column == ContainerArea::CENTER {
            if let Some(size) = self.center_cell_minimum_size {
                max_min_width = swift::max(max_min_width, size.x);
            }
        }

        max_min_width
    }

    pub fn min_row_heights(&self, respect_contribution_flag: bool) -> [f64; 3] {
        let mut row_heights = [
            self.min_height_of_row(ContainerArea::BEGIN, respect_contribution_flag),
            self.min_height_of_row(ContainerArea::CENTER, respect_contribution_flag),
            self.min_height_of_row(ContainerArea::END, respect_contribution_flag),
        ];

        if self.symmetrical {
            row_heights[0] = swift::max(row_heights[0], row_heights[2]);
            row_heights[2] = row_heights[0];
        }

        row_heights
    }

    pub fn min_height_of_row(&self, row: ContainerArea, respect_contribution_flag: bool) -> f64 {
        let mut max_min_height = 0.0;
        for column in 0..Self::COLUMNS {
            max_min_height = swift::max(
                max_min_height,
                ContainerCell::min_height_of_cell(self.cells[row.ordinal()][column].as_ref(), respect_contribution_flag),
            );
        }

        if row == ContainerArea::CENTER {
            if let Some(size) = self.center_cell_minimum_size {
                max_min_height = swift::max(max_min_height, size.y);
            }
        }

        max_min_height
    }

    pub fn sum_with_gaps(&self, values: &[f64]) -> f64 {
        let mut sum = 0.0;
        let mut active_components = 0;

        for &val in values {
            if val > 0.0 {
                sum += val;
                active_components += 1;
            }
        }

        if active_components > 1 {
            sum += self.gap * (active_components - 1) as f64;
        }

        sum
    }

    // MARK: - Layout Application

    pub fn apply_widths_to_row(&mut self, row: ContainerArea, col_widths: &[f64; 3]) {
        let cell_rectangle = self.cell.cell_rectangle;
        let cell_padding = self.cell.padding;

        self.apply_width_to_column(ContainerArea::BEGIN, cell_rectangle.x + cell_padding.left, col_widths);
        self.apply_width_to_column(
            ContainerArea::END,
            cell_rectangle.x + cell_rectangle.width - cell_padding.right - col_widths[2],
            col_widths,
        );

        let mut free_content_area_width = cell_rectangle.width - cell_padding.left - cell_padding.right;

        let mut adjusted_col_widths = *col_widths;
        if adjusted_col_widths[0] > 0.0 {
            adjusted_col_widths[0] += self.gap;
            free_content_area_width -= adjusted_col_widths[0];
        }

        if adjusted_col_widths[2] > 0.0 {
            adjusted_col_widths[2] += self.gap;
            free_content_area_width -= adjusted_col_widths[2];
        }

        let center_width = swift::max(0.0, free_content_area_width);
        adjusted_col_widths[1] = swift::max(adjusted_col_widths[1], free_content_area_width);

        self.apply_width_to_column(
            ContainerArea::CENTER,
            cell_rectangle.x + cell_padding.left + adjusted_col_widths[0]
                - (adjusted_col_widths[1] - free_content_area_width) / 2.0,
            &adjusted_col_widths,
        );

        if row == ContainerArea::CENTER {
            self.center_cell_rect.width = center_width;
            self.center_cell_rect.x = cell_rectangle.x + cell_padding.left + (center_width - free_content_area_width) / 2.0;
        }
    }

    pub fn apply_width_to_column(&self, column: ContainerArea, x: f64, col_widths: &[f64; 3]) {
        for row in 0..Self::ROWS {
            ContainerCell::apply_horizontal_layout(self.cells[row][column.ordinal()].as_ref(), x, col_widths[column.ordinal()]);
        }
    }

    pub fn apply_height_to_row(&self, row: ContainerArea, y: f64, row_heights: &[f64; 3]) {
        for column in 0..Self::COLUMNS {
            ContainerCell::apply_vertical_layout(self.cells[row.ordinal()][column].as_ref(), y, row_heights[row.ordinal()]);
        }
    }
}
