//! Port of `Render/ShapeRenderer.swift` (`NodeShapeRenderer`).

use objc2_core_foundation::{CFRetained, CGRect};
use objc2_core_graphics::{CGContext, CGPath};

use super::render_config::RenderConfig;
use crate::cg::{self, pt, Ctx, Path};
use crate::mermaid::src_types::SDict;
use crate::swift;
use crate::theme::DiagramTheme;

#[derive(Debug, Clone, Copy)]
pub struct NodeShapeRenderer {
    pub config: RenderConfig,
}

fn center(bounds: CGRect) -> objc2_core_foundation::CGPoint {
    pt(cg::mid_x(bounds), cg::mid_y(bounds))
}

impl NodeShapeRenderer {
    pub fn new(config: RenderConfig) -> NodeShapeRenderer {
        NodeShapeRenderer { config }
    }

    /// `drawShape(_:bounds:inlineStyles:in:theme:)`.
    pub fn draw_shape(&self, shape: &str, bounds: CGRect, inline_styles: &SDict<String>, context: &CGContext, theme: &DiagramTheme) {
        let ctx = Ctx(context);
        ctx.save_g_state();

        if shape == "state-start" {
            let path = self.true_circle_path(bounds);
            ctx.set_fill_color(&theme.foreground.CGColor());
            ctx.add_path(&path);
            ctx.fill_path();
            ctx.restore_g_state();
            return;
        }

        if shape == "state-end" {
            let (cx, cy) = (cg::mid_x(bounds), cg::mid_y(bounds));
            let outer_r = swift::min(cg::width(bounds), cg::height(bounds)) / 2.0 - 2.0;
            let outer_rect = cg::rect(cx - outer_r, cy - outer_r, outer_r * 2.0, outer_r * 2.0);
            ctx.set_stroke_color(&theme.foreground.CGColor());
            ctx.set_line_width(self.config.stroke_width_inner_box * 2.0);
            ctx.stroke_ellipse(outer_rect);
            let inner_r = outer_r - 4.0;
            let inner_rect = cg::rect(cx - inner_r, cy - inner_r, inner_r * 2.0, inner_r * 2.0);
            ctx.set_fill_color(&theme.foreground.CGColor());
            ctx.fill_ellipse(inner_rect);
            ctx.restore_g_state();
            return;
        }

        let fill_color = theme.node_fill_color(inline_styles);
        let stroke_color = theme.node_stroke_color(inline_styles);
        let path = self.shape_path(shape, bounds);

        ctx.set_fill_color(&fill_color.CGColor());
        ctx.add_path(&path);
        ctx.fill_path();

        ctx.set_stroke_color(&stroke_color.CGColor());
        ctx.set_line_width(self.config.stroke_width_inner_box);
        ctx.add_path(&path);
        ctx.stroke_path();

        self.draw_shape_details(shape, bounds, context, theme, inline_styles);

        ctx.restore_g_state();
    }

    /// `shapePath(for:in:)`.
    pub fn shape_path(&self, shape: &str, bounds: CGRect) -> CFRetained<CGPath> {
        match shape {
            "rectangle" | "entity" | "invisible" => cg::path_rect(bounds),
            "rounded" | "state-note" => self.rounded_rect_path(bounds, 6.0),
            "stadium" => self.rounded_rect_path(bounds, cg::height(bounds) / 2.0),
            "circle" | "doublecircle" | "state-choice" => cg::path_ellipse(bounds),
            "state-start" | "state-end" => self.true_circle_path(bounds),
            "diamond" | "rhombus" => self.diamond_path(bounds),
            "hexagon" => self.hexagon_path(bounds),
            "parallelogram" => self.parallelogram_path(bounds),
            "parallelogram-alt" => self.parallelogram_alt_path(bounds),
            "trapezoid" => self.trapezoid_path(bounds),
            "trapezoid-alt" => self.trapezoid_alt_path(bounds),
            "cylinder" => {
                let ry = self.config.cylinder_ellipse_radius;
                let body_rect = cg::rect(cg::min_x(bounds), cg::min_y(bounds) + ry, cg::width(bounds), cg::height(bounds) - 2.0 * ry);
                cg::path_rect(body_rect)
            }
            "subroutine" => cg::path_rect(bounds),
            "asymmetric" => self.asymmetric_path(bounds),
            "state-fork" => cg::path_rect(bounds),
            "class-box" => self.rounded_rect_path(bounds, 4.0),
            _ => cg::path_rect(bounds),
        }
    }

