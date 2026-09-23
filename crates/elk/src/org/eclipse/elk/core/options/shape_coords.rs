//! Port of `core/options/ShapeCoords.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum ShapeCoords {
    INHERIT,
    PARENT,
    ROOT,
}

impl ShapeCoords {
    pub const ALL: [ShapeCoords; 3] = [ShapeCoords::INHERIT, ShapeCoords::PARENT, ShapeCoords::ROOT];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            ShapeCoords::INHERIT => "INHERIT",
            ShapeCoords::PARENT => "PARENT",
            ShapeCoords::ROOT => "ROOT",
        }
    }
}

crate::enum_ordinal!(ShapeCoords);
