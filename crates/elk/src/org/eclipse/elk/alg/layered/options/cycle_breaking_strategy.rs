//! Port of `alg/layered/options/CycleBreakingStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum CycleBreakingStrategy {
    GREEDY,
    DEPTH_FIRST,
    INTERACTIVE,
    MODEL_ORDER,
    GREEDY_MODEL_ORDER,
    SCC_CONNECTIVITY,
    SCC_NODE_TYPE,
    DFS_NODE_ORDER,
    BFS_NODE_ORDER,
}

impl CycleBreakingStrategy {
    pub const ALL: [CycleBreakingStrategy; 9] = [CycleBreakingStrategy::GREEDY, CycleBreakingStrategy::DEPTH_FIRST, CycleBreakingStrategy::INTERACTIVE, CycleBreakingStrategy::MODEL_ORDER, CycleBreakingStrategy::GREEDY_MODEL_ORDER, CycleBreakingStrategy::SCC_CONNECTIVITY, CycleBreakingStrategy::SCC_NODE_TYPE, CycleBreakingStrategy::DFS_NODE_ORDER, CycleBreakingStrategy::BFS_NODE_ORDER];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            CycleBreakingStrategy::GREEDY => "GREEDY",
            CycleBreakingStrategy::DEPTH_FIRST => "DEPTH_FIRST",
            CycleBreakingStrategy::INTERACTIVE => "INTERACTIVE",
            CycleBreakingStrategy::MODEL_ORDER => "MODEL_ORDER",
            CycleBreakingStrategy::GREEDY_MODEL_ORDER => "GREEDY_MODEL_ORDER",
            CycleBreakingStrategy::SCC_CONNECTIVITY => "SCC_CONNECTIVITY",
            CycleBreakingStrategy::SCC_NODE_TYPE => "SCC_NODE_TYPE",
            CycleBreakingStrategy::DFS_NODE_ORDER => "DFS_NODE_ORDER",
            CycleBreakingStrategy::BFS_NODE_ORDER => "BFS_NODE_ORDER",
        }
    }
}

crate::enum_ordinal!(CycleBreakingStrategy);
