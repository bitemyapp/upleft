//! Port of `Tests/DownrightAppTests/ScrollZoomTests.swift`.
//!
//! The Swift suite is `@MainActor`, so this binary owns the main thread
//! (`harness = false`, `tests/main_thread`).
//!
//! Skipped, with the reason (the "Wired into the window" cases need
//! `DocumentWindowController`, which is being ported elsewhere):
//! - `optionScrollMovesTheDocumentsDetailLevelThroughTheRealCommands`:
//!   `documentScrollGestures`, `containerTextView.zoomLevel`,
//!   `markdownDocument.state.zoomLevel`.
//! - `commandScrollMovesTheReadingTextSize`: `documentScrollGestures` and
//!   `Preferences.shared.values.textSizeAdjustment` through the controller.
//! - `aModifiedSidewaysScrollNeverSwitchesPresentation`:
//!   `documentScrollGestures`, `presentationSegment`, `presentationSwipe`.

mod main_thread;
mod synthetic_scroll_events;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2_app_kit::NSEventModifierFlags;
use objc2_core_graphics::{CGMomentumScrollPhase, CGScrollPhase};
use upleft_app::app::document_scroll_gestures::ScrollGestureModifiers;
use upleft_app::app::scroll_zoom::{Host, Intent, ScrollZoomCoordinator, ScrollZoomPolicy};

use synthetic_scroll_events::SyntheticScroll;

// MARK: - Synthetic events

/// The suite's `scroll(…)`: nothing held by default.
fn scroll() -> SyntheticScroll {
    SyntheticScroll::new()
}

const BEGAN: Option<CGScrollPhase> = Some(CGScrollPhase::Began);
const ENDED: Option<CGScrollPhase> = Some(CGScrollPhase::Ended);
const COMMAND: NSEventModifierFlags = NSEventModifierFlags::Command;
const OPTION: NSEventModifierFlags = NSEventModifierFlags::Option;

/// A coordinator with everything it touches recorded rather than done.
#[derive(Default)]
struct Recorder {
    text_steps: RefCell<Vec<isize>>,
    detail_steps: RefCell<Vec<isize>>,
    haptics: Cell<usize>,
}

impl Recorder {
    fn new() -> Rc<Recorder> {
        Rc::new(Recorder::default())
    }

    fn host(self: &Rc<Self>) -> Host {
        let text = self.clone();
        let detail = self.clone();
        let haptic = self.clone();
        Host {
            step_text_size: Box::new(move |steps| text.text_steps.borrow_mut().push(steps)),
            step_detail: Box::new(move |steps| detail.detail_steps.borrow_mut().push(steps)),
            perform_haptic_feedback: Box::new(move || haptic.haptics.set(haptic.haptics.get() + 1)),
        }
    }

    fn text_steps(&self) -> Vec<isize> {
        self.text_steps.borrow().clone()
    }

    fn detail_steps(&self) -> Vec<isize> {
        self.detail_steps.borrow().clone()
    }
}

fn synthetic_events_carry_modifiers_and_both_delta_kinds() {
    let trackpad = scroll().delta_y(-18.0).modifiers(COMMAND).at(1.0).event();
    assert!(trackpad.hasPreciseScrollingDeltas());
    assert!(trackpad.modifierFlags().contains(NSEventModifierFlags::Command));
    assert!(!trackpad.modifierFlags().contains(NSEventModifierFlags::Option));
    assert!((trackpad.scrollingDeltaY() - -18.0).abs() < 0.001);

    // A wheel reports lines, and the coarse path only works if the event
    // really carries them.
    let wheel = scroll().delta_y(3.0).phase(None).precise(false).modifiers(OPTION).event();
    assert!(!wheel.hasPreciseScrollingDeltas());
    assert!(wheel.modifierFlags().contains(NSEventModifierFlags::Option));
    assert!((wheel.scrollingDeltaY() - 3.0).abs() < 0.001);

    // Nothing held means nothing held, whatever the machine running the
    // test has its hands on.
    let bare = scroll().delta_y(4.0).event();
    assert!(ScrollGestureModifiers::held(&bare).is_empty());

    // Time is a real input here — the wheel's gesture boundary and every
    // flick are read out of it — so the synthesized clock has to survive
    // the round trip through `CGEvent`.
    let later = scroll().delta_y(4.0).at(2.0).event();
    assert!(((later.timestamp() - bare.timestamp()) - 2.0).abs() < 0.001);
}

