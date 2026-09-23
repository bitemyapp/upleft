//! `DirectoryWatcher` hops to the main queue after a 0.15 s coalescing delay,
//! so this test owns the main thread (no libtest harness) and runs the main
//! run loop while it waits.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use upleft_render::theme::theme_store::DirectoryWatcher;

unsafe extern "C" {
    fn CFRunLoopRunInMode(mode: *const std::ffi::c_void, seconds: f64, return_after_source_handled: u8) -> i32;
    static kCFRunLoopDefaultMode: *const std::ffi::c_void;
}

fn pump(until: impl Fn() -> bool, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if until() {
            return true;
        }
        // SAFETY: runs the current (main) thread's run loop briefly.
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.05, 0) };
    }
    until()
}

fn main() {
    let directory = std::env::temp_dir().join(format!("upleft-watch-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let fired = Arc::new(AtomicUsize::new(0));
    let counter = fired.clone();
    let watcher = DirectoryWatcher::new(&directory.to_string_lossy(), move || {
        counter.fetch_add(1, Ordering::SeqCst);
    })
    .expect("watching a fresh directory");

    // A burst of writes coalesces into one reload.
    for index in 0..5 {
        std::fs::write(directory.join(format!("theme-{index}.json")), "{}").unwrap();
    }
    assert!(pump(|| fired.load(Ordering::SeqCst) >= 1, Duration::from_secs(3)), "the watcher never fired");
    pump(|| false, Duration::from_millis(400));
    assert_eq!(fired.load(Ordering::SeqCst), 1, "a burst of writes reloaded more than once");

    // An atomic save (write elsewhere, rename in) still reaches the watcher.
    let staging = std::env::temp_dir().join(format!("upleft-watch-staging-{}.json", std::process::id()));
    std::fs::write(&staging, "{}").unwrap();
    std::fs::rename(&staging, directory.join("renamed.json")).unwrap();
    assert!(pump(|| fired.load(Ordering::SeqCst) >= 2, Duration::from_secs(3)), "a rename into the folder was missed");

    // Dropping the watcher stops it.
    drop(watcher);
    let before = fired.load(Ordering::SeqCst);
    std::fs::write(directory.join("after-drop.json"), "{}").unwrap();
    pump(|| false, Duration::from_millis(500));
    assert_eq!(fired.load(Ordering::SeqCst), before, "a dropped watcher still fired");

    let _ = std::fs::remove_dir_all(&directory);
    println!("directory_watcher: ok");
}