    fn rounded_rect_path(&self, bounds: CGRect, corner_radius: f64) -> CFRetained<CGPath> {
        let path = Path::new();
        path.add_rounded_rect(bounds, corner_radius, corner_radius);
        into_path(path)
    }

    fn true_circle_path(&self, bounds: CGRect) -> CFRetained<CGPath> {
        let (cx, cy) = (cg::mid_x(bounds), cg::mid_y(bounds));
        let r = swift::min(cg::width(bounds), cg::height(bounds)) / 2.0 - 2.0;
        cg::path_ellipse(cg::rect(cx - r, cy - r, r * 2.0, r * 2.0))
    }

    fn diamond_path(&self, bounds: CGRect) -> CFRetained<CGPath> {
        let path = Path::new();
        let c = center(bounds);
        path.move_to(pt(c.x, cg::min_y(bounds)));
        path.add_line(pt(cg::max_x(bounds), c.y));
        path.add_line(pt(c.x, cg::max_y(bounds)));
        path.add_line(pt(cg::min_x(bounds), c.y));
        path.close_subpath();
        into_path(path)
    }

    fn hexagon_path(&self, bounds: CGRect) -> CFRetained<CGPath> {
        let path = Path::new();
        let inset = cg::height(bounds) / 4.0;
        let c = center(bounds);
        path.move_to(pt(cg::min_x(bounds) + inset, cg::min_y(bounds)));
        path.add_line(pt(cg::max_x(bounds) - inset, cg::min_y(bounds)));
        path.add_line(pt(cg::max_x(bounds), c.y));
        path.add_line(pt(cg::max_x(bounds) - inset, cg::max_y(bounds)));
        path.add_line(pt(cg::min_x(bounds) + inset, cg::max_y(bounds)));
        path.add_line(pt(cg::min_x(bounds), c.y));
        path.close_subpath();
        into_path(path)
    }

    fn parallelogram_path(&self, bounds: CGRect) -> CFRetained<CGPath> {
        let path = Path::new();
        let skew = cg::width(bounds) * 0.2;
        path.move_to(pt(cg::min_x(bounds) + skew, cg::min_y(bounds)));
        path.add_line(pt(cg::max_x(bounds), cg::min_y(bounds)));
        path.add_line(pt(cg::max_x(bounds) - skew, cg::max_y(bounds)));
        path.add_line(pt(cg::min_x(bounds), cg::max_y(bounds)));
        path.close_subpath();
        into_path(path)
    }

    fn parallelogram_alt_path(&self, bounds: CGRect) -> CFRetained<CGPath> {
        let path = Path::new();
        let skew = cg::width(bounds) * 0.2;
        path.move_to(pt(cg::min_x(bounds), cg::min_y(bounds)));
        path.add_line(pt(cg::max_x(bounds) - skew, cg::min_y(bounds)));
        path.add_line(pt(cg::max_x(bounds), cg::max_y(bounds)));
        path.add_line(pt(cg::min_x(bounds) + skew, cg::max_y(bounds)));
        path.close_subpath();
        into_path(path)
    }

    fn trapezoid_path(&self, bounds: CGRect) -> CFRetained<CGPath> {
        let path = Path::new();
        let inset = cg::width(bounds) * 0.15;
        path.move_to(pt(cg::min_x(bounds) + inset, cg::min_y(bounds)));
        path.add_line(pt(cg::max_x(bounds) - inset, cg::min_y(bounds)));
        path.add_line(pt(cg::max_x(bounds), cg::max_y(bounds)));
        path.add_line(pt(cg::min_x(bounds), cg::max_y(bounds)));
        path.close_subpath();
        into_path(path)
    }

    fn trapezoid_alt_path(&self, bounds: CGRect) -> CFRetained<CGPath> {
        let path = Path::new();
        let inset = cg::width(bounds) * 0.15;
        path.move_to(pt(cg::min_x(bounds), cg::min_y(bounds)));
        path.add_line(pt(cg::max_x(bounds), cg::min_y(bounds)));
        path.add_line(pt(cg::max_x(bounds) - inset, cg::max_y(bounds)));
        path.add_line(pt(cg::min_x(bounds) + inset, cg::max_y(bounds)));
        path.close_subpath();
        into_path(path)
    }

