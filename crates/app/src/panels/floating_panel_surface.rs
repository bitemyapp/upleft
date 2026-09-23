//! Port of `Panels/FloatingPanelSurface.swift`: `FloatingPanelWindow`, the
//! transparent child boundary for detached glass, and `FloatingPanelSurface`,
//! the floating panel body that owns the material and geometry.

#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, available, define_class, msg_send};
use objc2_app_kit::{
    NSAppearanceCustomization, NSAutoresizingMaskOptions, NSBackingStoreType, NSButton, NSColor, NSEvent, NSEventMask,
    NSEventType, NSGlassEffectView, NSGlassEffectViewStyle, NSNormalWindowLevel, NSPanel, NSPopUpButton, NSResponder,
    NSScrollView, NSSegmentedControl, NSTableView, NSTextField, NSView, NSWindow, NSWindowCollectionBehavior,
    NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_core_foundation::{CFRetained, CGFloat, CGPoint, CGRect};
use objc2_core_graphics::{CGMutablePath, CGPath};
use objc2_foundation::{NSArray, NSNumber, NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::{
    NSValueCATransform3DAdditions,
    CAAnimation, CAAnimationGroup, CABasicAnimation, CAGradientLayer, CAKeyframeAnimation, CAMediaTiming,
    CAShapeLayer, CATransaction, CATransform3DIdentity, CATransform3D, kCACornerCurveContinuous,
    kCALineCapRound,
};
use upleft_render::motion::{self, Curve, SpringRect, SpringScalar, SpringSurfaceView};
use upleft_render::theme::style_sheet::StyleSheet;

use super::appkit_support::{set_mask, superview, IDENTITY, is_same_view, RECT_ZERO, RectExt, cg, cg_array, downcast, null_actions, rect, role, set_label, set_role, smax, smin};
use super::chrome_glass::{ChromeGlass, Tint};
use super::inspector_host_view::InspectorHostView;
use super::panel_chrome::{PanelBackdrop, PanelMetrics, panel_surface_preferred_width, set_number_values};

// MARK: - FloatingPanelWindow

pub struct FloatingPanelWindowIvars {
    floating_surface: RefCell<ObjcWeak<FloatingPanelSurface>>,
    on_outside_mouse_down: RefCell<Option<Rc<dyn Fn()>>>,
    mouse_monitor: RefCell<Option<Retained<AnyObject>>>,
}

impl Drop for FloatingPanelWindowIvars {
    fn drop(&mut self) {
        if let Some(monitor) = self.mouse_monitor.get_mut().take() {
            // SAFETY: the monitor token came from `addLocalMonitorForEvents`.
            unsafe { NSEvent::removeMonitor(&monitor) };
        }
    }
}

define_class!(
    /// Transparent child boundary for detached glass.
    // SAFETY: `initWithContentRect:styleMask:backing:defer:` is forwarded in
    // `new` after the ivars are set.
    #[unsafe(super(NSPanel, NSWindow, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "FloatingPanelWindow"]
    #[ivars = FloatingPanelWindowIvars]
    pub struct FloatingPanelWindow;

    unsafe impl NSObjectProtocol for FloatingPanelWindow {}

    impl FloatingPanelWindow {
        #[unsafe(method(canBecomeKeyWindow))]
        fn __can_become_key(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn __can_become_main(&self) -> bool {
            false
        }

        #[unsafe(method(sendEvent:))]
        fn __send_event(&self, event: &NSEvent) {
            let kind = event.r#type();
            if kind == NSEventType::LeftMouseDown || kind == NSEventType::RightMouseDown {
                let surface = self.ivars().floating_surface.borrow().load();
                if let Some(surface) = surface {
                    let point = surface.convertPoint_fromView(event.locationInWindow(), None);
                    if !surface.visible_body_bounds_for_hit_testing().contains_point(point) {
                        let handler = self.ivars().on_outside_mouse_down.borrow().clone();
                        if let Some(handler) = handler {
                            handler();
                        }
                        return;
                    }
                }
            }
            let _: () = unsafe { msg_send![super(self), sendEvent: event] };
        }
    }
);

impl FloatingPanelWindow {
    /// `init(frame:)`.
    pub fn new(frame: NSRect, mtm: MainThreadMarker) -> Retained<FloatingPanelWindow> {
        let this = Self::alloc(mtm).set_ivars(FloatingPanelWindowIvars {
            floating_surface: RefCell::new(ObjcWeak::default()),
            on_outside_mouse_down: RefCell::new(None),
            mouse_monitor: RefCell::new(None),
        });
        let this: Retained<FloatingPanelWindow> = unsafe {
            msg_send![
                super(this),
                initWithContentRect: frame,
                styleMask: NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
                backing: NSBackingStoreType::Buffered,
                defer: false
            ]
        };
        this.setOpaque(false);
        this.setBackgroundColor(Some(&NSColor::clearColor()));
        this.setHasShadow(false);
        this.setBecomesKeyOnlyIfNeeded(true);
        this.setHidesOnDeactivate(false);
        this.setFloatingPanel(false);
        this.setLevel(NSNormalWindowLevel);
        this.setCollectionBehavior(NSWindowCollectionBehavior::MoveToActiveSpace);
        this.setIgnoresMouseEvents(false);
        // SAFETY: the owner keeps the window; it is never released on close.
        unsafe { this.setReleasedWhenClosed(false) };
        set_role(&*this, role::window());
        set_label(&*this, "Floating panel");

        let weak: ObjcWeak<FloatingPanelWindow> = ObjcWeak::from(&*this);
        let block = block2::RcBlock::new(move |event: std::ptr::NonNull<NSEvent>| -> *mut NSEvent {
            let event_ref = unsafe { event.as_ref() };
            let Some(this) = weak.load() else { return event.as_ptr() };
            let Some(surface) = this.ivars().floating_surface.borrow().load() else { return event.as_ptr() };
            let screen_point = match event_ref.window(this.mtm()) {
                Some(window) => window.convertPointToScreen(event_ref.locationInWindow()),
                None => NSEvent::mouseLocation(),
            };
            let panel_point = this.convertPointFromScreen(screen_point);
            if !surface.close_control_hit_zone_contains_window_point(panel_point) {
                return event.as_ptr();
            }
            surface.request_close();
            std::ptr::null_mut()
        });
        let monitor = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(
                NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown,
                &block,
            )
        };
        *this.ivars().mouse_monitor.borrow_mut() = monitor;
        this
    }

    pub fn floating_surface(&self) -> Option<Retained<FloatingPanelSurface>> {
        self.ivars().floating_surface.borrow().load()
    }

    pub fn set_floating_surface(&self, surface: Option<&FloatingPanelSurface>) {
        *self.ivars().floating_surface.borrow_mut() = surface.map(ObjcWeak::from).unwrap_or_default();
    }

    pub fn set_on_outside_mouse_down(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_outside_mouse_down.borrow_mut() = handler;
    }
}

// MARK: - FloatingPanelSurface

/// `FloatingPanelSurface.Top`.
pub struct Top;

impl Top {
    /// The ceiling leaves a readable document margin below the toolbar.
    pub const WINDOW_HEIGHT_FRACTION: CGFloat = 0.6;
    pub const MINIMUM_CONTENT_HEIGHT: CGFloat = 132.0;
    pub const POUR_SLIVER_HEIGHT: CGFloat = 22.0;
    pub const CORNER_RADIUS: CGFloat = PanelMetrics::FLOATING_SURFACE_RADIUS;
}

pub struct FloatingPanelSurfaceIvars {
    style_sheet: RefCell<Rc<StyleSheet>>,
    content: Retained<NSView>,
    on_close: RefCell<Option<Rc<dyn Fn()>>>,
    on_window_frame_change: RefCell<Option<Rc<dyn Fn(NSRect)>>>,
    on_frame_spring_settled: RefCell<Option<Rc<dyn Fn()>>>,
    fallback: Retained<PanelBackdrop>,
    rim_gradient: Retained<CAGradientLayer>,
    rim_mask: Retained<CAShapeLayer>,
    arrival_glint: Retained<CAShapeLayer>,
    reveal_mask: Retained<CAShapeLayer>,
    glass: RefCell<Option<Retained<NSView>>>,
    glass_layout_content: RefCell<Option<Retained<NSView>>>,
    is_hosting_content_in_glass: Cell<bool>,
    content_layout_height: Cell<CGFloat>,
    frame_spring: RefCell<SpringRect>,
    reveal_spring: RefCell<SpringScalar>,
    has_spring_frame: Cell<bool>,
    has_reveal_spring: Cell<bool>,
    frame_spring_moving: Cell<bool>,
    spring_moving: Cell<bool>,
    resting_window_frame: Cell<NSRect>,
    sliver_window_frame: Cell<NSRect>,
    anchor_window_frame: Cell<NSRect>,
    is_anchor_morphing: Cell<bool>,
    is_dismissing: Cell<bool>,
    uses_glass: Cell<bool>,
}

define_class!(
    /// A floating panel body. The body owns the material and geometry; the
    /// inspector host (when present) owns the one title/switcher header.
    // SAFETY: `initWithFrame:` is forwarded to `SpringSurfaceView` in `new`
    // after the ivars are set.
    #[unsafe(super(SpringSurfaceView, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "FloatingPanelSurface"]
    #[ivars = FloatingPanelSurfaceIvars]
    pub struct FloatingPanelSurface;

    unsafe impl NSObjectProtocol for FloatingPanelSurface {}

    impl FloatingPanelSurface {
        #[unsafe(method(springTick:))]
        fn __spring_tick(&self, dt: CGFloat) -> bool {
            let ivars = self.ivars();
            let frame_moving = ivars.frame_spring.borrow_mut().advance(dt);
            ivars.frame_spring_moving.set(frame_moving);
            let reveal_moving = ivars.reveal_spring.borrow_mut().advance(dt);
            let moving = frame_moving || reveal_moving;
            ivars.spring_moving.set(moving);
            moving
        }

        #[unsafe(method(springApply))]
        fn __spring_apply(&self) {
            let ivars = self.ivars();
            if ivars.frame_spring_moving.get() {
                let rect = ivars.frame_spring.borrow().rect();
                self.apply_frame(rect);
            }
            self.apply_reveal();
            if ivars.spring_moving.get() {
                return;
            }
            if ivars.is_anchor_morphing.get() && !ivars.is_dismissing.get() {
                self.finish_anchor_morph();
            }
            self.notify_settled();
        }

        #[unsafe(method(springsSettleImmediately))]
        fn __springs_settle_immediately(&self) {
            let ivars = self.ivars();
            {
                let mut frame_spring = ivars.frame_spring.borrow_mut();
                let target = frame_spring.target_value();
                frame_spring.snap(target);
            }
            {
                let mut reveal_spring = ivars.reveal_spring.borrow_mut();
                let target = reveal_spring.target_value();
                reveal_spring.snap(target);
            }
            ivars.frame_spring_moving.set(false);
            ivars.spring_moving.set(false);
            let rect = ivars.frame_spring.borrow().rect();
            self.apply_frame(rect);
            self.apply_reveal();
            self.notify_settled();
        }

        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            let ivars = self.ivars();
            let bounds = self.bounds();
            ivars.fallback.setFrame(bounds);
            if let Some(glass) = ivars.glass.borrow().as_ref() {
                glass.setFrame(bounds);
            }

            let layout_height = smax(bounds.height(), ivars.content_layout_height.get());
            let layout_width = if ivars.is_anchor_morphing.get() {
                smax(ivars.resting_window_frame.get().width(), bounds.width())
            } else {
                bounds.width()
            };
            let content_frame =
                rect(bounds.width() - layout_width, bounds.height() - layout_height, layout_width, layout_height);
            let glass_layout_content = ivars.glass_layout_content.borrow().clone();
            if ivars.glass.borrow().is_some()
                && let Some(glass_layout_content) = glass_layout_content
            {
                ivars.content.setFrame(glass_layout_content.bounds());
                glass_layout_content.layoutSubtreeIfNeeded();
            }
            if !ivars.is_hosting_content_in_glass.get() {
                ivars.content.setFrame(content_frame);
            }

            ivars.rim_gradient.setFrame(bounds);
            self.apply_reveal();
        }

        #[unsafe(method_id(hitTest:))]
        fn __hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            self.hit_test(point)
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn __accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(cancelOperation:))]
        fn __cancel_operation(&self, _sender: Option<&AnyObject>) {
            self.request_close();
        }
    }
);

