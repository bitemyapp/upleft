//! Port of `Fragments/FrontMatterFragment.swift`: YAML front matter as a
//! compact metadata card, not a code block (§5.1). In Live mode with the
//! caret inside, the provider hands back a plain fragment instead.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::any::Any;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2_app_kit::{
    NSAttributedStringNSExtendedStringDrawing, NSAttributedStringNSStringDrawing, NSFont, NSFontWeightMedium,
    NSFontWeightSemibold, NSStringDrawingOptions, NSTextElement, NSTextLayoutFragment, NSTextRange,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGSize};
use objc2_core_graphics::{CGContext, CGPath};

use crate::appkit_compat::{RectExt, attributed_string, attributes_dictionary, keys, rect, string_bounding_rect};
use crate::engine::render_metrics;
use crate::fragments::fragment_base::{DownrightFragment, FragmentBehavior, FragmentContext, draw_text, fill_rect};
use crate::render_contracts::FragmentPayload;
use crate::swift_compat::{smax, smin, split_on_whitespace_characters};

const HEADER_HEIGHT: CGFloat = 25.0;
const HORIZONTAL_INSET: CGFloat = 10.0;

/// `FrontMatterFragment`'s stored properties and hooks.
pub struct FrontMatterFragment {
    fields: Vec<(String, String)>,
}

/// `FrontMatterFragment(textElement:range:payload:context:fields:)`.
pub fn make(
    text_element: &NSTextElement,
    range: Option<&NSTextRange>,
    payload: &FragmentPayload,
    context: &Rc<FragmentContext>,
    fields: Vec<(String, String)>,
) -> Retained<NSTextLayoutFragment> {
    Retained::into_super(DownrightFragment::new(
        c"FrontMatterFragment",
        text_element,
        range,
        payload,
        context,
        Box::new(FrontMatterFragment { fields }),
    ))
}

/// Interior YAML newlines become prose spaces in the presentation layer.
fn single_line(value: &str) -> String {
    let collapsed = upleft_swift_text::replacing_occurrences(value, "\n", " ");
    let squashed = split_on_whitespace_characters(&collapsed).join(" ");
    if squashed.is_empty() { upleft_swift_text::trim_whitespaces(value).to_owned() } else { squashed }
}

impl FragmentBehavior for FrontMatterFragment {
    fn suppresses_text(&self, _fragment: &DownrightFragment) -> bool {
        true
    }

    fn override_height(&self, fragment: &DownrightFragment) -> Option<CGFloat> {
        if !fragment.is_first_paragraph_of_block() {
            return Some(0.0);
        }
        let Some(style) = fragment.style_sheet() else { return Some(0.0) };
        if self.fields.is_empty() {
            return Some(0.0);
        }
        let prose_width = card_width(fragment);
        let key_width = smin(112.0, smax(64.0, prose_width * 0.20));
        let value_width = smax(80.0, prose_width - key_width - 18.0 - HORIZONTAL_INSET);
        let font = style.body_font().fontWithSize(style.body_font().pointSize() * 0.86);
        let font_attributes = attributes_dictionary(&[(keys::font(), &font)]);
        let height = self.fields.iter().fold(0.0, |partial, (_, value)| {
            let bounds = string_bounding_rect(&single_line(value), CGSize::new(value_width, CGFloat::MAX), &font_attributes);
            partial + smax(style.line_height, bounds.height().ceil() + 6.0)
        });
        Some(render_metrics::snap_up(height + HEADER_HEIGHT + 10.0, smax(1.0, style.baseline_grid)))
    }

