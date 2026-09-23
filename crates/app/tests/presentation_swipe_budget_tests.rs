//! Port of `Tests/DownrightAppTests/PresentationSwipeBudgetTests.swift`: the
//! two-finger swipe must cost nothing while it is happening.
//!
//! The Swift suite is `@MainActor`, and the panes are AppKit views, so this
//! binary owns the main thread (`harness = false`, `tests/main_thread`).
//!
//! The Swift test opens a 2000-entry corpus in a `DocumentWindowController`
//! and times `controller.presentationSwipe`. Until the window controller is
//! ported it runs over `gesture_stand_in::DocumentStandIn`: the same text in
//! a real `MarkdownContainerView` (in a borderless window that is never
//! ordered in, laid out and `prepareForDisplay`ed), `documentLineCount` from
//! the parse's `lineStarts`, and the controller's host wiring for the swipe,
//! except that a presentation change records the segment and the rail
//! callbacks do nothing. So it times this port's coordinator, give and drag
//! (the budget sends this document to the give), not the window controller's
//! presentation switch. The temporary file the Swift writes only feeds
//! `open(_:mode:)`, so the stand-in takes the text directly.
//!
//! As in Swift, the style sheet is not overridden: the machine's Reduce Motion
//! setting decides whether the give is taken at all.
//!
//! One difference in the harness, not the assertions: the events are built
//! before each timed region rather than inside it. `CGEventCreateScrollWheelEvent2`
//! asks the window server for the cursor position, so on a loaded machine
//! building one event took 0.1–5 ms while handling it took ~4 µs (measured
//! 2026-09-23), and the budget measured the window server instead of the
//! gesture. A trackpad's events arrive already built.
//!
//! Nothing is skipped.

mod gesture_stand_in;
mod main_thread;

use std::time::Instant;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSEvent;
use objc2_core_foundation::CGFloat;
use objc2_core_graphics::{CGEvent, CGEventField, CGScrollEventUnit, CGScrollPhase};

use gesture_stand_in::DocumentStandIn;

/// Generous against the measured cost — engagement and abandonment are
/// hundredths of a millisecond in release, and a frame is 8 ms even at
/// 120 Hz with room to spare. A budget this loose still fails instantly if
/// anything starts rendering inside the gesture again.
const ENGAGEMENT_BUDGET: f64 = 8.0;
const TRACKING_FRAME_BUDGET: f64 = 1.0;

fn corpus(lines: usize) -> String {
    let mut out = String::from("# Renderer handoff\n\n");
    for index in 0..lines {
        match index % 8 {
            0 => out += &format!("## Section {index}\n\n"),
            1 => out += &format!(
                "Body text with `inline code`, **bold**, *italic* and a [link](https://example.com) in line {index}.\n\n"
            ),
            2 => out += &format!("- list item {index} with trailing prose to make the line wrap at a realistic measure\n"),
            3 => out += &format!("- [ ] task item {index}\n\n"),
            4 => out += &format!("```swift\nlet value{index} = compute({index})\n```\n\n"),
            5 => out += &format!("> quoted line {index}\n\n"),
            6 => out += &format!("| a | b |\n|---|---|\n| {index} | {} |\n\n", index * 2),
            _ => out += &format!("Plain paragraph {index}.\n\n"),
        }
    }
    out
}

/// `DispatchTime.now().uptimeNanoseconds` around `body`, in milliseconds.
fn elapsed(body: impl FnOnce()) -> f64 {
    let start = Instant::now();
    body();
    start.elapsed().as_nanos() as f64 / 1_000_000.0
}

/// The suite's private `scroll(_:phase:at:)`: continuous, horizontal only,
/// no modifier flags set.
fn scroll(delta_x: CGFloat, phase: CGScrollPhase, seconds: f64) -> Retained<NSEvent> {
    let raw = CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Pixel, 2, 0, 0, 0).expect("CGEvent");
    let event = Some(&*raw);
    CGEvent::set_integer_value_field(event, CGEventField::ScrollWheelEventIsContinuous, 1);
    CGEvent::set_double_value_field(event, CGEventField::ScrollWheelEventPointDeltaAxis2, delta_x);
    CGEvent::set_integer_value_field(event, CGEventField::ScrollWheelEventScrollPhase, phase.0 as i64);
    CGEvent::set_timestamp(event, (seconds * 1_000_000_000.0) as u64);
    NSEvent::eventWithCGEvent(&raw).expect("NSEvent")
}

fn swiping_and_abandoning_a_large_document_renders_nothing() {
    let lines = 2_000;
    let controller = DocumentStandIn::new(&corpus(lines), 1020.0, 728.0, None, MainThreadMarker::new().expect("main thread"));
    controller.primary_container.text_view().prepare_for_display();

    let swipe = controller.presentation_swipe();
    let _ = swipe.handle(&scroll(0.0, CGScrollPhase::Began, 0.0));

    // Engagement: the frame the gesture is claimed on. This is where the
    // rejected design spent half a second.
    let engaging = scroll(-20.0, CGScrollPhase::Changed, 0.008);
    let engage = elapsed(|| {
        let _ = swipe.handle(&engaging);
    });
    assert!(swipe.is_tracking());

    let frames = 100;
    let tracked: Vec<Retained<NSEvent>> =
        (2..=(frames + 1)).map(|step| scroll(-0.2, CGScrollPhase::Changed, step as f64 * 0.008)).collect();
    let tracking = elapsed(|| {
        for event in &tracked {
            let _ = swipe.handle(event);
        }
    });

    // Released short and slow: the reader thought better of it.
    let abandoning = scroll(0.0, CGScrollPhase::Ended, 4.0);
    let abandon = elapsed(|| {
        let _ = swipe.handle(&abandoning);
    });
    swipe.cancel_in_flight();

    println!("engage {engage:.3} ms, tracking {:.4} ms per frame, abandon {abandon:.3} ms", tracking / frames as f64);
    assert!(engage < ENGAGEMENT_BUDGET, "engaging the swipe took {engage} ms — something is rendering inside the gesture");
    assert!(
        tracking / (frames as f64) < TRACKING_FRAME_BUDGET,
        "tracking cost {} ms per frame",
        tracking / frames as f64
    );
    assert!(abandon < ENGAGEMENT_BUDGET, "abandoning the swipe took {abandon} ms — an abandoned swipe must undo nothing");
    // The whole point: an abandoned swipe leaves the document exactly as
    // it found it, having built nothing to throw away.
    assert_eq!(controller.presentation_segment(), 0);
}

fn main() {
    main_thread::run(&[(
        "swiping_and_abandoning_a_large_document_renders_nothing",
        swiping_and_abandoning_a_large_document_renders_nothing,
    )]);
}
