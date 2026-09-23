//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/org_eclipse_elk_alg_layered_intermediate_SelfLoopPostProcessor.swift`.
//!
//! Reconnects the hidden self loops, moves their bend points into the
//! node's coordinate system, and places their labels.

use super::loops::self_loop_edge::SlEdgeId;
use super::loops::self_loop_holder::SelfLoopHolder;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId, LNodeId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;

#[derive(Default)]
pub struct SelfLoopPostProcessor;

impl SelfLoopPostProcessor {
    pub fn new() -> SelfLoopPostProcessor {
        SelfLoopPostProcessor
    }
}

impl ILayoutProcessor for SelfLoopPostProcessor {
    fn process(&mut self, lg: &mut LGraphArena, graph: LGraphId, progress_monitor: &mut dyn IElkProgressMonitor) {
        progress_monitor.begin("Self-Loop post-processing", 1.0);

        for layer in lg[graph].layers.clone() {
            for l_node in lg[layer].nodes.clone() {
                if lg[l_node].node_type == NodeType::NORMAL && lg[l_node].props.has(&InternalProperties::SELF_LOOP_HOLDER) {
                    process_node(lg, l_node);
                }
            }
        }

        progress_monitor.done();
    }

    fn name(&self) -> &'static str {
        "SelfLoopPostProcessor"
    }
}

fn process_node(lg: &mut LGraphArena, l_node: LNodeId) {
    let Some(sl_holder) = SelfLoopHolder::of(lg, l_node) else { return };
    let sl_holder = sl_holder.borrow();

    for sl_loop in sl_holder.get_sl_hyper_loops() {
        // NONDETERMINISTIC IN SWIFT: `getSLEdges()` is a `Set`; the order
        // decides where each edge lands in its ports' edge lists (see
        // `SelfHyperLoop.sl_edges`: insertion order).
        for &sl_edge in sl_loop.get_sl_edges() {
            restore_edge(lg, l_node, &sl_holder, sl_edge);
        }
    }

    for sl_loop in sl_holder.get_sl_hyper_loops() {
        if let Some(sl_labels) = sl_loop.get_sl_labels() {
            let offset = lg[l_node].position;
            sl_labels.apply_placement(lg, offset);
        }
    }
}

fn restore_edge(lg: &mut LGraphArena, l_node: LNodeId, sl_holder: &SelfLoopHolder, sl_edge: SlEdgeId) {
    let e = sl_holder.sl_edge(sl_edge);
    let l_edge = e.get_l_edge();
    lg.edge_set_source(l_edge, Some(sl_holder.sl_port(e.get_sl_source()).get_l_port()));
    lg.edge_set_target(l_edge, Some(sl_holder.sl_port(e.get_sl_target()).get_l_port()));

    let position = lg[l_node].position;
    lg[l_edge].bend_points.offset(position);
}
