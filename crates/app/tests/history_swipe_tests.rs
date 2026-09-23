//! Port of `Tests/DownrightAppTests/HistorySwipeTests.swift`.
//!
//! The Swift suite is `@MainActor`, and the panes are AppKit views, so this
//! binary owns the main thread (`harness = false`, `tests/main_thread`).
//!
//! The coordinator tests build their own `HistorySwipeCoordinator` and use the
//! Swift `DocumentWindowController` only for its panes and style sheet; here
//! they run over `gesture_stand_in::DocumentStandIn` (a real
//! `MarkdownContainerView` in a borderless window that is never ordered in)
//! until the window controller is ported.
//!
//! Skipped, with the reason (need `DocumentWindowController` itself, which is
//! being ported elsewhere):
//! - `aSwipeInSplitViewCarriesBothPanes`: `toggleSplitView()`,
//!   `splitContainer` and `controller.historySwipe`.
//! - `aResizeMidSwipeGroundsItRatherThanHoldingAStaleTransform`:
//!   `recordJump(to:label:)`, `jumpHistory`, `controller.historySwipe` and
//!   `windowDidResize(_:)`.
//! - `shiftSwipingRightMovesBackThroughRealJumpHistory`:
//!   `documentScrollGestures`, `recordJump(to:label:)`, `jumpHistory`,
//!   `presentationSegment`.
//! - `shiftSwipingWithNoHistoryChangesNothingAtAll`: `documentScrollGestures`,
//!   `jumpHistory`, `presentationSwipe`, `historySwipe`.

mod gesture_stand_in;
mod main_thread;
mod synthetic_scroll_events;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2_app_kit::NSEventModifierFlags;
use objc2_core_graphics::{CGMomentumScrollPhase, CGScrollPhase};
use upleft_app::app::history_swipe::{Claim, Direction, HistorySwipeCoordinator, HistorySwipePolicy, Host};
use upleft_app::app::presentation_swipe::PresentationSwipePolicy;
use upleft_render::appkit_compat::RectExt;

use gesture_stand_in::{DocumentStandIn, translation_x};
use synthetic_scroll_events::SyntheticScroll;

// MARK: - Fixtures

/// The suite's `scroll(…)`: `SyntheticScroll.event` with ⇧ held by default.
fn scroll() -> SyntheticScroll {
    SyntheticScroll::new().modifiers(NSEventModifierFlags::Shift)
}

const BEGAN: Option<CGScrollPhase> = Some(CGScrollPhase::Began);
const ENDED: Option<CGScrollPhase> = Some(CGScrollPhase::Ended);

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("main thread")
}

fn sized_controller(reduce_motion: bool) -> DocumentStandIn {
    DocumentStandIn::sized(reduce_motion, mtm())
}

/// A coordinator over real panes with the trip recorded rather than taken.
#[derive(Default)]
struct Recorder {
    moves: RefCell<Vec<Direction>>,
    haptics: Cell<usize>,
    can_go_back: Cell<bool>,
    can_go_forward: Cell<bool>,
}

impl Recorder {
    fn new() -> Rc<Recorder> {
        let recorder = Recorder::default();
        recorder.can_go_back.set(true);
        recorder.can_go_forward.set(true);
        Rc::new(recorder)
    }

    fn moves(&self) -> Vec<Direction> {
        self.moves.borrow().clone()
    }
}

fn swipe(controller: &DocumentStandIn, recorder: &Rc<Recorder>) -> Rc<HistorySwipeCoordinator> {
    let panes = controller.document_panes();
    let style_sheet = controller.active_style_sheet();
    let can_move = recorder.clone();
    let mover = recorder.clone();
    let haptic = recorder.clone();
    HistorySwipeCoordinator::new(Host {
        panes: Box::new(move || panes.clone()),
        style_sheet: Box::new(move || style_sheet.clone()),
        can_move: Box::new(move |direction| match direction {
            Direction::Back => can_move.can_go_back.get(),
            Direction::Forward => can_move.can_go_forward.get(),
        }),
        r#move: Box::new(move |direction| mover.moves.borrow_mut().push(direction)),
        perform_haptic_feedback: Box::new(move || haptic.haptics.set(haptic.haptics.get() + 1)),
    })
}

// MARK: - Policy