// MARK: - Policy

fn only_exactly_command_or_exactly_option_is_a_zoom() {
    assert_eq!(ScrollZoomPolicy::intent(NSEventModifierFlags::Command), Some(Intent::TextSize));
    assert_eq!(ScrollZoomPolicy::intent(NSEventModifierFlags::Option), Some(Intent::StructuralDetail));
    assert_eq!(ScrollZoomPolicy::intent(NSEventModifierFlags::empty()), None);
    // ⇧⌘ is jump history's business plus a stray finger; a zoom that
    // answered to any superset would fire underneath it.
    assert_eq!(ScrollZoomPolicy::intent(NSEventModifierFlags::Command | NSEventModifierFlags::Shift), None);
    assert_eq!(ScrollZoomPolicy::intent(NSEventModifierFlags::Option | NSEventModifierFlags::Control), None);
    assert_eq!(ScrollZoomPolicy::intent(NSEventModifierFlags::Control), None);
    assert_eq!(ScrollZoomPolicy::intent(NSEventModifierFlags::Shift), None);
}

fn caps_lock_and_fn_ride_along_without_cancelling_a_zoom() {
    let event = scroll()
        .delta_y(-10.0)
        .modifiers(NSEventModifierFlags::Command | NSEventModifierFlags::CapsLock | NSEventModifierFlags::Function)
        .event();
    assert_eq!(ScrollZoomPolicy::intent(ScrollGestureModifiers::held(&event)), Some(Intent::TextSize));
}

fn a_wheels_acceleration_is_thrown_away_and_a_trackpads_points_are_not() {
    // Points are what the fingers actually travelled.
    assert_eq!(ScrollZoomPolicy::contribution(37.5, true), 37.5);
    assert_eq!(ScrollZoomPolicy::contribution(-37.5, true), -37.5);
    // Lines arrive with macOS's acceleration curve baked in, and that
    // curve turns one flick of the wheel into the whole scale.
    assert_eq!(ScrollZoomPolicy::contribution(12.0, false), 1.0);
    assert_eq!(ScrollZoomPolicy::contribution(-12.0, false), -1.0);
    assert_eq!(ScrollZoomPolicy::contribution(1.0, false), 1.0);
    assert_eq!(ScrollZoomPolicy::contribution(0.0, false), 0.0);
}

fn detail_asks_for_more_travel_than_text_size() {
    assert!(
        ScrollZoomPolicy::step_threshold(Intent::StructuralDetail, true)
            > ScrollZoomPolicy::step_threshold(Intent::TextSize, true)
    );
    // A wheel's notch is the detent, for both scales.
    assert_eq!(ScrollZoomPolicy::step_threshold(Intent::TextSize, false), ScrollZoomPolicy::WHEEL_STEP_LINES);
    assert_eq!(ScrollZoomPolicy::step_threshold(Intent::StructuralDetail, false), ScrollZoomPolicy::WHEEL_STEP_LINES);
}

fn text_size_ramps_and_structural_detail_does_not() {
    assert!(ScrollZoomPolicy::allows_further_steps(Intent::TextSize, 0));
    assert!(ScrollZoomPolicy::allows_further_steps(Intent::TextSize, 9));
    assert!(ScrollZoomPolicy::allows_further_steps(Intent::StructuralDetail, 0));
    assert!(!ScrollZoomPolicy::allows_further_steps(Intent::StructuralDetail, 1));
    assert!(!ScrollZoomPolicy::allows_further_steps(Intent::StructuralDetail, -1));
}

