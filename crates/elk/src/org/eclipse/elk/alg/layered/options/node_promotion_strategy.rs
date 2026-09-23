//! Port of `alg/layered/options/NodePromotionStrategy.swift`.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum NodePromotionStrategy {
    NONE,
    NIKOLOV,
    NIKOLOV_PIXEL,
    NIKOLOV_IMPROVED,
    NIKOLOV_IMPROVED_PIXEL,
    DUMMYNODE_PERCENTAGE,
    NODECOUNT_PERCENTAGE,
    NO_BOUNDARY,
    MODEL_ORDER_LEFT_TO_RIGHT,
    MODEL_ORDER_RIGHT_TO_LEFT,
}

impl NodePromotionStrategy {
    pub const ALL: [NodePromotionStrategy; 10] = [NodePromotionStrategy::NONE, NodePromotionStrategy::NIKOLOV, NodePromotionStrategy::NIKOLOV_PIXEL, NodePromotionStrategy::NIKOLOV_IMPROVED, NodePromotionStrategy::NIKOLOV_IMPROVED_PIXEL, NodePromotionStrategy::DUMMYNODE_PERCENTAGE, NodePromotionStrategy::NODECOUNT_PERCENTAGE, NodePromotionStrategy::NO_BOUNDARY, NodePromotionStrategy::MODEL_ORDER_LEFT_TO_RIGHT, NodePromotionStrategy::MODEL_ORDER_RIGHT_TO_LEFT];

    /// Declaration order, as `ordinal`/`allCases` index in Swift.
    pub fn ordinal(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            NodePromotionStrategy::NONE => "NONE",
            NodePromotionStrategy::NIKOLOV => "NIKOLOV",
            NodePromotionStrategy::NIKOLOV_PIXEL => "NIKOLOV_PIXEL",
            NodePromotionStrategy::NIKOLOV_IMPROVED => "NIKOLOV_IMPROVED",
            NodePromotionStrategy::NIKOLOV_IMPROVED_PIXEL => "NIKOLOV_IMPROVED_PIXEL",
            NodePromotionStrategy::DUMMYNODE_PERCENTAGE => "DUMMYNODE_PERCENTAGE",
            NodePromotionStrategy::NODECOUNT_PERCENTAGE => "NODECOUNT_PERCENTAGE",
            NodePromotionStrategy::NO_BOUNDARY => "NO_BOUNDARY",
            NodePromotionStrategy::MODEL_ORDER_LEFT_TO_RIGHT => "MODEL_ORDER_LEFT_TO_RIGHT",
            NodePromotionStrategy::MODEL_ORDER_RIGHT_TO_LEFT => "MODEL_ORDER_RIGHT_TO_LEFT",
        }
    }
}

crate::enum_ordinal!(NodePromotionStrategy);
