//! Port of `Tests/DownrightAppTests/PresentationSwipeTests.swift`.
//!
//! The Swift suite is `@MainActor`, and the panes are AppKit views, so this
//! binary owns the main thread (`harness = false`, `tests/main_thread`).
//!
//! The coordinator tests drive `controller.presentationSwipe`. Until
//! `DocumentWindowController` is ported they run over
//! `gesture_stand_in::DocumentStandIn`: a real `MarkdownContainerView` in a
//! borderless window that is never ordered in, with the controller's host
//! wiring for the swipe, except that a presentation change records the
//! segment instead of rebuilding the text view and the rail callbacks do
//! nothing. Only tests whose assertions the stand-in can answer faithfully
//! (the segment, the pane's layer, `isTracking`/`isSettling`) run here.
//!
//! Skipped, with the reason:
//! - `railTracksTheSwipeAndLandsWithoutSwitchingTwice`,
//!   `railReturnsToWhereItStartedWhenTheSwipeIsAbandoned`: not skipped; they
//!   test the rail alone (`ToolbarPresentationControl`, no stand-in) and run
//!   in `toolbar_controls_tests.rs`.
//! - `swipingLeftClaimsTheGestureAndLandsOnSource`,
//!   `abandonedSwipeLeavesTheDocumentWhereItWas`: assert on
//!   `controller.toolbarPresentationControl`, which the window controller
//!   wires to the swipe's rail callbacks (not ported).
//! - `aSwipeInSplitViewCarriesBothPanes`: needs `DocumentWindowController`'s
//!   `toggleSplitView()`, `splitContainer`, and its real presentation switch
//!   (`textView.sourceFocus`).
//! - `aResizeMidSwipeGroundsItRatherThanFlyingStaleStills`: needs
//!   `DocumentWindowController.windowDidResize(_:)`.

mod gesture_stand_in;
mod main_thread;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSEvent, NSEventPhase};
use objc2_core_foundation::CGFloat;
use objc2_core_graphics::{CGEvent, CGEventField, CGMomentumScrollPhase, CGScrollEventUnit, CGScrollPhase};
use upleft_app::app::presentation_drag::PresentationSwitchBudget;
use upleft_app::app::presentation_swipe::{Claim, PresentationSwipePolicy};

use gesture_stand_in::{DocumentStandIn, ResetCalibration};

// MARK: - Synthetic trackpad events

/// The suite's private `scroll(…)`: a continuous scroll event with real
/// phases. Unlike `SyntheticScroll`, it sets no modifier flags and no line
/// deltas, exactly as the Swift helper does.
#[derive(Clone, Copy)]
struct Scroll {
    delta_x: CGFloat,
    delta_y: CGFloat,
    phase: Option<CGScrollPhase>,
    momentum: CGMomentumScrollPhase,
    precise: bool,
    seconds: f64,
}

fn scroll() -> Scroll {
    Scroll {
        delta_x: 0.0,
        delta_y: 0.0,
        phase: Some(CGScrollPhase::Changed),
        momentum: CGMomentumScrollPhase::None,
        precise: true,
        seconds: 0.0,
    }
}

impl Scroll {
    fn delta_x(mut self, value: CGFloat) -> Self {
        self.delta_x = value;
        self
    }

    fn delta_y(mut self, value: CGFloat) -> Self {
        self.delta_y = value;
        self
    }

    fn phase(mut self, value: Option<CGScrollPhase>) -> Self {
        self.phase = value;
        self
    }

    fn momentum(mut self, value: CGMomentumScrollPhase) -> Self {
        self.momentum = value;
        self
    }

    fn precise(mut self, value: bool) -> Self {
        self.precise = value;
        self
    }

    fn at(mut self, seconds: f64) -> Self {
        self.seconds = seconds;
        self
    }

    fn event(&self) -> Retained<NSEvent> {
        let created = CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Pixel, 2, 0, 0, 0).expect("CGEvent");
        let event = Some(&*created);
        CGEvent::set_integer_value_field(event, CGEventField::ScrollWheelEventIsContinuous, if self.precise { 1 } else { 0 });
        CGEvent::set_double_value_field(event, CGEventField::ScrollWheelEventPointDeltaAxis1, self.delta_y);
        CGEvent::set_double_value_field(event, CGEventField::ScrollWheelEventPointDeltaAxis2, self.delta_x);
        if let Some(phase) = self.phase {
            CGEvent::set_integer_value_field(event, CGEventField::ScrollWheelEventScrollPhase, phase.0 as i64);
        }
        CGEvent::set_integer_value_field(event, CGEventField::ScrollWheelEventMomentumPhase, self.momentum.0 as i64);
        let seconds = if self.seconds >= 0.0 { self.seconds } else { 0.0 };
        CGEvent::set_timestamp(event, (seconds * 1_000_000_000.0) as u64);
        NSEvent::eventWithCGEvent(&created).expect("NSEvent")
    }
}