fn only_exactly_shift_spells_this_gesture() {
    assert!(HistorySwipePolicy::is_spelled_correctly(NSEventModifierFlags::Shift));
    // Nothing held is the Document↔Source swipe; ⇧⌘ is not a gesture at
    // all and must not half-start this one.
    assert!(!HistorySwipePolicy::is_spelled_correctly(NSEventModifierFlags::empty()));
    assert!(!HistorySwipePolicy::is_spelled_correctly(NSEventModifierFlags::Shift | NSEventModifierFlags::Command));
    assert!(!HistorySwipePolicy::is_spelled_correctly(NSEventModifierFlags::Command));
    assert!(!HistorySwipePolicy::is_spelled_correctly(NSEventModifierFlags::Option));
}

fn the_swipe_is_claimed_only_when_it_is_decidedly_sideways() {
    assert_eq!(HistorySwipePolicy::claim(6.0, 2.0), Claim::Undecided);
    assert_eq!(HistorySwipePolicy::claim(13.0, 0.0), Claim::Undecided);
    assert_eq!(HistorySwipePolicy::claim(14.0, 0.0), Claim::Swipe);
    assert_eq!(HistorySwipePolicy::claim(-40.0, 10.0), Claim::Swipe);
    // A diagonal is a scroll. This one is stricter than the presentation
    // swipe's, because ⇧ is held for plenty of reasons that are not this.
    assert_eq!(HistorySwipePolicy::claim(30.0, 25.0), Claim::Scroll);
    assert_eq!(HistorySwipePolicy::claim(0.0, -20.0), Claim::Scroll);
    assert_eq!(HistorySwipePolicy::claim(8.0, 9.0), Claim::Undecided);
}

fn the_content_follows_the_fingers_and_back_is_what_was_on_the_left() {
    // Safari's direction exactly: push the page right, uncovering what
    // sits to its left, and you go back.
    assert_eq!(HistorySwipePolicy::direction(40.0), Some(Direction::Back));
    assert_eq!(HistorySwipePolicy::direction(-40.0), Some(Direction::Forward));
    assert_eq!(HistorySwipePolicy::direction(0.0), None);
}

fn commit_distance_stays_reachable_at_every_pane_width() {
    assert_eq!(HistorySwipePolicy::commit_distance(120.0), HistorySwipePolicy::MINIMUM_COMMIT_DISTANCE);
    assert_eq!(HistorySwipePolicy::commit_distance(2400.0), HistorySwipePolicy::MAXIMUM_COMMIT_DISTANCE);
    assert_eq!(HistorySwipePolicy::commit_distance(400.0), 80.0);
    // A trip is cheaper to undo than a presentation switch, so it asks for
    // a little less travel.
    assert!(HistorySwipePolicy::commit_distance(900.0) < PresentationSwipePolicy::commit_distance(900.0));
}

fn a_short_swipe_commits_only_when_it_is_still_travelling() {
    let width = 900.0;
    let distance = HistorySwipePolicy::commit_distance(width);

    assert!(HistorySwipePolicy::should_commit(distance, 0.0, width));
    assert!(!HistorySwipePolicy::should_commit(distance - 1.0, 0.0, width));
    assert!(HistorySwipePolicy::should_commit(20.0, 600.0, width));
    // Pulled back at the last moment: however fast the hand was moving,
    // reversing means "put it back".
    assert!(!HistorySwipePolicy::should_commit(20.0, -600.0, width));
    assert!(!HistorySwipePolicy::should_commit(0.0, 900.0, width));
}

fn the_page_gives_against_the_fingers_and_never_arrives() {
    let limit = HistorySwipePolicy::MAXIMUM_GIVE;
    assert_eq!(HistorySwipePolicy::give(0.0), 0.0);
    assert!(HistorySwipePolicy::give(8.0) > 1.0);
    assert!(HistorySwipePolicy::give(-40.0) < 0.0);
    assert!(HistorySwipePolicy::give(40.0).abs() < HistorySwipePolicy::give(140.0).abs());
    assert!(HistorySwipePolicy::give(10_000.0).abs() < limit);
    assert!(HistorySwipePolicy::give(10_000.0).abs() > limit * 0.99);
}

// MARK: - Coordinator

