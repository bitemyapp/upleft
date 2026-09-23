//! Port of `Tests/DownrightAppTests/WorkspaceTests.swift`.
//!
//! `WorkspaceIndex` is main-thread state that publishes through the main
//! queue, as the Swift's `@MainActor` class does, so this test owns the main
//! thread (no libtest harness) and turns the main run loop while it waits.
//!
//! Skipped, with the reason:
//! - `siblingUnseenStateUsesContentHash` tests `SiblingScanner` and
//!   `DocumentStateStore` (another port's files, not the workspace index).
//! - `sidebarShowsTreeSearchAndAccessibleRows` builds `WorkspaceSidebarView`,
//!   an AppKit view (window-only).

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use common::ns_range_of;
use upleft_app::workspace::workspace_index::{
    WorkspaceIndex, WorkspaceIndexEntry, WorkspaceIndexPolicy, WorkspaceIndexPolicyInit, WorkspaceIndexSnapshot,
    WorkspaceHeading, WorkspaceLink, WorkspaceLinkKind,
};
use upleft_app::workspace::workspace_link_graph::WorkspaceLinkGraphBuilder;
use upleft_app::workspace::workspace_search::{WorkspaceSearch, WorkspaceSearchQuery};
use upleft_core::NSRange;
use upleft_foundation::url::FileUrl;

unsafe extern "C" {
    fn CFRunLoopRunInMode(mode: *const std::ffi::c_void, seconds: f64, return_after_source_handled: u8) -> i32;
    static kCFRunLoopDefaultMode: *const std::ffi::c_void;
}

/// Turns the main run loop until `until` holds or `limit` passes.
fn pump(until: impl Fn() -> bool, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if until() {
            return true;
        }
        // SAFETY: runs the current (main) thread's run loop briefly.
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.01, 0) };
    }
    until()
}

fn policy_accepts_markdown_and_skips_hidden_build_vendor() {
    let policy = WorkspaceIndexPolicy::default();
    assert!(policy.accepts(&FileUrl::from_path("/tmp/readme.md"), false));
    assert!(!policy.accepts(&FileUrl::from_path("/tmp/readme.txt"), false));
    assert!(!policy.accepts(&FileUrl::from_path("/tmp/.git"), true));
    assert!(!policy.accepts(&FileUrl::from_path("/tmp/vendor"), true));
    assert!(policy.accepts(&FileUrl::from_path("/tmp/docs"), true));
}

fn injected_index_extracts_headings_front_matter_and_links() {
    let root = FileUrl::from_path("/workspace");
    let file = root.appending_path_component("docs/guide.md");
    let source = "---\ntitle: Guide\n---\n\n# Guide\n\nSee [Home](../README.md) and [[Notes]].\n";
    let index = WorkspaceIndex::with_enumerator(
        WorkspaceIndexPolicy::new(WorkspaceIndexPolicyInit { read_concurrency: 2, ..Default::default() }),
        Arc::new(move |_, _| vec![file.clone()]),
        Arc::new(move |_| Some((source.to_owned(), source.len() as i64))),
    );
    let signalled = Rc::new(RefCell::new(false));
    let flag = signalled.clone();
    index.set_on_update(Some(Box::new(move |_| *flag.borrow_mut() = true)));
    index.start(&root);
    assert!(pump(|| *signalled.borrow(), Duration::from_secs(10)), "the index never published");

    let snapshot = index.snapshot();
    let entry = snapshot.entries.first().expect("an entry");
    assert_eq!(entry.relative_path, "docs/guide.md");
    assert_eq!(entry.headings.first().map(|heading| heading.title.as_str()), Some("Guide"));
    assert_eq!(entry.front_matter.first().map(|field| field.key.as_str()), Some("title"));
    assert_eq!(entry.links.len(), 2);
    assert_eq!(entry.links.first().map(|link| link.range), Some(ns_range_of(source, "[Home](../README.md)")));
}

/// `WorkspaceReadGate`: holds the reader until released.
#[derive(Default)]
struct ReadGate {
    state: Mutex<(bool, bool)>,
    condition: Condvar,
}

