//! Port of `Panels/TaskProgressRing.swift`: the toolbar progress ring (§8.5).
//!
//! It is the Tasks button, and it is the only permanent-looking element the
//! task layer adds to the toolbar. It earns that by answering two questions
//! without a panel being open — *is there a plan* and *how much of it is
//! left* — and by staying a button in every state, including the state where
//! the document has no tasks at all.
//!
//! The glyph is a ring, a remaining count, and nothing else:
//!
//! * no tasks — a quiet empty track. Nothing to say, still clickable.
//! * some done — an accent arc from twelve o'clock plus the number of tasks
//!   still open, which is the figure a reader acts on.
//! * all done — the ring closes and a check draws in its centre.
//!
//! The arc travels rather than jumps, the count crossfades, and the
//! panel-open state lights the disc, so the icon never silently stops
//! meaning "Tasks".

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSApplication, NSBezierPath, NSColor, NSCursor, NSEvent, NSFont, NSResponder, NSScreen,
    NSTrackingArea, NSTrackingAreaOptions, NSView,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGVector};
use objc2_core_graphics::CGPath;
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::{
    CABasicAnimation, CACurrentMediaTime, CALayer, CAMediaTiming, CAShapeLayer, CATextLayer, CATransaction,
    CATransform3D, CATransform3DIdentity, CATransition, kCAAlignmentCenter, kCAFillModeBackwards, kCALineCapRound, kCALineJoinRound,
    kCATransitionFade,
};
use upleft_render::motion::{self, Curve};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::view::style_sheet_defaults::PanelAlpha;

use super::appkit_support::{
    Presentation, RectExt, cg, null_actions, ns_string, rect, role, set_label, set_role, smax, smin, weight_semibold,
};
use super::floating_panel_surface::FloatingPanelSurface;
use super::panel_chrome::{
    CheckboxGeometry, PanelMetrics, presentation_transform, refresh_tracking_area, set_number_values,
    set_transform_values,
};
use crate::app::toolbar_controls::{InteractionState, ToolbarChromePolicy};
use crate::support::commands::KeyBinding;

/// `TaskProgressRing.Metrics`.
struct Metrics;

impl Metrics {
    /// The glyph. Two points larger than it was, because the count has to
    /// sit inside it and 20pt could not hold two digits.
    const RING: CGFloat = 22.0;
    /// The button: `ToolbarMenuButton`'s geometry to the point.
    const CONTROL: CGFloat = PanelMetrics::TOOLBAR_CONTROL_SIDE;
    const LINE_WIDTH: CGFloat = 2.5;
    /// The trigger carries the same surface curvature as the body. AppKit
    /// clamps it to the 30pt control's capsule bounds, so the ring stays a
    /// capsule while sharing the panel's radius family.
    const PLATE_RADIUS: CGFloat = PanelMetrics::SURFACE_RADIUS;
    const COUNT_SIZE: CGFloat = 9.5;
}

thread_local! {
    /// `TaskProgressRing.countFont`.
    static COUNT_FONT: Retained<NSFont> =
        NSFont::monospacedDigitSystemFontOfSize_weight(Metrics::COUNT_SIZE, weight_semibold());
}

fn count_font() -> Retained<NSFont> {
    COUNT_FONT.with(Clone::clone)
}

pub struct TaskProgressRingIvars {
    progress: Cell<(isize, isize)>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    is_active: Cell<bool>,
    on_activate: RefCell<Option<Rc<dyn Fn()>>>,
    on_visibility_change: RefCell<Option<Rc<dyn Fn(bool)>>>,
    feedback_layer: Retained<CALayer>,
    /// Everything the ring *is*, in one container: the press compresses the
    /// glyph, not the plate.
    glyph_layer: Retained<CALayer>,
    disc_layer: Retained<CAShapeLayer>,
    track_layer: Retained<CAShapeLayer>,
    arc_layer: Retained<CAShapeLayer>,
    check_layer: Retained<CAShapeLayer>,
    count_layer: Retained<CATextLayer>,
    /// The sonar ping a successful press emits.
    ping_layer: Retained<CAShapeLayer>,
    animated_fraction: Cell<CGFloat>,
    did_celebrate_completion: Cell<bool>,
    tracking_area: RefCell<Option<Retained<NSTrackingArea>>>,
    is_pointer_inside: Cell<bool>,
    is_pressed_for_feedback: Cell<bool>,
}

