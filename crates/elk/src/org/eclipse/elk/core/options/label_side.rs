//! Port of `core/options/LabelSide.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum LabelSide {
    UNKNOWN,
    ABOVE,
    BELOW,
    INLINE,
}

impl LabelSide {
    pub const ALL: [LabelSide; 4] = [LabelSide::UNKNOWN, LabelSide::ABOVE, LabelSide::BELOW, LabelSide::INLINE];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            LabelSide::UNKNOWN => "UNKNOWN",
            LabelSide::ABOVE => "ABOVE",
            LabelSide::BELOW => "BELOW",
            LabelSide::INLINE => "INLINE",
        }
    }

    pub fn opposite(self) -> LabelSide {
        match self {
            LabelSide::ABOVE => LabelSide::BELOW,
            LabelSide::BELOW => LabelSide::ABOVE,
            LabelSide::INLINE => LabelSide::INLINE,
            LabelSide::UNKNOWN => LabelSide::UNKNOWN,
        }
    }
}

crate::enum_ordinal!(LabelSide);
