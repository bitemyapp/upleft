//! Port of `alg/layered/options/EdgeStraighteningStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum EdgeStraighteningStrategy {
    NONE,
    IMPROVE_STRAIGHTNESS,
}

impl EdgeStraighteningStrategy {
    pub const ALL: [EdgeStraighteningStrategy; 2] = [EdgeStraighteningStrategy::NONE, EdgeStraighteningStrategy::IMPROVE_STRAIGHTNESS];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            EdgeStraighteningStrategy::NONE => "NONE",
            EdgeStraighteningStrategy::IMPROVE_STRAIGHTNESS => "IMPROVE_STRAIGHTNESS",
        }
    }
}

crate::enum_ordinal!(EdgeStraighteningStrategy);
