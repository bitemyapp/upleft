//! Port of `alg/layered/intermediate/unzipping/AlternatingLayerUnzipper.swift`.
//!
//! A stub in elk-swift: an empty class whose `process` (declared in
//! `IntermediateProcessorStrategy.swift`) does nothing.

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;

#[derive(Default)]
pub struct AlternatingLayerUnzipper;

impl AlternatingLayerUnzipper {
    pub fn new() -> AlternatingLayerUnzipper {
        AlternatingLayerUnzipper
    }
}

impl ILayoutProcessor for AlternatingLayerUnzipper {
    fn process(&mut self, _lg: &mut LGraphArena, _graph: LGraphId, _monitor: &mut dyn IElkProgressMonitor) {}

    fn name(&self) -> &'static str {
        "AlternatingLayerUnzipper"
    }
}
