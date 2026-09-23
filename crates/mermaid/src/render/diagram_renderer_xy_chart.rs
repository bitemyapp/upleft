//! Port of `Render/DiagramRenderer+XYChart.swift`. Unlike the other
//! renderers this one flips the context back to y-up and draws text with
//! `CTLineDraw`.

use objc2::rc::Retained;
use objc2_app_kit::{NSColor, NSFont, NSFontWeightMedium, NSFontWeightRegular, NSFontWeightSemibold, NSTextAlignment};
use objc2_core_foundation::{CFRetained, CGFloat};
use objc2_core_graphics::{CGColor, CGContext, CGLineCap, CGLineJoin};
use objc2_core_text::{CTLine, CTLineBoundsOptions};

use super::diagram_renderer::DiagramRenderer;
use super::label_renderer::{attributed, attributes};
use crate::cg::{self, pt, Ctx};
use crate::cross_platform::{color_from_hex, hex};
use crate::mermaid::src_xychart_colors::get_series_color;
use crate::mermaid::src_xychart_types::{LinePoint, XYSeriesType};
use crate::swift;
use crate::types::{PositionedContent, PositionedGraph};

impl DiagramRenderer {
    /// `_drawXYChart(_:in:bounds:)`.
    pub(crate) fn draw_xy_chart(&self, positioned: &PositionedGraph, context: &CGContext, bounds: objc2_core_foundation::CGRect) {
        let PositionedContent::XyChart(chart) = &positioned.content else { return };

        self.with_fitted_context(context, bounds, swift::max(1.0, chart.width), swift::max(1.0, chart.height), |context| {
            let ctx = Ctx(context);
            let ch = chart.height;
            // Flip to y=0-at-bottom: XY chart uses CTLineDraw which requires y-up.
            ctx.translate_by(0.0, ch);
            ctx.scale_by(1.0, -1.0);
            let fy = |y: f64| ch - y;

            // Background
            if !self.theme.transparent {
                ctx.set_fill_color(&self.theme.background.CGColor());
                ctx.fill(cg::rect(0.0, 0.0, chart.width, ch));
            }

            let text_color = self.theme.foreground.CGColor();
            let muted_color = self.theme.effective_muted().CGColor();
            let bg_color = self.theme.background.CGColor();

            // Grid dots
            let plot_area = chart.plot_area;
            let x_ticks: Vec<f64> = chart.x_axis.ticks.iter().map(|t| t.x).collect();
            let y_vals: Vec<f64> = if chart.horizontal {
                chart.y_axis.ticks.iter().map(|t| t.y).collect()
            } else {
                chart.grid_lines.iter().map(|g| g.y1).collect()
            };
            let x_base = if x_ticks.len() > 1 { (x_ticks[1] - x_ticks[0]).abs() } else { plot_area.width / 6.0 };
            let y_base = if y_vals.len() > 1 { (y_vals[1] - y_vals[0]).abs() } else { plot_area.height / 6.0 };
            let x_gap = x_base / swift::max(1, swift::int((x_base / 20.0).round())) as f64;
            let y_gap = y_base / swift::max(1, swift::int((y_base / 20.0).round())) as f64;
            let x_anchor = x_ticks.first().copied().unwrap_or(plot_area.x);
            let y_anchor = y_vals.first().copied().unwrap_or(plot_area.y);
            let x_start = x_anchor - ((x_anchor - plot_area.x) / x_gap).ceil() * x_gap;
            let y_start = y_anchor - ((y_anchor - plot_area.y) / y_gap).ceil() * y_gap;

            ctx.set_fill_color(&muted_color);
            ctx.set_alpha(0.3);
            let mut dot_y = y_start;
            while dot_y <= plot_area.y + plot_area.height + 0.5 {
                let mut dot_x = x_start;
                while dot_x <= plot_area.x + plot_area.width + 0.5 {
                    ctx.fill_ellipse(cg::rect(dot_x - 1.5, fy(dot_y) - 1.5, 3.0, 3.0));
                    dot_x += x_gap;
                }
                dot_y += y_gap;
            }
            ctx.set_alpha(1.0);

            let accent_hex = hex(&self.theme.effective_accent());
            let bg_hex = hex(&self.theme.background);

            // Bars
            for bar in &chart.bars {
                let series_color = self.xy_series_color(bar.color_index, accent_hex.as_deref(), bg_hex.as_deref());
                let fill_color = mix_cg_colors(&bg_color, &series_color, 0.25);
                let bar_rect = cg::rect(bar.x, fy(bar.y + bar.height), bar.width, bar.height);
                let cr = swift::min_n(&[8.0, bar.width / 2.0, bar.height / 2.0]);
                let bar_path = cg::path_rounded_rect(bar_rect, cr, cr);
                ctx.add_path(&bar_path);
                ctx.set_fill_color(&fill_color);
                ctx.fill_path();
                ctx.set_stroke_color(&series_color);
                ctx.set_line_width(1.5);
                ctx.add_path(&bar_path);
                ctx.stroke_path();
            }

            // Lines
            for line in &chart.lines {
                if line.points.is_empty() {
                    continue;
                }
                let series_color = self.xy_series_color(line.color_index, accent_hex.as_deref(), bg_hex.as_deref());
                let flipped: Vec<LinePoint> =
                    line.points.iter().map(|p| LinePoint { x: p.x, y: fy(p.y), value: p.value, label: p.label.clone() }).collect();

                // Shadow
                ctx.save_g_state();
                ctx.set_stroke_color(&series_color);
                ctx.set_line_width(5.0);
                ctx.set_alpha(0.12);
                add_curve_path(ctx, &flipped, -2.0);
                ctx.stroke_path();
                ctx.restore_g_state();

                // Main line
                ctx.set_stroke_color(&series_color);
                ctx.set_line_width(2.5);
                ctx.set_line_cap(CGLineCap::Round);
                ctx.set_line_join(CGLineJoin::Round);
                add_curve_path(ctx, &flipped, 0.0);
                ctx.stroke_path();

                // Dots for sparse lines
                if flipped.len() <= 12 {
                    for p in &flipped {
                        ctx.set_fill_color(&series_color);
                        ctx.fill_ellipse(cg::rect(p.x - 5.0, p.y - 5.0, 10.0, 10.0));
                        ctx.set_fill_color(&bg_color);
                        ctx.fill_ellipse(cg::rect(p.x - 3.0, p.y - 3.0, 6.0, 6.0));
                        ctx.set_fill_color(&series_color);
                        ctx.fill_ellipse(cg::rect(p.x - 2.0, p.y - 2.0, 4.0, 4.0));
                    }
                }
            }

            let anchor_for = |text_anchor: &str| {
                if text_anchor == "end" {
                    NSTextAlignment::Right
                } else if text_anchor == "start" {
                    NSTextAlignment::Left
                } else {
                    NSTextAlignment::Center
                }
            };

            // Axis labels (muted color, matching edge labels in flowcharts)
            let label_font = NSFont::systemFontOfSize_weight(12.0, unsafe { NSFontWeightRegular });
            for tick in &chart.x_axis.ticks {
                draw_text_xy(context, &tick.label, tick.label_x, fy(tick.label_y), &label_font, &muted_color, anchor_for(&tick.text_anchor));
            }
            for tick in &chart.y_axis.ticks {
                draw_text_xy(context, &tick.label, tick.label_x, fy(tick.label_y), &label_font, &muted_color, anchor_for(&tick.text_anchor));
            }

            // Axis titles
            let axis_title_font = NSFont::systemFontOfSize_weight(15.0, unsafe { NSFontWeightMedium });
            for title in [&chart.x_axis.title, &chart.y_axis.title].into_iter().flatten() {
                if let Some(rotate) = title.rotate {
                    ctx.save_g_state();
                    ctx.translate_by(title.x, fy(title.y));
                    ctx.rotate(-rotate * std::f64::consts::PI / 180.0);
                    draw_text_xy(context, &title.text, 0.0, 0.0, &axis_title_font, &text_color, NSTextAlignment::Center);
                    ctx.restore_g_state();
                } else {
                    draw_text_xy(context, &title.text, title.x, fy(title.y), &axis_title_font, &text_color, NSTextAlignment::Center);
                }
            }

            // Chart title (smaller font, centered at top)
            if let Some(title) = &chart.title {
                let title_font = NSFont::systemFontOfSize_weight(16.0, unsafe { NSFontWeightSemibold });
                draw_text_xy(context, &title.text, title.x, fy(title.y), &title_font, &text_color, NSTextAlignment::Center);
            }

            // Legend
            let legend_font = NSFont::systemFontOfSize_weight(12.0, unsafe { NSFontWeightRegular });

            for item in &chart.legend {
                let series_color = self.xy_series_color(item.color_index, accent_hex.as_deref(), bg_hex.as_deref());
                let iy = fy(item.y);
                let sy = iy; // swatch center matches text visual center
                if item.r#type == XYSeriesType::Bar {
                    let fill_color = mix_cg_colors(&bg_color, &series_color, 0.25);
                    let swatch_rect = cg::rect(item.x, sy - 5.0, 12.0, 10.0);
                    let swatch_path = cg::path_rounded_rect(swatch_rect, 2.0, 2.0);
                    ctx.add_path(&swatch_path);
                    ctx.set_fill_color(&fill_color);
                    ctx.fill_path();
                    ctx.add_path(&swatch_path);
                    ctx.set_stroke_color(&series_color);
                    ctx.set_line_width(1.5);
                    ctx.stroke_path();
                } else {
                    ctx.set_stroke_color(&series_color);
                    ctx.set_line_width(2.5);
                    ctx.set_line_cap(CGLineCap::Round);
                    ctx.move_to(pt(item.x, sy));
                    ctx.add_line(pt(item.x + 12.0, sy));
                    ctx.stroke_path();
                }
                draw_text_xy(context, &item.label, item.x + 17.0, iy, &legend_font, &muted_color, NSTextAlignment::Left);
            }
        });
    }

    /// `_xySeriesColor(_:accentHex:bgHex:)`.
    fn xy_series_color(&self, index: i64, accent_hex: Option<&str>, bg_hex: Option<&str>) -> CFRetained<CGColor> {
        if index == 0 {
            return cf(&self.theme.effective_accent().CGColor());
        }
        let accent = match accent_hex {
            Some(a) => a.to_owned(),
            None => hex(&self.theme.effective_accent()).unwrap_or_else(|| "#3b82f6".into()),
        };
        let hex = get_series_color(index, &accent, bg_hex);
        cf(&color_from_hex(&hex).CGColor())
    }
}

