//! Port of the parts of `App/ToolbarControls.swift` the panels build on:
//! `ToolbarChromePolicy` and `ToolbarInteractiveButton`.
//!
//! **Partial port, made on `port/panels`.** `BreadcrumbView`,
//! `CommandPaletteView`, `TaskProgressRing`, `UpdateStatusPill` and
//! `PanelChrome` use these two types, and the app shell's port of this file
//! was not on `main` yet. `port/app-shell` owns this file: when it merges
//! `port/panels`, its complete port replaces this one, keeping these names
//! and signatures (`ToolbarChromePolicy::{HOVER_DURATION, PRESS_IN_DURATION,
//! PRESS_OUT_DURATION, SELECTION_DURATION, EMPHASIS_DURATION, PRESSED_SCALE,
//! RING_PRESSED_SCALE, timing_function, feedback_opacity,
//! indicator_opacity, scrub_state_for_position, scrub_state}`,
//! `InteractionState`, `ScrubState`, and `ToolbarInteractiveButton` with its
//! Objective-C overridables `styleSheetDidChange` and
//! `permitsHoverFeedback`). The Objective-C class `ToolbarInteractiveButton`
//! must be registered exactly once.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::{Allocated, Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSButton, NSColor, NSControl, NSEvent, NSResponder, NSTrackingArea, NSTrackingAreaOptions, NSView, NSWorkspace,
    NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSNotification, NSOperationQueue, NSRect, NSString};
use objc2_quartz_core::{CABasicAnimation, CALayer, CAMediaTiming, CAMediaTimingFunction, CATransform3D};
use upleft_render::motion::{self, Curve};
use upleft_render::theme::style_sheet::StyleSheet;

use crate::panels::appkit_support::{Presentation, RectExt, cg, without_actions};
use crate::panels::panel_chrome::{presentation_transform, set_number_values, set_transform_values};

/// `ToolbarChromePolicy`: one policy for toolbar motion and emphasis.
pub struct ToolbarChromePolicy;

/// `ToolbarChromePolicy.InteractionState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionState {
    Idle,
    Hover,
    Pressed,
}

/// `ToolbarChromePolicy.ScrubState`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrubState {
    pub indicator_center_x: CGFloat,
    pub segment: isize,
}

impl ToolbarChromePolicy {
    pub const HOVER_DURATION: f64 = motion::HOVER;
    pub const PRESS_IN_DURATION: f64 = motion::PRESS_IN;
    pub const PRESS_OUT_DURATION: f64 = motion::PRESS_OUT;
    pub const SELECTION_DURATION: f64 = motion::SELECTION;
    pub const EMPHASIS_DURATION: f64 = motion::EMPHASIS;
    pub const PRESSED_SCALE: CGFloat = 0.985;
    /// The task ring's press dips further than a plate button's.
    pub const RING_PRESSED_SCALE: CGFloat = 0.86;

    pub fn timing_function() -> Retained<CAMediaTimingFunction> {
        motion::timing(Curve::Snap)
    }

    pub fn feedback_opacity(state: InteractionState, increase_contrast: bool) -> f32 {
        match (state, increase_contrast) {
            (InteractionState::Idle, _) => 0.0,
            (InteractionState::Hover, false) => 0.075,
            (InteractionState::Hover, true) => 0.11,
            (InteractionState::Pressed, false) => 0.14,
            (InteractionState::Pressed, true) => 0.19,
        }
    }

    pub fn indicator_opacity(is_window_active: bool, increase_contrast: bool) -> f32 {
        match (is_window_active, increase_contrast) {
            (true, false) => 0.82,
            (true, true) => 1.0,
            (false, false) => 0.38,
            (false, true) => 0.56,
        }
    }

    /// `scrubState(position:leftCenterX:rightCenterX:)`.
    pub fn scrub_state_for_position(position: CGFloat, left_center_x: CGFloat, right_center_x: CGFloat) -> ScrubState {
        let travelled = swift_min(swift_max(position, 0.0), 1.0);
        Self::scrub_state(left_center_x + (right_center_x - left_center_x) * travelled, left_center_x, right_center_x)
    }

    /// `scrubState(pointerX:leftCenterX:rightCenterX:)`.
    pub fn scrub_state(pointer_x: CGFloat, left_center_x: CGFloat, right_center_x: CGFloat) -> ScrubState {
        let lower_bound = swift_min(left_center_x, right_center_x);
        let upper_bound = swift_max(left_center_x, right_center_x);
        let center_x = swift_min(swift_max(pointer_x, lower_bound), upper_bound);
        ScrubState {
            indicator_center_x: center_x,
            segment: if center_x < ((lower_bound + upper_bound) / 2.0) { 0 } else { 1 },
        }
    }
}

fn swift_min(x: CGFloat, y: CGFloat) -> CGFloat {
    upleft_render::swift_compat::smin(x, y)
}

fn swift_max(x: CGFloat, y: CGFloat) -> CGFloat {
    upleft_render::swift_compat::smax(x, y)
}

