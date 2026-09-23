//! Port of `alg/layered/options/PortType.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum PortType {
    UNDEFINED,
    INPUT,
    OUTPUT,
}

impl PortType {
    pub const ALL: [PortType; 3] = [PortType::UNDEFINED, PortType::INPUT, PortType::OUTPUT];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            PortType::UNDEFINED => "UNDEFINED",
            PortType::INPUT => "INPUT",
            PortType::OUTPUT => "OUTPUT",
        }
    }
}

crate::enum_ordinal!(PortType);
