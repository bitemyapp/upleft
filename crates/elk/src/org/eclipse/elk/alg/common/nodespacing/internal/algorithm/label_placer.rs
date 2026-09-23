//! Port of `alg/common/nodespacing/internal/algorithm/LabelPlacer.swift`.
//!
//! Knows how to properly size and position outer node label containers and to
//! place node and port labels.

use crate::org::eclipse::elk::alg::common::nodespacing::internal::node_context::NodeContext;
use crate::org::eclipse::elk::core::options::size_options::SizeOptions;
use crate::prelude::*;

pub struct LabelPlacer;

impl LabelPlacer {
    /// Places outer node label containers as well as all labels.
    pub fn place_labels(lg: &mut LGraphArena, node_context: &NodeContext) {
        Self::place_outer_node_label_containers(node_context);

        // Swift iterates the `nodeLabelCells` dictionary in hash order. Each
        // label cell only positions its own labels (every node label is in
        // exactly one cell) from its own rectangle, so the order cannot affect
        // the result; the port uses `NodeLabelLocation` order.
        for label_cell in node_context.node_label_cells.iter().flatten() {
            label_cell.borrow().apply_label_layout(lg);
        }

        for (_, port_context_list) in node_context.port_contexts.iter() {
            for pc in port_context_list {
                if let Some(port_label_cell) = &pc.port_label_cell {
                    port_label_cell.apply_label_layout(lg);
                }
            }
        }
    }

    pub fn place_outer_node_label_containers(node_context: &NodeContext) {
        let outer_node_labels_overhang = node_context.size_options.contains(SizeOptions::OUTSIDE_NODE_LABELS_OVERHANG);

        Self::place_horizontal_outer_node_label_container(node_context, outer_node_labels_overhang, PortSide::NORTH);
        Self::place_horizontal_outer_node_label_container(node_context, outer_node_labels_overhang, PortSide::SOUTH);
        Self::place_vertical_outer_node_label_container(node_context, outer_node_labels_overhang, PortSide::EAST);
        Self::place_vertical_outer_node_label_container(node_context, outer_node_labels_overhang, PortSide::WEST);
    }

    pub fn place_horizontal_outer_node_label_container(
        node_context: &NodeContext,
        outer_node_labels_overhang: bool,
        port_side: PortSide,
    ) {
        let node_size = node_context.node_size;
        let Some(node_label_container) = node_context.outside_node_label_container(port_side) else { return };
        let mut node_label_container = node_label_container.borrow_mut();

        let min_width = node_label_container.get_minimum_width();
        let min_height = node_label_container.get_minimum_height();
        let rect = &mut node_label_container.cell.cell_rectangle;
        rect.width = min_width;
        rect.height = min_height;

        rect.width = swift::max(rect.width, node_size.x);

        if rect.width > node_size.x && !outer_node_labels_overhang {
            rect.width = node_size.x;
        }

        rect.x = -(rect.width - node_size.x) / 2.0;

        match port_side {
            PortSide::NORTH => rect.y = -rect.height,
            PortSide::SOUTH => rect.y = node_size.y,
            _ => {}
        }

        node_label_container.layout_children_horizontally();
        node_label_container.layout_children_vertically();
    }

    pub fn place_vertical_outer_node_label_container(
        node_context: &NodeContext,
        outer_node_labels_overhang: bool,
        port_side: PortSide,
    ) {
        let node_size = node_context.node_size;
        let Some(node_label_container) = node_context.outside_node_label_container(port_side) else { return };
        let mut node_label_container = node_label_container.borrow_mut();

        let min_width = node_label_container.get_minimum_width();
        let min_height = node_label_container.get_minimum_height();
        let rect = &mut node_label_container.cell.cell_rectangle;
        rect.width = min_width;
        rect.height = min_height;

        rect.height = swift::max(rect.height, node_size.y);

        if rect.height > node_size.y && !outer_node_labels_overhang {
            rect.height = node_size.y;
        }

        rect.y = -(rect.height - node_size.y) / 2.0;

        match port_side {
            PortSide::WEST => rect.x = -rect.width,
            PortSide::EAST => rect.x = node_size.x,
            _ => {}
        }

        node_label_container.layout_children_horizontally();
        node_label_container.layout_children_vertically();
    }
}
