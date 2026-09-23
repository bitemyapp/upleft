//! Port of `alg/layered/intermediate/LabelAndNodeSizeProcessor.swift`.
//!
//! Calculates node sizes, places ports, and places node and port labels.

use crate::org::eclipse::elk::alg::common::nodespacing::node_dimension_calculation::NodeDimensionCalculation;
use crate::org::eclipse::elk::alg::layered::graph::l_graph_adapters::LGraphAdapters;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::math::elk_rectangle::ElkRectangle;
use crate::org::eclipse::elk::core::options::port_label_placement::PortLabelPlacement;
use crate::prelude::*;

#[derive(Default)]
pub struct LabelAndNodeSizeProcessor;

/// The node filter `{ $0.type == .normal }`.
fn is_normal_node(lg: &LGraphArena, node: LNodeId) -> bool {
    lg[node].node_type == NodeType::NORMAL
}

impl LabelAndNodeSizeProcessor {
    pub fn new() -> LabelAndNodeSizeProcessor {
        LabelAndNodeSizeProcessor
    }

    /// Places the labels of the given external port dummy such that it results
    /// in correct node margins later on that will reserve enough space for the
    /// labels.
    fn place_external_port_dummy_labels(
        &self,
        lg: &mut LGraphArena,
        dummy: LNodeId,
        graph_port_label_placement: PortLabelPlacement,
        place_next_to_port_if_possible: bool,
        treat_as_group: bool,
    ) {
        let label_port_spacing_horizontal: f64 =
            lg[dummy].props.get_typed::<f64>(&LayeredOptions::SPACING_LABEL_PORT_HORIZONTAL).unwrap_or(0.0);
        let label_port_spacing_vertical: f64 =
            lg[dummy].props.get_typed::<f64>(&LayeredOptions::SPACING_LABEL_PORT_VERTICAL).unwrap_or(0.0);
        let label_label_spacing: f64 = lg[dummy].props.get_typed::<f64>(&LayeredOptions::SPACING_LABEL_LABEL).unwrap_or(0.0);

        let dummy_size = lg[dummy].size;

        // External port dummies have exactly one port
        let Some(&dummy_port) = lg[dummy].ports.first() else { return };
        let dummy_port_pos = lg[dummy_port].position;

        let Some(mut port_label_box) = self.compute_port_label_box(lg, dummy_port, label_label_spacing) else { return };

        let first_label_height = || lg[dummy_port].labels.first().map(|&l| lg[l].size.y).unwrap_or(0.0);

        // Determine the position of the box
        if graph_port_label_placement.contains(PortLabelPlacement::INSIDE) {
            if let Some(ext_port_side) = lg[dummy].props.get_typed::<PortSide>(&InternalProperties::EXT_PORT_SIDE) {
                match ext_port_side {
                    PortSide::NORTH => {
                        port_label_box.x = (dummy_size.x - port_label_box.width) / 2.0 - dummy_port_pos.x;
                        port_label_box.y = label_port_spacing_vertical;
                    }
                    PortSide::SOUTH => {
                        port_label_box.x = (dummy_size.x - port_label_box.width) / 2.0 - dummy_port_pos.x;
                        port_label_box.y = -label_port_spacing_vertical - port_label_box.height;
                    }
                    PortSide::EAST => {
                        if self.label_next_to_port(lg, dummy_port, true, place_next_to_port_if_possible) {
                            let label_height = if treat_as_group { port_label_box.height } else { first_label_height() };
                            port_label_box.y = (dummy_size.y - label_height) / 2.0 - dummy_port_pos.y;
                        } else {
                            port_label_box.y = dummy_size.y + label_port_spacing_vertical - dummy_port_pos.y;
                        }
                        port_label_box.x = -label_port_spacing_horizontal - port_label_box.width;
                    }
                    PortSide::WEST => {
                        if self.label_next_to_port(lg, dummy_port, true, place_next_to_port_if_possible) {
                            let label_height = if treat_as_group { port_label_box.height } else { first_label_height() };
                            port_label_box.y = (dummy_size.y - label_height) / 2.0 - dummy_port_pos.y;
                        } else {
                            port_label_box.y = dummy_size.y + label_port_spacing_vertical - dummy_port_pos.y;
                        }
                        port_label_box.x = label_port_spacing_horizontal;
                    }
                    _ => {}
                }
            }
        } else if graph_port_label_placement.contains(PortLabelPlacement::OUTSIDE) {
            if let Some(ext_port_side) = lg[dummy].props.get_typed::<PortSide>(&InternalProperties::EXT_PORT_SIDE) {
                match ext_port_side {
                    PortSide::NORTH | PortSide::SOUTH => {
                        port_label_box.x = dummy_port_pos.x + label_port_spacing_horizontal;
                    }
                    PortSide::EAST | PortSide::WEST => {
                        if self.label_next_to_port(lg, dummy_port, false, place_next_to_port_if_possible) {
                            let label_height = if treat_as_group { port_label_box.height } else { first_label_height() };
                            port_label_box.y = (dummy_size.y - label_height) / 2.0 - dummy_port_pos.y;
                        } else {
                            port_label_box.y = dummy_port_pos.y + label_port_spacing_vertical;
                        }
                    }
                    _ => {}
                }
            }
        }

        // Place the labels
        let mut current_y = port_label_box.y;
        for label in lg[dummy_port].labels.clone() {
            let label_pos = &mut lg[label].position;
            label_pos.x = port_label_box.x;
            label_pos.y = current_y;
            current_y += lg[label].size.y + label_label_spacing;
        }
    }

