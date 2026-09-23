//! Port of `core/options/EdgeType.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum EdgeType {
    NONE,
    DIRECTED,
    UNDIRECTED,
    ASSOCIATION,
    GENERALIZATION,
    DEPENDENCY,
}

impl EdgeType {
    pub const ALL: [EdgeType; 6] = [EdgeType::NONE, EdgeType::DIRECTED, EdgeType::UNDIRECTED, EdgeType::ASSOCIATION, EdgeType::GENERALIZATION, EdgeType::DEPENDENCY];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            EdgeType::NONE => "NONE",
            EdgeType::DIRECTED => "DIRECTED",
            EdgeType::UNDIRECTED => "UNDIRECTED",
            EdgeType::ASSOCIATION => "ASSOCIATION",
            EdgeType::GENERALIZATION => "GENERALIZATION",
            EdgeType::DEPENDENCY => "DEPENDENCY",
        }
    }
}

crate::enum_ordinal!(EdgeType);
