//! Port of `Render/DiagramRenderer+Flow.swift`: flowcharts and state diagrams.

use objc2_core_foundation::{CGFloat, CGPoint};
use objc2_core_graphics::CGContext;

use super::diagram_renderer::DiagramRenderer;
use super::label_renderer::{attributes, string_size, TextAlignment};
use super::render_adapters::parse_edge_style;
use crate::cg::{self, pt, Ctx};
use crate::cross_platform::{bezier_rect, bezier_rounded_rect, bm_cg_path};
use crate::mermaid::src_layout::{PositionedEdgePayload, PositionedGroupPayload};
use crate::swift;
use crate::types::{EdgeStyle, PositionedGraph};

/// `_parseCSSLength(_:)`: "2px", "1.5", "3pt" → points.
pub fn parse_css_length(value: &str) -> Option<CGFloat> {
    let stripped = swift::replacing_occurrences(&swift::replacing_occurrences(swift::trim_whitespaces_and_newlines(value), "px", ""), "pt", "");
    swift::parse_double(&stripped)
}

fn edge_style(edge: &PositionedEdgePayload) -> EdgeStyle {
    let mut style = parse_edge_style(&edge.style, edge.has_arrow_start, edge.has_arrow_end);
    style.color = edge.inline_style.as_ref().and_then(|s| s.get("stroke").cloned());
    style.stroke_width = edge.inline_style.as_ref().and_then(|s| s.get("stroke-width")).and_then(|v| parse_css_length(v));
    style
}

impl DiagramRenderer {
    /// `_drawFlowOrState(_:in:bounds:)`.
    pub(crate) fn draw_flow_or_state(&self, positioned: &PositionedGraph, context: &CGContext, bounds: objc2_core_foundation::CGRect) {
        let Some((nodes, edges, groups)) = positioned.flowchart() else { return };
        if nodes.is_empty() {
            return;
        }

        self.with_fitted_context(
            context,
            bounds,
            swift::max(1.0, positioned.width),
            swift::max(1.0, positioned.height),
            |ctx| {
                // 1. Subgraph backgrounds
                self.draw_subgraph_backgrounds(groups, ctx);

                // 2. Draw edges (lines only)
                for edge in edges {
                    let pts: Vec<CGPoint> = edge.points.iter().map(|p| pt(p.x, p.y)).collect();
                    let style = edge_style(edge);
                    self.edge_renderer.draw_edge_path(&pts, &style, ctx, &self.theme);
                }

                // 3. Draw arrow heads
                for edge in edges {
                    let pts: Vec<CGPoint> = edge.points.iter().map(|p| pt(p.x, p.y)).collect();
                    let style = edge_style(edge);
                    self.edge_renderer.draw_arrow_heads(&pts, &style, ctx, &self.theme);
                }

                // 4. Draw node shapes
                for node in nodes {
                    let rect = cg::rect(node.x, node.y, node.width, node.height);
                    self.shape_renderer.draw_shape(&node.shape, rect, &node.inline_style, ctx, &self.theme);
                }

                // 5. Draw node labels
                for node in nodes {
                    if node.label.is_empty() {
                        continue;
                    }
                    let text_color = self.theme.node_text_color(&node.inline_style);
                    let node_font = self.config.node_label_font();
                    if swift::contains(&node.label, "\n") {
                        let rect = cg::rect(node.x, node.y, node.width, node.height);
                        let inset = cg::inset(rect, 4.0, 2.0);
                        self.label_renderer.draw_multiline_text(
                            &node.label,
                            inset,
                            ctx,
                            &text_color,
                            &node_font,
                            TextAlignment::Center,
                        );
                    } else {
                        let center = pt(node.x + node.width / 2.0, node.y + node.height / 2.0);
                        self.draw_text_in_flipped(&node.label, center, ctx, &text_color, &node_font, TextAlignment::Center);
                    }
                }

                // 6. Draw edge labels (on top of nodes so they're not occluded)
                for edge in edges {
                    if let (Some(label), Some(lp)) = (&edge.label, edge.label_position) {
                        if !label.is_empty() {
                            self.draw_edge_label_in_flipped(label, pt(lp.x, lp.y), ctx);
                        }
                    }
                }

                // 7. Subgraph labels
                self.draw_subgraph_labels_in_flipped(groups, ctx);
            },
        );
    }

    fn draw_subgraph_backgrounds(&self, groups: &[PositionedGroupPayload], context: &CGContext) {
        let ctx = Ctx(context);
        for group in groups {
            let rect = cg::rect(group.x, group.y, group.width, group.height);
            let path = bezier_rect(rect);

            // Fill background
            ctx.set_fill_color(&self.theme.subgraph_background_color().CGColor());
            ctx.add_path(&bm_cg_path(&path));
            ctx.fill_path();

            // Fill header band at top of group box
            let header_y = group.y;
            let header_rect = cg::rect(group.x, header_y, group.width, group.header_height);
            let header_path = bezier_rounded_rect(header_rect, 0.0);
            ctx.set_fill_color(&self.theme.subgraph_header_color().CGColor());
            ctx.add_path(&bm_cg_path(&header_path));
            ctx.fill_path();

            // Stroke border
            ctx.set_stroke_color(&self.theme.effective_border().CGColor());
            ctx.set_line_width(1.0);
            ctx.add_path(&bm_cg_path(&path));
            ctx.stroke_path();

            // Header bottom line
            let header_bottom = header_y + group.header_height;
            ctx.move_to(pt(group.x, header_bottom));
            ctx.add_line(pt(group.x + group.width, header_bottom));
            ctx.stroke_path();

            // Recurse into children
            self.draw_subgraph_backgrounds(&group.children, context);
        }
    }

    fn draw_subgraph_labels_in_flipped(&self, groups: &[PositionedGroupPayload], context: &CGContext) {
        let header_font = self.config.group_header_font();
        for group in groups {
            let label_point = pt(group.x + 8.0, group.y + group.header_height / 2.0);
            self.draw_text_in_flipped(
                &group.label,
                label_point,
                context,
                &self.theme.effective_text_secondary(),
                &header_font,
                TextAlignment::Left,
            );
            self.draw_subgraph_labels_in_flipped(&group.children, context);
        }
    }

    /// `_drawEdgeLabelInFlipped(_:at:in:contentHeight:)`.
    pub(crate) fn draw_edge_label_in_flipped(&self, label: &str, position: CGPoint, context: &CGContext) {
        let ctx = Ctx(context);
        let config = &self.config;
        let edge_font = config.edge_label_font();
        let size = string_size(label, &attributes(&edge_font, None));

        let padding = config.edge_label_padding;
        let pill_rect = cg::rect(
            position.x - size.width / 2.0 - padding,
            position.y - size.height / 2.0 - padding / 2.0,
            size.width + padding * 2.0,
            size.height + padding,
        );

        let pill_path = bezier_rounded_rect(pill_rect, config.edge_label_corner_radius);
        ctx.set_fill_color(&self.theme.background.CGColor());
        ctx.add_path(&bm_cg_path(&pill_path));
        ctx.fill_path();

        ctx.set_stroke_color(&self.theme.effective_inner_stroke().CGColor());
        ctx.set_line_width(config.edge_label_border_width);
        ctx.add_path(&bm_cg_path(&pill_path));
        ctx.stroke_path();

        self.draw_text_in_flipped(
            label,
            position,
            context,
            &self.theme.effective_text_secondary(),
            &edge_font,
            TextAlignment::Center,
        );
    }
}
