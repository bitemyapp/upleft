//! Port of `core/options/EdgeRouting.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum EdgeRouting {
    UNDEFINED,
    POLYLINE,
    ORTHOGONAL,
    SPLINES,
}

impl EdgeRouting {
    pub const ALL: [EdgeRouting; 4] = [EdgeRouting::UNDEFINED, EdgeRouting::POLYLINE, EdgeRouting::ORTHOGONAL, EdgeRouting::SPLINES];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            EdgeRouting::UNDEFINED => "UNDEFINED",
            EdgeRouting::POLYLINE => "POLYLINE",
            EdgeRouting::ORTHOGONAL => "ORTHOGONAL",
            EdgeRouting::SPLINES => "SPLINES",
        }
    }

    /// `EdgeRouting(rawValue:)`.
    pub fn from_raw(s: &str) -> Option<EdgeRouting> {
        EdgeRouting::ALL.iter().copied().find(|d| d.name() == s)
    }
}

crate::enum_ordinal!(EdgeRouting);
