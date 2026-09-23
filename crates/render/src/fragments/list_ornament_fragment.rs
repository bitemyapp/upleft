//! Port of `Fragments/ListOrnamentFragment.swift`: typographic list markers.
//! Markdown syntax remains hidden while the semantic ornament sits in the
//! hanging indent in Read and Live modes.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::any::Any;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2_app_kit::{NSAttributedStringNSStringDrawing, NSColor, NSFont, NSFontWeightRegular, NSTextLayoutFragment};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect};
use objc2_core_graphics::{CGContext, CGLineCap, CGLineJoin, CGPath};

use crate::appkit_compat::{RectExt, attributed_string, keys, rect};
use crate::engine::render_metrics;
use crate::fragments::fragment_base::{
    CheckboxPulse, DownrightFragment, FragmentBehavior, FragmentContext, cf_absolute_time_get_current, draw_text,
};
use crate::render_contracts::FragmentPayload;
use crate::swift_compat::{pow, smax, smin};
use crate::theme::style_sheet::StyleSheet;

/// `ListOrnamentFragment.taskBoxSide`.
pub const TASK_BOX_SIDE: CGFloat = render_metrics::TASK_BOX_SIDE;
/// `ListOrnamentFragment.taskHitTargetSide`.
pub const TASK_HIT_TARGET_SIDE: CGFloat = 28.0;

/// `ListOrnamentFragment.taskHitRect(textEdge:centreY:bodySize:)`.
pub fn task_hit_rect(text_edge: CGFloat, centre_y: CGFloat, body_size: CGFloat) -> CGRect {
    let task_box = task_box_rect(text_edge, centre_y, body_size);
    rect(
        task_box.mid_x() - TASK_HIT_TARGET_SIDE / 2.0,
        task_box.mid_y() - TASK_HIT_TARGET_SIDE / 2.0,
        TASK_HIT_TARGET_SIDE,
        TASK_HIT_TARGET_SIDE,
    )
}

/// `ListOrnamentFragment.taskBoxRect(textEdge:centreY:bodySize:)`: the
/// paragraph reserves `taskMarkerColumn` as its head indent, so the box's
/// right edge meets the label's left edge after exactly one gap.
pub fn task_box_rect(text_edge: CGFloat, centre_y: CGFloat, _body_size: CGFloat) -> CGRect {
    rect(
        text_edge - TASK_BOX_SIDE - render_metrics::TASK_BOX_GAP,
        centre_y - TASK_BOX_SIDE / 2.0,
        TASK_BOX_SIDE,
        TASK_BOX_SIDE,
    )
}

/// `ListOrnamentFragment.ornamentCentreY(lineTop:lineHeight:font:)`: the
/// middle of the first line's x-height.
pub fn ornament_centre_y(line_top: CGFloat, line_height: CGFloat, font: &NSFont) -> CGFloat {
    let baseline = line_top + line_height + font.descender();
    baseline - font.xHeight() / 2.0
}

/// `ListOrnamentFragment`'s hooks: it only draws beneath its glyphs.
pub struct ListOrnamentFragment;

/// `ListOrnamentFragment(textElement:range:payload:context:)`.
pub fn make(
    text_element: &objc2_app_kit::NSTextElement,
    range: Option<&objc2_app_kit::NSTextRange>,
    payload: &FragmentPayload,
    context: &Rc<FragmentContext>,
) -> Retained<NSTextLayoutFragment> {
    Retained::into_super(DownrightFragment::new(
        c"ListOrnamentFragment",
        text_element,
        range,
        payload,
        context,
        Box::new(ListOrnamentFragment),
    ))
}