fn swiping_right_goes_back_and_swiping_left_goes_forward() {
    let controller = sized_controller(true);
    let recorder = Recorder::new();
    let gesture = swipe(&controller, &recorder);

    assert!(!gesture.handle(&scroll().phase(BEGAN).at(0.0).event()));
    // Still ambiguous, so the scroll view keeps this one.
    assert!(!gesture.handle(&scroll().delta_x(8.0).at(0.008).event()));
    // Past the intent threshold: the swipe takes over.
    assert!(gesture.handle(&scroll().delta_x(10.0).at(0.016).event()));
    assert!(gesture.is_tracking());
    assert_eq!(gesture.tracked_direction_for_testing(), Some(Direction::Back));

    for step in 3..=12 {
        assert!(gesture.handle(&scroll().delta_x(24.0).at(step as f64 * 0.008).event()));
    }
    assert!(gesture.handle(&scroll().phase(ENDED).at(0.12).event()));
    assert_eq!(recorder.moves(), vec![Direction::Back]);
    assert!(!gesture.is_tracking());

    let _ = gesture.handle(&scroll().phase(BEGAN).at(1.0).event());
    for step in 1..=12 {
        assert!(gesture.handle(&scroll().delta_x(-24.0).at(1.0 + step as f64 * 0.008).event()));
    }
    assert!(gesture.handle(&scroll().phase(ENDED).at(1.12).event()));
    assert_eq!(recorder.moves(), vec![Direction::Back, Direction::Forward]);
}

fn without_shift_the_gesture_is_not_even_considered() {
    let controller = sized_controller(true);
    let recorder = Recorder::new();
    let gesture = swipe(&controller, &recorder);
    let bare = NSEventModifierFlags::empty();

    assert!(!gesture.handle(&scroll().phase(BEGAN).modifiers(bare).at(0.0).event()));
    for step in 1..=12 {
        let event = scroll().delta_x(24.0).modifiers(bare).at(step as f64 * 0.008).event();
        assert!(!gesture.handle(&event));
    }
    assert!(!gesture.handle(&scroll().phase(ENDED).modifiers(bare).at(0.12).event()));
    assert!(recorder.moves().is_empty());
    assert!(!gesture.is_tracking());
}

fn a_swipe_into_an_empty_stack_never_catches() {
    let controller = sized_controller(true);
    let recorder = Recorder::new();
    recorder.can_go_back.set(false);
    let gesture = swipe(&controller, &recorder);

    let _ = gesture.handle(&scroll().phase(BEGAN).at(0.0).event());
    // A page that gives and then does nothing reads as a bug, so the
    // gesture is handed straight back and the surface just scrolls.
    for step in 1..=12 {
        assert!(!gesture.handle(&scroll().delta_x(24.0).at(step as f64 * 0.008).event()));
    }
    assert!(!gesture.handle(&scroll().phase(ENDED).at(0.12).event()));
    assert!(recorder.moves().is_empty());
    assert!(!gesture.is_tracking());

    // …and the other way is still open, so the reader is not locked out of
    // the direction that does have somewhere to go.
    let _ = gesture.handle(&scroll().phase(BEGAN).at(1.0).event());
    for step in 1..=12 {
        assert!(gesture.handle(&scroll().delta_x(-24.0).at(1.0 + step as f64 * 0.008).event()));
    }
    assert!(gesture.handle(&scroll().phase(ENDED).at(1.12).event()));
    assert_eq!(recorder.moves(), vec![Direction::Forward]);
}

fn reading_with_shift_held_is_still_just_reading() {
    let controller = sized_controller(true);
    let recorder = Recorder::new();
    let gesture = swipe(&controller, &recorder);

    assert!(!gesture.handle(&scroll().phase(BEGAN).at(0.0).event()));
    for step in 1..=12 {
        let event = scroll().delta_x(-1.0).delta_y(-30.0).at(step as f64 * 0.008).event();
        assert!(!gesture.handle(&event));
    }
    assert!(!gesture.handle(&scroll().phase(ENDED).at(0.12).event()));
    assert!(recorder.moves().is_empty());
}

fn a_wheel_is_left_alone() {
    let controller = sized_controller(true);
    let recorder = Recorder::new();
    let gesture = swipe(&controller, &recorder);

    // ⇧-wheel is a single unphased notch: no travel to measure and no
    // release to commit on. Back and Forward stay on the keyboard there.
    for tick in 0..=10 {
        let event = scroll().delta_x(-10.0).phase(None).precise(false).at(tick as f64 * 0.03).event();
        assert!(!gesture.handle(&event));
    }
    assert!(recorder.moves().is_empty());
}

