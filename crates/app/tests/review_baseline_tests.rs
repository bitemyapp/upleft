//! Port of `Tests/DownrightAppTests/ReviewBaselineTests.swift`, the parts that
//! exercise the change tracker, the text diff, the snapshot store and the
//! document state on their own.
//!
//! `aCorruptObjectIsReportedAsCorruptRatherThanReturnedAsMojibake` runs
//! against an isolated store: Swift's version truncates an object in the
//! user's real `SnapshotStore.shared` and puts it back afterwards, which a
//! port must never do.
//!
//! Not ported, because they drive `MarkdownDocument` (ported later by the
//! document-layer agent): `burstOfWritesReportsChangesAgainstTheOriginalBaseline`,
//! `aDebouncedBurstAbsorbsOnceAndStillReportsEveryChange`,
//! `marksSurviveACloseAndReopenCycle`,
//! `aPrunedBaselineSurfacesADegradedStateRatherThanSilence`,
//! `undoingAnExternalChangeSurfacesAConflict`,
//! `undoWalksBackThroughBurstOfExternalWrites`, `redoRestoresExternalVersion`,
//! `externalChangeUndoStackIsBounded`.

use upleft_app::ai::change_tracker::ChangeTracker;
use upleft_app::ai::document_state_store::DocumentState;
use upleft_app::ai::snapshot_store::{Content, SnapshotKind, SnapshotStore};
use upleft_core::contracts::{ChangeHunk, ChangeKind, Uuid};
use upleft_core::ns_range::NSRange;
use upleft_core::text_diff::TextDiff;
use upleft_foundation::date::Date;
use upleft_foundation::url::FileUrl;

const ORIGINAL: &str = "# Report\n\nAlpha paragraph.\n\nBravo paragraph.\n\nCharlie paragraph.\n";

fn utf16_length(text: &str) -> isize {
    text.encode_utf16().count() as isize
}

fn substring(text: &str, range: NSRange) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    String::from_utf16_lossy(&units[range.as_usize_range()])
}

#[test]
fn a_corrupt_object_is_reported_as_corrupt_rather_than_returned_as_mojibake() {
    let root = std::env::temp_dir().join(format!("downright-baseline-{}", Uuid::new_v4()));
    let root = FileUrl::from_path_is_directory(root.to_str().unwrap(), true);
    let history = root.appending_path_component_is_directory("history", true);
    let store = SnapshotStore::new(history.clone());
    let url = root.appending_path_component("report.md");

    let record = store.record(ORIGINAL, &url, SnapshotKind::Baseline).unwrap();
    store.wait_for_pending_writes();
    assert_eq!(store.content_for_hash(&record.hash), Content::Text(ORIGINAL.into()));

    let object_url = history
        .appending_path_component_is_directory("objects", true)
        .appending_path_component_is_directory(&record.hash[..2], true)
        .appending_path_component(&record.hash);
    let intact = std::fs::read(object_url.path()).unwrap();
    std::fs::write(object_url.path(), &intact[..intact.len() / 2]).unwrap();

    assert_eq!(store.content_for_hash(&record.hash), Content::Corrupt);
    assert_eq!(store.text_for_hash(&record.hash), None);
    assert_eq!(store.content_for_hash(&"f".repeat(64)), Content::Missing);
    store.forget(&url);
    store.wait_for_pending_writes();
    let _ = std::fs::remove_dir_all(root.path());
}

#[test]
fn visiting_a_mark_dims_it_rather_than_removing_it() {
    let tracker = ChangeTracker::new();
    tracker.apply(
        &[ChangeHunk::new(ChangeKind::Inserted, NSRange::new(0, 5), NSRange::new(0, 0), Vec::new())],
        "",
        "",
        true,
    );
    let mark = tracker.marks()[0].clone();
    tracker.mark_visited(mark.id);
    assert_eq!(tracker.marks().len(), 1, "a visited mark stays drawable so the reader can find it again");
    assert!(tracker.marks()[0].visited, "…dimmed");
    assert!(tracker.unread_marks().is_empty(), "…but no longer counted as unread");
    assert_eq!(tracker.next(0).map(|next| next.id), Some(mark.id), "…and still navigable");
}

#[test]
fn a_mark_is_visited_on_departure_not_on_arrival() {
    let tracker = ChangeTracker::new();
    tracker.set_dwell(1.5);
    tracker.apply(
        &[ChangeHunk::new(ChangeKind::Modified, NSRange::new(100, 20), NSRange::new(100, 20), Vec::new())],
        "",
        "",
        true,
    );
    let start = Date::now();
    tracker.note_visible_range(NSRange::new(80, 120), start);
    assert!(!tracker.marks()[0].visited, "arriving at a change must not dim it");
    tracker.note_visible_range(NSRange::new(80, 120), start.adding(0.2));
    assert!(!tracker.marks()[0].visited, "a glance is not a read");
    tracker.note_visible_range(NSRange::new(400, 120), start.adding(0.4));
    assert!(tracker.marks()[0].visited, "scrolling past it counts as having seen it");
    assert_eq!(tracker.marks().len(), 1);
}

#[test]
fn dwelling_on_a_mark_visits_it_without_scrolling_away() {
    let tracker = ChangeTracker::new();
    tracker.set_dwell(1.0);
    tracker.apply(
        &[ChangeHunk::new(ChangeKind::Modified, NSRange::new(10, 5), NSRange::new(10, 5), Vec::new())],
        "",
        "",
        true,
    );
    let start = Date::now();
    tracker.note_visible_range(NSRange::new(0, 200), start);
    assert!(!tracker.marks()[0].visited);
    tracker.note_visible_range(NSRange::new(0, 200), start.adding(1.2));
    assert!(tracker.marks()[0].visited);
}

