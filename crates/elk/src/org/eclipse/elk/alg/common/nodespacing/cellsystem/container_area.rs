//! Port of `alg/common/nodespacing/cellsystem/ContainerArea.swift`.

/// The three areas of containers that use three areas (the top row or left
/// column, the center, the bottom row or right column).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum ContainerArea {
    BEGIN = 0,
    CENTER = 1,
    END = 2,
}

impl ContainerArea {
    /// `values` / `allCases`.
    pub const ALL: [ContainerArea; 3] = [ContainerArea::BEGIN, ContainerArea::CENTER, ContainerArea::END];

    /// Number of container areas.
    pub const COUNT: usize = 3;

    pub fn ordinal(self) -> usize {
        self as usize
    }
}

crate::enum_ordinal!(ContainerArea);
