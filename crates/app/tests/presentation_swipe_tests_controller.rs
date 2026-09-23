//! Port of the `Tests/DownrightAppTests/PresentationSwipeTests.swift` cases
//! that need the real `DocumentWindowController`: the swipe's rail callbacks
//! drive `controller.toolbarPresentationControl`, its presentation switch is
//! `+Actions`' `changePresentation(to:)`/`setPresentationSegment(_:)`, and the
//! split-view and resize cases need the controller's panes and
//! `windowDidResize(_:)`. `presentation_swipe_tests.rs` runs the rest over a
//! stand-in.
//!
//! The Swift suite is `@MainActor` and `.serialized`: this binary owns the
//! main thread (`harness = false`). Adapted, with the reason: Swift's
//! `sizedController` sets the window frame at (0, 0); here the same 900 × 700
//! frame sits at (-30000, -30000). The window is never ordered in in either.

mod controller_support;

use std::rc::Rc;

use controller_support::{Closing, new_controller};
use objc2::rc::Retained;
use objc2_app_kit::NSEvent;
use objc2_core_foundation::CGFloat;
use objc2_core_graphics::{CGEvent, CGEventField, CGMomentumScrollPhase, CGScrollEventUnit, CGScrollPhase};
use objc2_foundation::{NSNotification, NSPoint, NSRect, NSSize};
use upleft_render::appkit_compat::RectExt;
use upleft_render::render_contracts::SourceFocus;
use upleft_render::theme::style_sheet::StyleSheet;

// MARK: - Synthetic trackpad events

/// The suite's private `scroll(…)`: a continuous scroll event with real
/// phases, as `presentation_swipe_tests.rs` builds it.
#[derive(Clone, Copy)]
struct Scroll {
    delta_x: CGFloat,
    phase: Option<CGScrollPhase>,
    seconds: f64,
}

fn scroll() -> Scroll {
    Scroll { delta_x: 0.0, phase: Some(CGScrollPhase::Changed), seconds: 0.0 }
}

impl Scroll {
    fn delta_x(mut self, value: CGFloat) -> Self {
        self.delta_x = value;
        self
    }

    fn phase(mut self, value: Option<CGScrollPhase>) -> Self {
        self.phase = value;
        self
    }

    fn at(mut self, seconds: f64) -> Self {
        self.seconds = seconds;
        self
    }

    fn event(&self) -> Retained<NSEvent> {
        let created = CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Pixel, 2, 0, 0, 0).expect("CGEvent");
        let event = Some(&*created);
        CGEvent::set_integer_value_field(event, CGEventField::ScrollWheelEventIsContinuous, 1);
        CGEvent::set_double_value_field(event, CGEventField::ScrollWheelEventPointDeltaAxis1, 0.0);
        CGEvent::set_double_value_field(event, CGEventField::ScrollWheelEventPointDeltaAxis2, self.delta_x);
        if let Some(phase) = self.phase {
            CGEvent::set_integer_value_field(event, CGEventField::ScrollWheelEventScrollPhase, phase.0 as i64);
        }
        CGEvent::set_integer_value_field(
            event,
            CGEventField::ScrollWheelEventMomentumPhase,
            CGMomentumScrollPhase::None.0 as i64,
        );
        let seconds = if self.seconds >= 0.0 { self.seconds } else { 0.0 };
        CGEvent::set_timestamp(event, (seconds * 1_000_000_000.0) as u64);
        NSEvent::eventWithCGEvent(&created).expect("NSEvent")
    }
}

const BEGAN: Option<CGScrollPhase> = Some(CGScrollPhase::Began);
const ENDED: Option<CGScrollPhase> = Some(CGScrollPhase::Ended);

fn sized_controller(reduce_motion: bool) -> Closing {
    let controller = Closing(new_controller());
    if let Some(window) = controller.window() {
        window.setFrame_display(NSRect::new(NSPoint::new(-30000.0, -30000.0), NSSize::new(900.0, 700.0)), false);
        window.layoutIfNeeded();
    }
    controller.primary_container().layoutSubtreeIfNeeded();
    // Always overridden, in both directions: a runner that inherits the
    // machine's Reduce Motion setting would otherwise silently run the wrong
    // state machine.
    let appearance = controller
        .window()
        .map(|window| window.effectiveAppearance())
        .unwrap_or_else(objc2_app_kit::NSAppearance::currentDrawingAppearance);
    controller.set_active_style_sheet(Rc::new(StyleSheet::new(
        controller.active_style_sheet().theme.clone(),
        &appearance,
        Some(reduce_motion),
    )));
    controller
}

