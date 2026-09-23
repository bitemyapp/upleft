//! Port of `Tests/DownrightAppTests/AsyncParseTests.swift`, plus coordinator
//! tests that pin the actor's latest-wins contract directly (additions, at
//! the end).
//!
//! The Swift suite is `@MainActor`, so this binary owns the main thread
//! (`harness = false`, see `main_thread`). Where the Swift test awaits
//! (`Task.yield()`, a gate, a signal), the port pumps the main run loop, which
//! is what lets the document's main-queue hops run. `Task.yield()` is a 5 ms
//! main-loop turn here: the parse lane is real background work, so a yield
//! that lasts no time at all would only test the scheduler.

mod document_support;
mod main_thread;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use document_support::{document, document_with_worker, whole};
use upleft_app::ai::markdown_document::Phase;
use upleft_app::ai::markdown_parse_worker::*;
use upleft_core::contracts::{DirtySet, TextEdit};
use upleft_core::text_diff::TextDiff;
use upleft_swift_text::NSRange;
use upleft_core::ast_diff::ASTDiff;
use upleft_core::model::ParsedDocument;
use upleft_core::parser::MarkdownParser;

fn injected_worker_runs_pure_parse_and_diff() {
    let worker = MarkdownParseWorker::with_operation(|text, previous, revision| {
        let document = MarkdownParser::parse(&text);
        let dirty = ASTDiff::dirty_set(Some(&previous), &document);
        MarkdownParseResult { revision, text, document, dirty }
    });
    let previous = MarkdownParser::parse("# One\n");
    let result = worker.run("# Two\n".into(), previous, MarkdownParseRevision::ZERO.advanced());

    assert_eq!(result.document.text, "# Two\n");
    assert_eq!(result.revision, MarkdownParseRevision::ZERO.advanced());
    assert!(!result.dirty.is_empty());
}

// MARK: - Additions: the coordinator on its own

/// `ParseGate`: holds each parse until released, counting requests.
#[derive(Default)]
struct ParseGate {
    state: Mutex<GateState>,
    changed: Condvar,
}

#[derive(Default)]
struct GateState {
    requests: usize,
    released: usize,
    active: usize,
    maximum_active: usize,
}

impl ParseGate {
    fn hold(&self, result: MarkdownParseResult) -> MarkdownParseResult {
        let mut state = self.state.lock().unwrap();
        state.requests += 1;
        state.active += 1;
        state.maximum_active = state.maximum_active.max(state.active);
        let ticket = state.requests;
        self.changed.notify_all();
        while state.released < ticket {
            state = self.changed.wait(state).unwrap();
        }
        state.active -= 1;
        result
    }

    fn wait_for_request_count(&self, count: usize) {
        let state = self.state.lock().unwrap();
        let (state, timeout) =
            self.changed.wait_timeout_while(state, Duration::from_secs(5), |state| state.requests < count).unwrap();
        assert!(!timeout.timed_out(), "only {} parse requests arrived", state.requests);
    }

    fn release_next(&self) {
        self.state.lock().unwrap().released += 1;
        self.changed.notify_all();
    }

    fn request_count(&self) -> usize {
        self.state.lock().unwrap().requests
    }

    fn maximum_active(&self) -> usize {
        self.state.lock().unwrap().maximum_active
    }
}

fn gated_worker(gate: &Arc<ParseGate>) -> MarkdownParseWorker {
    let gate = gate.clone();
    MarkdownParseWorker::with_operation(move |text, previous, revision| {
        let parsed = MarkdownParser::parse(&text);
        let dirty = ASTDiff::dirty_set(Some(&previous), &parsed);
        gate.hold(MarkdownParseResult { revision, text, document: parsed, dirty })
    })
}

fn request(text: &str, revision: u64) -> MarkdownParseRequest {
    MarkdownParseRequest { text: text.into(), previous: ParsedDocument::empty(), revision: MarkdownParseRevision::new(revision) }
}

/// The document's parse loop: `while let result = await coordinator.nextResult()`.
fn start_loop(coordinator: &MarkdownParseCoordinator, results: Arc<Mutex<Vec<Option<String>>>>) {
    let next = coordinator.clone();
    coordinator.next_result(move |result| {
        let finished = result.is_none();
        results.lock().unwrap().push(result.map(|result| result.text));
        if !finished {
            start_loop(&next, results);
        }
    });
}

