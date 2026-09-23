//! Port of `alg/layered/LayeredPhases.swift`, plus the phase factories
//! (`CycleBreakingStrategy.create()`, … `EdgeRouterFactory.create()`), which
//! the Swift declares on the strategy enums.
//!
//! The JSON importer can only ever produce the default strategies (it never
//! parses a strategy name into these enums, and the typed reads in
//! `GraphConfigurator` fall back to the defaults on any other value), so only
//! `GREEDY`, `NETWORK_SIMPLEX`, `LAYER_SWEEP`, `BRANDES_KOEPF` and the
//! orthogonal/polyline routers are reachable. The others are not ported.

use crate::org::eclipse::elk::alg::layered::options::{
    crossing_minimization_strategy::CrossingMinimizationStrategy, cycle_breaking_strategy::CycleBreakingStrategy,
    layering_strategy::LayeringStrategy, node_placement_strategy::NodePlacementStrategy,
};
use crate::org::eclipse::elk::alg::layered::p1cycles::greedy_cycle_breaker::GreedyCycleBreaker;
use crate::org::eclipse::elk::alg::layered::p2layers::network_simplex_layerer::NetworkSimplexLayerer;
use crate::org::eclipse::elk::alg::layered::p3order::layer_sweep_crossing_minimizer::{CrossMinType, LayerSweepCrossingMinimizer};
use crate::org::eclipse::elk::alg::layered::p4nodes::bk::bk_node_placer::BKNodePlacer;
use crate::org::eclipse::elk::alg::layered::p5edges::edge_router_factory::EdgeRouterFactory;
use crate::org::eclipse::elk::alg::layered::p5edges::orthogonal_edge_router::OrthogonalEdgeRouter;
use crate::org::eclipse::elk::alg::layered::p5edges::polyline_edge_router::PolylineEdgeRouter;
use crate::org::eclipse::elk::core::alg::i_layout_phase::ILayoutPhase;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum LayeredPhases {
    P1_CYCLE_BREAKING,
    P2_LAYERING,
    P3_NODE_ORDERING,
    P4_NODE_PLACEMENT,
    P5_EDGE_ROUTING,
}

impl LayeredPhases {
    pub const ALL: [LayeredPhases; 5] = [
        LayeredPhases::P1_CYCLE_BREAKING,
        LayeredPhases::P2_LAYERING,
        LayeredPhases::P3_NODE_ORDERING,
        LayeredPhases::P4_NODE_PLACEMENT,
        LayeredPhases::P5_EDGE_ROUTING,
    ];

    pub fn ordinal(self) -> usize {
        self as usize
    }
}

/// A phase factory (`ILayoutPhaseFactory`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PhaseFactory {
    CycleBreaking(CycleBreakingStrategy),
    Layering(LayeringStrategy),
    CrossingMinimization(CrossingMinimizationStrategy),
    NodePlacement(NodePlacementStrategy),
    EdgeRouting(EdgeRouterFactory),
}

impl PhaseFactory {
    pub fn create(self) -> Box<dyn ILayoutPhase> {
        match self {
            PhaseFactory::CycleBreaking(CycleBreakingStrategy::GREEDY) => Box::new(GreedyCycleBreaker::new()),
            PhaseFactory::Layering(LayeringStrategy::NETWORK_SIMPLEX) => Box::new(NetworkSimplexLayerer::new()),
            PhaseFactory::CrossingMinimization(CrossingMinimizationStrategy::LAYER_SWEEP) => {
                Box::new(LayerSweepCrossingMinimizer::new(CrossMinType::BARYCENTER))
            }
            PhaseFactory::CrossingMinimization(CrossingMinimizationStrategy::MEDIAN_LAYER_SWEEP) => {
                Box::new(LayerSweepCrossingMinimizer::new(CrossMinType::MEDIAN))
            }
            PhaseFactory::NodePlacement(NodePlacementStrategy::BRANDES_KOEPF) => Box::new(BKNodePlacer::new()),
            PhaseFactory::EdgeRouting(EdgeRouterFactory::POLYLINE) => Box::new(PolylineEdgeRouter::new()),
            // SPLINES asserts in Swift, then routes orthogonally.
            PhaseFactory::EdgeRouting(EdgeRouterFactory::ORTHOGONAL | EdgeRouterFactory::SPLINES) => Box::new(OrthogonalEdgeRouter::new()),
            other => unimplemented!("{other:?} cannot be selected through the JSON bridge and is not ported"),
        }
    }
}

crate::enum_ordinal!(LayeredPhases);
