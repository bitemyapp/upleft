//! Port of `View/FootnoteMarginView.swift`: presentation-only sidenotes
//! anchored to their in-flow references.

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2_app_kit::NSAccessibility;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAttributedStringNSExtendedStringDrawing, NSAttributedStringNSStringDrawing, NSFont, NSFontWeightSemibold,
    NSResponder, NSStringDrawingOptions, NSView,
};
use objc2_core_foundation::{CGFloat, CGSize};
use objc2_foundation::NSRect;

use crate::appkit_compat::{RectExt, attributed_string, keys, rect};
use crate::fragments::footnote_reference_display::FootnoteReferenceDisplay;
use crate::render_contracts::RenderMode;
use crate::swift_compat::{smax, trim_whitespaces_and_newlines};
use crate::view::markdown_text_view::MarkdownTextView;

pub struct FootnoteMarginViewIvars {
    text_view: ObjcWeak<MarkdownTextView>,
}

define_class!(
    // SAFETY: `initWithFrame:` is forwarded in `new`; overrides keep
    // AppKit's signatures. No Drop impl.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "FootnoteMarginView"]
    #[ivars = FootnoteMarginViewIvars]
    pub struct FootnoteMarginView;

    unsafe impl NSObjectProtocol for FootnoteMarginView {}

    impl FootnoteMarginView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, dirty_rect: NSRect) {
            self.draw(dirty_rect);
        }
    }
);

impl FootnoteMarginView {
    pub fn new(text_view: &MarkdownTextView, mtm: MainThreadMarker) -> Retained<FootnoteMarginView> {
        let this = Self::alloc(mtm).set_ivars(FootnoteMarginViewIvars { text_view: ObjcWeak::from(text_view) });
        let this: Retained<FootnoteMarginView> =
            unsafe { msg_send![super(this), initWithFrame: crate::appkit_compat::RECT_ZERO] };
        this.setAccessibilityElement(false);
        this
    }

    fn draw(&self, dirty_rect: NSRect) {
        let Some(text_view) = self.ivars().text_view.load() else { return };
        if text_view.mode() == RenderMode::Source {
            return;
        }
        let document = text_view.parsed_document();
        let references = FootnoteReferenceDisplay::references(&document);
        if references.is_empty() {
            return;
        }
        let style_sheet = text_view.style_sheet();
        let body = style_sheet.body_font();
        let font = body.fontWithSize(body.pointSize() * 0.85);
        let label_font = NSFont::monospacedDigitSystemFontOfSize_weight(font.pointSize() * 0.78, unsafe { NSFontWeightSemibold });
        let line_height = smax(font.ascender() - font.descender() + font.leading(), style_sheet.line_height * 0.85);
        let clip_origin = text_view.enclosingScrollView().map_or(0.0, |scroll| scroll.contentView().bounds().min_y());
        let bounds = self.bounds();
        let mut next_y: CGFloat = 0.0;

        for reference in references {
            let Some(definition) = document.footnotes.get(&reference.identifier) else { continue };
            let Some(anchor) = text_view.rect_for_offset(reference.range.location) else { continue };
            let target_y = anchor.min_y() - clip_origin;
            let body_text = trim_whitespaces_and_newlines(&document.substring(definition.content_range)).to_owned();
            if body_text.is_empty() {
                continue;
            }
            let text = attributed_string(
                &body_text,
                &[(keys::font(), &*font as &AnyObject), (keys::foreground_color(), &*style_sheet.text_secondary)],
            );
            let options = NSStringDrawingOptions::UsesLineFragmentOrigin | NSStringDrawingOptions::UsesFontLeading;
            let text_rect =
                text.boundingRectWithSize_options_context(CGSize::new(smax(40.0, bounds.width() - 24.0), CGFloat::MAX), options, None);
            let height = smax(line_height, text_rect.height().ceil());
            if !(target_y + height >= bounds.min_y() && target_y <= bounds.max_y()) {
                continue;
            }
            let y = smax(target_y, next_y);
            let frame = rect(0.0, y, bounds.width(), height);
            if !(frame.max_y() >= dirty_rect.min_y() && frame.min_y() <= dirty_rect.max_y()) {
                next_y = frame.max_y() + 10.0;
                continue;
            }
            attributed_string(
                &reference.identifier,
                &[(keys::font(), &*label_font as &AnyObject), (keys::foreground_color(), &*style_sheet.accent)],
            )
            .drawInRect(rect(0.0, y + 1.0, 18.0, line_height));
            text.drawWithRect_options_context(rect(24.0, y, smax(40.0, bounds.width() - 24.0), height), options, None);
            next_y = frame.max_y() + 10.0;
        }
    }
}
