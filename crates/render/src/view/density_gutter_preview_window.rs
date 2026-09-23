//! Port of `View/DensityGutterPreviewWindow.swift`: the gutter's hover/scrub
//! tooltip (§8.6).
//!
//! A borderless child window rather than an `NSPopover`: a popover animates,
//! takes focus, and draws an anchor arrow, all of which are wrong for
//! something that has to track a drag at 120fps. It ignores mouse events so
//! it stays non-interactive during scrubbing, but can accept pointer presence
//! during passive hover.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSAppearanceCustomization, NSAttributedStringNSExtendedStringDrawing, NSBackingStoreType,
    NSBezierPath, NSColor, NSEvent, NSFloatingWindowLevel, NSFont, NSLineBreakMode, NSMutableParagraphStyle,
    NSResponder, NSScreen, NSStringDrawingOptions, NSTrackingArea, NSTrackingAreaOptions, NSView, NSWindow,
    NSWindowAnimationBehavior, NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSMutableAttributedString, NSPoint, NSRange, NSRect, NSSize, NSString};
use objc2_quartz_core::{CAMediaTiming, CATransition, kCATransitionFade};

use crate::appkit_compat::{RECT_ZERO, RectExt, WorkItem, attributed_string, keys, rect};
use crate::motion::{self, Curve};
use crate::swift_compat::{smax, smin};
use crate::theme::style_sheet::StyleSheet;
use crate::view::style_sheet_defaults::GutterChrome;
use crate::view::tracking_area::refresh_tracking_area;

pub struct DensityGutterPreviewWindowIvars {
    style_sheet: RefCell<Rc<StyleSheet>>,
    on_pointer_presence: RefCell<Option<Rc<dyn Fn(bool)>>>,
    content: Retained<PreviewContentView>,
    /// Quick Look windows are often only 600–800pt wide; a host may opt into
    /// a compact overlay there.
    allows_content_overlap: Cell<bool>,
    presentation_generation: Cell<isize>,
}

define_class!(
    // SAFETY: `initWithContentRect:styleMask:backing:defer:` is forwarded in
    // `new` after the ivars are set. Drop is on the ivars only.
    #[unsafe(super(NSWindow, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "DensityGutterPreviewWindow"]
    #[ivars = DensityGutterPreviewWindowIvars]
    pub struct DensityGutterPreviewWindow;

    unsafe impl NSObjectProtocol for DensityGutterPreviewWindow {}
);

impl DensityGutterPreviewWindow {
    pub const PREFERRED_WIDTH: CGFloat = 320.0;
    /// Narrowest card worth showing.
    pub const MINIMUM_USEFUL_WIDTH: CGFloat = 140.0;
    pub const ANCHOR_GAP: CGFloat = 8.0;
    const ENTRANCE_DURATION: f64 = 0.10;
    const EXIT_DURATION: f64 = 0.08;

    pub fn compact_overlay_width(parent_width: CGFloat) -> CGFloat {
        smin(Self::PREFERRED_WIDTH, smax(Self::MINIMUM_USEFUL_WIDTH, parent_width * 0.42))
    }

