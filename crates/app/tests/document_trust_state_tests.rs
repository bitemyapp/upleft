//! Port of `Tests/DownrightAppTests/DocumentTrustStateTests.swift` ("Document
//! save trust and presentation state", `.serialized`).
//!
//! `@MainActor` suite: this binary owns the main thread (`harness = false`).
//! Documents use the sandboxed shared stores (`document_support`), never the
//! user's real Application Support folder.

mod document_support;
mod main_thread;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use document_support::{Fixture, document, exists, read, read_text, whole, write, write_atomically};
use upleft_app::ai::file_watcher::Event;
use upleft_app::ai::markdown_document::{DocumentError, Phase, PresentationState, SaveError, SaveIntent};
use upleft_core::contracts::TidyRule;
use upleft_core::document_io::DocumentIO;
use upleft_core::list_editing::ListEditing;
use upleft_core::text_diff::TextDiff;
use upleft_swift_text::NSRange;
use upleft_swift_text::ns::NSStringExt;

fn fixture(text: &str) -> Fixture {
    Fixture::new("downright-document-trust", "note.md", text)
}

/// `for _ in 0..<count where !condition() { try await Task.sleep(for: .milliseconds(5)) }`.
fn poll(count: usize, condition: impl Fn() -> bool) {
    for _ in 0..count {
        if condition() {
            return;
        }
        main_thread::sleep_pumping(Duration::from_millis(5));
    }
}

fn is_save_error(result: &Result<(), DocumentError>, predicate: impl Fn(&SaveError) -> bool) -> bool {
    matches!(result, Err(DocumentError::Save(error)) if predicate(error))
}

