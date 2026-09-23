//! Port of `alg/common/nodespacing/internal/NodeLabelLocation.swift`.
//!
//! Enumeration over all possible label placements and associated things. The
//! elk-swift `fromNodeLabelPlacement` differs from Java's for outside labels
//! with `V_CENTER` (it maps them to `OUT_L_*`); ported as is.

use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::container_area::ContainerArea;
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::horizontal_label_alignment::HorizontalLabelAlignment;
use crate::org::eclipse::elk::alg::common::nodespacing::cellsystem::vertical_label_alignment::VerticalLabelAlignment;
use crate::org::eclipse::elk::core::options::node_label_placement::NodeLabelPlacement;
use crate::org::eclipse::elk::core::options::port_side::PortSide;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum NodeLabelLocation {
    // Outside placements
    OUT_T_L,
    OUT_T_C,
    OUT_T_R,
    OUT_B_L,
    OUT_B_C,
    OUT_B_R,
    OUT_L_T,
    OUT_L_C,
    OUT_L_B,
    OUT_R_T,
    OUT_R_C,
    OUT_R_B,

    // Inside placements
    IN_T_L,
    IN_T_C,
    IN_T_R,
    IN_C_L,
    IN_C_C,
    IN_C_R,
    IN_B_L,
    IN_B_C,
    IN_B_R,

    // Undefined
    UNDEFINED,
}

use NodeLabelLocation::*;

impl NodeLabelLocation {
    /// `allCases`.
    pub const ALL: [NodeLabelLocation; 22] = [
        OUT_T_L, OUT_T_C, OUT_T_R, OUT_B_L, OUT_B_C, OUT_B_R, OUT_L_T, OUT_L_C, OUT_L_B, OUT_R_T, OUT_R_C, OUT_R_B, IN_T_L,
        IN_T_C, IN_T_R, IN_C_L, IN_C_C, IN_C_R, IN_B_L, IN_B_C, IN_B_R, UNDEFINED,
    ];

    pub const COUNT: usize = 22;

    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn from_node_label_placement(label_placement: NodeLabelPlacement) -> NodeLabelLocation {
        let is_inside = label_placement.contains(NodeLabelPlacement::INSIDE);
        let has_v_top = label_placement.contains(NodeLabelPlacement::V_TOP);
        let has_v_bottom = label_placement.contains(NodeLabelPlacement::V_BOTTOM);
        let has_v_center = label_placement.contains(NodeLabelPlacement::V_CENTER);
        let has_h_left = label_placement.contains(NodeLabelPlacement::H_LEFT);
        let has_h_right = label_placement.contains(NodeLabelPlacement::H_RIGHT);
        let has_h_center = label_placement.contains(NodeLabelPlacement::H_CENTER);

        if is_inside {
            if has_v_top {
                if has_h_left {
                    return IN_T_L;
                } else if has_h_center {
                    return IN_T_C;
                } else if has_h_right {
                    return IN_T_R;
                }
            } else if has_v_bottom {
                if has_h_left {
                    return IN_B_L;
                } else if has_h_center {
                    return IN_B_C;
                } else if has_h_right {
                    return IN_B_R;
                }
            } else if has_v_center {
                if has_h_left {
                    return IN_C_L;
                } else if has_h_center {
                    return IN_C_C;
                } else if has_h_right {
                    return IN_C_R;
                }
            }
        } else if has_v_top {
            if has_h_left {
                return OUT_T_L;
            } else if has_h_center {
                return OUT_T_C;
            } else if has_h_right {
                return OUT_T_R;
            }
        } else if has_v_bottom {
            if has_h_left {
                return OUT_B_L;
            } else if has_h_center {
                return OUT_B_C;
            } else if has_h_right {
                return OUT_B_R;
            }
        } else if has_v_center {
            if has_h_left {
                return OUT_L_T;
            } else if has_h_center {
                return OUT_L_C;
            } else if has_h_right {
                return OUT_L_B;
            }
        }
        UNDEFINED
    }

