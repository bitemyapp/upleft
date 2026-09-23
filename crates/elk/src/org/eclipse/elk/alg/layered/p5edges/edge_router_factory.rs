//! Port of `alg/layered/p5edges/EdgeRouterFactory.swift` (the phase creation
//! lives in [`crate::org::eclipse::elk::alg::layered::layered_phases::PhaseFactory`]).

use crate::org::eclipse::elk::core::options::edge_routing::EdgeRouting;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdgeRouterFactory {
    POLYLINE,
    ORTHOGONAL,
    SPLINES,
}

impl EdgeRouterFactory {
    pub fn factory_for(edge_routing: EdgeRouting) -> EdgeRouterFactory {
        match edge_routing {
            EdgeRouting::POLYLINE => EdgeRouterFactory::POLYLINE,
            EdgeRouting::ORTHOGONAL => EdgeRouterFactory::ORTHOGONAL,
            EdgeRouting::SPLINES => EdgeRouterFactory::SPLINES,
            _ => EdgeRouterFactory::ORTHOGONAL,
        }
    }
}