fn missing_file_save_fails_closed_until_explicit_recreation() {
    let fixture = fixture("before\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    assert!(document.replace(whole(&document), "after\n", Some("Replace")));
    std::fs::remove_file(fixture.url.path()).unwrap();

    let result = document.save_if_needed(SaveIntent::Normal);
    assert!(
        is_save_error(&result, |error| matches!(error, SaveError::FileMissing(_))),
        "a missing path must be an explicit save failure"
    );
    assert!(!exists(&fixture.url));
    assert!(document.is_dirty());
    assert_eq!(document.presentation_state().phase, Phase::SaveFailed);

    assert!(document.recreate_missing_file().is_ok(), "explicit recreation should write the retained buffer");
    assert_eq!(read_text(&fixture.url), "after\n");
    assert!(!document.is_dirty());
    assert_eq!(document.presentation_state().phase, Phase::Saved);
    document.close();
}

fn unreadable_path_is_not_overwritten() {
    let fixture = fixture("before\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    assert!(document.replace(whole(&document), "after\n", Some("Panel action")));
    std::fs::remove_file(fixture.url.path()).unwrap();
    std::fs::create_dir(fixture.url.path()).unwrap();

    let result = document.save_if_needed(SaveIntent::Normal);
    assert!(
        is_save_error(&result, |error| matches!(error, SaveError::FileUnreadable(..))),
        "an unreadable existing path must fail closed"
    );
    assert!(std::path::Path::new(&fixture.url.path()).is_dir());
    assert!(document.is_dirty());
    document.close();
}

fn task_toggle_immediate_save_never_recreates_a_deleted_file() {
    let fixture = fixture("- [ ] trust the save boundary\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    std::fs::remove_file(fixture.url.path()).unwrap();

    document.toggle_task(2);

    assert_eq!(document.text(), "- [x] trust the save boundary\n");
    assert!(!exists(&fixture.url));
    assert!(document.is_dirty());
    assert_eq!(document.presentation_state().phase, Phase::SaveFailed);
    document.close();
}

fn conflict_and_mutation_provenance_have_distinct_states() {
    let fixture = fixture("one\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    assert!(document.replace(whole(&document), "mine\n", Some("Paste")));
    assert_eq!(document.presentation_state().phase, Phase::Edited);
    assert_eq!(document.presentation_state().provenance.as_deref(), Some("Paste"));

    write_atomically(&fixture.url, b"theirs\n");
    let result = document.save_if_needed(SaveIntent::Normal);
    assert!(
        is_save_error(&result, |error| matches!(error, SaveError::BlockedByExternalConflict)),
        "a newer disk version must become a conflict"
    );
    assert_eq!(document.presentation_state().phase, Phase::Conflict);
    assert_eq!(read_text(&fixture.url), "theirs\n");
    document.close();
}

fn explicit_keep_mine_resolves_and_writes_a_real_conflict() {
    let fixture = fixture("before\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    assert!(document.replace(whole(&document), "mine\n", Some("Replace")));
    write_atomically(&fixture.url, b"theirs\n");
    assert!(
        is_save_error(&document.save_if_needed(SaveIntent::Normal), |error| matches!(
            error,
            SaveError::BlockedByExternalConflict
        )),
        "the initial save must surface a conflict"
    );

    assert!(document.resolve_conflict_keeping_mine().is_ok(), "Keep Mine must be an explicit permitted overwrite");
    assert_eq!(read(&fixture.url), b"mine\n");
    assert!(!document.is_dirty());
    assert!(document.pending_conflict().is_none());
    document.close();
}

fn atomic_replacement_at_commit_boundary_is_restored_and_becomes_conflict() {
    let fixture = fixture("before\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    assert!(document.replace(whole(&document), "mine\n", Some("Paste")));
    let url = fixture.url.clone();
    document.set_before_save_commit_for_testing(Some(move || write_atomically(&url, b"external-at-boundary\n")));

    assert!(
        is_save_error(&document.save_if_needed(SaveIntent::Normal), |error| matches!(
            error,
            SaveError::BlockedByExternalConflict
        )),
        "a replacement at the exact commit boundary must fail closed"
    );
    assert_eq!(read(&fixture.url), b"external-at-boundary\n");
    assert!(document.is_dirty());
    document.close();
}

fn deletion_at_commit_boundary_never_recreates_the_path() {
    let fixture = fixture("before\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    assert!(document.replace(whole(&document), "mine\n", Some("Replace")));
    let url = fixture.url.clone();
    document.set_before_save_commit_for_testing(Some(move || std::fs::remove_file(url.path()).unwrap()));

    assert!(document.save_if_needed(SaveIntent::Normal).is_err(), "a concurrent deletion must fail");
    assert!(!exists(&fixture.url));
    assert!(document.is_dirty());
    document.close();
}

fn recreate_choice_does_not_clobber_a_file_that_reappeared() {
    let fixture = fixture("before\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    assert!(document.replace(whole(&document), "mine\n", Some("Replace")));
    std::fs::remove_file(fixture.url.path()).unwrap();
    assert!(
        is_save_error(&document.save_if_needed(SaveIntent::Normal), |error| matches!(error, SaveError::FileMissing(_))),
        "missing file recovery must be active"
    );
    write(&fixture.url, b"restored-by-someone-else\n");

    assert!(
        is_save_error(&document.recreate_missing_file(), |error| matches!(error, SaveError::BlockedByExternalConflict)),
        "Recreate must reconcile a readable file that appeared after the alert"
    );
    assert_eq!(read(&fixture.url), b"restored-by-someone-else\n");
    document.close();
}

fn recreation_does_not_overwrite_a_file_created_at_commit_boundary() {
    let fixture = fixture("before\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    document.replace(whole(&document), "mine\n", Some("Typing"));
    std::fs::remove_file(fixture.url.path()).unwrap();
    let url = fixture.url.clone();
    document.set_before_save_commit_for_testing(Some(move || write_atomically(&url, b"external\n")));
    assert!(
        is_save_error(&document.recreate_missing_file(), |error| matches!(error, SaveError::BlockedByExternalConflict)),
        "recreation must not replace a concurrent external creation"
    );
    assert_eq!(read(&fixture.url), b"external\n");
    assert!(document.is_dirty());
    document.close();
}

fn discarded_edits_cannot_return_on_the_next_keystroke() {
    let fixture = fixture("saved\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    assert!(document.replace(whole(&document), "discard-me\n", Some("Paste")));
    document.discard_unsaved_changes();
    assert_eq!(document.text(), "saved\n");
    assert!(!document.is_dirty());
    let end = document.storage().length() as isize;
    assert!(document.replace(NSRange::new(end, 0), "new\n", Some("Typing")));
    document.save(SaveIntent::Normal).unwrap();
    assert_eq!(read(&fixture.url), b"saved\nnew\n");
    document.close();
}

/// §6.4: indent/outdent renumber ordered lists automatically, and the
/// renumbering lands in the SAME undo group as the indentation — one ⌘Z
/// restores the original text exactly.
fn outdenting_an_ordered_list_renumbers_in_one_undo_group() {
    let fixture = fixture("1. one\n   1. child\n2. two\n");
    let document = document();
    document.open(&fixture.url).unwrap();

    // Outdent the nested item: the raw edit only removes its indent.
    document.ensure_parsed_current();
    let line = upleft_swift_text::ns::utf16(&document.text()).as_slice().line_range_for(NSRange::new(7, 0));
    let edits = ListEditing::indent(&document.parsed(), line, true);
    assert!(!edits.is_empty());
    document.apply(&edits, "Outdent", Some(&[TidyRule::OrderedListNumbers]));

    assert_eq!(document.text(), "1. one\n2. child\n3. two\n", "the outdented list is renumbered automatically");

    // One undo step removes the renumber and the indent together.
    document.undo_manager().undo();
    assert_eq!(document.text(), "1. one\n   1. child\n2. two\n");
    document.close();
}

/// The Discard composition, end to end: whatever implicit save work was
/// already queued when the user pressed Discard must find a clean buffer
/// afterwards and leave the declined bytes off disk.
fn late_implicit_save_after_discard_writes_nothing() {
    let fixture = fixture("on disk\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    assert!(document.replace(whole(&document), "declined\n", Some("Paste")));
    let disk_before = read(&fixture.url);

    // The alert handler's action for "Discard Changes".
    document.discard_unsaved_changes();
    assert!(!document.is_dirty());

    assert!(
        document.save_if_needed(SaveIntent::Normal).is_ok(),
        "a discarded buffer must read as clean to implicit saves"
    );
    assert_eq!(read(&fixture.url), disk_before, "the declined edits must not reach disk through any later save");
    document.close();
}

fn byte_only_external_rewrite_updates_fidelity_before_saving() {
    let fixture = fixture("one\ntwo\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    let mut bom_crlf = vec![0xEF, 0xBB, 0xBF];
    bom_crlf.extend_from_slice(b"one\r\ntwo\r\n");
    write_atomically(&fixture.url, &bom_crlf);
    assert!(document.replace(NSRange::new(0, 3), "ONE", Some("Replace")));

    assert!(
        document.save_if_needed(SaveIntent::Normal).is_ok(),
        "a byte-only disk generation should reconcile without a content conflict"
    );
    let mut expected = vec![0xEF, 0xBB, 0xBF];
    expected.extend_from_slice(b"ONE\r\ntwo\r\n");
    assert_eq!(read(&fixture.url), expected);
    document.close();
}

fn mixed_line_ending_external_edit_preserves_conflict_detection(changes_content: bool) {
    let fixture = fixture("original\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    assert!(document.replace(whole(&document), "header\na", Some("Replace")));
    let incoming: &[u8] = if changes_content { b"header\nab\r\n" } else { b"header\na\r\n" };
    write_atomically(&fixture.url, incoming);
    let finished = Rc::new(Cell::new(false));
    let flag = finished.clone();
    document.set_on_external_write_activity(Some(move |active: bool| {
        if !active {
            flag.set(true);
        }
    }));
    document.handle_external_write();
    document.flush_pending_external_write();
    poll(100, || finished.get());
    assert!(finished.get());
    assert_eq!(document.text(), "header\na");
    assert_eq!(document.pending_conflict().is_some(), changes_content);
    if changes_content {
        assert!(
            document.save_if_needed(SaveIntent::Normal).is_err(),
            "an external content change must block saving"
        );
    }
    assert_eq!(read(&fixture.url), incoming);
    document.close();
}

fn mixed_line_ending_external_edit_preserves_conflict_detection_without_content_change() {
    mixed_line_ending_external_edit_preserves_conflict_detection(false);
}

fn mixed_line_ending_external_edit_preserves_conflict_detection_with_content_change() {
    mixed_line_ending_external_edit_preserves_conflict_detection(true);
}

fn removing_final_newline_is_a_real_user_edit() {
    let fixture = fixture("body\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    assert!(document.replace(NSRange::new(4, 1), "", Some("Delete")));
    document.save(SaveIntent::Normal).unwrap();
    assert_eq!(read(&fixture.url), b"body");
    document.close();
}

fn clean_external_final_newline_edit_survives_the_next_save() {
    let fixture = fixture("body\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    write_atomically(&fixture.url, b"body");
    document.handle_external_write();
    document.flush_pending_external_write();
    poll(100, || document.text() == "body");
    assert_eq!(document.text(), "body");
    assert!(!document.is_dirty());
    document.replace(NSRange::new(0, 4), "changed", Some("Typing"));
    document.save(SaveIntent::Normal).unwrap();
    assert_eq!(read(&fixture.url), b"changed");
    document.close();
}

fn watcher_does_not_undo_a_local_final_newline_edit() {
    let fixture = fixture("body\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    document.replace(NSRange::new(4, 1), "", Some("Delete"));
    let finished = Rc::new(Cell::new(false));
    let flag = finished.clone();
    document.set_on_external_write_activity(Some(move |active: bool| {
        if !active {
            flag.set(true);
        }
    }));
    document.handle_external_write();
    document.flush_pending_external_write();
    poll(100, || finished.get());
    assert!(finished.get());
    assert_eq!(document.text(), "body");
    assert!(document.is_dirty());
    assert!(document.pending_conflict().is_none());
    document.save(SaveIntent::Normal).unwrap();
    assert_eq!(read(&fixture.url), b"body");
    document.close();
}

fn taking_theirs_adopts_recovery_text_and_byte_fidelity() {
    let fixture = fixture("original\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    document.replace(whole(&document), "mine\n", Some("Typing"));
    let mut incoming = vec![0xEF, 0xBB, 0xBF];
    incoming.extend_from_slice(b"theirs\r\n");
    write_atomically(&fixture.url, &incoming);
    let _ = document.save_if_needed(SaveIntent::Normal);
    let conflict = document.pending_conflict().expect("the save surfaced a conflict");
    std::fs::remove_file(fixture.url.path()).unwrap();
    document.resolve_conflict_taking_theirs(&conflict);
    assert_eq!(DocumentIO::encoded_data(&document.text(), document.fidelity()).unwrap(), incoming);
    document.replace(whole(&document), "discard me\n", Some("Typing"));
    document.discard_unsaved_changes();
    assert_eq!(document.text(), "theirs\n");
    assert!(!document.is_dirty());
    assert!(!exists(&fixture.url));
    document.close();
}

fn identical_restore_clears_the_missing_file_presentation() {
    let fixture = fixture("same\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    document.handle_watch_event(Event::Removed);
    assert_eq!(document.presentation_state().phase, Phase::ChangedOnDisk);
    assert_eq!(document.presentation_state().detail.as_deref(), Some("File missing"));

    document.handle_watch_event(Event::Restored);
    document.flush_pending_external_write();
    poll(100, || document.presentation_state().phase == Phase::Neutral);
    assert_eq!(document.presentation_state(), PresentationState::NEUTRAL);
    document.close();
}

fn parsing_and_other_non_mutating_work_never_marks_dirty() {
    let fixture = fixture("# Heading\n\nBody.\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    document.ensure_parsed_current();
    document.mark_changes_reviewed();
    let _ = document.restored_offset();
    assert!(!document.is_dirty());
    assert_eq!(document.presentation_state(), PresentationState::NEUTRAL);
    document.close();
}

fn clean_external_rewrite_parses_asynchronously_and_restores_after_commit() {
    let document = document();
    let original = "# One\n\nBody.\n";
    let incoming = format!("# One\n\n{}", "A longer external paragraph.\n\n".repeat(2_000));
    document.adopt(original, None);
    let incoming_commits = Rc::new(Cell::new(0));
    let commits = incoming_commits.clone();
    let expected = incoming.clone();
    document.set_on_reparse(Some(move |parsed: &std::sync::Arc<upleft_core::model::ParsedDocument>, _: &_| {
        if parsed.text == expected {
            commits.set(commits.get() + 1);
        }
    }));

    document.apply_external_text(&incoming, &TextDiff::hunks(original, &incoming));
    assert_eq!(document.text(), incoming);
    assert_ne!(document.parsed().text, incoming, "the main actor must not synchronously parse a wholesale rewrite");

    poll(200, || document.parsed().text == incoming);
    assert_eq!(document.parsed().text, incoming);
    assert_eq!(incoming_commits.get(), 1);
    document.close();
}

fn local_edit_during_external_parse_owns_the_latest_revision() {
    let document = document();
    let original = "# One\n\nBody.\n";
    let incoming = "# External\n\nBody.\n";
    document.adopt(original, None);
    document.apply_external_text(incoming, &TextDiff::hunks(original, incoming));
    let end = document.storage().length() as isize;
    assert!(document.replace(NSRange::new(end, 0), "Local tail.\n", Some("Paste")));

    poll(200, || document.parsed().text == document.text());
    assert_eq!(document.parsed().text, document.text());
    assert!(document.text().ends_with("Local tail.\n"));
    assert!(document.is_dirty());
    assert_eq!(document.presentation_state().provenance.as_deref(), Some("Paste"));
    document.close();
}

fn main() {
    document_support::sandbox();
    main_thread::run(&[
        ("missing_file_save_fails_closed_until_explicit_recreation", missing_file_save_fails_closed_until_explicit_recreation),
        ("unreadable_path_is_not_overwritten", unreadable_path_is_not_overwritten),
        ("task_toggle_immediate_save_never_recreates_a_deleted_file", task_toggle_immediate_save_never_recreates_a_deleted_file),
        ("conflict_and_mutation_provenance_have_distinct_states", conflict_and_mutation_provenance_have_distinct_states),
        ("explicit_keep_mine_resolves_and_writes_a_real_conflict", explicit_keep_mine_resolves_and_writes_a_real_conflict),
        (
            "atomic_replacement_at_commit_boundary_is_restored_and_becomes_conflict",
            atomic_replacement_at_commit_boundary_is_restored_and_becomes_conflict,
        ),
        ("deletion_at_commit_boundary_never_recreates_the_path", deletion_at_commit_boundary_never_recreates_the_path),
        ("recreate_choice_does_not_clobber_a_file_that_reappeared", recreate_choice_does_not_clobber_a_file_that_reappeared),
        (
            "recreation_does_not_overwrite_a_file_created_at_commit_boundary",
            recreation_does_not_overwrite_a_file_created_at_commit_boundary,
        ),
        ("discarded_edits_cannot_return_on_the_next_keystroke", discarded_edits_cannot_return_on_the_next_keystroke),
        ("outdenting_an_ordered_list_renumbers_in_one_undo_group", outdenting_an_ordered_list_renumbers_in_one_undo_group),
        ("late_implicit_save_after_discard_writes_nothing", late_implicit_save_after_discard_writes_nothing),
        ("byte_only_external_rewrite_updates_fidelity_before_saving", byte_only_external_rewrite_updates_fidelity_before_saving),
        (
            "mixed_line_ending_external_edit_preserves_conflict_detection(changesContent: false)",
            mixed_line_ending_external_edit_preserves_conflict_detection_without_content_change,
        ),
        (
            "mixed_line_ending_external_edit_preserves_conflict_detection(changesContent: true)",
            mixed_line_ending_external_edit_preserves_conflict_detection_with_content_change,
        ),
        ("removing_final_newline_is_a_real_user_edit", removing_final_newline_is_a_real_user_edit),
        ("clean_external_final_newline_edit_survives_the_next_save", clean_external_final_newline_edit_survives_the_next_save),
        ("watcher_does_not_undo_a_local_final_newline_edit", watcher_does_not_undo_a_local_final_newline_edit),
        ("taking_theirs_adopts_recovery_text_and_byte_fidelity", taking_theirs_adopts_recovery_text_and_byte_fidelity),
        ("identical_restore_clears_the_missing_file_presentation", identical_restore_clears_the_missing_file_presentation),
        ("parsing_and_other_non_mutating_work_never_marks_dirty", parsing_and_other_non_mutating_work_never_marks_dirty),
        (
            "clean_external_rewrite_parses_asynchronously_and_restores_after_commit",
            clean_external_rewrite_parses_asynchronously_and_restores_after_commit,
        ),
        ("local_edit_during_external_parse_owns_the_latest_revision", local_edit_during_external_parse_owns_the_latest_revision),
    ]);
    document_support::remove_sandbox();
}