fn settle_queues() {
    main_thread::sleep_pumping(Duration::from_millis(50));
}

fn coordinator_keeps_one_parse_in_flight_and_only_the_newest_pending_snapshot() {
    let gate = Arc::new(ParseGate::default());
    let coordinator = MarkdownParseCoordinator::new(gated_worker(&gate));
    let results = Arc::new(Mutex::new(Vec::new()));
    start_loop(&coordinator, results.clone());

    coordinator.submit(request("one\n", 1));
    gate.wait_for_request_count(1);
    for (text, revision) in [("two\n", 2), ("three\n", 3), ("four\n", 4)] {
        coordinator.submit(request(text, revision));
    }
    // An older revision never replaces a newer pending one.
    coordinator.submit(request("stale\n", 3));
    settle_queues();
    assert_eq!(gate.request_count(), 1, "the pending snapshot must not start while a parse is held");

    gate.release_next();
    gate.wait_for_request_count(2);
    gate.release_next();
    assert!(main_thread::pump_until(|| results.lock().unwrap().len() == 2, Duration::from_secs(5)));
    assert_eq!(*results.lock().unwrap(), vec![Some("one\n".to_owned()), Some("four\n".to_owned())]);
    assert_eq!(gate.maximum_active(), 1);
    assert_eq!(gate.request_count(), 2);
    coordinator.shutdown();
}

fn busy_state_is_published_on_transitions_only() {
    let gate = Arc::new(ParseGate::default());
    let coordinator = MarkdownParseCoordinator::new(gated_worker(&gate));
    let transitions = Arc::new(Mutex::new(Vec::new()));
    let sink = transitions.clone();
    coordinator.set_on_busy_change(move |busy| sink.lock().unwrap().push(busy));
    let results = Arc::new(Mutex::new(Vec::new()));

    coordinator.submit(request("one\n", 1));
    coordinator.submit(request("two\n", 2));
    start_loop(&coordinator, results.clone());
    gate.wait_for_request_count(1);
    gate.release_next();
    assert!(main_thread::pump_until(|| results.lock().unwrap().len() == 1, Duration::from_secs(5)));
    settle_queues();
    assert_eq!(*transitions.lock().unwrap(), vec![true, false]);
    assert_eq!(*results.lock().unwrap(), vec![Some("two\n".to_owned())]);
    coordinator.shutdown();
}

fn suspend_drops_the_pending_snapshot_and_refuses_new_ones() {
    let gate = Arc::new(ParseGate::default());
    let coordinator = MarkdownParseCoordinator::new(gated_worker(&gate));
    let results = Arc::new(Mutex::new(Vec::new()));

    coordinator.submit(request("one\n", 1));
    coordinator.suspend();
    coordinator.submit(request("two\n", 2));
    start_loop(&coordinator, results.clone());
    settle_queues();
    assert_eq!(gate.request_count(), 0, "a suspended coordinator starts no parse");

    // `runImmediately` on a suspended coordinator returns the placeholder.
    let placeholder = Arc::new(Mutex::new(None));
    let sink = placeholder.clone();
    coordinator.run_immediately(request("three\n", 3), move |result| {
        *sink.lock().unwrap() = Some(result);
    });
    assert!(main_thread::pump_until(|| placeholder.lock().unwrap().is_some(), Duration::from_secs(5)));
    let placeholder = placeholder.lock().unwrap().take().unwrap();
    assert_eq!(placeholder.text, "three\n");
    assert_eq!(placeholder.document.text, "");
    assert!(!placeholder.dirty.is_wholesale && placeholder.dirty.ranges.is_empty());

    coordinator.resume();
    coordinator.submit(request("four\n", 4));
    gate.wait_for_request_count(1);
    gate.release_next();
    assert!(main_thread::pump_until(|| results.lock().unwrap().len() == 1, Duration::from_secs(5)));
    assert_eq!(*results.lock().unwrap(), vec![Some("four\n".to_owned())]);

    coordinator.shutdown();
    assert!(main_thread::pump_until(|| results.lock().unwrap().len() == 2, Duration::from_secs(5)));
    assert_eq!(results.lock().unwrap()[1], None, "shutdown ends the loop");
}

