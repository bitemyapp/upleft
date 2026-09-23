//! Port of `Render/DiagramRenderer+Sequence.swift`.

use objc2_core_foundation::{CGFloat, CGPoint, CGRect};
use objc2_core_graphics::{CGContext, CGLineCap, CGLineJoin};

use super::diagram_renderer::DiagramRenderer;
use super::label_renderer::TextAlignment;
use crate::cg::{self, pt, Ctx, Path};
use crate::cross_platform::{bezier_rounded_rect, bm_cg_path};
use crate::mermaid::src_sequence_parser::PositionedSequenceActor;
use crate::swift;
use crate::types::{PositionedContent, PositionedGraph};

impl DiagramRenderer {
    /// `_drawSequence(_:in:bounds:)`.
    pub(crate) fn draw_sequence(&self, positioned: &PositionedGraph, context: &CGContext, bounds: CGRect) {
        let PositionedContent::SequenceDiagram { actors, messages, blocks, lifelines, activations, notes } = &positioned.content
        else {
            return;
        };
        if actors.is_empty() {
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

                // 1. Block regions (loop/alt/opt/par/critical)
                for block in blocks {
                    let block_rect = cg::rect(block.x, block.y, block.width, block.height);
                    // Border only (transparent background, matching OSS)
                    ctx.set_stroke_color(&self.theme.effective_border().CGColor());
                    ctx.set_line_width(config.stroke_width_outer_box);
                    ctx.stroke(block_rect);

                    // Tab label
                    let label_text = if block.label.is_empty() {
                        block.r#type.clone()
                    } else {
                        format!("{} [{}]", block.r#type, block.label)
                    };
                    let tab_width = config.estimate_text_width(&label_text, config.font_size_edge_label, config.font_weight_group_header) + 16.0;
                    let tab_height = config.sequence_tab_height;
                    let tab_rect = cg::rect(block.x, block.y, tab_width, tab_height);
                    ctx.set_fill_color(&self.theme.subgraph_header_color().CGColor());
                    ctx.fill(tab_rect);
                    ctx.set_stroke_color(&self.theme.effective_border().CGColor());
                    ctx.stroke(tab_rect);

                    self.draw_text_in_flipped(
                        &label_text,
                        pt(block.x + 6.0, block.y + tab_height / 2.0),
                        context,
                        &self.theme.effective_text_secondary(),
                        &config.group_header_font(),
                        TextAlignment::Left,
                    );

                    // Dividers
                    for divider in &block.dividers {
                        ctx.save_g_state();
                        ctx.set_stroke_color(&self.theme.effective_line().CGColor());
                        ctx.set_line_width(0.75);
                        ctx.set_line_dash(0.0, &[6.0, 4.0]);
                        ctx.move_to(pt(block.x, divider.y));
                        ctx.add_line(pt(block.x + block.width, divider.y));
                        ctx.stroke_path();
                        ctx.restore_g_state();

                        if !divider.label.is_empty() {
                            self.draw_text_in_flipped(
                                &format!("[{}]", divider.label),
                                pt(block.x + 8.0, divider.y + 14.0),
                                context,
                                &self.theme.effective_muted(),
                                &config.edge_label_font(),
                                TextAlignment::Left,
                            );
                        }
                    }
                }

                // 2. Lifelines (dashed vertical lines)
                ctx.save_g_state();
                ctx.set_stroke_color(&self.theme.effective_line().CGColor());
                ctx.set_line_width(0.75);
                ctx.set_line_dash(0.0, &[6.0, 4.0]);
                if lifelines.is_empty() {
                    // Fallback: compute from actors
                    let max_y = swift::seq_max(messages.iter().map(|m| m.y)).unwrap_or(300.0);
                    for actor in actors {
                        ctx.move_to(pt(actor.x, actor.y + actor.height));
                        ctx.add_line(pt(actor.x, max_y + 60.0));
                        ctx.stroke_path();
                    }
                } else {
                    for ll in lifelines {
                        ctx.move_to(pt(ll.x, ll.top_y));
                        ctx.add_line(pt(ll.x, ll.bottom_y));
                        ctx.stroke_path();
                    }
                }
                ctx.restore_g_state();

                // 3. Activation bars
                for act in activations {
                    let act_rect = cg::rect(act.x - act.width / 2.0, act.top_y, act.width, act.bottom_y - act.top_y);
                    ctx.set_fill_color(&self.theme.effective_surface().CGColor());
                    ctx.fill(act_rect);
                    ctx.set_stroke_color(&self.theme.effective_border().CGColor());
                    ctx.set_line_width(config.stroke_width_inner_box);
                    ctx.stroke(act_rect);
                }

                // 4. Messages (arrows with labels)
                for msg in messages {
                    ctx.save_g_state();
                    ctx.set_stroke_color(&self.theme.effective_line().CGColor());
                    ctx.set_line_width(config.stroke_width_connector);
                    ctx.set_line_cap(CGLineCap::Round);
                    ctx.set_line_join(CGLineJoin::Round);

                    if msg.line_style == "dashed" {
                        ctx.set_line_dash(0.0, &[6.0, 4.0]);
                    }

                    if msg.is_self {
                        let (loop_w, loop_h): (CGFloat, CGFloat) = (28.0, 20.0);
                        let pts = [
                            pt(msg.x1, msg.y),
                            pt(msg.x1 + loop_w, msg.y),
                            pt(msg.x1 + loop_w, msg.y + loop_h),
                            pt(msg.x2, msg.y + loop_h),
                        ];
                        ctx.move_to(pts[0]);
                        for p in &pts[1..] {
                            ctx.add_line(*p);
                        }
                        ctx.stroke_path();
                        ctx.restore_g_state();

                        let last_pt = pts[pts.len() - 1];
                        self.draw_sequence_arrow_head(last_pt, pts[pts.len() - 2], &msg.arrow_head, context);
                        self.draw_text_in_flipped(
                            &msg.label,
                            pt(msg.x1 + loop_w + 4.0, msg.y + loop_h / 2.0),
                            context,
                            &self.theme.effective_muted(),
                            &config.edge_label_font(),
                            TextAlignment::Left,
                        );
                    } else {
                        ctx.move_to(pt(msg.x1, msg.y));
                        ctx.add_line(pt(msg.x2, msg.y));
                        ctx.stroke_path();
                        ctx.restore_g_state();

                        self.draw_sequence_arrow_head(pt(msg.x2, msg.y), pt(msg.x1, msg.y), &msg.arrow_head, context);
                        self.draw_text_in_flipped(
                            &msg.label,
                            pt((msg.x1 + msg.x2) / 2.0, msg.y - 8.0),
                            context,
                            &self.theme.effective_muted(),
                            &config.edge_label_font(),
                            TextAlignment::Center,
                        );
                    }
                }

                // 5. Notes (sticky-note polygons with fold corner)
                for note in notes {
                    let note_rect = cg::rect(note.x, note.y, note.width, note.height);
                    let fold_size: CGFloat = 6.0;

                    let note_path = Path::new();
                    note_path.move_to(pt(cg::min_x(note_rect), cg::min_y(note_rect)));
                    note_path.add_line(pt(cg::max_x(note_rect) - fold_size, cg::min_y(note_rect)));
                    note_path.add_line(pt(cg::max_x(note_rect), cg::min_y(note_rect) + fold_size));
                    note_path.add_line(pt(cg::max_x(note_rect), cg::max_y(note_rect)));
                    note_path.add_line(pt(cg::min_x(note_rect), cg::max_y(note_rect)));
                    note_path.close_subpath();

                    ctx.set_fill_color(&self.theme.subgraph_header_color().CGColor());
                    ctx.add_path(note_path.as_path());
                    ctx.fill_path();
                    ctx.set_stroke_color(&self.theme.effective_border().CGColor());
                    ctx.set_line_width(config.stroke_width_inner_box);
                    ctx.add_path(note_path.as_path());
                    ctx.stroke_path();

                    // Fold triangle
                    let fold_path = Path::new();
                    fold_path.move_to(pt(cg::max_x(note_rect) - fold_size, cg::min_y(note_rect)));
                    fold_path.add_line(pt(cg::max_x(note_rect) - fold_size, cg::min_y(note_rect) + fold_size));
                    fold_path.add_line(pt(cg::max_x(note_rect), cg::min_y(note_rect) + fold_size));
                    fold_path.close_subpath();
                    ctx.set_fill_color(&self.theme.effective_border().CGColor());
                    ctx.add_path(fold_path.as_path());
                    ctx.fill_path();

                    if !note.text.is_empty() {
                        let inset = cg::inset(note_rect, 6.0, 4.0);
                        self.label_renderer.draw_multiline_text(
                            &note.text,
                            inset,
                            context,
                            &self.theme.effective_muted(),
                            &config.edge_label_font(),
                            TextAlignment::Center,
                        );
                    }
                }

                // 6. Actor boxes (on top)
                for actor in actors {
                    if actor.r#type == "actor" {
                        self.draw_actor_figure(actor, context);
                    } else {
                        let bx = cg::rect(actor.x - actor.width / 2.0, actor.y, actor.width, actor.height);
                        let path = bezier_rounded_rect(bx, 4.0);
                        ctx.set_fill_color(&self.theme.effective_surface().CGColor());
                        ctx.add_path(&bm_cg_path(&path));
                        ctx.fill_path();
                        ctx.set_stroke_color(&self.theme.effective_border().CGColor());
                        ctx.set_line_width(1.0);
                        ctx.add_path(&bm_cg_path(&path));
                        ctx.stroke_path();

                        self.draw_text_in_flipped(
                            &actor.label,
                            pt(cg::mid_x(bx), cg::mid_y(bx)),
                            context,
                            &self.theme.foreground,
                            &config.node_label_font(),
                            TextAlignment::Center,
                        );
                    }
                }
            },
        );
    }

    fn draw_sequence_arrow_head(&self, point: CGPoint, prev: CGPoint, style: &str, context: &CGContext) {
        let ctx = Ctx(context);
        let config = &self.config;
        let arrow_color = self.theme.effective_arrow();
        let line_width = config.stroke_width_connector;
        let arrow_width = config.arrow_head_width * line_width;
        let arrow_height = config.arrow_head_height * line_width;
        let angle = (point.y - prev.y).atan2(point.x - prev.x);

        ctx.save_g_state();
        ctx.translate_by(point.x, point.y);
        ctx.rotate(angle);
        let cg_color = arrow_color.CGColor();
        ctx.set_stroke_color(&cg_color);
        ctx.set_fill_color(&cg_color);
        ctx.set_line_width(config.stroke_width_connector);

        match style {
            "open" => {
                // Open V shape (not filled)
                ctx.move_to(pt(-arrow_width, -arrow_height / 2.0));
                ctx.add_line(pt(0.0, 0.0));
                ctx.add_line(pt(-arrow_width, arrow_height / 2.0));
                ctx.stroke_path();
            }
            "cross" => {
                // X mark
                let cross_size = arrow_height * 0.5;
                ctx.move_to(pt(-cross_size * 2.0, -cross_size));
                ctx.add_line(pt(0.0, cross_size));
                ctx.move_to(pt(-cross_size * 2.0, cross_size));
                ctx.add_line(pt(0.0, -cross_size));
                ctx.stroke_path();
            }
            _ => {
                // Filled triangle
                let path = Path::new();
                path.move_to(pt(0.0, 0.0));
                path.add_line(pt(-arrow_width, -arrow_height / 2.0));
                path.add_line(pt(-arrow_width, arrow_height / 2.0));
                path.close_subpath();
                ctx.add_path(path.as_path());
                ctx.fill_path();
            }
        }
        ctx.restore_g_state();
    }

    fn draw_actor_figure(&self, actor: &PositionedSequenceActor, context: &CGContext) {
        let ctx = Ctx(context);
        let config = &self.config;
        let cx = actor.x;
        let box_top = actor.y;
        let fig_h = actor.height - 16.0; // leave room for label below
        let scale = fig_h / 24.0; // SVG viewBox is 24x24
        let origin_x = cx - 12.0 * scale;
        let origin_y = box_top;

        ctx.save_g_state();
        ctx.set_stroke_color(&self.theme.effective_line().CGColor());
        ctx.set_line_width(1.5);
        ctx.set_line_cap(CGLineCap::Round);
        ctx.set_line_join(CGLineJoin::Round);

        // Outer circle (24x24 space: circle at center 12,12 radius 11)
        let outer_r = 11.0 * scale;
        ctx.stroke_ellipse(cg::rect(
            origin_x + 12.0 * scale - outer_r,
            origin_y + 12.0 * scale - outer_r,
            outer_r * 2.0,
            outer_r * 2.0,
        ));

        // Head circle (center at 12, 10, radius ~3)
        let head_r = 3.0 * scale;
        ctx.stroke_ellipse(cg::rect(
            origin_x + 12.0 * scale - head_r,
            origin_y + 10.0 * scale - head_r,
            head_r * 2.0,
            head_r * 2.0,
        ));

        // Shoulders arc: bezier from (5.6, 18.4) through (12, 16) to (18.4, 18.4)
        let path = Path::new();
        path.move_to(pt(origin_x + 5.6 * scale, origin_y + 18.4 * scale));
        path.add_quad_curve(pt(origin_x + 18.4 * scale, origin_y + 18.4 * scale), pt(origin_x + 12.0 * scale, origin_y + 16.0 * scale));
        ctx.add_path(path.as_path());
        ctx.stroke_path();
        ctx.restore_g_state();

        // Label below the figure
        let label_y = box_top + fig_h + 8.0;
        self.draw_text_in_flipped(
            &actor.label,
            pt(cx, label_y),
            context,
            &self.theme.foreground,
            &config.node_label_font(),
            TextAlignment::Center,
        );
    }
}
