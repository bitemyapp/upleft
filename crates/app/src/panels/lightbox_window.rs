//! Port of `Panels/LightboxWindow.swift`: the image lightbox (§7.1).
//!
//! "Click an image → lightbox with zoom and pan; alt text renders as a
//! caption." Scroll to zoom about the pointer, drag to pan, `⎋` or a click on
//! the backdrop to leave.
//!
//! This is the one surface that does **not** take its colours from the
//! theme. A lightbox works by removing everything around the image — a
//! themed scrim would tint the picture you came to look at, which is the
//! opposite of the point. Chrome is themed; a viewing surround is not.
//!
//! Objective-C class names: `LightboxWindow` (an `NSWindow` subclass) and
//! `LightboxContentView`. `scale` is an Objective-C property (`scale` /
//! `setScale:`), as Swift's `@objc dynamic var scale` is, so the view's
//! animator can animate it through `+defaultAnimationForKey:`.
//!
//! Members that are `private` in Swift but that the conformance scene
//! reaches through `@_private(sourceFile:) import` (`lightboxView`,
//! `resetZoom`, `zoom(by:about:)`, `fitScale`, `imageRect`, `scale`,
//! `offset`) are `pub` here.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyClass, AnyObject, NSObjectProtocol};
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSBackingStoreType, NSBezierPath, NSColor, NSCompositingOperation, NSCursor,
    NSEvent, NSFont, NSImage, NSLineBreakMode, NSMutableParagraphStyle, NSResponder, NSStringDrawing, NSTextAlignment,
    NSView, NSWindow, NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_core_foundation::{CGFloat, CGSize};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::CABasicAnimation;
use upleft_render::appkit_compat::{RectExt, attributes_dictionary, keys, rect_fill};
use upleft_render::motion::{self, Curve};

use super::appkit_support::{ns_string, object, role, set_label, set_role, smax, smin, weight_medium};
use super::panel_chrome::{PanelAnimation, PanelFont};
use crate::support::commands::KeyBinding;

/// `NSWindow.Level.floating`.
const FLOATING_WINDOW_LEVEL: isize = 3;

pub struct LightboxWindowIvars {
    reduce_motion: Cell<bool>,
    reduce_transparency: Cell<bool>,
}

define_class!(
    /// `LightboxWindow`.
    // SAFETY: `initWithContentRect:styleMask:backing:defer:` is forwarded in
    // `new` after the ivars are set.
    #[unsafe(super(NSWindow, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "LightboxWindow"]
    #[ivars = LightboxWindowIvars]
    pub struct LightboxWindow;

    unsafe impl NSObjectProtocol for LightboxWindow {}

    impl LightboxWindow {
        #[unsafe(method(canBecomeKeyWindow))]
        fn __can_become_key_window(&self) -> bool {
            true
        }

        #[unsafe(method(cancelOperation:))]
        fn __cancel_operation(&self, _sender: Option<&AnyObject>) {
            self.dismiss();
        }
    }
);

impl LightboxWindow {
    /// `convenience init(image:caption:reduceMotion:reduceTransparency:)`;
    /// Swift defaults both flags to `false`.
    pub fn new(
        image: &NSImage,
        caption: Option<&str>,
        reduce_motion: bool,
        reduce_transparency: bool,
        mtm: MainThreadMarker,
    ) -> Retained<LightboxWindow> {
        let this = Self::alloc(mtm)
            .set_ivars(LightboxWindowIvars { reduce_motion: Cell::new(false), reduce_transparency: Cell::new(false) });
        let this: Retained<LightboxWindow> = unsafe {
            msg_send![
                super(this),
                initWithContentRect: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(800.0, 600.0)),
                styleMask: NSWindowStyleMask::Borderless,
                backing: NSBackingStoreType::Buffered,
                defer: true
            ]
        };
        this.setOpaque(false);
        this.setBackgroundColor(Some(&NSColor::clearColor()));
        this.setHasShadow(false);
        this.setLevel(FLOATING_WINDOW_LEVEL);
        unsafe { this.setReleasedWhenClosed(false) };
        this.setAnimationBehavior(NSWindowAnimationBehavior::None);
        this.setCollectionBehavior(
            NSWindowCollectionBehavior::FullScreenAuxiliary | NSWindowCollectionBehavior::Transient,
        );
        this.ivars().reduce_motion.set(reduce_motion);
        this.ivars().reduce_transparency.set(reduce_transparency);

