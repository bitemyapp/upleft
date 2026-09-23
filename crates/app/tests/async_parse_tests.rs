//! Port of `Tests/DownrightAppTests/AsyncParseTests.swift`.
//!
//! Phase 1 holds the worker-level test (`injectedWorkerRunsPureParseAndDiff`)
//! and coordinator tests that pin the actor's latest-wins contract directly;
//! the `MarkdownDocument` tests arrive with `ai::markdown_document`. The
//! Swift suite is `@MainActor`, so this binary owns the main thread
//! (`harness = false`, see `main_thread`).

mod main_thread;

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use upleft_app::ai::markdown_parse_worker::*;
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

fn main() {
    main_thread::run(&[
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
}
