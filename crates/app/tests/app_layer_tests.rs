//! Port of the store tests in `Tests/DownrightAppTests/AppLayerTests.swift`:
//! scroll anchoring, change marks, the snapshot store, reading state,
//! preferences values, path resolution, editor deep links, and jump history.
//!
//! Not ported here (other owners or a window): `failedSaveReturnsErrorAndPreservesBuffer`
//! and `changeMarksDoNotLeakAcrossInPlaceReopen` (MarkdownDocument, ported
//! later); `executableTargetsAreClassifiedForRevealInsteadOfLaunch`
//! (DocumentTypes); the `find*` tests (FindEngine); the key binding tests
//! (Keybindings, palette port); the `htmlExport*` tests (HTMLExporter);
//! `plainTextRenderingStripsMarkup`, `slugsMatchGitHubConventions`,
//! `siblingScannerFindsMarkdownInDocsSubdirectory` (not in this layer).
//! `pathResolverWarmsCacheAndReturnsOnMainQueue` is ported without its
//! main-thread assertion: a test binary does not service the main queue, so
//! the test waits for the warmed cache instead of the completion.

use std::time::{Duration, Instant};

use upleft_app::ai::change_tracker::ChangeTracker;
use upleft_app::ai::document_state_store::{DocumentState, DocumentStateStore, ScrollAnchor, ScrollAnchoring};
use upleft_app::ai::path_resolver::{ExternalEditor, PathResolver};
use upleft_app::ai::snapshot_store::{Content, SnapshotKind, SnapshotStore};
use upleft_app::support::jump_history::{Entry, JumpHistory};
use upleft_app::support::preferences::{ThemePreferenceSlot, Values};
use upleft_core::contracts::{ChangeHunk, ChangeKind, Uuid, ZoomLevel};
use upleft_core::model::PathToken;
use upleft_core::ns_range::NSRange;
use upleft_core::parser::MarkdownParser;
use upleft_foundation::date::Date;
use upleft_foundation::json_encoder::{self, JsonValue, OutputFormatting};
use upleft_foundation::json_serialization::{self, AnyJson, ReadingOptions, WritingOptions};
use upleft_foundation::url::FileUrl;
use upleft_render::render_contracts::{BodyPreset, RenderMode};

fn temporary(prefix: &str) -> FileUrl {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::new_v4().hyphenated().to_string().to_uppercase()));
    FileUrl::from_path_is_directory(path.to_str().unwrap(), true)
}

fn remove(url: &FileUrl) {
    let _ = std::fs::remove_dir_all(url.path());
}

fn hunk(kind: ChangeKind, new: (isize, isize), old: (isize, isize)) -> ChangeHunk {
    ChangeHunk::new(kind, NSRange::new(new.0, new.1), NSRange::new(old.0, old.1), Vec::new())
}

// MARK: - Scroll anchoring (§8.1, §8.2)

#[test]
fn anchor_survives_insertion_above_it() {
    let before = "# Title\n\nIntro paragraph.\n\n## Second section\n\nThe paragraph the reader is looking at.\n\n## Third section\n\nTrailing content.";
    let after = "# Title\n\nIntro paragraph.\n\n## A section the agent inserted\n\nSeveral new paragraphs of content that did not exist before.\n\nMore new content, pushing everything below it down the file.\n\n## Second section\n\nThe paragraph the reader is looking at.\n\n## Third section\n\nTrailing content.";
    let old_document = MarkdownParser::parse(before);
    let new_document = MarkdownParser::parse(after);
    let second = old_document.headings.iter().find(|heading| heading.title == "Second section").unwrap();
    let reading_offset = second.section_range.location + second.section_range.length / 2;
    let anchor = ScrollAnchoring::anchor(reading_offset, &old_document);
    let restored = ScrollAnchoring::offset(&anchor, &new_document);
    let new_second = new_document.headings.iter().find(|heading| heading.title == "Second section").unwrap();
    assert!(
        new_second.section_range.contains(restored),
        "reading position must land back inside the same section, not at the same byte offset"
    );
}

