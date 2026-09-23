//! Port of `alg/layered/options/CuttingStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum CuttingStrategy {
    ARD,
    MSD,
    MANUAL,
}

impl CuttingStrategy {
    pub const ALL: [CuttingStrategy; 3] = [CuttingStrategy::ARD, CuttingStrategy::MSD, CuttingStrategy::MANUAL];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            CuttingStrategy::ARD => "ARD",
            CuttingStrategy::MSD => "MSD",
            CuttingStrategy::MANUAL => "MANUAL",
        }
    }
}

crate::enum_ordinal!(CuttingStrategy);