impl FloatingPanelSurface {
    /// `init(styleSheet:content:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, content: &NSView, mtm: MainThreadMarker) -> Retained<FloatingPanelSurface> {
        let fallback = PanelBackdrop::new_default(style_sheet.clone(), mtm);
        let this = Self::alloc(mtm).set_ivars(FloatingPanelSurfaceIvars {
            style_sheet: RefCell::new(style_sheet),
            content: content.retain(),
            on_close: RefCell::new(None),
            on_window_frame_change: RefCell::new(None),
            on_frame_spring_settled: RefCell::new(None),
            fallback,
            rim_gradient: CAGradientLayer::new(),
            rim_mask: CAShapeLayer::new(),
            arrival_glint: CAShapeLayer::new(),
            reveal_mask: CAShapeLayer::new(),
            glass: RefCell::new(None),
            glass_layout_content: RefCell::new(None),
            is_hosting_content_in_glass: Cell::new(false),
            content_layout_height: Cell::new(0.0),
            frame_spring: RefCell::new(SpringRect::new(RECT_ZERO, motion::LIQUID_SETTLE, 0.055)),
            reveal_spring: RefCell::new(SpringScalar::new(0.0, 0.0, motion::LIQUID_SETTLE, 0.07)),
            has_spring_frame: Cell::new(false),
            has_reveal_spring: Cell::new(false),
            frame_spring_moving: Cell::new(false),
            spring_moving: Cell::new(false),
            resting_window_frame: Cell::new(RECT_ZERO),
            sliver_window_frame: Cell::new(RECT_ZERO),
            anchor_window_frame: Cell::new(RECT_ZERO),
            is_anchor_morphing: Cell::new(false),
            is_dismissing: Cell::new(false),
            uses_glass: Cell::new(false),
        });
        let this: Retained<FloatingPanelSurface> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.finish_init();
        this
    }

    fn finish_init(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        self.setWantsLayer(true);
        if let Some(layer) = self.layer() {
            layer.setShadowColor(Some(&cg(&NSColor::blackColor())));
            layer.setShadowRadius(24.0);
            layer.setShadowOffset(NSSize::new(0.0, -8.0));
            layer.setShadowOpacity(if style_sheet.increase_contrast { 0.34 } else { 0.22 });
        }

        ivars.rim_mask.setFillColor(Some(&cg(&NSColor::clearColor())));
        ivars.rim_mask.setStrokeColor(Some(&cg(&NSColor::whiteColor())));
        null_actions(&ivars.rim_mask, &["path", "frame", "strokeColor"]);
        set_mask(&ivars.rim_gradient, Some(&ivars.rim_mask));
        null_actions(&ivars.rim_gradient, &["frame", "colors"]);
        ivars.rim_gradient.setZPosition(100.0);
        if let Some(layer) = self.layer() {
            layer.addSublayer(&ivars.rim_gradient);
        }

        ivars.arrival_glint.setFillColor(Some(&cg(&NSColor::clearColor())));
        ivars.arrival_glint.setStrokeColor(Some(&cg(&NSColor::whiteColor().colorWithAlphaComponent(0.62))));
        ivars.arrival_glint.setLineWidth(1.15);
        ivars.arrival_glint.setLineCap(unsafe { kCALineCapRound });
        ivars.arrival_glint.setOpacity(0.0);
        ivars.arrival_glint.setZPosition(101.0);
        null_actions(&ivars.arrival_glint, &["path", "frame", "opacity", "strokeStart", "strokeEnd"]);
        if let Some(layer) = self.layer() {
            layer.addSublayer(&ivars.arrival_glint);
        }

        ivars.reveal_mask.setFillColor(Some(&cg(&NSColor::blackColor())));
        null_actions(&ivars.reveal_mask, &["path", "frame"]);
        if let Some(layer) = self.layer() {
            set_mask(&layer, Some(&ivars.reveal_mask));
        }

        ivars.fallback.set_blends_within_window(false);
        ivars.fallback.set_uses_surface_fill(true);
        ivars.fallback.set_opaque_surface_color(Some(Self::opaque_fallback_color(&style_sheet)));
        ivars.fallback.set_veil_alpha(0.0);
        ivars
            .fallback
            .setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
        ivars.fallback.setFrame(self.bounds());
        if !Self::supports_glass(&style_sheet) {
            self.addSubview_positioned_relativeTo(&ivars.fallback, NSWindowOrderingMode::Below, None);
        }
        self.mount_content_on_self();
        set_role(self, role::group());
        let label = super::appkit_support::accessibility_label(&*ivars.content);
        set_label(self, label.as_deref().unwrap_or("Floating panel"));
        self.update_material();
        self.apply_surface_style();
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        let ivars = self.ivars();
        *ivars.style_sheet.borrow_mut() = style_sheet.clone();
        ivars.fallback.set_style_sheet(style_sheet.clone());
        ivars.fallback.set_opaque_surface_color(Some(Self::opaque_fallback_color(&style_sheet)));
        if let Some(host) = downcast::<InspectorHostView>(&ivars.content) {
            host.set_style_sheet(style_sheet);
        }
        self.update_material();
        self.apply_surface_style();
    }

    pub fn content(&self) -> Retained<NSView> {
        self.ivars().content.clone()
    }

    pub fn set_on_close(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_close.borrow_mut() = handler;
    }

    pub fn set_on_window_frame_change(&self, handler: Option<Rc<dyn Fn(NSRect)>>) {
        *self.ivars().on_window_frame_change.borrow_mut() = handler;
    }

    pub fn set_on_frame_spring_settled(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_frame_spring_settled.borrow_mut() = handler;
    }

    pub fn is_dismissing(&self) -> bool {
        self.ivars().is_dismissing.get()
    }

    pub fn uses_glass(&self) -> bool {
        self.ivars().uses_glass.get()
    }

    fn notify_settled(&self) {
        let handler = self.ivars().on_frame_spring_settled.borrow().clone();
        if let Some(handler) = handler {
            handler();
        }
    }

    fn glass_effect(&self) -> Option<Retained<NSGlassEffectView>> {
        let glass = self.ivars().glass.borrow().clone()?;
        downcast::<NSGlassEffectView>(&glass)
    }

    // MARK: - Geometry

    /// The host calls this once before the surface is shown.
    pub fn set_resting_frame(&self, frame: NSRect) {
        if !Self::is_usable_frame(frame) {
            return;
        }
        let ivars = self.ivars();
        ivars.frame_spring.borrow_mut().snap(frame);
        ivars.has_spring_frame.set(true);
        self.apply_frame(frame);
        if !ivars.has_reveal_spring.get() {
            ivars.reveal_spring.borrow_mut().snap(frame.height());
            ivars.has_reveal_spring.set(true);
            self.apply_reveal();
        }
    }

    pub fn configure_window_frames(&self, resting: NSRect, sliver: NSRect, content_height: CGFloat) {
        if !(Self::is_usable_frame(resting) && Self::is_usable_frame(sliver)) {
            return;
        }
        let ivars = self.ivars();
        ivars.resting_window_frame.set(resting);
        ivars.sliver_window_frame.set(sliver);
        ivars.content_layout_height.set(smax(0.0, content_height));
        if !ivars.has_reveal_spring.get() {
            return;
        }
        let target = if ivars.is_dismissing.get() { sliver.height() } else { resting.height() };
        ivars.reveal_spring.borrow_mut().target(target);
    }

    pub fn present_from_sliver(&self, animated: bool) {
        let ivars = self.ivars();
        ivars.is_dismissing.set(false);
        if !ivars.has_spring_frame.get() {
            self.set_resting_frame(ivars.sliver_window_frame.get());
        }
        ivars.reveal_spring.borrow_mut().snap(ivars.sliver_window_frame.get().height());
        self.apply_reveal();
        if !(animated && !self.style_sheet().reduce_motion) {
            ivars.reveal_spring.borrow_mut().snap(ivars.resting_window_frame.get().height());
            self.apply_reveal();
            self.notify_settled();
            return;
        }
        ivars.reveal_spring.borrow_mut().target(ivars.resting_window_frame.get().height());
        self.arm_springs();
    }

    pub fn dismiss_to_sliver(&self, animated: bool) {
        let ivars = self.ivars();
        ivars.is_dismissing.set(true);
        if !(animated && !self.style_sheet().reduce_motion) {
            ivars.reveal_spring.borrow_mut().snap(ivars.sliver_window_frame.get().height());
            self.apply_reveal();
            self.notify_settled();
            return;
        }
        ivars.reveal_spring.borrow_mut().target(ivars.sliver_window_frame.get().height());
        self.arm_springs();
    }

    /// Prime the actual panel at the invoking control's host-space frame.
    pub fn prepare_anchor_presentation(&self, anchor: NSRect) {
        let ivars = self.ivars();
        if !Self::is_usable_anchor(anchor) {
            ivars.is_anchor_morphing.set(false);
            self.set_resting_frame(ivars.resting_window_frame.get());
            return;
        }
        ivars.is_dismissing.set(false);
        ivars.is_anchor_morphing.set(true);
        ivars.anchor_window_frame.set(anchor);
        ivars.frame_spring.borrow_mut().snap(anchor);
        ivars.reveal_spring.borrow_mut().snap(anchor.height());
        ivars.has_spring_frame.set(true);
        ivars.has_reveal_spring.set(true);
        ivars.content.setAlphaValue(0.0);
        self.apply_frame(anchor);
        self.apply_reveal();
    }

    pub fn start_anchor_presentation(&self, animated: bool) {
        let ivars = self.ivars();
        if !(ivars.is_anchor_morphing.get() && !ivars.is_dismissing.get()) {
            return;
        }
        let resting = ivars.resting_window_frame.get();
        if !(animated && !self.style_sheet().reduce_motion) {
            ivars.frame_spring.borrow_mut().snap(resting);
            ivars.reveal_spring.borrow_mut().snap(resting.height());
            self.apply_frame(resting);
            self.apply_reveal();
            self.finish_anchor_morph();
            self.notify_settled();
            return;
        }
        ivars.frame_spring.borrow_mut().target(resting);
        ivars.reveal_spring.borrow_mut().target(resting.height());
        self.arm_springs();
    }

    pub fn dismiss_to_anchor(&self, anchor: NSRect, animated: bool) {
        if !Self::is_usable_anchor(anchor) {
            self.dismiss_to_sliver(animated);
            return;
        }
        let ivars = self.ivars();
        ivars.is_dismissing.set(true);
        ivars.is_anchor_morphing.set(true);
        ivars.anchor_window_frame.set(anchor);
        if !(animated && !self.style_sheet().reduce_motion) {
            ivars.frame_spring.borrow_mut().snap(anchor);
            ivars.reveal_spring.borrow_mut().snap(anchor.height());
            self.apply_frame(anchor);
            self.apply_reveal();
            self.notify_settled();
            return;
        }
        ivars.frame_spring.borrow_mut().target(anchor);
        ivars.reveal_spring.borrow_mut().target(anchor.height());
        self.arm_springs();
    }

    /// Deterministic landing hook for geometry tests.
    pub fn settle_for_testing(&self) {
        let _: () = unsafe { msg_send![self, springsSettleImmediately] };
    }

    /// Gives Auto Layout the width it will actually receive before asking
    /// for a fitting height.
    pub fn prepare_for_measurement(&self, width: CGFloat, height: CGFloat) {
        let ivars = self.ivars();
        let size = NSSize::new(width, smax(1.0, height));
        if ivars.on_window_frame_change.borrow().is_none() {
            let mut frame = self.frame();
            if frame.size != size {
                frame.size = size;
                self.setFrame(frame);
            }
        } else {
            let mut frame = self.frame();
            frame.size.width = size.width;
            self.setFrame(frame);
        }
        ivars.content_layout_height.set(smax(ivars.content_layout_height.get(), size.height));
        self.layoutSubtreeIfNeeded();
        ivars.content.layoutSubtreeIfNeeded();
    }

    pub fn set_content_layout_height(&self, height: CGFloat) {
        self.ivars().content_layout_height.set(smax(0.0, height));
        self.setNeedsLayout(true);
    }

    /// Height changes use the shared rect spring.
    pub fn retarget_frame(&self, frame: NSRect, animated: bool) {
        if !Self::is_usable_frame(frame) {
            return;
        }
        let ivars = self.ivars();
        if !ivars.has_spring_frame.get() {
            self.set_resting_frame(self.frame());
        }
        if !(animated && !self.style_sheet().reduce_motion && self.window().is_some() && !self.inLiveResize()) {
            ivars.frame_spring.borrow_mut().snap(frame);
            let reveal = if ivars.is_dismissing.get() { ivars.sliver_window_frame.get().height() } else { frame.height() };
            ivars.reveal_spring.borrow_mut().snap(reveal);
            self.apply_frame(frame);
            self.apply_reveal();
            return;
        }
        ivars.frame_spring.borrow_mut().target(frame);
        self.arm_springs();
    }

    fn apply_frame(&self, frame: NSRect) {
        let handler = self.ivars().on_window_frame_change.borrow().clone();
        match handler {
            Some(handler) => handler(frame),
            None => self.setFrame(frame),
        }
        if self.ivars().is_anchor_morphing.get() {
            self.update_anchor_morph_visuals(frame);
        } else {
            self.update_surface_opacity(frame);
        }
        self.setNeedsLayout(true);
    }

    fn update_anchor_morph_visuals(&self, frame: NSRect) {
        let ivars = self.ivars();
        let anchor = ivars.anchor_window_frame.get();
        let width_span = ivars.resting_window_frame.get().width() - anchor.width();
        let raw = if width_span.abs() > 0.5 { (frame.width() - anchor.width()) / width_span } else { 1.0 };
        let progress = Self::clamped_unit(raw);
        let content_progress = Self::clamped_unit((progress - 0.56) / 0.28);
        let smooth = content_progress * content_progress * (3.0 - 2.0 * content_progress);
        ivars.content.setAlphaValue(smooth);
        self.setAlphaValue(1.0);
        let radius = anchor.height() / 2.0 + (Top::CORNER_RADIUS - anchor.height() / 2.0) * progress;
        if available!(macos = 26.0)
            && let Some(glass) = self.glass_effect()
        {
            glass.setCornerRadius(radius);
        }
        if let Some(layer) = self.layer() {
            let contrast = self.style_sheet().increase_contrast;
            layer.setShadowOpacity(((if contrast { 0.28 } else { 0.14 }) * progress) as f32);
        }
        ivars.rim_gradient.setOpacity(progress as f32);
    }

    fn finish_anchor_morph(&self) {
        let ivars = self.ivars();
        ivars.is_anchor_morphing.set(false);
        ivars.content.setAlphaValue(1.0);
        if available!(macos = 26.0)
            && let Some(glass) = self.glass_effect()
        {
            glass.setCornerRadius(Top::CORNER_RADIUS);
        }
        ivars.rim_gradient.setOpacity(1.0);
        self.apply_surface_style();
    }

    fn clamped_unit(value: CGFloat) -> CGFloat {
        if !value.is_finite() {
            return 0.0;
        }
        smin(1.0, smax(0.0, value))
    }

    fn is_usable_anchor(rect: NSRect) -> bool {
        rect.origin.x.is_finite()
            && rect.origin.y.is_finite()
            && rect.width().is_finite()
            && rect.height().is_finite()
            && rect.width() > 1.0
            && rect.height() > 1.0
    }

    fn is_usable_frame(rect: NSRect) -> bool {
        rect.origin.x.is_finite()
            && rect.origin.y.is_finite()
            && rect.width().is_finite()
            && rect.height().is_finite()
            && rect.width() > 1.0
            && rect.height() > 1.0
    }

    fn update_surface_opacity(&self, frame: NSRect) {
        let ivars = self.ivars();
        let sliver = ivars.sliver_window_frame.get();
        let resting = ivars.resting_window_frame.get();
        if !(sliver.height() > 0.0 && resting.height() > sliver.height()) {
            self.setAlphaValue(1.0);
            return;
        }
        let span = resting.height() - sliver.height();
        let progress = smin(1.0, smax(0.0, (frame.height() - sliver.height()) / span));
        let sliver_opacity = motion::FLOATING_SURFACE_SLIVER_OPACITY;
        self.setAlphaValue(smin(
            1.0,
            sliver_opacity + (1.0 - sliver_opacity) * progress / motion::FLOATING_SURFACE_PRESENCE_FRACTION,
        ));
    }

    fn apply_reveal(&self) {
        let ivars = self.ivars();
        let bounds = self.bounds();
        if !(bounds.height() > 0.0) {
            return;
        }
        if ivars.is_anchor_morphing.get() {
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            if let Some(layer) = self.layer() {
                set_mask(&layer, None);
            }
            let radius = smin(Top::CORNER_RADIUS, smin(bounds.width(), bounds.height()) / 2.0);
            ivars.rim_mask.setFrame(bounds);
            ivars.rim_mask.setPath(Some(&Self::top_rim_path(bounds.inset_by(0.75, 0.75), smax(0.0, radius - 0.75))));
            ivars.arrival_glint.setFrame(bounds);
            ivars.arrival_glint.setPath(ivars.rim_mask.path().as_deref());
            CATransaction::commit();
            return;
        }
        let resting = ivars.resting_window_frame.get();
        let sliver = ivars.sliver_window_frame.get();
        let height = smin(bounds.height(), smax(0.0, ivars.reveal_spring.borrow().value()));
        let progress = if resting.height() > sliver.height() {
            smin(1.0, smax(0.0, (height - sliver.height()) / (resting.height() - sliver.height())))
        } else {
            1.0
        };
        let radius = smin(Top::CORNER_RADIUS * (0.55 + 0.45 * progress), smin(bounds.width(), smax(1.0, height)) / 2.0);
        let seed_width = smin(44.0, bounds.width());
        let width_progress = 1.0 - upleft_render::swift_compat::pow(1.0 - progress, 1.65);
        let visible_width = seed_width + (bounds.width() - seed_width) * width_progress;
        let visible_rect = rect(bounds.max_x() - visible_width, bounds.max_y() - height, visible_width, height);
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        ivars.reveal_mask.setFrame(bounds);
        ivars.reveal_mask.setPath(Some(&PanelMetrics::continuous_rounded_path(visible_rect, radius)));
        if let Some(layer) = self.layer() {
            set_mask(&layer, if height >= bounds.height() - 0.5 { None } else { Some(&ivars.reveal_mask) });
            layer.setShadowPath(ivars.reveal_mask.path().as_deref());
        }
        ivars.rim_mask.setFrame(bounds);
        let rim_rect = visible_rect.inset_by(0.75, 0.75);
        ivars.rim_mask.setPath(Some(&*if ivars.uses_glass.get() {
            Self::top_rim_path(rim_rect, smax(0.0, radius - 0.75))
        } else {
            PanelMetrics::continuous_rounded_path(rim_rect, smax(0.0, radius - 0.75))
        }));
        ivars.arrival_glint.setFrame(bounds);
        ivars.arrival_glint.setPath(ivars.rim_mask.path().as_deref());
        if let Some(layer) = self.layer() {
            layer.setAffineTransform(objc2_core_foundation::CGAffineTransform {
                a: 1.0,
                b: 0.0,
                c: 0.0,
                d: 1.0,
                tx: 0.0,
                ty: 0.0,
            });
        }
        CATransaction::commit();
        self.update_surface_opacity(rect(resting.min_x(), resting.min_y(), resting.width(), height));
        self.setNeedsLayout(true);
    }

    /// `fittedContentHeight`.
    pub fn fitted_content_height(&self) -> CGFloat {
        let content = &self.ivars().content;
        if let Some(host) = downcast::<InspectorHostView>(content) {
            return host.floating_fitting_height();
        }
        let height = content.fittingSize().height;
        if height.is_finite() && height > 0.0 { height } else { 0.0 }
    }

    /// `preferredWidth`.
    pub fn preferred_width(&self) -> CGFloat {
        panel_surface_preferred_width(&self.ivars().content).unwrap_or(PanelMetrics::DETAIL_WIDTH)
    }

    pub fn resting_window_frame_for_morph(&self) -> NSRect {
        self.ivars().resting_window_frame.get()
    }

    pub fn current_window_frame_for_testing(&self) -> NSRect {
        self.ivars().frame_spring.borrow().rect()
    }

    pub fn content_layout_height_for_testing(&self) -> CGFloat {
        self.ivars().content_layout_height.get()
    }

    // MARK: - Material

    pub fn supports_glass(style_sheet: &StyleSheet) -> bool {
        ChromeGlass::supports_glass(style_sheet)
    }

    pub fn glass_tint(style_sheet: &StyleSheet) -> Retained<NSColor> {
        ChromeGlass::glass_tint(style_sheet, Tint::Panel)
    }

    fn opaque_fallback_color(style_sheet: &StyleSheet) -> Retained<NSColor> {
        ChromeGlass::opaque_fallback_color(style_sheet, Tint::Panel)
    }

    fn glass_material_alpha(_style_sheet: &StyleSheet) -> CGFloat {
        1.0
    }

    fn clear_glass_dimming_color(_style_sheet: &StyleSheet) -> Retained<NSColor> {
        NSColor::clearColor()
    }

    fn is_dark_background(color: &NSColor) -> bool {
        ChromeGlass::is_dark_background(color)
    }

    fn update_material(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        let wants_glass = Self::supports_glass(&style_sheet);
        match (ivars.uses_glass.get(), wants_glass) {
            (false, false) => {
                if ivars.is_hosting_content_in_glass.get() || !is_same_view(superview(&ivars.content), self) {
                    self.mount_content_on_self();
                }
            }
            (false, true) => {
                ivars.uses_glass.set(true);
                if available!(macos = 26.0) {
                    self.mount_content_on_glass();
                }
            }
            (true, false) => {
                ivars.uses_glass.set(false);
                self.mount_content_on_self();
            }
            (true, true) => {
                if available!(macos = 26.0) {
                    if let Some(glass) = self.glass_effect() {
                        glass.setAppearance(ChromeGlass::material_appearance(&style_sheet).as_deref());
                    }
                    if let Some(glass) = self.glass_effect() {
                        glass.setTintColor(None);
                    }
                    if let Some(glass) = self.glass_effect() {
                        glass.setAlphaValue(Self::glass_material_alpha(&style_sheet));
                    }
                    if let Some(content) = ivars.glass_layout_content.borrow().as_ref()
                        && let Some(layer) = content.layer()
                    {
                        layer.setBackgroundColor(Some(&cg(&Self::clear_glass_dimming_color(&style_sheet))));
                    }
                }
            }
        }
        self.apply_surface_style();
    }

    /// AppKit requires controls to be the glass view's `contentView`.
    fn mount_content_on_glass(&self) {
        let ivars = self.ivars();
        let mtm = self.mtm();
        let style_sheet = self.style_sheet();
        ivars.fallback.removeFromSuperview();
        if let Some(glass) = ivars.glass.borrow().as_ref() {
            glass.removeFromSuperview();
        }
        let material = NSGlassEffectView::new(mtm);
        material.setStyle(NSGlassEffectViewStyle::Clear);
        material.setAppearance(ChromeGlass::material_appearance(&style_sheet).as_deref());
        material.setTintColor(None);
        material.setAlphaValue(Self::glass_material_alpha(&style_sheet));
        material.setCornerRadius(Top::CORNER_RADIUS);
        material.setWantsLayer(true);
        if let Some(layer) = material.layer() {
            layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
        }
        material.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
        material.setFrame(self.bounds());
        material.setClipsToBounds(true);
        self.addSubview_positioned_relativeTo(&material, NSWindowOrderingMode::Above, None);
        *ivars.glass.borrow_mut() = Some(Retained::into_super(material.clone()));

        ivars.content.removeFromSuperview();
        let layout_content = NSView::initWithFrame(NSView::alloc(mtm), self.bounds());
        layout_content.setWantsLayer(true);
        if let Some(layer) = layout_content.layer() {
            layer.setBackgroundColor(Some(&cg(&Self::clear_glass_dimming_color(&style_sheet))));
        }
        layout_content.setClipsToBounds(true);
        material.setContentView(Some(&layout_content));
        layout_content.addSubview(&ivars.content);
        *ivars.glass_layout_content.borrow_mut() = Some(layout_content.clone());
        ivars.content.setTranslatesAutoresizingMaskIntoConstraints(true);
        ivars
            .content
            .setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
        ivars.content.setFrame(layout_content.bounds());
        ivars.is_hosting_content_in_glass.set(true);
        PanelBackdrop::resolve_detached_glass(&ivars.content);
    }

    /// Glass is created before attachment so measurement can run without a
    /// window.
    pub fn refresh_glass_after_window_attach(&self) {
        if !(self.ivars().uses_glass.get() && available!(macos = 26.0)) {
            return;
        }
        let style_sheet = self.style_sheet();
        if let Some(glass) = self.glass_effect() {
            glass.setAppearance(ChromeGlass::material_appearance(&style_sheet).as_deref());
        }
        if let Some(glass) = self.glass_effect() {
            glass.setTintColor(None);
        }
        if let Some(glass) = self.glass_effect() {
            glass.setAlphaValue(Self::glass_material_alpha(&style_sheet));
        }
        if let Some(content) = self.ivars().glass_layout_content.borrow().as_ref()
            && let Some(layer) = content.layer()
        {
            layer.setBackgroundColor(Some(&cg(&Self::clear_glass_dimming_color(&style_sheet))));
        }
        self.apply_surface_style();
    }

    pub fn glass_identity_for_testing(&self) -> Option<*const NSView> {
        self.ivars().glass.borrow().as_ref().map(|glass| Retained::as_ptr(glass))
    }

    pub fn glass_alpha_for_testing(&self) -> Option<CGFloat> {
        self.ivars().glass.borrow().as_ref().map(|glass| glass.alphaValue())
    }

    pub fn uses_clear_native_glass_for_testing(&self) -> bool {
        if !available!(macos = 26.0) {
            return false;
        }
        self.glass_effect()
            .is_some_and(|glass| glass.style() == NSGlassEffectViewStyle::Clear && glass.tintColor().is_none())
    }

    pub fn content_shares_glass_opacity_for_testing(&self) -> bool {
        match self.ivars().glass.borrow().as_ref() {
            Some(glass) => self.ivars().content.isDescendantOf(glass),
            None => false,
        }
    }

    pub fn opaque_fallback_is_mounted_for_testing(&self) -> bool {
        superview(&self.ivars().fallback).is_some()
    }

    /// `visibleBodyBoundsForHitTesting`.
    pub fn visible_body_bounds_for_hit_testing(&self) -> NSRect {
        let ivars = self.ivars();
        let bounds = self.bounds();
        let height = if ivars.is_anchor_morphing.get() {
            smin(bounds.height(), smax(0.0, ivars.frame_spring.borrow().rect().height()))
        } else {
            smin(bounds.height(), smax(0.0, ivars.reveal_spring.borrow().value()))
        };
        rect(bounds.min_x(), bounds.max_y() - height, bounds.width(), height)
    }

    pub fn visible_body_height_for_testing(&self) -> CGFloat {
        self.visible_body_bounds_for_hit_testing().height()
    }

    pub fn close_control_hit_zone_contains(&self, point: NSPoint) -> bool {
        let Some(host) = downcast::<InspectorHostView>(&self.ivars().content) else { return false };
        let button = host.close_button_for_testing();
        let button_frame = self.convertRect_fromView(button.bounds(), Some(&button));
        button_frame.inset_by(-18.0, -18.0).contains_point(point)
    }

    pub fn close_control_hit_zone_contains_window_point(&self, point: NSPoint) -> bool {
        let Some(host) = downcast::<InspectorHostView>(&self.ivars().content) else { return false };
        let button = host.close_button_for_testing();
        let button_frame = button.convertRect_toView(button.bounds(), None);
        let hit_zone = rect(button_frame.mid_x() - 28.0, button_frame.mid_y() - 28.0, 56.0, 56.0);
        hit_zone.contains_point(point)
    }

    pub fn request_close(&self) {
        let handler = self.ivars().on_close.borrow().clone();
        if let Some(handler) = handler {
            handler();
        }
    }

