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
    NSAttributedStringNSStringDrawing, NSColor, NSCompositingOperation, NSFont, NSFontWeightBold, NSFontWeightMedium,
    NSFontWeightSemibold, NSImage, NSImageSymbolConfiguration, NSRectFillUsingOperation, NSTextElement,
    NSTextLayoutFragment, NSTextRange,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect};
use objc2_core_graphics::{CGContext, CGPath};
use objc2_foundation::{NSAttributedString, NSRect, NSString};

use crate::appkit_compat::{RectExt, attributed_string, keys, rect};
use crate::engine::render_metrics;
use crate::fragments::fragment_base::{
    DownrightFragment, FragmentBehavior, FragmentContext, RectCorners, draw_ns_image, draw_text, fill_gradient, fill_rect,
    fill_rect_corners,
};
use crate::render_contracts::FragmentPayload;
use crate::swift_compat::{smax, smin};
use crate::theme::style_sheet::{CodeLook, StyleSheet};
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
                    card_rect(band, &style),
                    &style.code_background,
                    code_radius(&style),
                    RectCorners::TOP_LEFT | RectCorners::TOP_RIGHT,
                );
                if header_bar(&style) {
                    self.draw_header_bar(fragment, band, &style, cg);
                } else {
                    self.draw_horizontal_edge(band, true, &style, cg);
                    self.draw_rule(band, &style, cg);
                    self.draw_chip(fragment, band, &style, cg);
                }
            }
            Role::CloseChrome => {
                fill_rect_corners(
                    cg,
                    card_rect(band, &style),
                    &style.code_background,
                    code_radius(&style),
                    RectCorners::BOTTOM_LEFT | RectCorners::BOTTOM_RIGHT,
                );
                // The edge spans the card the view shows, or its ends would
                // square off the rounded corners.
                self.draw_horizontal_edge(card_rect(band, &style), false, &style, cg);
                if !header_bar(&style) {
                    self.draw_rule(band, &style, cg);
                }
                // The closing fence carries a second copy control (§7.1).
                self.draw_copy_control(fragment, band, &style, "", cg);
            }
            Role::Body => {
                // Opaque, so it may reach a point into its neighbours: two
                // antialiased edges meeting part way through a pixel let the
                // page show through as a seam.
                let fill = if header_bar(&style) { rect(band.min_x(), band.min_y() - 1.0, band.width(), band.height() + 2.0) } else { band };
                fill_rect(cg, fill, &style.code_background, 0.0);
                if header_bar(&style) {
                    self.draw_body_extras(fragment, band, point, &style, cg);
                } else {
                    self.draw_rule(band, &style, cg);
                }
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The info string's language (its first word) and the file it names, if
/// any: a second word that looks like a path, or `title="…"`.
fn split_info(info: &str) -> (&str, Option<&str>) {
    let mut words = info.split_whitespace();
    let language = words.next().unwrap_or("");
    let file = words.find_map(|word| {
        let word = word.strip_prefix("title=").unwrap_or(word).trim_matches(|c| c == '"' || c == '\'');
        (!word.is_empty() && (word.contains('.') || word.contains('/'))).then_some(word)
    });
    (language, file)
}

/// The glyph beside a language's name.
fn language_symbol(language: &str) -> &'static str {
    match language.to_ascii_lowercase().as_str() {
        "sh" | "bash" | "zsh" | "fish" | "shell" | "console" | "terminal" => "terminal",
        "json" | "yaml" | "yml" | "toml" | "ini" | "plist" => "curlybraces",
        "diff" | "patch" => "plus.forwardslash.minus",
        "md" | "markdown" | "text" | "txt" => "text.alignleft",
        "sql" => "cylinder.split.1x2",
        "html" | "xml" | "svg" => "chevron.left.slash.chevron.right",
        _ => "chevron.left.forwardslash.chevron.right",
    }
}

