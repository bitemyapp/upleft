//! Port of `alg/common/nodespacing/cellsystem/LabelCell.swift`.
//!
//! A cell which manages the size and placement of labels. The labels are
//! adapters (arena ids); reading their sizes and writing their positions takes
//! the arena. `applyLabelLayout` hands each label a fresh position vector
//! (`label.setPosition(labelPos)` with a new local `KVector`), so no aliasing
//! is left behind.

use std::cell::RefCell;
use std::rc::Rc;

use super::cell::Cell;
use super::horizontal_label_alignment::HorizontalLabelAlignment;
use super::vertical_label_alignment::VerticalLabelAlignment;
use crate::org::eclipse::elk::alg::common::nodespacing::internal::node_label_location::NodeLabelLocation;
use crate::org::eclipse::elk::alg::layered::graph::l_graph_adapters::LLabelAdapter;
use crate::prelude::*;

/// A shared `LabelCell` reference.
pub type LabelCellRef = Rc<RefCell<LabelCell>>;

#[derive(Clone, Debug)]
pub struct LabelCell {
    pub cell: Cell,
    pub horizontal_layout_mode: bool,
    pub horizontal_alignment: HorizontalLabelAlignment,
    pub vertical_alignment: VerticalLabelAlignment,
    pub gap: f64,
    pub labels: Vec<LLabelAdapter>,
    pub minimum_content_area_size: KVector,
}

impl LabelCell {
    /// `init(gap:)` (horizontal layout mode).
    pub fn new(gap: f64) -> LabelCell {
        LabelCell::with_mode(gap, true)
    }

    /// `init(gap:horizontalLayoutMode:)`.
    pub fn with_mode(gap: f64, horizontal_layout_mode: bool) -> LabelCell {
        LabelCell {
            cell: Cell::default(),
            horizontal_layout_mode,
            horizontal_alignment: HorizontalLabelAlignment::CENTER,
            vertical_alignment: VerticalLabelAlignment::CENTER,
            gap,
            labels: Vec::new(),
            minimum_content_area_size: KVector::default(),
        }
    }

    /// `init(gap:nodeLabelLocation:horizontalLayoutMode:)`.
    pub fn with_location(gap: f64, node_label_location: NodeLabelLocation, horizontal_layout_mode: bool) -> LabelCell {
        let mut cell = LabelCell::with_mode(gap, horizontal_layout_mode);
        cell.horizontal_alignment = node_label_location.horizontal_alignment();
        cell.vertical_alignment = node_label_location.vertical_alignment();
        cell
    }

    pub fn into_ref(self) -> LabelCellRef {
        Rc::new(RefCell::new(self))
    }

    pub fn get_horizontal_alignment(&self) -> HorizontalLabelAlignment {
        self.horizontal_alignment
    }

    pub fn set_horizontal_alignment(&mut self, new_horizontal_alignment: HorizontalLabelAlignment) -> &mut LabelCell {
        self.horizontal_alignment = new_horizontal_alignment;
        self
    }

    pub fn get_vertical_alignment(&self) -> VerticalLabelAlignment {
        self.vertical_alignment
    }

    pub fn set_vertical_alignment(&mut self, new_vertical_alignment: VerticalLabelAlignment) -> &mut LabelCell {
        self.vertical_alignment = new_vertical_alignment;
        self
    }

    pub fn get_labels(&self) -> &[LLabelAdapter] {
        &self.labels
    }

    // MARK: - Cell

    pub fn get_minimum_width(&self) -> f64 {
        let padding = &self.cell.padding;
        self.minimum_content_area_size.x + padding.left + padding.right
    }

    pub fn get_minimum_height(&self) -> f64 {
        let padding = &self.cell.padding;
        self.minimum_content_area_size.y + padding.top + padding.bottom
    }

    // MARK: - Adding Labels

    pub fn add_label(&mut self, lg: &LGraphArena, label: LLabelAdapter) {
        self.labels.push(label);

        let label_size = label.get_size(lg);

        if self.horizontal_layout_mode {
            self.minimum_content_area_size.x = swift::max(self.minimum_content_area_size.x, label_size.x);
            self.minimum_content_area_size.y += label_size.y;

            if self.labels.len() > 1 {
                self.minimum_content_area_size.y += self.gap;
            }
        } else {
            self.minimum_content_area_size.x += label_size.x;
            self.minimum_content_area_size.y = swift::max(self.minimum_content_area_size.y, label_size.y);

            if self.labels.len() > 1 {
                self.minimum_content_area_size.x += self.gap;
            }
        }
    }

    pub fn has_labels(&self) -> bool {
        !self.labels.is_empty()
    }

    // MARK: - Label Layout

    pub fn apply_label_layout(&self, lg: &mut LGraphArena) {
        if self.horizontal_layout_mode {
            self.apply_horizontal_mode_label_layout(lg);
        } else {
            self.apply_vertical_mode_label_layout(lg);
        }
    }

    pub fn apply_horizontal_mode_label_layout(&self, lg: &mut LGraphArena) {
        let cell_rect = self.cell.cell_rectangle;
        let cell_padding = self.cell.padding;

        let mut y_pos = cell_rect.y;

        if self.vertical_alignment == VerticalLabelAlignment::CENTER {
            y_pos += (cell_rect.height - self.minimum_content_area_size.y) / 2.0;
        } else if self.vertical_alignment == VerticalLabelAlignment::BOTTOM {
            y_pos += cell_rect.height - self.minimum_content_area_size.y;
        }

        for label in &self.labels {
            let label_size = label.get_size(lg);
            let mut label_pos = KVector::default();

            label_pos.y = y_pos;
            y_pos += label_size.y + self.gap;

            match self.horizontal_alignment {
                HorizontalLabelAlignment::LEFT => {
                    label_pos.x = cell_rect.x + cell_padding.left;
                }
                HorizontalLabelAlignment::CENTER => {
                    label_pos.x = cell_rect.x + cell_padding.left + (cell_rect.width - label_size.x) / 2.0;
                }
                HorizontalLabelAlignment::RIGHT => {
                    label_pos.x = cell_rect.x + cell_rect.width - cell_padding.right - label_size.x;
                }
            }

            label.set_position(lg, label_pos);
        }
    }

    pub fn apply_vertical_mode_label_layout(&self, lg: &mut LGraphArena) {
        let cell_rect = self.cell.cell_rectangle;
        let cell_padding = self.cell.padding;

        let mut x_pos = cell_rect.x;

        if self.horizontal_alignment == HorizontalLabelAlignment::CENTER {
            x_pos += (cell_rect.width - self.minimum_content_area_size.x) / 2.0;
        } else if self.horizontal_alignment == HorizontalLabelAlignment::RIGHT {
            x_pos += cell_rect.width - self.minimum_content_area_size.x;
        }

        for label in &self.labels {
            let label_size = label.get_size(lg);
            let mut label_pos = KVector::default();

            label_pos.x = x_pos;
            x_pos += label_size.x + self.gap;

            match self.vertical_alignment {
                VerticalLabelAlignment::TOP => {
                    label_pos.y = cell_rect.y + cell_padding.top;
                }
                VerticalLabelAlignment::CENTER => {
                    label_pos.y = cell_rect.y + cell_padding.top + (cell_rect.height - label_size.y) / 2.0;
                }
                VerticalLabelAlignment::BOTTOM => {
                    label_pos.y = cell_rect.y + cell_rect.height - cell_padding.bottom - label_size.y;
                }
            }

            label.set_position(lg, label_pos);
        }
    }
}
