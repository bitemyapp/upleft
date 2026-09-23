//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/org_eclipse_elk_alg_layered_intermediate_HierarchicalPortPositionProcessor.swift`.
//!
//! Not ported yet.

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;

#[derive(Default)]
pub struct HierarchicalPortPositionProcessor;

impl HierarchicalPortPositionProcessor {
    pub fn new() -> HierarchicalPortPositionProcessor {
        HierarchicalPortPositionProcessor
    }
}

impl ILayoutProcessor for HierarchicalPortPositionProcessor {
    fn process(&mut self, _lg: &mut LGraphArena, _graph: LGraphId, _monitor: &mut dyn IElkProgressMonitor) {
        unimplemented!("HierarchicalPortPositionProcessor is not ported yet")
    }

    fn name(&self) -> &'static str {
        "HierarchicalPortPositionProcessor"
    }
}
