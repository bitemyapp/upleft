//! Port of `Fragments/InlineCodePill.swift`: the tinted band behind an inline
//! code span, and the invisible-character marks, both measured from the
//! fragment's own line fragments at draw time (§11.3).
//!
//! Ported with the view layer because `ProseFragment` and `DownrightFragment`
//! paint both in their own `draw(at:in:)`.

use objc2_app_kit::{NSColor, NSTextLayoutFragment, NSTextLineFragment};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect};
use objc2_core_graphics::{CGContext, CGPath};
use objc2_foundation::NSRange;

use crate::appkit_compat::{RectExt, enumerate_attribute, intersection_range, rect};
use crate::engine::render_metrics;
use crate::render_contracts::attribute_keys;
use crate::theme::style_sheet::StyleSheet;

/// Horizontal air on each side of the code run.
pub const INLINE_CODE_PILL_PAD_X: CGFloat = 3.0;

/// Bands for every `.drInlineCode` run `fragment` lays out, one per visual
/// line, in the fragment's own drawing space anchored at `point`.
pub fn inline_code_pill_bands(fragment: &NSTextLayoutFragment, point: CGPoint, text_offset: CGFloat) -> Vec<CGRect> {
    let mut bands = Vec::new();
    for line in fragment.textLineFragments().iter() {
        let string = line.attributedString();
        let line_range = intersection_range(line.characterRange(), NSRange::new(0, string.length()));
        if line_range.length == 0 {
            continue;
        }
        let bounds = line.typographicBounds();
        enumerate_attribute(&string, attribute_keys::dr_inline_code(), line_range, false, |value, range| {
            if value.is_none() {
                return true;
            }
            let leading = line.locationForCharacterAtIndex(range.location as isize).x;
            let trailing = trailing_edge(&line, range.location + range.length, leading, line_range, bounds);
            if !(trailing > leading) {
                return true;
            }
            bands.push(rect(
                point.x + leading - INLINE_CODE_PILL_PAD_X,
                point.y + bounds.min_y() + text_offset,
                trailing - leading + INLINE_CODE_PILL_PAD_X * 2.0,
                bounds.height(),
            ));
            true
        });
    }
    bands
}

/// One box per invisible character `fragment` lays out.
pub fn invisible_mark_boxes(fragment: &NSTextLayoutFragment, point: CGPoint, text_offset: CGFloat) -> Vec<(CGRect, bool)> {
    let mut boxes = Vec::new();
    for line in fragment.textLineFragments().iter() {
        let string = line.attributedString();
        let line_range = intersection_range(line.characterRange(), NSRange::new(0, string.length()));
        if line_range.length == 0 {
            continue;
        }
        let text = string.string();
        let bounds = line.typographicBounds();
        enumerate_attribute(&string, attribute_keys::dr_invisible(), line_range, false, |value, range| {
            if value.is_none() {
                return true;
            }
            for index in range.location..range.location + range.length {
                let leading = line.locationForCharacterAtIndex(index as isize).x;
                let trailing = trailing_edge(&line, index + 1, leading, line_range, bounds);
                if !(trailing > leading) {
                    continue;
                }
                boxes.push((
                    rect(point.x + leading, point.y + bounds.min_y() + text_offset, trailing - leading, bounds.height()),
                    text.characterAtIndex(index) == 0x09,
                ));
            }
            true
        });
    }
    boxes
}

/// A dot for a space, a small arrow for a tab.
pub fn draw_invisible_marks(
    fragment: &NSTextLayoutFragment,
    point: CGPoint,
    text_offset: CGFloat,
    style_sheet: &StyleSheet,
    cg: &CGContext,
) {
    let boxes = invisible_mark_boxes(fragment, point, text_offset);
    if boxes.is_empty() {
        return;
    }
    let cg = Some(cg);
    CGContext::save_g_state(cg);
    let faint = style_sheet.text_faint.CGColor();
    CGContext::set_stroke_color_with_color(cg, Some(&faint));
    CGContext::set_fill_color_with_color(cg, Some(&faint));
    CGContext::set_line_width(cg, 1.0);
    for (bounds, is_tab) in boxes {
        let middle = CGPoint::new(bounds.mid_x(), bounds.mid_y());
        if is_tab {
            CGContext::move_to_point(cg, bounds.min_x(), middle.y);
            CGContext::add_line_to_point(cg, bounds.max_x(), middle.y);
            CGContext::add_line_to_point(cg, bounds.max_x() - 3.0, middle.y - 2.0);
            CGContext::move_to_point(cg, bounds.max_x(), middle.y);
            CGContext::add_line_to_point(cg, bounds.max_x() - 3.0, middle.y + 2.0);
            CGContext::stroke_path(cg);
        } else {
            CGContext::fill_ellipse_in_rect(cg, rect(middle.x - 1.0, middle.y - 1.0, 2.0, 2.0));
        }
    }
    CGContext::restore_g_state(cg);
}

/// Paints the pill bands, before the glyphs.
pub fn draw_inline_code_pills(
    fragment: &NSTextLayoutFragment,
    point: CGPoint,
    text_offset: CGFloat,
    style_sheet: &StyleSheet,
    cg: &CGContext,
) {
    let bands = inline_code_pill_bands(fragment, point, text_offset);
    if bands.is_empty() {
        return;
    }
    let cg = Some(cg);
    CGContext::save_g_state(cg);
    CGContext::set_fill_color_with_color(cg, Some(&style_sheet.inline_code_background.CGColor()));
    let edge: objc2::rc::Retained<NSColor> = style_sheet.code_rule.colorWithAlphaComponent(0.5);
    CGContext::set_stroke_color_with_color(cg, Some(&edge.CGColor()));
    CGContext::set_line_width(cg, 1.0);
    for band in bands {
        // SAFETY: a null transform is allowed.
        let path = unsafe {
            CGPath::with_rounded_rect(
                band.inset_by(0.5, 0.5),
                render_metrics::INLINE_CODE_CORNER_RADIUS,
                render_metrics::INLINE_CODE_CORNER_RADIUS,
                std::ptr::null(),
            )
        };
        CGContext::add_path(cg, Some(&path));
        CGContext::fill_path(cg);
        CGContext::add_path(cg, Some(&path));
        CGContext::stroke_path(cg);
    }
    CGContext::restore_g_state(cg);
}

/// `NSTextLineFragment.trailingEdge(after:past:in:bounds:)`: where the run
/// ending at `index` stops, stepping past zero-width hidden syntax to the
/// first character really to the right, else the line's right edge.
pub fn trailing_edge(line: &NSTextLineFragment, index: usize, leading: CGFloat, line_range: NSRange, bounds: CGRect) -> CGFloat {
    let mut probe = index;
    while probe <= line_range.location + line_range.length {
        let x = line.locationForCharacterAtIndex(probe as isize).x;
        if x > leading {
            return crate::swift_compat::smin(x, bounds.max_x());
        }
        probe += 1;
    }
    bounds.max_x()
}