define_class!(
    /// `TaskProgressRing`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "TaskProgressRing"]
    #[ivars = TaskProgressRingIvars]
    pub struct TaskProgressRing;

    unsafe impl NSObjectProtocol for TaskProgressRing {}

    impl TaskProgressRing {
        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            NSSize::new(Metrics::CONTROL, Metrics::CONTROL)
        }

        /// A toolbar lives inside the titlebar's draggable region. Explicitly
        /// claiming the pointer prevents a double-click on this custom NSView
        /// from leaking through to the titlebar and miniaturizing the window.
        #[unsafe(method(mouseDownCanMoveWindow))]
        fn __mouse_down_can_move_window(&self) -> bool {
            false
        }

        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            let ivars = self.ivars();
            ivars.feedback_layer.setFrame(self.bounds().inset_by(1.0, 1.0));
            ivars.feedback_layer.setCornerRadius(Metrics::PLATE_RADIUS);
            ivars.glyph_layer.setFrame(self.bounds());
            self.place_layers();
            CATransaction::commit();
        }

        #[unsafe(method(updateTrackingAreas))]
        fn __update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            refresh_tracking_area(
                self,
                &self.ivars().tracking_area,
                NSTrackingAreaOptions::ActiveInKeyWindow
                    | NSTrackingAreaOptions::InVisibleRect
                    | NSTrackingAreaOptions::MouseEnteredAndExited,
            );
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
        fn __mouse_down(&self, _event: &NSEvent) {
            self.set_pressed_feedback(true);
        }

        #[unsafe(method(mouseDragged:))]
        fn __mouse_dragged(&self, event: &NSEvent) {
            let inside = self.bounds().contains_point(self.convertPoint_fromView(event.locationInWindow(), None));
            self.set_pressed_feedback(inside);
        }

        #[unsafe(method(mouseUp:))]
        fn __mouse_up(&self, event: &NSEvent) {
            let inside = self.bounds().contains_point(self.convertPoint_fromView(event.locationInWindow(), None));
            if inside {
                self.release_pressed_feedback_for_activation();
                self.play_release_moment();
                self.fire_on_activate();
            } else {
                self.set_pressed_feedback(false);
            }
        }

        /// Tab reaches the ring and Space or Return opens the panel, gated on
        /// Full Keyboard Access like every AppKit control that is not a text
        /// field (§11.4).
        #[unsafe(method(acceptsFirstResponder))]
        fn __accepts_first_responder(&self) -> bool {
            NSApplication::sharedApplication(self.mtm()).isFullKeyboardAccessEnabled()
        }

        #[unsafe(method(canBecomeKeyView))]
        fn __can_become_key_view(&self) -> bool {
            NSApplication::sharedApplication(self.mtm()).isFullKeyboardAccessEnabled()
        }

        #[unsafe(method(focusRingMaskBounds))]
        fn __focus_ring_mask_bounds(&self) -> NSRect {
            self.bounds().inset_by(1.0, 1.0)
        }

        #[unsafe(method(drawFocusRingMask))]
        fn __draw_focus_ring_mask(&self) {
            NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                self.bounds().inset_by(1.0, 1.0),
                Metrics::PLATE_RADIUS,
                Metrics::PLATE_RADIUS,
            )
            .fill();
        }

        #[unsafe(method(keyDown:))]
        fn __key_down(&self, event: &NSEvent) {
            match KeyBinding::key_for_event(event).as_deref() {
                Some("space") | Some("return") => {
                    self.play_release_moment();
                    self.fire_on_activate();
                }
                _ => {
                    let _: () = unsafe { msg_send![super(self), keyDown: event] };
                }
            }
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn __accessibility_perform_press(&self) -> bool {
            self.accessibility_perform_press()
        }

        #[unsafe(method(resetCursorRects))]
        fn __reset_cursor_rects(&self) {
            self.addCursorRect_cursor(self.bounds(), &NSCursor::pointingHandCursor());
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.apply_style(false);
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn __view_did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            let scale = match self.window() {
                Some(window) => window.backingScaleFactor(),
                None => NSScreen::mainScreen(self.mtm()).map_or(2.0, |screen| screen.backingScaleFactor()),
            };
            self.ivars().count_layer.setContentsScale(scale);
        }
    }
);