fn run_immediately_overlaps_a_held_parse() {
    let gate = Arc::new(ParseGate::default());
    let coordinator = MarkdownParseCoordinator::new(gated_worker(&gate));
    let results = Arc::new(Mutex::new(Vec::new()));
    start_loop(&coordinator, results.clone());

    coordinator.submit(request("stale\n", 1));
    gate.wait_for_request_count(1);
    let immediate = Arc::new(Mutex::new(None));
    let sink = immediate.clone();
    coordinator.run_immediately(request("fresh\n", 2), move |result| {
        *sink.lock().unwrap() = Some(result.text);
    });
    gate.wait_for_request_count(2);
    gate.release_next();
    gate.release_next();
    assert!(main_thread::pump_until(|| immediate.lock().unwrap().is_some(), Duration::from_secs(5)));
    assert_eq!(immediate.lock().unwrap().as_deref(), Some("fresh\n"));
    assert_eq!(gate.maximum_active(), 2, "the priority parse does not wait behind the stale one");
    coordinator.shutdown();
}

fn default_worker_parses_wholesale_against_an_empty_tree() {
    let worker = MarkdownParseWorker::default();
    let first = worker.run("# One\n\nBody.\n".into(), ParsedDocument::empty(), MarkdownParseRevision::new(7));
    assert!(first.dirty.is_wholesale);
    assert_eq!(first.revision.raw_value, 7);
    let second = worker.run("# One\n\nBody, edited.\n".into(), first.document.clone(), MarkdownParseRevision::new(8));
    assert!(!second.dirty.is_wholesale);
    assert!(!second.dirty.ranges.is_empty());
    assert_eq!(MarkdownParseRevision::new(u64::MAX).advanced(), MarkdownParseRevision::ZERO);
}

// MARK: - MarkdownDocument tests

/// `await Task.yield()` from the main actor.
fn yield_main() {
    main_thread::sleep_pumping(Duration::from_millis(5));
}

impl ParseGate {
    /// `await gate.waitForRequestCount(count)` from the main actor: the main
    /// actor keeps running while the test waits.
    fn wait_for_request_count_pumping(&self, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.request_count() < count {
            assert!(Instant::now() < deadline, "only {} parse requests arrived", self.request_count());
            main_thread::run_loop_once(0.005);
        }
    }
}

/// `ParseSignal`: `signal()` from `onReparse`, `wait()` on the main actor.
#[derive(Clone, Default)]
struct ParseSignal(Rc<Cell<bool>>);

impl ParseSignal {
    fn signal(&self) {
        self.0.set(true);
    }

    fn wait(&self) {
        assert!(main_thread::pump_until(|| self.0.get(), Duration::from_secs(5)), "the signal never came");
    }
}

fn external_absorb_publishes_bounded_incremental_render() {
    let document = document();
    let initial = "# One\n\nA calm paragraph.\n";
    let incoming = "# One\n\nA rewritten paragraph with a different shape.\n";
    document.adopt(initial, None);

    let observed_dirty: Rc<RefCell<Option<DirtySet>>> = Rc::default();
    let sink = observed_dirty.clone();
    document.set_on_reparse(Some(move |_: &Arc<ParsedDocument>, dirty: &DirtySet| {
        *sink.borrow_mut() = Some(dirty.clone());
    }));
    document.apply_external_text(incoming, &TextDiff::hunks(initial, incoming));

    assert_eq!(document.text(), incoming);
    for _ in 0..100 {
        if observed_dirty.borrow().is_some() {
            break;
        }
        main_thread::sleep_pumping(Duration::from_millis(5));
    }
    let dirty = observed_dirty.borrow().clone();
    assert_eq!(dirty.as_ref().map(|dirty| dirty.is_wholesale), Some(false));
    assert_eq!(dirty.as_ref().map(|dirty| dirty.ranges.is_empty()), Some(false));
}