        let view = LightboxContentView::new(image, caption, reduce_motion, reduce_transparency, mtm);
        let weak: ObjcWeak<LightboxWindow> = ObjcWeak::from(&*this);
        view.set_on_dismiss(Some(Rc::new(move || {
            if let Some(this) = weak.load() {
                this.dismiss();
            }
        })));
        this.setContentView(Some(&view));
        this
    }

    /// `lightboxView` (private in Swift): `contentView as? LightboxContentView`.
    pub fn lightbox_view(&self) -> Option<Retained<LightboxContentView>> {
        self.contentView().and_then(|view| super::appkit_support::downcast::<LightboxContentView>(&view))
    }

    pub fn present(&self, window: &NSWindow) {
        let frame = window.screen().map_or_else(|| window.frame(), |screen| screen.visibleFrame());
        self.setFrame_display(frame, true);
        if let Some(view) = self.lightbox_view() {
            view.reset_zoom(false);
        }

        unsafe { window.addChildWindow_ordered(self, NSWindowOrderingMode::Above) };
        let reduce_motion = self.ivars().reduce_motion.get();
        self.setAlphaValue(if reduce_motion { 1.0 } else { 0.0 });
        self.makeKeyAndOrderFront(None);
        let content = self.contentView();
        self.makeFirstResponder(content.as_deref().map(|view| -> &NSResponder { view }));
        if reduce_motion {
            return;
        }
        let weak: ObjcWeak<LightboxWindow> = ObjcWeak::from(self);
        PanelAnimation::run(
            false,
            motion::SELECTION,
            move |_| {
                if let Some(this) = weak.load() {
                    this.animator().setAlphaValue(1.0);
                }
            },
            None,
        );
    }

    pub fn dismiss(&self) {
        let weak: ObjcWeak<LightboxWindow> = ObjcWeak::from(self);
        let finish = move || {
            let Some(this) = weak.load() else { return };
            if let Some(parent) = this.parentWindow() {
                parent.removeChildWindow(&this);
            }
            this.orderOut(None);
            this.setAlphaValue(1.0);
        };
        if self.ivars().reduce_motion.get() {
            finish();
            return;
        }
        let weak: ObjcWeak<LightboxWindow> = ObjcWeak::from(self);
        PanelAnimation::run(
            false,
            motion::QUICK,
            move |_| {
                if let Some(this) = weak.load() {
                    this.animator().setAlphaValue(0.0);
                }
            },
            Some(Box::new(finish)),
        );
    }
}

// MARK: - Content

pub struct LightboxContentViewIvars {
    on_dismiss: RefCell<Option<Rc<dyn Fn()>>>,
    image: Retained<NSImage>,
    caption: Option<String>,
    reduce_motion: bool,
    reduce_transparency: bool,
    scale: Cell<CGFloat>,
    offset: Cell<CGSize>,
    did_drag: Cell<bool>,
    is_gripping: Cell<bool>,
}

impl LightboxContentViewIvars {
    const CAPTION_INSET: CGFloat = 24.0;
    const MINIMUM_SCALE: CGFloat = 0.05;
    const MAXIMUM_SCALE: CGFloat = 16.0;
}