    /// Returns the amount of space required to place the labels later, or
    /// `None` if there are no labels.
    fn compute_port_label_box(&self, lg: &LGraphArena, dummy_port: LPortId, label_label_spacing: f64) -> Option<ElkRectangle> {
        let labels = &lg[dummy_port].labels;
        if labels.is_empty() {
            return None;
        }

        let mut result = ElkRectangle::default();
        for &label in labels {
            let label_size = lg[label].size;
            result.width = swift::max(result.width, label_size.x);
            result.height += label_size.y;
        }
        result.height += (labels.len() as i64 - 1) as f64 * label_label_spacing;
        Some(result)
    }

    /// Checks whether the labels of the given port should be placed next to
    /// the port or below it.
    fn label_next_to_port(&self, lg: &LGraphArena, dummy_port: LPortId, inside_labels: bool, place_next_to_port_if_possible: bool) -> bool {
        if !place_next_to_port_if_possible {
            return false;
        }
        if inside_labels {
            lg[dummy_port].incoming_edges.is_empty() && lg[dummy_port].outgoing_edges.is_empty()
        } else {
            !lg[dummy_port].connected_to_external_nodes
        }
    }
}

impl ILayoutProcessor for LabelAndNodeSizeProcessor {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Node and Port Label Placement and Node Sizing", 1.0);

        NodeDimensionCalculation::calculate_label_and_node_sizes(
            lg,
            &LGraphAdapters::adapt_filtered(layered_graph, true, true, is_normal_node),
        );

        // If the graph has external ports, treat labels of external port dummies differently
        let graph_props = lg[layered_graph].props.get_as::<EnumSet<GraphProperties>>(&InternalProperties::GRAPH_PROPERTIES);
        if let Some(graph_props) = graph_props {
            if graph_props.contains(GraphProperties::EXTERNAL_PORTS) {
                let port_label_placement: PortLabelPlacement = lg[layered_graph]
                    .props
                    .get_as::<PortLabelPlacement>(&LayeredOptions::PORT_LABELS_PLACEMENT)
                    .unwrap_or(PortLabelPlacement::empty());
                let place_next_to_port = port_label_placement.contains(PortLabelPlacement::NEXT_TO_PORT_IF_POSSIBLE);
                let treat_as_group: bool =
                    lg[layered_graph].props.get_as::<bool>(&CoreOptions::PORT_LABELS_TREAT_AS_GROUP).unwrap_or(true);

                for layer in lg[layered_graph].layers.clone() {
                    for node in lg[layer].nodes.clone() {
                        if lg[node].node_type == NodeType::EXTERNAL_PORT {
                            self.place_external_port_dummy_labels(lg, node, port_label_placement, place_next_to_port, treat_as_group);
                        }
                    }
                }
            }
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "LabelAndNodeSizeProcessor"
    }
}
