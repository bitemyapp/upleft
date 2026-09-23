//! Port of `alg/layered/intermediate/ReversedEdgeRestorer.swift`.
//!
//! Turns every edge marked `REVERSED` back to its original direction.

use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::prelude::*;

#[derive(Default)]
pub struct ReversedEdgeRestorer;

impl ReversedEdgeRestorer {
    pub fn new() -> ReversedEdgeRestorer {
        ReversedEdgeRestorer
    }
}

impl ILayoutProcessor for ReversedEdgeRestorer {
    fn process(&mut self, lg: &mut LGraphArena, layered_graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        monitor.begin("Restoring reversed edges", 1.0);

        for li in 0..lg[layered_graph].layers.len() {
            let layer = lg[layered_graph].layers[li];
            for ni in 0..lg[layer].nodes.len() {
                let node = lg[layer].nodes[ni];
                for pi in 0..lg[node].ports.len() {
                    let port = lg[node].ports[pi];
                    let edge_array = lg[port].outgoing_edges.clone();
                    for edge in edge_array {
                        if lg[edge].props.get_as::<bool>(&InternalProperties::REVERSED).unwrap_or(false) {
                            lg.edge_reverse(edge, layered_graph, false);
                        }
                    }
                }
            }
        }

        monitor.done();
    }

    fn name(&self) -> &'static str {
        "ReversedEdgeRestorer"
    }
}
