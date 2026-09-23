//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/org_eclipse_elk_alg_layered_p3order_LayerSweepCrossingMinimizer.swift`.
//!
//! Not ported yet.

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};
use crate::org::eclipse::elk::core::alg::i_layout_phase::ILayoutPhase;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;

/// `CrossMinType` (the phase-level one declared in this file).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrossMinType {
    BARYCENTER,
    ONE_SIDED_GREEDY_SWITCH,
    TWO_SIDED_GREEDY_SWITCH,
    MEDIAN,
}

pub struct LayerSweepCrossingMinimizer {
    pub cross_min_type: CrossMinType,
}

impl LayerSweepCrossingMinimizer {
    pub fn new(cross_min_type: CrossMinType) -> LayerSweepCrossingMinimizer {
        LayerSweepCrossingMinimizer { cross_min_type }
    }
}

impl ILayoutProcessor for LayerSweepCrossingMinimizer {
    fn process(&mut self, _lg: &mut LGraphArena, _graph: LGraphId, _monitor: &mut dyn IElkProgressMonitor) {
        unimplemented!("LayerSweepCrossingMinimizer is not ported yet")
    }

    fn is_hierarchy_aware(&self) -> bool {
        true
    }

    fn name(&self) -> &'static str {
        "LayerSweepCrossingMinimizer"
    }
}

impl ILayoutPhase for LayerSweepCrossingMinimizer {}