fn a_flick_commits_and_a_slow_nudge_does_not() {
    let controller = sized_controller(true);
    let recorder = Recorder::new();
    let gesture = swipe(&controller, &recorder);

    // Short, fast, still going: the flick.
    let _ = gesture.handle(&scroll().phase(BEGAN).at(0.0).event());
    assert!(gesture.handle(&scroll().delta_x(16.0).at(0.008).event()));
    assert!(gesture.handle(&scroll().delta_x(20.0).at(0.016).event()));
    assert!(gesture.handle(&scroll().phase(ENDED).at(0.02).event()));
    assert_eq!(recorder.moves(), vec![Direction::Back]);

    // The same distance, released slowly: the reader thought better of it.
    let _ = gesture.handle(&scroll().phase(BEGAN).at(1.0).event());
    assert!(gesture.handle(&scroll().delta_x(16.0).at(1.008).event()));
    assert!(gesture.handle(&scroll().delta_x(4.0).at(1.5).event()));
    assert!(gesture.handle(&scroll().phase(ENDED).at(2.0).event()));
    assert_eq!(recorder.moves(), vec![Direction::Back]);
}

fn the_coast_after_a_flick_is_swallowed_and_buys_nothing() {
    let controller = sized_controller(true);
    let recorder = Recorder::new();
    let gesture = swipe(&controller, &recorder);

    let _ = gesture.handle(&scroll().phase(BEGAN).at(0.0).event());
    assert!(gesture.handle(&scroll().delta_x(16.0).at(0.008).event()));
    assert!(gesture.handle(&scroll().delta_x(20.0).at(0.016).event()));
    assert!(gesture.handle(&scroll().phase(ENDED).at(0.02).event()));
    assert_eq!(recorder.moves(), vec![Direction::Back]);

    // Hundreds of points of coast, all of it belonging to a gesture that
    // has already been answered. It must not travel again, and it must not
    // fall through and scroll the document under the trip either.
    for tick in 1..=30 {
        let event = scroll()
            .delta_x(60.0)
            .phase(None)
            .momentum(CGMomentumScrollPhase::Continue)
            .at(0.02 + tick as f64 * 0.008)
            .event();
        assert!(gesture.handle(&event));
    }
    assert_eq!(recorder.moves(), vec![Direction::Back]);
    let tail = scroll().phase(None).momentum(CGMomentumScrollPhase::End).at(0.3).event();
    assert!(gesture.handle(&tail));
    // With the coast over, the next scroll belongs to the scroll view.
    assert!(!gesture.handle(&scroll().delta_y(-20.0).phase(None).momentum(CGMomentumScrollPhase::Continue).at(0.31).event()));
    assert_eq!(recorder.moves(), vec![Direction::Back]);
}

fn the_commit_point_is_a_detent_the_hand_can_feel_exactly_once_per_crossing() {
    let controller = sized_controller(true);
    let recorder = Recorder::new();
    let gesture = swipe(&controller, &recorder);
    let distance = HistorySwipePolicy::commit_distance(controller.primary_container.bounds().width());

    let _ = gesture.handle(&scroll().phase(BEGAN).at(0.0).event());
    assert!(gesture.handle(&scroll().delta_x(20.0).at(0.008).event()));
    assert_eq!(recorder.haptics.get(), 0);
    assert!(gesture.handle(&scroll().delta_x(distance).at(0.016).event()));
    assert_eq!(recorder.haptics.get(), 1);
    // Further out is still the same side of the line.
    assert!(gesture.handle(&scroll().delta_x(200.0).at(0.024).event()));
    assert_eq!(recorder.haptics.get(), 1);
    // Back under it re-arms, so hovering at the threshold feels like a
    // line rather than a burst.
    assert!(gesture.handle(&scroll().delta_x(-300.0).at(0.032).event()));
    assert_eq!(recorder.haptics.get(), 1);
    assert!(gesture.handle(&scroll().delta_x(300.0).at(0.04).event()));
    assert_eq!(recorder.haptics.get(), 2);
}

// MARK: - The give

