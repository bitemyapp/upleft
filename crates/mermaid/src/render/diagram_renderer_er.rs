//! Port of `Render/DiagramRenderer+ER.swift`.

use objc2_app_kit::{NSFont, NSFontWeightBold, NSFontWeightSemibold};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect};
use objc2_core_graphics::CGContext;

use super::diagram_renderer::DiagramRenderer;
use super::label_renderer::TextAlignment;
use crate::cg::{self, pt, Ctx};
use crate::cross_platform::{bezier_rounded_rect, bm_cg_path};
use crate::swift;
use crate::types::{PositionedContent, PositionedGraph};

impl DiagramRenderer {
    /// `_drawEr(_:in:bounds:)`.
    pub(crate) fn draw_er(&self, positioned: &PositionedGraph, context: &CGContext, bounds: CGRect) {
        let PositionedContent::ErDiagram { entities, relationships } = &positioned.content else { return };
        if entities.is_empty() {
            return;
        }

        self.with_fitted_context(
            context,
            bounds,
            swift::max(1.0, positioned.width),
            swift::max(1.0, positioned.height),
            |context| {
                let ctx = Ctx(context);
                let config = &self.config;

                // Relationship lines
                for rel in relationships {
                    let pts: Vec<CGPoint> = rel.points.iter().map(|p| pt(p.x, p.y)).collect();
                    if pts.len() < 2 {
                        continue;
                    }
                    ctx.save_g_state();
                    ctx.set_stroke_color(&self.theme.effective_line().CGColor());
                    ctx.set_line_width(config.stroke_width_connector);
                    if !rel.identifying {
                        ctx.set_line_dash(0.0, &[6.0, 4.0]);
                    }
                    ctx.move_to(pts[0]);
                    for p in &pts[1..] {
                        ctx.add_line(*p);
                    }
                    ctx.stroke_path();
                    ctx.restore_g_state();
                }

                // Entity boxes
                for entity in entities {
                    let bx = cg::rect(entity.x, entity.y, entity.width, entity.height);
                    ctx.set_fill_color(&self.theme.effective_surface().CGColor());
                    ctx.fill(bx);
                    ctx.set_stroke_color(&self.theme.effective_border().CGColor());
                    ctx.set_line_width(config.stroke_width_outer_box);
                    ctx.stroke(bx);

                    let header_rect = cg::rect(entity.x, entity.y, entity.width, entity.header_height);
                    ctx.set_fill_color(&self.theme.subgraph_header_color().CGColor());
                    ctx.fill(header_rect);
                    ctx.set_stroke_color(&self.theme.effective_border().CGColor());
                    ctx.stroke(header_rect);

                    let name_font = NSFont::systemFontOfSize_weight(config.font_size_node_label, unsafe { NSFontWeightBold });
                    self.draw_text_in_flipped(
                        &entity.label,
                        pt(entity.x + entity.width / 2.0, entity.y + entity.header_height / 2.0),
                        context,
                        &self.theme.foreground,
                        &name_font,
                        TextAlignment::Center,
                    );

                    let attr_top = entity.y + entity.header_height;
                    ctx.set_stroke_color(&self.theme.effective_border().CGColor());
                    ctx.set_line_width(config.stroke_width_inner_box);
                    ctx.move_to(pt(entity.x, attr_top));
                    ctx.add_line(pt(entity.x + entity.width, attr_top));
                    ctx.stroke_path();

                    let mono_font = self.mono_font(config.er_attr_font_size);
                    if entity.attributes.is_empty() {
                        // Empty attribute placeholder
                        let italic_font = self.italic_system_font(config.er_attr_font_size, 0.0);
                        self.draw_text_in_flipped(
                            "(no attributes)",
                            pt(entity.x + entity.width / 2.0, attr_top + entity.row_height / 2.0),
                            context,
                            &self.theme.effective_text_faint(),
                            &italic_font,
                            TextAlignment::Center,
                        );
                    }
                    for (i, attr) in entity.attributes.iter().enumerate() {
                        let row_y = attr_top + i as f64 * entity.row_height + entity.row_height / 2.0;

                        // Key badges
                        if !attr.keys.is_empty() {
                            let key_text = attr.keys.join(",");
                            let key_width = config.estimate_text_width(&key_text, 9.0, 600) + 8.0;
                            let badge_rect = cg::rect(entity.x + 6.0, row_y - 7.0, key_width, 14.0);
                            let badge_path = bezier_rounded_rect(badge_rect, 2.0);
                            ctx.set_fill_color(&self.theme.key_badge_color().CGColor());
                            ctx.add_path(&bm_cg_path(&badge_path));
                            ctx.fill_path();

                            let key_font = NSFont::systemFontOfSize_weight(9.0, unsafe { NSFontWeightSemibold });
                            self.draw_text_in_flipped(
                                &key_text,
                                pt(entity.x + 6.0 + key_width / 2.0, row_y),
                                context,
                                &self.theme.effective_text_secondary(),
                                &key_font,
                                TextAlignment::Center,
                            );
                        }

                        // Type (left)
                        let type_x = entity.x
                            + 8.0
                            + if attr.keys.is_empty() {
                                0.0
                            } else {
                                config.estimate_text_width(&attr.keys.join(","), 9.0, 600) + 14.0
                            };
                        self.draw_text_in_flipped(&attr.r#type, pt(type_x, row_y), context, &self.theme.effective_muted(), &mono_font, TextAlignment::Left);

                        // Name (right)
                        self.draw_text_in_flipped(
                            &attr.name,
                            pt(entity.x + entity.width - 8.0, row_y),
                            context,
                            &self.theme.effective_text_secondary(),
                            &mono_font,
                            TextAlignment::Right,
                        );
                    }
                }

                // Cardinality markers
                for rel in relationships {
                    let pts: Vec<CGPoint> = rel.points.iter().map(|p| pt(p.x, p.y)).collect();
                    if pts.len() < 2 {
                        continue;
                    }
                    self.draw_crows_foot(pts[0], pts[1], &rel.cardinality1, context);
                    self.draw_crows_foot(pts[pts.len() - 1], pts[pts.len() - 2], &rel.cardinality2, context);
                }

                // Relationship labels with background + border
                for rel in relationships {
                    if rel.label.is_empty() {
                        continue;
                    }
                    let pts: Vec<CGPoint> = rel.points.iter().map(|p| pt(p.x, p.y)).collect();
                    let mid = arc_length_midpoint(&pts);
                    let label_font = config.edge_label_font();
                    let text_w = config.estimate_text_width(&rel.label, config.font_size_edge_label, 400) + 8.0;
                    let text_h = config.font_size_edge_label + 6.0;
                    let bg_rect = cg::rect(mid.x - text_w / 2.0, mid.y - text_h / 2.0, text_w, text_h);
                    let bg_path = bezier_rounded_rect(bg_rect, 2.0);
                    ctx.set_fill_color(&self.theme.background.CGColor());
                    ctx.add_path(&bm_cg_path(&bg_path));
                    ctx.fill_path();
                    ctx.set_stroke_color(&self.theme.effective_inner_stroke().CGColor());
                    ctx.set_line_width(0.5);
                    ctx.add_path(&bm_cg_path(&bg_path));
                    ctx.stroke_path();
                    self.draw_text_in_flipped(&rel.label, mid, context, &self.theme.effective_muted(), &label_font, TextAlignment::Center);
                }
            },
        );
    }

    fn draw_crows_foot(&self, point: CGPoint, toward: CGPoint, cardinality: &str, context: &CGContext) {
        let ctx = Ctx(context);
        let sw = self.config.stroke_width_connector + 0.25;
        let (dx, dy) = (point.x - toward.x, point.y - toward.y);
        let len = (dx * dx + dy * dy).sqrt();
        if !(len > 0.0) {
            return;
        }
        let (ux, uy) = (dx / len, dy / len);
        let (px, py) = (-uy, ux);

        let (tip_x, tip_y) = (point.x - ux * 4.0, point.y - uy * 4.0);

        let has_one_line = cardinality == "one" || cardinality == "zero-one";
        let has_crows_foot = cardinality == "many" || cardinality == "zero-many";
        let has_circle = cardinality == "zero-one" || cardinality == "zero-many";

        ctx.save_g_state();
        ctx.set_stroke_color(&self.theme.effective_line().CGColor());
        ctx.set_line_width(sw);

        if has_one_line {
            let half_w: CGFloat = 6.0;
            ctx.move_to(pt(tip_x + px * half_w, tip_y + py * half_w));
            ctx.add_line(pt(tip_x - px * half_w, tip_y - py * half_w));
            ctx.stroke_path();
            let (line2_x, line2_y) = (tip_x - ux * 4.0, tip_y - uy * 4.0);
            ctx.move_to(pt(line2_x + px * half_w, line2_y + py * half_w));
            ctx.add_line(pt(line2_x - px * half_w, line2_y - py * half_w));
            ctx.stroke_path();
        }

        if has_crows_foot {
            let fan_w: CGFloat = 7.0;
            let (back_x, back_y) = (point.x - ux * 16.0, point.y - uy * 16.0);
            ctx.move_to(pt(tip_x + px * fan_w, tip_y + py * fan_w));
            ctx.add_line(pt(back_x, back_y));
            ctx.stroke_path();
            ctx.move_to(pt(tip_x, tip_y));
            ctx.add_line(pt(back_x, back_y));
            ctx.stroke_path();
            ctx.move_to(pt(tip_x - px * fan_w, tip_y - py * fan_w));
            ctx.add_line(pt(back_x, back_y));
            ctx.stroke_path();
        }

        if has_circle {
            let circle_offset: CGFloat = if has_crows_foot { 20.0 } else { 12.0 };
            let (cx, cy) = (point.x - ux * circle_offset, point.y - uy * circle_offset);
            let circle_rect = cg::rect(cx - 4.0, cy - 4.0, 8.0, 8.0);
            ctx.set_fill_color(&self.theme.background.CGColor());
            ctx.fill_ellipse(circle_rect);
            ctx.stroke_ellipse(circle_rect);
        }

        ctx.restore_g_state();
    }
}

/// `_arcLengthMidpoint(_:)`.
pub(crate) fn arc_length_midpoint(points: &[CGPoint]) -> CGPoint {
    if points.len() <= 1 {
        return points.first().copied().unwrap_or(pt(0.0, 0.0));
    }
    let mut total_len: CGFloat = 0.0;
    for i in 1..points.len() {
        let (dx, dy) = (points[i].x - points[i - 1].x, points[i].y - points[i - 1].y);
        total_len += (dx * dx + dy * dy).sqrt();
    }
    if !(total_len > 0.0) {
        return points[0];
    }
    let half_len = total_len / 2.0;
    let mut walked: CGFloat = 0.0;
    for i in 1..points.len() {
        let (dx, dy) = (points[i].x - points[i - 1].x, points[i].y - points[i - 1].y);
        let seg_len = (dx * dx + dy * dy).sqrt();
        if walked + seg_len >= half_len {
            let t = if seg_len > 0.0 { (half_len - walked) / seg_len } else { 0.0 };
            return pt(points[i - 1].x + dx * t, points[i - 1].y + dy * t);
        }
        walked += seg_len;
    }
    points[points.len() - 1]
}
