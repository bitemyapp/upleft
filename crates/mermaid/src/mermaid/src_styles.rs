//! Port of `Mermaid/src_styles.swift` (from `original/src/styles.ts`): the
//! shared font sizes, weights, paddings and text-width estimators.

use super::src_text_metrics;

pub struct FontSizes {
    pub node_label: f64,
    pub edge_label: f64,
    pub group_header: f64,
}

pub struct FontWeights {
    pub node_label: i64,
    pub edge_label: i64,
    pub group_header: i64,
}

pub struct NodePadding {
    pub horizontal: f64,
    pub vertical: f64,
    pub diamond_extra: f64,
}

pub const FONT_SIZES: FontSizes = FontSizes { node_label: 13.0, edge_label: 11.0, group_header: 12.0 };
pub const FONT_WEIGHTS: FontWeights = FontWeights { node_label: 500, edge_label: 400, group_header: 600 };

pub const GROUP_HEADER_CONTENT_PAD: f64 = 12.0;
pub const NODE_PADDING: NodePadding = NodePadding { horizontal: 20.0, vertical: 10.0, diamond_extra: 24.0 };

/// `estimateTextWidth(_:_:_:)`.
pub fn estimate_text_width(text: &str, font_size: f64, font_weight: i64) -> f64 {
    src_text_metrics::measure_text_width(text, font_size, font_weight)
}

/// `estimateMonoTextWidth(_:_:)`: `Double(text.count) * fontSize * 0.6`.
pub fn estimate_mono_text_width(text: &str, font_size: f64) -> f64 {
    crate::swift::character_count(text) as f64 * font_size * 0.6
}
