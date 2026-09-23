//! Port of `alg/layered/options/GraphCompactionStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum GraphCompactionStrategy {
    NONE,
    LEFT,
    RIGHT,
    LEFT_RIGHT_CONSTRAINT_LOCKING,
    LEFT_RIGHT_CONNECTION_LOCKING,
    EDGE_LENGTH,
}

impl GraphCompactionStrategy {
    pub const ALL: [GraphCompactionStrategy; 6] = [GraphCompactionStrategy::NONE, GraphCompactionStrategy::LEFT, GraphCompactionStrategy::RIGHT, GraphCompactionStrategy::LEFT_RIGHT_CONSTRAINT_LOCKING, GraphCompactionStrategy::LEFT_RIGHT_CONNECTION_LOCKING, GraphCompactionStrategy::EDGE_LENGTH];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            GraphCompactionStrategy::NONE => "NONE",
            GraphCompactionStrategy::LEFT => "LEFT",
            GraphCompactionStrategy::RIGHT => "RIGHT",
            GraphCompactionStrategy::LEFT_RIGHT_CONSTRAINT_LOCKING => "LEFT_RIGHT_CONSTRAINT_LOCKING",
            GraphCompactionStrategy::LEFT_RIGHT_CONNECTION_LOCKING => "LEFT_RIGHT_CONNECTION_LOCKING",
            GraphCompactionStrategy::EDGE_LENGTH => "EDGE_LENGTH",
        }
    }
}

crate::enum_ordinal!(GraphCompactionStrategy);
