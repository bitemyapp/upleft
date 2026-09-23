//! Port of `alg/layered/intermediate/LongEdgeSplitter.swift`.
//!
//! Splits edges spanning more than one layer into chains of edges and
//! `LONG_EDGE` dummy nodes, one per layer crossed.

use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::options::edge_label_placement::EdgeLabelPlacement;
use crate::prelude::*;

#[derive(Default)]
pub struct LongEdgeSplitter;

impl LongEdgeSplitter {
    pub fn new() -> LongEdgeSplitter {
        LongEdgeSplitter
    }

    fn create_dummy_node(lg: &mut LGraphArena, layered_graph: LGraphId, target_layer: LayerId, edge_to_split: LEdgeId) -> LNodeId {
        let dummy_node = lg.new_node(Some(layered_graph));
        lg[dummy_node].node_type = NodeType::LONG_EDGE;
        lg[dummy_node].props.set(&InternalProperties::ORIGIN, edge_to_split);
        lg[dummy_node].props.set(&LayeredOptions::PORT_CONSTRAINTS, PortConstraints::FIXED_POS);
        lg.node_set_layer(dummy_node, Some(target_layer));
        dummy_node
    }

    /// `splitEdge(_:_:)`: routes `edge` into a new west port of `dummy_node`
    /// and continues it with a new edge (a property copy of `edge`, without
    /// junction points) from a new east port to the old target. Returns the
    /// new edge.
    pub fn split_edge(lg: &mut LGraphArena, edge: LEdgeId, dummy_node: LNodeId) -> LEdgeId {
        let Some(old_edge_target) = lg[edge].target else { return lg.new_edge() };

        // Set thickness of the edge
        let mut thickness = lg[edge].props.get_as::<f64>(&LayeredOptions::EDGE_THICKNESS).unwrap_or(0.0);
        if thickness < 0.0 {
            thickness = 0.0;
            lg[edge].props.set(&LayeredOptions::EDGE_THICKNESS, thickness);
        }
        lg[dummy_node].size.y = thickness;
        let port_pos = (thickness / 2.0).floor();

        // Create dummy input and output ports
        let dummy_input = lg.new_port();
        lg[dummy_input].side = PortSide::WEST;
        lg.port_set_node(dummy_input, Some(dummy_node));
        lg[dummy_input].position.y = port_pos;

        let dummy_output = lg.new_port();
        lg[dummy_output].side = PortSide::EAST;
        lg.port_set_node(dummy_output, Some(dummy_node));
        lg[dummy_output].position.y = port_pos;

        lg.edge_set_target(edge, Some(dummy_input));

        // Create a dummy edge
        let dummy_edge = lg.new_edge();
        let props = lg[edge].props.clone();
        lg[dummy_edge].props.copy_properties(&props);
        lg[dummy_edge].props.remove(&LayeredOptions::JUNCTION_POINTS);
        lg.edge_set_source(dummy_edge, Some(dummy_output));
        lg.edge_set_target(dummy_edge, Some(old_edge_target));

        Self::set_dummy_node_properties(lg, dummy_node, edge, dummy_edge);
        Self::move_head_labels(lg, edge, dummy_edge);

        dummy_edge
    }

    fn set_dummy_node_properties(lg: &mut LGraphArena, dummy_node: LNodeId, in_edge: LEdgeId, out_edge: LEdgeId) {
        let (Some(in_edge_source), Some(out_edge_target)) = (lg[in_edge].source, lg[out_edge].target) else { return };
        let (Some(in_edge_source_node), Some(out_edge_target_node)) = (lg[in_edge_source].owner, lg[out_edge_target].owner) else { return };

        // `setProperty(P, value: other.getProperty(P))` with an `Any?`: a nil value removes.
        let copy_from = |lg: &mut LGraphArena, from: LNodeId, p: &Property| {
            let v = lg[from].props.get(p);
            lg[dummy_node].props.set_opt(p, v);
        };

        if lg[in_edge_source_node].node_type == NodeType::LONG_EDGE {
            copy_from(lg, in_edge_source_node, &InternalProperties::LONG_EDGE_SOURCE);
            copy_from(lg, in_edge_source_node, &InternalProperties::LONG_EDGE_TARGET);
            copy_from(lg, in_edge_source_node, &InternalProperties::LONG_EDGE_HAS_LABEL_DUMMIES);
        } else if lg[in_edge_source_node].node_type == NodeType::LABEL {
            copy_from(lg, in_edge_source_node, &InternalProperties::LONG_EDGE_SOURCE);
            copy_from(lg, in_edge_source_node, &InternalProperties::LONG_EDGE_TARGET);
            lg[dummy_node].props.set(&InternalProperties::LONG_EDGE_HAS_LABEL_DUMMIES, true);
        } else if lg[out_edge_target_node].node_type == NodeType::LABEL {
            copy_from(lg, out_edge_target_node, &InternalProperties::LONG_EDGE_SOURCE);
            copy_from(lg, out_edge_target_node, &InternalProperties::LONG_EDGE_TARGET);
            lg[dummy_node].props.set(&InternalProperties::LONG_EDGE_HAS_LABEL_DUMMIES, true);
        } else {
            lg[dummy_node].props.set(&InternalProperties::LONG_EDGE_SOURCE, in_edge_source);
            lg[dummy_node].props.set(&InternalProperties::LONG_EDGE_TARGET, out_edge_target);
        }
    }

    fn move_head_labels(lg: &mut LGraphArena, old_edge: LEdgeId, new_edge: LEdgeId) {
        let mut i = 0;
        while i < lg[old_edge].labels.len() {
            let label = lg[old_edge].labels[i];
            let label_placement = lg[label].props.get_as::<EdgeLabelPlacement>(&LayeredOptions::EDGE_LABELS_PLACEMENT);
            if label_placement == Some(EdgeLabelPlacement::HEAD) {
                lg[old_edge].labels.remove(i);
                lg[new_edge].labels.push(label);
                if !lg[label].props.has(&InternalProperties::END_LABEL_EDGE) {
                    lg[label].props.set(&InternalProperties::END_LABEL_EDGE, old_edge);
                }
            } else {
                i += 1;
            }
        }
    }
}

impl ILayoutProcessor for LongEdgeSplitter {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Edge splitting", 1.0);

        if lg[layered_graph].layers.len() <= 2 {
            monitor.done();
            return;
        }

        let mut layer_index = 0;
        while layer_index < lg[layered_graph].layers.len() - 1 {
            let layer = lg[layered_graph].layers[layer_index];
            let next_layer = lg[layered_graph].layers[layer_index + 1];

            for node in lg[layer].nodes.clone() {
                for port in lg[node].ports.clone() {
                    for edge in lg[port].outgoing_edges.clone() {
                        let Some(target_port) = lg[edge].target else { continue };
                        let target_layer = lg[target_port].owner.and_then(|n| lg[n].layer);

                        if target_layer != Some(layer) && target_layer != Some(next_layer) {
                            let dummy_node = Self::create_dummy_node(lg, layered_graph, next_layer, edge);
                            Self::split_edge(lg, edge, dummy_node);
                        }
                    }
                }
            }

            layer_index += 1;
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "LongEdgeSplitter"
    }
}