/// Runs `draw` clipped to the card's rounded shape.
fn clipped_to_card(cg: &CGContext, card: CGRect, radius: CGFloat, draw: impl FnOnce()) {
    let context = Some(cg);
    CGContext::save_g_state(context);
    // SAFETY: a null transform is allowed.
    let path = unsafe { CGPath::with_rounded_rect(card, radius, radius, std::ptr::null()) };
    CGContext::add_path(context, Some(&path));
    CGContext::clip(context);
    draw();
    CGContext::restore_g_state(context);
}

/// `font` in italics at `size`, through its descriptor.
fn italic(font: &NSFont, size: CGFloat) -> Retained<NSFont> {
    use objc2_app_kit::NSFontDescriptorSymbolicTraits;
    let descriptor = font.fontDescriptor();
    let traits = descriptor.symbolicTraits() | NSFontDescriptorSymbolicTraits::TraitItalic;
    NSFont::fontWithDescriptor_size(&descriptor.fontDescriptorWithSymbolicTraits(traits), size)
        .unwrap_or_else(|| font.fontWithSize(size))
}

/// The card's corner radius: a terminal window's are tighter.
fn code_radius(style: &StyleSheet) -> CGFloat {
    if header_bar(style) && style.host.code_look == Some(CodeLook::Console) {
        6.0
    } else {
        render_metrics::CODE_CORNER_RADIUS
    }
}

/// Whether the host asked for the header bar (`HostTypography::code_header`).
fn header_bar(style: &StyleSheet) -> bool {
    style.host.code_header == Some(true)
}

/// The part of `band` the block shows: with the header bar, the band less
/// the code inset it reaches back past the view's leading edge, so the
/// card's leading corners round where they can be seen.
fn card_rect(band: CGRect, style: &StyleSheet) -> CGRect {
    if !header_bar(style) {
        return band;
    }
    let inset = render_metrics::CODE_INSET_X;
    rect(band.min_x() + inset, band.min_y(), smax(1.0, band.width() - inset), band.height())
}

