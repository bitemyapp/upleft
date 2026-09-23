//! The `FindEngine` part of `Tests/DownrightAppTests/SiblingSearchTests.swift`.
//!
//! Every other case there drives a `DocumentWindowController` (the find bar,
//! the inspector, the scanner's rescans) and needs a window; they belong to
//! the window controller's port. `SiblingSearch` takes a URL list, so it has
//! no seam into `SiblingScanner` to cover here.

mod common;

use common::temporary_directory;
use upleft_app::support::find_engine::{FindQuery, SiblingSearch};

#[test]
fn an_already_cancelled_search_reads_no_files() {
    let (root, _cleanup) = temporary_directory("downright-sibling-search");
    for (name, contents) in [
        ("CURRENT.md", "# Current\n"),
        ("FIRST.md", "The cancellation needle is first.\n"),
        ("SECOND.md", "The cancellation needle is second.\n"),
    ] {
        std::fs::write(root.appending_path_component(name).path(), contents).unwrap();
    }

    let hits = SiblingSearch::search(
        &FindQuery::new("needle"),
        &[root.appending_path_component("FIRST.md"), root.appending_path_component("SECOND.md")],
        20,
        &|| true,
    );

    assert!(hits.is_empty());
}

/// Not in the Swift file: the uncancelled search over the same fixture, so
/// the hit fields the window renders are checked too.
#[test]
fn an_uncancelled_search_reports_context_heading_and_line() {
    let (root, _cleanup) = temporary_directory("downright-sibling-search");
    std::fs::write(root.appending_path_component("FIRST.md").path(), "# Intro\n\nThe cancellation needle is first.\n").unwrap();
    std::fs::write(root.appending_path_component("SECOND.md").path(), "```\nneedle in code\n```\n").unwrap();

    let hits = SiblingSearch::search_default(
        &FindQuery::new("needle"),
        &[root.appending_path_component("FIRST.md"), root.appending_path_component("SECOND.md")],
    );

    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].display_name, "FIRST");
    assert_eq!(hits[0].context_text, "The cancellation needle is first.");
    assert_eq!(hits[0].heading_title.as_deref(), Some("Intro"));
    assert_eq!(hits[0].line_number, 3);
    assert_eq!(hits[1].context_text, "needle in code", "a code block contributes one line of context");
    assert!(hits[1].heading_title.is_none());
}