fn swiping_left_claims_the_gesture_and_lands_on_source() {
    let controller = sized_controller(true);
    let rail = controller.toolbar_presentation_control().expect("the presentation rail");
    let swipe = controller.presentation_swipe();

    assert!(!swipe.handle(&scroll().phase(BEGAN).at(0.0).event()));
    // Still ambiguous, so the scroll view keeps this one.
    assert!(!swipe.handle(&scroll().delta_x(-6.0).at(0.008).event()));
    // Past the intent threshold: the swipe takes over.
    assert!(swipe.handle(&scroll().delta_x(-10.0).at(0.016).event()));
    assert!(swipe.is_tracking());
    assert!(rail.selection_indicator_frame_for_testing().mid_x() > rail.selected_segment_center_for_testing());

    for step in 3..=12 {
        let event = scroll().delta_x(-20.0).at(step as f64 * 0.008).event();
        assert!(swipe.handle(&event));
    }
    assert!(swipe.handle(&scroll().phase(ENDED).at(0.12).event()));
    assert_eq!(controller.presentation_segment(), 1);
    assert_eq!(rail.selected_segment(), 1);
    assert!(!swipe.is_tracking());
}

fn abandoned_swipe_leaves_the_document_where_it_was() {
    let controller = sized_controller(true);
    let rail = controller.toolbar_presentation_control().expect("the presentation rail");
    let swipe = controller.presentation_swipe();

    let _ = swipe.handle(&scroll().phase(BEGAN).at(0.0).event());
    assert!(swipe.handle(&scroll().delta_x(-16.0).at(0.008).event()));
    // Barely moved and released slowly: not enough to commit.
    assert!(swipe.handle(&scroll().delta_x(-4.0).at(0.4).event()));
    assert!(swipe.handle(&scroll().phase(ENDED).at(0.8).event()));

    assert_eq!(controller.presentation_segment(), 0);
    // The rail is back on Document. Where the indicator has physically
    // reached is the spring's business and is covered on a windowless
    // control above, which settles rather than flies.
    assert_eq!(rail.selected_segment(), 0);
    assert!(!swipe.is_tracking());
}

fn a_swipe_in_split_view_carries_both_panes() {
    let controller = sized_controller(true);
    controller.toggle_split_view();
    if let Some(window) = controller.window() {
        window.layoutIfNeeded();
    }
    let split = controller.split_container().expect("the split pane");
    assert_eq!(controller.document_panes().len(), 2);
    let swipe = controller.presentation_swipe();

    let _ = swipe.handle(&scroll().phase(BEGAN).at(0.0).event());
    for step in 1..=10 {
        let event = scroll().delta_x(-24.0).at(step as f64 * 0.008).event();
        assert!(swipe.handle(&event));
    }
    assert!(swipe.handle(&scroll().phase(ENDED).at(0.1).event()));

    // The mode is the window's, not one pane's: both surfaces land in it.
    assert_ne!(controller.primary_container().text_view().source_focus(), SourceFocus::None);
    assert_ne!(split.text_view().source_focus(), SourceFocus::None);
}

fn a_resize_mid_swipe_grounds_it_rather_than_flying_stale_stills() {
    let controller = sized_controller(false);
    let swipe = controller.presentation_swipe();

    let _ = swipe.handle(&scroll().phase(BEGAN).at(0.0).event());
    assert!(swipe.handle(&scroll().delta_x(-40.0).at(0.008).event()));
    assert!(swipe.is_tracking());

    // `Notification(name: NSWindow.didResizeNotification)`.
    // SAFETY: a plain notification with no object.
    let notification = unsafe {
        NSNotification::notificationWithName_object(objc2_app_kit::NSWindowDidResizeNotification, None)
    };
    controller.window_did_resize(&notification);
    assert_eq!(controller.presentation_segment(), 0);
    assert_eq!(
        controller.primary_container().scroll_view().layer().map(|layer| layer.transform().m41),
        Some(0.0)
    );
    assert!(!swipe.is_tracking());
    assert!(!swipe.is_settling());
}

fn main() {
    controller_support::prepare();
    controller_support::main_thread::run(&[
        ("swiping_left_claims_the_gesture_and_lands_on_source", swiping_left_claims_the_gesture_and_lands_on_source),
        ("abandoned_swipe_leaves_the_document_where_it_was", abandoned_swipe_leaves_the_document_where_it_was),
        ("a_swipe_in_split_view_carries_both_panes", a_swipe_in_split_view_carries_both_panes),
        (
            "a_resize_mid_swipe_grounds_it_rather_than_flying_stale_stills",
            a_resize_mid_swipe_grounds_it_rather_than_flying_stale_stills,
        ),
    ]);
    controller_support::finish();
}
