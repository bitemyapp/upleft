//! Port of `alg/layered/options/SplineRoutingMode.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum SplineRoutingMode {
    CONSERVATIVE,
    CONSERVATIVE_SOFT,
    SLOPPY,
}

impl SplineRoutingMode {
    pub const ALL: [SplineRoutingMode; 3] = [SplineRoutingMode::CONSERVATIVE, SplineRoutingMode::CONSERVATIVE_SOFT, SplineRoutingMode::SLOPPY];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            SplineRoutingMode::CONSERVATIVE => "CONSERVATIVE",
            SplineRoutingMode::CONSERVATIVE_SOFT => "CONSERVATIVE_SOFT",
            SplineRoutingMode::SLOPPY => "SLOPPY",
        }
    }
}

crate::enum_ordinal!(SplineRoutingMode);
