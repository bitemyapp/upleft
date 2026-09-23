//! Port of `alg/layered/options/EdgeLabelSideSelection.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum EdgeLabelSideSelection {
    ALWAYS_UP,
    ALWAYS_DOWN,
    DIRECTION_UP,
    DIRECTION_DOWN,
    SMART_UP,
    SMART_DOWN,
}

impl EdgeLabelSideSelection {
    pub const ALL: [EdgeLabelSideSelection; 6] = [EdgeLabelSideSelection::ALWAYS_UP, EdgeLabelSideSelection::ALWAYS_DOWN, EdgeLabelSideSelection::DIRECTION_UP, EdgeLabelSideSelection::DIRECTION_DOWN, EdgeLabelSideSelection::SMART_UP, EdgeLabelSideSelection::SMART_DOWN];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            EdgeLabelSideSelection::ALWAYS_UP => "ALWAYS_UP",
            EdgeLabelSideSelection::ALWAYS_DOWN => "ALWAYS_DOWN",
            EdgeLabelSideSelection::DIRECTION_UP => "DIRECTION_UP",
            EdgeLabelSideSelection::DIRECTION_DOWN => "DIRECTION_DOWN",
            EdgeLabelSideSelection::SMART_UP => "SMART_UP",
            EdgeLabelSideSelection::SMART_DOWN => "SMART_DOWN",
        }
    }

    pub fn transpose(self) -> EdgeLabelSideSelection {
        use EdgeLabelSideSelection::*;
        match self {
            ALWAYS_UP => ALWAYS_DOWN,
            ALWAYS_DOWN => ALWAYS_UP,
            DIRECTION_UP => DIRECTION_DOWN,
            DIRECTION_DOWN => DIRECTION_UP,
            SMART_UP => SMART_DOWN,
            SMART_DOWN => SMART_UP,
        }
    }
}

crate::enum_ordinal!(EdgeLabelSideSelection);
