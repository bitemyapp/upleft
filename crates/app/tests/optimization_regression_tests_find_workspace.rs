//! The `FindSession` and `WorkspaceSearch` cases of
//! `Tests/DownrightAppTests/OptimizationRegressionTests.swift`. Its other
//! cases (fuzzy matcher, scroll anchoring, snapshot store, parse
//! coordinator) belong to the other app-layer ports.

mod common;

use common::temporary_directory;
use upleft_app::support::find_engine::{FindQuery, FindSession};
use upleft_app::workspace::workspace_index::{WorkspaceHeading, WorkspaceIndexEntry, WorkspaceIndexSnapshot};
use upleft_app::workspace::workspace_search::{WorkspaceSearch, WorkspaceSearchQuery};
use upleft_core::NSRange;

#[test]
fn find_session_clears() {
    let mut session = FindSession::new();
    session.update(FindQuery::new("alpha"), "alpha beta alpha", 0);
    assert_eq!(session.matches().len(), 2);
    session.clear();
    assert!(session.matches().is_empty());
    assert!(session.query().is_empty());
}

#[test]
fn workspace_search_loads_text_on_demand() {
    let (root, _cleanup) = temporary_directory("downright-ws-search");
    let file = root.appending_path_component("note.md");
    let body = "# Intro\n\nShip the release.\n";
    std::fs::write(file.path(), body).unwrap();

    let entry = WorkspaceIndexEntry::new(
        file,
        "note.md",
        "",
        vec![WorkspaceHeading { title: "Intro".into(), range: NSRange::new(0, 7), level: 1 }],
        Vec::new(),
        Vec::new(),
        body.len() as i64,
    );
    let snapshot = WorkspaceIndexSnapshot::new(root, 1, vec![entry]);
    let results = WorkspaceSearch::search(&WorkspaceSearchQuery::new("release"), &snapshot);
    let result = results.first().expect("a result");
    assert_eq!(result.context_text, "Ship the release.");
    assert_eq!(result.line, 3);
}

#[test]
fn workspace_search_rejects_files_that_outgrow_their_indexed_bound() {
    let (root, _cleanup) = temporary_directory("downright-ws-search-bound");
    let file = root.appending_path_component("note.md");
    let indexed = "# Note\n";
    std::fs::write(file.path(), format!("{indexed}{}", "oversized ".repeat(10_000))).unwrap();
    let entry = WorkspaceIndexEntry::new(file, "note.md", "", Vec::new(), Vec::new(), Vec::new(), indexed.len() as i64);
    let snapshot = WorkspaceIndexSnapshot::new(root, 1, vec![entry]);

    assert!(WorkspaceSearch::search(&WorkspaceSearchQuery::new("oversized"), &snapshot).is_empty());
}
