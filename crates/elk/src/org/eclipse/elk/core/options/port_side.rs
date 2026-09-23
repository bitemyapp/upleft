//! Port of `core/options/PortSide.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum PortSide {
    UNDEFINED,
    NORTH,
    EAST,
    SOUTH,
    WEST,
}

impl PortSide {
    pub const ALL: [PortSide; 5] = [PortSide::UNDEFINED, PortSide::NORTH, PortSide::EAST, PortSide::SOUTH, PortSide::WEST];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            PortSide::UNDEFINED => "UNDEFINED",
            PortSide::NORTH => "NORTH",
            PortSide::EAST => "EAST",
            PortSide::SOUTH => "SOUTH",
            PortSide::WEST => "WEST",
        }
    }

    pub fn right(self) -> PortSide {
        match self {
            PortSide::NORTH => PortSide::EAST,
            PortSide::EAST => PortSide::SOUTH,
            PortSide::SOUTH => PortSide::WEST,
            PortSide::WEST => PortSide::NORTH,
            PortSide::UNDEFINED => PortSide::UNDEFINED,
        }
    }

    pub fn left(self) -> PortSide {
        match self {
            PortSide::NORTH => PortSide::WEST,
            PortSide::EAST => PortSide::NORTH,
            PortSide::SOUTH => PortSide::EAST,
            PortSide::WEST => PortSide::SOUTH,
            PortSide::UNDEFINED => PortSide::UNDEFINED,
        }
    }

    pub fn opposed(self) -> PortSide {
        match self {
            PortSide::NORTH => PortSide::SOUTH,
            PortSide::EAST => PortSide::WEST,
            PortSide::SOUTH => PortSide::NORTH,
            PortSide::WEST => PortSide::EAST,
            PortSide::UNDEFINED => PortSide::UNDEFINED,
        }
    }

    pub fn are_adjacent(self, other: PortSide) -> bool {
        if self == PortSide::UNDEFINED {
            return false;
        }
        self.left() == other || self.right() == other
    }

    pub fn from_direction(direction: super::direction::Direction) -> PortSide {
        use super::direction::Direction;
        match direction {
            Direction::UP => PortSide::NORTH,
            Direction::RIGHT => PortSide::EAST,
            Direction::DOWN => PortSide::SOUTH,
            Direction::LEFT => PortSide::WEST,
            _ => PortSide::UNDEFINED,
        }
    }

    pub fn is_vertical(self) -> bool {
        self == PortSide::NORTH || self == PortSide::SOUTH
    }

    pub fn is_horizontal(self) -> bool {
        self == PortSide::WEST || self == PortSide::EAST
    }
}

crate::enum_ordinal!(PortSide);
