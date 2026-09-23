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

/// [`run`] for view tests (the panels'): `constrainFrameRect:toScreen:` is
/// the identity for the whole run ([`keep_windows_off_screen`]), and after
/// every test [`assert_off_screen`] stops the run if any visible window of
/// the process touches a display.
pub fn run_off_screen(tests: &[(&str, TestFn)]) {
    keep_windows_off_screen();
    OFF_SCREEN_CHECKS.with(|checks| checks.set(true));
    run(tests);
}

thread_local! {
    static OFF_SCREEN_CHECKS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

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
        if OFF_SCREEN_CHECKS.with(std::cell::Cell::get) {
            assert_off_screen();
        }
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

// MARK: - Windows that must be ordered in

/// AppKit pulls a *titled* window back onto a display when it is ordered in
/// (`-[NSWindow constrainFrameRect:toScreen:]`), even one parked at
/// (-30000, -30000), and a borderless child ordered in with `addChildWindow`
/// orders its titled parent in too. A test that genuinely needs a window
/// ordered in calls this first: the method becomes the identity for the
/// test process, as in the app-window harness (crates/app/PORTING.md), and
/// [`assert_off_screen`] then proves the window stayed off every display.
pub fn keep_windows_off_screen() {
    use objc2::runtime::{AnyObject, Imp, Sel};
    use objc2::{ClassType, sel};
    use objc2_foundation::NSRect;
    static ONCE: std::sync::Once = std::sync::Once::new();
    extern "C-unwind" fn identity(_this: &AnyObject, _cmd: Sel, rect: NSRect, _screen: *mut AnyObject) -> NSRect {
        rect
    }
    ONCE.call_once(|| {
        let Some(method) = objc2_app_kit::NSWindow::class().instance_method(sel!(constrainFrameRect:toScreen:)) else {
            return;
        };
        // SAFETY: the replacement has the method's exact signature.
        unsafe {
            let imp: Imp = std::mem::transmute::<
                extern "C-unwind" fn(&AnyObject, Sel, NSRect, *mut AnyObject) -> NSRect,
                Imp,
            >(identity);
            method.set_implementation(imp);
        }
    });
}

/// Orders every window out and aborts the test run if any visible window of
/// this process touches a display.
pub fn assert_off_screen() {
    let mtm = objc2::MainThreadMarker::new().expect("main thread");
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    let screens = objc2_app_kit::NSScreen::screens(mtm);
    for window in app.windows().iter() {
        if !window.isVisible() {
            continue;
        }
        let frame = window.frame();
        let touches = screens.iter().any(|screen| {
            let s = screen.frame();
            frame.origin.x < s.origin.x + s.size.width
                && s.origin.x < frame.origin.x + frame.size.width
                && frame.origin.y < s.origin.y + s.size.height
                && s.origin.y < frame.origin.y + frame.size.height
        });
        if touches {
            for window in app.windows().iter() {
                window.orderOut(None);
            }
            eprintln!("a test window reached a display at {frame:?}; stopping");
            std::process::exit(101);
        }
    }
}