fn steps_are_earned_by_travel_and_capped_by_what_the_gesture_has_left() {
    let text = ScrollZoomPolicy::TEXT_SIZE_STEP_POINTS;
    assert_eq!(ScrollZoomPolicy::steps(Intent::TextSize, text - 0.01, 0, true), 0);
    assert_eq!(ScrollZoomPolicy::steps(Intent::TextSize, text, 0, true), 1);
    // A ramp is allowed to be a ramp.
    assert_eq!(ScrollZoomPolicy::steps(Intent::TextSize, text * 3.5, 4, true), 3);
    assert_eq!(ScrollZoomPolicy::steps(Intent::TextSize, -text * 2.0, 0, true), -2);

    let detail = ScrollZoomPolicy::DETAIL_STEP_POINTS;
    assert_eq!(ScrollZoomPolicy::steps(Intent::StructuralDetail, detail - 0.01, 0, true), 0);
    assert_eq!(ScrollZoomPolicy::steps(Intent::StructuralDetail, detail, 0, true), 1);
    // Five levels per flick is the failure this whole detent exists for:
    // one event that crosses the line five times still buys one step.
    assert_eq!(ScrollZoomPolicy::steps(Intent::StructuralDetail, detail * 5.0, 0, true), 1);
    assert_eq!(ScrollZoomPolicy::steps(Intent::StructuralDetail, -detail * 5.0, 0, true), -1);
    // …and once it is spent the gesture is over as far as detail goes.
    assert_eq!(ScrollZoomPolicy::steps(Intent::StructuralDetail, detail * 5.0, 1, true), 0);
}

fn a_wheels_gesture_is_the_quiet_around_it() {
    assert!(!ScrollZoomPolicy::starts_new_wheel_gesture(0.0));
    // A spin: events a couple of frames apart.
    assert!(!ScrollZoomPolicy::starts_new_wheel_gesture(0.03));
    assert!(!ScrollZoomPolicy::starts_new_wheel_gesture(ScrollZoomPolicy::WHEEL_GESTURE_GAP - 0.001));
    // A notch, a look, another notch.
    assert!(ScrollZoomPolicy::starts_new_wheel_gesture(ScrollZoomPolicy::WHEEL_GESTURE_GAP));
    assert!(ScrollZoomPolicy::starts_new_wheel_gesture(5.0));
}

// MARK: - Coordinator: nothing held

fn an_unmodified_scroll_is_never_claimed_and_never_zooms() {
    let recorder = Recorder::new();
    let zoom = ScrollZoomCoordinator::new(recorder.host());

    assert!(!zoom.handle(&scroll().phase(BEGAN).at(0.0).event()));
    for step in 1..=20 {
        assert!(!zoom.handle(&scroll().delta_y(-40.0).at(step as f64 * 0.008).event()));
    }
    assert!(!zoom.handle(&scroll().phase(ENDED).at(0.2).event()));
    assert!(recorder.text_steps().is_empty());
    assert!(recorder.detail_steps().is_empty());
}

// MARK: - Coordinator: ⌘ text size

fn command_scroll_banks_travel_and_releases_whole_steps() {
    let recorder = Recorder::new();
    let zoom = ScrollZoomCoordinator::new(recorder.host());
    let step = ScrollZoomPolicy::TEXT_SIZE_STEP_POINTS;

    assert!(zoom.handle(&scroll().phase(BEGAN).modifiers(COMMAND).at(0.0).event()));
    // Short of the threshold the event is still swallowed — the page must
    // not scroll while ⌘ is down — but nothing has been earned yet.
    assert!(zoom.handle(&scroll().delta_y(step / 2.0).modifiers(COMMAND).at(0.008).event()));
    assert!(recorder.text_steps().is_empty());

    assert!(zoom.handle(&scroll().delta_y(step / 2.0).modifiers(COMMAND).at(0.016).event()));
    assert_eq!(recorder.text_steps(), vec![1]);
    // Only the travel the step cost is spent; the remainder rolls on, so a
    // steady drag steps at a steady rate instead of stuttering.
    assert!(zoom.accumulated_travel_for_testing().abs() < 0.001);

    assert!(zoom.handle(&scroll().delta_y(step * 2.0).modifiers(COMMAND).at(0.024).event()));
    assert_eq!(recorder.text_steps(), vec![1, 2]);
    assert!(recorder.detail_steps().is_empty());
    // Sizing text is not a detent; it is a ramp, and a ramp does not tap.
    assert_eq!(recorder.haptics.get(), 0);
}

