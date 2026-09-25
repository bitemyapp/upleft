//! Port of `Fragments/CalloutFragment.swift`: quotes and callouts (§11.3), a
//! coloured left rule plus an SF Symbol icon, with a restrained tint.
//!
//! Unlike the object fragments this one keeps its glyphs; it only adds the
//! rule and the icon underneath them. One callout is several fragments, each
//! drawing its own slice of the same shape: the band comes from the block's
//! own head indent, only the true top and bottom are rounded or inset, and
//! the header belongs to exactly one element.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::any::Any;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2_app_kit::{
    NSAttributedStringNSStringDrawing, NSColor, NSFont, NSFontWeightBold, NSFontWeightMedium, NSFontWeightSemibold, NSImage,
    NSImageSymbolConfiguration, NSParagraphStyle, NSTextElement, NSTextLayoutFragment, NSTextRange,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect};
use objc2_core_graphics::{CGContext, CGPath};
use objc2_foundation::NSString;
use upleft_core::model::CalloutKind;

use crate::appkit_compat::{RectExt, attribute_value, attributed_string, intersection_range, keys, rect};
use crate::engine::render_metrics;
use crate::fragments::code_block_fragment::tinted;
use crate::fragments::fragment_base::{
    DownrightFragment, FragmentBehavior, FragmentContext, draw_ns_image, draw_text, fill_gradient, fill_rect, stroke_rect,
};
use crate::render_contracts::{FragmentPayload, ThemeAppearance};
use crate::swift_compat::{is_whitespace_or_newline, smax, smin};
use crate::theme::style_sheet::StyleSheet;

/// `CalloutFragment`'s stored properties and hooks.
pub struct CalloutFragment {
    kind: Option<CalloutKind>,
    title: String,
}

/// `CalloutFragment(textElement:range:payload:context:)`.
pub fn make(
    text_element: &NSTextElement,
    range: Option<&NSTextRange>,
    payload: &FragmentPayload,
    context: &Rc<FragmentContext>,
) -> Retained<NSTextLayoutFragment> {
    let parts = upleft_swift_text::split(payload.detail(), '|', 1, false);
    let kind = parts.first().and_then(|token| CalloutKind::from_token(token));
    let title = if parts.len() > 1 { parts[1].to_owned() } else { String::new() };
    Retained::into_super(DownrightFragment::new(
        c"CalloutFragment",
        text_element,
        range,
        payload,
        context,
        Box::new(CalloutFragment { kind, title }),
    ))
}

impl FragmentBehavior for CalloutFragment {
    fn vertical_padding(&self, fragment: &DownrightFragment) -> (CGFloat, CGFloat) {
        if self.kind.is_none() {
            return (0.0, 0.0);
        }
        // No reserved header row: the hidden `> [!KIND] …` line is the header.
        (
            if is_header_element(fragment) { render_metrics::CALLOUT_INSET_Y } else { 0.0 },
            if is_footer_element(fragment) { render_metrics::CALLOUT_INSET_Y } else { 0.0 },
        )
    }