impl CodeBlockFragment {
    /// The host's header bar (`HostTypography::code_header`), dressed by
    /// `HostTypography::code_look`: the language with its glyph at the
    /// leading edge, then the file the info string names; the line count and
    /// the copy control, always shown, at the trailing edge — the control a
    /// solid accent chip with a check once copied. While the stream is still
    /// writing the block, the bar carries an accent wash and says so.
    fn draw_header_bar(&self, fragment: &DownrightFragment, band: CGRect, style: &StyleSheet, cg: &CGContext) {
        let card = card_rect(band, style);
        let look = style.host.code_look.unwrap_or(CodeLook::Card);
        let radius = code_radius(style);
        let top = RectCorners::TOP_LEFT | RectCorners::TOP_RIGHT;
        match look {
            CodeLook::Card => {
                fill_rect_corners(cg, card, &style.code_rule.colorWithAlphaComponent(0.45), radius, top);
                fill_rect(cg, rect(card.min_x(), card.max_y() - 1.0, card.width(), 1.0), &style.code_rule, 0.0);
            }
            CodeLook::Ledger => {
                clipped_to_card(cg, card, radius, || {
                    fill_rect(cg, rect(card.min_x(), card.min_y(), card.width(), 2.0), &style.accent, 0.0);
                });
                fill_rect(
                    cg,
                    rect(card.min_x() + 14.0, card.max_y() - 1.0, smax(0.0, card.width() - 28.0), 1.0),
                    &style.accent.colorWithAlphaComponent(0.30),
                    0.0,
                );
            }
            CodeLook::Console => {
                fill_rect_corners(cg, card, &style.text.colorWithAlphaComponent(0.075), radius, top);
                fill_rect(cg, rect(card.min_x(), card.max_y() - 1.0, card.width(), 1.0), &style.text.colorWithAlphaComponent(0.10), 0.0);
            }
        }
        let streaming = self.is_streaming_block(fragment);
        if streaming {
            clipped_to_card(cg, card, radius, || {
                fill_gradient(
                    cg,
                    card,
                    0.0,
                    &style.accent,
                    (0.30, 0.0),
                    (CGPoint::new(card.min_x(), card.mid_y()), CGPoint::new(card.min_x() + card.width() * 0.6, card.mid_y())),
                );
            });
        }

        let (language, file) = split_info(&self.language);
        let mut x = card.min_x() + 12.0;
        if look == CodeLook::Console {
            use upleft_core::model::CalloutKind;
            for kind in [CalloutKind::Caution, CalloutKind::Important, CalloutKind::Tip] {
                fill_rect(cg, rect(x, card.mid_y() - 5.0, 10.0, 10.0), &style.callout_color(kind), 5.0);
                x += 16.0;
            }
            x += 6.0;
        }
        if !language.is_empty() {
            let glyph = symbol_image(language_symbol(language), None, 10.0, unsafe { NSFontWeightSemibold });
            let text = match look {
                CodeLook::Ledger => {
                    let font = style.body_font().fontWithSize(10.5);
                    let kern = objc2_foundation::NSNumber::new_f64(1.2);
                    attributed_string(
                        &language.to_uppercase(),
                        &[(keys::font(), &font), (keys::foreground_color(), &style.accent), (keys::kern(), &kern)],
                    )
                }
                _ => {
                    let font = style.mono_font(Some(10.5));
                    attributed_string(language, &[(keys::font(), &font), (keys::foreground_color(), &style.accent)])
                }
            };
            let glyph_width = glyph.as_ref().map_or(0.0, |image| image.size().width + 5.0);
            let width = text.size().width + glyph_width + 18.0;
            let pill = rect(x, card.mid_y() - 10.5, width, 21.0);
            if look != CodeLook::Ledger {
                fill_rect(cg, pill, &style.accent.colorWithAlphaComponent(0.16), 10.5);
            }
            let mut inner = pill.min_x() + if look == CodeLook::Ledger { 0.0 } else { 9.0 };
            if let Some(image) = glyph {
                let size = image.size();
                draw_ns_image(
                    &tinted(&image, &style.accent),
                    rect(inner, pill.mid_y() - size.height / 2.0, size.width, size.height),
                    cg,
                    0.0,
                );
                inner += size.width + 5.0;
            }
            let height = text.size().height;
            draw_text(cg, &text, rect(inner, pill.mid_y() - height / 2.0, text.size().width + 2.0, height), true);
            x = if look == CodeLook::Ledger { inner + text.size().width + 10.0 } else { pill.max_x() + 10.0 };
        }
        let copy = copy_button_rect(band, style, &self.language);
        if let Some(file) = file {
            let font = match look {
                CodeLook::Ledger => italic(&style.body_font(), 12.0),
                _ => style.mono_font(Some(11.0)),
            };
            let text = attributed_string(file, &[(keys::font(), &font), (keys::foreground_color(), &style.text)]);
            let height = text.size().height;
            let room = smax(0.0, copy.min_x() - 90.0 - x);
            draw_text(cg, &text, rect(x, card.mid_y() - height / 2.0, smin(text.size().width + 2.0, room), height), true);
        }
        let count = if streaming {
            "writing…".to_owned()
        } else if self.line_count == 1 {
            "1 line".to_owned()
        } else {
            format!("{} lines", self.line_count)
        };
        let count_color = if streaming { &style.accent } else { &style.text_secondary };
        let count_text =
            attributed_string(&count, &[(keys::font(), &style.mono_font(Some(10.0))), (keys::foreground_color(), count_color)]);
        let size = count_text.size();
        draw_text(
            cg,
            &count_text,
            rect(copy.min_x() - 8.0 - size.width, card.mid_y() - size.height / 2.0, size.width + 1.0, size.height),
            true,
        );

        let Some(context) = fragment.context() else { return };
        let source_range = fragment.payload().source_range();
        let hovered = context.hovered_fragment_range.get() == Some(source_range);
        let copied = context.copied_code_range.get() == Some(source_range);
        if copied {
            // The check pops out of a solid accent chip.
            fill_rect(cg, copy.inset_by(1.0, 1.0), &style.accent, 7.0);
        } else if hovered {
            fill_rect(cg, copy, &style.code_rule.colorWithAlphaComponent(0.8), 7.0);
        }
        let symbol = if copied { "checkmark" } else { "doc.on.doc" };
        let description = if copied { "Copied" } else { "Copy code" };
        let weight = if copied { unsafe { NSFontWeightBold } } else { unsafe { NSFontWeightMedium } };
        if let Some(image) = symbol_image(symbol, Some(description), if copied { 13.0 } else { 12.0 }, weight) {
            let icon_color = if copied { &style.background } else { &style.text_secondary };
            let size = image.size();
            draw_ns_image(
                &tinted(&image, icon_color),
                rect(copy.mid_x() - size.width / 2.0, copy.mid_y() - size.height / 2.0, size.width, size.height),
                cg,
                0.0,
            );
        }
    }

