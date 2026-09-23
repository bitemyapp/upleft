//! Port of `alg/layered/intermediate/LayerConstraintPostprocessor.swift`.
//!
//! Moves `FIRST`/`LAST` nodes into the first/last layer (their label dummies
//! into extra layers beyond), drops layers left empty, and puts the nodes the
//! `LayerConstraintPreprocessor` hid into separate first/last layers with
//! their edges reattached.

use crate::org::eclipse::elk::alg::layered::options::layer_constraint::LayerConstraint;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::prelude::*;

#[derive(Default)]
pub struct LayerConstraintPostprocessor;

fn layer_constraint_of(lg: &LGraphArena, node: LNodeId) -> LayerConstraint {
    lg[node].props.get_as::<LayerConstraint>(&LayeredOptions::LAYERING_LAYER_CONSTRAINT).unwrap_or(LayerConstraint::NONE)
}

impl LayerConstraintPostprocessor {
    pub fn new() -> LayerConstraintPostprocessor {
        LayerConstraintPostprocessor
    }

    fn move_first_and_last_nodes(
        lg: &mut LGraphArena,
        layered_graph: LGraphId,
        first_layer: LayerId,
        last_layer: LayerId,
        first_label_layer: LayerId,
        last_label_layer: LayerId,
    ) {
        for layer in lg[layered_graph].layers.clone() {
            let nodes = lg[layer].nodes.clone();
            for node in nodes {
                match layer_constraint_of(lg, node) {
                    LayerConstraint::FIRST => {
                        Self::throw_up_unless_no_incoming_edges(lg, node);
                        lg.node_set_layer(node, Some(first_layer));
                        Self::move_labels_to_label_layer(lg, node, true, first_label_layer);
                    }
                    LayerConstraint::LAST => {
                        Self::throw_up_unless_no_outgoing_edges(lg, node);
                        lg.node_set_layer(node, Some(last_layer));
                        Self::move_labels_to_label_layer(lg, node, false, last_label_layer);
                    }
                    _ => {}
                }
            }
        }

        // Remove empty layers
        let layers = std::mem::take(&mut lg[layered_graph].layers);
        let kept: Vec<LayerId> = layers.into_iter().filter(|&l| !lg[l].nodes.is_empty()).collect();
        lg[layered_graph].layers = kept;
    }

    fn move_labels_to_label_layer(lg: &mut LGraphArena, node: LNodeId, incoming: bool, label_layer: LayerId) {
        let edges = if incoming { lg.node_incoming_edges(node) } else { lg.node_outgoing_edges(node) };
        for edge in edges {
            let possible = if incoming { lg.edge_source_node(edge) } else { lg.edge_target_node(edge) };
            let Some(possible_label_dummy) = possible else { continue };
            if lg[possible_label_dummy].node_type == NodeType::LABEL {
                lg.node_set_layer(possible_label_dummy, Some(label_layer));
            }
        }
    }

    fn restore_hidden_nodes(lg: &mut LGraphArena, layered_graph: LGraphId, first_separate_layer: LayerId, last_separate_layer: LayerId) {
        let Some(hidden_nodes) = lg[layered_graph].props.get_as::<Vec<LNodeId>>(&InternalProperties::HIDDEN_NODES) else { return };

        for hidden_node in hidden_nodes {
            match layer_constraint_of(lg, hidden_node) {
                LayerConstraint::FIRST_SEPARATE => lg.node_set_layer(hidden_node, Some(first_separate_layer)),
                LayerConstraint::LAST_SEPARATE => lg.node_set_layer(hidden_node, Some(last_separate_layer)),
                _ => {}
            }

            for hidden_edge in lg.node_connected_edges(hidden_node) {
                if lg[hidden_edge].source.is_some() && lg[hidden_edge].target.is_some() {
                    continue;
                }
                let is_outgoing = lg[hidden_edge].target.is_none();
                let original_opposite_port = lg[hidden_edge].props.get_as::<LPortId>(&InternalProperties::ORIGINAL_OPPOSITE_PORT);
                if is_outgoing {
                    lg.edge_set_target(hidden_edge, original_opposite_port);
                } else {
                    lg.edge_set_source(hidden_edge, original_opposite_port);
                }
            }
        }
    }

    /// Java throws here; elk-swift only returns.
    fn throw_up_unless_no_incoming_edges(_lg: &LGraphArena, _node: LNodeId) {}

    /// Java throws here; elk-swift only returns.
    fn throw_up_unless_no_outgoing_edges(_lg: &LGraphArena, _node: LNodeId) {}
}

impl ILayoutProcessor for LayerConstraintPostprocessor {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Layer constraint postprocessing", 1.0);

        let layers = lg[layered_graph].layers.clone();

        if !layers.is_empty() {
            let first_layer = layers[0];
            let last_layer = layers[layers.len() - 1];

            let first_label_layer = lg.new_layer(layered_graph);
            let last_label_layer = lg.new_layer(layered_graph);

            Self::move_first_and_last_nodes(lg, layered_graph, first_layer, last_layer, first_label_layer, last_label_layer);

            if !lg[first_label_layer].nodes.is_empty() {
                lg[layered_graph].layers.insert(0, first_label_layer);
            }
            if !lg[last_label_layer].nodes.is_empty() {
                lg[layered_graph].layers.push(last_label_layer);
            }
        }

        if lg[layered_graph].props.has(&InternalProperties::HIDDEN_NODES) {
            let first_separate_layer = lg.new_layer(layered_graph);
            let last_separate_layer = lg.new_layer(layered_graph);

            Self::restore_hidden_nodes(lg, layered_graph, first_separate_layer, last_separate_layer);

            if !lg[first_separate_layer].nodes.is_empty() {
                lg[layered_graph].layers.insert(0, first_separate_layer);
            }
            if !lg[last_separate_layer].nodes.is_empty() {
                lg[layered_graph].layers.push(last_separate_layer);
            }
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "LayerConstraintPostprocessor"
    }
}
