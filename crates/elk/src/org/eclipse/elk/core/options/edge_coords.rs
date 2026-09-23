//! Port of `core/options/EdgeCoords.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum EdgeCoords {
    INHERIT,
    CONTAINER,
    PARENT,
    ROOT,
}

impl EdgeCoords {
    pub const ALL: [EdgeCoords; 4] = [EdgeCoords::INHERIT, EdgeCoords::CONTAINER, EdgeCoords::PARENT, EdgeCoords::ROOT];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            EdgeCoords::INHERIT => "INHERIT",
            EdgeCoords::CONTAINER => "CONTAINER",
            EdgeCoords::PARENT => "PARENT",
            EdgeCoords::ROOT => "ROOT",
        }
    }
}

crate::enum_ordinal!(EdgeCoords);