fn external_absorb_undo_redo_does_not_consume_next_local_edit() {
    let document = document();
    let original = "# One\n\nOriginal body.\n";
    let incoming = "# One\n\nExternal body.\n";
    document.adopt(original, None);

    document.apply_external_text(incoming, &TextDiff::hunks(original, incoming));
    for _ in 0..100 {
        if document.parsed().text == incoming {
            break;
        }
        main_thread::sleep_pumping(Duration::from_millis(2));
    }
    assert_eq!(document.parsed().text, incoming);

    document.undo_manager().undo();
    assert_eq!(document.text(), original);
    assert_eq!(document.parsed().text, original);

    document.undo_manager().redo();
    assert_eq!(document.text(), incoming);
    assert_eq!(document.parsed().text, incoming);

    // Drain the delegate callbacks from absorb, undo, and redo. The next
    // edit must still be treated as a user mutation after all of them.
    for _ in 0..20 {
        yield_main();
    }
    let reparse_count = Rc::new(Cell::new(0));
    let counter = reparse_count.clone();
    let weak = objc2::rc::Weak::from_retained(&document);
    document.set_on_reparse(Some(move |parsed: &Arc<ParsedDocument>, _: &DirtySet| {
        if let Some(document) = weak.load()
            && parsed.text == document.text()
        {
            counter.set(counter.get() + 1);
        }
    }));
    let end = document.storage().length() as isize;
    assert!(document.replace(NSRange::new(end, 0), "Local tail.\n", Some("Paste")));
    assert!(document.is_dirty());
    assert_eq!(document.presentation_state().phase, Phase::Edited);

    for _ in 0..100 {
        if document.parsed().text == document.text() {
            break;
        }
        main_thread::sleep_pumping(Duration::from_millis(2));
    }
    assert_eq!(document.parsed().text, document.text());
    assert_eq!(reparse_count.get(), 1);
}

fn semantic_edit_converges_before_reading_tree() {
    let document = document();
    document.adopt("# Heading\n", None);

    // The source edit leaves the old tree in place until the worker result
    // commits.  A semantic command must synchronously converge first.
    assert!(document.replace(whole(&document), "- [ ] task\n", None));
    document.toggle_task(2);

    assert_eq!(document.text(), "- [x] task\n");
    assert_eq!(document.parsed().text, document.text());
}

fn undo_and_redo_lock_viewport_and_reparse_before_returning() {
    let document = document();
    document.adopt("one\n", None);
    assert!(document.replace(whole(&document), "two lines\nsecond\n", Some("Expand")));
    document.ensure_parsed_current();

    let viewport_locks = Rc::new(Cell::new(0));
    let reparses_observed_after_lock = Rc::new(Cell::new(0));
    let text_seen_at_lock: Rc<RefCell<Vec<String>>> = Rc::default();
    let weak = objc2::rc::Weak::from_retained(&document);
    let (locks, seen) = (viewport_locks.clone(), text_seen_at_lock.clone());
    document.set_on_will_apply_undo_redo(Some(move || {
        locks.set(locks.get() + 1);
        if let Some(document) = weak.load() {
            seen.borrow_mut().push(document.text());
        }
    }));
    let (locks, observed) = (viewport_locks.clone(), reparses_observed_after_lock.clone());
    document.set_on_reparse(Some(move |_: &Arc<ParsedDocument>, _: &DirtySet| {
        if locks.get() > observed.get() {
            observed.set(observed.get() + 1);
        }
    }));

    document.undo_manager().undo();
    assert_eq!(document.text(), "one\n");
    assert_eq!(document.parsed().text, document.text());

    document.undo_manager().redo();
    assert_eq!(document.text(), "two lines\nsecond\n");
    assert_eq!(document.parsed().text, document.text());
    assert_eq!(viewport_locks.get(), 2);
    assert_eq!(reparses_observed_after_lock.get(), 2);
    assert_eq!(*text_seen_at_lock.borrow(), vec!["two lines\nsecond\n".to_owned(), "one\n".to_owned()]);
}

fn grouped_undo_locks_once_for_several_inverse_edits() {
    let document = document();
    let source = "alpha\nbeta\n";
    document.adopt(source, None);
    let alpha = NSRange::new(0, 5);
    let beta = NSRange::new(6, 4);
    document.apply(
        &[TextEdit::new(alpha, "ALPHA", "Uppercase", None), TextEdit::new(beta, "BETA", "Uppercase", None)],
        "Uppercase",
        None,
    );
    assert_eq!(document.text(), "ALPHA\nBETA\n");

    let viewport_locks = Rc::new(Cell::new(0));
    let edit_locks = Rc::new(Cell::new(0));
    let locks = viewport_locks.clone();
    document.set_on_will_apply_undo_redo(Some(move || locks.set(locks.get() + 1)));
    let edits = edit_locks.clone();
    document.set_on_will_apply_edits(Some(move |_: &[TextEdit]| edits.set(edits.get() + 1)));

    document.undo_manager().undo();

    assert_eq!(viewport_locks.get(), 1);
    assert_eq!(edit_locks.get(), 0);
    assert_eq!(document.text(), source);
    assert_eq!(document.parsed().text, source);
}

