//! Port of `alg/layered/intermediate/InvertedPortProcessor.swift`.
//!
//! Inserts long-edge dummies for inverted ports (input ports on the east
//! side, output ports on the west side of nodes with fixed port sides), so
//! that the edges can be routed around the node. Runs before phase 3.

use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::options::edge_label_placement::EdgeLabelPlacement;
use crate::prelude::*;

#[derive(Default)]
pub struct InvertedPortProcessor;

impl InvertedPortProcessor {
    pub fn new() -> InvertedPortProcessor {
        InvertedPortProcessor
    }

    /// Creates the dummy node and its two ports for an inverted port; returns
    /// `(dummy, dummy_input, dummy_output)`.
    fn create_dummy(lg: &mut LGraphArena, layered_graph: LGraphId, edge: LEdgeId, layer_node_list: &mut Vec<LNodeId>) -> (LNodeId, LPortId, LPortId) {
        let dummy = lg.new_node(Some(layered_graph));
        lg[dummy].node_type = NodeType::LONG_EDGE;
        lg[dummy].props.set(&InternalProperties::ORIGIN, PropValue::LEdge(edge));
        lg[dummy].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_POS);
        layer_node_list.push(dummy);

        let dummy_input = lg.new_port();
        lg.port_set_node(dummy_input, Some(dummy));
        lg.port_set_side(dummy_input, PortSide::WEST);

        let dummy_output = lg.new_port();
        lg.port_set_node(dummy_output, Some(dummy));
        lg.port_set_side(dummy_output, PortSide::EAST);

        (dummy, dummy_input, dummy_output)
    }

    /// `createEastPortSideDummies(_:_:_:_:)`.
    fn create_east_port_side_dummies(lg: &mut LGraphArena, layered_graph: LGraphId, eastward_port: LPortId, edge: LEdgeId, layer_node_list: &mut Vec<LNodeId>) {
        // Ignore self loops
        if lg.edge_source_node(edge) == lg[eastward_port].owner {
            return;
        }

        // Dummy node in the same layer
        let (dummy, dummy_input, dummy_output) = Self::create_dummy(lg, layered_graph, edge, layer_node_list);

        // Reroute the original edge
        lg.edge_set_target(edge, Some(dummy_input));

        // Connect the dummy with the original port
        let dummy_edge = lg.new_edge();
        let edge_props = lg[edge].props.clone();
        lg[dummy_edge].props.copy_properties(&edge_props);
        lg[dummy_edge].props.remove(&LayeredOptions::JUNCTION_POINTS);
        lg.edge_set_source(dummy_edge, Some(dummy_output));
        lg.edge_set_target(dummy_edge, Some(eastward_port));

        // Set LONG_EDGE_SOURCE and LONG_EDGE_TARGET
        Self::set_long_edge_source_and_target(lg, dummy, dummy_input, dummy_output, eastward_port);

        // Move head labels from the old edge over to the new one
        let mut i = 0;
        while i < lg[edge].labels.len() {
            let label = lg[edge].labels[i];
            let label_placement = lg[label].props.get_as::<EdgeLabelPlacement>(&LayeredOptions::EDGE_LABELS_PLACEMENT);

            if label_placement == Some(EdgeLabelPlacement::HEAD) {
                if !lg[label].props.has(&InternalProperties::END_LABEL_EDGE) {
                    lg[label].props.set(&InternalProperties::END_LABEL_EDGE, PropValue::LEdge(edge));
                }
                lg[edge].labels.remove(i);
                lg[dummy_edge].labels.push(label);
            } else {
                i += 1;
            }
        }
    }

    /// `createWestPortSideDummies(_:_:_:_:)`.
    fn create_west_port_side_dummies(lg: &mut LGraphArena, layered_graph: LGraphId, westward_port: LPortId, edge: LEdgeId, layer_node_list: &mut Vec<LNodeId>) {
        // Ignore self loops
        if lg.edge_target_node(edge) == lg[westward_port].owner {
            return;
        }

        // Dummy node in the same layer
        let (dummy, dummy_input, dummy_output) = Self::create_dummy(lg, layered_graph, edge, layer_node_list);

        // Reroute the original edge
        let original_target = lg[edge].target;
        lg.edge_set_target(edge, Some(dummy_input));

        // Connect the dummy with the original port
        let dummy_edge = lg.new_edge();
        let edge_props = lg[edge].props.clone();
        lg[dummy_edge].props.copy_properties(&edge_props);
        lg[dummy_edge].props.remove(&LayeredOptions::JUNCTION_POINTS);
        lg.edge_set_source(dummy_edge, Some(dummy_output));
        lg.edge_set_target(dummy_edge, original_target);

        // Move head labels over to the new dummy edge
        let mut i = 0;
        while i < lg[edge].labels.len() {
            let label = lg[edge].labels[i];

            if lg[label].props.get_as::<EdgeLabelPlacement>(&LayeredOptions::EDGE_LABELS_PLACEMENT) == Some(EdgeLabelPlacement::HEAD) {
                lg[label].props.set(&InternalProperties::END_LABEL_EDGE, PropValue::LEdge(edge));
                lg[edge].labels.remove(i);
                lg[dummy_edge].labels.push(label);
            } else {
                i += 1;
            }
        }

        // Set LONG_EDGE_SOURCE and LONG_EDGE_TARGET
        Self::set_long_edge_source_and_target(lg, dummy, dummy_input, dummy_output, westward_port);
    }

    /// `setLongEdgeSourceAndTarget(_:_:_:_:)`.
    fn set_long_edge_source_and_target(lg: &mut LGraphArena, long_edge_dummy: LNodeId, dummy_input_port: LPortId, dummy_output_port: LPortId, _odd_port: LPortId) {
        let in_edge = lg[dummy_input_port].incoming_edges[0];
        let Some(source_port) = lg[in_edge].source else { return };
        let Some(source_node) = lg[source_port].owner else { return };
        let source_node_type = lg[source_node].node_type;

        let out_edge = lg[dummy_output_port].outgoing_edges[0];
        let Some(target_port) = lg[out_edge].target else { return };
        let Some(target_node) = lg[target_port].owner else { return };
        let target_node_type = lg[target_node].node_type;

        // Set the LONG_EDGE_SOURCE property
        if source_node_type == NodeType::LONG_EDGE {
            let v = lg[source_node].props.get(&InternalProperties::LONG_EDGE_SOURCE);
            lg[long_edge_dummy].props.set_opt(&InternalProperties::LONG_EDGE_SOURCE, v);
        } else {
            lg[long_edge_dummy].props.set(&InternalProperties::LONG_EDGE_SOURCE, PropValue::LPort(source_port));
        }

        // Set the LONG_EDGE_TARGET property
        if target_node_type == NodeType::LONG_EDGE {
            let v = lg[target_node].props.get(&InternalProperties::LONG_EDGE_TARGET);
            lg[long_edge_dummy].props.set_opt(&InternalProperties::LONG_EDGE_TARGET, v);
        } else {
            lg[long_edge_dummy].props.set(&InternalProperties::LONG_EDGE_TARGET, PropValue::LPort(target_port));
        }
    }
}