#[test]
fn deletions_produce_a_hunk_the_ui_can_locate() {
    let old = "Keep this.\nDelete this whole line.\nKeep this too.\n";
    let new = "Keep this.\nKeep this too.\n";
    let hunks = TextDiff::hunks(old, new);
    let hunk = hunks.first().unwrap();
    assert_eq!(hunk.kind, ChangeKind::Deleted);
    assert_eq!(substring(old, hunk.old_range), "Delete this whole line.\n");
    assert_eq!(hunk.new_range.length, 0);
    let anchor = TextDiff::anchor_range(hunk, utf16_length(new));
    assert!(anchor.length > 0, "a zero-length range is clamped away by the overlay");
    assert!(anchor.upper_bound() <= utf16_length(new));

    let tracker = ChangeTracker::new();
    tracker.apply(&hunks, new, old, true);
    let mark = tracker.marks()[0].clone();
    assert_eq!(mark.kind, ChangeKind::Deleted);
    assert!(mark.range.length > 0);
    assert!(mark.range.upper_bound() <= utf16_length(new));
    assert_eq!(mark.deleted_text, "Delete this whole line.\n", "the ghost block needs the removed bytes");
}

#[test]
fn a_deletion_at_the_end_of_the_document_still_anchors() {
    let old = "Alpha.\nOmega.\n";
    let new = "Alpha.\n";
    let hunks = TextDiff::hunks(old, new);
    let hunk = hunks.first().unwrap();
    assert_eq!(hunk.kind, ChangeKind::Deleted);
    let anchor = TextDiff::anchor_range(hunk, utf16_length(new));
    assert_eq!(anchor.length, 1);
    assert!(anchor.upper_bound() <= utf16_length(new), "the anchor must stay inside the buffer");
}

#[test]
fn insertions_carry_word_ranges() {
    let old = "Opening line.\nClosing line.\n";
    let new = "Opening line.\nA brand new paragraph the agent added.\nClosing line.\n";
    let hunks = TextDiff::hunks(old, new);
    let hunk = hunks.first().unwrap();
    assert_eq!(hunk.kind, ChangeKind::Inserted);
    assert!(!hunk.word_ranges.is_empty());
    for range in &hunk.word_ranges {
        assert!(range.location >= hunk.new_range.location);
        assert!(range.upper_bound() <= hunk.new_range.upper_bound());
    }
    let highlighted: Vec<String> = hunk.word_ranges.iter().map(|range| substring(new, *range)).collect();
    let highlighted = highlighted.join(" ");
    assert!(highlighted.contains("brand new paragraph"));
    assert!(!highlighted.contains('\n'), "a background painted over a line terminator draws a full-width block");

    let tracker = ChangeTracker::new();
    tracker.apply(&hunks, new, old, true);
    assert!(!tracker.marks()[0].word_ranges.is_empty());
}

#[test]
fn persisted_marks_round_trip_through_document_state() {
    let tracker = ChangeTracker::new();
    tracker.apply(&TextDiff::hunks("a\ngone\nb\n", "a\nb\nnew\n"), "a\nb\nnew\n", "a\ngone\nb\n", true);
    assert!(!tracker.marks().is_empty());
    tracker.mark_visited(tracker.marks()[0].id);

    let mut state = DocumentState::new("/tmp/note.md");
    state.marks = tracker.persisted_marks();
    state.review_baseline_hash = "abc123".into();

    let decoded = DocumentState::decoded(&state.encoded()).unwrap();
    assert_eq!(decoded.review_baseline_hash, "abc123");
    let ids = |marks: &[upleft_app::ai::change_tracker::PersistedMark]| marks.iter().map(|mark| mark.id).collect::<Vec<_>>();
    assert_eq!(ids(&decoded.marks), ids(&state.marks));
    for (decoded_mark, original) in decoded.marks.iter().zip(&state.marks) {
        assert_eq!(decoded_mark.kind, original.kind);
        assert_eq!(decoded_mark.range, original.range);
        assert_eq!(decoded_mark.word_ranges, original.word_ranges);
        assert_eq!(decoded_mark.deleted_text, original.deleted_text);
        assert_eq!(decoded_mark.visited, original.visited);
        assert!(decoded_mark.created.time_interval_since(original.created).abs() < 1.0);
    }

    let restored = ChangeTracker::new();
    restored.restore(&decoded.marks, utf16_length("a\nb\nnew\n"), Date::now());
    assert_eq!(restored.marks().len(), tracker.marks().len());
    assert!(restored.marks()[0].visited);
    assert_eq!(restored.marks()[0].deleted_text, tracker.marks()[0].deleted_text);
}

/// A state file written before the baseline existed must not re-mark the
/// whole document on first launch after the upgrade.
#[test]
fn legacy_state_falls_back_to_the_disk_hash_as_its_baseline() {
    let legacy = concat!(
        r#"{"path":"/tmp/note.md","lastSeenHash":"deadbeef","anchor":"#,
        r#"{"headingSlug":"","headingIndex":0,"fractionThroughSection":0},"#,
        r#""mode":"live","zoomLevel":5,"foldedHeadings":[],"#,
        r#""expandedCodeBlocks":[],"collapsedCodeBlocks":[],"#,
        r#""lastOpened":"2024-01-01T00:00:00Z","sidebarVisible":false,"#,
        r#""selectionLocation":0,"selectionLength":0,"splitViewEnabled":false}"#
    );
    let decoded = DocumentState::decoded(legacy.as_bytes()).unwrap();
    assert_eq!(decoded.review_baseline_hash, "deadbeef");
    assert!(decoded.marks.is_empty());
}
