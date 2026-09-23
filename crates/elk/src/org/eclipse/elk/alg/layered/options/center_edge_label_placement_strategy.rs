//! Port of `alg/layered/options/CenterEdgeLabelPlacementStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum CenterEdgeLabelPlacementStrategy {
    MEDIAN_LAYER,
    TAIL_LAYER,
    HEAD_LAYER,
    SPACE_EFFICIENT_LAYER,
    WIDEST_LAYER,
    CENTER_LAYER,
}

impl CenterEdgeLabelPlacementStrategy {
    pub const ALL: [CenterEdgeLabelPlacementStrategy; 6] = [CenterEdgeLabelPlacementStrategy::MEDIAN_LAYER, CenterEdgeLabelPlacementStrategy::TAIL_LAYER, CenterEdgeLabelPlacementStrategy::HEAD_LAYER, CenterEdgeLabelPlacementStrategy::SPACE_EFFICIENT_LAYER, CenterEdgeLabelPlacementStrategy::WIDEST_LAYER, CenterEdgeLabelPlacementStrategy::CENTER_LAYER];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            CenterEdgeLabelPlacementStrategy::MEDIAN_LAYER => "MEDIAN_LAYER",
            CenterEdgeLabelPlacementStrategy::TAIL_LAYER => "TAIL_LAYER",
            CenterEdgeLabelPlacementStrategy::HEAD_LAYER => "HEAD_LAYER",
            CenterEdgeLabelPlacementStrategy::SPACE_EFFICIENT_LAYER => "SPACE_EFFICIENT_LAYER",
            CenterEdgeLabelPlacementStrategy::WIDEST_LAYER => "WIDEST_LAYER",
            CenterEdgeLabelPlacementStrategy::CENTER_LAYER => "CENTER_LAYER",
        }
    }

    pub fn uses_label_size_information(self) -> bool {
        self == CenterEdgeLabelPlacementStrategy::WIDEST_LAYER
            || self == CenterEdgeLabelPlacementStrategy::CENTER_LAYER
            || self == CenterEdgeLabelPlacementStrategy::SPACE_EFFICIENT_LAYER
    }
}

crate::enum_ordinal!(CenterEdgeLabelPlacementStrategy);
