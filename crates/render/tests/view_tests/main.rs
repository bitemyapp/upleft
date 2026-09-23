//! View-level MarkdownRenderTests, ported.
//!
//! `MarkdownTextView` and its container are main-thread-only AppKit views,
//! so these tests run in a custom harness (`harness = false`) on the process's
//! main thread, one after another, the way Swift's `@MainActor` suites run.
//! `cargo test -p upleft-render --test view_tests [FILTER]` runs them; a
//! failing test is reported and the binary exits non-zero.
//!
//! Tests that assert what the view puts on screen need a real display cycle;
//! they declare the same prerequisite Swift's `RenderSmokeTests` does
//! (`support::viewport_layout_runs`) instead of being weakened.

mod support;

mod click_stability_tests;
mod content_resize_tests;
mod fragment_seam_tests;
mod layout_filler_tests;
mod speech_accessibility_tests;

use std::panic::{AssertUnwindSafe, catch_unwind};

use objc2::MainThreadMarker;

pub type Test = (&'static str, fn(MainThreadMarker));

fn main() {
    let mtm = MainThreadMarker::new().expect("the view tests run on the main thread");
    // AppKit views work without a running app, but windows want the shared
    // application to exist.
    let _ = objc2_app_kit::NSApplication::sharedApplication(mtm);
    let filters: Vec<String> = std::env::args().skip(1).filter(|arg| !arg.starts_with('-')).collect();
    let mut tests: Vec<Test> = Vec::new();
    tests.extend(layout_filler_tests::TESTS);
    tests.extend(speech_accessibility_tests::TESTS);
    tests.extend(click_stability_tests::TESTS);
    tests.extend(content_resize_tests::TESTS);
    tests.extend(fragment_seam_tests::TESTS);

    let selected: Vec<&Test> = tests
        .iter()
        .filter(|(name, _)| filters.is_empty() || filters.iter().any(|filter| name.contains(filter.as_str())))
        .collect();
    println!("\nrunning {} view tests", selected.len());
    let mut failures: Vec<&str> = Vec::new();
    for (name, test) in &selected {
        let result = catch_unwind(AssertUnwindSafe(|| test(mtm)));
        match result {
            Ok(()) => println!("test {name} ... ok"),
            Err(_) => {
                println!("test {name} ... FAILED");
                failures.push(name);
            }
        }
    }
    println!(
        "\ntest result: {}. {} passed; {} failed",
        if failures.is_empty() { "ok" } else { "FAILED" },
        selected.len() - failures.len(),
        failures.len()
    );
    if !failures.is_empty() {
        for name in &failures {
            println!("    {name}");
        }
        std::process::exit(101);
    }
}
