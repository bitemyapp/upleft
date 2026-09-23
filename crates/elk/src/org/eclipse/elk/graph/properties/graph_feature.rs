//! Port of `graph/properties/GraphFeature.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum GraphFeature {
    SELF_LOOPS,
    INSIDE_SELF_LOOPS,
    MULTI_EDGES,
    EDGE_LABELS,
    PORTS,
    COMPOUND,
    CLUSTERS,
    DISCONNECTED,
}
