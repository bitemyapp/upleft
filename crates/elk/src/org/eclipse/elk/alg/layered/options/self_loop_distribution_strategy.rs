//! Port of `alg/layered/options/SelfLoopDistributionStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum SelfLoopDistributionStrategy {
    EQUALLY,
    NORTH,
    NORTH_SOUTH,
}

impl SelfLoopDistributionStrategy {
    pub const ALL: [SelfLoopDistributionStrategy; 3] = [SelfLoopDistributionStrategy::EQUALLY, SelfLoopDistributionStrategy::NORTH, SelfLoopDistributionStrategy::NORTH_SOUTH];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            SelfLoopDistributionStrategy::EQUALLY => "EQUALLY",
            SelfLoopDistributionStrategy::NORTH => "NORTH",
            SelfLoopDistributionStrategy::NORTH_SOUTH => "NORTH_SOUTH",
        }
    }
}

crate::enum_ordinal!(SelfLoopDistributionStrategy);
