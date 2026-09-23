//! Port of `Tests/DownrightAppTests/PresentationDragTests.swift`: the live
//! Document↔Source drag, and the budget that decides whether the reader gets
//! it.
//!
//! The Swift suite is `@MainActor`, and the panes are AppKit views, so this
//! binary owns the main thread (`harness = false`, `tests/main_thread`).
//!
//! The "Choosing" and "Finishing" cases open a document of a known length in a
//! `DocumentWindowController` and drive `controller.presentationSwipe`. Until
//! the window controller is ported they run over
//! `gesture_stand_in::DocumentStandIn`: the same text in a real
//! `MarkdownContainerView` (in a borderless window that is never ordered in),
//! `documentLineCount` from the parse's `lineStarts`, and the controller's
//! host wiring for the swipe, except that a presentation change records the
//! segment instead of rebuilding the text view in Source mode. The file the
//! Swift writes to a temporary folder only feeds `open(_:mode:)`, so the
//! stand-in takes the text directly.
//!
//! Skipped, with the reason:
//! - `aResizeMidDragGroundsItAndRestoresTheMode`: needs
//!   `DocumentWindowController.windowDidResize(_:)`, which is being ported
//!   elsewhere.

mod gesture_stand_in;
mod main_thread;
mod synthetic_scroll_events;

use objc2::MainThreadMarker;
use objc2_core_foundation::CGFloat;
use objc2_core_graphics::CGScrollPhase;
use upleft_app::app::presentation_drag::PresentationSwitchBudget;

use gesture_stand_in::{DocumentStandIn, ResetCalibration, translation_x};
use synthetic_scroll_events::SyntheticScroll;

fn corpus(lines: usize) -> String {
    let mut out = String::from("# Drag corpus\n\n");
    for index in 0..lines {
        if index % 4 == 0 {
            out += &format!("## Section {index}\n\n");
        } else {
            out += &format!("Paragraph {index} of prose.\n\n");
        }
    }
    out
}

/// A real document of a known length, since the budget's whole input is how
/// long the document is.
fn controller(lines: usize, reduce_motion: bool) -> DocumentStandIn {
    DocumentStandIn::new(&corpus(lines), 1020.0, 728.0, Some(reduce_motion), MainThreadMarker::new().expect("main thread"))
}

fn swipe_left(controller: &DocumentStandIn, steps: usize, per_step: CGFloat) {
    let swipe = controller.presentation_swipe();
    let _ = swipe.handle(&SyntheticScroll::new().phase(Some(CGScrollPhase::Began)).at(0.0).event());
    for step in 1..=steps {
        let _ = swipe.handle(&SyntheticScroll::new().delta_x(per_step).at(step as f64 * 0.008).event());
    }
}

// MARK: - Budget

fn budget_grants_the_drag_only_while_the_switch_fits_inside_a_few_frames() {
    PresentationSwitchBudget::reset_calibration_for_testing();
    let _reset = ResetCalibration;

    assert!(PresentationSwitchBudget::allows_drag(100));
    assert!(PresentationSwitchBudget::allows_drag(1_000));
    // Measured: past roughly twelve hundred lines of file, rendering
    // Source stops fitting inside the engagement budget.
    assert!(!PresentationSwitchBudget::allows_drag(2_000));
    assert!(!PresentationSwitchBudget::allows_drag(9_000));
    assert_eq!(PresentationSwitchBudget::estimated_cost(0), PresentationSwitchBudget::FIXED_COST);
}

fn budget_recalibrates_from_what_the_switch_actually_cost() {
    PresentationSwitchBudget::reset_calibration_for_testing();
    let _reset = ResetCalibration;
    let before = PresentationSwitchBudget::milliseconds_per_line();

    // A machine four times slower than the seeded constant.
    for _ in 0..12 {
        PresentationSwitchBudget::record(0.272 * 1_000.0, 1_000);
    }
    assert!(PresentationSwitchBudget::milliseconds_per_line() > before * 2.0);
    // …and now refuses documents it would previously have accepted.
    assert!(!PresentationSwitchBudget::allows_drag(1_000));

    // Nonsense never moves the estimate: a short document's cost is mostly
    // fixed overhead, so dividing it by the line count describes nothing.
    let calibrated = PresentationSwitchBudget::milliseconds_per_line();
    PresentationSwitchBudget::record(9_999.0, 10);
    PresentationSwitchBudget::record(-5.0, 5_000);
    PresentationSwitchBudget::record(f64::INFINITY, 5_000);
    assert_eq!(PresentationSwitchBudget::milliseconds_per_line(), calibrated);
}

