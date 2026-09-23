//! Port of `alg/layered/options/CrossingMinimizationStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum CrossingMinimizationStrategy {
    LAYER_SWEEP,
    MEDIAN_LAYER_SWEEP,
    INTERACTIVE,
    NONE,
}

impl CrossingMinimizationStrategy {
    pub const ALL: [CrossingMinimizationStrategy; 4] = [CrossingMinimizationStrategy::LAYER_SWEEP, CrossingMinimizationStrategy::MEDIAN_LAYER_SWEEP, CrossingMinimizationStrategy::INTERACTIVE, CrossingMinimizationStrategy::NONE];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            CrossingMinimizationStrategy::LAYER_SWEEP => "LAYER_SWEEP",
            CrossingMinimizationStrategy::MEDIAN_LAYER_SWEEP => "MEDIAN_LAYER_SWEEP",
            CrossingMinimizationStrategy::INTERACTIVE => "INTERACTIVE",
            CrossingMinimizationStrategy::NONE => "NONE",
        }
    }
}

crate::enum_ordinal!(CrossingMinimizationStrategy);
