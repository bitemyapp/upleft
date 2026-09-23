//! The private view classes of `DocumentWindowController.swift`:
//! `FloatingOverlayHostView`, `FocusDimmingView` and `DocumentRootView`.

use std::cell::{Cell, RefCell};

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{NSBezierPath, NSColor, NSResponder, NSView, NSWindingRule};
use objc2_foundation::{NSPoint, NSRect};
use upleft_render::appkit_compat::{RECT_ZERO, rect_fill};

// MARK: - FloatingOverlayHostView

define_class!(
    /// Lets the document keep its normal hit testing while hosting transient
    /// controls above it. Only real descendants of the overlay consume clicks.
    // SAFETY: a plain `NSView` subclass with no ivars; `hitTest:` keeps
    // AppKit's signature.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "FloatingOverlayHostView"]
    pub struct FloatingOverlayHostView;

    unsafe impl NSObjectProtocol for FloatingOverlayHostView {}

    impl FloatingOverlayHostView {
        #[unsafe(method_id(hitTest:))]
        fn __hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            self.hit_test(point)
        }
    }
);

impl FloatingOverlayHostView {
    /// `FloatingOverlayHostView()`: `init(frame: .zero)`.
    pub fn new(mtm: MainThreadMarker) -> Retained<FloatingOverlayHostView> {
        unsafe { msg_send![FloatingOverlayHostView::alloc(mtm), initWithFrame: RECT_ZERO] }
    }

    fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
        let hit: Option<Retained<NSView>> = unsafe { msg_send![super(self), hitTest: point] };
        match hit {
            Some(hit) if std::ptr::eq(&*hit, self.as_ref() as &NSView) => None,
            other => other,
        }
    }
}

// MARK: - FocusDimmingView

pub struct FocusDimmingViewIvars {
    highlight_rect: Cell<Option<NSRect>>,
}

define_class!(
    /// Focus mode's dimming overlay: everything but the caret's paragraph.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set;
    // the overrides keep AppKit's signatures.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "FocusDimmingView"]
    #[ivars = FocusDimmingViewIvars]
    pub struct FocusDimmingView;

    unsafe impl NSObjectProtocol for FocusDimmingView {}

    impl FocusDimmingView {
        #[unsafe(method(isFlipped))]
        fn __is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method_id(hitTest:))]
        fn __hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, _dirty_rect: NSRect) {
            self.draw();
        }
    }
);

impl FocusDimmingView {
    /// `FocusDimmingView()`: `init(frame: .zero)`.
    pub fn new(mtm: MainThreadMarker) -> Retained<FocusDimmingView> {
        let this = Self::alloc(mtm).set_ivars(FocusDimmingViewIvars { highlight_rect: Cell::new(None) });
        unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] }
    }

    /// `var highlightRect`.
    pub fn highlight_rect(&self) -> Option<NSRect> {
        self.ivars().highlight_rect.get()
    }

    pub fn set_highlight_rect(&self, rect: Option<NSRect>) {
        self.ivars().highlight_rect.set(rect);
    }

    fn draw(&self) {
        let path = NSBezierPath::bezierPathWithRect(self.bounds());
        if let Some(highlight_rect) = self.highlight_rect() {
            let cutout = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(highlight_rect, 6.0, 6.0);
            path.appendBezierPath(&cutout);
            path.setWindingRule(NSWindingRule::EvenOdd);
        }
        NSColor::blackColor().colorWithAlphaComponent(0.18).setFill();
        path.fill();
    }
}

// MARK: - DocumentRootView

pub struct DocumentRootViewIvars {
    background_color: RefCell<Retained<NSColor>>,
}

define_class!(
    /// Document chrome is laid out top-to-bottom. Keeping the root flipped
    /// makes its frame coordinates agree with the flipped Markdown container
    /// and avoids inverted bar/document constraints.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set;
    // the overrides keep AppKit's signatures.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "DocumentRootView"]
    #[ivars = DocumentRootViewIvars]
    pub struct DocumentRootView;

    unsafe impl NSObjectProtocol for DocumentRootView {}

    impl DocumentRootView {
        #[unsafe(method(isFlipped))]
        fn __is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, dirty_rect: NSRect) {
            self.background_color().setFill();
            rect_fill(dirty_rect);
        }
    }
);

impl DocumentRootView {
    /// `init(backgroundColor:)`.
    pub fn new(background_color: &NSColor, mtm: MainThreadMarker) -> Retained<DocumentRootView> {
        let this = Self::alloc(mtm)
            .set_ivars(DocumentRootViewIvars { background_color: RefCell::new(background_color.retain()) });
        unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] }
    }

    /// `var backgroundColor`.
    pub fn background_color(&self) -> Retained<NSColor> {
        self.ivars().background_color.borrow().clone()
    }

    /// `backgroundColor = …`, with its `didSet { needsDisplay = true }`.
    pub fn set_background_color(&self, color: &NSColor) {
        *self.ivars().background_color.borrow_mut() = color.retain();
        self.setNeedsDisplay(true);
    }
}
