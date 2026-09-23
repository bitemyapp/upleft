//! Shared by the updater test binaries, which own the main thread (no libtest
//! harness): the updater's objects are main-actor objects in Swift and hop
//! through the main dispatch queue, which only runs while the main thread
//! runs its run loop.

#![allow(dead_code)]

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use dispatch2::DispatchQueue;

unsafe extern "C" {
    fn CFRunLoopRunInMode(mode: *const std::ffi::c_void, seconds: f64, return_after_source_handled: u8) -> i32;
    static kCFRunLoopDefaultMode: *const std::ffi::c_void;
}

/// Runs the main run loop until `until` holds or `limit` passes.
pub fn pump(until: impl Fn() -> bool, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if until() {
            return true;
        }
        // SAFETY: runs the current (main) thread's run loop briefly.
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.01, 1) };
    }
    until()
}

/// Lets every main-queue block enqueued so far run: the queue is FIFO, so a
/// sentinel enqueued now runs after all of them (what the Swift tests get by
/// hopping through `DispatchQueue.main.async`).
pub fn drain_main_queue() {
    let done = Arc::new(AtomicBool::new(false));
    let flag = done.clone();
    DispatchQueue::main().exec_async(move || flag.store(true, Ordering::SeqCst));
    assert!(pump(|| done.load(Ordering::SeqCst), Duration::from_secs(5)), "the main queue never drained");
}

pub struct Test {
    pub name: &'static str,
    pub run: fn(),
}

pub struct Skipped {
    pub name: &'static str,
    pub reason: &'static str,
}

/// Runs `tests` in order on the main thread (the Swift suites are
/// `.serialized` and `@MainActor`), filtered by the first non-flag argument.
pub fn run(binary: &str, tests: &[Test], skipped: &[Skipped]) {
    assert!(upleft_app::updater::main_actor::is_main_thread(), "{binary} must run on the main thread");
    let filter = std::env::args().skip(1).find(|argument| !argument.starts_with('-'));
    let mut passed = 0;
    let mut failed = Vec::new();
    for test in tests {
        if filter.as_ref().is_some_and(|filter| !test.name.contains(filter.as_str())) {
            continue;
        }
        match catch_unwind(AssertUnwindSafe(test.run)) {
            Ok(()) => {
                passed += 1;
                println!("test {} ... ok", test.name);
            }
            Err(_) => {
                failed.push(test.name);
                println!("test {} ... FAILED", test.name);
            }
        }
    }
    for skip in skipped {
        println!("test {} ... skipped ({})", skip.name, skip.reason);
    }
    println!(
        "\n{binary}: {passed} passed; {} failed; {} skipped",
        failed.len(),
        skipped.len()
    );
    if !failed.is_empty() {
        for name in &failed {
            println!("    failed: {name}");
        }
        std::process::exit(101);
    }
}
