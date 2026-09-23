//! Port of `alg/layered/options/DirectionCongruency.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum DirectionCongruency {
    READING_DIRECTION,
    ROTATION,
}

impl DirectionCongruency {
    pub const ALL: [DirectionCongruency; 2] = [DirectionCongruency::READING_DIRECTION, DirectionCongruency::ROTATION];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            DirectionCongruency::READING_DIRECTION => "READING_DIRECTION",
            DirectionCongruency::ROTATION => "ROTATION",
        }
    }
}

crate::enum_ordinal!(DirectionCongruency);
