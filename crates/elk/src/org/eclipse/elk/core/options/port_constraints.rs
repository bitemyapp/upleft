//! Port of `core/options/PortConstraints.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum PortConstraints {
    UNDEFINED,
    FREE,
    FIXED_SIDE,
    FIXED_ORDER,
    FIXED_RATIO,
    FIXED_POS,
}

impl PortConstraints {
    pub const ALL: [PortConstraints; 6] = [PortConstraints::UNDEFINED, PortConstraints::FREE, PortConstraints::FIXED_SIDE, PortConstraints::FIXED_ORDER, PortConstraints::FIXED_RATIO, PortConstraints::FIXED_POS];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            PortConstraints::UNDEFINED => "UNDEFINED",
            PortConstraints::FREE => "FREE",
            PortConstraints::FIXED_SIDE => "FIXED_SIDE",
            PortConstraints::FIXED_ORDER => "FIXED_ORDER",
            PortConstraints::FIXED_RATIO => "FIXED_RATIO",
            PortConstraints::FIXED_POS => "FIXED_POS",
        }
    }

    pub fn is_pos_fixed(self) -> bool {
        self == PortConstraints::FIXED_POS
    }

    pub fn is_ratio_fixed(self) -> bool {
        self == PortConstraints::FIXED_RATIO
    }

    pub fn is_order_fixed(self) -> bool {
        matches!(self, PortConstraints::FIXED_ORDER | PortConstraints::FIXED_RATIO | PortConstraints::FIXED_POS)
    }

    pub fn is_side_fixed(self) -> bool {
        self != PortConstraints::FREE && self != PortConstraints::UNDEFINED
    }
}

crate::enum_ordinal!(PortConstraints);