const BEGAN: Option<CGScrollPhase> = Some(CGScrollPhase::Began);
const ENDED: Option<CGScrollPhase> = Some(CGScrollPhase::Ended);

fn sized_controller(reduce_motion: bool) -> DocumentStandIn {
    DocumentStandIn::sized(reduce_motion, MainThreadMarker::new().expect("main thread"))
}

fn synthetic_scroll_events_carry_trackpad_phases_and_precise_deltas() {
    let event = scroll().delta_x(-20.0).delta_y(3.0).phase(BEGAN).at(1.0).event();
    assert!(event.hasPreciseScrollingDeltas());
    assert_eq!(event.phase(), NSEventPhase::Began);
    assert!(event.momentumPhase().is_empty());
    assert!((event.scrollingDeltaX() - -20.0).abs() < 0.001);
    assert!((event.scrollingDeltaY() - 3.0).abs() < 0.001);

    let ended = scroll().phase(ENDED).event();
    assert_eq!(ended.phase(), NSEventPhase::Ended);

    let coasting = scroll().delta_x(-8.0).phase(None).momentum(CGMomentumScrollPhase::Continue).event();
    assert!(!coasting.momentumPhase().is_empty());
}

// MARK: - Policy

fn swipe_is_claimed_only_when_it_is_decidedly_sideways() {
    // Nothing has happened yet: the scroll view keeps the event, so
    // vertical scrolling never waits on this decision.
    assert_eq!(PresentationSwipePolicy::claim(4.0, 2.0), Claim::Undecided);
    assert_eq!(PresentationSwipePolicy::claim(-11.0, 0.0), Claim::Undecided);
    // Sideways and past the threshold.
    assert_eq!(PresentationSwipePolicy::claim(-12.0, 0.0), Claim::Swipe);
    assert_eq!(PresentationSwipePolicy::claim(30.0, 20.0), Claim::Swipe);
    // A diagonal is a scroll: sideways travel that has not clearly beaten
    // the vertical is the far commoner intent, and once the page has moved
    // that far the gesture stops being re-litigated.
    assert_eq!(PresentationSwipePolicy::claim(30.0, 25.0), Claim::Scroll);
    assert_eq!(PresentationSwipePolicy::claim(10.0, 40.0), Claim::Scroll);
    assert_eq!(PresentationSwipePolicy::claim(0.0, -12.0), Claim::Scroll);
    // Below both thresholds nothing has been decided yet either way.
    assert_eq!(PresentationSwipePolicy::claim(9.0, 8.0), Claim::Undecided);
}

fn content_follows_the_fingers_onto_the_rail() {
    // Pushing the page left uncovers Source, which sits right on the rail.
    assert_eq!(PresentationSwipePolicy::target_segment(-40.0), Some(1));
    assert_eq!(PresentationSwipePolicy::target_segment(40.0), Some(0));
    assert_eq!(PresentationSwipePolicy::target_segment(0.0), None);
}

fn commit_distance_stays_reachable_at_every_pane_width() {
    // A narrow split pane must not switch on a twitch…
    assert_eq!(PresentationSwipePolicy::commit_distance(120.0), PresentationSwipePolicy::MINIMUM_COMMIT_DISTANCE);
    // …and a wide window must not ask for a swipe past the trackpad.
    assert_eq!(PresentationSwipePolicy::commit_distance(2400.0), PresentationSwipePolicy::MAXIMUM_COMMIT_DISTANCE);
    assert_eq!(PresentationSwipePolicy::commit_distance(400.0), 100.0);
}

fn rail_fills_exactly_where_releasing_would_commit() {
    let width: CGFloat = 900.0;
    let distance = PresentationSwipePolicy::commit_distance(width);
    assert_eq!(PresentationSwipePolicy::rail_progress(0.0, width), 0.0);
    assert!((PresentationSwipePolicy::rail_progress(-distance / 2.0, width) - 0.5).abs() < 0.001);
    assert_eq!(PresentationSwipePolicy::rail_progress(-distance, width), 1.0);
    // Past the commit point the bar has nowhere further to go.
    assert_eq!(PresentationSwipePolicy::rail_progress(-width, width), 1.0);
}

