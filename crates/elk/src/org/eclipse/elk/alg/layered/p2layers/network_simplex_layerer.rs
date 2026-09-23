//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p2layers/org_eclipse_elk_alg_layered_p2layers_NetworkSimplexLayerer.swift`.
//!
//! Not ported yet.

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};
use crate::org::eclipse::elk::core::alg::i_layout_phase::ILayoutPhase;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;

#[derive(Default)]
pub struct NetworkSimplexLayerer;

impl NetworkSimplexLayerer {
    pub fn new() -> NetworkSimplexLayerer {
        NetworkSimplexLayerer
    }
}

impl ILayoutProcessor for NetworkSimplexLayerer {
    fn process(&mut self, _lg: &mut LGraphArena, _graph: LGraphId, _monitor: &mut dyn IElkProgressMonitor) {
        unimplemented!("NetworkSimplexLayerer is not ported yet")
    }

    fn name(&self) -> &'static str {
        "NetworkSimplexLayerer"
    }
}

impl ILayoutPhase for NetworkSimplexLayerer {}
