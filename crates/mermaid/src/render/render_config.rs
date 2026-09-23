//! Port of `Render/RenderConfig.swift`.

use objc2::rc::Retained;
use objc2_app_kit::{
    NSFont, NSFontWeight, NSFontWeightBlack, NSFontWeightBold, NSFontWeightHeavy, NSFontWeightLight,
    NSFontWeightMedium, NSFontWeightRegular, NSFontWeightSemibold, NSFontWeightThin, NSFontWeightUltraLight,
};
use objc2_core_foundation::CGFloat;

use crate::mermaid::src_text_metrics;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderConfig {
    pub node_padding_horizontal: CGFloat,
    pub node_padding_vertical: CGFloat,
    pub node_padding_diamond_extra: CGFloat,

    pub font_size_node_label: CGFloat,
    pub font_size_edge_label: CGFloat,
    pub font_size_group_header: CGFloat,

    pub font_weight_node_label: i64,
    pub font_weight_edge_label: i64,
    pub font_weight_group_header: i64,

    pub stroke_width_outer_box: CGFloat,
    pub stroke_width_inner_box: CGFloat,
    pub stroke_width_connector: CGFloat,

    pub arrow_head_width: CGFloat,
    pub arrow_head_height: CGFloat,

    pub group_header_content_pad: CGFloat,
    pub subgraph_padding: CGFloat,
    pub node_spacing: CGFloat,
    pub layer_spacing: CGFloat,
    pub graph_padding: CGFloat,

    pub text_baseline_shift_em: CGFloat,

    pub minimum_node_width: CGFloat,
    pub minimum_node_height: CGFloat,
    pub state_pseudostate_size: CGFloat,

    pub cylinder_ellipse_radius: CGFloat,
    pub subroutine_inset: CGFloat,
    pub asymmetric_indent: CGFloat,
    pub double_circle_gap: CGFloat,

    pub edge_label_padding: CGFloat,
    pub edge_label_corner_radius: CGFloat,
    pub edge_label_border_width: CGFloat,

    pub sequence_loop_h: CGFloat,
    pub sequence_tab_height: CGFloat,
    pub sequence_fold_size: CGFloat,

    pub class_padding: CGFloat,
    pub class_box_pad_x: CGFloat,
    pub class_header_base_height: CGFloat,
    pub class_annotation_height: CGFloat,
    pub class_member_row_height: CGFloat,
    pub class_section_pad_y: CGFloat,
    pub class_empty_section_height: CGFloat,
    pub class_min_width: CGFloat,
    pub class_member_font_size: CGFloat,
    pub class_member_font_weight: i64,
    pub class_node_spacing: CGFloat,
    pub class_layer_spacing: CGFloat,

    pub er_padding: CGFloat,
    pub er_box_pad_x: CGFloat,
    pub er_header_height: CGFloat,
    pub er_row_height: CGFloat,
    pub er_min_width: CGFloat,
    pub er_attr_font_size: CGFloat,
}

impl Default for RenderConfig {
    fn default() -> Self {
        RenderConfig::SHARED
    }
}

impl RenderConfig {
    /// `RenderConfig.shared`.
    pub const SHARED: RenderConfig = RenderConfig {
        node_padding_horizontal: 20.0,
        node_padding_vertical: 10.0,
        node_padding_diamond_extra: 24.0,
        font_size_node_label: 13.0,
        font_size_edge_label: 11.0,
        font_size_group_header: 12.0,
        font_weight_node_label: 500,
        font_weight_edge_label: 400,
        font_weight_group_header: 600,
        stroke_width_outer_box: 1.0,
        stroke_width_inner_box: 0.75,
        stroke_width_connector: 1.0,
        arrow_head_width: 8.0,
        arrow_head_height: 5.0,
        group_header_content_pad: 12.0,
        subgraph_padding: 24.0,
        node_spacing: 28.0,
        layer_spacing: 48.0,
        graph_padding: 40.0,
        text_baseline_shift_em: 0.35,
        minimum_node_width: 60.0,
        minimum_node_height: 36.0,
        state_pseudostate_size: 28.0,
        cylinder_ellipse_radius: 7.0,
        subroutine_inset: 8.0,
        asymmetric_indent: 12.0,
        double_circle_gap: 5.0,
        edge_label_padding: 8.0,
        edge_label_corner_radius: 2.0,
        edge_label_border_width: 1.0,
        sequence_loop_h: 20.0,
        sequence_tab_height: 18.0,
        sequence_fold_size: 6.0,
        class_padding: 40.0,
        class_box_pad_x: 8.0,
        class_header_base_height: 32.0,
        class_annotation_height: 16.0,
        class_member_row_height: 20.0,
        class_section_pad_y: 8.0,
        class_empty_section_height: 8.0,
        class_min_width: 120.0,
        class_member_font_size: 11.0,
        class_member_font_weight: 400,
        class_node_spacing: 40.0,
        class_layer_spacing: 60.0,
        er_padding: 40.0,
        er_box_pad_x: 14.0,
        er_header_height: 34.0,
        er_row_height: 22.0,
        er_min_width: 140.0,
        er_attr_font_size: 11.0,
    };

    /// `fontWeight(from:)`.
    pub fn font_weight(weight: i64) -> NSFontWeight {
        unsafe {
            match weight {
                100 => NSFontWeightUltraLight,
                200 => NSFontWeightThin,
                300 => NSFontWeightLight,
                400 => NSFontWeightRegular,
                500 => NSFontWeightMedium,
                600 => NSFontWeightSemibold,
                700 => NSFontWeightBold,
                800 => NSFontWeightHeavy,
                900 => NSFontWeightBlack,
                _ => NSFontWeightRegular,
            }
        }
    }

    /// `nodeLabelFont(family: nil)`.
    pub fn node_label_font(&self) -> Retained<NSFont> {
        NSFont::systemFontOfSize_weight(self.font_size_node_label, Self::font_weight(self.font_weight_node_label))
    }

    /// `edgeLabelFont(family: nil)`.
    pub fn edge_label_font(&self) -> Retained<NSFont> {
        NSFont::systemFontOfSize_weight(self.font_size_edge_label, Self::font_weight(self.font_weight_edge_label))
    }

    /// `groupHeaderFont(family: nil)`.
    pub fn group_header_font(&self) -> Retained<NSFont> {
        NSFont::systemFontOfSize_weight(self.font_size_group_header, Self::font_weight(self.font_weight_group_header))
    }

    pub fn estimate_text_width(&self, text: &str, font_size: CGFloat, font_weight: i64) -> CGFloat {
        src_text_metrics::measure_text_width(text, font_size, font_weight)
    }

    pub fn estimate_mono_text_width(&self, text: &str, font_size: CGFloat) -> CGFloat {
        crate::swift::character_count(text) as f64 * font_size * 0.6
    }
}
