//! Port of `core/options/PortAlignment.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum PortAlignment {
    DISTRIBUTED,
    JUSTIFIED,
    BEGIN,
    CENTER,
    END,
}

impl PortAlignment {
    pub const ALL: [PortAlignment; 5] = [PortAlignment::DISTRIBUTED, PortAlignment::JUSTIFIED, PortAlignment::BEGIN, PortAlignment::CENTER, PortAlignment::END];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            PortAlignment::DISTRIBUTED => "DISTRIBUTED",
            PortAlignment::JUSTIFIED => "JUSTIFIED",
            PortAlignment::BEGIN => "BEGIN",
            PortAlignment::CENTER => "CENTER",
            PortAlignment::END => "END",
        }
    }
}

crate::enum_ordinal!(PortAlignment);
