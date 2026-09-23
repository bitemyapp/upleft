//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/org_eclipse_elk_alg_layered_intermediate_SelfLoopRouter.swift`.
//!
//! Routes every node's self loops: loop routes, label placement, routing
//! slots, then bend points.

use super::loops::routing::abstract_self_loop_router::AbstractSelfLoopRouter;
use super::loops::routing::label_placer::LabelPlacer;
use super::loops::routing::orthogonal_self_loop_router::OrthogonalSelfLoopRouter;
use super::loops::routing::polyline_self_loop_router::PolylineSelfLoopRouter;
use super::loops::routing::routing_director::RoutingDirector;
use super::loops::routing::routing_slot_assigner::RoutingSlotAssigner;
use super::loops::self_loop_holder::SelfLoopHolder;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::options::edge_routing::EdgeRouting;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;

#[derive(Default)]
pub struct SelfLoopRouter {
    routing_director: RoutingDirector,
    label_placer: LabelPlacer,
    routing_slot_assigner: RoutingSlotAssigner,
}

impl SelfLoopRouter {
    pub fn new() -> SelfLoopRouter {
        SelfLoopRouter::default()
    }

    fn process_node(&mut self, lg: &mut LGraphArena, sl_holder: &mut SelfLoopHolder, sl_router: &dyn AbstractSelfLoopRouter, monitor: &mut dyn IElkProgressMonitor) {
        self.routing_director.determine_loop_routes(lg, sl_holder);
        self.label_placer.place_labels(lg, sl_holder, monitor);
        self.routing_slot_assigner.assign_routing_slots(lg, sl_holder);
        sl_router.route_self_loops(lg, sl_holder);
    }
}

fn router_for_graph(lg: &LGraphArena, graph: LGraphId) -> Box<dyn AbstractSelfLoopRouter> {
    match lg[graph].props.get_as::<EdgeRouting>(&LayeredOptions::EDGE_ROUTING) {
        Some(EdgeRouting::POLYLINE) => Box::new(PolylineSelfLoopRouter::new()),
        // Spline routing not supported (mermaid hard-codes ORTHOGONAL); fall through to orthogonal
        Some(EdgeRouting::SPLINES) => Box::new(OrthogonalSelfLoopRouter::new()),
        _ => Box::new(OrthogonalSelfLoopRouter::new()),
    }
}

impl ILayoutProcessor for SelfLoopRouter {
    fn process(&mut self, lg: &mut LGraphArena, graph: LGraphId, progress_monitor: &mut dyn IElkProgressMonitor) {
        progress_monitor.begin("Self-Loop routing", 1.0);

        let router = router_for_graph(lg, graph);
        // `graph.getProperty(LabelManagementOptions.LABEL_MANAGER) as? ILabelManager`
        // is always nil (see `LabelPlacer::place_labels`).

        for layer in lg[graph].layers.clone() {
            for l_node in lg[layer].nodes.clone() {
                if lg[l_node].node_type == NodeType::NORMAL && lg[l_node].props.has(&InternalProperties::SELF_LOOP_HOLDER) {
                    if let Some(sl_holder) = SelfLoopHolder::of(lg, l_node) {
                        self.process_node(lg, &mut sl_holder.borrow_mut(), router.as_ref(), progress_monitor);
                    }
                }
            }
        }

        progress_monitor.done();
    }

    fn name(&self) -> &'static str {
        "SelfLoopRouter"
    }
}
