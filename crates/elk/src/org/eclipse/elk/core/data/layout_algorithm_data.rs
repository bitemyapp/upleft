//! Port of `core/data/LayoutAlgorithmData.swift`.

use crate::org::eclipse::elk::graph::properties::graph_feature::GraphFeature;

/// A registered layout algorithm. elk-swift registers exactly one, ELK
/// Layered; its provider pool lives in
/// [`crate::org::eclipse::elk::alg::layered::layered_layout_provider`].
#[derive(Debug)]
pub struct LayoutAlgorithmData {
    pub id: &'static str,
    pub name: &'static str,
    pub supported_features: &'static [GraphFeature],
}

impl LayoutAlgorithmData {
    pub fn supports_feature(&self, feature: GraphFeature) -> bool {
        self.supported_features.contains(&feature)
    }
}
