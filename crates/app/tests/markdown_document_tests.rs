//! Additions (no Swift counterpart file): `MarkdownDocument` and
//! `SiblingScanner` through their production paths — the real `FileWatcher`
//! feeding the document, own saves staying silent, rename following, the
//! version timeline — plus the scanner's cycling, grouping and rescans, and
//! the Objective-C class names the port must keep.

mod document_support;
mod main_thread;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use document_support::{Fixture, document, temporary_directory, unique, whole, write_atomically};
use objc2::runtime::AnyObject;
use upleft_app::ai::markdown_document::{ExternalEvent, Phase, SaveIntent};
use upleft_app::ai::sibling_scanner::SiblingScanner;
use upleft_app::ai::snapshot_store::{Content, SnapshotStore};
use upleft_foundation::url::FileUrl;

fn class_name(object: &AnyObject) -> String {
    object.class().name().to_str().unwrap().to_owned()
}

fn objective_c_class_names_match_swift() {
    let document = document();
    assert_eq!(class_name(document.as_ref()), "MarkdownDocument");
    assert_eq!(class_name(document.undo_manager().as_ref()), "MarkdownUndoManager");
}

fn an_external_write_reaches_the_document_through_the_watcher() {
    let fixture = Fixture::new("upleft-document-watch", "note.md", "# One\n\nBefore.\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    let applied = Rc::new(Cell::new(0));
    let counter = applied.clone();
    document.set_on_external_event(Some(move |event: &ExternalEvent| {
        if matches!(event, ExternalEvent::Applied { .. }) {
            counter.set(counter.get() + 1);
        }
    }));
    main_thread::sleep_pumping(Duration::from_millis(100));

    write_atomically(&fixture.url, b"# One\n\nAfter.\n");
    assert!(
        main_thread::pump_until(|| document.text() == "# One\n\nAfter.\n", Duration::from_secs(5)),
        "the watcher never delivered the external write"
    );
    assert_eq!(applied.get(), 1);
    assert_eq!(document.presentation_state().phase, Phase::ChangedOnDisk);
    assert_eq!(document.presentation_state().detail.as_deref(), Some("1 changed block"));
    assert!(!document.is_dirty());
    assert_eq!(document.changes().count(), 1);
    document.close();
}

fn an_own_save_does_not_come_back_as_an_external_change() {
    let fixture = Fixture::new("upleft-document-own-save", "note.md", "before\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    let events = Rc::new(Cell::new(0));
    let counter = events.clone();
    document.set_on_external_event(Some(move |_: &ExternalEvent| counter.set(counter.get() + 1)));
    main_thread::sleep_pumping(Duration::from_millis(100));

    assert!(document.replace(whole(&document), "after\n", Some("Typing")));
    // Let the edit's storage-delegate callback run first, as it does between
    // a keystroke and a save in the app. (A save in the same main-queue turn
    // as the edit is re-marked dirty when that callback lands, in Swift too.)
    main_thread::sleep_pumping(Duration::from_millis(50));
    document.save(SaveIntent::Normal).unwrap();
    assert_eq!(document.presentation_state().phase, Phase::Saved);
    main_thread::sleep_pumping(Duration::from_millis(2_000));
    assert_eq!(events.get(), 0, "our own save came back as an external event");
    assert_eq!(document.presentation_state().phase, Phase::Neutral, "the saved cue settles back to neutral");
    document.close();
}

fn a_rename_on_disk_moves_the_document_identity() {
    let fixture = Fixture::new("upleft-document-rename", "note.md", "# Title\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    let renamed: Rc<RefCell<Option<FileUrl>>> = Rc::default();
    let sink = renamed.clone();
    document.set_on_file_renamed(Some(move |url: &FileUrl| *sink.borrow_mut() = Some(url.clone())));
    main_thread::sleep_pumping(Duration::from_millis(100));

    let target = fixture.root.appending_path_component("moved.md");
    std::fs::rename(fixture.url.path(), target.path()).unwrap();
    assert!(main_thread::pump_until(|| renamed.borrow().is_some(), Duration::from_secs(5)), "rename never reported");
    let expected = target.resolving_symlinks_in_path();
    assert_eq!(renamed.borrow().as_ref(), Some(&expected));
    assert_eq!(document.url(), Some(expected.clone()));
    assert_eq!(document.display_name(), "moved");
    assert_eq!(document.state().path, expected.path());
    document.close();
}

fn versions_record_open_save_and_restore() {
    let fixture = Fixture::new("upleft-document-versions", "note.md", "first\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    assert!(document.replace(whole(&document), "second\n", Some("Typing")));
    document.save(SaveIntent::Normal).unwrap();
    SnapshotStore::shared().wait_for_pending_writes();

    let versions = document.versions();
    assert!(versions.len() >= 2, "baseline and local save are both recorded");
    let first = versions.iter().find(|version| document.content(version) == Content::Text("first\n".into())).cloned();
    let first = first.expect("the opening text is in the timeline");
    assert!(document.restore(&first));
    assert_eq!(document.text(), "first\n");
    assert!(document.is_dirty(), "restoring is an undoable edit, not a save");
    assert_eq!(document.review_baseline_text(), "first\n", "choosing a version is a review decision");
    document.undo_manager().undo();
    assert_eq!(document.text(), "second\n");
    document.close();
}

// MARK: - SiblingScanner

fn sibling_directory(files: &[(&str, &str)]) -> FileUrl {
    let root = temporary_directory().appending_path_component_is_directory(&format!("upleft-siblings-{}", unique()), true);
    std::fs::create_dir_all(root.path()).unwrap();
    for (name, contents) in files {
        std::fs::write(root.appending_path_component(name).path(), contents).unwrap();
    }
    root
}

fn scanner_cycles_and_groups_in_scan_order() {
    let root = sibling_directory(&[("A.md", "a"), ("B.md", "b"), ("C.markdown", "c"), ("skip.txt", "t")]);
    let docs = root.appending_path_component_is_directory("docs", true);
    std::fs::create_dir_all(docs.path()).unwrap();
    std::fs::write(docs.appending_path_component("D.md").path(), "d").unwrap();
    let current = root.appending_path_component("B.md");

    let scanner = SiblingScanner::new(&current, vec!["docs".to_owned(), "missing".to_owned()]);
    let siblings = scanner.siblings();
    assert_eq!(siblings.len(), 4);
    assert!(siblings[0].is_current && siblings[0].display_name == "B");
    for pair in siblings[1..].windows(2) {
        assert!(pair[0].modified >= pair[1].modified, "newest first after the current document");
    }

    let order: Vec<FileUrl> = siblings.iter().map(|sibling| sibling.url.clone()).collect();
    assert_eq!(scanner.neighbour(&current, true), Some(order[1].clone()));
    assert_eq!(scanner.neighbour(&current, false), Some(order[3].clone()));
    assert_eq!(scanner.neighbour(&root.appending_path_component("elsewhere.md"), true), Some(order[0].clone()));

    let groups = scanner.grouped();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].0, None);
    assert_eq!(groups[1].0.as_deref(), Some("docs"));
    assert_eq!(groups[1].1.len(), 1);
    drop(scanner);
    let _ = std::fs::remove_dir_all(root.path());
}

fn scanner_picks_up_a_new_sibling_through_its_directory_watch() {
    let root = sibling_directory(&[("CURRENT.md", "# Current\n")]);
    let current = root.appending_path_component("CURRENT.md");
    let scanner = SiblingScanner::new(&current, Vec::new());
    let changes = Rc::new(Cell::new(0));
    let counter = changes.clone();
    scanner.set_on_change(Some(move || counter.set(counter.get() + 1)));
    // The initial asynchronous pass lands first.
    main_thread::sleep_pumping(Duration::from_millis(200));
    assert_eq!(scanner.siblings().len(), 1);

    std::fs::write(root.appending_path_component("SIXTH.md").path(), "# Written later\n").unwrap();
    assert!(
        main_thread::pump_until(|| scanner.siblings().len() == 2, Duration::from_secs(5)),
        "the new sibling never appeared"
    );
    assert!(changes.get() >= 1);
    drop(scanner);
    let _ = std::fs::remove_dir_all(root.path());
}

fn main() {
    document_support::sandbox();
    main_thread::run(&[
        ("objective_c_class_names_match_swift", objective_c_class_names_match_swift),
        ("an_external_write_reaches_the_document_through_the_watcher", an_external_write_reaches_the_document_through_the_watcher),
        ("an_own_save_does_not_come_back_as_an_external_change", an_own_save_does_not_come_back_as_an_external_change),
        ("a_rename_on_disk_moves_the_document_identity", a_rename_on_disk_moves_the_document_identity),
        ("versions_record_open_save_and_restore", versions_record_open_save_and_restore),
        ("scanner_cycles_and_groups_in_scan_order", scanner_cycles_and_groups_in_scan_order),
        ("scanner_picks_up_a_new_sibling_through_its_directory_watch", scanner_picks_up_a_new_sibling_through_its_directory_watch),
    ]);
    document_support::remove_sandbox();
}
