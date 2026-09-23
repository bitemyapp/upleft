//! Port of `alg/layered/options/GraphProperties.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum GraphProperties {
    COMMENTS,
    EXTERNAL_PORTS,
    HYPEREDGES,
    HYPERNODES,
    NON_FREE_PORTS,
    NORTH_SOUTH_PORTS,
    SELF_LOOPS,
    CENTER_LABELS,
    END_LABELS,
    PARTITIONS,
}

impl GraphProperties {
    pub const ALL: [GraphProperties; 10] = [GraphProperties::COMMENTS, GraphProperties::EXTERNAL_PORTS, GraphProperties::HYPEREDGES, GraphProperties::HYPERNODES, GraphProperties::NON_FREE_PORTS, GraphProperties::NORTH_SOUTH_PORTS, GraphProperties::SELF_LOOPS, GraphProperties::CENTER_LABELS, GraphProperties::END_LABELS, GraphProperties::PARTITIONS];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            GraphProperties::COMMENTS => "COMMENTS",
            GraphProperties::EXTERNAL_PORTS => "EXTERNAL_PORTS",
            GraphProperties::HYPEREDGES => "HYPEREDGES",
            GraphProperties::HYPERNODES => "HYPERNODES",
            GraphProperties::NON_FREE_PORTS => "NON_FREE_PORTS",
            GraphProperties::NORTH_SOUTH_PORTS => "NORTH_SOUTH_PORTS",
            GraphProperties::SELF_LOOPS => "SELF_LOOPS",
            GraphProperties::CENTER_LABELS => "CENTER_LABELS",
            GraphProperties::END_LABELS => "END_LABELS",
            GraphProperties::PARTITIONS => "PARTITIONS",
        }
    }
}

crate::enum_ordinal!(GraphProperties);
