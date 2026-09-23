//! `Preferences`' load, recovery and persistence (Preferences.swift's
//! `read(contentsOf:)`, `moveAside(_:)`, `update(_:)` and `persist()`), on
//! `Preferences::for_testing`, which never touches the global Quick Look
//! preferences. No Swift test covers these directly (the Swift ones reach
//! `Preferences.shared` through windows), and the conformance suite cannot
//! either: `Preferences.shared` writes the user's real global preferences.

use std::sync::{Arc, Mutex};

use upleft_app::ai::path_resolver::ExternalEditor;
use upleft_app::ai::snapshot_store::SnapshotStore;
use upleft_app::support::preferences::{Load, Preferences, Values};
use upleft_core::contracts::Uuid;
use upleft_foundation::url::FileUrl;

fn sandbox() -> FileUrl {
    let path = std::env::temp_dir().join(format!("downright-preferences-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&path).unwrap();
    FileUrl::from_path_is_directory(path.to_str().unwrap(), true)
}

#[test]
fn a_missing_file_is_a_first_run_with_the_best_available_editor() {
    let root = sandbox();
    let preferences = Preferences::for_testing(root.appending_path_component("preferences.json"), None);
    assert_eq!(preferences.load(), Load::Absent);
    assert!(preferences.is_first_run());
    assert_eq!(preferences.values().external_editor, ExternalEditor::best_available());
    assert!(!std::path::Path::new(&root.appending_path_component("preferences.json").path()).exists(), "loading writes nothing");
    let _ = std::fs::remove_dir_all(root.path());
}

#[test]
fn a_readable_file_loads_and_is_not_rewritten() {
    let root = sandbox();
    let file = root.appending_path_component("preferences.json");
    std::fs::write(file.path(), r#"{"themeName":"Nord","vimKeys":true}"#).unwrap();
    let preferences = Preferences::for_testing(file.clone(), None);
    assert_eq!(preferences.load(), Load::Loaded);
    assert!(!preferences.is_first_run());
    assert_eq!(preferences.values().theme_name, "Nord");
    assert!(preferences.values().vim_keys);
    assert_eq!(preferences.values().external_editor, ExternalEditor::SystemDefault);
    assert_eq!(std::fs::read_to_string(file.path()).unwrap(), r#"{"themeName":"Nord","vimKeys":true}"#);
    let _ = std::fs::remove_dir_all(root.path());
}

#[test]
fn an_undecodable_file_is_moved_aside_and_reported() {
    let root = sandbox();
    let file = root.appending_path_component("preferences.json");
    std::fs::write(file.path(), "[not settings]").unwrap();
    std::fs::write(root.appending_path_component("preferences.json.bad").path(), "older backup").unwrap();
    let preferences = Preferences::for_testing(file.clone(), None);
    let backup = root.appending_path_component("preferences.json.bad");
    assert_eq!(preferences.load(), Load::Recovered { backup: Some(backup.clone()) });
    assert_eq!(std::fs::read_to_string(backup.path()).unwrap(), "[not settings]");
    assert!(!std::path::Path::new(&file.path()).exists());

    // Installing the handler after the fact reports immediately.
    let reported = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&reported);
    preferences.set_on_load_fault(Some(Box::new(move |load| sink.lock().unwrap().push(load.clone()))));
    assert_eq!(reported.lock().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(root.path());
}

#[test]
fn update_persists_pretty_sorted_only_when_something_changed() {
    let root = sandbox();
    let file = root.appending_path_component("support/preferences.json");
    let store = SnapshotStore::new(root.appending_path_component_is_directory("history", true));
    let preferences = Preferences::for_testing(file.clone(), Some(store.clone()));
    assert_eq!(store.maximum_age(), 30.0 * 86_400.0);

    preferences.update(|_| {});
    assert!(!std::path::Path::new(&file.path()).exists(), "an unchanged update writes nothing");

    preferences.update(|values| {
        values.history_maximum_days = 7;
        values.history_maximum_megabytes = 2;
    });
    let written = std::fs::read(file.path()).unwrap();
    assert_eq!(written, preferences.values().persisted_data());
    assert!(String::from_utf8(written).unwrap().starts_with("{\n  \"appearancePreferenceVersion\" : 1,\n"));
    assert_eq!(store.maximum_age(), 7.0 * 86_400.0);
    assert_eq!(store.maximum_bytes(), 2 * 1024 * 1024);
    assert_eq!(Values::decoded(&std::fs::read(file.path()).unwrap()).unwrap(), preferences.values());
    let _ = std::fs::remove_dir_all(root.path());
}

#[test]
fn persistence_failures_are_reported_once_per_run() {
    let root = sandbox();
    // A directory where the file should be: every write fails.
    let file = root.appending_path_component("preferences.json");
    std::fs::create_dir_all(file.path()).unwrap();
    let preferences = Preferences::for_testing(file.clone(), None);
    let failures = Arc::new(Mutex::new(0));
    let counter = Arc::clone(&failures);
    preferences.set_on_persistence_failure(Some(Box::new(move |_| *counter.lock().unwrap() += 1)));
    preferences.update(|values| values.launch_count = 1);
    preferences.update(|values| values.launch_count = 2);
    assert_eq!(*failures.lock().unwrap(), 1, "a run of failures reports once");
    assert!(preferences.last_persistence_error().is_some());
    assert_eq!(preferences.values().launch_count, 2, "the change stays in memory");

    std::fs::remove_dir_all(file.path()).unwrap();
    preferences.update(|values| values.launch_count = 3);
    assert!(preferences.last_persistence_error().is_none());
    std::fs::remove_file(file.path()).unwrap();
    std::fs::create_dir_all(file.path()).unwrap();
    preferences.update(|values| values.launch_count = 4);
    assert_eq!(*failures.lock().unwrap(), 2, "a new run after a success reports again");
    let _ = std::fs::remove_dir_all(root.path());
}
