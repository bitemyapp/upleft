//! Port of `core/options/Direction.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum Direction {
    UNDEFINED,
    RIGHT,
    LEFT,
    DOWN,
    UP,
}

impl Direction {
    pub const ALL: [Direction; 5] = [Direction::UNDEFINED, Direction::RIGHT, Direction::LEFT, Direction::DOWN, Direction::UP];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Direction::UNDEFINED => "UNDEFINED",
            Direction::RIGHT => "RIGHT",
            Direction::LEFT => "LEFT",
            Direction::DOWN => "DOWN",
            Direction::UP => "UP",
        }
    }

    /// `Direction(rawValue:)`.
    pub fn from_raw(s: &str) -> Option<Direction> {
        Direction::ALL.iter().copied().find(|d| d.name() == s)
    }

    pub fn is_horizontal(self) -> bool {
        matches!(self, Direction::LEFT | Direction::RIGHT)
    }

    pub fn is_vertical(self) -> bool {
        matches!(self, Direction::UP | Direction::DOWN)
    }

    pub fn opposite(self) -> Direction {
        match self {
            Direction::LEFT => Direction::RIGHT,
            Direction::RIGHT => Direction::LEFT,
            Direction::UP => Direction::DOWN,
            Direction::DOWN => Direction::UP,
            Direction::UNDEFINED => Direction::UNDEFINED,
        }
    }
}

crate::enum_ordinal!(Direction);
