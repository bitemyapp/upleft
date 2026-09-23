//! Port of `Render/DiagramRenderer+Class.swift`.

use objc2_app_kit::{NSFont, NSFontWeightBold};
use objc2_core_foundation::{CGPoint, CGRect};
use objc2_core_graphics::{CGContext, CGPathDrawingMode};

use super::diagram_renderer::DiagramRenderer;
use super::label_renderer::TextAlignment;
use super::render_adapters::render_rel_type;
use super::render_config::RenderConfig;
use crate::cg::{self, pt, Ctx, Path};
use crate::mermaid::src_class_parser::{ClassMember, PositionedClassRelationship};
use crate::swift;
use crate::types::{PositionedContent, PositionedGraph};

impl DiagramRenderer {
    /// `_drawClass(_:in:bounds:)`.
    pub(crate) fn draw_class(&self, positioned: &PositionedGraph, context: &CGContext, bounds: CGRect) {
        let PositionedContent::ClassDiagram { classes, relationships } = &positioned.content else { return };
        if classes.is_empty() {
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

                // Relationships (lines)
                for rel in relationships {
                    let pts: Vec<CGPoint> = rel.points.iter().map(|p| pt(p.x, p.y)).collect();
                    if pts.len() < 2 {
                        continue;
                    }
                    ctx.save_g_state();
                    ctx.set_stroke_color(&self.theme.effective_line().CGColor());
                    ctx.set_line_width(config.stroke_width_connector);
                    let is_dashed = rel.r#type == render_rel_type::DEPENDENCY || rel.r#type == render_rel_type::REALIZATION;
                    if is_dashed {
                        ctx.set_line_dash(0.0, &[6.0, 4.0]);
                    }
                    ctx.move_to(pts[0]);
                    for p in &pts[1..] {
                        ctx.add_line(*p);
                    }
                    ctx.stroke_path();
                    ctx.restore_g_state();

                    // Marker
                    self.draw_class_marker(rel, &pts, context);
                }

                // Class boxes
                for c in classes {
                    let bx = cg::rect(c.x, c.y, c.width, c.height);
                    ctx.set_fill_color(&self.theme.effective_surface().CGColor());
                    ctx.fill(bx);
                    ctx.set_stroke_color(&self.theme.effective_border().CGColor());
                    ctx.set_line_width(config.stroke_width_outer_box);
                    ctx.stroke(bx);

                    // Header
                    let header_rect = cg::rect(c.x, c.y, c.width, c.header_height);
                    ctx.set_fill_color(&self.theme.subgraph_header_color().CGColor());
                    ctx.fill(header_rect);
                    ctx.set_stroke_color(&self.theme.effective_border().CGColor());
                    ctx.stroke(header_rect);

                    // Annotation (<<interface>>, <<abstract>>, etc.)
                    let mut name_y = c.y + c.header_height / 2.0;
                    if let Some(annotation) = &c.annotation {
                        if !annotation.is_empty() {
                            let annot_y = c.y + 12.0;
                            let annot_font = self.italic_system_font(10.0, 0.23);
                            self.draw_text_in_flipped(
                                &format!("<<{annotation}>>"),
                                pt(c.x + c.width / 2.0, annot_y),
                                context,
                                &self.theme.effective_muted(),
                                &annot_font,
                                TextAlignment::Center,
                            );
                            name_y = c.y + c.header_height / 2.0 + 6.0;
                        }
                    }

                    // Class name
                    let name_font = NSFont::systemFontOfSize_weight(config.font_size_node_label, unsafe { NSFontWeightBold });
                    self.draw_text_in_flipped(
                        &c.label,
                        pt(c.x + c.width / 2.0, name_y),
                        context,
                        &self.theme.foreground,
                        &name_font,
                        TextAlignment::Center,
                    );

                    // Divider
                    let attr_top = c.y + c.header_height;
                    ctx.set_stroke_color(&self.theme.effective_border().CGColor());
                    ctx.set_line_width(config.stroke_width_inner_box);
                    ctx.move_to(pt(c.x, attr_top));
                    ctx.add_line(pt(c.x + c.width, attr_top));
                    ctx.stroke_path();

                    // Attributes
                    for (i, member) in c.attributes.iter().enumerate() {
                        let member_y = attr_top + 4.0 + i as f64 * config.class_member_row_height + config.class_member_row_height / 2.0;
                        self.draw_class_member_highlighted(member, pt(c.x + config.class_box_pad_x, member_y), context, config);
                    }

                    // Method divider
                    let method_top = attr_top + c.attr_height;
                    ctx.move_to(pt(c.x, method_top));
                    ctx.add_line(pt(c.x + c.width, method_top));
                    ctx.stroke_path();

                    // Methods
                    for (i, method) in c.methods.iter().enumerate() {
                        let member_y = method_top + 4.0 + i as f64 * config.class_member_row_height + config.class_member_row_height / 2.0;
                        self.draw_class_member_highlighted(method, pt(c.x + config.class_box_pad_x, member_y), context, config);
                    }
                }

                // Relationship labels + cardinality
                for rel in relationships {
                    if rel.label.is_none() && rel.from_cardinality.is_none() && rel.to_cardinality.is_none() {
                        continue;
                    }
                    let pts: Vec<CGPoint> = rel.points.iter().map(|p| pt(p.x, p.y)).collect();
                    if pts.len() < 2 {
                        continue;
                    }
                    let label_font = config.edge_label_font();

                    if let Some(label) = &rel.label {
                        if !label.is_empty() {
                            let pos = rel.label_position.map_or(pts[pts.len() / 2], |p| pt(p.x, p.y));
                            self.draw_text_in_flipped(
                                label,
                                pt(pos.x, pos.y - 8.0),
                                context,
                                &self.theme.effective_muted(),
                                &label_font,
                                TextAlignment::Center,
                            );
                        }
                    }

                    // From cardinality (near start)
                    if let Some(from_card) = &rel.from_cardinality {
                        if !from_card.is_empty() {
                            let (p, next) = (pts[0], pts[1]);
                            let offset = cardinality_offset(p, next);
                            self.draw_text_in_flipped(
                                from_card,
                                pt(p.x + offset.x, p.y + offset.y),
                                context,
                                &self.theme.effective_muted(),
                                &label_font,
                                TextAlignment::Center,
                            );
                        }
                    }

                    // To cardinality (near end)
                    if let Some(to_card) = &rel.to_cardinality {
                        if !to_card.is_empty() {
                            let (p, prev) = (pts[pts.len() - 1], pts[pts.len() - 2]);
                            let offset = cardinality_offset(p, prev);
                            self.draw_text_in_flipped(
                                to_card,
                                pt(p.x + offset.x, p.y + offset.y),
                                context,
                                &self.theme.effective_muted(),
                                &label_font,
                                TextAlignment::Center,
                            );
                        }
                    }
                }
            },
        );
    }

    fn draw_class_marker(&self, rel: &PositionedClassRelationship, pts: &[CGPoint], context: &CGContext) {
        if pts.len() < 2 {
            return;
        }
        let ctx = Ctx(context);
        let (endpoint, prev_point) =
            if rel.marker_at == "from" { (pts[0], pts[1]) } else { (pts[pts.len() - 1], pts[pts.len() - 2]) };
        let angle = (endpoint.y - prev_point.y).atan2(endpoint.x - prev_point.x);

        ctx.save_g_state();
        ctx.translate_by(endpoint.x, endpoint.y);
        ctx.rotate(angle);

        let t = rel.r#type.as_str();
        if t == render_rel_type::INHERITANCE || t == render_rel_type::REALIZATION {
            let path = Path::new();
            path.move_to(pt(0.0, 0.0));
            path.add_line(pt(-12.0, -5.0));
            path.add_line(pt(-12.0, 5.0));
            path.close_subpath();
            ctx.add_path(path.as_path());
            ctx.set_fill_color(&self.theme.background.CGColor());
            ctx.set_stroke_color(&self.theme.effective_arrow().CGColor());
            ctx.set_line_width(1.5);
            ctx.draw_path(CGPathDrawingMode::FillStroke);
        } else if t == render_rel_type::COMPOSITION {
            let path = Path::new();
            path.move_to(pt(0.0, 0.0));
            path.add_line(pt(-6.0, -5.0));
            path.add_line(pt(-12.0, 0.0));
            path.add_line(pt(-6.0, 5.0));
            path.close_subpath();
            ctx.add_path(path.as_path());
            ctx.set_fill_color(&self.theme.effective_arrow().CGColor());
            ctx.draw_path(CGPathDrawingMode::FillStroke);
        } else if t == render_rel_type::AGGREGATION {
            let path = Path::new();
            path.move_to(pt(0.0, 0.0));
            path.add_line(pt(-6.0, -5.0));
            path.add_line(pt(-12.0, 0.0));
            path.add_line(pt(-6.0, 5.0));
            path.close_subpath();
            ctx.add_path(path.as_path());
            ctx.set_fill_color(&self.theme.background.CGColor());
            ctx.set_stroke_color(&self.theme.effective_arrow().CGColor());
            ctx.set_line_width(1.5);
            ctx.draw_path(CGPathDrawingMode::FillStroke);
        } else {
            // Open arrow for association/dependency
            let path = Path::new();
            path.move_to(pt(-8.0, -3.0));
            path.add_line(pt(0.0, 0.0));
            path.add_line(pt(-8.0, 3.0));
            ctx.add_path(path.as_path());
            ctx.set_stroke_color(&self.theme.effective_arrow().CGColor());
            ctx.set_line_width(1.5);
            ctx.stroke_path();
        }
        ctx.restore_g_state();
    }

    /// `_drawClassMemberHighlighted(_:at:context:contentHeight:config:)`:
    /// visibility, name and type in separate colours; italic for abstract,
    /// underline for static.
    fn draw_class_member_highlighted(&self, member: &ClassMember, point: CGPoint, context: &CGContext, config: &RenderConfig) {
        let ctx = Ctx(context);
        let member_font =
            if member.is_abstract { self.italic_mono_font(config.class_member_font_size) } else { self.mono_font(config.class_member_font_size) };
        let mut current_x = point.x;

        // Visibility symbol
        if !member.visibility.is_empty() {
            let vis_text = format!("{} ", member.visibility);
            self.draw_text_in_flipped(&vis_text, pt(current_x, point.y), context, &self.theme.effective_text_faint(), &member_font, TextAlignment::Left);
            current_x += config.estimate_mono_text_width(&vis_text, config.class_member_font_size);
        }

        // Member name (methods include parentheses and params)
        let display_name = if member.is_method {
            format!("{}({})", member.name, member.params.as_deref().unwrap_or(""))
        } else {
            member.name.clone()
        };
        self.draw_text_in_flipped(
            &display_name,
            pt(current_x, point.y),
            context,
            &self.theme.effective_text_secondary(),
            &member_font,
            TextAlignment::Left,
        );

        // Underline for static members
        if member.is_static {
            let name_width = config.estimate_mono_text_width(&display_name, config.class_member_font_size);
            let underline_y = point.y + 6.0;
            ctx.save_g_state();
            ctx.set_stroke_color(&self.theme.effective_text_secondary().CGColor());
            ctx.set_line_width(1.0);
            ctx.move_to(pt(current_x, underline_y));
            ctx.add_line(pt(current_x + name_width, underline_y));
            ctx.stroke_path();
            ctx.restore_g_state();
        }

        current_x += config.estimate_mono_text_width(&display_name, config.class_member_font_size);

        // Type annotation
        if let Some(t) = &member.r#type {
            if !t.is_empty() {
                let colon_text = ": ";
                self.draw_text_in_flipped(colon_text, pt(current_x, point.y), context, &self.theme.effective_text_faint(), &member_font, TextAlignment::Left);
                current_x += config.estimate_mono_text_width(colon_text, config.class_member_font_size);
                self.draw_text_in_flipped(t, pt(current_x, point.y), context, &self.theme.effective_muted(), &member_font, TextAlignment::Left);
            }
        }
    }
}

/// `_cardinalityOffset(from:to:)`: perpendicular to the edge direction.
fn cardinality_offset(from: CGPoint, to: CGPoint) -> CGPoint {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    if dx.abs() > dy.abs() {
        return pt(if dx > 0.0 { 14.0 } else { -14.0 }, -10.0);
    }
    pt(-14.0, if dy > 0.0 { 14.0 } else { -14.0 })
}
