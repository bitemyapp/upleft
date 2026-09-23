//! Port of `alg/common/nodespacing/internal/algorithm/NodeSizeCalculator.swift`.
//!
//! Configures the cell system according to the node size constraints and
//! determines the ultimate node size.

use super::node_label_and_size_utilities::NodeLabelAndSizeUtilities;
use crate::org::eclipse::elk::alg::common::nodespacing::internal::node_context::NodeContext;
use crate::org::eclipse::elk::core::options::size_constraint::SizeConstraint;
use crate::org::eclipse::elk::core::options::size_options::SizeOptions;
use crate::prelude::*;

pub struct NodeSizeCalculator;

impl NodeSizeCalculator {
    // MARK: - Node Width

    pub fn set_node_width(lg: &LGraphArena, node_context: &mut NodeContext) {
        let width: f64;

        if NodeLabelAndSizeUtilities::are_size_constraints_fixed(node_context) {
            width = node_context.node_size.x;
        } else {
            let mut w = if node_context.topdown_layout {
                swift::max(node_context.node_size.x, node_context.node_container.borrow().get_minimum_width())
            } else {
                node_context.node_container.borrow().get_minimum_width()
            };

            if node_context.size_constraints.contains(SizeConstraint::NODE_LABELS)
                && !node_context.size_options.contains(SizeOptions::OUTSIDE_NODE_LABELS_OVERHANG)
            {
                if let Some(north_container) = node_context.outside_node_label_container(PortSide::NORTH) {
                    w = swift::max(w, north_container.borrow().get_minimum_width());
                }
                if let Some(south_container) = node_context.outside_node_label_container(PortSide::SOUTH) {
                    w = swift::max(w, south_container.borrow().get_minimum_width());
                }
            }

            if let Some(min_node_size) = NodeLabelAndSizeUtilities::get_minimum_node_size(lg, node_context) {
                w = swift::max(w, min_node_size.x);
            }
            width = w;
        }

        // Set the node's width
        let graph = node_context.node.get_graph();
        let fixed_graph_size: bool =
            graph.and_then(|g| g.get_property::<bool>(lg, &CoreOptions::NODE_SIZE_FIXED_GRAPH_SIZE)).unwrap_or(false);
        if fixed_graph_size {
            node_context.node_size.x = swift::max(node_context.node_size.x, width);
        } else {
            node_context.node_size.x = width;
        }

        let mut node_container = node_context.node_container.borrow_mut();
        node_container.cell.cell_rectangle.x = 0.0;
        node_container.cell.cell_rectangle.width = width;

        node_container.layout_children_horizontally();
    }

    // MARK: - Node Height

    pub fn set_node_height(lg: &LGraphArena, node_context: &mut NodeContext) {
        let height: f64;

        if NodeLabelAndSizeUtilities::are_size_constraints_fixed(node_context) {
            height = node_context.node_size.y;
        } else {
            let mut h = if node_context.topdown_layout {
                swift::max(node_context.node_size.y, node_context.node_container.borrow().get_minimum_height())
            } else {
                node_context.node_container.borrow().get_minimum_height()
            };

            if node_context.size_constraints.contains(SizeConstraint::NODE_LABELS)
                && !node_context.size_options.contains(SizeOptions::OUTSIDE_NODE_LABELS_OVERHANG)
            {
                if let Some(east_container) = node_context.outside_node_label_container(PortSide::EAST) {
                    h = swift::max(h, east_container.borrow().get_minimum_height());
                }
                if let Some(west_container) = node_context.outside_node_label_container(PortSide::WEST) {
                    h = swift::max(h, west_container.borrow().get_minimum_height());
                }
            }

            if let Some(min_node_size) = NodeLabelAndSizeUtilities::get_minimum_node_size(lg, node_context) {
                h = swift::max(h, min_node_size.y);
            }

            if node_context.size_constraints.contains(SizeConstraint::PORTS) {
                if node_context.port_constraints == PortConstraints::FIXED_RATIO
                    || node_context.port_constraints == PortConstraints::FIXED_POS
                {
                    if let Some(east_cell) = node_context.inside_port_label_cell(PortSide::EAST) {
                        h = swift::max(h, east_cell.borrow().get_minimum_height());
                    }
                    if let Some(west_cell) = node_context.inside_port_label_cell(PortSide::WEST) {
                        h = swift::max(h, west_cell.borrow().get_minimum_height());
                    }
                }
            }
            height = h;
        }

        // Set the node's height
        let graph = node_context.node.get_graph();
        let fixed_graph_size: bool =
            graph.and_then(|g| g.get_property::<bool>(lg, &CoreOptions::NODE_SIZE_FIXED_GRAPH_SIZE)).unwrap_or(false);
        if fixed_graph_size {
            node_context.node_size.y = swift::max(node_context.node_size.y, height);
        } else {
            node_context.node_size.y = height;
        }

        let mut node_container = node_context.node_container.borrow_mut();
        node_container.cell.cell_rectangle.y = 0.0;
        node_container.cell.cell_rectangle.height = height;

        node_container.layout_children_vertically();
    }
}