    pub fn resolved_maximum_width(
        anchor_x: CGFloat,
        maximum_trailing_x: Option<CGFloat>,
        minimum_origin_x: Option<CGFloat>,
        opens_inward: bool,
    ) -> Option<CGFloat> {
        let Some(maximum_trailing_x) = maximum_trailing_x.filter(|_| !opens_inward) else {
            return Some(Self::PREFERRED_WIDTH);
        };
        let origin_x = smax(anchor_x + Self::ANCHOR_GAP, minimum_origin_x.unwrap_or(-CGFloat::MAX));
        let available = smin(Self::PREFERRED_WIDTH, maximum_trailing_x - origin_x);
        if available >= Self::MINIMUM_USEFUL_WIDTH { Some(available) } else { None }
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<DensityGutterPreviewWindow> {
        let content = PreviewContentView::new(style_sheet.clone(), mtm);
        let this = Self::alloc(mtm).set_ivars(DensityGutterPreviewWindowIvars {
            style_sheet: RefCell::new(style_sheet.clone()),
            on_pointer_presence: RefCell::new(None),
            content: content.clone(),
            allows_content_overlap: Cell::new(false),
            presentation_generation: Cell::new(0),
        });
        let this: Retained<DensityGutterPreviewWindow> = unsafe {
            msg_send![
                super(this),
                initWithContentRect: rect(0.0, 0.0, Self::PREFERRED_WIDTH, 40.0),
                styleMask: NSWindowStyleMask::Borderless,
                backing: NSBackingStoreType::Buffered,
                defer: true
            ]
        };
        this.setOpaque(false);
        this.setBackgroundColor(Some(&NSColor::clearColor()));
        this.setHasShadow(true);
        this.setLevel(NSFloatingWindowLevel);
        this.setIgnoresMouseEvents(true);
        // SAFETY: the window is owned by the gutter, never released on close.
        unsafe { this.setReleasedWhenClosed(false) };
        this.setAnimationBehavior(NSWindowAnimationBehavior::None);
        this.setAppearance(Some(&style_sheet.appearance));
        content.setAppearance(Some(&style_sheet.appearance));
        this.setContentView(Some(&content));
        let weak: ObjcWeak<DensityGutterPreviewWindow> = ObjcWeak::from(&*this);
        content.set_on_pointer_presence(Some(Rc::new(move |is_inside| {
            let Some(this) = weak.load() else { return };
            let callback = this.ivars().on_pointer_presence.borrow().clone();
            if let Some(callback) = callback {
                callback(is_inside);
            }
        })));
        this
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet.clone();
        self.setAppearance(Some(&style_sheet.appearance));
        let content = &self.ivars().content;
        content.setAppearance(Some(&style_sheet.appearance));
        content.set_style_sheet(style_sheet);
    }

    pub fn set_on_pointer_presence(&self, callback: Option<Box<dyn Fn(bool)>>) {
        *self.ivars().on_pointer_presence.borrow_mut() = callback.map(Rc::from);
    }

    pub fn allows_content_overlap(&self) -> bool {
        self.ivars().allows_content_overlap.get()
    }

    /// The card's content view, for harnesses that inspect it.
    pub fn content(&self) -> &Retained<PreviewContentView> {
        &self.ivars().content
    }

    fn parent_is(&self, parent: &NSWindow) -> bool {
        self.parentWindow().is_some_and(|current| std::ptr::eq(&*current, parent))
    }

    fn animator(&self) -> Retained<AnyObject> {
        unsafe { msg_send![self, animator] }
    }

    /// `anchor` is the screen point at the gutter's trailing edge, level with
    /// the pointer.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &self,
        title: &str,
        snippet: &str,
        footer: &str,
        anchor: NSPoint,
        parent: &NSWindow,
        maximum_trailing_x: Option<CGFloat>,
        reduce_motion: bool,
        interactive: bool,
        allow_content_overlap: bool,
    ) {
        let ivars = self.ivars();
        ivars.presentation_generation.set(ivars.presentation_generation.get().wrapping_add(1));
        ivars.allows_content_overlap.set(allow_content_overlap);
        self.setIgnoresMouseEvents(!interactive);
        let content = ivars.content.clone();
        let title_changed = content.update(title, snippet, footer);
        let already_presented = self.isVisible() && self.parent_is(parent);
        // Keep the card geometry stable within a section; reflow only when a
        // new heading arrives.
        let opens_inward = anchor.x > parent.frame().mid_x();
        let visible_frame =
            parent.screen().or_else(|| NSScreen::mainScreen(self.mtm())).map(|screen| screen.visibleFrame());
        let resolved_width = Self::resolved_maximum_width(
            anchor.x,
            maximum_trailing_x,
            visible_frame.map(|frame| frame.min_x() + 4.0),
            opens_inward,
        );
        let uses_compact_overlay = resolved_width.is_none() && ivars.allows_content_overlap.get();
        let maximum_width = if let Some(resolved_width) = resolved_width {
            resolved_width
        } else if uses_compact_overlay {
            Self::compact_overlay_width(parent.frame().width())
        } else {
            self.hide();
            return;
        };
        let size = if already_presented && !title_changed && self.frame().width() <= maximum_width {
            self.frame().size
        } else {
            content.fitting_size_max_width(maximum_width)
        };

        let mut origin = NSPoint::new(
            if opens_inward {
                anchor.x - size.width - Self::ANCHOR_GAP
            } else {
                smax(
                    anchor.x + Self::ANCHOR_GAP,
                    visible_frame.map_or(anchor.x + Self::ANCHOR_GAP, |frame| frame.min_x() + 4.0),
                )
            },
            anchor.y - size.height / 2.0,
        );
        if !opens_inward && let Some(maximum_trailing_x) = maximum_trailing_x {
            origin.x = smin(origin.x, maximum_trailing_x - size.width);
        }
        if let Some(visible) = visible_frame {
            origin.x = smin(smax(visible.min_x() + 4.0, origin.x), visible.max_x() - size.width - 4.0);
            origin.y = smin(smax(visible.min_y() + 4.0, origin.y), visible.max_y() - size.height - 4.0);
        }
        // Covering prose is never an acceptable fallback.
        if !opens_inward
            && let Some(maximum_trailing_x) = maximum_trailing_x
            && !uses_compact_overlay
            && origin.x + size.width > maximum_trailing_x + 0.5
        {
            self.hide();
            return;
        }
        let final_frame = NSRect::new(origin, size);

        if already_presented {
            // Once visible, the preview tracks the pointer directly.
            self.setAlphaValue(1.0);
            self.setFrame_display(final_frame, true);
            if title_changed {
                content.animate_content_change(reduce_motion);
                content.stagger_snippet_reveal(reduce_motion);
            }
            content.setNeedsDisplay(true);
            return;
        }

        // Rise ~4pt while fading in — card lifts toward the mark.
        let entrance_frame = final_frame.offset_by(
            if reduce_motion {
                0.0
            } else if opens_inward {
                4.0
            } else {
                -4.0
            },
            if reduce_motion { 0.0 } else { -4.0 },
        );
        self.setFrame_display(entrance_frame, true);

        if self.parent_is(parent) && self.isVisible() {
            content.setNeedsDisplay(true);
            return;
        }
        // SAFETY: the child is a live window owned by the gutter.
        unsafe { parent.addChildWindow_ordered(self, NSWindowOrderingMode::Above) };
        self.setAlphaValue(if reduce_motion { 1.0 } else { 0.0 });
        self.orderFront(None);
        content.prepare_snippet_stagger(reduce_motion);
        if reduce_motion {
            content.reveal_snippet_immediately();
            return;
        }
        let this = self.retain();
        GutterChrome::animate(
            false,
            Self::ENTRANCE_DURATION,
            move |_| {
                let animator = this.animator();
                let _: () = unsafe { msg_send![&*animator, setAlphaValue: 1.0 as CGFloat] };
                let _: () = unsafe { msg_send![&*animator, setFrame: final_frame, display: true] };
            },
            None,
        );
        content.stagger_snippet_reveal(false);
    }

