//! Port of `alg/common/nodespacing/NodeMarginCalculator.swift`.
//!
//! Sets the node margins. Node margins are influenced by both port positions
//! and sizes and label positions and sizes.
//!
//! Usage as in Swift, e.g. `NodeDimensionCalculation.getNodeMarginCalculator(
//! LGraphAdapters.adapt(graph, transparentNorthSouthEdges: false)).process(
//! node: LGraphAdapters.adapt(dummy, transparentNorthSouthEdges: false))`:
//!
//! ```ignore
//! NodeDimensionCalculation::get_node_margin_calculator(LGraphAdapters::adapt_ns(graph, false))
//!     .process_node(lg, &LGraphAdapters::adapt_node(dummy, false));
//! ```

use crate::org::eclipse::elk::alg::layered::graph::l_graph_adapters::{
    LEdgeAdapter, LGraphAdapter, LLabelAdapter, LNodeAdapter, LPortAdapter,
};
use crate::org::eclipse::elk::core::math::elk_rectangle::ElkRectangle;
use crate::org::eclipse::elk::core::options::edge_label_placement::EdgeLabelPlacement;
use crate::org::eclipse::elk::core::options::port_label_placement::PortLabelPlacement;
use crate::prelude::*;

pub struct NodeMarginCalculator {
    pub include_labels: bool,
    pub include_ports: bool,
    pub include_port_labels: bool,
    pub include_edge_head_tail_labels: bool,
    pub adapter: LGraphAdapter,
}

impl NodeMarginCalculator {
    pub fn new(adapter: LGraphAdapter) -> NodeMarginCalculator {
        NodeMarginCalculator {
            include_labels: true,
            include_ports: true,
            include_port_labels: true,
            include_edge_head_tail_labels: true,
            adapter,
        }
    }

    // MARK: - Configure

    pub fn exclude_labels(mut self) -> NodeMarginCalculator {
        self.include_labels = false;
        self
    }

    pub fn exclude_ports(mut self) -> NodeMarginCalculator {
        self.include_ports = false;
        self
    }

    pub fn exclude_port_labels(mut self) -> NodeMarginCalculator {
        self.include_port_labels = false;
        self
    }

    pub fn exclude_edge_head_tail_labels(mut self) -> NodeMarginCalculator {
        self.include_edge_head_tail_labels = false;
        self
    }

    // MARK: - Process

    /// `process()`: all nodes of the adapted graph.
    pub fn process(&self, lg: &mut LGraphArena) {
        let spacing: f64 = self.adapter.get_property::<f64>(lg, &CoreOptions::SPACING_LABEL_NODE).unwrap_or(0.0);

        for node in self.adapter.get_nodes(lg) {
            self.process_node_with_spacing(lg, &node, spacing);
        }
    }

    /// `process(node:)`.
    pub fn process_node(&self, lg: &mut LGraphArena, node: &LNodeAdapter) {
        let spacing: f64 = self.adapter.get_property::<f64>(lg, &CoreOptions::SPACING_LABEL_NODE).unwrap_or(0.0);
        self.process_node_with_spacing(lg, node, spacing);
    }