// MARK: - ToolbarInteractiveButton

pub struct ToolbarInteractiveButtonIvars {
    style_sheet: RefCell<Rc<StyleSheet>>,
    feedback_inset_x: Cell<CGFloat>,
    feedback_inset_y: Cell<CGFloat>,
    feedback_corner_radius: Cell<CGFloat>,
    feedback_layer: Retained<CALayer>,
    is_pointer_inside: Cell<bool>,
    is_pressed_for_feedback: Cell<bool>,
    accessibility_observer: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
}

impl Drop for ToolbarInteractiveButtonIvars {
    fn drop(&mut self) {
        if let Some(observer) = self.accessibility_observer.get_mut().take() {
            let center = NSWorkspace::sharedWorkspace().notificationCenter();
            // SAFETY: the observer token came from this centre.
            unsafe { center.removeObserver(&*(Retained::as_ptr(&observer) as *const AnyObject)) };
        }
    }
}

define_class!(
    /// Shares the toolbar's hover plate and press feedback.  Not final in
    /// Swift: subclasses override `styleSheetDidChange` and
    /// `permitsHoverFeedback`, which are Objective-C methods here so the
    /// base dispatches to an override.
    // SAFETY: `initWithFrame:` sets the ivars, so every initialiser path
    // (including AppKit's class factories) creates a valid instance.
    #[unsafe(super(NSButton, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "ToolbarInteractiveButton"]
    #[ivars = ToolbarInteractiveButtonIvars]
    pub struct ToolbarInteractiveButton;

    unsafe impl NSObjectProtocol for ToolbarInteractiveButton {}

    impl ToolbarInteractiveButton {
        #[unsafe(method_id(initWithFrame:))]
        fn __init_with_frame(this: Allocated<Self>, frame: NSRect) -> Retained<Self> {
            let mtm = MainThreadMarker::new().expect("ToolbarInteractiveButton is created on the main thread");
            let this = this.set_ivars(ToolbarInteractiveButtonIvars {
                style_sheet: RefCell::new(Rc::new(StyleSheet::current(mtm))),
                feedback_inset_x: Cell::new(5.0),
                feedback_inset_y: Cell::new(3.0),
                feedback_corner_radius: Cell::new(5.0),
                feedback_layer: CALayer::new(),
                is_pointer_inside: Cell::new(false),
                is_pressed_for_feedback: Cell::new(false),
                accessibility_observer: RefCell::new(None),
            });
            let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
            this.setWantsLayer(true);
            this.ivars().feedback_layer.setOpacity(0.0);
            if let Some(layer) = this.layer() {
                layer.insertSublayer_atIndex(&this.ivars().feedback_layer, 0);
            }
            this.refresh_feedback_color();
            let weak: ObjcWeak<ToolbarInteractiveButton> = ObjcWeak::from(&*this);
            let block = block2::RcBlock::new(move |_notification: std::ptr::NonNull<NSNotification>| {
                if let Some(this) = weak.load() {
                    this.refresh_interaction_feedback(false);
                }
            });
            let observer = unsafe {
                NSWorkspace::sharedWorkspace().notificationCenter().addObserverForName_object_queue_usingBlock(
                    Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
                    None,
                    Some(&NSOperationQueue::mainQueue()),
                    &block,
                )
            };
            *this.ivars().accessibility_observer.borrow_mut() = Some(observer);
            this
        }

        /// Overridable hook, called after `styleSheet` changes.
        #[unsafe(method(styleSheetDidChange))]
        fn __style_sheet_did_change(&self) {}

        /// Overridable: whether hover shows the feedback plate.
        #[unsafe(method(permitsHoverFeedback))]
        fn __permits_hover_feedback(&self) -> bool {
            true
        }

        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            let ivars = self.ivars();
            without_actions(|| {
                ivars
                    .feedback_layer
                    .setFrame(self.bounds().inset_by(ivars.feedback_inset_x.get(), ivars.feedback_inset_y.get()));
                ivars.feedback_layer.setCornerRadius(ivars.feedback_corner_radius.get());
            });
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.refresh_feedback_color();
        }

        #[unsafe(method(updateTrackingAreas))]
        fn __update_tracking_areas(&self) {
            for area in self.trackingAreas().iter() {
                self.removeTrackingArea(&area);
            }
            // SAFETY: the owner is the view itself.
            let area = unsafe {
                NSTrackingArea::initWithRect_options_owner_userInfo(
                    NSTrackingArea::alloc(),
                    self.bounds(),
                    NSTrackingAreaOptions::ActiveInKeyWindow
                        | NSTrackingAreaOptions::InVisibleRect
                        | NSTrackingAreaOptions::MouseEnteredAndExited,
                    Some(self),
                    None,
                )
            };
            self.addTrackingArea(&area);
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
        }

        #[unsafe(method(mouseEntered:))]
        fn __mouse_entered(&self, _event: &NSEvent) {
            self.ivars().is_pointer_inside.set(true);
            self.refresh_interaction_feedback(true);
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, _event: &NSEvent) {
            self.ivars().is_pointer_inside.set(false);
            self.refresh_interaction_feedback(true);
        }

        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, event: &NSEvent) {
            self.set_pressed_feedback(true);
            let _: () = unsafe { msg_send![super(self), mouseDown: event] };
            self.set_pressed_feedback(false);
        }
    }
);