/// An `NSColor.cgColor` result as a `CFRetained` (both are the same +1 CF object).
fn cf(color: &CGColor) -> CFRetained<CGColor> {
    unsafe { CFRetained::retain(std::ptr::NonNull::from(color)) }
}

/// `CGColor.components` (`nil` → the fallback).
fn components(color: &CGColor, fallback: [CGFloat; 4]) -> Vec<CGFloat> {
    let count = CGColor::number_of_components(Some(color));
    let pointer = CGColor::components(Some(color));
    if pointer.is_null() {
        return fallback.to_vec();
    }
    unsafe { std::slice::from_raw_parts(pointer, count) }.to_vec()
}

/// `_mixCGColors(_:_:ratio:)`.
fn mix_cg_colors(bg: &CGColor, fg: &CGColor, ratio: CGFloat) -> CFRetained<CGColor> {
    let bg_comps = components(bg, [1.0, 1.0, 1.0, 1.0]);
    let fg_comps = components(fg, [0.0, 0.0, 0.0, 1.0]);
    let r = bg_comps[0] * (1.0 - ratio) + fg_comps[0] * ratio;
    let g = (if bg_comps.len() > 1 { bg_comps[1] } else { bg_comps[0] }) * (1.0 - ratio)
        + (if fg_comps.len() > 1 { fg_comps[1] } else { fg_comps[0] }) * ratio;
    let b = (if bg_comps.len() > 2 { bg_comps[2] } else { bg_comps[0] }) * (1.0 - ratio)
        + (if fg_comps.len() > 2 { fg_comps[2] } else { fg_comps[0] }) * ratio;
    cg_color_rgb(r, g, b, 1.0)
}