impl TaskProgressRing {
    pub const MORPH_SIDE: CGFloat = 22.0;

    /// `TaskProgressRing()`: hosts build panels before they have a theme in
    /// hand and assign `styleSheet` immediately afterwards.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<TaskProgressRing> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<TaskProgressRing> {
        // Stored-property initial values, in declaration order.
        let feedback_layer = CALayer::new();
        let glyph_layer = CALayer::new();
        let disc_layer = CAShapeLayer::new();
        let track_layer = CAShapeLayer::new();
        let arc_layer = CAShapeLayer::new();
        let check_layer = CAShapeLayer::new();
        let count_layer = CATextLayer::new();
        let ping_layer = CAShapeLayer::new();
        let this = Self::alloc(mtm).set_ivars(TaskProgressRingIvars {
            progress: Cell::new((0, 0)),
            style_sheet: RefCell::new(style_sheet),
            is_active: Cell::new(false),
            on_activate: RefCell::new(None),
            on_visibility_change: RefCell::new(None),
            feedback_layer: feedback_layer.clone(),
            glyph_layer: glyph_layer.clone(),
            disc_layer: disc_layer.clone(),
            track_layer: track_layer.clone(),
            arc_layer: arc_layer.clone(),
            check_layer: check_layer.clone(),
            count_layer: count_layer.clone(),
            ping_layer: ping_layer.clone(),
            animated_fraction: Cell::new(0.0),
            did_celebrate_completion: Cell::new(false),
            tracking_area: RefCell::new(None),
            is_pointer_inside: Cell::new(false),
            is_pressed_for_feedback: Cell::new(false),
        });
        let this: Retained<TaskProgressRing> =
            unsafe { msg_send![super(this), initWithFrame: rect(0.0, 0.0, Metrics::CONTROL, Metrics::CONTROL)] };
        this.setWantsLayer(true);

        feedback_layer.setOpacity(0.0);
        if let Some(layer) = this.layer() {
            layer.insertSublayer_atIndex(&feedback_layer, 0);
        }
        null_actions(&glyph_layer, &["position", "bounds"]);
        if let Some(layer) = this.layer() {
            layer.addSublayer(&glyph_layer);
        }
        for sub in [&disc_layer, &track_layer, &arc_layer, &check_layer] {
            sub.setFillColor(Some(&cg(&NSColor::clearColor())));
            glyph_layer.addSublayer(sub);
        }
        ping_layer.setFillColor(Some(&cg(&NSColor::clearColor())));
        ping_layer.setOpacity(0.0);
        null_actions(&ping_layer, &["position", "bounds", "path"]);
        // Outside the glyph container: the wave has to keep travelling while
        // the ring springs back, not inherit the spring.
        if let Some(layer) = this.layer() {
            layer.addSublayer(&ping_layer);
        }
        count_layer.setAlignmentMode(unsafe { kCAAlignmentCenter });
        count_layer.setContentsScale(NSScreen::mainScreen(mtm).map_or(2.0, |screen| screen.backingScaleFactor()));
        glyph_layer.addSublayer(&count_layer);