fn rail_position_runs_from_the_mode_you_are_in_toward_the_one_you_are_heading_for() {
    assert_eq!(PresentationSwipePolicy::rail_position(0.0, 0, 1), 0.0);
    assert_eq!(PresentationSwipePolicy::rail_position(0.5, 0, 1), 0.5);
    assert_eq!(PresentationSwipePolicy::rail_position(1.0, 0, 1), 1.0);
    // Coming back the other way the bar travels toward Document.
    assert_eq!(PresentationSwipePolicy::rail_position(0.25, 1, 0), 0.75);
    assert_eq!(PresentationSwipePolicy::rail_position(4.0, 1, 0), 0.0);
}

fn a_short_swipe_commits_only_when_it_is_still_travelling() {
    let width: CGFloat = 900.0;
    let distance = PresentationSwipePolicy::commit_distance(width);

    assert!(PresentationSwipePolicy::should_commit(-distance, 0.0, width));
    assert!(!PresentationSwipePolicy::should_commit(-distance + 1.0, 0.0, width));
    // The flick: short, but leaving fast in the direction it was going.
    assert!(PresentationSwipePolicy::should_commit(-20.0, -600.0, width));
    // Pulled back at the last moment. However fast the hand was moving,
    // reversing means "put it back".
    assert!(!PresentationSwipePolicy::should_commit(-20.0, 600.0, width));
    assert!(!PresentationSwipePolicy::should_commit(0.0, -900.0, width));
}

fn the_page_gives_against_the_fingers_and_never_arrives() {
    let limit = PresentationSwipePolicy::MAXIMUM_GIVE;
    assert_eq!(PresentationSwipePolicy::give(0.0), 0.0);
    // It answers immediately…
    assert!(PresentationSwipePolicy::give(-8.0) < -1.0);
    // …keeps its sign…
    assert!(PresentationSwipePolicy::give(40.0) > 0.0);
    assert!(PresentationSwipePolicy::give(-40.0) < 0.0);
    // …grows monotonically…
    assert!(PresentationSwipePolicy::give(-40.0).abs() < PresentationSwipePolicy::give(-140.0).abs());
    // …and asymptotes rather than hitting a wall the hand can feel.
    assert!(PresentationSwipePolicy::give(-10_000.0).abs() < limit);
    assert!(PresentationSwipePolicy::give(-10_000.0).abs() > limit * 0.99);
}

// MARK: - Coordinator

fn scrolling_the_page_is_never_taken_for_a_swipe() {
    let controller = sized_controller(true);
    let swipe = controller.presentation_swipe();

    assert!(!swipe.handle(&scroll().phase(BEGAN).at(0.0).event()));
    for step in 1..=12 {
        let event = scroll().delta_x(-1.0).delta_y(-30.0).at(step as f64 * 0.008).event();
        assert!(!swipe.handle(&event));
    }
    assert!(!swipe.handle(&scroll().phase(ENDED).at(0.2).event()));
    assert_eq!(controller.presentation_segment(), 0);
    assert!(!swipe.is_tracking());
}

fn a_wheel_without_precise_deltas_is_left_to_the_scroll_view() {
    let controller = sized_controller(true);
    let swipe = controller.presentation_swipe();

    let event = scroll().delta_x(-80.0).phase(Some(CGScrollPhase::Changed)).precise(false).event();
    assert!(!swipe.handle(&event));
    assert_eq!(controller.presentation_segment(), 0);
}

fn swiping_toward_the_mode_you_are_already_in_is_not_claimed() {
    let controller = sized_controller(true);
    let swipe = controller.presentation_swipe();

    let _ = swipe.handle(&scroll().phase(BEGAN).at(0.0).event());
    // Document is already showing, and nothing sits to the left of it.
    assert!(!swipe.handle(&scroll().delta_x(40.0).at(0.008).event()));
    assert!(!swipe.is_tracking());
    assert_eq!(controller.presentation_segment(), 0);
}

fn swiping_back_from_source_returns_to_the_document() {
    let controller = sized_controller(true);
    let swipe = controller.presentation_swipe();
    controller.change_presentation(1);
    assert_eq!(controller.presentation_segment(), 1);

    let _ = swipe.handle(&scroll().phase(BEGAN).at(0.0).event());
    for step in 1..=10 {
        let event = scroll().delta_x(24.0).at(step as f64 * 0.008).event();
        assert!(swipe.handle(&event));
    }
    assert!(swipe.handle(&scroll().phase(ENDED).at(0.1).event()));
    assert_eq!(controller.presentation_segment(), 0);
}

