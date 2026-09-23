//! Port of `Fragments/CodeBlockFragment.swift`: fenced code (§11.3), a subtle
//! tint plus a left rule, never a heavy bordered card. Language chip
//! top-right, copy button on hover (§7.1).
//!
//! The fence lines are not hidden — they become the band's chrome.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::any::Any;
use std::rc::Rc;

use block2::RcBlock;
use objc2::Message;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_app_kit::{
    NSAttributedStringNSStringDrawing, NSColor, NSCompositingOperation, NSFontWeightBold, NSFontWeightMedium,
    NSFontWeightSemibold, NSImage, NSImageSymbolConfiguration, NSRectFillUsingOperation, NSTextElement,
    NSTextLayoutFragment, NSTextRange,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect};
use objc2_core_graphics::CGContext;
use objc2_foundation::{NSAttributedString, NSRect, NSString};

use crate::appkit_compat::{RectExt, attributed_string, keys, rect};
use crate::engine::render_metrics;
use crate::fragments::fragment_base::{
    DownrightFragment, FragmentBehavior, FragmentContext, RectCorners, draw_ns_image, draw_text, fill_rect,
    fill_rect_corners,
};
use crate::render_contracts::FragmentPayload;
use crate::swift_compat::{smax, smin};
use crate::theme::style_sheet::StyleSheet;
use crate::view::fragment_provider::CodeBlockRole as Role;

/// `CodeBlockFragment`'s stored properties and hooks.
pub struct CodeBlockFragment {
    pub role: Role,
    pub language: String,
    pub line_count: isize,
}

/// `CodeBlockFragment(textElement:range:payload:context:role:lineCount:)`.
pub fn make(
    text_element: &NSTextElement,
    range: Option<&NSTextRange>,
    payload: &FragmentPayload,
    context: &Rc<FragmentContext>,
    role: Role,
    line_count: isize,
) -> Retained<NSTextLayoutFragment> {
    let behavior = CodeBlockFragment { role, language: payload.detail().to_owned(), line_count };
    Retained::into_super(DownrightFragment::new(
        c"CodeBlockFragment",
        text_element,
        range,
        payload,
        context,
        Box::new(behavior),
    ))
}

/// `CodeBlockFragment.copyControlSide`: a 28pt square, the smallest pointer
/// target that does not need aiming.
pub const COPY_CONTROL_SIDE: CGFloat = 28.0;

/// `CodeBlockFragment.chipText(_:style:)`.
pub fn chip_text(string: &str, style: &StyleSheet) -> Retained<NSAttributedString> {
    let font = style.mono_font(Some(10.0));
    attributed_string(string, &[(keys::font(), &font), (keys::foreground_color(), &style.text_secondary)])
}

/// `CodeBlockFragment.chipRect(in:style:language:)`.
pub fn chip_rect(band: CGRect, style: &StyleSheet, language: &str) -> CGRect {
    let width = chip_text(language, style).size().width + 12.0;
    // Vertically centred in the header row (§11.3).
    let y = band.min_y() + smax(0.0, (band.height() - 17.0) / 2.0);
    rect(band.max_x() - width - render_metrics::CODE_INSET_X, y, width, 17.0)
}

/// `CodeBlockFragment.copyButtonRect(in:style:language:)`: clamped to the
/// band so the control never paints outside the fragment's frame.
pub fn copy_button_rect(band: CGRect, _style: &StyleSheet, _language: &str) -> CGRect {
    let side = smin(COPY_CONTROL_SIDE, smax(0.0, band.height()));
    let trailing = band.max_x() - render_metrics::CODE_INSET_X;
    rect(trailing - side, band.mid_y() - side / 2.0, side, side)
}

impl FragmentBehavior for CodeBlockFragment {
    fn suppresses_text(&self, _fragment: &DownrightFragment) -> bool {
        match self.role {
            Role::OpenChrome | Role::CloseChrome | Role::CollapsedChip => true,
            Role::Body => false,
        }
    }

    /// The chrome rows claim their band plus `codeBlockGap` of page
    /// background on the block's outer edge.
    fn override_height(&self, _fragment: &DownrightFragment) -> Option<CGFloat> {
        match self.role {
            Role::OpenChrome => Some(render_metrics::CODE_HEADER_HEIGHT + render_metrics::CODE_BLOCK_GAP),
            Role::CloseChrome => Some(render_metrics::CODE_INSET_Y + render_metrics::CODE_BLOCK_GAP),
            Role::CollapsedChip => Some(render_metrics::CHIP_HEIGHT),
            Role::Body => None,
        }
    }