define_class!(
    /// `LightboxContentView` (private in Swift).
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "LightboxContentView"]
    #[ivars = LightboxContentViewIvars]
    pub struct LightboxContentView;

    unsafe impl NSObjectProtocol for LightboxContentView {}

    impl LightboxContentView {
        #[unsafe(method(acceptsFirstResponder))]
        fn __accepts_first_responder(&self) -> bool {
            true
        }

        /// `@objc dynamic var scale`.
        #[unsafe(method(scale))]
        fn __scale(&self) -> CGFloat {
            self.ivars().scale.get()
        }

        #[unsafe(method(setScale:))]
        fn __set_scale(&self, scale: CGFloat) {
            self.ivars().scale.set(scale);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method_id(defaultAnimationForKey:))]
        fn __default_animation_for_key(key: &NSString) -> Option<Retained<AnyObject>> {
            Self::default_animation_for_key(key)
        }

        #[unsafe(method(scrollWheel:))]
        fn __scroll_wheel(&self, event: &NSEvent) {
            let delta = if event.hasPreciseScrollingDeltas() {
                event.scrollingDeltaY() / 200.0
            } else {
                event.deltaY() / 20.0
            };
            if delta == 0.0 {
                return;
            }
            self.zoom(1.0 + delta, self.convertPoint_fromView(event.locationInWindow(), None));
        }

        #[unsafe(method(magnifyWithEvent:))]
        fn __magnify(&self, event: &NSEvent) {
            self.zoom(1.0 + event.magnification(), self.convertPoint_fromView(event.locationInWindow(), None));
        }

        #[unsafe(method(smartMagnifyWithEvent:))]
        fn __smart_magnify(&self, event: &NSEvent) {
            let anchor = self.convertPoint_fromView(event.locationInWindow(), None);
            let scale = self.scale();
            if (scale - self.fit_scale()).abs() < 0.001 {
                let target_scale: CGFloat = if self.fit_scale() < 1.0 { 1.0 } else { 2.0 };
                self.zoom(target_scale / smax(0.01, scale), anchor);
            } else {
                self.reset_zoom(true);
            }
        }

        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, _event: &NSEvent) {
            self.ivars().did_drag.set(false);
        }

        #[unsafe(method(mouseDragged:))]
        fn __mouse_dragged(&self, event: &NSEvent) {
            // The hand closes while it is dragging. An open hand that never
            // grips is the one cue a pan gesture owes the pointer.
            let ivars = self.ivars();
            if !ivars.did_drag.get() {
                NSCursor::closedHandCursor().push();
                ivars.is_gripping.set(true);
            }
            ivars.did_drag.set(true);
            let mut offset = ivars.offset.get();
            offset.width += event.deltaX();
            offset.height -= event.deltaY();
            ivars.offset.set(offset);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(mouseUp:))]
        fn __mouse_up(&self, event: &NSEvent) {
            self.mouse_up(event);
        }

        #[unsafe(method(keyDown:))]
        fn __key_down(&self, event: &NSEvent) {
            self.key_down(event);
        }

        #[unsafe(method(cancelOperation:))]
        fn __cancel_operation(&self, _sender: Option<&AnyObject>) {
            self.dismiss();
        }

        #[unsafe(method(resetCursorRects))]
        fn __reset_cursor_rects(&self) {
            self.addCursorRect_cursor(self.image_rect(), &NSCursor::openHandCursor());
        }

        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, dirty_rect: NSRect) {
            self.draw(dirty_rect);
        }

        #[unsafe(method(viewDidEndLiveResize))]
        fn __view_did_end_live_resize(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidEndLiveResize] };
            self.setNeedsDisplay(true);
        }
    }
);

impl LightboxContentView {
    /// `init(image:caption:reduceMotion:reduceTransparency:)`.
    pub fn new(
        image: &NSImage,
        caption: Option<&str>,
        reduce_motion: bool,
        reduce_transparency: bool,
        mtm: MainThreadMarker,
    ) -> Retained<LightboxContentView> {
        let caption = match caption {
            Some("") => None,
            other => other.map(str::to_owned),
        };
        let this = Self::alloc(mtm).set_ivars(LightboxContentViewIvars {
            on_dismiss: RefCell::new(None),
            image: image.retain(),
            caption,
            reduce_motion,
            reduce_transparency,
            scale: Cell::new(1.0),
            offset: Cell::new(CGSize::new(0.0, 0.0)),
            did_drag: Cell::new(false),
            is_gripping: Cell::new(false),
        });
        let this: Retained<LightboxContentView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        set_role(&*this, role::image());
        let accessibility = this.ivars().caption.clone().unwrap_or_else(|| "Image".to_owned());
        set_label(&*this, &accessibility);
        this
    }

    pub fn set_on_dismiss(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_dismiss.borrow_mut() = handler;
    }

    fn dismiss(&self) {
        let handler = self.ivars().on_dismiss.borrow().clone();
        if let Some(handler) = handler {
            handler();
        }
    }

    /// `scale` (read through the Objective-C getter, as a `dynamic`
    /// property is).
    pub fn scale(&self) -> CGFloat {
        unsafe { msg_send![self, scale] }
    }

    /// `scale = value` (through `setScale:`, as a `dynamic` property is).
    fn set_scale(&self, scale: CGFloat) {
        let _: () = unsafe { msg_send![self, setScale: scale] };
    }

    pub fn offset(&self) -> CGSize {
        self.ivars().offset.get()
    }

    // MARK: Geometry