impl ToolbarInteractiveButton {
    /// `ToolbarInteractiveButton(frame:)`.
    pub fn new(frame: NSRect, mtm: MainThreadMarker) -> Retained<ToolbarInteractiveButton> {
        unsafe { msg_send![ToolbarInteractiveButton::alloc(mtm), initWithFrame: frame] }
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet;
        let _: () = unsafe { msg_send![self, styleSheetDidChange] };
    }

    pub fn feedback_inset_x(&self) -> CGFloat {
        self.ivars().feedback_inset_x.get()
    }

    pub fn set_feedback_inset_x(&self, value: CGFloat) {
        self.ivars().feedback_inset_x.set(value);
    }

    pub fn feedback_inset_y(&self) -> CGFloat {
        self.ivars().feedback_inset_y.get()
    }

    pub fn set_feedback_inset_y(&self, value: CGFloat) {
        self.ivars().feedback_inset_y.set(value);
    }

    pub fn feedback_corner_radius(&self) -> CGFloat {
        self.ivars().feedback_corner_radius.get()
    }

    pub fn set_feedback_corner_radius(&self, value: CGFloat) {
        self.ivars().feedback_corner_radius.set(value);
    }

    fn permits_hover_feedback(&self) -> bool {
        unsafe { msg_send![self, permitsHoverFeedback] }
    }

    pub fn set_pressed_feedback(&self, pressed: bool) {
        if pressed == self.ivars().is_pressed_for_feedback.get() {
            return;
        }
        self.ivars().is_pressed_for_feedback.set(pressed);
        self.refresh_interaction_feedback(true);
        self.update_press_transform(true);
    }

    pub fn refresh_interaction_feedback(&self, animated: bool) {
        let ivars = self.ivars();
        let state = if ivars.is_pressed_for_feedback.get() {
            InteractionState::Pressed
        } else if ivars.is_pointer_inside.get() && self.permits_hover_feedback() {
            InteractionState::Hover
        } else {
            InteractionState::Idle
        };
        let target_opacity = ToolbarChromePolicy::feedback_opacity(
            state,
            NSWorkspace::sharedWorkspace().accessibilityDisplayShouldIncreaseContrast(),
        );
        self.animate_feedback_opacity(target_opacity, animated);
    }

    fn refresh_feedback_color(&self) {
        self.ivars().feedback_layer.setBackgroundColor(Some(&cg(&NSColor::labelColor())));
    }

    fn animate_feedback_opacity(&self, opacity: f32, animated: bool) {
        let layer = &self.ivars().feedback_layer;
        let reduce_motion = self.ivars().style_sheet.borrow().reduce_motion;
        if !animated || reduce_motion {
            layer.removeAnimationForKey(&NSString::from_str("feedback-opacity"));
            layer.setOpacity(opacity);
            return;
        }
        let animation = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("opacity")));
        let from = layer.__presentation().map(|presentation| presentation.opacity()).unwrap_or_else(|| layer.opacity());
        set_number_values(&animation, Some(from as f64), opacity as f64);
        animation.setDuration(ToolbarChromePolicy::HOVER_DURATION);
        animation.setTimingFunction(Some(&ToolbarChromePolicy::timing_function()));
        layer.addAnimation_forKey(&animation, Some(&NSString::from_str("feedback-opacity")));
        layer.setOpacity(opacity);
    }

    fn update_press_transform(&self, animated: bool) {
        let pressed = self.ivars().is_pressed_for_feedback.get();
        let scale = if pressed { ToolbarChromePolicy::PRESSED_SCALE } else { 1.0 };
        let transform = CATransform3D::new_scale(scale, scale, 1.0);
        let reduce_motion = self.ivars().style_sheet.borrow().reduce_motion;
        let Some(layer) = self.layer() else { return };
        if !animated || reduce_motion {
            layer.setTransform(transform);
            return;
        }
        let animation = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("transform")));
        set_transform_values(&animation, presentation_transform(&layer), transform);
        animation.setDuration(if pressed {
            ToolbarChromePolicy::PRESS_IN_DURATION
        } else {
            ToolbarChromePolicy::PRESS_OUT_DURATION
        });
        animation.setTimingFunction(Some(&ToolbarChromePolicy::timing_function()));
        layer.addAnimation_forKey(&animation, Some(&NSString::from_str("press-transform")));
        layer.setTransform(transform);
    }

    pub fn feedback_layer_for_testing(&self) -> Retained<CALayer> {
        self.ivars().feedback_layer.clone()
    }
}

#[allow(unused)]
fn _unused(_: &AnyObject, _: &NSView) {}
