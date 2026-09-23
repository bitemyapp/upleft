//! Port of `alg/common/nodespacing/internal/algorithm/InsidePortLabelCellCreator.swift`.
//!
//! Sets up the inside port label cells.

use std::rc::Rc;

use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::atomic_cell::AtomicCell;
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::cell::CellRef;
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::container_area::ContainerArea;
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::strip_container_cell::StripContainerCellRef;
use crate::org::eclipse::elk::alg::common::nodespacing::internal::node_context::NodeContext;
use crate::org::eclipse::elk::core::options::port_label_placement::PortLabelPlacement;
use crate::prelude::*;

pub struct InsidePortLabelCellCreator;

impl InsidePortLabelCellCreator {
    pub fn create_inside_port_label_cells(node_context: &mut NodeContext) {
        let node_container = Rc::clone(&node_context.node_container);
        let middle_row = Rc::clone(&node_context.node_container_middle_row);
        Self::create_inside_port_label_cell(node_context, &node_container, ContainerArea::BEGIN, PortSide::NORTH);
        Self::create_inside_port_label_cell(node_context, &node_container, ContainerArea::END, PortSide::SOUTH);

        Self::create_inside_port_label_cell(node_context, &middle_row, ContainerArea::BEGIN, PortSide::WEST);
        Self::create_inside_port_label_cell(node_context, &middle_row, ContainerArea::END, PortSide::EAST);

        Self::setup_north_or_south_port_label_cell(node_context, PortSide::NORTH);
        Self::setup_north_or_south_port_label_cell(node_context, PortSide::SOUTH);
        Self::setup_east_or_west_port_label_cell(node_context, PortSide::EAST);
        Self::setup_east_or_west_port_label_cell(node_context, PortSide::WEST);
    }

    pub fn create_inside_port_label_cell(
        node_context: &mut NodeContext,
        container: &StripContainerCellRef,
        container_area: ContainerArea,
        port_side: PortSide,
    ) {
        let port_label_cell = AtomicCell::new_ref();
        container.borrow_mut().set_cell(container_area, Some(CellRef::Atomic(Rc::clone(&port_label_cell))));
        node_context.inside_port_label_cells[port_side.ordinal()] = Some(port_label_cell);
    }

    // North or South

    pub fn setup_north_or_south_port_label_cell(node_context: &NodeContext, port_side: PortSide) {
        let Some(cell) = node_context.inside_port_label_cell(port_side) else { return };
        let mut cell = cell.borrow_mut();
        let padding = &mut cell.cell.padding;

        match port_side {
            PortSide::NORTH => {
                if node_context.port_label_spacing_vertical >= 0.0 {
                    padding.top = node_context.port_label_spacing_vertical;
                }
            }
            PortSide::SOUTH => {
                if node_context.port_label_spacing_vertical >= 0.0 {
                    padding.bottom = node_context.port_label_spacing_vertical;
                }
            }
            _ => {}
        }

        let surrounding_port_margins = node_context.surrounding_port_margins;
        padding.left = surrounding_port_margins.left;
        padding.right = surrounding_port_margins.right;
    }

    // East or West

    pub fn setup_east_or_west_port_label_cell(node_context: &NodeContext, port_side: PortSide) {
        if node_context.port_labels_placement.contains(PortLabelPlacement::INSIDE) {
            Self::calculate_width_due_to_labels(node_context, port_side);
        }
        Self::setup_top_and_bottom_padding(node_context, port_side);
    }

    pub fn calculate_width_due_to_labels(node_context: &NodeContext, port_side: PortSide) {
        let Some(the_appropriate_cell) = node_context.inside_port_label_cell(port_side) else { return };
        let mut cell = the_appropriate_cell.borrow_mut();

        for port_context in node_context.port_contexts.get_or_empty(port_side) {
            if let Some(port_label_cell) = &port_context.port_label_cell {
                cell.minimum_content_area_size.x = swift::max(cell.minimum_content_area_size.x, port_label_cell.get_minimum_width());
            }
        }

        if cell.minimum_content_area_size.x > 0.0 {
            match port_side {
                PortSide::EAST => cell.cell.padding.right = node_context.port_label_spacing_horizontal,
                PortSide::WEST => cell.cell.padding.left = node_context.port_label_spacing_horizontal,
                _ => {}
            }
        }
    }

    pub fn setup_top_and_bottom_padding(node_context: &NodeContext, port_side: PortSide) {
        let surrounding_port_margins = node_context.surrounding_port_margins;
        let Some(cell) = node_context.inside_port_label_cell(port_side) else { return };
        let mut cell = cell.borrow_mut();
        let padding = &mut cell.cell.padding;
        padding.top = surrounding_port_margins.top;
        padding.bottom = surrounding_port_margins.bottom;
    }
}
