//! Port of `Render/EdgeRenderer.swift`.

use objc2_app_kit::NSColor;
use objc2_core_foundation::{CGFloat, CGPoint};
use objc2_core_graphics::{CGContext, CGLineCap, CGLineJoin, CGPathDrawingMode};

use super::render_config::RenderConfig;
use crate::cg::{pt, rect, Ctx, Path};
use crate::theme::DiagramTheme;
use crate::types::{ArrowHead, EdgeStyle, LineStyle};

impl LineStyle {
    /// `dashPattern`.
    pub fn dash_pattern(self) -> Option<&'static [CGFloat]> {
        match self {
            LineStyle::Solid | LineStyle::Thick => None,
            LineStyle::Dotted => Some(&[2.0, 4.0]),
            LineStyle::Dashed => Some(&[8.0, 4.0]),
        }
    }

    /// `widthMultiplier`.
    pub fn width_multiplier(self) -> CGFloat {
        if self == LineStyle::Thick { 2.0 } else { 1.0 }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EdgeRenderer {
    pub config: RenderConfig,
}

impl EdgeRenderer {
    pub fn new(config: RenderConfig) -> EdgeRenderer {
        EdgeRenderer { config }
    }

    /// `drawEdgePath(points:style:in:theme:)`.
    pub fn draw_edge_path(&self, points: &[CGPoint], style: &EdgeStyle, context: &CGContext, theme: &DiagramTheme) {
        if points.len() < 2 {
            return;
        }
        let ctx = Ctx(context);

        let color = theme.edge_color(style);
        let base_line_width = self.config.stroke_width_connector;
        let line_width = style.stroke_width.unwrap_or(base_line_width * style.line_style.width_multiplier());

        ctx.save_g_state();
        ctx.set_stroke_color(&color.CGColor());
        ctx.set_line_width(line_width);
        ctx.set_line_cap(CGLineCap::Round);
        ctx.set_line_join(CGLineJoin::Round);

        if let Some(pattern) = style.line_style.dash_pattern() {
            ctx.set_line_dash(0.0, pattern);
        }

        ctx.move_to(points[0]);
        for p in &points[1..] {
            ctx.add_line(*p);
        }
        ctx.stroke_path();
        ctx.restore_g_state();
    }

    /// `drawArrowHeads(points:style:in:theme:)`.
    pub fn draw_arrow_heads(&self, points: &[CGPoint], style: &EdgeStyle, context: &CGContext, theme: &DiagramTheme) {
        if points.len() < 2 {
            return;
        }

        let arrow_color = if style.color.is_some() { theme.edge_color(style) } else { theme.effective_arrow() };
        let base_line_width = self.config.stroke_width_connector;
        let line_width = style.stroke_width.unwrap_or(base_line_width * style.line_style.width_multiplier());

        if style.target_arrow != ArrowHead::None {
            let p0 = points[points.len() - 2];
            let p1 = points[points.len() - 1];
            let angle = (p1.y - p0.y).atan2(p1.x - p0.x);
            self.draw_arrow_head(style.target_arrow, p1, angle, line_width, &arrow_color, context);
        }

        if style.source_arrow != ArrowHead::None {
            let p0 = points[1];
            let p1 = points[0];
            let angle = (p1.y - p0.y).atan2(p1.x - p0.x);
            self.draw_arrow_head(style.source_arrow, p1, angle, line_width, &arrow_color, context);
        }
    }

    fn draw_arrow_head(
        &self,
        style: ArrowHead,
        point: CGPoint,
        angle: CGFloat,
        line_width: CGFloat,
        color: &NSColor,
        context: &CGContext,
    ) {
        let ctx = Ctx(context);
        let arrow_width = self.config.arrow_head_width;
        let arrow_height = self.config.arrow_head_height;

        ctx.save_g_state();
        ctx.translate_by(point.x, point.y);
        ctx.rotate(angle);
        let cg_color = color.CGColor();
        ctx.set_fill_color(&cg_color);
        ctx.set_stroke_color(&cg_color);
        ctx.set_line_width(line_width);

        match style {
            ArrowHead::None => {}
            ArrowHead::Arrow => {
                let path = Path::new();
                path.move_to(pt(0.0, 0.0));
                path.add_line(pt(-arrow_width, -arrow_height / 2.0));
                path.add_line(pt(-arrow_width, arrow_height / 2.0));
                path.close_subpath();
                ctx.set_line_join(CGLineJoin::Round);
                ctx.set_line_width(0.75);
                ctx.add_path(path.as_path());
                ctx.draw_path(CGPathDrawingMode::FillStroke);
            }
            ArrowHead::Open => {
                ctx.move_to(pt(-arrow_width, -arrow_height / 2.0));
                ctx.add_line(pt(0.0, 0.0));
                ctx.add_line(pt(-arrow_width, arrow_height / 2.0));
                ctx.stroke_path();
            }
            ArrowHead::Circle => {
                let circle_size = arrow_height * 0.8;
                ctx.add_ellipse(rect(-circle_size - line_width, -circle_size / 2.0, circle_size, circle_size));
                ctx.fill_path();
            }
            ArrowHead::Cross => {
                let cross_size = arrow_height * 0.4;
                ctx.move_to(pt(-cross_size * 2.0 - line_width, -cross_size));
                ctx.add_line(pt(-line_width, cross_size));
                ctx.move_to(pt(-cross_size * 2.0 - line_width, cross_size));
                ctx.add_line(pt(-line_width, -cross_size));
                ctx.stroke_path();
            }
            ArrowHead::Diamond => {
                let diamond_width = arrow_width * 1.2;
                let diamond_height = arrow_height;
                let path = Path::new();
                path.move_to(pt(0.0, 0.0));
                path.add_line(pt(-diamond_width / 2.0, -diamond_height / 2.0));
                path.add_line(pt(-diamond_width, 0.0));
                path.add_line(pt(-diamond_width / 2.0, diamond_height / 2.0));
                path.close_subpath();
                ctx.add_path(path.as_path());
                ctx.fill_path();
            }
        }

        ctx.restore_g_state();
    }
}
