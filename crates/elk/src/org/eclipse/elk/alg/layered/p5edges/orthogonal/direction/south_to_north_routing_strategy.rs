//! Port of `alg/layered/p5edges/orthogonal/direction/SouthToNorthRoutingStrategy.swift`.
//!
//! Like `NorthToSouthRoutingStrategy`, an empty subclass in elk-swift: it
//! behaves exactly like `BaseRoutingDirectionStrategy` (port positions 0, port
//! sides `UNDEFINED`, no bend points).

/// Marker for the (empty) Swift class.
#[derive(Clone, Copy, Debug, Default)]
pub struct SouthToNorthRoutingStrategy;