    fn draw_object(&self, fragment: &DownrightFragment, point: CGPoint, cg: &CGContext) {
        if !fragment.is_first_paragraph_of_block() {
            return;
        }
        let Some(style) = fragment.style_sheet() else { return };
        if self.fields.is_empty() {
            return;
        }
        let card = rect(point.x, point.y, card_width(fragment), fragment.layoutFragmentFrame().height());
        let key_width = smin(112.0, smax(64.0, card.width() * 0.20));
        let value_x = card.min_x() + key_width + 18.0;
        let value_width = smax(80.0, card.max_x() - value_x - HORIZONTAL_INSET);
        let font = style.body_font().fontWithSize(style.body_font().pointSize() * 0.86);
        let key_font = NSFont::systemFontOfSize_weight(font.pointSize() * 0.90, unsafe { NSFontWeightMedium });
        let is_hovered = fragment
            .context()
            .is_some_and(|context| context.hovered_fragment_range.get() == Some(fragment.payload().source_range()));
        fill_rect(
            cg,
            card.inset_by(1.0, 1.0),
            &(if is_hovered { &style.accent } else { &style.surface })
                .colorWithAlphaComponent(if is_hovered { 0.12 } else { 0.26 }),
            10.0,
        );
        let context = Some(cg);
        CGContext::set_stroke_color_with_color(
            context,
            Some(&style.rule.colorWithAlphaComponent(if is_hovered { 0.8 } else { 0.45 }).CGColor()),
        );
        CGContext::set_line_width(context, 1.0);
        // SAFETY: a null transform is allowed.
        let path = unsafe { CGPath::with_rounded_rect(card.inset_by(0.5, 0.5), 10.0, 10.0, std::ptr::null()) };
        CGContext::add_path(context, Some(&path));
        CGContext::stroke_path(context);
        fill_rect(
            cg,
            rect(card.min_x(), card.min_y() + 8.0, 3.0, smax(1.0, card.height() - 16.0)),
            &style.accent,
            1.5,
        );

        let header_font =
            NSFont::systemFontOfSize_weight(smax(9.0, font.pointSize() * 0.72), unsafe { NSFontWeightSemibold });
        let header = attributed_string(
            "Metadata",
            &[(keys::font(), &header_font), (keys::foreground_color(), &style.text_faint)],
        );
        draw_text(
            cg,
            &header,
            rect(
                card.min_x() + HORIZONTAL_INSET,
                card.min_y() + 5.0,
                smax(1.0, card.width() / 2.0 - HORIZONTAL_INSET),
                14.0,
            ),
            true,
        );

        let edit_title = if card.width() < 180.0 { "Edit" } else { "Edit metadata" };
        let edit = attributed_string(
            edit_title,
            &[
                (keys::font(), &header_font),
                (keys::foreground_color(), if is_hovered { &style.accent } else { &style.text_faint }),
            ],
        );
        let edit_width = edit.size().width.ceil();
        draw_text(
            cg,
            &edit,
            rect(card.max_x() - HORIZONTAL_INSET - edit_width, card.min_y() + 5.0, edit_width, 14.0),
            true,
        );
        fill_rect(
            cg,
            rect(
                card.min_x() + HORIZONTAL_INSET,
                card.min_y() + HEADER_HEIGHT - 1.0,
                smax(1.0, card.width() - 2.0 * HORIZONTAL_INSET),
                1.0,
            ),
            &style.rule.colorWithAlphaComponent(0.35),
            0.0,
        );

        let mut y = card.min_y() + HEADER_HEIGHT + 2.0;
        for (key, value) in &self.fields {
            let value = attributed_string(
                &single_line(value),
                &[(keys::font(), &font), (keys::foreground_color(), &style.text_secondary)],
            );
            let value_height = smax(
                style.line_height,
                value
                    .boundingRectWithSize_options_context(
                        CGSize::new(value_width, CGFloat::MAX),
                        NSStringDrawingOptions::UsesLineFragmentOrigin | NSStringDrawingOptions::UsesFontLeading,
                        None,
                    )
                    .height()
                    .ceil()
                    + 6.0,
            );
            let key = attributed_string(key, &[(keys::font(), &key_font), (keys::foreground_color(), &style.text_faint)]);
            draw_text(
                cg,
                &key,
                rect(
                    card.min_x() + smax(0.0, key_width - key.size().width),
                    y,
                    smin(key_width, key.size().width),
                    style.line_height,
                ),
                true,
            );
            draw_text(cg, &value, rect(value_x, y, value_width, value_height), true);
            y += value_height;
        }
        fill_rect(cg, rect(card.min_x(), card.max_y() - 1.0, card.width(), 1.0), &style.rule, 0.0);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::single_line;

    #[test]
    fn single_line_collapses_whitespace() {
        assert_eq!(single_line("a\n  b\tc"), "a b c");
        assert_eq!(single_line("   "), "");
        // `.whitespaces` does not trim newlines.
        assert_eq!(single_line("\n"), "\n");
    }
}

/// The card spans the prose measure from the fragment's code inset. With
/// Downright's bleed lane that always fits the column; a host that narrows
/// the lane (`HostTypography::code_bleed`) keeps it inside the column.
fn card_width(fragment: &DownrightFragment) -> CGFloat {
    smin(fragment.prose_content_width(), smax(1.0, fragment.content_width() - render_metrics::CODE_INSET_X))
}