    /// Whether the stream is still writing this block: the view is
    /// streaming, the block runs to the end of the text, and no closing
    /// fence has arrived.
    fn is_streaming_block(&self, fragment: &DownrightFragment) -> bool {
        let Some(context) = fragment.context() else { return false };
        if !context.text_view().is_some_and(|view| view.is_streaming()) {
            return false;
        }
        let Some(storage) = context.storage() else { return false };
        let block = fragment.payload().source_range();
        if block.upper_bound() + 2 < storage.length() as isize {
            return false;
        }
        let text = fragment.source_text(block);
        let mut lines = text.trim_end().lines();
        let _opening = lines.next();
        !lines.last().is_some_and(|line| {
            let line = line.trim_start();
            line.starts_with("```") || line.starts_with("~~~")
        })
    }

    /// A body row under the header bar: line numbers in the inset's gutter
    /// (`HostTypography::code_line_numbers`), and a diff's whole-line bands
    /// with a coloured gutter bar.
    fn draw_body_extras(&self, fragment: &DownrightFragment, band: CGRect, point: CGPoint, style: &StyleSheet, cg: &CGContext) {
        let card = card_rect(band, style);
        let (language, _) = split_info(&self.language);
        if language.eq_ignore_ascii_case("diff") {
            let line = fragment.source_text(fragment.element_source_range());
            let tone = match line.chars().next() {
                Some('+') => Some((style.code_colors_diff(true), 0.14)),
                Some('-') => Some((style.code_colors_diff(false), 0.14)),
                Some('@') => Some((style.accent.clone(), 0.10)),
                _ => None,
            };
            if let Some((color, alpha)) = tone {
                fill_rect(cg, card, &color.colorWithAlphaComponent(alpha), 0.0);
                fill_rect(cg, rect(card.min_x(), card.min_y(), 3.0, card.height()), &color, 0.0);
            }
        }
        let Some(threshold) = style.host.code_line_numbers else { return };
        if self.line_count < threshold {
            return;
        }
        let Some(context) = fragment.context() else { return };
        let index = context.paragraph_index();
        let first = index.index_containing(fragment.payload().source_range().location) as isize;
        let here = index.index_containing(fragment.element_source_range().location) as isize;
        drop(index);
        let number = here - first;
        if !(number >= 1) {
            return;
        }
        let Some(line) = fragment.textLineFragments().firstObject() else { return };
        let bounds = line.typographicBounds();
        let text = attributed_string(
            &number.to_string(),
            &[(keys::font(), &style.mono_font(Some(9.5))), (keys::foreground_color(), &style.text_faint)],
        );
        let size = text.size();
        let right = card.min_x() + render_metrics::CODE_INSET_X - 7.0;
        draw_text(
            cg,
            &text,
            rect(right - size.width, point.y + bounds.origin.y + (bounds.height() - size.height) / 2.0 + 0.5, size.width + 1.0, size.height),
            true,
        );
    }

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
