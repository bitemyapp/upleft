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
mod density_rail_tests;
mod drop_and_quick_look_tests;
mod fragment_seam_tests;
mod layout_filler_tests;
mod smart_paste_integration_tests;
mod speech_accessibility_tests;

use std::panic::{AssertUnwindSafe, catch_unwind};

use objc2::MainThreadMarker;

pub type Test = (&'static str, fn(MainThreadMarker));

fn main() {
    let mtm = MainThreadMarker::new().expect("the view tests run on the main thread");
    // AppKit views work without a running app, but windows want the shared
    // application to exist.
    let _ = objc2_app_kit::NSApplication::sharedApplication(mtm);
    // The pump-based tests wait on main-queue timers (the 40–80 ms resize
    // idle delays). A background process is eligible for App Nap, which
    // coalesces those timers by up to seconds; hold a latency-critical
    // activity for the run so a busy machine cannot time them out.
    // An app's main thread runs at user-interactive QoS; a test binary's
    // starts lower, and its main-queue timers then fire hundreds of
    // milliseconds late on a loaded machine.
    unsafe extern "C" {
        fn pthread_set_qos_class_self_np(class: u32, relative_priority: i32) -> i32;
    }
    const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
    // SAFETY: sets the calling thread's own QoS class.
    unsafe { pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) };
    let reason = objc2_foundation::NSString::from_str("upleft view tests");
    let _activity = objc2_foundation::NSProcessInfo::processInfo().beginActivityWithOptions_reason(
        objc2_foundation::NSActivityOptions::UserInitiated | objc2_foundation::NSActivityOptions::LatencyCritical,
        &reason,
    );
    let filters: Vec<String> = std::env::args().skip(1).filter(|arg| !arg.starts_with('-')).collect();
    let mut tests: Vec<Test> = Vec::new();
    tests.extend(layout_filler_tests::TESTS);
    tests.extend(speech_accessibility_tests::TESTS);
    tests.extend(click_stability_tests::TESTS);
    tests.extend(content_resize_tests::TESTS);
    tests.extend(fragment_seam_tests::TESTS);
    tests.extend(smart_paste_integration_tests::TESTS);
    tests.extend(drop_and_quick_look_tests::TESTS);
    tests.extend(density_rail_tests::TESTS);

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
