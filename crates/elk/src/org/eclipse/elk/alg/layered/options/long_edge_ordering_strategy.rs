//! Port of `alg/layered/options/LongEdgeOrderingStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum LongEdgeOrderingStrategy {
    DUMMY_NODE_OVER,
    DUMMY_NODE_UNDER,
    EQUAL,
}

impl LongEdgeOrderingStrategy {
    pub const ALL: [LongEdgeOrderingStrategy; 3] = [LongEdgeOrderingStrategy::DUMMY_NODE_OVER, LongEdgeOrderingStrategy::DUMMY_NODE_UNDER, LongEdgeOrderingStrategy::EQUAL];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            LongEdgeOrderingStrategy::DUMMY_NODE_OVER => "DUMMY_NODE_OVER",
            LongEdgeOrderingStrategy::DUMMY_NODE_UNDER => "DUMMY_NODE_UNDER",
            LongEdgeOrderingStrategy::EQUAL => "EQUAL",
        }
    }

    pub fn return_value(self) -> i64 {
        match self {
            LongEdgeOrderingStrategy::DUMMY_NODE_OVER => i32::MAX as i64,
            LongEdgeOrderingStrategy::DUMMY_NODE_UNDER => i32::MIN as i64,
            LongEdgeOrderingStrategy::EQUAL => 0,
        }
    }
}

crate::enum_ordinal!(LongEdgeOrderingStrategy);