    pub fn hide(&self) {
        if !(self.isVisible() || self.parentWindow().is_some()) {
            return;
        }
        let ivars = self.ivars();
        ivars.presentation_generation.set(ivars.presentation_generation.get().wrapping_add(1));
        let generation = ivars.presentation_generation.get();

        if self.style_sheet().reduce_motion {
            self.setIgnoresMouseEvents(true);
            if let Some(parent) = self.parentWindow() {
                parent.removeChildWindow(self);
            }
            self.orderOut(None);
            self.setAlphaValue(1.0);
            ivars.content.reveal_snippet_immediately();
            return;
        }

        // Keep the child window attached during the short fade.
        let this = self.retain();
        let weak: ObjcWeak<DensityGutterPreviewWindow> = ObjcWeak::from(self);
        GutterChrome::animate(
            false,
            Self::EXIT_DURATION,
            move |_| {
                let animator = this.animator();
                let _: () = unsafe { msg_send![&*animator, setAlphaValue: 0.0 as CGFloat] };
            },
            Some(Box::new(move || {
                let Some(this) = weak.load() else { return };
                if this.ivars().presentation_generation.get() != generation {
                    return;
                }
                this.setIgnoresMouseEvents(true);
                if let Some(parent) = this.parentWindow() {
                    parent.removeChildWindow(&this);
                }
                this.orderOut(None);
                this.setAlphaValue(1.0);
                this.ivars().content.reveal_snippet_immediately();
            })),
        );
    }

    pub fn cancel_hide_animation(&self) {
        if !self.isVisible() {
            return;
        }
        let ivars = self.ivars();
        ivars.presentation_generation.set(ivars.presentation_generation.get().wrapping_add(1));
        self.setAlphaValue(1.0);
    }
}

// MARK: - Content

pub struct PreviewContentViewIvars {
    on_pointer_presence: RefCell<Option<Rc<dyn Fn(bool)>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    title: RefCell<String>,
    snippet: RefCell<String>,
    footer: RefCell<String>,
    cached_title: RefCell<Option<Retained<objc2_foundation::NSAttributedString>>>,
    cached_snippet: RefCell<Option<Retained<objc2_foundation::NSAttributedString>>>,
    cached_footer: RefCell<Option<Retained<objc2_foundation::NSAttributedString>>>,
    tracking_area: RefCell<Option<Retained<NSTrackingArea>>>,
    snippet_alpha: Cell<CGFloat>,
    snippet_reveal_work: RefCell<Option<WorkItem>>,
    stagger_generation: Cell<isize>,
}