impl ReadGate {
    fn wait(&self) {
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        self.condition.notify_all();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !state.1 && Instant::now() < deadline {
            state = self.condition.wait_timeout(state, Duration::from_millis(50)).unwrap().0;
        }
    }

    fn wait_until_blocked(&self) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if self.state.lock().unwrap().0 {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    }

    fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.condition.notify_all();
    }
}

fn latest_revision_wins_when_a_second_scan_starts() {
    let root = FileUrl::from_path("/workspace");
    let first = root.appending_path_component("first.md");
    let gate = Arc::new(ReadGate::default());
    let reader_gate = gate.clone();
    let blocked = first.clone();
    let listed = first.clone();
    let index = WorkspaceIndex::with_enumerator(
        WorkspaceIndexPolicy::default(),
        Arc::new(move |_, _| vec![listed.clone()]),
        Arc::new(move |url| {
            if *url == blocked {
                reader_gate.wait();
            }
            Some((format!("# {}", url.last_path_component()), 10))
        }),
    );
    let updates = Rc::new(RefCell::new(Vec::new()));
    let sink = updates.clone();
    index.set_on_update(Some(Box::new(move |snapshot: &WorkspaceIndexSnapshot| sink.borrow_mut().push(snapshot.revision))));
    index.start(&root);
    assert!(gate.wait_until_blocked());
    // A new scan has a new revision. The old scan may finish later but
    // cannot publish its snapshot.
    index.start(&root.appending_path_component("second"));
    gate.release();
    pump(|| !updates.borrow().is_empty(), Duration::from_secs(10));
    // Let the first scan's late hop run too, so a stale publish would show.
    pump(|| false, Duration::from_millis(200));
    assert_eq!(*updates.borrow(), vec![2]);
    assert_eq!(index.snapshot().revision, 2);
}

fn index_enforces_total_byte_budget() {
    let root = FileUrl::from_path("/workspace");
    let first = root.appending_path_component("first.md");
    let second = root.appending_path_component("second.md");
    let index = WorkspaceIndex::with_enumerator(
        WorkspaceIndexPolicy::new(WorkspaceIndexPolicyInit {
            maximum_total_bytes: 10,
            read_concurrency: 2,
            ..Default::default()
        }),
        Arc::new(move |_, _| vec![first.clone(), second.clone()]),
        Arc::new(|url| Some((url.last_path_component(), 6))),
    );
    let signalled = Rc::new(RefCell::new(false));
    let flag = signalled.clone();
    index.set_on_update(Some(Box::new(move |_| *flag.borrow_mut() = true)));
    index.start(&root);
    assert!(pump(|| *signalled.borrow(), Duration::from_secs(10)), "the index never published");

    let snapshot = index.snapshot();
    assert_eq!(snapshot.entries.len(), 1);
    assert!(snapshot.entries.iter().map(|entry| entry.byte_count).sum::<i64>() <= 10);
    assert_eq!(snapshot.skipped_files, 1);
}

fn search_returns_exact_file_range_and_context() {
    let text = "# Intro\n\nShip the release.\n";
    let entry = WorkspaceIndexEntry::new(
        FileUrl::from_path("/workspace/readme.md"),
        "readme.md",
        text,
        vec![WorkspaceHeading { title: "Intro".into(), range: NSRange::new(0, 7), level: 1 }],
        Vec::new(),
        Vec::new(),
        28,
    );
    let snapshot = WorkspaceIndexSnapshot::new(FileUrl::from_path("/workspace"), 1, vec![entry]);
    let results = WorkspaceSearch::search(&WorkspaceSearchQuery::new("release"), &snapshot);
    let result = results.first().expect("a result");
    assert_eq!(result.range, ns_range_of(text, "release"));
    assert_eq!(result.context_text, "Ship the release.");
    assert_eq!(result.line, 3);
    assert_eq!(result.heading.as_deref(), Some("Intro"));
}

fn entry(path: &str, relative_path: &str, links: Vec<WorkspaceLink>) -> WorkspaceIndexEntry {
    WorkspaceIndexEntry::new(FileUrl::from_path(path), relative_path, "", Vec::new(), Vec::new(), links, 0)
}