    /// Scale at which the image just fits, never enlarging a small image —
    /// blowing a 64pt icon up to fill a 27" display helps nobody.
    pub fn fit_scale(&self) -> CGFloat {
        let size = self.ivars().image.size();
        if !(size.width > 0.0 && size.height > 0.0) {
            return 1.0;
        }
        let available = self.bounds().inset_by(40.0, 60.0);
        smin(1.0, smin(available.width() / size.width, available.height() / size.height))
    }

    pub fn reset_zoom(&self, animated: bool) {
        self.set_zoom(self.fit_scale(), CGSize::new(0.0, 0.0), animated);
    }

    fn default_animation_for_key(key: &NSString) -> Option<Retained<AnyObject>> {
        if key.to_string() == "scale" {
            let animation = CABasicAnimation::new();
            return Some(Retained::into_super(Retained::into_super(Retained::into_super(Retained::into_super(animation)))));
        }
        let superclass: &AnyClass = NSView::class();
        unsafe { msg_send![super(Self::class(), superclass.metaclass()), defaultAnimationForKey: key] }
    }

    pub fn set_zoom(&self, target_scale: CGFloat, target_offset: CGSize, animated: bool) {
        let ivars = self.ivars();
        ivars.offset.set(target_offset);
        if !(animated && !ivars.reduce_motion && self.window().is_some()) {
            self.set_scale(target_scale);
            self.setNeedsDisplay(true);
            return;
        }
        let weak: ObjcWeak<LightboxContentView> = ObjcWeak::from(self);
        motion::run(
            false,
            motion::STANDARD,
            Curve::Decelerate,
            move |_| {
                if let Some(this) = weak.load() {
                    let animator = this.animator();
                    let _: () = unsafe { msg_send![&*animator, setScale: target_scale] };
                }
            },
            None,
        );
    }

    pub fn image_rect(&self) -> NSRect {
        let ivars = self.ivars();
        let image_size = ivars.image.size();
        let scale = self.scale();
        let size = NSSize::new(image_size.width * scale, image_size.height * scale);
        let bounds = self.bounds();
        let offset = ivars.offset.get();
        NSRect::new(
            NSPoint::new(
                bounds.mid_x() - size.width / 2.0 + offset.width,
                bounds.mid_y() - size.height / 2.0 + offset.height,
            ),
            size,
        )
    }

    /// Zooms about `anchor` so the pixel under the pointer stays under it.
    pub fn zoom(&self, factor: CGFloat, anchor: NSPoint) {
        let old = self.image_rect();
        let scale = self.scale();
        let new_scale = smin(LightboxContentViewIvars::MAXIMUM_SCALE, smax(LightboxContentViewIvars::MINIMUM_SCALE, scale * factor));
        if !(new_scale != scale && old.width() > 0.0 && old.height() > 0.0) {
            return;
        }

        let unit_x = (anchor.x - old.min_x()) / old.width();
        let unit_y = (anchor.y - old.min_y()) / old.height();
        let image_size = self.ivars().image.size();
        let size = NSSize::new(image_size.width * new_scale, image_size.height * new_scale);
        let origin_x = anchor.x - unit_x * size.width;
        let origin_y = anchor.y - unit_y * size.height;
        let bounds = self.bounds();
        let target_offset = CGSize::new(
            origin_x - (bounds.mid_x() - size.width / 2.0),
            origin_y - (bounds.mid_y() - size.height / 2.0),
        );
        self.set_zoom(new_scale, target_offset, true);
    }

    // MARK: Input

    fn mouse_up(&self, event: &NSEvent) {
        let ivars = self.ivars();
        if ivars.is_gripping.get() {
            NSCursor::pop_class();
            ivars.is_gripping.set(false);
        }
        if event.clickCount() == 2 {
            // Fit ⇄ actual size, the standard image-viewer double-click.
            let scale = if (self.scale() - self.fit_scale()).abs() < 0.001 { 1.0 } else { self.fit_scale() };
            self.set_zoom(scale, CGSize::new(0.0, 0.0), true);
            return;
        }
        if ivars.did_drag.get() {
            return;
        }
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        if !self.image_rect().contains_point(point) {
            self.dismiss();
        }
    }