#[test]
fn anchor_falls_back_when_heading_disappears() {
    let document = MarkdownParser::parse("# Only heading\n\nBody.\n");
    let anchor = ScrollAnchor { heading_slug: "long-gone".into(), heading_index: 4, fraction_through_section: 0.5 };
    let offset = ScrollAnchoring::offset(&anchor, &document);
    assert!(offset >= 0 && offset <= document.length);
}

// MARK: - Change marks (§8.1)

#[test]
fn change_marks_shift_with_edits_and_drop_when_overwritten() {
    let tracker = ChangeTracker::new();
    tracker.apply(
        &[hunk(ChangeKind::Modified, (100, 20), (100, 18)), hunk(ChangeKind::Inserted, (300, 40), (300, 0))],
        "",
        "",
        true,
    );
    assert_eq!(tracker.count(), 2);
    tracker.adjust(NSRange::new(10, 0), 5);
    assert_eq!(tracker.marks()[0].range.location, 105);
    assert_eq!(tracker.marks()[1].range.location, 305);
    tracker.adjust(NSRange::new(106, 4), 0);
    assert_eq!(tracker.count(), 1);
    assert_eq!(tracker.marks()[0].range.location, 305);
}

#[test]
fn change_navigation_wraps_around() {
    let tracker = ChangeTracker::new();
    tracker.apply(
        &[hunk(ChangeKind::Modified, (50, 10), (50, 10)), hunk(ChangeKind::Modified, (200, 10), (200, 10))],
        "",
        "",
        true,
    );
    assert_eq!(tracker.next(0).map(|mark| mark.range.location), Some(50));
    assert_eq!(tracker.next(100).map(|mark| mark.range.location), Some(200));
    assert_eq!(tracker.next(900).map(|mark| mark.range.location), Some(50), "wraps to the first mark");
    assert_eq!(tracker.previous(100).map(|mark| mark.range.location), Some(50));
    assert_eq!(tracker.previous(0).map(|mark| mark.range.location), Some(200), "wraps to the last mark");
}

#[test]
fn visited_marks_stop_drawing() {
    let tracker = ChangeTracker::new();
    tracker.apply(&[hunk(ChangeKind::Inserted, (0, 5), (0, 0))], "", "", true);
    let id = tracker.marks()[0].id;
    assert_eq!(tracker.visible_marks().len(), 1);
    tracker.mark_visited(id);
    assert!(tracker.visible_marks().is_empty());
    assert_eq!(tracker.count(), 1, "the mark still exists for navigation, it just stops drawing");
}

// MARK: - Snapshot store (§8.3)

#[test]
fn snapshot_store_deduplicates_and_restores() {
    let root = temporary("downright-store");
    let store = SnapshotStore::new(root.appending_path_component("history"));
    let url = root.appending_path_component("document.md");
    let first = "# One\n\nBody.\n";
    let second = "# One\n\nBody, revised.\n";
    assert!(store.record(first, &url, SnapshotKind::Baseline).is_some());
    assert!(store.record(first, &url, SnapshotKind::External).is_none(), "identical content must not create a version");
    assert!(store.record(second, &url, SnapshotKind::External).is_some());
    store.wait_for_pending_writes();
    let versions = store.versions(&url);
    assert_eq!(versions.len(), 2);
    assert_eq!(store.text(&versions[0]).as_deref(), Some(first));
    assert_eq!(store.text(&versions[1]).as_deref(), Some(second));
    remove(&root);
}

fn snapshot_index_file(url: &FileUrl, history_directory: &FileUrl) -> FileUrl {
    history_directory
        .appending_path_component_is_directory("index", true)
        .appending_path_component(&(SnapshotStore::document_key(url) + ".json"))
}

fn write_snapshot_index(path: &str, age: f64, file: &FileUrl) {
    let stamp = Date::now().adding(-age).iso8601();
    let encoded_path = json_encoder::encode_string(&JsonValue::from(path), OutputFormatting::DEFAULT);
    let json = format!(
        "{{\"path\":{encoded_path},\"versions\":[{{\"hash\":\"{}\",\"date\":\"{stamp}\",\"byteCount\":12,\"kind\":\"external\"}}]}}",
        "a".repeat(64)
    );
    std::fs::create_dir_all(file.deleting_last_path_component().path()).unwrap();
    std::fs::write(file.path(), json).unwrap();
}