    fn asymmetric_path(&self, bounds: CGRect) -> CFRetained<CGPath> {
        let path = Path::new();
        let indent = self.config.asymmetric_indent;
        let c = center(bounds);
        path.move_to(pt(cg::min_x(bounds) + indent, cg::min_y(bounds)));
        path.add_line(pt(cg::max_x(bounds), cg::min_y(bounds)));
        path.add_line(pt(cg::max_x(bounds), cg::max_y(bounds)));
        path.add_line(pt(cg::min_x(bounds) + indent, cg::max_y(bounds)));
        path.add_line(pt(cg::min_x(bounds), c.y));
        path.close_subpath();
        into_path(path)
    }

    fn draw_shape_details(&self, shape: &str, bounds: CGRect, context: &CGContext, theme: &DiagramTheme, inline_styles: &SDict<String>) {
        let ctx = Ctx(context);
        let config = &self.config;

        match shape {
            "subroutine" => {
                let inset = config.subroutine_inset;
                ctx.set_stroke_color(&theme.node_stroke_color(inline_styles).CGColor());
                ctx.set_line_width(config.stroke_width_inner_box);
                ctx.move_to(pt(cg::min_x(bounds) + inset, cg::min_y(bounds)));
                ctx.add_line(pt(cg::min_x(bounds) + inset, cg::max_y(bounds)));
                ctx.stroke_path();
                ctx.move_to(pt(cg::max_x(bounds) - inset, cg::min_y(bounds)));
                ctx.add_line(pt(cg::max_x(bounds) - inset, cg::max_y(bounds)));
                ctx.stroke_path();
            }
            "doublecircle" => {
                let inner_bounds = cg::inset(bounds, config.double_circle_gap, config.double_circle_gap);
                ctx.set_stroke_color(&theme.node_stroke_color(inline_styles).CGColor());
                ctx.set_line_width(config.stroke_width_inner_box);
                ctx.add_path(&cg::path_ellipse(inner_bounds));
                ctx.stroke_path();
            }
            "cylinder" => {
                let ry = config.cylinder_ellipse_radius;
                let ellipse_height = ry * 2.0;
                let body_top = cg::min_y(bounds) + ry;
                let body_bottom = cg::max_y(bounds) - ry;

                let stroke_color = theme.node_stroke_color(inline_styles);
                let fill_color = theme.node_fill_color(inline_styles);

                ctx.set_stroke_color(&stroke_color.CGColor());
                ctx.set_line_width(config.stroke_width_inner_box);
                ctx.move_to(pt(cg::min_x(bounds), body_top));
                ctx.add_line(pt(cg::min_x(bounds), body_bottom));
                ctx.stroke_path();
                ctx.move_to(pt(cg::max_x(bounds), body_top));
                ctx.add_line(pt(cg::max_x(bounds), body_bottom));
                ctx.stroke_path();

                let bottom_ellipse = cg::rect(cg::min_x(bounds), cg::max_y(bounds) - ellipse_height, cg::width(bounds), ellipse_height);
                let bottom_path = cg::path_ellipse(bottom_ellipse);
                ctx.set_fill_color(&fill_color.CGColor());
                ctx.add_path(&bottom_path);
                ctx.fill_path();
                ctx.set_stroke_color(&stroke_color.CGColor());
                ctx.add_path(&bottom_path);
                ctx.stroke_path();

                let top_ellipse = cg::rect(cg::min_x(bounds), cg::min_y(bounds), cg::width(bounds), ellipse_height);
                let top_path = cg::path_ellipse(top_ellipse);
                ctx.set_fill_color(&fill_color.CGColor());
                ctx.add_path(&top_path);
                ctx.fill_path();
                ctx.set_stroke_color(&stroke_color.CGColor());
                ctx.add_path(&top_path);
                ctx.stroke_path();
            }
            _ => {}
        }
    }
}

/// A finished `CGMutablePath` used where the Swift returns `CGPath`.
fn into_path(path: Path) -> CFRetained<CGPath> {
    let mutable = path.0;
    // CGMutablePath is a CGPath; retain it as one.
    unsafe { CFRetained::cast_unchecked(mutable) }
}