fn the_page_travels_under_the_fingers_and_is_handed_back_before_the_trip() {
    let controller = sized_controller(false);
    let recorder = Recorder::new();
    let gesture = swipe(&controller, &recorder);
    let pane_subviews = controller.primary_container.subviews().count();

    let _ = gesture.handle(&scroll().phase(BEGAN).at(0.0).event());
    assert!(gesture.handle(&scroll().delta_x(40.0).at(0.008).event()));
    let scroll_layer = controller.primary_container.scroll_view().layer().expect("the scroll view's layer");
    assert!(scroll_layer.transform().m41 > 0.0);
    assert_eq!(controller.primary_container.layer().map(|layer| layer.masksToBounds()), Some(true));
    // Nothing was rendered to get here: the whole gesture so far is one
    // layer translation, and the destination is not visited until release.
    assert_eq!(controller.primary_container.subviews().count(), pane_subviews);
    assert!(recorder.moves().is_empty());

    for step in 2..=10 {
        assert!(gesture.handle(&scroll().delta_x(24.0).at(step as f64 * 0.008).event()));
    }
    assert!(gesture.handle(&scroll().phase(ENDED).at(0.1).event()));
    // The destination scroll animates from the pane's resting frame, so
    // the give is surrendered before the trip, not after.
    assert_eq!(scroll_layer.transform().m41, 0.0);
    assert_eq!(recorder.moves(), vec![Direction::Back]);
    assert!(!gesture.is_tracking());
    assert!(!gesture.is_settling());
}

fn reduce_motion_takes_the_trip_without_moving_the_page() {
    let controller = sized_controller(true);
    let recorder = Recorder::new();
    let gesture = swipe(&controller, &recorder);

    let _ = gesture.handle(&scroll().phase(BEGAN).at(0.0).event());
    assert!(gesture.handle(&scroll().delta_x(40.0).at(0.008).event()));
    assert!(gesture.is_tracking());
    // No give was taken, so there is no transform and no spring to run —
    // and the gesture still does its job.
    assert_eq!(translation_x(&controller.primary_container).unwrap_or(0.0), 0.0);
    for step in 2..=10 {
        assert!(gesture.handle(&scroll().delta_x(24.0).at(step as f64 * 0.008).event()));
    }
    assert!(gesture.handle(&scroll().phase(ENDED).at(0.1).event()));
    assert_eq!(recorder.moves(), vec![Direction::Back]);
    assert!(!gesture.is_settling());
}

fn main() {
    main_thread::run(&[
        ("only_exactly_shift_spells_this_gesture", only_exactly_shift_spells_this_gesture),
        ("the_swipe_is_claimed_only_when_it_is_decidedly_sideways", the_swipe_is_claimed_only_when_it_is_decidedly_sideways),
        (
            "the_content_follows_the_fingers_and_back_is_what_was_on_the_left",
            the_content_follows_the_fingers_and_back_is_what_was_on_the_left,
        ),
        ("commit_distance_stays_reachable_at_every_pane_width", commit_distance_stays_reachable_at_every_pane_width),
        ("a_short_swipe_commits_only_when_it_is_still_travelling", a_short_swipe_commits_only_when_it_is_still_travelling),
        ("the_page_gives_against_the_fingers_and_never_arrives", the_page_gives_against_the_fingers_and_never_arrives),
        ("swiping_right_goes_back_and_swiping_left_goes_forward", swiping_right_goes_back_and_swiping_left_goes_forward),
        ("without_shift_the_gesture_is_not_even_considered", without_shift_the_gesture_is_not_even_considered),
        ("a_swipe_into_an_empty_stack_never_catches", a_swipe_into_an_empty_stack_never_catches),
        ("reading_with_shift_held_is_still_just_reading", reading_with_shift_held_is_still_just_reading),
        ("a_wheel_is_left_alone", a_wheel_is_left_alone),
        ("a_flick_commits_and_a_slow_nudge_does_not", a_flick_commits_and_a_slow_nudge_does_not),
        ("the_coast_after_a_flick_is_swallowed_and_buys_nothing", the_coast_after_a_flick_is_swallowed_and_buys_nothing),
        (
            "the_commit_point_is_a_detent_the_hand_can_feel_exactly_once_per_crossing",
            the_commit_point_is_a_detent_the_hand_can_feel_exactly_once_per_crossing,
        ),
        (
            "the_page_travels_under_the_fingers_and_is_handed_back_before_the_trip",
            the_page_travels_under_the_fingers_and_is_handed_back_before_the_trip,
        ),
        ("reduce_motion_takes_the_trip_without_moving_the_page", reduce_motion_takes_the_trip_without_moving_the_page),
    ]);
}
