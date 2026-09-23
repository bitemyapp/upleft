//! Port of the `MarkdownDocument` tests in
//! `Tests/DownrightAppTests/ReviewBaselineTests.swift` (the rest of that file
//! is `review_baseline_tests.rs`, which runs under libtest; these need the
//! main thread, so they live in their own `harness = false` binary).
//!
//! The review baseline (§8.1, §8.2): change marks are *reviewable state*, and
//! only the user advances it.
//!
//! Tests that use `MarkdownDocument()` run on the sandboxed shared stores
//! (`document_support`), as the Swift ones run on the process-wide
//! singletons; `makeIsolatedDocument(in:)` keeps its per-test stores.

mod document_support;
mod main_thread;

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use document_support::{document, isolated_document, temporary_directory, unique};
use objc2::rc::Retained;
use upleft_app::ai::document_state_store::DocumentStateStore;
use upleft_app::ai::markdown_document::{
    DocumentError, ExternalEvent, MarkdownDocument, SaveError, SaveIntent, Unavailable, UnreadChanges,
};
use upleft_app::ai::snapshot_store::SnapshotStore;
use upleft_foundation::url::FileUrl;

const ORIGINAL: &str = "# Report\n\nAlpha paragraph.\n\nBravo paragraph.\n\nCharlie paragraph.\n";

/// The agent's three writes, each touching a different paragraph.  Diffed
/// against each other they are one change apiece; diffed against the
/// original they accumulate, which is the distinction under test.
fn writes() -> [String; 3] {
    let first = ORIGINAL.replace("Alpha paragraph.", "Alpha paragraph, revised.");
    let second = first.replace("Bravo paragraph.", "Bravo paragraph, revised.");
    let third = second.replace("Charlie paragraph.", "Charlie paragraph, revised.");
    [first, second, third]
}

fn make_sandbox() -> FileUrl {
    let root = temporary_directory().appending_path_component_is_directory(&format!("downright-baseline-{}", unique()), true);
    std::fs::create_dir_all(root.path()).unwrap();
    root
}

struct Cleanup(FileUrl);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.path());
    }
}

fn wait_for_external_text(text: &str, document: &MarkdownDocument) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while document.text() != text && Instant::now() < deadline {
        main_thread::sleep_pumping(Duration::from_millis(5));
    }
    document.text() == text
}

fn open(document: &Retained<MarkdownDocument>, url: &FileUrl) {
    document.open(url).unwrap();
}

// MARK: - 1. A burst diffs against the baseline, not the previous write

fn burst_of_writes_reports_changes_against_the_original_baseline() {
    let root = make_sandbox();
    let _cleanup = Cleanup(root.clone());
    let url = root.appending_path_component("report.md");
    std::fs::write(url.path(), ORIGINAL).unwrap();

    let document = isolated_document(&root);
    open(&document, &url);

    // Absorb each write separately — the case the debounce does *not* cover,
    // and the one that used to lose writes 1 and 2.
    for write in writes() {
        std::fs::write(url.path(), &write).unwrap();
        document.handle_external_write();
        document.flush_pending_external_write();
        assert!(wait_for_external_text(&write, &document));
    }

    assert_eq!(document.text(), writes()[2]);
    assert_eq!(
        document.changes().count(),
        3,
        "after three writes the reader must still see all three changes; got {}, which means the diff was taken \
         against the previous write instead of the review baseline",
        document.changes().count()
    );
    assert_eq!(document.review_baseline_text(), ORIGINAL, "an incoming write must not move the baseline");

    // Finishing the review is the only thing that moves it.
    document.mark_changes_reviewed();
    assert!(document.changes().is_empty());
    assert_eq!(document.review_baseline_text(), writes()[2]);
    assert_eq!(document.unread_changes(), UnreadChanges::None);
    document.close();
}

