//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p1cycles/org_eclipse_elk_alg_layered_p1cycles_GreedyCycleBreaker.swift`.
//!
//! Not ported yet.

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};
use crate::org::eclipse::elk::core::alg::i_layout_phase::ILayoutPhase;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;

#[derive(Default)]
pub struct GreedyCycleBreaker;

impl GreedyCycleBreaker {
    pub fn new() -> GreedyCycleBreaker {
        GreedyCycleBreaker
    }
}

impl ILayoutProcessor for GreedyCycleBreaker {
    fn process(&mut self, _lg: &mut LGraphArena, _graph: LGraphId, _monitor: &mut dyn IElkProgressMonitor) {
        unimplemented!("GreedyCycleBreaker is not ported yet")
    }

    fn name(&self) -> &'static str {
        "GreedyCycleBreaker"
    }
}

impl ILayoutPhase for GreedyCycleBreaker {}
