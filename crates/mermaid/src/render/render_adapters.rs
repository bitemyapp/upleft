//! Port of `Render/RenderAdapters.swift`.

use crate::types::{ArrowHead, EdgeStyle, LineStyle};

/// `EdgeStyleParser.parse(from:hasArrowStart:hasArrowEnd:)`.
pub fn parse_edge_style(style_string: &str, has_arrow_start: bool, has_arrow_end: bool) -> EdgeStyle {
    let source_arrow = if has_arrow_start { ArrowHead::Arrow } else { ArrowHead::None };
    let target_arrow = if has_arrow_end { ArrowHead::Arrow } else { ArrowHead::None };

    let line_style = match crate::swift::lowercased(style_string).as_str() {
        "dotted" => LineStyle::Dotted,
        "dashed" => LineStyle::Dashed,
        "thick" => LineStyle::Thick,
        _ => LineStyle::Solid,
    };

    EdgeStyle { line_style, source_arrow, target_arrow, color: None, stroke_width: None }
}

/// `RenderRelType` raw values.
pub mod render_rel_type {
    pub const INHERITANCE: &str = "inheritance";
    pub const COMPOSITION: &str = "composition";
    pub const AGGREGATION: &str = "aggregation";
    pub const ASSOCIATION: &str = "association";
    pub const DEPENDENCY: &str = "dependency";
    pub const REALIZATION: &str = "realization";
}
