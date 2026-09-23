//! Port of `core/options/Alignment.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Alignment {
    AUTOMATIC,
    LEFT,
    RIGHT,
    TOP,
    BOTTOM,
    CENTER,
}

impl Alignment {
    pub const ALL: [Alignment; 6] = [Alignment::AUTOMATIC, Alignment::LEFT, Alignment::RIGHT, Alignment::TOP, Alignment::BOTTOM, Alignment::CENTER];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Alignment::AUTOMATIC => "AUTOMATIC",
            Alignment::LEFT => "LEFT",
            Alignment::RIGHT => "RIGHT",
            Alignment::TOP => "TOP",
            Alignment::BOTTOM => "BOTTOM",
            Alignment::CENTER => "CENTER",
        }
    }
}

crate::enum_ordinal!(Alignment);
