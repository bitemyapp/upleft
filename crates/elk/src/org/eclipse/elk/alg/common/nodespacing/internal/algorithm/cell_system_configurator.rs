//! Port of `alg/common/nodespacing/internal/algorithm/CellSystemConfigurator.swift`.
//!
//! Configures constraints of the cell system such that the various cells
//! contribute properly to the node size calculation.

use crate::org::eclipse::elk::alg::common::nodespacing::internal::node_context::NodeContext;
use crate::org::eclipse::elk::alg::common::nodespacing::internal::node_label_location::NodeLabelLocation;
use crate::org::eclipse::elk::core::options::size_constraint::SizeConstraint;
use crate::org::eclipse::elk::core::options::size_options::SizeOptions;
use crate::prelude::*;

pub struct CellSystemConfigurator;

impl CellSystemConfigurator {
    // MARK: - Size Contribution Configuration

    pub fn configure_cell_system_size_contributions(node_context: &NodeContext) {
        if node_context.size_constraints.is_empty() {
            return;
        }

        let set_width = |side: PortSide, value: bool| {
            if let Some(c) = node_context.inside_port_label_cell(side) {
                c.borrow_mut().cell.contributes_to_minimum_width = value;
            }
        };
        let set_height = |side: PortSide, value: bool| {
            if let Some(c) = node_context.inside_port_label_cell(side) {
                c.borrow_mut().cell.contributes_to_minimum_height = value;
            }
        };

        if node_context.size_constraints.contains(SizeConstraint::PORTS) {
            set_width(PortSide::NORTH, true);
            set_width(PortSide::SOUTH, true);

            let free_port_placement = node_context.port_constraints != PortConstraints::FIXED_RATIO
                && node_context.port_constraints != PortConstraints::FIXED_POS;

            set_height(PortSide::EAST, free_port_placement);
            set_height(PortSide::WEST, free_port_placement);

            node_context.node_container_middle_row.borrow_mut().cell.contributes_to_minimum_height = free_port_placement;

            if node_context.size_constraints.contains(SizeConstraint::PORT_LABELS) {
                set_height(PortSide::NORTH, true);
                set_height(PortSide::SOUTH, true);
                set_width(PortSide::EAST, true);
                set_width(PortSide::WEST, true);

                node_context.node_container_middle_row.borrow_mut().cell.contributes_to_minimum_width = true;
            }
        }

        if node_context.size_constraints.contains(SizeConstraint::NODE_LABELS) {
            if let Some(container) = &node_context.inside_node_label_container {
                let mut container = container.borrow_mut();
                container.cell.contributes_to_minimum_height = true;
                container.cell.contributes_to_minimum_width = true;
            }

            {
                let mut middle_row = node_context.node_container_middle_row.borrow_mut();
                middle_row.cell.contributes_to_minimum_height = true;
                middle_row.cell.contributes_to_minimum_width = true;
            }

            let overhang = node_context.size_options.contains(SizeOptions::OUTSIDE_NODE_LABELS_OVERHANG);
            for location in NodeLabelLocation::ALL {
                let Some(label_cell) = node_context.node_label_cell(location) else { continue };
                let mut label_cell = label_cell.borrow_mut();
                if location.is_inside_location() {
                    label_cell.cell.contributes_to_minimum_height = true;
                    label_cell.cell.contributes_to_minimum_width = true;
                } else {
                    label_cell.cell.contributes_to_minimum_height = !overhang;
                    label_cell.cell.contributes_to_minimum_width = !overhang;
                }
            }
        }

        if node_context.size_constraints.contains(SizeConstraint::MINIMUM_SIZE)
            && node_context.size_options.contains(SizeOptions::MINIMUM_SIZE_ACCOUNTS_FOR_PADDING)
        {
            {
                let mut middle_row = node_context.node_container_middle_row.borrow_mut();
                middle_row.cell.contributes_to_minimum_height = true;
                middle_row.cell.contributes_to_minimum_width = true;
            }

            if let Some(container) = &node_context.inside_node_label_container {
                let mut container = container.borrow_mut();
                if !container.cell.contributes_to_minimum_height {
                    container.cell.contributes_to_minimum_height = true;
                    container.cell.contributes_to_minimum_width = true;
                    container.only_center_cell_contributes_to_minimum_size = true;
                }
            }
        }
    }

    // MARK: - Update East and West Inside Port Label Cells

    pub fn update_vertical_inside_port_label_cell_padding(node_context: &NodeContext) {
        if node_context.port_constraints == PortConstraints::FIXED_RATIO
            || node_context.port_constraints == PortConstraints::FIXED_POS
        {
            return;
        }

        let container_padding = node_context.node_container.borrow().cell.padding;
        let top_border_offset = container_padding.top
            + node_context.inside_port_label_cell(PortSide::NORTH).map(|c| c.borrow().get_minimum_height()).unwrap_or(0.0)
            + node_context.label_cell_spacing;
        let bottom_border_offset = container_padding.bottom
            + node_context.inside_port_label_cell(PortSide::SOUTH).map(|c| c.borrow().get_minimum_height()).unwrap_or(0.0)
            + node_context.label_cell_spacing;

        let (Some(east_cell), Some(west_cell)) =
            (node_context.inside_port_label_cell(PortSide::EAST), node_context.inside_port_label_cell(PortSide::WEST))
        else {
            return;
        };

        let east_padding = east_cell.borrow().cell.padding;
        let west_padding = west_cell.borrow().cell.padding;
        let mut top_padding = swift::max(0.0, east_padding.top - top_border_offset);
        top_padding = swift::max(top_padding, west_padding.top - top_border_offset);
        let mut bottom_padding = swift::max(0.0, east_padding.bottom - bottom_border_offset);
        bottom_padding = swift::max(bottom_padding, west_padding.bottom - bottom_border_offset);

        east_cell.borrow_mut().cell.padding.top = top_padding;
        west_cell.borrow_mut().cell.padding.top = top_padding;
        east_cell.borrow_mut().cell.padding.bottom = bottom_padding;
        west_cell.borrow_mut().cell.padding.bottom = bottom_padding;
    }
}
