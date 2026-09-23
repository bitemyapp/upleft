//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/org_eclipse_elk_alg_layered_intermediate_GraphTransformer.swift`.
//!
//! Not ported yet.

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;

/// `GraphTransformer.Mode` (declared in `IntermediateProcessorStrategy.swift`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    TO_INPUT_DIRECTION,
    TO_INTERNAL_LTR,
}

pub struct GraphTransformer {
    pub mode: Mode,
}

impl GraphTransformer {
    pub fn new(mode: Mode) -> GraphTransformer {
        GraphTransformer { mode }
    }
}

impl ILayoutProcessor for GraphTransformer {
    fn process(&mut self, _lg: &mut LGraphArena, _graph: LGraphId, _monitor: &mut dyn IElkProgressMonitor) {
        unimplemented!("GraphTransformer is not ported yet")
    }

    fn name(&self) -> &'static str {
        "GraphTransformer"
    }
}