fn command_scroll_the_other_way_makes_text_smaller() {
    let recorder = Recorder::new();
    let zoom = ScrollZoomCoordinator::new(recorder.host());

    assert!(zoom.handle(&scroll().phase(BEGAN).modifiers(COMMAND).at(0.0).event()));
    assert!(zoom.handle(&scroll().delta_y(-ScrollZoomPolicy::TEXT_SIZE_STEP_POINTS).modifiers(COMMAND).at(0.008).event()));
    assert_eq!(recorder.text_steps(), vec![-1]);
}

fn the_momentum_tail_is_swallowed_but_never_zooms() {
    let recorder = Recorder::new();
    let zoom = ScrollZoomCoordinator::new(recorder.host());
    let step = ScrollZoomPolicy::TEXT_SIZE_STEP_POINTS;

    assert!(zoom.handle(&scroll().phase(BEGAN).modifiers(COMMAND).at(0.0).event()));
    assert!(zoom.handle(&scroll().delta_y(step).modifiers(COMMAND).at(0.008).event()));
    assert!(zoom.handle(&scroll().phase(ENDED).modifiers(COMMAND).at(0.016).event()));
    assert_eq!(recorder.text_steps(), vec![1]);

    // The coast after the fingers lift. Handing it back would scroll the
    // page out from under a size the reader just settled on.
    for tick in 1..=20 {
        let event = scroll()
            .delta_y(200.0)
            .phase(None)
            .momentum(CGMomentumScrollPhase::Continue)
            .modifiers(COMMAND)
            .at(0.016 + tick as f64 * 0.008)
            .event();
        assert!(zoom.handle(&event));
    }
    assert_eq!(recorder.text_steps(), vec![1]);
}

fn letting_go_of_the_modifier_hands_the_rest_of_the_gesture_back() {
    let recorder = Recorder::new();
    let zoom = ScrollZoomCoordinator::new(recorder.host());
    let step = ScrollZoomPolicy::TEXT_SIZE_STEP_POINTS;

    assert!(zoom.handle(&scroll().phase(BEGAN).modifiers(COMMAND).at(0.0).event()));
    assert!(zoom.handle(&scroll().delta_y(step).modifiers(COMMAND).at(0.008).event()));
    assert_eq!(recorder.text_steps(), vec![1]);
    // ⌘ up mid-gesture: the rest of it is an ordinary scroll again.
    assert!(!zoom.handle(&scroll().delta_y(step * 4.0).at(0.016).event()));
    assert_eq!(recorder.text_steps(), vec![1]);
}

// MARK: - Coordinator: ⌥ structural detail

fn option_scroll_spends_exactly_one_level_per_trackpad_gesture() {
    let recorder = Recorder::new();
    let zoom = ScrollZoomCoordinator::new(recorder.host());

    assert!(zoom.handle(&scroll().phase(BEGAN).modifiers(OPTION).at(0.0).event()));
    // A hard flick: far more travel than one level's worth, all in one
    // gesture. Five relayouts of the whole document is the thing this is
    // here to prevent.
    for step in 1..=20 {
        let event = scroll().delta_y(-40.0).modifiers(OPTION).at(step as f64 * 0.008).event();
        assert!(zoom.handle(&event));
    }
    assert_eq!(recorder.detail_steps(), vec![-1]);
    assert_eq!(recorder.haptics.get(), 1);
    assert!(zoom.handle(&scroll().phase(ENDED).modifiers(OPTION).at(0.2).event()));

    // Fingers down again is a new gesture, and buys one more.
    assert!(zoom.handle(&scroll().phase(BEGAN).modifiers(OPTION).at(0.4).event()));
    for step in 1..=20 {
        let event = scroll().delta_y(-40.0).modifiers(OPTION).at(0.4 + step as f64 * 0.008).event();
        assert!(zoom.handle(&event));
    }
    assert_eq!(recorder.detail_steps(), vec![-1, -1]);
    assert_eq!(recorder.haptics.get(), 2);
}