    /// The band paints exactly its own frame.
    fn draw_object(&self, fragment: &DownrightFragment, point: CGPoint, cg: &CGContext) {
        let Some(style) = fragment.style_sheet() else { return };
        let band = self.band_rect(fragment, point);

        match self.role {
            Role::CollapsedChip => self.draw_collapsed_chip(band, &style, cg),
            Role::OpenChrome => {
                // Rounded top edge; square bottom butting the first code line.
                fill_rect_corners(
                    cg,
                    band,
                    &style.code_background,
                    render_metrics::CODE_CORNER_RADIUS,
                    RectCorners::TOP_LEFT | RectCorners::TOP_RIGHT,
                );
                self.draw_horizontal_edge(band, true, &style, cg);
                self.draw_rule(band, &style, cg);
                self.draw_chip(fragment, band, &style, cg);
            }
            Role::CloseChrome => {
                fill_rect_corners(
                    cg,
                    band,
                    &style.code_background,
                    render_metrics::CODE_CORNER_RADIUS,
                    RectCorners::BOTTOM_LEFT | RectCorners::BOTTOM_RIGHT,
                );
                self.draw_horizontal_edge(band, false, &style, cg);
                self.draw_rule(band, &style, cg);
                // The closing fence carries a second copy control (§7.1).
                self.draw_copy_control(fragment, band, &style, "", cg);
            }
            Role::Body => {
                fill_rect(cg, band, &style.code_background, 0.0);
                self.draw_rule(band, &style, cg);
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl CodeBlockFragment {
    /// Untinted page background this fragment reserves, and where it sits.
    fn outer_gap(&self) -> (CGFloat, CGFloat) {
        match self.role {
            Role::OpenChrome => (render_metrics::CODE_BLOCK_GAP, 0.0),
            Role::CloseChrome => (0.0, render_metrics::CODE_BLOCK_GAP),
            Role::Body | Role::CollapsedChip => (0.0, 0.0),
        }
    }

    /// A one-pixel optical edge on the outer fragments only.
    fn draw_horizontal_edge(&self, band: CGRect, at_top: bool, style: &StyleSheet, cg: &CGContext) {
        let y = if at_top { band.min_y() + 0.5 } else { band.max_y() - 1.0 };
        let edge = if at_top {
            style.surface.colorWithAlphaComponent(0.72)
        } else {
            style.text.colorWithAlphaComponent(0.10)
        };
        fill_rect(
            cg,
            rect(
                band.min_x() + render_metrics::CODE_CORNER_RADIUS,
                y,
                smax(0.0, band.width() - render_metrics::CODE_CORNER_RADIUS * 2.0),
                1.0,
            ),
            &edge,
            0.0,
        );
    }

    fn draw_rule(&self, band: CGRect, style: &StyleSheet, cg: &CGContext) {
        let top_inset: CGFloat = if self.role == Role::OpenChrome { 4.0 } else { 0.0 };
        let bottom_inset: CGFloat = if self.role == Role::CloseChrome { 4.0 } else { 0.0 };
        fill_rect(
            cg,
            rect(
                band.min_x(),
                band.min_y() + top_inset,
                render_metrics::CODE_RULE_WIDTH,
                smax(1.0, band.height() - top_inset - bottom_inset),
            ),
            &style.code_rule,
            render_metrics::CODE_RULE_WIDTH / 2.0,
        );
    }

    fn draw_chip(&self, fragment: &DownrightFragment, band: CGRect, style: &StyleSheet, cg: &CGContext) {
        let is_hovered = fragment
            .context()
            .is_some_and(|context| context.hovered_fragment_range.get() == Some(fragment.payload().source_range()));
        if !self.language.is_empty() && !is_hovered {
            let chip = chip_rect(band, style, &self.language);
            fill_rect(cg, chip, &style.code_rule.colorWithAlphaComponent(0.22), 4.0);
            draw_text(cg, &chip_text(&self.language, style), chip.inset_by(6.0, 2.0), true);
        }
        self.draw_copy_control(fragment, band, style, &self.language, cg);
    }

    fn draw_copy_control(
        &self,
        fragment: &DownrightFragment,
        band: CGRect,
        style: &StyleSheet,
        language: &str,
        cg: &CGContext,
    ) {
        let Some(context) = fragment.context() else { return };
        let source_range = fragment.payload().source_range();
        if context.hovered_fragment_range.get() != Some(source_range) {
            return;
        }
        let copy = copy_button_rect(band, style, language);
        let copied = context.copied_code_range.get() == Some(source_range);
        let bg_color = if copied {
            style.accent.colorWithAlphaComponent(0.20)
        } else {
            style.code_rule.colorWithAlphaComponent(0.22)
        };
        fill_rect(cg, copy, &bg_color, 6.0);
        let symbol = if copied { "checkmark" } else { "doc.on.doc" };
        let description = if copied { "Copied" } else { "Copy code" };
        let weight = if copied { unsafe { NSFontWeightBold } } else { unsafe { NSFontWeightMedium } };
        if let Some(image) = symbol_image(symbol, Some(description), 12.0, weight) {
            let icon_color = if copied { &style.accent } else { &style.text_secondary };
            let tinted = tinted(&image, icon_color);
            draw_ns_image(&tinted, copy.inset_by(8.0, 8.0), cg, 0.0);
        }
    }

    fn draw_collapsed_chip(&self, band: CGRect, style: &StyleSheet, cg: &CGContext) {
        let chip = rect(band.min_x(), band.min_y() + 2.0, band.width(), band.height() - 4.0);
        fill_rect(cg, chip, &style.code_background, render_metrics::CODE_CORNER_RADIUS);
        fill_rect(
            cg,
            rect(chip.min_x(), chip.min_y(), render_metrics::CODE_RULE_WIDTH, chip.height()),
            &style.code_rule,
            0.0,
        );

        let label = if self.language.is_empty() { "code" } else { self.language.as_str() };
        let text = format!("{label} · {} lines", self.line_count);
        let font = style.mono_font(Some(11.0));
        let attributed =
            attributed_string(&text, &[(keys::font(), &font), (keys::foreground_color(), &style.text_secondary)]);
        draw_text(cg, &attributed, chip.inset_by(render_metrics::CODE_INSET_X, 6.0), true);
        if let Some(triangle) = symbol_image("chevron.right", Some("Expand code"), 9.0, unsafe { NSFontWeightSemibold }) {
            draw_ns_image(
                &tinted(&triangle, &style.text_secondary),
                rect(chip.min_x() + 7.0, chip.mid_y() - 5.0, 10.0, 10.0),
                cg,
                0.0,
            );
        }
    }

    /// The tinted band, inset to the block's own indentation so a code block
    /// inside a list stays inside the list.
    pub fn band_rect(&self, fragment: &DownrightFragment, point: CGPoint) -> CGRect {
        // `firstLineHeadIndent`, not `headIndent`: wrapped code rows hang by
        // a continuation indent.
        let indent = smax(
            0.0,
            fragment.paragraph_style().map_or(0.0, |style| style.firstLineHeadIndent()) - render_metrics::CODE_INSET_X,
        );
        let gap = self.outer_gap();
        let frame = fragment.layoutFragmentFrame();
        // Back out the rendering surface's code inset and the paragraph's,
        // then restore only the block's structural indent.
        rect(
            point.x + indent - render_metrics::CODE_INSET_X * 2.0,
            point.y + gap.0,
            smax(1.0, fragment.content_width() - indent),
            smax(1.0, frame.height() - gap.0 - gap.1),
        )
    }
}

/// `NSImage(systemSymbolName:accessibilityDescription:)?
/// .withSymbolConfiguration(.init(pointSize:weight:))`.
pub fn symbol_image(
    name: &str,
    description: Option<&str>,
    point_size: CGFloat,
    weight: objc2_app_kit::NSFontWeight,
) -> Option<Retained<NSImage>> {
    let description = description.map(NSString::from_str);
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(name), description.as_deref())?;
    let configuration = NSImageSymbolConfiguration::configurationWithPointSize_weight(point_size, weight);
    image.imageWithSymbolConfiguration(&configuration)
}

/// The private `NSImage.tinted(_:)`: the image drawn, then `color` filled
/// over it with `.sourceAtop`, in a drawing-handler image of the same size.
pub fn tinted(image: &NSImage, color: &NSColor) -> Retained<NSImage> {
    let source: Retained<NSImage> = image.retain();
    let color: Retained<NSColor> = color.retain();
    let handler = RcBlock::new(move |bounds: NSRect| -> Bool {
        source.drawInRect(bounds);
        color.set();
        NSRectFillUsingOperation(bounds, NSCompositingOperation::SourceAtop);
        Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(image.size(), false, &handler)
}