fn the_give_path_renders_nothing_until_the_fingers_lift() {
    // Priced out of the drag on purpose. This is the path a long document
    // takes, and its whole promise is that it builds nothing — so the test
    // must not depend on how long the fixture happens to be.
    PresentationSwitchBudget::reset_calibration_for_testing_to(100.0);
    let _reset = ResetCalibration;

    let controller = sized_controller(false);
    let swipe = controller.presentation_swipe();
    let pane_subviews = controller.primary_container.subviews().count();

    let _ = swipe.handle(&scroll().phase(BEGAN).at(0.0).event());
    assert!(swipe.handle(&scroll().delta_x(-40.0).at(0.008).event()));
    // The gesture asks for the backing store itself, so the layer only
    // exists once a swipe has actually caught.
    let scroll_layer = controller.primary_container.scroll_view().layer().expect("the scroll view's layer");

    // Mid-gesture the document has not been rebuilt and no surface has
    // been added: the whole transition so far is one layer translation.
    assert_eq!(controller.presentation_segment(), 0);
    assert_eq!(controller.primary_container.subviews().count(), pane_subviews);
    assert!(scroll_layer.transform().m41 < 0.0);
    // …and it leans, rather than travelling: the give never promises the
    // page is really going anywhere.
    assert!(scroll_layer.transform().m41.abs() <= PresentationSwipePolicy::MAXIMUM_GIVE);
    assert_eq!(controller.primary_container.layer().map(|layer| layer.masksToBounds()), Some(true));

    swipe.cancel_in_flight();
    assert_eq!(controller.presentation_segment(), 0);
    assert_eq!(scroll_layer.transform().m41, 0.0);

    // `defer { controller.presentationSwipe.cancelInFlight() … }`.
    swipe.cancel_in_flight();
}

fn the_give_puts_the_page_back_before_the_switch_draws() {
    PresentationSwitchBudget::reset_calibration_for_testing_to(100.0);
    let _reset = ResetCalibration;
    let controller = sized_controller(false);
    let swipe = controller.presentation_swipe();
    let _ = swipe.handle(&scroll().phase(BEGAN).at(0.0).event());
    for step in 1..=10 {
        let _ = swipe.handle(&scroll().delta_x(-20.0).at(step as f64 * 0.008).event());
    }
    let scroll_layer = controller.primary_container.scroll_view().layer().expect("the scroll view's layer");
    assert!(scroll_layer.transform().m41 < 0.0);
    assert!(swipe.handle(&scroll().phase(ENDED).at(0.1).event()));

    // The mode change animates from the pane's resting frame, so the give
    // has to be surrendered before it runs, not after.
    assert_eq!(scroll_layer.transform().m41, 0.0);
    assert_eq!(controller.presentation_segment(), 1);
    assert!(!swipe.is_tracking());
    assert!(!swipe.is_settling());
}

fn main() {
    main_thread::run(&[
        (
            "synthetic_scroll_events_carry_trackpad_phases_and_precise_deltas",
            synthetic_scroll_events_carry_trackpad_phases_and_precise_deltas,
        ),
        ("swipe_is_claimed_only_when_it_is_decidedly_sideways", swipe_is_claimed_only_when_it_is_decidedly_sideways),
        ("content_follows_the_fingers_onto_the_rail", content_follows_the_fingers_onto_the_rail),
        ("commit_distance_stays_reachable_at_every_pane_width", commit_distance_stays_reachable_at_every_pane_width),
        ("rail_fills_exactly_where_releasing_would_commit", rail_fills_exactly_where_releasing_would_commit),
        (
            "rail_position_runs_from_the_mode_you_are_in_toward_the_one_you_are_heading_for",
            rail_position_runs_from_the_mode_you_are_in_toward_the_one_you_are_heading_for,
        ),
        ("a_short_swipe_commits_only_when_it_is_still_travelling", a_short_swipe_commits_only_when_it_is_still_travelling),
        ("the_page_gives_against_the_fingers_and_never_arrives", the_page_gives_against_the_fingers_and_never_arrives),
        ("scrolling_the_page_is_never_taken_for_a_swipe", scrolling_the_page_is_never_taken_for_a_swipe),
        ("a_wheel_without_precise_deltas_is_left_to_the_scroll_view", a_wheel_without_precise_deltas_is_left_to_the_scroll_view),
        ("swiping_toward_the_mode_you_are_already_in_is_not_claimed", swiping_toward_the_mode_you_are_already_in_is_not_claimed),
        ("swiping_back_from_source_returns_to_the_document", swiping_back_from_source_returns_to_the_document),
        ("the_give_path_renders_nothing_until_the_fingers_lift", the_give_path_renders_nothing_until_the_fingers_lift),
        ("the_give_puts_the_page_back_before_the_switch_draws", the_give_puts_the_page_back_before_the_switch_draws),
    ]);
}