/// The trailing quiet period folds a genuine burst into a single absorb —
/// one buffer replace, one reparse, one scroll restore — while still
/// reporting every change the burst made.
fn a_debounced_burst_absorbs_once_and_still_reports_every_change() {
    let root = make_sandbox();
    let _cleanup = Cleanup(root.clone());
    let url = root.appending_path_component("report.md");
    std::fs::write(url.path(), ORIGINAL).unwrap();

    let document = isolated_document(&root);
    open(&document, &url);

    let applied_events = Rc::new(Cell::new(0));
    let counter = applied_events.clone();
    document.set_on_external_event(Some(move |event: &ExternalEvent| {
        if matches!(event, ExternalEvent::Applied { .. }) {
            counter.set(counter.get() + 1);
        }
    }));

    for write in writes() {
        std::fs::write(url.path(), &write).unwrap();
        document.handle_external_write();
    }
    document.flush_pending_external_write();
    assert!(wait_for_external_text(&writes()[2], &document));

    assert_eq!(applied_events.get(), 1, "a burst must rebuild the document once, not once per write");
    assert_eq!(document.text(), writes()[2]);
    assert_eq!(document.changes().count(), 3);
    document.close();
}

// MARK: - 2. Marks survive a close/reopen cycle

fn marks_survive_a_close_and_reopen_cycle() {
    let root = make_sandbox();
    let _cleanup = Cleanup(root.clone());
    let url = root.appending_path_component("report.md");
    std::fs::write(url.path(), ORIGINAL).unwrap();

    let first = document();
    open(&first, &url);
    std::fs::write(url.path(), &writes()[2]).unwrap();
    first.handle_external_write();
    first.flush_pending_external_write();
    assert!(wait_for_external_text(&writes()[2], &first));

    let mark_count = first.changes().count();
    assert_eq!(mark_count, 3);
    // The reader worked through exactly one of them before closing.
    let reviewed = first.changes().marks().first().cloned().expect("a mark");
    first.changes().mark_visited(reviewed.id);
    first.close();

    SnapshotStore::shared().wait_for_pending_writes();

    let second = document();
    open(&second, &url);

    assert_eq!(second.changes().count(), mark_count, "closing a window is not reviewing its changes");
    assert_eq!(second.unread_changes(), UnreadChanges::Marked { count: mark_count });
    assert_eq!(
        second.changes().marks().iter().filter(|mark| mark.visited).count(),
        1,
        "the one mark the reader had worked through must come back dimmed, not unread"
    );
    assert_eq!(second.changes().unread_count(), mark_count - 1);
    second.close();
}

/// The degraded case: the bytes moved while the app was closed and the
/// previous text has been pruned.  The old guard collapsed this into
/// "nothing changed"; it has to stay separable from that.
fn a_pruned_baseline_surfaces_a_degraded_state_rather_than_silence() {
    let root = make_sandbox();
    let _cleanup = Cleanup(root.clone());
    let url = root.appending_path_component("report.md");
    std::fs::write(url.path(), &writes()[2]).unwrap();

    let mut state = DocumentStateStore::shared().state(&url);
    state.review_baseline_hash = "0".repeat(64); // no such object
    DocumentStateStore::shared().save(&state, &url);

    let document = document();
    open(&document, &url);

    assert_eq!(document.unread_changes(), UnreadChanges::PreviousVersionUnavailable { reason: Unavailable::Pruned });
    assert!(document.changes().is_empty(), "there is nothing to anchor a mark to");
    document.close();
}

// MARK: - Undo of external changes

