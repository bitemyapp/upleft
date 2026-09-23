//! Port of the `MarkdownDocument` and `SiblingScanner` tests in
//! `Tests/DownrightAppTests/AppLayerTests.swift` (the store tests are
//! `app_layer_tests.rs`, under libtest; these need the main thread, so they
//! live in their own `harness = false` binary), and of
//! `WorkspaceTests.siblingUnseenStateUsesContentHash`.
//!
//! Stores are the sandboxed shared ones (`document_support`).

mod document_support;
mod main_thread;

use std::cell::Cell;
use std::rc::Rc;

use document_support::{document, temporary_directory, unique, whole};
use upleft_app::ai::document_state_store::DocumentStateStore;
use upleft_app::ai::markdown_document::{DocumentError, SaveIntent};
use upleft_app::ai::sibling_scanner::SiblingScanner;
use upleft_app::ai::snapshot_store::SnapshotStore;
use upleft_core::contracts::{ChangeHunk, ChangeKind};
use upleft_foundation::url::FileUrl;
use upleft_swift_text::NSRange;

struct Cleanup(FileUrl);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.path());
    }
}

fn root(prefix: &str) -> FileUrl {
    let root = temporary_directory().appending_path_component_is_directory(&format!("{prefix}-{}", unique()), true);
    std::fs::create_dir_all(root.path()).unwrap();
    root
}

fn hunk(kind: ChangeKind, new: (isize, isize), old: (isize, isize)) -> ChangeHunk {
    ChangeHunk::new(kind, NSRange::new(new.0, new.1), NSRange::new(old.0, old.1), Vec::new())
}

// MARK: - AppLayerTests

fn failed_save_returns_error_and_preserves_buffer() {
    let root = root("downright-save");
    let _cleanup = Cleanup(root.clone());
    let url = root.appending_path_component("note.md");
    std::fs::write(url.path(), "before\n").unwrap();

    let document = document();
    document.open(&url).unwrap();
    assert!(document.replace(whole(&document), "after\n", None));

    let failure_count = Rc::new(Cell::new(0));
    let counter = failure_count.clone();
    document.set_on_save_failure(Some(move |_: &DocumentError| counter.set(counter.get() + 1)));
    std::fs::remove_dir_all(root.path()).unwrap();

    let result = document.save_if_needed(SaveIntent::Normal);
    assert!(result.is_err(), "saveIfNeeded must report a failed disk write");
    assert!(document.is_dirty());
    assert_eq!(document.text(), "after\n");
    assert_eq!(failure_count.get(), 1);

    document.changes().apply(&[hunk(ChangeKind::Modified, (0, 5), (0, 6))], "", "", true);
    let conflict_result = document.resolve_conflict_keeping_mine();
    assert!(conflict_result.is_err(), "conflict resolution must report a failed save");
    assert_eq!(document.changes().count(), 1);
    assert_eq!(failure_count.get(), 2);
    document.close();
}

fn change_marks_do_not_leak_across_in_place_reopen() {
    let root = root("downright-hop");
    let _cleanup = Cleanup(root.clone());

    let first_url = root.appending_path_component("first.md");
    let second_url = root.appending_path_component("second.md");
    std::fs::write(first_url.path(), "# First\n\nAlpha.\n").unwrap();
    std::fs::write(second_url.path(), "# Second\n\nBeta.\n").unwrap();

    let document = document();
    document.open(&first_url).unwrap();
    document.changes().apply(&[hunk(ChangeKind::Modified, (0, 5), (0, 5))], "", "", true);
    assert_eq!(document.changes().count(), 1);

    // An in-place hop reuses the same document; stale marks must not
    // decorate the next file.
    document.open(&second_url).unwrap();
    assert!(document.changes().is_empty(), "change marks leaked across an in-place reopen");
    document.close();
}

fn sibling_scanner_finds_markdown_in_docs_subdirectory() {
    let root = root("downright-siblings");
    let _cleanup = Cleanup(root.clone());
    let docs = root.appending_path_component_is_directory("docs", true);
    std::fs::create_dir_all(docs.path()).unwrap();

    let main = root.appending_path_component("PLAN.md");
    std::fs::write(main.path(), "# Plan\n").unwrap();
    std::fs::write(root.appending_path_component("NOTES.md").path(), "# Notes\n").unwrap();
    std::fs::write(docs.appending_path_component("DEEP.md").path(), "# Deep\n").unwrap();
    std::fs::write(root.appending_path_component("data.csv").path(), "not markdown").unwrap();

    let scanner = SiblingScanner::new(&main, vec!["docs".to_owned()]);
    let mut names: Vec<String> = scanner.siblings().iter().map(|sibling| sibling.display_name.clone()).collect();
    names.sort();
    assert_eq!(names, vec!["DEEP", "NOTES", "PLAN"]);
    assert!(scanner.siblings().iter().any(|sibling| sibling.group.as_deref() == Some("docs")));
    assert!(scanner.siblings().first().is_some_and(|sibling| sibling.is_current), "the open document sorts first");
}

// MARK: - WorkspaceTests

fn sibling_unseen_state_uses_content_hash() {
    let root = root("downright-sibling-hash");
    let _cleanup = Cleanup(root.clone());

    let current = root.appending_path_component("CURRENT.md");
    let sibling = root.appending_path_component("SIBLING.md");
    let original = "# Same\n";
    std::fs::write(current.path(), original).unwrap();
    std::fs::write(sibling.path(), original).unwrap();

    let mut state = DocumentStateStore::shared().state(&sibling);
    state.last_seen_hash = SnapshotStore::hash(original);
    DocumentStateStore::shared().save(&state, &sibling);

    let scanner = SiblingScanner::new(&current, Vec::new());
    let unseen = |scanner: &SiblingScanner| {
        scanner
            .siblings()
            .iter()
            .find(|entry| entry.url.last_path_component() == sibling.last_path_component())
            .map(|entry| entry.has_unseen_changes)
    };
    assert_eq!(scanner.siblings().len(), 2);
    assert_eq!(unseen(&scanner), Some(false));

    std::fs::write(sibling.path(), "# Changed\n").unwrap();
    scanner.scan(true, true);
    assert_eq!(unseen(&scanner), Some(true));
}

fn main() {
    document_support::sandbox();
    main_thread::run(&[
        ("failed_save_returns_error_and_preserves_buffer", failed_save_returns_error_and_preserves_buffer),
        ("change_marks_do_not_leak_across_in_place_reopen", change_marks_do_not_leak_across_in_place_reopen),
        ("sibling_scanner_finds_markdown_in_docs_subdirectory", sibling_scanner_finds_markdown_in_docs_subdirectory),
        ("sibling_unseen_state_uses_content_hash", sibling_unseen_state_uses_content_hash),
    ]);
    document_support::remove_sandbox();
}
