//! Port of `core/alg/ILayoutPhase.swift`.

use super::i_layout_processor::ILayoutProcessor;
use super::layout_processor_configuration::LayoutProcessorConfiguration;
use crate::org::eclipse::elk::alg::layered::graph::l_graph::{LGraphArena, LGraphId};

pub trait ILayoutPhase: ILayoutProcessor {
    /// The intermediate processors the phase needs. elk-swift only asks the
    /// phases that conform to `AnyLayoutPhaseBox` (see
    /// `AlgorithmAssembler.swift`): `GreedyCycleBreaker`,
    /// `NetworkSimplexLayerer`, `LongestPathLayerer`,
    /// `LayerSweepCrossingMinimizer`, `BKNodePlacer`, `SimpleNodePlacer`,
    /// `PolylineEdgeRouter` and `OrthogonalEdgeRouter`. Every other phase
    /// keeps this default and contributes nothing.
    fn get_layout_processor_configuration(&self, _lg: &LGraphArena, _graph: LGraphId) -> Option<LayoutProcessorConfiguration> {
        None
    }
}
