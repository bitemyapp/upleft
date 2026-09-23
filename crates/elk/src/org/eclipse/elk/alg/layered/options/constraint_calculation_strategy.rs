//! Port of `alg/layered/options/ConstraintCalculationStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum ConstraintCalculationStrategy {
    QUADRATIC,
    SCANLINE,
}

impl ConstraintCalculationStrategy {
    pub const ALL: [ConstraintCalculationStrategy; 2] = [ConstraintCalculationStrategy::QUADRATIC, ConstraintCalculationStrategy::SCANLINE];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            ConstraintCalculationStrategy::QUADRATIC => "QUADRATIC",
            ConstraintCalculationStrategy::SCANLINE => "SCANLINE",
        }
    }
}

crate::enum_ordinal!(ConstraintCalculationStrategy);
