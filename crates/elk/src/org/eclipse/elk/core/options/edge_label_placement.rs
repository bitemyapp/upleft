//! Port of `core/options/EdgeLabelPlacement.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum EdgeLabelPlacement {
    CENTER,
    HEAD,
    TAIL,
}

impl EdgeLabelPlacement {
    pub const ALL: [EdgeLabelPlacement; 3] = [EdgeLabelPlacement::CENTER, EdgeLabelPlacement::HEAD, EdgeLabelPlacement::TAIL];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            EdgeLabelPlacement::CENTER => "CENTER",
            EdgeLabelPlacement::HEAD => "HEAD",
            EdgeLabelPlacement::TAIL => "TAIL",
        }
    }

    pub fn is_end_label_placement(self) -> bool {
        self == EdgeLabelPlacement::HEAD || self == EdgeLabelPlacement::TAIL
    }
}

crate::enum_ordinal!(EdgeLabelPlacement);
