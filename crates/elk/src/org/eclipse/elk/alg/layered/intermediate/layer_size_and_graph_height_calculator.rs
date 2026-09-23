//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/org_eclipse_elk_alg_layered_intermediate_LayerSizeAndGraphHeightCalculator.swift`.
//!
//! Not ported yet.

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;

#[derive(Default)]
pub struct LayerSizeAndGraphHeightCalculator;

impl LayerSizeAndGraphHeightCalculator {
    pub fn new() -> LayerSizeAndGraphHeightCalculator {
        LayerSizeAndGraphHeightCalculator
    }
}

impl ILayoutProcessor for LayerSizeAndGraphHeightCalculator {
    fn process(&mut self, _lg: &mut LGraphArena, _graph: LGraphId, _monitor: &mut dyn IElkProgressMonitor) {
        unimplemented!("LayerSizeAndGraphHeightCalculator is not ported yet")
    }

    fn name(&self) -> &'static str {
        "LayerSizeAndGraphHeightCalculator"
    }
}