        this.setAccessibilityElement(true);
        set_role(&*this, role::button());
        this.update_accessibility();
        this.update_count(false);
        this.apply_style(false);
        this
    }

    // MARK: - Properties

    pub fn progress(&self) -> (isize, isize) {
        self.ivars().progress.get()
    }

    /// `progress = (done: …, total: …)`.
    pub fn set_progress(&self, done: isize, total: isize) {
        let old_value = self.ivars().progress.replace((done, total));
        if !(done != old_value.0 || total != old_value.1) {
            return;
        }
        let had_tasks = old_value.1 > 0;
        self.animate_arc(self.fraction());
        self.update_count(self.window().is_some());
        let complete = self.is_complete();
        if complete && !self.ivars().did_celebrate_completion.get() {
            self.ivars().did_celebrate_completion.set(true);
            self.celebrate_completion();
        } else if !complete {
            self.ivars().did_celebrate_completion.set(false);
        }
        self.update_accessibility();
        self.apply_style(false);
        // The item's label and tooltip change when a document gains or loses
        // its plan; the toolbar caches both until it revalidates.
        if had_tasks != (total > 0) {
            let handler = self.ivars().on_visibility_change.borrow().clone();
            if let Some(handler) = handler {
                handler(false);
            }
        }
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet;
        self.apply_style(false);
        self.setNeedsLayout(true);
    }

    /// Mirrors whether the task panel is open so the toolbar icon shows its
    /// on-state — the ring is the button that owns that panel (§8.5).
    pub fn is_active(&self) -> bool {
        self.ivars().is_active.get()
    }

    pub fn set_is_active(&self, is_active: bool) {
        let old_value = self.ivars().is_active.replace(is_active);
        if is_active == old_value {
            return;
        }
        // Opening the panel hands it the numbers, so the ring stops repeating
        // them — see `count_text`.
        self.update_count(self.window().is_some());
        self.apply_style(true);
    }

    /// Fired when the reader clicks the ring — typically opens the task
    /// panel.
    pub fn on_activate(&self) -> Option<Rc<dyn Fn()>> {
        self.ivars().on_activate.borrow().clone()
    }

    pub fn set_on_activate(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_activate.borrow_mut() = handler;
    }

    /// Kept for hosts that ask the toolbar to revalidate when the ring's
    /// meaning changes. The ring itself is never hidden.
    pub fn set_on_visibility_change(&self, handler: Option<Rc<dyn Fn(bool)>>) {
        *self.ivars().on_visibility_change.borrow_mut() = handler;
    }

    fn fire_on_activate(&self) {
        let handler = self.ivars().on_activate.borrow().clone();
        if let Some(handler) = handler {
            handler();
        }
    }

    // MARK: - Geometry

    fn place_layers(&self) {
        let ivars = self.ivars();
        let bounds = self.bounds();
        let centre = CGPoint::new(bounds.mid_x(), bounds.mid_y());
        let radius = (smin(Metrics::RING, smin(bounds.width(), bounds.height())) - Metrics::LINE_WIDTH) / 2.0;
        if !(radius > 0.0) {
            return;
        }
        let square = rect(centre.x - radius, centre.y - radius, radius * 2.0, radius * 2.0);

        let glyph_bounds = ivars.glyph_layer.bounds();
        for sub in [&ivars.disc_layer, &ivars.track_layer, &ivars.arc_layer, &ivars.check_layer] {
            sub.setFrame(glyph_bounds);
        }

        // The ping rides the track's circle exactly.
        ivars.ping_layer.setFrame(bounds);
        ivars.ping_layer.setLineWidth(Metrics::LINE_WIDTH);

        // The lit disc stops at the inside of the track.
        let disc = unsafe {
            CGPath::with_ellipse_in_rect(
                square.inset_by(Metrics::LINE_WIDTH / 2.0, Metrics::LINE_WIDTH / 2.0),
                std::ptr::null(),
            )
        };
        ivars.disc_layer.setPath(Some(&disc));

        let track = NSBezierPath::bezierPath();
        track.appendBezierPathWithArcWithCenter_radius_startAngle_endAngle(centre, radius, 0.0, 360.0);
        ivars.track_layer.setPath(Some(&track.CGPath()));
        ivars.track_layer.setLineWidth(Metrics::LINE_WIDTH);
        ivars.ping_layer.setPath(Some(&track.CGPath()));

        // Twelve o'clock, clockwise — the direction a reader reads a dial.
        let arc = NSBezierPath::bezierPath();
        arc.appendBezierPathWithArcWithCenter_radius_startAngle_endAngle_clockwise(
            centre,
            radius,
            90.0,
            90.0 - 360.0,
            true,
        );
        ivars.arc_layer.setPath(Some(&arc.CGPath()));
        ivars.arc_layer.setLineWidth(Metrics::LINE_WIDTH);
        ivars.arc_layer.setLineCap(unsafe { kCALineCapRound });
        ivars.arc_layer.setStrokeStart(0.0);
        // Layout must not undo state: re-placing the paths used to reset the
        // arc and wipe the completion check.
        ivars.arc_layer.setStrokeEnd(ivars.animated_fraction.get());

        ivars.check_layer.setPath(Some(&Self::check_path(square.inset_by(radius * 0.34, radius * 0.34))));
        ivars.check_layer.setLineWidth(1.8);
        ivars.check_layer.setLineCap(unsafe { kCALineCapRound });
        ivars.check_layer.setLineJoin(unsafe { kCALineJoinRound });
        ivars.check_layer.setStrokeStart(0.0);
        ivars.check_layer.setStrokeEnd(if self.is_complete() { 1.0 } else { 0.0 });

        // A `CATextLayer` draws from the top of its bounds, so the count is
        // centred by giving the layer exactly one line and centring that.
        let font = count_font();
        let line_height = (font.ascender() - font.descender()).ceil();
        ivars.count_layer.setFrame(rect(
            0.0,
            (centre.y - line_height / 2.0).round(),
            ivars.glyph_layer.bounds().width(),
            line_height,
        ));
    }

    /// The check is drawn in the view's (unflipped) coordinates from the
    /// same unit tick the checkbox and the document renderer use (§8.5).
    fn check_path(rect: NSRect) -> Retained<CGPath> {
        let path = NSBezierPath::bezierPath();
        for (index, unit) in CheckboxGeometry::TICK.iter().enumerate() {
            let point = NSPoint::new(rect.min_x() + unit.x * rect.width(), rect.min_y() + unit.y * rect.height());
            if index == 0 {
                path.moveToPoint(point);
            } else {
                path.lineToPoint(point);
            }
        }
        path.CGPath()
    }

    fn fraction(&self) -> CGFloat {
        let (done, total) = self.ivars().progress.get();
        if !(total > 0) {
            return 0.0;
        }
        smin(1.0, smax(0.0, done as CGFloat / total as CGFloat))
    }

    fn is_complete(&self) -> bool {
        let (done, total) = self.ivars().progress.get();
        total > 0 && done >= total
    }

    fn remaining(&self) -> isize {
        let (done, total) = self.ivars().progress.get();
        std::cmp::max(0, total - done)
    }

    /// The numeral inside the ring — and the two states that deliberately
    /// have none. Three digits will not fit a 22pt ring, so a very long plan
    /// says "99+" and the tooltip carries the exact figure. The open-panel
    /// and all-done states drop it: the panel owns the tally while open, and
    /// the checkmark owns completion.
    fn count_text(&self) -> String {
        let (_, total) = self.ivars().progress.get();
        if !(total > 0 && !self.is_complete() && !self.ivars().is_active.get()) {
            return String::new();
        }
        let remaining = self.remaining();
        if remaining > 99 { "99+".to_owned() } else { format!("{remaining}") }
    }

    /// `countTextForTesting`.
    pub fn count_text_for_testing(&self) -> String {
        self.count_text()
    }

    // MARK: - Styling

    /// Every colour the ring wears, set in one place. Styling is
    /// instantaneous by default, and the one styling change a reader
    /// watches (the panel opening) asks for `quick`.
    fn apply_style(&self, animated: bool) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        CATransaction::begin();
        if animated && !style_sheet.reduce_motion && self.window().is_some() {
            CATransaction::setAnimationDuration(motion::QUICK);
            CATransaction::setAnimationTimingFunction(Some(&motion::timing(Curve::EaseOut)));
        } else {
            CATransaction::setDisableActions(true);
        }

        let contrast = style_sheet.increase_contrast;
        let complete = self.is_complete();
        let has_tasks = ivars.progress.get().1 > 0;
        let is_active = ivars.is_active.get();

        ivars.feedback_layer.setBackgroundColor(Some(&cg(&FloatingPanelSurface::glass_tint(&style_sheet))));
        ivars.feedback_layer.setBorderWidth(if contrast { 1.0 } else { 0.5 });
        ivars
            .feedback_layer
            .setBorderColor(Some(&cg(&style_sheet.text.panel_alpha(if contrast { 0.55 } else { 0.18 }, false))));

        // Open panel: the whole glyph tints.
        ivars
            .disc_layer
            .setFillColor(Some(&cg(&style_sheet.accent.panel_alpha(if is_active { 0.13 } else { 0.0 }, contrast))));
        ivars.disc_layer.setOpacity(if is_active { 1.0 } else { 0.0 });

        // An empty plan keeps its track, one step quieter.
        let track_color = if is_active {
            cg(&style_sheet.accent.panel_alpha(0.34, contrast))
        } else {
            cg(&style_sheet.text.panel_alpha(if has_tasks { 0.20 } else { 0.12 }, contrast))
        };
        ivars.track_layer.setStrokeColor(Some(&track_color));
        ivars.track_layer.setOpacity(1.0);

        ivars.arc_layer.setStrokeColor(Some(&cg(&style_sheet.accent)));
        ivars.arc_layer.setOpacity(if ivars.animated_fraction.get() > 0.0 { 1.0 } else { 0.0 });

        ivars.check_layer.setStrokeColor(Some(&cg(&style_sheet.accent)));
        ivars.check_layer.setOpacity(if complete { 1.0 } else { 0.0 });

        ivars
            .count_layer
            .setForegroundColor(Some(&cg(if contrast { &style_sheet.text } else { &style_sheet.text_secondary })));
        let font = count_font();
        unsafe { ivars.count_layer.setFont(Some(&*(Retained::as_ptr(&font) as *const objc2_core_foundation::CFType))) };
        ivars.count_layer.setFontSize(Metrics::COUNT_SIZE);

        CATransaction::commit();
    }

    // MARK: - Animation

    fn animate_arc(&self, target: CGFloat) {
        let ivars = self.ivars();
        let arc_layer = &ivars.arc_layer;
        let previous = ivars.animated_fraction.replace(target);
        let key = NSString::from_str("arc");
        if !(!self.style_sheet().reduce_motion && self.window().is_some()) {
            arc_layer.removeAnimationForKey(&key);
            arc_layer.setStrokeEnd(target);
            arc_layer.setOpacity(if target > 0.0 { 1.0 } else { 0.0 });
            return;
        }
        // The arc travels. Ticking one task of eleven moves it nine degrees,
        // and a jump that small is invisible — the travel is what the eye
        // catches.
        let animation = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("strokeEnd")));
        let from = arc_layer.__presentation().map_or(previous, |presentation| presentation.strokeEnd());
        set_number_values(&animation, Some(from), target);
        animation.setDuration(motion::STANDARD);
        animation.setTimingFunction(Some(&motion::timing(Curve::Decelerate)));
        arc_layer.addAnimation_forKey(&animation, Some(&key));
        arc_layer.setStrokeEnd(target);
        arc_layer.setOpacity(if target > 0.0 { 1.0 } else { 0.0 });
    }

    fn update_count(&self, animated: bool) {
        let count_layer = &self.ivars().count_layer;
        let text = self.count_text();
        let current: Option<String> = count_layer
            .string()
            .and_then(|value| value.downcast::<NSString>().ok())
            .map(|value| value.to_string());
        if current.as_deref() == Some(text.as_str()) {
            return;
        }
        let key = NSString::from_str("count");
        if !(animated && !self.style_sheet().reduce_motion) {
            count_layer.removeAnimationForKey(&key);
            unsafe { count_layer.setString(Some(&ns_string(&text))) };
            return;
        }
        let fade = CATransition::new();
        fade.setType(unsafe { kCATransitionFade });
        fade.setDuration(motion::QUICK);
        count_layer.addAnimation_forKey(&fade, Some(&key));
        unsafe { count_layer.setString(Some(&ns_string(&text))) };
    }

    fn celebrate_completion(&self) {
        let ivars = self.ivars();
        let check_layer = &ivars.check_layer;
        if !(!self.style_sheet().reduce_motion && self.window().is_some()) {
            check_layer.setStrokeEnd(1.0);
            check_layer.setOpacity(1.0);
            return;
        }
        // The last task of a plan gets a moment, not a mutation: the ring
        // closes, then the check draws itself and the whole glyph lands with
        // one short pop.
        let draw = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("strokeEnd")));
        set_number_values(&draw, Some(0.0), 1.0);
        draw.setBeginTime(CACurrentMediaTime() + motion::QUICK);
        draw.setDuration(motion::STANDARD);
        draw.setTimingFunction(Some(&motion::timing(Curve::EaseOut)));
        draw.setFillMode(unsafe { kCAFillModeBackwards });
        let key = NSString::from_str("completion-check");
        check_layer.removeAnimationForKey(&key);
        check_layer.addAnimation_forKey(&draw, Some(&key));
        check_layer.setStrokeEnd(1.0);
        check_layer.setOpacity(1.0);

        let pop = motion::pop(0.9, 1.08, motion::DELIBERATE, Some(CGVector::new(0.0, 1.0)), None);
        pop.setBeginTime(CACurrentMediaTime() + motion::QUICK);
        pop.setFillMode(unsafe { kCAFillModeBackwards });
        ivars.glyph_layer.addAnimation_forKey(&pop, Some(&NSString::from_str("completion-pop")));
    }

    // MARK: - Interaction

    /// The moment the press hands off to the panel: the circle springs back
    /// out of its compression and throws a wave outward. Reduce Motion keeps
    /// the state change and drops the moment entirely.
    fn play_release_moment(&self) {
        let ivars = self.ivars();
        let glyph_layer = &ivars.glyph_layer;
        if !(!self.style_sheet().reduce_motion && self.window().is_some()) {
            glyph_layer.setTransform(identity());
            return;
        }

        let pop = motion::pop(
            ToolbarChromePolicy::RING_PRESSED_SCALE,
            1.08,
            motion::STANDARD,
            Some(CGVector::new(0.0, 1.0)),
            Some(Metrics::PLATE_RADIUS),
        );
        let press_key = NSString::from_str("press-transform");
        glyph_layer.removeAnimationForKey(&press_key);
        glyph_layer.addAnimation_forKey(&pop, Some(&press_key));
        glyph_layer.setTransform(identity());

        let ping_layer = &ivars.ping_layer;
        ping_layer.removeAnimationForKey(&NSString::from_str("ping-grow"));
        ping_layer.removeAnimationForKey(&NSString::from_str("ping-fade"));
        ping_layer.setStrokeColor(Some(&cg(&self.style_sheet().accent)));
        CATransaction::begin();
        let weak_ping: ObjcWeak<CAShapeLayer> = ObjcWeak::from(&**ping_layer);
        let completion = block2::RcBlock::new(move || {
            if let Some(ping_layer) = weak_ping.load() {
                ping_layer.setOpacity(0.0);
                ping_layer.setTransform(identity());
            }
        });
        unsafe { CATransaction::setCompletionBlock(Some(&completion)) };
        CATransaction::setDisableActions(true);
        let grow = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("transform.scale")));
        set_number_values(&grow, Some(1.0), 2.1);
        grow.setDuration(motion::DELIBERATE);
        grow.setTimingFunction(Some(&motion::timing(Curve::Structural)));
        ping_layer.addAnimation_forKey(&grow, Some(&NSString::from_str("ping-grow")));
        ping_layer.setTransform(CATransform3D::new_scale(2.1, 2.1, 1.0));
        let fade = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("opacity")));
        set_number_values(&fade, Some(0.5), 0.0);
        fade.setDuration(motion::DELIBERATE);
        fade.setTimingFunction(Some(&motion::timing(Curve::EaseOut)));
        ping_layer.addAnimation_forKey(&fade, Some(&NSString::from_str("ping-fade")));
        ping_layer.setOpacity(0.0);
        CATransaction::commit();
    }

    fn accessibility_perform_press(&self) -> bool {
        self.play_release_moment();
        self.fire_on_activate();
        true
    }

    fn set_pressed_feedback(&self, pressed: bool) {
        let ivars = self.ivars();
        if pressed == ivars.is_pressed_for_feedback.get() {
            return;
        }
        ivars.is_pressed_for_feedback.set(pressed);
        self.refresh_interaction_feedback(true);
        self.update_press_transform(true);
    }

    /// Release inside skips the plain scale-back: the release moment's
    /// spring owns the transform from here.
    fn release_pressed_feedback_for_activation(&self) {
        let ivars = self.ivars();
        if !ivars.is_pressed_for_feedback.get() {
            return;
        }
        ivars.is_pressed_for_feedback.set(false);
        self.refresh_interaction_feedback(true);
    }

    fn refresh_interaction_feedback(&self, animated: bool) {
        // A press does not darken the plate: the plate holds its hover value
        // and the glyph carries the whole press.
        let ivars = self.ivars();
        let state = if ivars.is_pressed_for_feedback.get() || ivars.is_pointer_inside.get() {
            InteractionState::Hover
        } else {
            InteractionState::Idle
        };
        let target_opacity = ToolbarChromePolicy::feedback_opacity(state, self.style_sheet().increase_contrast);
        self.animate_feedback_opacity(target_opacity, animated);
    }

    fn animate_feedback_opacity(&self, opacity: f32, animated: bool) {
        let feedback_layer = &self.ivars().feedback_layer;
        let key = NSString::from_str("feedback-opacity");
        if !(animated && !self.style_sheet().reduce_motion) {
            feedback_layer.removeAnimationForKey(&key);
            feedback_layer.setOpacity(opacity);
            return;
        }
        let animation = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("opacity")));
        let from = feedback_layer.__presentation().map_or(feedback_layer.opacity(), |presentation| presentation.opacity());
        set_number_values(&animation, Some(from as f64), opacity as f64);
        animation.setDuration(ToolbarChromePolicy::HOVER_DURATION);
        animation.setTimingFunction(Some(&ToolbarChromePolicy::timing_function()));
        feedback_layer.addAnimation_forKey(&animation, Some(&key));
        feedback_layer.setOpacity(opacity);
    }

    fn update_press_transform(&self, animated: bool) {
        let ivars = self.ivars();
        let glyph_layer = &ivars.glyph_layer;
        let pressed = ivars.is_pressed_for_feedback.get();
        let scale = if pressed { ToolbarChromePolicy::RING_PRESSED_SCALE } else { 1.0 };
        let transform = CATransform3D::new_scale(scale, scale, 1.0);
        if !(animated && !self.style_sheet().reduce_motion) {
            glyph_layer.setTransform(transform);
            return;
        }
        let animation = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("transform")));
        set_transform_values(&animation, presentation_transform(glyph_layer), transform);
        animation.setDuration(if pressed {
            ToolbarChromePolicy::PRESS_IN_DURATION
        } else {
            ToolbarChromePolicy::PRESS_OUT_DURATION
        });
        animation.setTimingFunction(Some(&ToolbarChromePolicy::timing_function()));
        glyph_layer.addAnimation_forKey(&animation, Some(&NSString::from_str("press-transform")));
        glyph_layer.setTransform(transform);
    }

    /// The count in words, always.
    fn update_accessibility(&self) {
        let (done, total) = self.ivars().progress.get();
        if !(total > 0) {
            set_label(self, "No tasks");
            self.setAccessibilityValueDescription(Some(&NSString::from_str("No tasks")));
            self.setToolTip(Some(&ns_string("No tasks — Open Tasks")));
            return;
        }
        let progress_label = format!("{done} of {total} tasks complete");
        let remaining = self.remaining();
        let tail = if self.is_complete() {
            "all done".to_owned()
        } else if remaining == 1 {
            "1 left".to_owned()
        } else {
            format!("{remaining} left")
        };
        set_label(self, &progress_label);
        self.setAccessibilityValueDescription(Some(&ns_string(&progress_label)));
        self.setToolTip(Some(&ns_string(&format!("{progress_label}, {tail} — Open Tasks"))));
    }

    // MARK: - Test hooks

    /// The ring's own layers, for the conformance scene.
    pub fn glyph_layer_for_testing(&self) -> Retained<CALayer> {
        self.ivars().glyph_layer.clone()
    }
}

/// `CATransform3DIdentity`.
fn identity() -> CATransform3D {
    // SAFETY: Core Animation exports the identity as an immutable global.
    unsafe { CATransform3DIdentity }
}

#[allow(unused)]
fn _unused(_: &AnyObject, _: &NSView) {}
