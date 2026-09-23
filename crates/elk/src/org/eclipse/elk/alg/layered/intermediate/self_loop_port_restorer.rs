//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/org_eclipse_elk_alg_layered_intermediate_SelfLoopPortRestorer.swift`.
//!
//! Assigns sides to hidden self loop ports and puts them back on their
//! nodes; determines every hyper loop's type.

use super::loops::ordering::port_restorer::PortRestorer;
use super::loops::ordering::port_side_assigner::PortSideAssigner;
use super::loops::self_loop_holder::SelfLoopHolder;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};
use crate::org::eclipse::elk::alg::layered::graph::l_node::NodeType;
use crate::org::eclipse::elk::alg::layered::options::internal_properties as InternalProperties;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::options::port_constraints::PortConstraints;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;

#[derive(Default)]
pub struct SelfLoopPortRestorer {
    port_side_assigner: PortSideAssigner,
    port_restorer: PortRestorer,
}

impl SelfLoopPortRestorer {
    pub fn new() -> SelfLoopPortRestorer {
        SelfLoopPortRestorer::default()
    }

    fn process_node(&mut self, lg: &mut LGraphArena, sl_holder: &mut SelfLoopHolder, monitor: &mut dyn IElkProgressMonitor) {
        if sl_holder.are_ports_hidden() {
            let original_pc = lg[sl_holder.get_l_node()]
                .props
                .get_as::<PortConstraints>(&InternalProperties::ORIGINAL_PORT_CONSTRAINTS)
                .unwrap_or(PortConstraints::UNDEFINED);
            match original_pc {
                PortConstraints::UNDEFINED | PortConstraints::FREE => {
                    // Assign port sides first, then fall through to restore
                    self.port_side_assigner.assign_port_sides(lg, sl_holder);
                    compute_self_loop_types(lg, sl_holder);
                    self.port_restorer.restore_ports(lg, sl_holder, monitor);
                }
                PortConstraints::FIXED_SIDE => {
                    compute_self_loop_types(lg, sl_holder);
                    self.port_restorer.restore_ports(lg, sl_holder, monitor);
                }
                _ => {}
            }
        } else {
            compute_self_loop_types(lg, sl_holder);
        }
    }
}

impl ILayoutProcessor for SelfLoopPortRestorer {
    fn process(&mut self, lg: &mut LGraphArena, graph: LGraphId, progress_monitor: &mut dyn IElkProgressMonitor) {
        progress_monitor.begin("Self-Loop ordering", 1.0);

        for layer in lg[graph].layers.clone() {
            for l_node in lg[layer].nodes.clone() {
                if lg[l_node].node_type == NodeType::NORMAL && lg[l_node].props.has(&InternalProperties::SELF_LOOP_HOLDER) {
                    if let Some(sl_holder) = SelfLoopHolder::of(lg, l_node) {
                        self.process_node(lg, &mut sl_holder.borrow_mut(), progress_monitor);
                    }
                }
            }
        }

        progress_monitor.done();
    }

    fn name(&self) -> &'static str {
        "SelfLoopPortRestorer"
    }
}

fn compute_self_loop_types(lg: &LGraphArena, sl_holder: &mut SelfLoopHolder) {
    for sl_loop in sl_holder.sl_loop_ids() {
        sl_holder.compute_ports_per_side(lg, sl_loop);
    }
}