fn a_short_option_nudge_does_not_move_the_detail_level() {
    let recorder = Recorder::new();
    let zoom = ScrollZoomCoordinator::new(recorder.host());

    assert!(zoom.handle(&scroll().phase(BEGAN).modifiers(OPTION).at(0.0).event()));
    assert!(zoom.handle(&scroll().delta_y(-(ScrollZoomPolicy::DETAIL_STEP_POINTS - 1.0)).modifiers(OPTION).at(0.008).event()));
    assert!(zoom.handle(&scroll().phase(ENDED).modifiers(OPTION).at(0.016).event()));
    assert!(recorder.detail_steps().is_empty());
    // …and the travel that did not earn anything is not banked toward the
    // next gesture, which would fire early for no visible reason.
    assert_eq!(zoom.accumulated_travel_for_testing(), 0.0);
    assert_eq!(zoom.steps_fired_for_testing(), 0);
}

fn option_scroll_upward_shows_more_detail() {
    let recorder = Recorder::new();
    let zoom = ScrollZoomCoordinator::new(recorder.host());

    assert!(zoom.handle(&scroll().phase(BEGAN).modifiers(OPTION).at(0.0).event()));
    assert!(zoom.handle(&scroll().delta_y(ScrollZoomPolicy::DETAIL_STEP_POINTS).modifiers(OPTION).at(0.008).event()));
    assert_eq!(recorder.detail_steps(), vec![1]);
}

// MARK: - Coordinator: the wheel

fn one_wheel_notch_is_one_text_size_step_however_hard_it_is_spun() {
    let recorder = Recorder::new();
    let zoom = ScrollZoomCoordinator::new(recorder.host());

    // A gentle notch.
    assert!(zoom.handle(&scroll().delta_y(1.0).phase(None).precise(false).modifiers(COMMAND).at(1.0).event()));
    assert_eq!(recorder.text_steps(), vec![1]);
    // An accelerated one: macOS reports twelve lines, the hand turned one
    // notch, and one notch is what it gets.
    assert!(zoom.handle(&scroll().delta_y(12.0).phase(None).precise(false).modifiers(COMMAND).at(1.03).event()));
    assert_eq!(recorder.text_steps(), vec![1, 1]);
    // Text size is a ramp, so a spin keeps ramping.
    assert!(zoom.handle(&scroll().delta_y(12.0).phase(None).precise(false).modifiers(COMMAND).at(1.06).event()));
    assert_eq!(recorder.text_steps(), vec![1, 1, 1]);
}

fn a_spin_of_the_wheel_is_one_structural_level_and_a_deliberate_notch_is_another() {
    let recorder = Recorder::new();
    let zoom = ScrollZoomCoordinator::new(recorder.host());

    // A spin: notches a frame or two apart, accelerated hard.
    for tick in 0..=9 {
        let event = scroll()
            .delta_y(-8.0)
            .phase(None)
            .precise(false)
            .modifiers(OPTION)
            .at(1.0 + tick as f64 * 0.03)
            .event();
        assert!(zoom.handle(&event));
    }
    assert_eq!(recorder.detail_steps(), vec![-1]);

    // A pause, then a deliberate notch: the reader looked at the result
    // and asked for one more.
    assert!(zoom.handle(
        &scroll()
            .delta_y(-1.0)
            .phase(None)
            .precise(false)
            .modifiers(OPTION)
            .at(1.0 + 9.0 * 0.03 + ScrollZoomPolicy::WHEEL_GESTURE_GAP + 0.05)
            .event()
    ));
    assert_eq!(recorder.detail_steps(), vec![-1, -1]);
    assert_eq!(recorder.haptics.get(), 2);
}