impl ILayoutProcessor for InvertedPortProcessor {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Inverted port preprocessing", 1.0);

        let layers = lg[layered_graph].layers.clone();

        let mut current_layer: Option<LayerId> = None;
        let mut unassigned_nodes: Vec<LNodeId> = Vec::new();
        let mut layer_index = 0;

        while layer_index < layers.len() {
            let previous_layer = current_layer;
            current_layer = Some(layers[layer_index]);
            layer_index += 1;

            // If the previous layer had unassigned nodes, assign them now
            if let Some(prev) = previous_layer {
                for &node in &unassigned_nodes {
                    lg.node_set_layer(node, Some(prev));
                }
            }
            unassigned_nodes.clear();

            // Iterate through the layer's nodes
            let Some(layer) = current_layer else { continue };
            for node in lg[layer].nodes.clone() {
                // Skip dummy nodes
                if lg[node].node_type != NodeType::NORMAL {
                    continue;
                }

                // Skip nodes whose port sides are not fixed
                let port_constraints = lg[node].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).unwrap_or(PortConstraints::UNDEFINED);
                if !port_constraints.is_side_fixed() {
                    continue;
                }

                // Look for input ports on the right side
                for port in lg.node_ports_of_type_on_side(node, PortType::INPUT, PortSide::EAST) {
                    for edge in lg[port].incoming_edges.clone() {
                        Self::create_east_port_side_dummies(lg, layered_graph, port, edge, &mut unassigned_nodes);
                    }
                }

                // Look for output ports on the left side
                for port in lg.node_ports_of_type_on_side(node, PortType::OUTPUT, PortSide::WEST) {
                    for edge in lg[port].outgoing_edges.clone() {
                        Self::create_west_port_side_dummies(lg, layered_graph, port, edge, &mut unassigned_nodes);
                    }
                }
            }
        }

        // There may be unassigned nodes left
        if let Some(last) = current_layer {
            for &node in &unassigned_nodes {
                lg.node_set_layer(node, Some(last));
            }
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "InvertedPortProcessor"
    }
}
