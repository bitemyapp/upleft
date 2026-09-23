//! Port of `alg/layered/p5edges/orthogonal/direction/NorthToSouthRoutingStrategy.swift`.
//!
//! elk-swift declares this subclass without overriding anything, so every
//! "abstract" method falls through to `BaseRoutingDirectionStrategy`, whose
//! bodies are an `assertionFailure` (a no-op in release builds) plus a dummy
//! result: port positions are 0, both port sides `UNDEFINED`, and no bend
//! points are ever computed. Ported as is (see
//! [`super::base_routing_direction_strategy::RoutingStrategyKind`]).

/// Marker for the (empty) Swift class.
#[derive(Clone, Copy, Debug, Default)]
pub struct NorthToSouthRoutingStrategy;