    fn key_down(&self, event: &NSEvent) {
        // The three zoom keys every image viewer has. Scroll-to-zoom alone
        // leaves trackpad-less and keyboard-only users without a zoom at
        // all.
        let bounds = self.bounds();
        let centre = NSPoint::new(bounds.mid_x(), bounds.mid_y());
        let ivars = self.ivars();
        let nudge = |dx: CGFloat, dy: CGFloat| {
            let mut offset = ivars.offset.get();
            offset.width += dx;
            offset.height += dy;
            ivars.offset.set(offset);
            self.setNeedsDisplay(true);
        };
        match KeyBinding::key_for_event(event).as_deref() {
            Some("escape") | Some("space") => self.dismiss(),
            Some("+") | Some("=") => self.zoom(1.25, centre),
            Some("-") | Some("_") => self.zoom(1.0 / 1.25, centre),
            Some("0") => self.reset_zoom(true),
            Some("1") => self.set_zoom(1.0, CGSize::new(0.0, 0.0), true),
            Some("left") => nudge(40.0, 0.0),
            Some("right") => nudge(-40.0, 0.0),
            Some("up") => nudge(0.0, -40.0),
            Some("down") => nudge(0.0, 40.0),
            _ => {
                let _: () = unsafe { msg_send![super(self), keyDown: event] };
            }
        }
    }

    // MARK: Drawing

    fn draw(&self, dirty_rect: NSRect) {
        let ivars = self.ivars();
        // Opaque under Reduce Transparency; a scrim is decoration, and the
        // setting says not to depend on it.
        NSColor::blackColor().colorWithAlphaComponent(if ivars.reduce_transparency { 1.0 } else { 0.88 }).setFill();
        rect_fill(dirty_rect);

        let rect = self.image_rect();
        if rect.intersects(dirty_rect) {
            ivars.image.drawInRect_fromRect_operation_fraction(rect, NSRect::ZERO, NSCompositingOperation::SourceOver, 1.0);
        }

        self.draw_zoom_readout();

        let Some(caption) = ivars.caption.as_deref() else { return };
        // A long alt text is truncated with an ellipsis rather than sliced
        // mid-glyph, and the pill never grows past the window.
        let paragraph = NSMutableParagraphStyle::new();
        paragraph.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        paragraph.setAlignment(NSTextAlignment::Center);
        let font = PanelFont::system_regular(12.0);
        let white = NSColor::whiteColor();
        let attributes = attributes_dictionary(&[
            (keys::font(), object(&*font)),
            (keys::foreground_color(), object(&*white)),
            (keys::paragraph_style(), object(&*paragraph)),
        ]);
        let bounds = self.bounds();
        let maximum_width = smax(80.0, bounds.width() - 80.0);
        let caption_string = ns_string(caption);
        let natural = unsafe { caption_string.sizeWithAttributes(Some(&attributes)) };
        let pill_width = smin(maximum_width, natural.width + 20.0);
        let pill = NSRect::new(
            NSPoint::new(bounds.mid_x() - pill_width / 2.0, LightboxContentViewIvars::CAPTION_INSET - 5.0),
            NSSize::new(pill_width, natural.height + 10.0),
        );
        NSColor::whiteColor().colorWithAlphaComponent(0.12).setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(pill, 6.0, 6.0).fill();
        unsafe { caption_string.drawInRect_withAttributes(pill.inset_by(10.0, 5.0), Some(&attributes)) };
    }

    /// The current zoom, so "+" and scroll have a visible effect even on an
    /// image with no detail to judge scale by.
    fn draw_zoom_readout(&self) {
        let text = format!("{}%", (self.scale() * 100.0).round() as i64);
        let font = NSFont::monospacedDigitSystemFontOfSize_weight(11.0, weight_medium());
        let color = NSColor::whiteColor().colorWithAlphaComponent(0.85);
        let attributes = attributes_dictionary(&[(keys::font(), object(&*font)), (keys::foreground_color(), object(&*color))]);
        let text = ns_string(&text);
        let size = unsafe { text.sizeWithAttributes(Some(&attributes)) };
        let bounds = self.bounds();
        let pill = NSRect::new(
            NSPoint::new(bounds.max_x() - size.width - 34.0, bounds.max_y() - size.height - 26.0),
            NSSize::new(size.width + 16.0, size.height + 8.0),
        );
        NSColor::whiteColor().colorWithAlphaComponent(0.12).setFill();
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(pill, 6.0, 6.0).fill();
        unsafe { text.drawAtPoint_withAttributes(NSPoint::new(pill.min_x() + 8.0, pill.min_y() + 4.0), Some(&attributes)) };
    }
}
