//! Port of `alg/layered/options/NodePlacementStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum NodePlacementStrategy {
    SIMPLE,
    INTERACTIVE,
    LINEAR_SEGMENTS,
    BRANDES_KOEPF,
    NETWORK_SIMPLEX,
}

impl NodePlacementStrategy {
    pub const ALL: [NodePlacementStrategy; 5] = [NodePlacementStrategy::SIMPLE, NodePlacementStrategy::INTERACTIVE, NodePlacementStrategy::LINEAR_SEGMENTS, NodePlacementStrategy::BRANDES_KOEPF, NodePlacementStrategy::NETWORK_SIMPLEX];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            NodePlacementStrategy::SIMPLE => "SIMPLE",
            NodePlacementStrategy::INTERACTIVE => "INTERACTIVE",
            NodePlacementStrategy::LINEAR_SEGMENTS => "LINEAR_SEGMENTS",
            NodePlacementStrategy::BRANDES_KOEPF => "BRANDES_KOEPF",
            NodePlacementStrategy::NETWORK_SIMPLEX => "NETWORK_SIMPLEX",
        }
    }
}

crate::enum_ordinal!(NodePlacementStrategy);