/// `CGColor(red:green:blue:alpha:)`.
fn cg_color_rgb(r: CGFloat, g: CGFloat, b: CGFloat, a: CGFloat) -> CFRetained<CGColor> {
    CGColor::new_generic_rgb(r, g, b, a)
}

/// `_addCurvePath(_:_:offsetY:)`: a natural cubic spline through the points.
fn add_curve_path(ctx: Ctx, points: &[LinePoint], offset_y: f64) {
    if points.is_empty() {
        return;
    }
    if points.len() == 1 {
        ctx.move_to(pt(points[0].x, points[0].y + offset_y));
        return;
    }
    if points.len() == 2 {
        ctx.move_to(pt(points[0].x, points[0].y + offset_y));
        ctx.add_line(pt(points[1].x, points[1].y + offset_y));
        return;
    }

    let n = points.len();
    let mut h: Vec<f64> = Vec::with_capacity(n - 1);
    let mut delta: Vec<f64> = Vec::with_capacity(n - 1);
    for i in 0..n - 1 {
        let hi = points[i + 1].x - points[i].x;
        h.push(hi);
        delta.push(if hi == 0.0 { 0.0 } else { (points[i + 1].y - points[i].y) / hi });
    }

    let mut c = vec![0.0f64; n];
    if n > 2 {
        let mut cp = vec![0.0f64; n];
        let mut dp = vec![0.0f64; n];
        for i in 1..n - 1 {
            let diag = 2.0 * (h[i - 1] + h[i]);
            let rhs = 3.0 * (delta[i] - delta[i - 1]);
            if i == 1 {
                cp[i] = h[i] / diag;
                dp[i] = rhs / diag;
            } else {
                let w = diag - h[i - 1] * cp[i - 1];
                cp[i] = h[i] / w;
                dp[i] = (rhs - h[i - 1] * dp[i - 1]) / w;
            }
        }
        let mut i = n - 2;
        while i >= 1 {
            c[i] = dp[i] - cp[i] * c[i + 1];
            i -= 1;
        }
    }

    let mut slopes = vec![0.0f64; n];
    for i in 0..n - 1 {
        slopes[i] = delta[i] - h[i] * (2.0 * c[i] + c[i + 1]) / 3.0;
    }
    slopes[n - 1] = delta[n - 2] + h[n - 2] * c[n - 2] / 3.0;

    ctx.move_to(pt(points[0].x, points[0].y + offset_y));
    for i in 0..n - 1 {
        let seg = h[i] / 3.0;
        let cp1 = pt(points[i].x + seg, points[i].y + slopes[i] * seg + offset_y);
        let cp2 = pt(points[i + 1].x - seg, points[i + 1].y - slopes[i + 1] * seg + offset_y);
        ctx.add_curve(pt(points[i + 1].x, points[i + 1].y + offset_y), cp1, cp2);
    }
}

