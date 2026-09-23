//! Port of `alg/layered/options/PortSortingStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum PortSortingStrategy {
    INPUT_ORDER,
    PORT_DEGREE,
}

impl PortSortingStrategy {
    pub const ALL: [PortSortingStrategy; 2] = [PortSortingStrategy::INPUT_ORDER, PortSortingStrategy::PORT_DEGREE];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            PortSortingStrategy::INPUT_ORDER => "INPUT_ORDER",
            PortSortingStrategy::PORT_DEGREE => "PORT_DEGREE",
        }
    }
}

crate::enum_ordinal!(PortSortingStrategy);