/// AppKit supplies `point` in the receiver's superview coordinates.
fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
        let local_point = if self.bounds().contains_point(point) {
            point
        } else {
            match superview(&self) {
                Some(superview) => self.convertPoint_fromView(point, Some(&superview)),
                None => point,
            }
        };
        if !self.visible_body_bounds_for_hit_testing().contains_point(local_point) {
            return None;
        }
        let content = self.ivars().content.clone();
        if let Some(host) = downcast::<InspectorHostView>(&content)
            && self.close_control_hit_zone_contains(local_point)
        {
            return Some(host.close_button_for_testing());
        }
        let content_point = content.convertPoint_fromView(local_point, Some(self));
        let hit_test_point = match superview(&content) {
            Some(superview) => superview.convertPoint_fromView(local_point, Some(self)),
            None => content_point,
        };
        if content.bounds().contains_point(content_point)
            && let Some(hit) = content.hitTest(hit_test_point)
        {
            return Some(hit);
        }
        unsafe { msg_send![super(self), hitTest: point] }
    }

    /// Finds a native control by geometry.
    pub fn native_control(&self, point: NSPoint) -> Option<Retained<NSView>> {
        fn target(view: &NSView, point: NSPoint) -> Option<Retained<NSView>> {
            if view.isHidden() || !(view.alphaValue() > 0.01) {
                return None;
            }
            let local = view.convertPoint_fromView(point, None);
            if !view.bounds().contains_point(local) {
                return None;
            }
            for child in view.subviews().to_vec().into_iter().rev() {
                if let Some(hit) = target(&child, point) {
                    return Some(hit);
                }
            }
            if let Some(field) = downcast::<NSTextField>(view)
                && (field.isEditable() || field.isSelectable())
            {
                return Some(Retained::into_super(Retained::into_super(field)));
            }
            if super::appkit_support::is::<NSButton>(view)
                || super::appkit_support::is::<NSPopUpButton>(view)
                || super::appkit_support::is::<NSSegmentedControl>(view)
                || super::appkit_support::is::<NSTableView>(view)
                || super::appkit_support::is::<NSScrollView>(view)
            {
                return Some(view.retain());
            }
            None
        }
        target(&self.ivars().content, point)
    }

    fn mount_content_on_self(&self) {
        let ivars = self.ivars();
        if let Some(glass) = ivars.glass.borrow().as_ref() {
            glass.removeFromSuperview();
        }
        *ivars.glass.borrow_mut() = None;
        if !is_same_view(superview(&ivars.fallback), self) {
            ivars.fallback.setFrame(self.bounds());
            self.addSubview_positioned_relativeTo(&ivars.fallback, NSWindowOrderingMode::Below, None);
        }
        if ivars.is_hosting_content_in_glass.get() {
            ivars.content.removeFromSuperview();
        }
        *ivars.glass_layout_content.borrow_mut() = None;
        ivars.content.setTranslatesAutoresizingMaskIntoConstraints(true);
        ivars
            .content
            .setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
        if !is_same_view(superview(&ivars.content), self) {
            self.addSubview(&ivars.content);
        }
        ivars.content.setFrame(self.bounds());
        ivars.is_hosting_content_in_glass.set(false);
        PanelBackdrop::resolve_detached_glass(&ivars.content);
    }

    fn apply_surface_style(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        if let Some(layer) = self.layer() {
            layer.setShadowRadius(if style_sheet.increase_contrast { 42.0 } else { 40.0 });
            layer.setShadowOffset(NSSize::new(0.0, -10.0));
            layer.setShadowOpacity(if style_sheet.increase_contrast { 0.28 } else { 0.14 });
        }
        let is_dark = Self::is_dark_background(&style_sheet.background);
        let specular = NSColor::whiteColor();
        let rim_alpha: CGFloat = if style_sheet.increase_contrast {
            0.92
        } else if is_dark {
            0.58
        } else {
            0.72
        };
        let muted_alpha: CGFloat = if style_sheet.increase_contrast { 0.24 } else { 0.08 };
        let colors = cg_array(&[
            cg(&specular.colorWithAlphaComponent(rim_alpha)),
            cg(&specular.colorWithAlphaComponent(rim_alpha * 0.44)),
            cg(&specular.colorWithAlphaComponent(muted_alpha)),
            cg(&specular.colorWithAlphaComponent(muted_alpha * 0.35)),
        ]);
        unsafe { ivars.rim_gradient.setColors(Some(&colors)) };
        ivars.rim_gradient.setStartPoint(CGPoint::new(0.08, 0.98));
        ivars.rim_gradient.setEndPoint(CGPoint::new(0.92, 0.02));
        ivars.rim_mask.setLineWidth(if style_sheet.increase_contrast { 1.5 } else { 1.0 });
        ivars.rim_gradient.setHidden(false);

        let uses_glass = ivars.uses_glass.get();
        if let Some(layer) = self.layer() {
            layer.setCornerRadius(if uses_glass { 0.0 } else { Top::CORNER_RADIUS });
            layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
            layer.setMasksToBounds(false);
        }
        if !uses_glass {
            ivars.fallback.setWantsLayer(true);
            if let Some(layer) = ivars.fallback.layer() {
                layer.setCornerRadius(Top::CORNER_RADIUS);
                layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
                layer.setMasksToBounds(true);
            }
        }
        self.setNeedsDisplay(true);
    }

    /// Native glass supplies the body edge; this path adds only the top
    /// specular.
    fn top_rim_path(rect: NSRect, radius: CGFloat) -> CFRetained<CGPath> {
        let radius = smin(radius, smin(rect.width(), rect.height()) / 2.0);
        let path = CGMutablePath::new();
        let kappa: CGFloat = 0.552_284_75;
        let p = Some(&*path);
        // SAFETY: `IDENTITY` outlives each call.
        unsafe {
            CGMutablePath::move_to_point(p, &IDENTITY, rect.min_x(), rect.max_y() - radius);
            CGMutablePath::add_curve_to_point(
                p,
                &IDENTITY,
                rect.min_x(),
                rect.max_y() - radius + radius * kappa,
                rect.min_x() + radius - radius * kappa,
                rect.max_y(),
                rect.min_x() + radius,
                rect.max_y(),
            );
            CGMutablePath::add_line_to_point(p, &IDENTITY, rect.max_x() - radius, rect.max_y());
            CGMutablePath::add_curve_to_point(
                p,
                &IDENTITY,
                rect.max_x() - radius + radius * kappa,
                rect.max_y(),
                rect.max_x(),
                rect.max_y() - radius + radius * kappa,
                rect.max_x(),
                rect.max_y() - radius,
            );
        }
        super::appkit_support::immutable(path)
    }

    /// Prime content while the travelling glass is still the visible body.
    pub fn prepare_for_morph_arrival(&self) {
        let ivars = self.ivars();
        ivars.is_dismissing.set(false);
        ivars.content.setWantsLayer(true);
        if let Some(layer) = ivars.content.layer() {
            layer.removeAnimationForKey(&NSString::from_str("floating-content-arrival"));
            layer.setTransform(CATransform3D::new_translation(0.0, -6.0, 0.0));
        }
    }

    pub fn prepare_for_morph_dismissal(&self) {
        let ivars = self.ivars();
        ivars.is_dismissing.set(true);
        ivars.arrival_glint.removeAllAnimations();
        if let Some(layer) = ivars.content.layer() {
            layer.removeAnimationForKey(&NSString::from_str("floating-content-arrival"));
        }
    }

    /// The vessel calls this at the empty-glass handoff.
    pub fn play_morph_arrival_details(&self) {
        let ivars = self.ivars();
        if !(!ivars.is_dismissing.get() && !self.style_sheet().reduce_motion && self.window().is_some()) {
            if let Some(layer) = ivars.content.layer() {
                layer.setTransform(unsafe { CATransform3DIdentity });
            }
            return;
        }
        let settle = CAKeyframeAnimation::animationWithKeyPath(Some(&NSString::from_str("transform")));
        let values = NSArray::from_retained_slice(&[
            transform_value(CATransform3D::new_translation(0.0, -6.0, 0.0)),
            transform_value(CATransform3D::new_translation(0.0, 1.0, 0.0)),
            transform_value(unsafe { CATransform3DIdentity }),
        ]);
        unsafe { settle.setValues(Some(Retained::cast_unchecked::<NSArray>(values).as_ref())) };
        settle.setKeyTimes(Some(&NSArray::from_retained_slice(&[
            NSNumber::new_f64(0.0),
            NSNumber::new_f64(0.76),
            NSNumber::new_f64(1.0),
        ])));
        settle.setDuration(motion::LIQUID_SETTLE);
        settle.setTimingFunctions(Some(&NSArray::from_retained_slice(&[
            motion::timing(Curve::Structural),
            motion::timing(Curve::Decelerate),
        ])));
        if let Some(layer) = ivars.content.layer() {
            layer.setTransform(unsafe { CATransform3DIdentity });
            layer.addAnimation_forKey(&settle, Some(&NSString::from_str("floating-content-arrival")));
        }

        ivars.arrival_glint.removeAllAnimations();
        ivars.arrival_glint.setStrokeStart(0.0);
        ivars.arrival_glint.setStrokeEnd(1.0);
        let start = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("strokeStart")));
        set_number_values(&start, Some(0.0), 0.82);
        let end = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("strokeEnd")));
        set_number_values(&end, Some(0.08), 1.0);
        let opacity = CAKeyframeAnimation::animationWithKeyPath(Some(&NSString::from_str("opacity")));
        let opacity_values =
            NSArray::from_retained_slice(&[NSNumber::new_f64(0.0), NSNumber::new_f64(0.42), NSNumber::new_f64(0.0)]);
        unsafe { opacity.setValues(Some(Retained::cast_unchecked::<NSArray>(opacity_values).as_ref())) };
        opacity.setKeyTimes(Some(&NSArray::from_retained_slice(&[
            NSNumber::new_f64(0.0),
            NSNumber::new_f64(0.38),
            NSNumber::new_f64(1.0),
        ])));
        let group = CAAnimationGroup::animation();
        let animations: Retained<NSArray<CAAnimation>> = NSArray::from_retained_slice(&[
            Retained::into_super(Retained::into_super(start)),
            Retained::into_super(Retained::into_super(end)),
            Retained::into_super(Retained::into_super(opacity)),
        ]);
        group.setAnimations(Some(&animations));
        group.setDuration(motion::LIQUID_SETTLE);
        group.setTimingFunction(Some(&motion::timing(Curve::Structural)));
        ivars.arrival_glint.addAnimation_forKey(&group, Some(&NSString::from_str("floating-arrival-glint")));
    }

    /// Tests use this to assert the material has a drawable body.
    pub fn renders_body_for_testing(&self) -> bool {
        let ivars = self.ivars();
        let bounds = self.bounds();
        let body = match ivars.glass.borrow().as_ref() {
            Some(glass) => glass.bounds(),
            None => ivars.fallback.bounds(),
        };
        bounds.width() > 0.0 && bounds.height() > 0.0 && body.width() > 0.0 && body.height() > 0.0
    }
}

fn transform_value(transform: objc2_quartz_core::CATransform3D) -> Retained<objc2_foundation::NSValue> {
    // SAFETY: a plain value conversion.
    unsafe { objc2_foundation::NSValue::valueWithCATransform3D(transform) }
}

#[allow(unused)]
fn _unused(_: CGRect, _: &CABasicAnimation) {}
