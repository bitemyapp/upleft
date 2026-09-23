//! Port of `alg/layered/intermediate/InLayerConstraintProcessor.swift`.
//!
//! Moves `TOP`-constrained nodes to the top of their layer (after any leading
//! `TOP` nodes) and `BOTTOM`-constrained nodes to the bottom.

use crate::org::eclipse::elk::alg::layered::options::in_layer_constraint::InLayerConstraint;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::prelude::*;

#[derive(Default)]
pub struct InLayerConstraintProcessor;

impl InLayerConstraintProcessor {
    pub fn new() -> InLayerConstraintProcessor {
        InLayerConstraintProcessor
    }
}

impl ILayoutProcessor for InLayerConstraintProcessor {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Layer constraint edge reversal", 1.0);

        for layer in lg[layered_graph].layers.clone() {
            let mut top_insertion_index: i64 = -1;
            let mut bottom_constrained_nodes: Vec<LNodeId> = Vec::new();

            let nodes = lg[layer].nodes.clone();

            for (i, &node) in nodes.iter().enumerate() {
                let constraint = lg[node].props.get_as::<InLayerConstraint>(&InternalProperties::IN_LAYER_CONSTRAINT).unwrap_or(InLayerConstraint::NONE);

                if top_insertion_index == -1 {
                    if constraint != InLayerConstraint::TOP {
                        top_insertion_index = i as i64;
                    }
                } else if constraint == InLayerConstraint::TOP {
                    // Move the node to the top insertion point
                    lg.node_set_layer(node, None);
                    lg[layer].nodes.insert(top_insertion_index as usize, node);
                    lg[node].layer = Some(layer);
                    top_insertion_index += 1;
                }

                if constraint == InLayerConstraint::BOTTOM {
                    bottom_constrained_nodes.push(node);
                }
            }

            // Append the bottom-constrained nodes
            for node in bottom_constrained_nodes {
                lg.node_set_layer(node, None);
                lg.node_set_layer(node, Some(layer));
            }
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "InLayerConstraintProcessor"
    }
}
