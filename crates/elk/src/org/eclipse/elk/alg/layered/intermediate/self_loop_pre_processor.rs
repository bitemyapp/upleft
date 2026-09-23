//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/org_eclipse_elk_alg_layered_intermediate_SelfLoopPreProcessor.swift`.
//!
//! Installs a `SelfLoopHolder` on every node with self loops, then hides the
//! self loops (disconnects them) and, where allowed, the ports that only
//! carry self loops.

use super::loops::self_loop_holder::SelfLoopHolder;
use crate::bridge::java_compat::EnumSet;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};
use crate::org::eclipse::elk::alg::layered::options::graph_properties::GraphProperties;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::options::port_constraints::PortConstraints;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;

#[derive(Default)]
pub struct SelfLoopPreProcessor;

impl SelfLoopPreProcessor {
    pub fn new() -> SelfLoopPreProcessor {
        SelfLoopPreProcessor
    }
}

impl ILayoutProcessor for SelfLoopPreProcessor {
    fn process(&mut self, lg: &mut LGraphArena, graph: LGraphId, progress_monitor: &mut dyn IElkProgressMonitor) {
        progress_monitor.begin("Self-Loop pre-processing", 1.0);

        for lnode in lg[graph].layerless_nodes.clone() {
            if SelfLoopHolder::needs_self_loop_processing(lg, lnode) {
                let sl_holder = SelfLoopHolder::install(lg, lnode);
                let mut sl_holder = sl_holder.borrow_mut();
                hide_self_loops(lg, &sl_holder);
                hide_ports(lg, &mut sl_holder);
            }
        }

        progress_monitor.done();
    }

    fn name(&self) -> &'static str {
        "SelfLoopPreProcessor"
    }
}

fn hide_self_loops(lg: &mut LGraphArena, sl_holder: &SelfLoopHolder) {
    for sl_loop in sl_holder.get_sl_hyper_loops() {
        // `getSLEdges()` is a Swift `Set`; removing edges from their ports is
        // order-independent.
        for &sl_edge in sl_loop.get_sl_edges() {
            let l_edge = sl_holder.sl_edge(sl_edge).get_l_edge();
            lg.edge_set_source(l_edge, None);
            lg.edge_set_target(l_edge, None);
        }
    }
}

fn hide_ports(lg: &mut LGraphArena, sl_holder: &mut SelfLoopHolder) {
    let l_node = sl_holder.get_l_node();
    let nested_graph = lg[l_node].nested_graph;

    let order_fixed = lg[l_node].props.get_as::<PortConstraints>(&LayeredOptions::PORT_CONSTRAINTS).is_some_and(|pc| pc.is_order_fixed());
    let hierarchy_mode = nested_graph.is_some_and(|g| {
        lg[g]
            .props
            .get_as::<EnumSet<GraphProperties>>(&InternalProperties::GRAPH_PROPERTIES)
            .is_some_and(|p| p.contains(GraphProperties::EXTERNAL_PORTS))
    });

    if order_fixed || hierarchy_mode {
        return;
    }

    for sl_port in sl_holder.get_sl_port_values() {
        if sl_holder.sl_port(sl_port).had_only_self_loops() {
            let l_port = sl_holder.sl_port(sl_port).get_l_port();
            lg.port_set_node(l_port, None);

            sl_holder.sl_port_mut(sl_port).set_hidden(true);
            sl_holder.set_ports_hidden(true);
        }
    }
}
