//! Port of `alg/layered/intermediate/LabelManagementProcessor.swift`.
//!
//! Its `process` is the no-op declared in `IntermediateProcessorStrategy.swift`
//! (and it only runs when a label manager is set, which never happens).

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;

pub struct LabelManagementProcessor {
    pub center_labels: bool,
}

impl LabelManagementProcessor {
    pub fn new(center_labels: bool) -> LabelManagementProcessor {
        LabelManagementProcessor { center_labels }
    }
}

impl ILayoutProcessor for LabelManagementProcessor {
    fn process(&mut self, _lg: &mut LGraphArena, _graph: LGraphId, _monitor: &mut dyn IElkProgressMonitor) {}

    fn name(&self) -> &'static str {
        "LabelManagementProcessor"
    }
}