// MARK: - Choosing

fn a_short_document_gets_the_real_drag_and_switches_behind_it() {
    PresentationSwitchBudget::reset_calibration_for_testing();
    let _reset = ResetCalibration;
    let controller = controller(120, false);

    swipe_left(&controller, 10, -24.0);
    assert!(controller.presentation_swipe().is_tracking());
    // The drag renders what it is pulling in, so by the time the fingers
    // have moved the live surface is already Source — hidden behind a
    // still of the page being left.
    assert_eq!(controller.presentation_segment(), 1);
    let scroll_layer = controller.primary_container.scroll_view().layer().expect("the scroll view's layer");
    assert!(scroll_layer.transform().m41 != 0.0);

    // `defer { controller.presentationSwipe.cancelInFlight(); … }`.
    controller.presentation_swipe().cancel_in_flight();
}

fn a_long_document_gets_the_give_and_is_not_rebuilt_mid_gesture() {
    PresentationSwitchBudget::reset_calibration_for_testing();
    let _reset = ResetCalibration;
    let controller = controller(3_000, false);

    swipe_left(&controller, 10, -24.0);
    assert!(controller.presentation_swipe().is_tracking());
    // Nothing was rendered: the give promises less precisely so that it can
    // always answer.
    assert_eq!(controller.presentation_segment(), 0);

    controller.presentation_swipe().cancel_in_flight();
}

fn reduce_motion_takes_neither_and_switches_on_release() {
    PresentationSwitchBudget::reset_calibration_for_testing();
    let _reset = ResetCalibration;
    let controller = controller(120, true);

    swipe_left(&controller, 10, -24.0);
    assert_eq!(controller.presentation_segment(), 0);
    let _ = controller
        .presentation_swipe()
        .handle(&SyntheticScroll::new().phase(Some(CGScrollPhase::Ended)).at(0.2).event());
    assert_eq!(controller.presentation_segment(), 1);
}

// MARK: - Finishing

fn abandoning_a_drag_puts_the_mode_back() {
    PresentationSwitchBudget::reset_calibration_for_testing();
    let _reset = ResetCalibration;
    let controller = controller(120, false);

    // Far enough to engage, nowhere near far enough to commit.
    let swipe = controller.presentation_swipe();
    let _ = swipe.handle(&SyntheticScroll::new().phase(Some(CGScrollPhase::Began)).at(0.0).event());
    let _ = swipe.handle(&SyntheticScroll::new().delta_x(-16.0).at(0.008).event());
    assert_eq!(controller.presentation_segment(), 1);

    swipe.cancel_in_flight();
    assert_eq!(controller.presentation_segment(), 0);
    assert_eq!(translation_x(&controller.primary_container), Some(0.0));
    assert!(!swipe.is_tracking());
}

fn swiping_toward_the_mode_you_are_in_never_engages_either_track() {
    PresentationSwitchBudget::reset_calibration_for_testing();
    let _reset = ResetCalibration;
    let controller = controller(120, false);

    let swipe = controller.presentation_swipe();
    let _ = swipe.handle(&SyntheticScroll::new().phase(Some(CGScrollPhase::Began)).at(0.0).event());
    assert!(!swipe.handle(&SyntheticScroll::new().delta_x(40.0).at(0.008).event()));
    assert!(!swipe.is_tracking());
    assert_eq!(controller.presentation_segment(), 0);
}

fn main() {
    main_thread::run(&[
        (
            "budget_grants_the_drag_only_while_the_switch_fits_inside_a_few_frames",
            budget_grants_the_drag_only_while_the_switch_fits_inside_a_few_frames,
        ),
        ("budget_recalibrates_from_what_the_switch_actually_cost", budget_recalibrates_from_what_the_switch_actually_cost),
        (
            "a_short_document_gets_the_real_drag_and_switches_behind_it",
            a_short_document_gets_the_real_drag_and_switches_behind_it,
        ),
        (
            "a_long_document_gets_the_give_and_is_not_rebuilt_mid_gesture",
            a_long_document_gets_the_give_and_is_not_rebuilt_mid_gesture,
        ),
        ("reduce_motion_takes_neither_and_switches_on_release", reduce_motion_takes_neither_and_switches_on_release),
        ("abandoning_a_drag_puts_the_mode_back", abandoning_a_drag_puts_the_mode_back),
        (
            "swiping_toward_the_mode_you_are_in_never_engages_either_track",
            swiping_toward_the_mode_you_are_in_never_engages_either_track,
        ),
    ]);
}
