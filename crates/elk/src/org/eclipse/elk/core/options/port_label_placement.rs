//! Port of `core/options/PortLabelPlacement.swift`.

crate::option_set!(PortLabelPlacement {
    OUTSIDE = 0,
    INSIDE = 1,
    NEXT_TO_PORT_IF_POSSIBLE = 2,
    ALWAYS_SAME_SIDE = 3,
    ALWAYS_OTHER_SAME_SIDE = 4,
    SPACE_EFFICIENT = 5,
});

impl PortLabelPlacement {
    pub const fn fixed() -> PortLabelPlacement {
        PortLabelPlacement(0)
    }

    pub fn is_fixed(placement: PortLabelPlacement) -> bool {
        !placement.contains(Self::INSIDE) && !placement.contains(Self::OUTSIDE)
    }

    pub fn is_valid(placement: PortLabelPlacement) -> bool {
        if placement.contains(Self::INSIDE) && placement.contains(Self::OUTSIDE) {
            return false;
        }
        let pos_count = placement.contains(Self::ALWAYS_SAME_SIDE) as i32
            + placement.contains(Self::ALWAYS_OTHER_SAME_SIDE) as i32
            + placement.contains(Self::SPACE_EFFICIENT) as i32;
        pos_count <= 1
    }
}