fn age_snapshot_index(file: &FileUrl, age: f64) {
    let data = std::fs::read(file.path()).unwrap();
    let mut json = json_serialization::json_object(&data, ReadingOptions::default()).unwrap();
    let members = json.as_object_mut().unwrap();
    let mut versions = members.iter().find(|(key, _)| key == "versions").unwrap().1.clone();
    if let AnyJson::Array(list) = &mut versions {
        let last = list.last_mut().unwrap().as_object_mut().unwrap();
        AnyJson::set(last, "date", AnyJson::String(Date::now().adding(-age).iso8601()));
    }
    AnyJson::set(members, "versions", versions);
    std::fs::write(file.path(), json_serialization::data(&json, WritingOptions::default()).unwrap()).unwrap();
}

#[test]
fn prune_keeps_newest_history_for_missing_document() {
    let root = temporary("downright-prune");
    let history = root.appending_path_component_is_directory("history", true);
    let store = SnapshotStore::new(history.clone());
    let url = root.appending_path_component("gone.md");
    let file = snapshot_index_file(&url, &history);
    let text = "gone but cached\n";
    assert!(store.record(text, &url, SnapshotKind::Baseline).is_some());
    store.wait_for_pending_writes();
    age_snapshot_index(&file, 90.0 * 24.0 * 60.0 * 60.0);
    store.prune_one_generation_for_testing();
    store.prune_one_generation_for_testing();
    assert!(std::path::Path::new(&file.path()).exists());
    assert_eq!(store.versions(&url).len(), 1, "the newest version survives age pruning");
    assert_eq!(store.text(&store.versions(&url)[0]).as_deref(), Some(text));
    remove(&root);
}

