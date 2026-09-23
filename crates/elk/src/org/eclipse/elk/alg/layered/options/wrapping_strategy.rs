//! Port of `alg/layered/options/WrappingStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum WrappingStrategy {
    OFF,
    SINGLE_EDGE,
    MULTI_EDGE,
}

impl WrappingStrategy {
    pub const ALL: [WrappingStrategy; 3] = [WrappingStrategy::OFF, WrappingStrategy::SINGLE_EDGE, WrappingStrategy::MULTI_EDGE];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            WrappingStrategy::OFF => "OFF",
            WrappingStrategy::SINGLE_EDGE => "SINGLE_EDGE",
            WrappingStrategy::MULTI_EDGE => "MULTI_EDGE",
        }
    }
}

crate::enum_ordinal!(WrappingStrategy);