fn undoing_an_external_change_surfaces_a_conflict() {
    let root = make_sandbox();
    let _cleanup = Cleanup(root.clone());
    let url = root.appending_path_component("report.md");
    std::fs::write(url.path(), ORIGINAL).unwrap();

    let document = document();
    open(&document, &url);

    let conflicts = Rc::new(Cell::new(0));
    let counter = conflicts.clone();
    document.set_on_external_event(Some(move |event: &ExternalEvent| {
        if matches!(event, ExternalEvent::Conflict(_)) {
            counter.set(counter.get() + 1);
        }
    }));

    std::fs::write(url.path(), &writes()[2]).unwrap();
    document.handle_external_write();
    document.flush_pending_external_write();
    assert!(wait_for_external_text(&writes()[2], &document));
    assert_eq!(document.text(), writes()[2]);
    assert!(!document.is_dirty());

    document.undo_manager().undo();

    assert_eq!(document.text(), ORIGINAL, "⌘Z must put the agent's rewrite back");
    assert!(document.is_dirty(), "the buffer now disagrees with the file on disk");
    assert_eq!(conflicts.get(), 1, "and that disagreement has to be visible, not discovered at save time");
    assert!(document.pending_conflict().is_some());

    // The save that would have failed for no visible reason is now refused
    // with the conflict already on screen.
    assert!(matches!(document.save(SaveIntent::Normal), Err(DocumentError::Save(SaveError::BlockedByExternalConflict))));
    document.close();
}

fn undo_walks_back_through_burst_of_external_writes() {
    let root = make_sandbox();
    let _cleanup = Cleanup(root.clone());
    let url = root.appending_path_component("report.md");
    std::fs::write(url.path(), ORIGINAL).unwrap();

    let document = isolated_document(&root);
    open(&document, &url);

    for write in writes() {
        std::fs::write(url.path(), &write).unwrap();
        document.handle_external_write();
        document.flush_pending_external_write();
        assert!(wait_for_external_text(&write, &document));
    }

    for expected in writes()[..2].iter().rev() {
        document.undo_manager().undo();
        assert_eq!(&document.text(), expected);
    }
    document.undo_manager().undo();
    assert_eq!(document.text(), ORIGINAL);
    document.close();
}

fn redo_restores_external_version() {
    let root = make_sandbox();
    let _cleanup = Cleanup(root.clone());
    let url = root.appending_path_component("report.md");
    std::fs::write(url.path(), ORIGINAL).unwrap();

    let document = isolated_document(&root);
    open(&document, &url);

    std::fs::write(url.path(), &writes()[2]).unwrap();
    document.handle_external_write();
    document.flush_pending_external_write();
    assert!(wait_for_external_text(&writes()[2], &document));

    document.undo_manager().undo();
    assert_eq!(document.text(), ORIGINAL);
    document.undo_manager().redo();
    assert_eq!(document.text(), writes()[2]);
    document.close();
}

fn external_change_undo_stack_is_bounded() {
    let root = temporary_directory().appending_path_component_is_directory(&format!("downright-undo-{}", unique()), true);
    let _cleanup = Cleanup(root.clone());
    let document = isolated_document(&root);
    assert_eq!(document.undo_manager().levelsOfUndo(), 200);
}

fn main() {
    document_support::sandbox();
    main_thread::run(&[
        (
            "burst_of_writes_reports_changes_against_the_original_baseline",
            burst_of_writes_reports_changes_against_the_original_baseline,
        ),
        (
            "a_debounced_burst_absorbs_once_and_still_reports_every_change",
            a_debounced_burst_absorbs_once_and_still_reports_every_change,
        ),
        ("marks_survive_a_close_and_reopen_cycle", marks_survive_a_close_and_reopen_cycle),
        (
            "a_pruned_baseline_surfaces_a_degraded_state_rather_than_silence",
            a_pruned_baseline_surfaces_a_degraded_state_rather_than_silence,
        ),
        ("undoing_an_external_change_surfaces_a_conflict", undoing_an_external_change_surfaces_a_conflict),
        ("undo_walks_back_through_burst_of_external_writes", undo_walks_back_through_burst_of_external_writes),
        ("redo_restores_external_version", redo_restores_external_version),
        ("external_change_undo_stack_is_bounded", external_change_undo_stack_is_bounded),
    ]);
    document_support::remove_sandbox();
}