    /// `process(node:spacing:)`.
    pub fn process_node_with_spacing(&self, lg: &mut LGraphArena, node: &LNodeAdapter, label_spacing: f64) {
        let node_pos = node.get_position(lg);
        let node_size = node.get_size(lg);

        let mut bounding_box = ElkRectangle::new(node_pos.x, node_pos.y, node_size.x, node_size.y);

        let mut element_box = ElkRectangle::default();

        // Put the node's labels into the bounding box
        if self.include_labels {
            for label in node.get_labels(lg) {
                let label_pos = label.get_position(lg);
                let label_size = label.get_size(lg);
                element_box.x = label_pos.x + node_pos.x;
                element_box.y = label_pos.y + node_pos.y;
                element_box.width = label_size.x;
                element_box.height = label_size.y;
                bounding_box.union(&element_box);
            }
        }

        // Do the same for ports and their labels
        for port in node.get_ports(lg) {
            let port_pos = port.get_position(lg);
            let port_size = port.get_size(lg);
            let port_x = port_pos.x + node_pos.x;
            let port_y = port_pos.y + node_pos.y;

            if self.include_ports {
                element_box.x = port_x;
                element_box.y = port_y;
                element_box.width = port_size.x;
                element_box.height = port_size.y;
                bounding_box.union(&element_box);
            }

            if self.include_port_labels {
                for label in port.get_labels(lg) {
                    let label_pos = label.get_position(lg);
                    let label_size = label.get_size(lg);
                    element_box.x = label_pos.x + port_x;
                    element_box.y = label_pos.y + port_y;
                    element_box.width = label_size.x;
                    element_box.height = label_size.y;
                    bounding_box.union(&element_box);
                }
            }

            if self.include_edge_head_tail_labels {
                let mut required_port_label_space = KVector::new(-label_spacing, -label_spacing);

                let port_labels_placement: PortLabelPlacement = node
                    .get_property::<PortLabelPlacement>(lg, &CoreOptions::PORT_LABELS_PLACEMENT)
                    .unwrap_or(PortLabelPlacement::empty());
                if port_labels_placement.contains(PortLabelPlacement::OUTSIDE) {
                    for label in port.get_labels(lg) {
                        let label_size = label.get_size(lg);
                        required_port_label_space.x += label_size.x + label_spacing;
                        required_port_label_space.y += label_size.y + label_spacing;
                    }
                }

                required_port_label_space.x = swift::max(required_port_label_space.x, 0.0);
                required_port_label_space.y = swift::max(required_port_label_space.y, 0.0);

                self.process_edge_head_tail_labels(
                    lg,
                    &mut bounding_box,
                    &port.get_outgoing_edges(lg),
                    &port.get_incoming_edges(lg),
                    node,
                    Some(&port),
                    Some(required_port_label_space),
                    label_spacing,
                );
            }
        }

        // Process end labels of edges directly connected to the node
        if self.include_edge_head_tail_labels {
            self.process_edge_head_tail_labels(
                lg,
                &mut bounding_box,
                &node.get_outgoing_edges(lg),
                &node.get_incoming_edges(lg),
                node,
                None,
                None,
                label_spacing,
            );
        }

        // Reset the margin
        let mut margin = node.get_margin(lg);
        margin.top = swift::max(0.0, node_pos.y - bounding_box.y);
        margin.bottom = swift::max(0.0, bounding_box.y + bounding_box.height - (node_pos.y + node_size.y));
        margin.left = swift::max(0.0, node_pos.x - bounding_box.x);
        margin.right = swift::max(0.0, bounding_box.x + bounding_box.width - (node_pos.x + node_size.x));
        node.set_margin(lg, &margin);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process_edge_head_tail_labels(
        &self,
        lg: &LGraphArena,
        bounding_box: &mut ElkRectangle,
        outgoing_edges: &[LEdgeAdapter],
        incoming_edges: &[LEdgeAdapter],
        node: &LNodeAdapter,
        port: Option<&LPortAdapter>,
        port_label_space: Option<KVector>,
        label_spacing: f64,
    ) {
        let mut label_box = ElkRectangle::default();

        for edge in outgoing_edges {
            for label in edge.get_labels(lg) {
                let placement: Option<EdgeLabelPlacement> = label.get_property(lg, &CoreOptions::EDGE_LABELS_PLACEMENT);
                if placement == Some(EdgeLabelPlacement::TAIL) {
                    self.compute_label_box(lg, &mut label_box, &label, false, node, port, port_label_space, label_spacing);
                    bounding_box.union(&label_box);
                }
            }
        }

        for edge in incoming_edges {
            for label in edge.get_labels(lg) {
                let placement: Option<EdgeLabelPlacement> = label.get_property(lg, &CoreOptions::EDGE_LABELS_PLACEMENT);
                if placement == Some(EdgeLabelPlacement::HEAD) {
                    self.compute_label_box(lg, &mut label_box, &label, true, node, port, port_label_space, label_spacing);
                    bounding_box.union(&label_box);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn compute_label_box(
        &self,
        lg: &LGraphArena,
        label_box: &mut ElkRectangle,
        label: &LLabelAdapter,
        incoming_edge: bool,
        node: &LNodeAdapter,
        port: Option<&LPortAdapter>,
        port_label_space: Option<KVector>,
        label_spacing: f64,
    ) {
        let node_pos = node.get_position(lg);
        let node_size = node.get_size(lg);
        let label_size = label.get_size(lg);

        label_box.x = node_pos.x;
        label_box.y = node_pos.y;
        if let Some(port) = port {
            let port_pos = port.get_position(lg);
            label_box.x += port_pos.x;
            label_box.y += port_pos.y;
        }

        label_box.width = label_size.x;
        label_box.height = label_size.y;

        let space_x = port_label_space.map(|s| s.x).unwrap_or(0.0);
        let space_y = port_label_space.map(|s| s.y).unwrap_or(0.0);

        match port {
            None => {
                if incoming_edge {
                    label_box.x -= label_spacing + label_size.x;
                } else {
                    label_box.x += node_size.x + label_spacing;
                }
            }
            Some(port) => {
                let port_size = port.get_size(lg);
                let port_side = port.get_side(lg);
                match port_side {
                    PortSide::UNDEFINED | PortSide::EAST => {
                        label_box.x += port_size.x + label_spacing + space_x + label_spacing;
                    }
                    PortSide::WEST => {
                        label_box.x -= label_spacing + space_x + label_spacing + label_size.x;
                    }
                    PortSide::NORTH => {
                        label_box.x += port_size.x + label_spacing;
                        label_box.y -= label_spacing + space_y + label_spacing + label_size.y;
                    }
                    PortSide::SOUTH => {
                        label_box.x += port_size.x + label_spacing;
                        label_box.y += port_size.y + label_spacing + space_y + label_spacing;
                    }
                }
            }
        }
    }
}