fn set_permissions(path: &str, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn prune_keeps_objects_when_index_directory_cannot_be_listed() {
    let root = temporary("downright-prune-index-directory");
    let history = root.appending_path_component_is_directory("history", true);
    let index_directory = history.appending_path_component_is_directory("index", true);
    let store = SnapshotStore::new(history.clone());
    let document = root.appending_path_component("document.md");
    let text = "must survive an unreadable index directory\n";
    let record = store.record(text, &document, SnapshotKind::Baseline).unwrap();
    store.wait_for_pending_writes();
    set_permissions(&index_directory.path(), 0);
    store.prune_one_generation_for_testing();
    set_permissions(&index_directory.path(), 0o700);
    assert_eq!(store.content(&record), Content::Text(text.into()));
    remove(&root);
}

#[test]
fn prune_keeps_objects_when_any_index_is_corrupt() {
    let root = temporary("downright-prune-corrupt-index");
    let history = root.appending_path_component_is_directory("history", true);
    let store = SnapshotStore::new(history.clone());
    let document = root.appending_path_component("document.md");
    let text = "must survive a corrupt index\n";
    let record = store.record(text, &document, SnapshotKind::Baseline).unwrap();
    store.wait_for_pending_writes();
    std::fs::write(snapshot_index_file(&document, &history).path(), "not json").unwrap();
    store.prune_one_generation_for_testing();
    assert_eq!(store.content(&record), Content::Text(text.into()));
    remove(&root);
}

#[test]
fn prune_keeps_objects_when_any_index_file_is_unreadable() {
    let root = temporary("downright-prune-unreadable-index");
    let history = root.appending_path_component_is_directory("history", true);
    let store = SnapshotStore::new(history.clone());
    let document = root.appending_path_component("document.md");
    let text = "must survive an unreadable index file\n";
    let record = store.record(text, &document, SnapshotKind::Baseline).unwrap();
    store.wait_for_pending_writes();
    let index = snapshot_index_file(&document, &history);
    set_permissions(&index.path(), 0);
    store.prune_one_generation_for_testing();
    set_permissions(&index.path(), 0o600);
    assert_eq!(store.content(&record), Content::Text(text.into()));
    remove(&root);
}

/// Regression: a present-but-undecodable index used to be treated as an
/// empty one, so the next record overwrote it and the following prune
/// garbage-collected every object the old index referenced.
#[test]
fn record_never_overwrites_an_unreadable_index() {
    let root = temporary("downright-record-unreadable");
    let history = root.appending_path_component_is_directory("history", true);
    let store = SnapshotStore::new(history.clone());
    let document = root.appending_path_component("document.md");
    let file = snapshot_index_file(&document, &history);
    let first = "first version\n";
    assert!(store.record(first, &document, SnapshotKind::Baseline).is_some());
    store.wait_for_pending_writes();
    let versions_before = store.versions(&document);
    assert_eq!(versions_before.len(), 1);
    std::fs::write(file.path(), "not json").unwrap();
    let corrupted = std::fs::read(file.path()).unwrap();
    assert!(store.record("second version\n", &document, SnapshotKind::External).is_some());
    store.wait_for_pending_writes();
    assert_eq!(std::fs::read(file.path()).unwrap(), corrupted, "the undecodable index file must be left exactly as it was");
    assert_eq!(
        store.content_for_hash(&versions_before[0].hash),
        Content::Text(first.into()),
        "the recorded object must survive a prune that fails closed"
    );
    remove(&root);
}

/// The `forget` sweep shares prune's rule: with an undecodable index in
/// play, reference knowledge is incomplete and nothing may be deleted.
#[test]
fn forget_does_not_sweep_objects_behind_an_unreadable_index() {
    let root = temporary("downright-forget-unreadable");
    let history = root.appending_path_component_is_directory("history", true);
    let store = SnapshotStore::new(history.clone());
    let kept = root.appending_path_component("kept.md");
    let kept_text = "kept behind a corrupt index\n";
    assert!(store.record(kept_text, &kept, SnapshotKind::Baseline).is_some());
    store.wait_for_pending_writes();
    let forgotten = root.appending_path_component("forgotten.md");
    assert!(store.record("forgotten\n", &forgotten, SnapshotKind::Baseline).is_some());
    store.wait_for_pending_writes();
    std::fs::write(snapshot_index_file(&kept, &history).path(), "not json").unwrap();
    store.forget(&forgotten);
    store.wait_for_pending_writes();
    assert!(store.versions(&kept).is_empty(), "the corrupt index reads as no versions");
    assert_eq!(
        store.content_for_hash(&SnapshotStore::hash(kept_text)),
        Content::Text(kept_text.into()),
        "its objects must not be swept while its index is unreadable"
    );
    remove(&root);
}

#[test]
fn prune_keeps_old_history_for_live_document() {
    let root = temporary("downright-prune");
    std::fs::create_dir_all(root.path()).unwrap();
    let history = root.appending_path_component_is_directory("history", true);
    let store = SnapshotStore::new(history.clone());
    let url = root.appending_path_component("live.md");
    std::fs::write(url.path(), "# Live\n").unwrap();
    let file = snapshot_index_file(&url, &history);
    write_snapshot_index(&url.path(), 90.0 * 24.0 * 60.0 * 60.0, &file);
    store.prune_one_generation_for_testing();
    assert!(std::path::Path::new(&file.path()).exists());
    remove(&root);
}

/// The per-document size budget trims a document's own history oldest first
/// and never drops its newest version.
#[test]
fn per_document_byte_cap_evicts_oldest_first_and_keeps_newest() {
    let root = temporary("downright-cap-perdoc");
    let history = root.appending_path_component_is_directory("history", true);
    let store = SnapshotStore::new(history);
    let document = root.appending_path_component("document.md");
    store.set_maximum_bytes_per_document(8);
    let first = "first version of the document body\n";
    let second = "second version of the document body, revised\n";
    let third = "third version of the document body, revised again\n";
    assert!(store.record(first, &document, SnapshotKind::Baseline).is_some());
    assert!(store.record(second, &document, SnapshotKind::External).is_some());
    assert!(store.record(third, &document, SnapshotKind::External).is_some());
    store.wait_for_pending_writes();
    store.prune_one_generation_for_testing();
    let versions = store.versions(&document);
    assert_eq!(versions.len(), 1, "the per-document budget sheds older versions");
    assert_eq!(store.text(&versions[0]).as_deref(), Some(third), "the newest version survives the budget");
    remove(&root);
}

/// The global backstop must shed history fairly: no single document may lose
/// its newest version.
#[test]
fn global_byte_cap_keeps_every_documents_newest_version() {
    let root = temporary("downright-cap-global");
    let history = root.appending_path_component_is_directory("history", true);
    let store = SnapshotStore::new(history);
    store.set_maximum_bytes(0);
    for name in ["a.md", "b.md"] {
        let url = root.appending_path_component(name);
        for index in 0..3 {
            let kind = if index == 0 { SnapshotKind::Baseline } else { SnapshotKind::External };
            assert!(store.record(&format!("version {index} of {name}\n"), &url, kind).is_some());
        }
    }
    store.wait_for_pending_writes();
    store.prune_one_generation_for_testing();
    for name in ["a.md", "b.md"] {
        let url = root.appending_path_component(name);
        let versions = store.versions(&url);
        assert_eq!(versions.len(), 1, "{name} keeps exactly its newest version");
        assert_eq!(store.text(&versions[0]), Some(format!("version 2 of {name}\n")));
    }
    remove(&root);
}

#[test]
fn prune_does_not_rewrite_unchanged_index() {
    let root = temporary("downright-prune");
    std::fs::create_dir_all(root.path()).unwrap();
    let history = root.appending_path_component_is_directory("history", true);
    let store = SnapshotStore::new(history.clone());
    let url = root.appending_path_component("stable.md");
    std::fs::write(url.path(), "# Stable\n").unwrap();
    let file = snapshot_index_file(&url, &history);
    store.record("# Stable\n", &url, SnapshotKind::Baseline);
    store.wait_for_pending_writes();
    let mark = std::time::UNIX_EPOCH + Duration::from_secs(1_000_000);
    std::fs::File::options().write(true).open(file.path()).unwrap().set_modified(mark).unwrap();
    store.prune_one_generation_for_testing();
    assert_eq!(std::fs::metadata(file.path()).unwrap().modified().unwrap(), mark);
    remove(&root);
}

#[test]
fn reading_state_persists_when_document_is_missing() {
    let root = temporary("downright-state");
    let support = root.appending_path_component_is_directory("support", true);
    let store = DocumentStateStore::new(support.clone());
    let document = root.appending_path_component("missing.md");
    let state_file = support
        .appending_path_component_is_directory("state", true)
        .appending_path_component(&(SnapshotStore::document_key(&document) + ".json"));
    let mut state = DocumentState::new(&document.path());
    state.last_opened = Date::from_reference(-63_114_076_800.0);
    state.selection_location = 42;
    store.save(&state, &document);
    assert!(std::path::Path::new(&state_file.path()).exists());
    let reopened = DocumentStateStore::new(support);
    assert_eq!(reopened.state(&document).selection_location, 42);
    remove(&root);
}

#[test]
fn content_hash_is_stable() {
    assert_eq!(SnapshotStore::hash("abc"), SnapshotStore::hash("abc"));
    assert_ne!(SnapshotStore::hash("abc"), SnapshotStore::hash("abd"));
    assert_eq!(SnapshotStore::hash("").len(), 64, "sha256 hex");
}

#[test]
fn document_state_keeps_selection_and_split_view() {
    let mut state = DocumentState::new("/tmp/note.md");
    state.selection_location = 42;
    state.selection_length = 7;
    state.split_view_enabled = true;
    state.mode = RenderMode::Source;
    state.zoom_level = ZoomLevel::Skeleton;
    let decoded = DocumentState::decoded(&state.encoded()).unwrap();
    assert_eq!(decoded.selection_location, 42);
    assert_eq!(decoded.selection_length, 7);
    assert!(decoded.split_view_enabled);
    assert_eq!(decoded.mode, RenderMode::Live, "transient Source Focus must not restore");
    assert_eq!(decoded.zoom_level, ZoomLevel::Everything, "editable documents must reopen without hidden prose");
}

#[test]
fn preferences_values_round_trip_every_setting() {
    let mut values = Values::default();
    values.theme_name = "Light".into();
    values.dark_theme_name = "Dark".into();
    values.follows_system_appearance = false;
    values.typography.preset = BodyPreset::Working;
    values.typography.body_size = 18.0;
    values.typography.scale_ratio = 1.333;
    values.typography.line_height_multiple = 1.7;
    values.typography.measure_characters = 72.0;
    values.typography.mono_family = "Menlo".into();
    values.typography.mono_size_adjust = 1.0;
    values.typography.mono_ligatures = true;
    values.typography.optical_margins = false;
    values.typography.math_scale = 1.2;
    values.text_size_adjustment = 2.0;
    values.typographic_substitution = true;
    values.show_invisibles = true;
    values.typewriter_scrolling = true;
    values.focus_mode = true;
    values.code_block_collapse_threshold = 40;
    values.default_mode = RenderMode::Source;
    values.restore_session = false;
    values.external_editor = ExternalEditor::Vscode;
    values.resolve_path_tokens = false;
    values.sibling_scan_directories = vec!["custom".into()];
    values.history_maximum_days = 90;
    values.history_maximum_megabytes = 900;
    values.watch_files = false;
    values.vim_keys = true;
    values.reveal_markers_at_all_cursors = true;
    values.large_file_threshold_megabytes = 20;
    let data = json_encoder::encode(&values.encode(), OutputFormatting::DEFAULT);
    let decoded = Values::decoded(&data).unwrap();
    assert_eq!(decoded, values);
}

#[test]
fn legacy_preferences_restore_system_appearance_following() {
    let decoded = Values::decoded(br#"{"followsSystemAppearance":false}"#).unwrap();
    assert!(decoded.follows_system_appearance);
}

#[test]
fn choosing_a_theme_does_not_change_the_appearance_mode() {
    let mut values = Values::default();
    values.follows_system_appearance = true;
    values.select_theme("Solarized Light", ThemePreferenceSlot::Light);
    values.select_theme("Nord", ThemePreferenceSlot::Dark);
    assert_eq!(values.theme_name, "Solarized Light");
    assert_eq!(values.dark_theme_name, "Nord");
    assert!(values.follows_system_appearance);
}

// MARK: - Path resolution (§8.4)

#[test]
fn path_resolver_distinguishes_present_from_missing() {
    let root = temporary("downright-paths");
    let source = root.appending_path_component_is_directory("src", true);
    std::fs::create_dir_all(source.path()).unwrap();
    std::fs::write(source.appending_path_component("real.ts").path(), "// real\n").unwrap();
    let document_url = root.appending_path_component("PLAN.md");
    std::fs::write(document_url.path(), "# Plan\n").unwrap();
    let resolver = PathResolver::new(Some(&document_url));
    assert!(resolver.resolve(&PathToken::new("src/real.ts", None, None)).exists);
    assert!(resolver.resolve(&PathToken::new("src/real.ts", Some(42), None)).exists);
    assert_eq!(resolver.resolve(&PathToken::new("src/real.ts", Some(42), None)).line, Some(42));
    assert!(
        !resolver.resolve(&PathToken::new("src/auth/session.ts", Some(42), None)).exists,
        "a file the agent claims to have touched that isn't there must resolve as missing"
    );
    remove(&root);
}

fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    condition()
}

#[test]
fn path_resolver_warms_cache_and_returns_on_main_queue() {
    let root = temporary("downright-path-warm");
    std::fs::create_dir_all(root.path()).unwrap();
    std::fs::write(root.appending_path_component("ready.md").path(), "ready\n").unwrap();
    let resolver = PathResolver::new(Some(&root.appending_path_component("document.md")));
    assert!(resolver.cached_resolution(&PathToken::new("ready.md", None, None)).is_none());
    resolver.warm(
        &[
            PathToken::new("ready.md", None, None),
            PathToken::new("ready.md", Some(9), None),
            PathToken::new("missing.md", None, None),
        ],
        || {},
    );
    assert!(wait_until(|| resolver.cached_resolution(&PathToken::new("missing.md", None, None)).is_some()));
    assert!(resolver.resolve(&PathToken::new("ready.md", Some(9), None)).exists);
    assert_eq!(resolver.resolve(&PathToken::new("ready.md", Some(9), None)).line, Some(9));
    assert!(!resolver.resolve(&PathToken::new("missing.md", None, None)).exists);
    assert_eq!(resolver.cached_resolution(&PathToken::new("ready.md", None, None)).map(|hit| hit.exists), Some(true));

    std::fs::remove_file(root.appending_path_component("ready.md").path()).unwrap();
    resolver.warm(&[PathToken::new("ready.md", None, None)], || {});
    assert_eq!(
        resolver.cached_resolution(&PathToken::new("ready.md", None, None)).map(|hit| hit.exists),
        Some(true),
        "a current-generation cache hit must not be restatted on every reparse"
    );
    remove(&root);
}

#[test]
fn newly_parsed_path_token_remains_uncached_until_background_warm_completes() {
    let root = temporary("downright-new-path-token");
    std::fs::create_dir_all(root.path()).unwrap();
    let resolver = PathResolver::new(Some(&root.appending_path_component("document.md")));
    let new_token = PathToken::new("generated.md", None, None);
    assert!(resolver.cached_resolution(&new_token).is_none());
    std::fs::write(root.appending_path_component(&new_token.raw_path).path(), "generated\n").unwrap();
    resolver.warm(std::slice::from_ref(&new_token), || {});
    assert!(wait_until(|| resolver.cached_resolution(&new_token).is_some()));
    assert_eq!(resolver.cached_resolution(&new_token).map(|hit| hit.exists), Some(true));
    remove(&root);
}

#[test]
fn git_root_discovery_stops_at_the_repository() {
    let root = temporary("downright-git");
    let nested = root.appending_path_component_is_directory("docs/deep", true);
    std::fs::create_dir_all(nested.path()).unwrap();
    std::fs::create_dir_all(root.appending_path_component(".git").path()).unwrap();
    let found = PathResolver::find_git_root(&nested).map(|url| url.standardized_file_url().path());
    assert!(
        found == Some(root.standardized_file_url().resolving_symlinks_in_path().path())
            || found == Some(root.standardized_file_url().path())
    );
    remove(&root);
}

/// An editor deep link carries the path inside a URL, so a directory named
/// `Design #2` or `100%` has to survive the trip.
#[test]
fn editor_deep_links_percent_encode_awkward_paths() {
    for editor in [ExternalEditor::Vscode, ExternalEditor::Cursor, ExternalEditor::Zed] {
        let scheme = match editor {
            ExternalEditor::Vscode => "vscode",
            ExternalEditor::Cursor => "cursor",
            _ => "zed",
        };
        let plain = editor.url(&FileUrl::from_path("/src/auth/session.ts"), Some(42)).unwrap();
        assert_eq!(plain.absoluteString().unwrap().to_string(), format!("{scheme}://file/src/auth/session.ts:42"));
        for awkward in ["/notes/Design #2/plan.md", "/notes/why?/plan.md", "/notes/100%/plan.md", "/notes/My Notes/plan.md"] {
            let url = editor.url(&FileUrl::from_path(awkward), Some(7)).unwrap();
            assert!(url.fragment().is_none(), "{awkward} leaked a fragment");
            assert!(url.query().is_none(), "{awkward} leaked a query");
            assert_eq!(url.path().unwrap().to_string(), format!("{awkward}:7"), "{awkward} did not round-trip");
            assert!(url.absoluteString().unwrap().to_string().ends_with(":7"), "{awkward} lost its line suffix");
        }
    }
    assert!(ExternalEditor::Xcode.url(&FileUrl::from_path("/a.md"), Some(1)).is_none());
    assert!(ExternalEditor::SystemDefault.url(&FileUrl::from_path("/a.md"), Some(1)).is_none());
    let no_line = ExternalEditor::Zed.url(&FileUrl::from_path("/a b.md"), None).unwrap();
    assert_eq!(no_line.absoluteString().unwrap().to_string(), "zed://file/a%20b.md");
}

// MARK: - Jump history (§7.1)

#[test]
fn jump_history_behaves_like_a_browser() {
    let mut history = JumpHistory::new();
    let from = Entry::new(None, 0, "start");
    history.record(Some(from), Entry::new(None, 100, "a"));
    history.record(Some(Entry::new(None, 100, "a")), Entry::new(None, 200, "b"));
    assert!(history.can_go_back());
    assert!(!history.can_go_forward());
    assert_eq!(history.go_back().map(|entry| entry.offset), Some(100));
    assert!(history.can_go_forward());
    assert_eq!(history.go_forward().map(|entry| entry.offset), Some(200));
    let _ = history.go_back();
    history.record(Some(Entry::new(None, 100, "a")), Entry::new(None, 300, "c"));
    assert!(!history.can_go_forward());
}