    pub fn horizontal_alignment(self) -> HorizontalLabelAlignment {
        match self {
            OUT_T_L | OUT_B_L | OUT_L_T | OUT_L_C | OUT_L_B | IN_T_L | IN_B_L | IN_C_L => HorizontalLabelAlignment::LEFT,
            OUT_T_C | OUT_B_C | IN_T_C | IN_B_C | IN_C_C => HorizontalLabelAlignment::CENTER,
            OUT_T_R | OUT_B_R | OUT_R_T | OUT_R_C | OUT_R_B | IN_T_R | IN_B_R | IN_C_R => HorizontalLabelAlignment::RIGHT,
            _ => HorizontalLabelAlignment::CENTER,
        }
    }

    pub fn vertical_alignment(self) -> VerticalLabelAlignment {
        match self {
            OUT_T_L | OUT_T_C | OUT_T_R | OUT_L_T | OUT_R_T | IN_T_L | IN_T_C | IN_T_R | IN_C_L | IN_C_C | IN_C_R => {
                VerticalLabelAlignment::TOP
            }
            OUT_B_L | OUT_B_C | OUT_B_R | OUT_L_B | OUT_R_B | IN_B_L | IN_B_C | IN_B_R => VerticalLabelAlignment::BOTTOM,
            OUT_L_C | OUT_R_C => VerticalLabelAlignment::CENTER,
            _ => VerticalLabelAlignment::CENTER,
        }
    }

    pub fn get_horizontal_alignment(self) -> HorizontalLabelAlignment {
        self.horizontal_alignment()
    }

    pub fn get_vertical_alignment(self) -> VerticalLabelAlignment {
        self.vertical_alignment()
    }

    pub fn container_row(self) -> ContainerArea {
        match self {
            OUT_T_L | OUT_T_C | OUT_T_R | OUT_L_T | OUT_R_T | IN_T_L | IN_T_C | IN_T_R => ContainerArea::BEGIN,
            OUT_B_L | OUT_B_C | OUT_B_R | OUT_L_B | OUT_R_B | IN_B_L | IN_B_C | IN_B_R => ContainerArea::END,
            OUT_L_C | OUT_R_C | IN_C_L | IN_C_C | IN_C_R => ContainerArea::CENTER,
            _ => ContainerArea::CENTER,
        }
    }

    pub fn container_column(self) -> ContainerArea {
        match self {
            OUT_T_L | OUT_B_L | OUT_L_T | OUT_L_C | OUT_L_B | IN_T_L | IN_B_L | IN_C_L => ContainerArea::BEGIN,
            OUT_T_R | OUT_B_R | OUT_R_T | OUT_R_C | OUT_R_B | IN_T_R | IN_B_R | IN_C_R => ContainerArea::END,
            OUT_T_C | OUT_B_C | IN_T_C | IN_B_C | IN_C_C => ContainerArea::CENTER,
            _ => ContainerArea::CENTER,
        }
    }

    pub fn get_container_row(self) -> ContainerArea {
        self.container_row()
    }

    pub fn get_container_column(self) -> ContainerArea {
        self.container_column()
    }

    pub fn is_inside_location(self) -> bool {
        matches!(self, IN_T_L | IN_T_C | IN_T_R | IN_C_L | IN_C_C | IN_C_R | IN_B_L | IN_B_C | IN_B_R)
    }

    pub fn get_outside_side(self) -> PortSide {
        match self {
            OUT_T_L | OUT_T_C | OUT_T_R => PortSide::NORTH,
            OUT_B_L | OUT_B_C | OUT_B_R => PortSide::SOUTH,
            OUT_L_T | OUT_L_C | OUT_L_B => PortSide::WEST,
            OUT_R_T | OUT_R_C | OUT_R_B => PortSide::EAST,
            _ => PortSide::UNDEFINED,
        }
    }
}

crate::enum_ordinal!(NodeLabelLocation);
