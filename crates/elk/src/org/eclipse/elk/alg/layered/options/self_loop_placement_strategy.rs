//! Port of `alg/layered/options/SelfLoopPlacementStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum SelfLoopPlacementStrategy {
    EQUALLY_DISTRIBUTED,
    NORTH_STACKED,
    NORTH_SEQUENCE,
}

impl SelfLoopPlacementStrategy {
    pub const ALL: [SelfLoopPlacementStrategy; 3] = [SelfLoopPlacementStrategy::EQUALLY_DISTRIBUTED, SelfLoopPlacementStrategy::NORTH_STACKED, SelfLoopPlacementStrategy::NORTH_SEQUENCE];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            SelfLoopPlacementStrategy::EQUALLY_DISTRIBUTED => "EQUALLY_DISTRIBUTED",
            SelfLoopPlacementStrategy::NORTH_STACKED => "NORTH_STACKED",
            SelfLoopPlacementStrategy::NORTH_SEQUENCE => "NORTH_SEQUENCE",
        }
    }
}

crate::enum_ordinal!(SelfLoopPlacementStrategy);