/// `_drawTextXY(_:_:x:y:font:color:align:)`.
fn draw_text_xy(context: &CGContext, text: &str, x: f64, y: f64, font: &NSFont, color: &CGColor, align: NSTextAlignment) {
    let ns_color: Retained<NSColor> = NSColor::colorWithCGColor(color).unwrap_or_else(NSColor::blackColor);
    let attr_string = attributed(text, &attributes(font, Some(&ns_color)));
    let line = unsafe { CTLine::with_attributed_string(std::mem::transmute::<&objc2_foundation::NSAttributedString, &objc2_core_foundation::CFAttributedString>(&attr_string)) };
    let text_bounds = unsafe { line.bounds_with_options(CTLineBoundsOptions::UseOpticalBounds) };
    let (mut ascent, mut descent): (CGFloat, CGFloat) = (0.0, 0.0);
    unsafe { line.typographic_bounds(&mut ascent, &mut descent, std::ptr::null_mut()) };

    let draw_x = if align == NSTextAlignment::Center {
        x - cg::width(text_bounds) / 2.0
    } else if align == NSTextAlignment::Right {
        x - cg::width(text_bounds)
    } else {
        x
    };
    // Vertically center: baseline = y - (ascent - descent)/2 in y-up space.
    let draw_y = y - (ascent - descent) / 2.0;

    let ctx = Ctx(context);
    ctx.save_g_state();
    ctx.set_text_position(pt(draw_x, draw_y));
    unsafe { line.draw(context) };
    ctx.restore_g_state();
}