fn switching_modifier_mid_stream_starts_the_other_scale_from_zero() {
    let recorder = Recorder::new();
    let zoom = ScrollZoomCoordinator::new(recorder.host());

    assert!(zoom.handle(&scroll().phase(BEGAN).modifiers(OPTION).at(0.0).event()));
    assert!(zoom.handle(&scroll().delta_y(-(ScrollZoomPolicy::DETAIL_STEP_POINTS - 5.0)).modifiers(OPTION).at(0.008).event()));
    assert!(recorder.detail_steps().is_empty());

    // ⌥ up, ⌘ down, same fingers still moving. The travel banked toward a
    // detail level must not fall through into a text-size step.
    assert!(zoom.handle(&scroll().delta_y(-5.0).modifiers(COMMAND).at(0.016).event()));
    assert!(recorder.text_steps().is_empty());
    assert!(recorder.detail_steps().is_empty());
    assert!(zoom.handle(&scroll().delta_y(-ScrollZoomPolicy::TEXT_SIZE_STEP_POINTS).modifiers(COMMAND).at(0.024).event()));
    assert_eq!(recorder.text_steps(), vec![-1]);
}

fn main() {
    main_thread::run(&[
        ("synthetic_events_carry_modifiers_and_both_delta_kinds", synthetic_events_carry_modifiers_and_both_delta_kinds),
        ("only_exactly_command_or_exactly_option_is_a_zoom", only_exactly_command_or_exactly_option_is_a_zoom),
        ("caps_lock_and_fn_ride_along_without_cancelling_a_zoom", caps_lock_and_fn_ride_along_without_cancelling_a_zoom),
        (
            "a_wheels_acceleration_is_thrown_away_and_a_trackpads_points_are_not",
            a_wheels_acceleration_is_thrown_away_and_a_trackpads_points_are_not,
        ),
        ("detail_asks_for_more_travel_than_text_size", detail_asks_for_more_travel_than_text_size),
        ("text_size_ramps_and_structural_detail_does_not", text_size_ramps_and_structural_detail_does_not),
        (
            "steps_are_earned_by_travel_and_capped_by_what_the_gesture_has_left",
            steps_are_earned_by_travel_and_capped_by_what_the_gesture_has_left,
        ),
        ("a_wheels_gesture_is_the_quiet_around_it", a_wheels_gesture_is_the_quiet_around_it),
        ("an_unmodified_scroll_is_never_claimed_and_never_zooms", an_unmodified_scroll_is_never_claimed_and_never_zooms),
        ("command_scroll_banks_travel_and_releases_whole_steps", command_scroll_banks_travel_and_releases_whole_steps),
        ("command_scroll_the_other_way_makes_text_smaller", command_scroll_the_other_way_makes_text_smaller),
        ("the_momentum_tail_is_swallowed_but_never_zooms", the_momentum_tail_is_swallowed_but_never_zooms),
        (
            "letting_go_of_the_modifier_hands_the_rest_of_the_gesture_back",
            letting_go_of_the_modifier_hands_the_rest_of_the_gesture_back,
        ),
        (
            "option_scroll_spends_exactly_one_level_per_trackpad_gesture",
            option_scroll_spends_exactly_one_level_per_trackpad_gesture,
        ),
        ("a_short_option_nudge_does_not_move_the_detail_level", a_short_option_nudge_does_not_move_the_detail_level),
        ("option_scroll_upward_shows_more_detail", option_scroll_upward_shows_more_detail),
        (
            "one_wheel_notch_is_one_text_size_step_however_hard_it_is_spun",
            one_wheel_notch_is_one_text_size_step_however_hard_it_is_spun,
        ),
        (
            "a_spin_of_the_wheel_is_one_structural_level_and_a_deliberate_notch_is_another",
            a_spin_of_the_wheel_is_one_structural_level_and_a_deliberate_notch_is_another,
        ),
        (
            "switching_modifier_mid_stream_starts_the_other_scale_from_zero",
            switching_modifier_mid_stream_starts_the_other_scale_from_zero,
        ),
    ]);
}
