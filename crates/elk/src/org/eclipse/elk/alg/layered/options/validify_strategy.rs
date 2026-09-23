//! Port of `alg/layered/options/ValidifyStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum ValidifyStrategy {
    NO,
    GREEDY,
    LOOK_BACK,
}

impl ValidifyStrategy {
    pub const ALL: [ValidifyStrategy; 3] = [ValidifyStrategy::NO, ValidifyStrategy::GREEDY, ValidifyStrategy::LOOK_BACK];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            ValidifyStrategy::NO => "NO",
            ValidifyStrategy::GREEDY => "GREEDY",
            ValidifyStrategy::LOOK_BACK => "LOOK_BACK",
        }
    }
}

crate::enum_ordinal!(ValidifyStrategy);
