//! Port of `vendor/elk-swift/Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/intermediate/loops/org_eclipse_elk_alg_layered_intermediate_loops_SelfLoopType.swift`.

use crate::bridge::java_compat::EnumSet;
use crate::org::eclipse::elk::core::options::port_side::PortSide;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SelfLoopType {
    ONE_SIDE,
    TWO_SIDES_CORNER,
    TWO_SIDES_OPPOSING,
    THREE_SIDES,
    FOUR_SIDES,
}

impl SelfLoopType {
    /// `fromPortSides(_:)`. Only counts and membership tests, so the set's
    /// iteration order does not matter.
    pub fn from_port_sides(port_sides: EnumSet<PortSide>) -> Option<SelfLoopType> {
        if port_sides.contains(PortSide::UNDEFINED) {
            // assertionFailure (a no-op in release builds)
            return None;
        }

        match port_sides.len() {
            1 => Some(SelfLoopType::ONE_SIDE),
            2 => {
                let east_west = port_sides.contains(PortSide::EAST) && port_sides.contains(PortSide::WEST);
                let north_south = port_sides.contains(PortSide::NORTH) && port_sides.contains(PortSide::SOUTH);
                Some(if east_west || north_south { SelfLoopType::TWO_SIDES_OPPOSING } else { SelfLoopType::TWO_SIDES_CORNER })
            }
            3 => Some(SelfLoopType::THREE_SIDES),
            4 => Some(SelfLoopType::FOUR_SIDES),
            _ => None,
        }
    }
}