define_class!(
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set;
    // overrides keep AppKit's signatures. Drop is on the ivars only.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "PreviewContentView"]
    #[ivars = PreviewContentViewIvars]
    pub struct PreviewContentView;

    unsafe impl NSObjectProtocol for PreviewContentView {}

    impl PreviewContentView {
        #[unsafe(method(isFlipped))]
        fn __is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(updateTrackingAreas))]
        fn __update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            refresh_tracking_area(
                self,
                &self.ivars().tracking_area,
                NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::ActiveAlways
                    | NSTrackingAreaOptions::InVisibleRect,
            );
        }

        #[unsafe(method(mouseEntered:))]
        fn __mouse_entered(&self, _event: &NSEvent) {
            self.pointer_presence(true);
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, _event: &NSEvent) {
            self.pointer_presence(false);
        }

        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, dirty_rect: NSRect) {
            let _: () = unsafe { msg_send![super(self), drawRect: dirty_rect] };
            self.draw();
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            let ivars = self.ivars();
            *ivars.cached_title.borrow_mut() = None;
            *ivars.cached_snippet.borrow_mut() = None;
            *ivars.cached_footer.borrow_mut() = None;
            self.setNeedsDisplay(true);
        }
    }
);

impl PreviewContentView {
    const PADDING: CGFloat = 9.0;
    /// Enough to recognise the section, not enough to read it here.
    const SNIPPET_LIMIT: usize = 220;

    fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<PreviewContentView> {
        let this = Self::alloc(mtm).set_ivars(PreviewContentViewIvars {
            on_pointer_presence: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet.clone()),
            title: RefCell::new(String::new()),
            snippet: RefCell::new(String::new()),
            footer: RefCell::new(String::new()),
            cached_title: RefCell::new(None),
            cached_snippet: RefCell::new(None),
            cached_footer: RefCell::new(None),
            tracking_area: RefCell::new(None),
            snippet_alpha: Cell::new(1.0),
            snippet_reveal_work: RefCell::new(None),
            stagger_generation: Cell::new(0),
        });
        let this: Retained<PreviewContentView> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        this.setWantsLayer(true);
        if let Some(layer) = this.layer() {
            layer.setCornerRadius(14.0);
            layer.setMasksToBounds(true);
            layer.setBackgroundColor(Some(&style_sheet.surface.colorWithAlphaComponent(0.96).CGColor()));
        }
        // SAFETY: AppKit exports the role as an immutable global.
        this.setAccessibilityRole(Some(unsafe { objc2_app_kit::NSAccessibilityGroupRole }));
        this.setAccessibilityLabel(Some(&NSString::from_str("Document map preview")));
        this
    }

    fn pointer_presence(&self, is_inside: bool) {
        let callback = self.ivars().on_pointer_presence.borrow().clone();
        if let Some(callback) = callback {
            callback(is_inside);
        }
    }

    fn set_on_pointer_presence(&self, callback: Option<Rc<dyn Fn(bool)>>) {
        *self.ivars().on_pointer_presence.borrow_mut() = callback;
    }

    fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        let ivars = self.ivars();
        *ivars.style_sheet.borrow_mut() = style_sheet.clone();
        if let Some(layer) = self.layer() {
            layer.setBackgroundColor(Some(&style_sheet.surface.colorWithAlphaComponent(0.96).CGColor()));
        }
        *ivars.cached_title.borrow_mut() = None;
        *ivars.cached_snippet.borrow_mut() = None;
        *ivars.cached_footer.borrow_mut() = None;
        self.setNeedsDisplay(true);
    }

    /// The card's current text, for harnesses.
    pub fn text(&self) -> (String, String, String) {
        let ivars = self.ivars();
        (ivars.title.borrow().clone(), ivars.snippet.borrow().clone(), ivars.footer.borrow().clone())
    }

    fn update(&self, title: &str, snippet: &str, footer: &str) -> bool {
        let ivars = self.ivars();
        let title_changed = title != *ivars.title.borrow();
        if !(title_changed || snippet != *ivars.snippet.borrow() || footer != *ivars.footer.borrow()) {
            return false;
        }
        *ivars.title.borrow_mut() = title.to_owned();
        *ivars.snippet.borrow_mut() = snippet.to_owned();
        *ivars.footer.borrow_mut() = footer.to_owned();
        *ivars.cached_title.borrow_mut() = None;
        *ivars.cached_snippet.borrow_mut() = None;
        *ivars.cached_footer.borrow_mut() = None;
        self.setNeedsDisplay(true);
        title_changed
    }

    fn animate_content_change(&self, reduce_motion: bool) {
        if reduce_motion {
            return;
        }
        let Some(layer) = self.layer() else { return };
        let transition = CATransition::new();
        // SAFETY: Core Animation exports the type as an immutable global.
        transition.setType(unsafe { kCATransitionFade });
        transition.setDuration(motion::PREVIEW_CROSSFADE);
        transition.setTimingFunction(Some(&motion::timing(Curve::EaseOut)));
        layer.addAnimation_forKey(&transition, Some(&NSString::from_str("preview-content-change")));
    }

    /// Title paints immediately; snippet waits a beat on entrance / section change.
    fn prepare_snippet_stagger(&self, reduce_motion: bool) {
        let ivars = self.ivars();
        if let Some(work) = ivars.snippet_reveal_work.borrow().as_ref() {
            work.cancel();
        }
        if reduce_motion || ivars.snippet.borrow().is_empty() {
            ivars.snippet_alpha.set(1.0);
        } else {
            ivars.snippet_alpha.set(0.0);
        }
        self.setNeedsDisplay(true);
    }

    fn stagger_snippet_reveal(&self, reduce_motion: bool) {
        let ivars = self.ivars();
        if let Some(work) = ivars.snippet_reveal_work.borrow().as_ref() {
            work.cancel();
        }
        ivars.stagger_generation.set(ivars.stagger_generation.get().wrapping_add(1));
        let generation = ivars.stagger_generation.get();
        if reduce_motion || ivars.snippet.borrow().is_empty() {
            ivars.snippet_alpha.set(1.0);
            self.setNeedsDisplay(true);
            return;
        }
        ivars.snippet_alpha.set(0.0);
        self.setNeedsDisplay(true);
        let weak: ObjcWeak<PreviewContentView> = ObjcWeak::from(self);
        let work = WorkItem::new(move || {
            let Some(this) = weak.load() else { return };
            if this.ivars().stagger_generation.get() != generation {
                return;
            }
            this.animate_snippet_alpha(1.0, generation);
        });
        *ivars.snippet_reveal_work.borrow_mut() = Some(work.clone());
        work.dispatch_main_after(motion::PREVIEW_STAGGER);
    }

    fn reveal_snippet_immediately(&self) {
        let ivars = self.ivars();
        if let Some(work) = ivars.snippet_reveal_work.borrow_mut().take() {
            work.cancel();
        }
        ivars.stagger_generation.set(ivars.stagger_generation.get().wrapping_add(1));
        ivars.snippet_alpha.set(1.0);
        self.setNeedsDisplay(true);
    }

    fn animate_snippet_alpha(&self, target: CGFloat, generation: isize) {
        let ivars = self.ivars();
        if !(ivars.stagger_generation.get() == generation && (target - ivars.snippet_alpha.get()).abs() > 0.01) {
            return;
        }
        let fade = CATransition::new();
        // SAFETY: Core Animation exports the type as an immutable global.
        fade.setType(unsafe { kCATransitionFade });
        fade.setDuration(motion::QUICK);
        fade.setTimingFunction(Some(&motion::timing(Curve::Decelerate)));
        if let Some(layer) = self.layer() {
            layer.addAnimation_forKey(&fade, Some(&NSString::from_str("snippet-reveal")));
        }
        ivars.snippet_alpha.set(target);
        self.setNeedsDisplay(true);
    }

    /// `fittingSize(maxWidth:)`.
    fn fitting_size_max_width(&self, max_width: CGFloat) -> NSSize {
        let text = self.combined_attributed(1.0);
        let bounds = text.boundingRectWithSize_options_context(
            NSSize::new(max_width - Self::PADDING * 2.0, 400.0),
            NSStringDrawingOptions::UsesLineFragmentOrigin | NSStringDrawingOptions::UsesFontLeading,
            None,
        );
        NSSize::new(max_width, smin(140.0, bounds.height().ceil() + Self::PADDING * 2.0))
    }

    fn paragraph() -> Retained<NSMutableParagraphStyle> {
        let paragraph = NSMutableParagraphStyle::new();
        paragraph.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        paragraph.setLineSpacing(2.0);
        paragraph
    }

    fn title_attributed(&self) -> Retained<objc2_foundation::NSAttributedString> {
        let ivars = self.ivars();
        if let Some(cached) = ivars.cached_title.borrow().as_ref() {
            return cached.clone();
        }
        let style_sheet = self.style_sheet();
        let paragraph = Self::paragraph();
        let font = GutterChrome::title_font();
        let value = attributed_string(
            &ivars.title.borrow(),
            &[
                (keys::font(), &font),
                (keys::foreground_color(), &style_sheet.text),
                (keys::paragraph_style(), &paragraph),
            ],
        );
        *ivars.cached_title.borrow_mut() = Some(value.clone());
        value
    }

    fn snippet_attributed(&self) -> Retained<objc2_foundation::NSAttributedString> {
        let ivars = self.ivars();
        if let Some(cached) = ivars.cached_snippet.borrow().as_ref() {
            return cached.clone();
        }
        let style_sheet = self.style_sheet();
        let snippet = ivars.snippet.borrow().clone();
        let mut body = upleft_swift_text::trim_whitespaces_and_newlines(&snippet).to_owned();
        if upleft_swift_text::count(&body) > Self::SNIPPET_LIMIT {
            body = format!("{}…", upleft_swift_text::prefix(&body, Self::SNIPPET_LIMIT));
        }
        let paragraph = Self::paragraph();
        let face = NSFont::fontWithDescriptor_size(&style_sheet.body_font().fontDescriptor(), 12.0)
            .unwrap_or_else(|| NSFont::systemFontOfSize(12.0));
        let text = if body.is_empty() { String::new() } else { format!("\n{body}") };
        let value = attributed_string(
            &text,
            &[
                (keys::font(), &face),
                (keys::foreground_color(), &style_sheet.text_secondary),
                (keys::paragraph_style(), &paragraph),
            ],
        );
        *ivars.cached_snippet.borrow_mut() = Some(value.clone());
        value
    }

    fn footer_attributed(&self) -> Retained<objc2_foundation::NSAttributedString> {
        let ivars = self.ivars();
        if let Some(cached) = ivars.cached_footer.borrow().as_ref() {
            return cached.clone();
        }
        let style_sheet = self.style_sheet();
        let paragraph = Self::paragraph();
        let footer = ivars.footer.borrow().clone();
        let text = if footer.is_empty() { String::new() } else { format!("\n{footer}") };
        let font = GutterChrome::body_font();
        let value = attributed_string(
            &text,
            &[
                (keys::font(), &font),
                (keys::foreground_color(), &style_sheet.text_faint),
                (keys::paragraph_style(), &paragraph),
            ],
        );
        *ivars.cached_footer.borrow_mut() = Some(value.clone());
        value
    }

    fn combined_attributed(&self, snippet_alpha: CGFloat) -> Retained<NSMutableAttributedString> {
        let result = NSMutableAttributedString::from_attributed_nsstring(&self.title_attributed());
        let style_sheet = self.style_sheet();
        if !self.ivars().snippet.borrow().is_empty() {
            let snippet = NSMutableAttributedString::from_attributed_nsstring(&self.snippet_attributed());
            let color = style_sheet
                .text_secondary
                .colorWithAlphaComponent(style_sheet.text_secondary.alphaComponent() * snippet_alpha);
            // SAFETY: the value is an NSColor, as the key expects.
            unsafe {
                snippet.addAttribute_value_range(
                    keys::foreground_color(),
                    &color,
                    NSRange::new(0, snippet.length()),
                );
            }
            result.appendAttributedString(&snippet);
        }
        if !self.ivars().footer.borrow().is_empty() {
            result.appendAttributedString(&self.footer_attributed());
        }
        result
    }

    fn draw(&self) {
        let style_sheet = self.style_sheet();
        let bounds = self.bounds();
        let card = bounds.inset_by(0.5, 0.5);
        let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(card, 14.0, 14.0);
        style_sheet.surface.colorWithAlphaComponent(0.96).setFill();
        path.fill();
        style_sheet.rule.setStroke();
        path.setLineWidth(1.0);
        path.stroke();

        self.combined_attributed(self.ivars().snippet_alpha.get()).drawWithRect_options_context(
            bounds.inset_by(Self::PADDING, Self::PADDING),
            NSStringDrawingOptions::UsesLineFragmentOrigin | NSStringDrawingOptions::UsesFontLeading,
            None,
        );
    }
}

#[allow(dead_code)]
fn _types_in_scope(_: &NSView) {}
