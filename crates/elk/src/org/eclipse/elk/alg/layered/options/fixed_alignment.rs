//! Port of `alg/layered/options/FixedAlignment.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum FixedAlignment {
    NONE,
    LEFTUP,
    RIGHTUP,
    LEFTDOWN,
    RIGHTDOWN,
    BALANCED,
}

impl FixedAlignment {
    pub const ALL: [FixedAlignment; 6] = [FixedAlignment::NONE, FixedAlignment::LEFTUP, FixedAlignment::RIGHTUP, FixedAlignment::LEFTDOWN, FixedAlignment::RIGHTDOWN, FixedAlignment::BALANCED];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            FixedAlignment::NONE => "NONE",
            FixedAlignment::LEFTUP => "LEFTUP",
            FixedAlignment::RIGHTUP => "RIGHTUP",
            FixedAlignment::LEFTDOWN => "LEFTDOWN",
            FixedAlignment::RIGHTDOWN => "RIGHTDOWN",
            FixedAlignment::BALANCED => "BALANCED",
        }
    }

    /// `FixedAlignment(rawValue:)`.
    pub fn from_raw(s: &str) -> Option<FixedAlignment> {
        FixedAlignment::ALL.iter().copied().find(|d| d.name() == s)
    }
}

crate::enum_ordinal!(FixedAlignment);
