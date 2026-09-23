//! Port of `Tests/DownrightAppTests/ScrollGestureChainTests.swift`.
//!
//! The Swift suite is `@MainActor`, so this binary owns the main thread
//! (`harness = false`, `tests/main_thread`). Nothing is skipped.

mod main_thread;
mod synthetic_scroll_events;

use std::cell::Cell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2_app_kit::NSEvent;
use upleft_app::app::document_scroll_gestures::{ScrollGestureChain, ScrollGestureHandler};

use synthetic_scroll_events::SyntheticScroll;

#[derive(Default)]
struct Stub {
    claims: Cell<bool>,
    owns: Cell<bool>,
    seen: Cell<usize>,
}

impl ScrollGestureHandler for Stub {
    fn handle(&self, _event: &NSEvent) -> bool {
        self.seen.set(self.seen.get() + 1);
        self.claims.get()
    }

    fn is_claiming_gesture(&self) -> bool {
        self.owns.get()
    }
}

fn stub() -> Rc<Stub> {
    Rc::new(Stub::default())
}

fn chain(handlers: &[&Rc<Stub>]) -> ScrollGestureChain {
    ScrollGestureChain::new(handlers.iter().map(|&handler| handler.clone() as Rc<dyn ScrollGestureHandler>).collect())
}

fn event() -> Retained<NSEvent> {
    SyntheticScroll::new().delta_y(-12.0).event()
}

fn the_first_handler_to_claim_ends_the_chain() {
    let first = stub();
    let second = stub();
    let third = stub();
    second.claims.set(true);
    let chain = chain(&[&first, &second, &third]);

    assert!(chain.handle(&event()));
    assert_eq!(first.seen.get(), 1);
    assert_eq!(second.seen.get(), 1);
    // Two gestures acting on one event is a page that scrolls and zooms at
    // the same time; the one behind the winner never hears about it.
    assert_eq!(third.seen.get(), 0);
}

fn handlers_that_decline_still_see_every_event() {
    let first = stub();
    let second = stub();
    let chain = chain(&[&first, &second]);

    // Both swipes spend the first few events deciding, and they can only
    // decide by accumulating travel — so declining must not cost them the
    // events they are declining.
    assert!(!chain.handle(&event()));
    assert!(!chain.handle(&event()));
    assert_eq!(first.seen.get(), 2);
    assert_eq!(second.seen.get(), 2);
}

fn a_gesture_that_has_already_caught_keeps_the_rest_of_it() {
    let first = stub();
    let second = stub();
    first.claims.set(true);
    second.owns.set(true);
    let chain = chain(&[&first, &second]);

    // The swipe on screen owns the fingers. Polling from the top here is
    // how a modifier pressed halfway through a swipe would hand the events
    // to a zoom and leave a translated pane nobody owns.
    assert!(!chain.handle(&event()));
    assert_eq!(first.seen.get(), 0);
    assert_eq!(second.seen.get(), 1);

    second.owns.set(false);
    assert!(chain.handle(&event()));
    assert_eq!(first.seen.get(), 1);
    assert_eq!(second.seen.get(), 1);
}

fn an_event_nobody_wants_goes_straight_back_to_the_scroll_view() {
    let chain = chain(&[&stub(), &stub()]);
    assert!(!chain.handle(&event()));
}

fn main() {
    main_thread::run(&[
        ("the_first_handler_to_claim_ends_the_chain", the_first_handler_to_claim_ends_the_chain),
        ("handlers_that_decline_still_see_every_event", handlers_that_decline_still_see_every_event),
        ("a_gesture_that_has_already_caught_keeps_the_rest_of_it", a_gesture_that_has_already_caught_keeps_the_rest_of_it),
        (
            "an_event_nobody_wants_goes_straight_back_to_the_scroll_view",
            an_event_nobody_wants_goes_straight_back_to_the_scroll_view,
        ),
    ]);
}
