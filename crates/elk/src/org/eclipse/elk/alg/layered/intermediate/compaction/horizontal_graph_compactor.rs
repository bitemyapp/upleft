//! Port of `alg/layered/intermediate/compaction/HorizontalGraphCompactor.swift`.
//!
//! elk-swift never finished this processor: past the `NONE` early exit it only
//! opens and closes the progress monitor. The compaction machinery it would
//! use (`OneDimensionalCompactor`, `LGraphToCGraphTransformer`, …) is
//! therefore unreachable.

use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};
use crate::org::eclipse::elk::alg::layered::options::graph_compaction_strategy::GraphCompactionStrategy;
use crate::org::eclipse::elk::alg::layered::options::layered_options as LayeredOptions;
use crate::org::eclipse::elk::core::alg::i_layout_processor::ILayoutProcessor;
use crate::org::eclipse::elk::core::util::i_elk_progress_monitor::IElkProgressMonitor;

#[derive(Default)]
pub struct HorizontalGraphCompactor;

impl HorizontalGraphCompactor {
    pub fn new() -> HorizontalGraphCompactor {
        HorizontalGraphCompactor
    }
}

impl ILayoutProcessor for HorizontalGraphCompactor {
    fn process(&mut self, lg: &mut LGraphArena, graph: LGraphId, monitor: &mut dyn IElkProgressMonitor) {
        let strategy = lg[graph].props.get_as::<GraphCompactionStrategy>(&LayeredOptions::COMPACTION_POST_COMPACTION_STRATEGY).unwrap_or(GraphCompactionStrategy::NONE);
        if strategy == GraphCompactionStrategy::NONE {
            return;
        }
        monitor.begin("Horizontal Compaction", 1.0);
        monitor.done();
    }

    fn name(&self) -> &'static str {
        "HorizontalGraphCompactor"
    }
}