    fn draw_object(&self, fragment: &DownrightFragment, point: CGPoint, cg: &CGContext) {
        let Some(style) = fragment.style_sheet() else { return };
        let reserved_inset =
            if self.kind.is_none() { render_metrics::CALLOUT_INSET_X } else { render_metrics::CALLOUT_ICON_INSET_X };
        let color = self.kind.map_or_else(|| style.quote_rule.clone(), |kind| style.callout_color(kind));
        let card = self.kind.is_some() && style.host.callout_card == Some(true);
        let rule_width = if self.kind.is_none() {
            render_metrics::QUOTE_RULE_WIDTH
        } else if card {
            CARD_SPINE_WIDTH
        } else {
            render_metrics::CALLOUT_RULE_WIDTH
        };
        let band = band_rect(fragment, point, reserved_inset);

        if card {
            self.draw_card(fragment, band, &color, &style, cg);
        } else if self.kind.is_some() {
            let tint_alpha: CGFloat = if style.theme.appearance == ThemeAppearance::Light { 0.042 } else { 0.060 };
            fill_rect(
                cg,
                continuous(fragment, band),
                &color.colorWithAlphaComponent(tint_alpha),
                render_metrics::CALLOUT_CORNER_RADIUS,
            );
        }
        if !card {
            fill_rect(cg, self.rule_rect(fragment, band, rule_width), &color, rule_width / 2.0);
        }

        let Some(kind) = self.kind else { return };
        if !(is_header_element(fragment) && header_row_is_blank(fragment)) {
            return;
        }
        self.draw_header(fragment, kind, band, reserved_inset, rule_width, &color, point, &style, cg);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// MARK: - Which slice of the block this is

/// The element carrying the `> [!KIND]` line.
fn is_header_element(fragment: &DownrightFragment) -> bool {
    fragment.element_source_range().location <= fragment.payload().source_range().location
}

fn is_footer_element(fragment: &DownrightFragment) -> bool {
    fragment.element_source_range().upper_bound() >= fragment.payload().source_range().upper_bound()
}

// MARK: - Drawing

/// `bandRect(at:reservedInset:)`: the tinted band and its rule, anchored on
/// the callout's own indentation, relative to `point`.
pub fn band_rect(fragment: &DownrightFragment, point: CGPoint, reserved_inset: CGFloat) -> CGRect {
    let indent = smax(0.0, block_head_indent(fragment).unwrap_or(reserved_inset) - reserved_inset);
    let row_indent = fragment
        .paragraph_style()
        .map(|style| style.headIndent())
        .or_else(|| block_head_indent(fragment))
        .unwrap_or(reserved_inset);
    let block_indent = block_head_indent(fragment).unwrap_or(reserved_inset);
    // A callout stands beside prose: its band stops at the reading column.
    rect(
        point.x + (block_indent - row_indent) - reserved_inset,
        point.y,
        smax(1.0, fragment.prose_content_width() - indent),
        fragment.layoutFragmentFrame().height(),
    )
}

/// Head indent of the callout's own first paragraph.
fn block_head_indent(fragment: &DownrightFragment) -> Option<CGFloat> {
    let storage = fragment.context()?.storage()?;
    let location = fragment.payload().source_range().location;
    if !(location >= 0 && location < storage.length() as isize) {
        return None;
    }
    let style = attribute_value(&storage, keys::paragraph_style(), location as usize)?;
    style.downcast::<NSParagraphStyle>().ok().map(|style| style.headIndent())
}

/// The band grown into its neighbours wherever the block continues.
fn continuous(fragment: &DownrightFragment, band: CGRect) -> CGRect {
    let radius = render_metrics::CALLOUT_CORNER_RADIUS;
    let mut grown = band;
    if !is_header_element(fragment) {
        grown.origin.y -= radius;
        grown.size.height += radius;
    }
    if !is_footer_element(fragment) {
        grown.size.height += radius;
    }
    grown
}

/// The card's spine (`HostTypography::callout_card`).
const CARD_SPINE_WIDTH: CGFloat = 4.0;
/// The card's icon badge: as large as the reserved lane allows.
const CARD_BADGE_SIDE: CGFloat = 22.0;

impl CalloutFragment {
    /// The host's card (`HostTypography::callout_card`): the kind's colour
    /// as a tint fading across the card, a hairline edge in the colour, a
    /// spine following the card's rounded corners, and in dark mode a lit
    /// top edge. Each slice draws the whole card grown past its own ends,
    /// and its surface clips it, so the slices join without a seam.
    fn draw_card(&self, fragment: &DownrightFragment, band: CGRect, color: &NSColor, style: &StyleSheet, cg: &CGContext) {
        let dark = style.theme.appearance != ThemeAppearance::Light;
        let radius = render_metrics::CALLOUT_CORNER_RADIUS;
        let card = continuous(fragment, band);
        let (from, to): (CGFloat, CGFloat) = if dark { (0.20, 0.05) } else { (0.13, 0.03) };
        fill_gradient(
            cg,
            card,
            radius,
            color,
            (from, to),
            (CGPoint::new(card.min_x(), card.mid_y()), CGPoint::new(card.max_x(), card.mid_y())),
        );
        stroke_rect(cg, card, &color.colorWithAlphaComponent(if dark { 0.30 } else { 0.24 }), radius, 1.0);
        let context = Some(cg);
        CGContext::save_g_state(context);
        // SAFETY: a null transform is allowed.
        let path = unsafe { CGPath::with_rounded_rect(card, radius, radius, std::ptr::null()) };
        CGContext::add_path(context, Some(&path));
        CGContext::clip(context);
        fill_rect(cg, rect(card.min_x(), card.min_y(), CARD_SPINE_WIDTH, card.height()), color, 0.0);
        CGContext::restore_g_state(context);
        if dark && is_header_element(fragment) {
            fill_rect(
                cg,
                rect(card.min_x() + radius, card.min_y() + 1.0, smax(0.0, card.width() - radius * 2.0), 1.0),
                &color.colorWithAlphaComponent(0.42),
                0.0,
            );
        }
    }

    /// One stroke down the whole callout.
    fn rule_rect(&self, fragment: &DownrightFragment, band: CGRect, width: CGFloat) -> CGRect {
        let end: CGFloat = if self.kind.is_none() { 0.0 } else { 4.0 };
        let top = if is_header_element(fragment) { end } else { 0.0 };
        let bottom = if is_footer_element(fragment) { end } else { 0.0 };
        rect(band.min_x(), band.min_y() + top, width, smax(1.0, band.height() - top - bottom))
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_header(
        &self,
        fragment: &DownrightFragment,
        kind: CalloutKind,
        band: CGRect,
        reserved_inset: CGFloat,
        rule_width: CGFloat,
        color: &NSColor,
        point: CGPoint,
        style: &StyleSheet,
        cg: &CGContext,
    ) {
        let row_y = point.y + render_metrics::CALLOUT_INSET_Y;
        let row_height = fragment
            .textLineFragments()
            .firstObject()
            .map_or(style.line_height, |line| smax(1.0, line.typographicBounds().height()));

        let card = style.host.callout_card == Some(true);
        if card {
            // A solid badge in the kind's colour with the icon knocked out.
            let badge = rect(
                band.min_x() + rule_width + 4.0,
                row_y + (row_height - CARD_BADGE_SIDE) / 2.0,
                CARD_BADGE_SIDE,
                CARD_BADGE_SIDE,
            );
            fill_rect(cg, badge, color, CARD_BADGE_SIDE / 2.0);
            if let Some(icon) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str(style.callout_symbol(kind)),
                Some(&NSString::from_str(default_label(kind))),
            ) {
                let configuration =
                    NSImageSymbolConfiguration::configurationWithPointSize_weight(11.0, unsafe { NSFontWeightBold });
                let configured = icon.imageWithSymbolConfiguration(&configuration).unwrap_or(icon);
                let size = configured.size();
                draw_ns_image(
                    &tinted(&configured, &style.background),
                    rect(badge.mid_x() - size.width / 2.0, badge.mid_y() - size.height / 2.0, size.width, size.height),
                    cg,
                    0.0,
                );
            }
        } else if let Some(icon) =
            NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(style.callout_symbol(kind)), None)
        {
            let configuration = NSImageSymbolConfiguration::configurationWithPointSize_weight(
                style.body_font().pointSize(),
                unsafe { NSFontWeightMedium },
            );
            let configured = icon.imageWithSymbolConfiguration(&configuration).unwrap_or(icon);
            let tinted = tinted(&configured, color);
            let size = configured.size();
            draw_ns_image(
                &tinted,
                rect(
                    band.min_x() + rule_width + 7.0,
                    row_y + smax(0.0, (row_height - size.height) / 2.0),
                    size.width,
                    size.height,
                ),
                cg,
                0.0,
            );
        }

        // An untitled callout gets its kind's name.
        let text = if self.title.is_empty() { default_label(kind) } else { self.title.as_str() };
        let label = if card && self.title.is_empty() {
            // The kind's own name, as tracked capitals: a label, not a heading.
            let font = NSFont::systemFontOfSize_weight(style.body_font().pointSize() * 0.76, unsafe { NSFontWeightBold });
            let kern = objc2_foundation::NSNumber::new_f64(1.1);
            attributed_string(
                &text.to_uppercase(),
                &[(keys::font(), &font), (keys::foreground_color(), color), (keys::kern(), &kern)],
            )
        } else {
            let weight = if card { unsafe { NSFontWeightBold } } else { unsafe { NSFontWeightSemibold } };
            let font = NSFont::systemFontOfSize_weight(style.body_font().pointSize() * 0.94, weight);
            attributed_string(text, &[(keys::font(), &font), (keys::foreground_color(), color)])
        };
        let label_height = smin(row_height, label.size().height.ceil());
        draw_text(
            cg,
            &label,
            rect(
                band.min_x() + reserved_inset,
                row_y + smax(0.0, (row_height - label_height) / 2.0),
                smax(40.0, band.width() - reserved_inset),
                label_height,
            ),
            true,
        );
    }
}

/// True while the header row really is the blank remains of hidden marker
/// syntax; a scoped source lens puts the marker back as real text.
fn header_row_is_blank(fragment: &DownrightFragment) -> bool {
    let focused = fragment
        .context()
        .map(|context| context.is_source_focused(fragment.payload().source_range()));
    if focused == Some(true) {
        return false;
    }
    let Some(line) = fragment.textLineFragments().firstObject() else { return false };
    let text = line.attributedString().string();
    let range = intersection_range(line.characterRange(), objc2_foundation::NSRange::new(0, text.length()));
    if range.length == 0 {
        return true;
    }
    // `text.substring(with:)` as a Swift `String`: a lone surrogate becomes
    // U+FFFD, which is not blank.
    let units: Vec<u16> = (range.location..range.location + range.length).map(|index| text.characterAtIndex(index)).collect();
    char::decode_utf16(units.iter().copied())
        .map(|scalar| scalar.unwrap_or(char::REPLACEMENT_CHARACTER))
        .all(is_blank)
}

/// Hidden runs survive layout as word joiners and reflowed breaks as spaces,
/// so "blank" means "no ink".
fn is_blank(scalar: char) -> bool {
    scalar == '\u{2060}' || scalar == '\u{200B}' || scalar == '\u{FEFF}' || is_whitespace_or_newline(scalar)
}

/// The name a callout carries when its author gave it none.
fn default_label(kind: CalloutKind) -> &'static str {
    match kind {
        CalloutKind::Note => "Note",
        CalloutKind::Tip => "Tip",
        CalloutKind::Important => "Important",
        CalloutKind::Warning => "Warning",
        CalloutKind::Caution => "Caution",
        CalloutKind::Info => "Info",
        CalloutKind::Success => "Success",
        CalloutKind::Question => "Question",
        CalloutKind::Danger => "Danger",
        CalloutKind::Example => "Example",
        CalloutKind::Quote => "Quote",
        CalloutKind::Abstract => "Abstract",
        CalloutKind::Bug => "Bug",
        CalloutKind::Todo => "To do",
    }
}
