//! Port of `alg/layered/options/OrderingStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum OrderingStrategy {
    NONE,
    NODES_AND_EDGES,
    PREFER_EDGES,
    PREFER_NODES,
}

impl OrderingStrategy {
    pub const ALL: [OrderingStrategy; 4] = [OrderingStrategy::NONE, OrderingStrategy::NODES_AND_EDGES, OrderingStrategy::PREFER_EDGES, OrderingStrategy::PREFER_NODES];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            OrderingStrategy::NONE => "NONE",
            OrderingStrategy::NODES_AND_EDGES => "NODES_AND_EDGES",
            OrderingStrategy::PREFER_EDGES => "PREFER_EDGES",
            OrderingStrategy::PREFER_NODES => "PREFER_NODES",
        }
    }

    /// `OrderingStrategy(rawValue:)`.
    pub fn from_raw(s: &str) -> Option<OrderingStrategy> {
        OrderingStrategy::ALL.iter().copied().find(|d| d.name() == s)
    }
}

crate::enum_ordinal!(OrderingStrategy);