/// `WorkerConcurrencyCounter`.
#[derive(Default)]
struct WorkerConcurrencyCounter {
    current: AtomicUsize,
    maximum: AtomicUsize,
}

impl WorkerConcurrencyCounter {
    fn enter(&self) {
        let current = self.current.fetch_add(1, Ordering::SeqCst) + 1;
        self.maximum.fetch_max(current, Ordering::SeqCst);
    }

    fn leave(&self) {
        self.current.fetch_sub(1, Ordering::SeqCst);
    }

    fn maximum(&self) -> usize {
        self.maximum.load(Ordering::SeqCst)
    }
}

fn latest_revision_wins_with_one_in_flight_worker() {
    let gate = Arc::new(ParseGate::default());
    let concurrency = Arc::new(WorkerConcurrencyCounter::default());
    let committed = ParseSignal::default();
    let (worker_gate, worker_concurrency) = (gate.clone(), concurrency.clone());
    let worker = MarkdownParseWorker::with_operation(move |text, previous, revision| {
        worker_concurrency.enter();
        let parsed = MarkdownParser::parse(&text);
        let dirty = ASTDiff::dirty_set(Some(&previous), &parsed);
        let held = worker_gate.hold(MarkdownParseResult { revision, text, document: parsed, dirty });
        worker_concurrency.leave();
        held
    });
    let document = document_with_worker(worker);
    document.adopt("one\n", None);
    let signal = committed.clone();
    document.set_on_reparse(Some(move |parsed: &Arc<ParsedDocument>, _: &DirtySet| {
        if parsed.text == "three\n" {
            signal.signal();
        }
    }));
    document.replace(whole(&document), "two\n", None);
    document.flush_scheduled_reparse();
    gate.wait_for_request_count_pumping(1);

    document.replace(whole(&document), "three\n", None);
    document.flush_scheduled_reparse();
    // The second snapshot waits in the coordinator's one-item pending slot.
    // It must not start while the first cmark parse is held.
    for _ in 0..4 {
        yield_main();
    }
    assert_eq!(gate.request_count(), 1);
    assert_eq!(concurrency.maximum(), 1);

    gate.release_next();
    gate.wait_for_request_count_pumping(2);
    gate.release_next();
    committed.wait();
    assert_eq!(document.parsed().text, "three\n");
    assert_eq!(concurrency.maximum(), 1);
}

fn burst_keeps_only_latest_pending_snapshot() {
    let gate = Arc::new(ParseGate::default());
    let document = document_with_worker(gated_worker(&gate));
    document.adopt("zero\n", None);

    document.replace(whole(&document), "one\n", None);
    document.flush_scheduled_reparse();
    gate.wait_for_request_count_pumping(1);

    for text in ["two\n", "three\n", "four\n"] {
        document.replace(whole(&document), text, None);
        document.flush_scheduled_reparse();
    }

    for _ in 0..4 {
        yield_main();
    }
    assert_eq!(gate.request_count(), 1);
    gate.release_next();
    gate.wait_for_request_count_pumping(2);
    gate.release_next();
    for _ in 0..6 {
        yield_main();
    }
    assert_eq!(document.parsed().text, "four\n");
}

fn close_drops_queued_parse_before_worker_starts() {
    let gate = Arc::new(ParseGate::default());
    let document = document_with_worker(gated_worker(&gate));
    document.adopt("one\n", None);
    document.replace(whole(&document), "two\n", None);
    document.close();
    document.flush_scheduled_reparse();
    yield_main();
    assert_eq!(gate.request_count(), 0);
}