fn link(destination: &str, location: isize, length: isize) -> WorkspaceLink {
    WorkspaceLink { destination: destination.into(), range: NSRange::new(location, length), kind: WorkspaceLinkKind::Markdown }
}

fn graph_resolves_relative_links_and_backlinks() {
    let root = FileUrl::from_path("/workspace");
    let readme = WorkspaceIndexEntry::new(
        root.appending_path_component("README.md"),
        "README.md",
        "",
        Vec::new(),
        Vec::new(),
        vec![link("docs/guide.md", 0, 18)],
        0,
    );
    let guide = WorkspaceIndexEntry::new(
        root.appending_path_component("docs/guide.md"),
        "docs/guide.md",
        "",
        Vec::new(),
        Vec::new(),
        Vec::new(),
        0,
    );
    let snapshot = WorkspaceIndexSnapshot::new(root, 1, vec![readme.clone(), guide.clone()]);
    let graph = WorkspaceLinkGraphBuilder::build(&snapshot);
    assert!(graph.unresolved.is_empty());
    assert_eq!(graph.links_to(&guide.id).len(), 1);
    assert_eq!(graph.links_to(&guide.id).first().map(|link| link.source_file.clone()), Some(readme.id));
}

fn graph_tolerates_normalized_path_collisions() {
    // `normalize` folds backslashes to slashes and trims a leading `./`, so
    // `a\b.md` collides with `a/b.md` and `.x.md` with `x.md`. The builder
    // must prefer the already-normalized path deterministically.
    let root = FileUrl::from_path("/workspace");
    let slash = entry("/workspace/a/b.md", "a/b.md", Vec::new());
    let backslash = entry("/workspace/a\\b.md", "a\\b.md", Vec::new());
    let visible = entry("/workspace/x.md", "x.md", Vec::new());
    let hidden = entry("/workspace/.x.md", ".x.md", Vec::new());
    let source = entry("/workspace/notes.md", "notes.md", vec![link("a/b.md", 0, 6), link("x.md", 7, 4)]);
    let snapshot = WorkspaceIndexSnapshot::new(
        root,
        1,
        vec![slash.clone(), backslash.clone(), visible.clone(), hidden.clone(), source.clone()],
    );
    let graph = WorkspaceLinkGraphBuilder::build(&snapshot);
    let resolved: Vec<String> = graph
        .outgoing
        .get(&source.id)
        .map(|links| links.iter().filter_map(|link| link.target_file.clone()).collect())
        .unwrap_or_default();
    assert!(resolved.contains(&slash.id));
    assert!(resolved.contains(&visible.id));
    assert!(!resolved.contains(&backslash.id));
    assert!(!resolved.contains(&hidden.id));
    assert!(graph.unresolved.is_empty());
}

fn main() {
    let tests: [(&str, fn()); 7] = [
        ("policy_accepts_markdown_and_skips_hidden_build_vendor", policy_accepts_markdown_and_skips_hidden_build_vendor),
        ("injected_index_extracts_headings_front_matter_and_links", injected_index_extracts_headings_front_matter_and_links),
        ("latest_revision_wins_when_a_second_scan_starts", latest_revision_wins_when_a_second_scan_starts),
        ("index_enforces_total_byte_budget", index_enforces_total_byte_budget),
        ("search_returns_exact_file_range_and_context", search_returns_exact_file_range_and_context),
        ("graph_resolves_relative_links_and_backlinks", graph_resolves_relative_links_and_backlinks),
        ("graph_tolerates_normalized_path_collisions", graph_tolerates_normalized_path_collisions),
    ];
    let failed = AtomicBool::new(false);
    for (name, test) in tests {
        match std::panic::catch_unwind(test) {
            Ok(()) => println!("test {name} ... ok"),
            Err(_) => {
                println!("test {name} ... FAILED");
                failed.store(true, Ordering::SeqCst);
            }
        }
    }
    if failed.load(Ordering::SeqCst) {
        std::process::exit(101);
    }
    println!("workspace_tests: {} passed", tests.len());
}
