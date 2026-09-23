//! Port of `alg/layered/options/GreedySwitchType.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum GreedySwitchType {
    ONE_SIDED,
    TWO_SIDED,
    OFF,
}

impl GreedySwitchType {
    pub const ALL: [GreedySwitchType; 3] = [GreedySwitchType::ONE_SIDED, GreedySwitchType::TWO_SIDED, GreedySwitchType::OFF];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            GreedySwitchType::ONE_SIDED => "ONE_SIDED",
            GreedySwitchType::TWO_SIDED => "TWO_SIDED",
            GreedySwitchType::OFF => "OFF",
        }
    }
}

crate::enum_ordinal!(GreedySwitchType);