fn close_rejects_in_flight_parse_result() {
    let gate = Arc::new(ParseGate::default());
    let document = document_with_worker(gated_worker(&gate));
    document.adopt("one\n", None);
    document.replace(whole(&document), "two\n", None);
    document.flush_scheduled_reparse();
    gate.wait_for_request_count_pumping(1);

    document.close();
    gate.release_next();
    for _ in 0..4 {
        yield_main();
    }
    assert_eq!(document.parsed().text, "one\n");
}

fn reopen_after_close_accepts_the_newest_snapshot() {
    let committed = ParseSignal::default();
    let document = document();
    document.adopt("one\n", None);
    document.close();
    document.adopt("two\n", None);
    let signal = committed.clone();
    document.set_on_reparse(Some(move |parsed: &Arc<ParsedDocument>, _: &DirtySet| {
        if parsed.text == "three\n" {
            signal.signal();
        }
    }));

    document.replace(whole(&document), "three\n", None);
    document.flush_scheduled_reparse();
    committed.wait();

    assert_eq!(document.parsed().text, "three\n");
}

/// The architectural P0 invariant: the synchronous edit path never parses.
/// The 8 ms budget itself is a release measurement owned by the benchmark;
/// the wall clock stays informational.
fn source_edit_path_never_parses_synchronously() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let worker = MarkdownParseWorker::with_operation(move |text, previous, revision| {
        counter.fetch_add(1, Ordering::SeqCst);
        let parsed = MarkdownParser::parse(&text);
        let dirty = ASTDiff::dirty_set(Some(&previous), &parsed);
        MarkdownParseResult { revision, text, document: parsed, dirty }
    });
    let document = document_with_worker(worker);
    let corpus = "line of markdown\n".repeat(5_000);
    document.adopt(&corpus, None);

    let mut durations: Vec<u128> = Vec::with_capacity(100);
    for _ in 0..100 {
        let start = Instant::now();
        let _ = document.replace(NSRange::new(0, 0), "x", None);
        durations.push(start.elapsed().as_nanos());
    }
    durations.sort();
    let p95 = durations[94];
    println!("[typing response] p95 {} ms (informational; budget enforced by the benchmark)", p95 as f64 / 1_000_000.0);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

fn main() {
    document_support::sandbox();
    main_thread::run(&[
        ("external_absorb_publishes_bounded_incremental_render", external_absorb_publishes_bounded_incremental_render),
        (
            "external_absorb_undo_redo_does_not_consume_next_local_edit",
            external_absorb_undo_redo_does_not_consume_next_local_edit,
        ),
        ("semantic_edit_converges_before_reading_tree", semantic_edit_converges_before_reading_tree),
        ("undo_and_redo_lock_viewport_and_reparse_before_returning", undo_and_redo_lock_viewport_and_reparse_before_returning),
        ("grouped_undo_locks_once_for_several_inverse_edits", grouped_undo_locks_once_for_several_inverse_edits),
        ("latest_revision_wins_with_one_in_flight_worker", latest_revision_wins_with_one_in_flight_worker),
        ("burst_keeps_only_latest_pending_snapshot", burst_keeps_only_latest_pending_snapshot),
        ("close_drops_queued_parse_before_worker_starts", close_drops_queued_parse_before_worker_starts),
        ("close_rejects_in_flight_parse_result", close_rejects_in_flight_parse_result),
        ("reopen_after_close_accepts_the_newest_snapshot", reopen_after_close_accepts_the_newest_snapshot),
        ("source_edit_path_never_parses_synchronously", source_edit_path_never_parses_synchronously),
        ("injected_worker_runs_pure_parse_and_diff", injected_worker_runs_pure_parse_and_diff),
        (
            "coordinator_keeps_one_parse_in_flight_and_only_the_newest_pending_snapshot",
            coordinator_keeps_one_parse_in_flight_and_only_the_newest_pending_snapshot,
        ),
        ("busy_state_is_published_on_transitions_only", busy_state_is_published_on_transitions_only),
        ("suspend_drops_the_pending_snapshot_and_refuses_new_ones", suspend_drops_the_pending_snapshot_and_refuses_new_ones),
        ("run_immediately_overlaps_a_held_parse", run_immediately_overlaps_a_held_parse),
        ("default_worker_parses_wholesale_against_an_empty_tree", default_worker_parses_wholesale_against_an_empty_tree),
    ]);
    document_support::remove_sandbox();
}