impl FragmentBehavior for ListOrnamentFragment {
    fn draw_object(&self, fragment: &DownrightFragment, point: CGPoint, cg: &CGContext) {
        if !fragment.is_first_paragraph_of_block() {
            return;
        }
        let Some(style) = fragment.style_sheet() else { return };
        // `point.x` is the layout fragment origin; read the actual glyph edge
        // so tasks, ordered markers and bullets share one hanging column.
        let first_line = fragment.textLineFragments().firstObject().map(|line| line.typographicBounds());
        let text_edge = point.x + first_line.map_or(0.0, |line| line.min_x());
        let centre_y = ornament_centre_y(
            point.y + first_line.map_or(0.0, |line| line.min_y()),
            first_line.map_or(style.line_height, |line| smax(1.0, line.height())),
            &style.body_font(),
        );

        let payload = fragment.payload();
        let detail = payload.detail();
        if upleft_swift_text::has_prefix(detail, "task:") {
            self.draw_task(
                fragment,
                upleft_swift_text::str_eq(detail, "task:checked"),
                text_edge,
                centre_y,
                &style,
                cg,
            );
            return;
        }

        let marker: String;
        let color: Retained<NSColor>;
        let font: Retained<NSFont>;
        if upleft_swift_text::has_prefix(detail, "ordered:") {
            marker = format!("{}.", upleft_swift_text::drop_first(detail, upleft_swift_text::count("ordered:")));
            color = style.text_secondary.clone();
            font = NSFont::monospacedDigitSystemFontOfSize_weight(style.body_font().pointSize() * 0.92, unsafe {
                NSFontWeightRegular
            });
        } else {
            let level = upleft_swift_text::parse_int(upleft_swift_text::drop_first(
                detail,
                upleft_swift_text::count("unordered:"),
            ))
            .unwrap_or(1);
            marker = if level % 3 == 1 {
                "●"
            } else if level % 3 == 2 {
                "○"
            } else {
                "▪"
            }
            .to_owned();
            color = style.text_faint.clone();
            font = NSFont::systemFontOfSize_weight(style.body_font().pointSize() * 0.30, unsafe { NSFontWeightRegular });
        }
        let attributed = attributed_string(&marker, &[(keys::font(), &font), (keys::foreground_color(), &color)]);
        let size = attributed.size();
        // The marker's baseline sits `size.height + descender` below the
        // rect's top; put it half a cap-height under the optical centre.
        draw_text(
            cg,
            &attributed,
            rect(
                text_edge - size.width - style.body_font().pointSize() * 0.5,
                centre_y + font.capHeight() / 2.0 - (size.height + font.descender()),
                size.width,
                size.height,
            ),
            true,
        );
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ListOrnamentFragment {
    fn draw_task(
        &self,
        fragment: &DownrightFragment,
        checked: bool,
        text_edge: CGFloat,
        centre_y: CGFloat,
        style: &StyleSheet,
        cg: &CGContext,
    ) {
        let side = TASK_BOX_SIDE;
        let mut task_box = task_box_rect(text_edge, centre_y, style.body_font().pointSize());

        // Micro-feedback: the box pops and a ring fades out (§7.1).
        let mut ring: Option<(CGFloat, CGFloat)> = None;
        let source_range = fragment.payload().source_range();
        let pulse = fragment
            .context()
            .and_then(|context| context.checkbox_pulses.borrow().iter().find(|pulse| pulse.source_range == source_range).copied());
        if let Some(pulse) = pulse {
            let elapsed = cf_absolute_time_get_current() - pulse.started;
            if elapsed < CheckboxPulse::DURATION {
                let t = elapsed / CheckboxPulse::DURATION;
                let scale = 1.0 + 0.18 * (smin(1.0, t * 2.0) * std::f64::consts::PI).sin();
                let center = CGPoint::new(task_box.mid_x(), task_box.mid_y());
                task_box = rect(
                    center.x - task_box.width() * scale / 2.0,
                    center.y - task_box.height() * scale / 2.0,
                    task_box.width() * scale,
                    task_box.height() * scale,
                );
                let eased = 1.0 - pow(1.0 - t, 2.0);
                ring = Some((side * (0.55 + 1.2 * eased), (1.0 - t) * 0.5));
            }
        }

        // One checkbox look everywhere (§8.5).
        let context = Some(cg);
        let radius = task_box.width() * render_metrics::TASK_BOX_CORNER_RATIO;
        // SAFETY: a null transform is allowed.
        let path = unsafe { CGPath::with_rounded_rect(task_box, radius, radius, std::ptr::null()) };
        if checked {
            CGContext::add_path(context, Some(&path));
            CGContext::set_fill_color_with_color(context, Some(&style.task_field_color().CGColor()));
            CGContext::fill_path(context);
        }
        CGContext::add_path(context, Some(&path));
        CGContext::set_stroke_color_with_color(context, Some(&style.task_ring_color(checked).CGColor()));
        CGContext::set_line_width(context, task_box.width() * render_metrics::TASK_BOX_STROKE_RATIO);
        CGContext::stroke_path(context);
        if checked {
            CGContext::set_stroke_color_with_color(context, Some(&style.task_tick_color().CGColor()));
            CGContext::set_line_width(context, task_box.width() * render_metrics::TASK_TICK_STROKE_RATIO);
            CGContext::set_line_cap(context, CGLineCap::Round);
            CGContext::set_line_join(context, CGLineJoin::Round);
            // The unit tick is measured from the bottom of the box and this
            // context is flipped, so y counts down from `maxY`.
            for (index, unit) in render_metrics::TASK_TICK.iter().enumerate() {
                let x = task_box.min_x() + unit.x * task_box.width();
                let y = task_box.max_y() - unit.y * task_box.height();
                if index == 0 {
                    CGContext::move_to_point(context, x, y);
                } else {
                    CGContext::add_line_to_point(context, x, y);
                }
            }
            CGContext::stroke_path(context);
        }

        if let Some((ring_radius, alpha)) = ring {
            let ring_rect = rect(
                task_box.mid_x() - ring_radius,
                task_box.mid_y() - ring_radius,
                ring_radius * 2.0,
                ring_radius * 2.0,
            );
            CGContext::set_stroke_color_with_color(context, Some(&style.accent.colorWithAlphaComponent(alpha).CGColor()));
            CGContext::set_line_width(context, 1.5);
            CGContext::stroke_ellipse_in_rect(context, ring_rect);
        }
    }
}
