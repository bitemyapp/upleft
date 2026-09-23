//! Port of `alg/layered/p5edges/orthogonal/direction/RoutingDirection.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum RoutingDirection {
    WEST_TO_EAST,
    NORTH_TO_SOUTH,
    SOUTH_TO_NORTH,
}
