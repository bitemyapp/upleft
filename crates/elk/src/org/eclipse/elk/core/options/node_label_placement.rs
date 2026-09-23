//! Port of `core/options/NodeLabelPlacement.swift`.

crate::option_set!(NodeLabelPlacement {
    H_LEFT = 0,
    H_CENTER = 1,
    H_RIGHT = 2,
    V_TOP = 3,
    V_CENTER = 4,
    V_BOTTOM = 5,
    INSIDE = 6,
    OUTSIDE = 7,
    H_PRIORITY = 8,
});

impl NodeLabelPlacement {
    pub const fn fixed() -> NodeLabelPlacement {
        NodeLabelPlacement(0)
    }

    pub const fn inside_top_left() -> NodeLabelPlacement {
        NodeLabelPlacement::of(&[Self::INSIDE, Self::V_TOP, Self::H_LEFT])
    }

    pub const fn inside_top_center() -> NodeLabelPlacement {
        NodeLabelPlacement::of(&[Self::INSIDE, Self::V_TOP, Self::H_CENTER])
    }

    pub const fn inside_top_right() -> NodeLabelPlacement {
        NodeLabelPlacement::of(&[Self::INSIDE, Self::V_TOP, Self::H_RIGHT])
    }

    pub const fn inside_center() -> NodeLabelPlacement {
        NodeLabelPlacement::of(&[Self::INSIDE, Self::V_CENTER, Self::H_CENTER])
    }

    pub const fn inside_bottom_left() -> NodeLabelPlacement {
        NodeLabelPlacement::of(&[Self::INSIDE, Self::V_BOTTOM, Self::H_LEFT])
    }

    pub const fn inside_bottom_center() -> NodeLabelPlacement {
        NodeLabelPlacement::of(&[Self::INSIDE, Self::V_BOTTOM, Self::H_CENTER])
    }

    pub const fn inside_bottom_right() -> NodeLabelPlacement {
        NodeLabelPlacement::of(&[Self::INSIDE, Self::V_BOTTOM, Self::H_RIGHT])
    }

    pub const fn outside_top_left() -> NodeLabelPlacement {
        NodeLabelPlacement::of(&[Self::OUTSIDE, Self::V_TOP, Self::H_LEFT])
    }

    pub const fn outside_top_center() -> NodeLabelPlacement {
        NodeLabelPlacement::of(&[Self::OUTSIDE, Self::V_TOP, Self::H_CENTER])
    }

    pub const fn outside_top_right() -> NodeLabelPlacement {
        NodeLabelPlacement::of(&[Self::OUTSIDE, Self::V_TOP, Self::H_RIGHT])
    }

    pub const fn outside_bottom_left() -> NodeLabelPlacement {
        NodeLabelPlacement::of(&[Self::OUTSIDE, Self::V_BOTTOM, Self::H_LEFT])
    }

    pub const fn outside_bottom_center() -> NodeLabelPlacement {
        NodeLabelPlacement::of(&[Self::OUTSIDE, Self::V_BOTTOM, Self::H_CENTER])
    }

    pub const fn outside_bottom_right() -> NodeLabelPlacement {
        NodeLabelPlacement::of(&[Self::OUTSIDE, Self::V_BOTTOM, Self::H_RIGHT])
    }

    pub fn is_valid(placement: NodeLabelPlacement) -> bool {
        if placement.contains(Self::INSIDE) && placement.contains(Self::OUTSIDE) {
            return false;
        }
        let h_count = placement.contains(Self::H_LEFT) as i32 + placement.contains(Self::H_CENTER) as i32 + placement.contains(Self::H_RIGHT) as i32;
        if h_count > 1 {
            return false;
        }
        let v_count = placement.contains(Self::V_TOP) as i32 + placement.contains(Self::V_CENTER) as i32 + placement.contains(Self::V_BOTTOM) as i32;
        if v_count > 1 {
            return false;
        }
        true
    }
}
