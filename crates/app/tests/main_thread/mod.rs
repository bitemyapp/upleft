//! A test runner for suites whose Swift originals run on the main actor, or
//! whose code under test delivers to the main queue (`FileWatcher`,
//! `LocalAILatestWinsController`, `MarkdownDocument`).
//!
//! libtest runs tests on worker threads while the main thread waits, so the
//! main dispatch queue never drains. These test binaries are declared with
//! `harness = false`: `main` runs every test in order on the main thread
//! (Swift's `@Suite(.serialized)`), and waiting pumps the main run loop,
//! which drains the main queue, as Swift Testing's main actor does.
//!
//! Positional arguments filter tests by substring and `--skip` excludes, as
//! in libtest; `--list` lists them; other flags (and their values) are
//! ignored.

#![allow(dead_code)]

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::{Duration, Instant};

unsafe extern "C" {
    fn CFRunLoopRunInMode(mode: *const std::ffi::c_void, seconds: f64, return_after_source_handled: u8) -> i32;
    static kCFRunLoopDefaultMode: *const std::ffi::c_void;
}

/// Runs the main run loop (and with it the main dispatch queue) once for up
/// to `seconds`.
pub fn run_loop_once(seconds: f64) {
    // SAFETY: runs the calling (main) thread's run loop.
    unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, seconds, 1) };
}

/// Pumps the main run loop until `condition` holds or `limit` passes.
pub fn pump_until(condition: impl Fn() -> bool, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if condition() {
            return true;
        }
        run_loop_once(0.005);
    }
    condition()
}

/// `try await Task.sleep(…)` on the main actor: time passes and main-queue
/// work runs meanwhile.
pub fn sleep_pumping(duration: Duration) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        run_loop_once(deadline.saturating_duration_since(Instant::now()).as_secs_f64().min(0.005));
    }
}

/// `await Task.yield()` on the main actor: lets queued main-queue work run.
pub fn yield_main() {
    run_loop_once(0.0);
}

pub type TestFn = fn();

/// Runs `tests` in order on the main thread and exits non-zero on failure.
pub fn run(tests: &[(&str, TestFn)]) {
    assert!(objc2::MainThreadMarker::new().is_some(), "main-thread tests must run on the main thread");
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.iter().any(|argument| argument == "--list") {
        for (name, _) in tests {
            println!("{name}: test");
        }
        return;
    }
    // libtest's flags that take a separate value; `--skip` excludes.
    const VALUED: [&str; 5] = ["--test-threads", "--color", "--format", "--logfile", "--shuffle-seed"];
    let mut filters: Vec<&String> = Vec::new();
    let mut skips: Vec<&String> = Vec::new();
    let mut arguments_iter = arguments.iter();
    while let Some(argument) = arguments_iter.next() {
        if argument == "--skip" {
            skips.extend(arguments_iter.next());
        } else if VALUED.contains(&argument.as_str()) {
            arguments_iter.next();
        } else if !argument.starts_with('-') {
            filters.push(argument);
        }
    }
    let selected: Vec<&(&str, TestFn)> = tests
        .iter()
        .filter(|(name, _)| filters.is_empty() || filters.iter().any(|filter| name.contains(filter.as_str())))
        .filter(|(name, _)| !skips.iter().any(|skip| name.contains(skip.as_str())))
        .collect();
    println!("\nrunning {} tests", selected.len());
    let mut failed = Vec::new();
    for (name, test) in &selected {
        let started = Instant::now();
        let outcome = catch_unwind(AssertUnwindSafe(test));
        let elapsed = started.elapsed().as_secs_f64();
        match outcome {
            Ok(()) => println!("test {name} ... ok ({elapsed:.2}s)"),
            Err(_) => {
                println!("test {name} ... FAILED ({elapsed:.2}s)");
                failed.push(*name);
            }
        }
    }
    let passed = selected.len() - failed.len();
    if failed.is_empty() {
        println!("\ntest result: ok. {passed} passed; 0 failed\n");
    } else {
        println!("\nfailures:");
        for name in &failed {
            println!("    {name}");
        }
        println!("\ntest result: FAILED. {passed} passed; {} failed\n", failed.len());
        std::process::exit(101);
    }
}
