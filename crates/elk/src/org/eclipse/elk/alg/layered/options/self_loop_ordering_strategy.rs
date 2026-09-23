//! Port of `alg/layered/options/SelfLoopOrderingStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum SelfLoopOrderingStrategy {
    STACKED,
    REVERSE_STACKED,
    SEQUENCED,
}

impl SelfLoopOrderingStrategy {
    pub const ALL: [SelfLoopOrderingStrategy; 3] = [SelfLoopOrderingStrategy::STACKED, SelfLoopOrderingStrategy::REVERSE_STACKED, SelfLoopOrderingStrategy::SEQUENCED];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            SelfLoopOrderingStrategy::STACKED => "STACKED",
            SelfLoopOrderingStrategy::REVERSE_STACKED => "REVERSE_STACKED",
            SelfLoopOrderingStrategy::SEQUENCED => "SEQUENCED",
        }
    }
}

crate::enum_ordinal!(SelfLoopOrderingStrategy);
