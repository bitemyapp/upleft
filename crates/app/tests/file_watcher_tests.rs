//! Port of `Tests/DownrightAppTests/FileWatcherTests.swift` ("File watcher
//! reconciliation", `.serialized`).
//!
//! `FileWatcher` delivers on the main queue, so this binary owns the main
//! thread (`harness = false`, see `main_thread`) and every wait pumps the
//! main run loop, as the Swift tests' `Task.sleep` lets the main actor run.
//!
//! One deliberate change: the Swift fixture names its directory
//! `downright-filewatcher-(UUID().uuidString)` (the interpolation is missing
//! its backslash, so every test shares one directory, which is safe only
//! because the suite is serialized). Here each fixture gets a unique
//! directory.

mod main_thread;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use objc2::Message;
use objc2::rc::Retained;
use objc2_foundation::{NSDate, NSDictionary, NSFileManager, NSFileModificationDate, NSString, NSURL};
use upleft_app::ai::file_watcher::{Event, FileWatcher};
use upleft_foundation::url::FileUrl;

// MARK: - Support

#[derive(Clone, Default)]
struct EventCollector {
    values: Arc<Mutex<Vec<Event>>>,
}

impl EventCollector {
    fn count(&self) -> usize {
        self.values.lock().unwrap().len()
    }

    fn append(&self, event: Event) {
        self.values.lock().unwrap().push(event);
    }

    fn is_changed(&self, index: usize) -> bool {
        matches!(self.values.lock().unwrap().get(index), Some(Event::Changed))
    }

    fn is_removed(&self, index: usize) -> bool {
        matches!(self.values.lock().unwrap().get(index), Some(Event::Removed))
    }

    fn is_restored(&self, index: usize) -> bool {
        matches!(self.values.lock().unwrap().get(index), Some(Event::Restored))
    }

    fn is_rename(&self, url: &FileUrl, index: usize) -> bool {
        match self.values.lock().unwrap().get(index) {
            Some(Event::Renamed(target)) => target.standardized_file_url() == url.standardized_file_url(),
            _ => false,
        }
    }
}

fn watch(url: &FileUrl, events: &EventCollector) -> FileWatcher {
    let events = events.clone();
    FileWatcher::new(url, false, None, move |event| events.append(event))
}

fn unique() -> String {
    objc2_foundation::NSUUID::UUID().UUIDString().to_string()
}

fn temporary_directory() -> FileUrl {
    FileUrl::from_path(&objc2_foundation::NSTemporaryDirectory().to_string())
}

struct Fixture {
    directory: FileUrl,
    url: FileUrl,
}

impl Fixture {
    fn new(contents: &str) -> Fixture {
        let directory = temporary_directory().appending_path_component(&format!("downright-filewatcher-{}", unique()));
        std::fs::create_dir_all(directory.path()).unwrap();
        let url = directory.appending_path_component("document.md");
        std::fs::write(url.path(), contents).unwrap();
        Fixture { directory, url }
    }

    /// `FileManager.default.replaceItemAt(url, withItemAt: temporary)`.
    fn atomic_replace(&self, data: &[u8]) {
        let temporary = self.directory.appending_path_component(&format!(".tmp-{}", unique()));
        std::fs::write(temporary.path(), data).unwrap();
        let mut resulting: Option<Retained<NSURL>> = None;
        NSFileManager::defaultManager()
            .replaceItemAtURL_withItemAtURL_backupItemName_options_resultingItemURL_error(
                &self.url.to_nsurl(),
                &temporary.to_nsurl(),
                None,
                objc2_foundation::NSFileManagerItemReplacementOptions::empty(),
                Some(&mut resulting),
            )
            .unwrap();
    }

    fn remove(&self) {
        let _ = std::fs::remove_dir_all(self.directory.path());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.remove();
    }
}

fn modification_date(url: &FileUrl) -> Retained<NSDate> {
    let attributes = NSFileManager::defaultManager().attributesOfItemAtPath_error(&NSString::from_str(&url.path())).unwrap();
    let date = attributes.objectForKey(unsafe { NSFileModificationDate }).unwrap();
    date.downcast::<NSDate>().unwrap()
}

fn set_modification_date(url: &FileUrl, date: &NSDate) {
    let attributes = NSDictionary::from_retained_objects(&[unsafe { NSFileModificationDate }], &[Retained::into_super(
        Retained::into_super(date.retain()),
    )]);
    // SAFETY: a dictionary of `NSFileAttributeKey` to the attribute's class.
    unsafe {
        NSFileManager::defaultManager()
            .setAttributes_ofItemAtPath_error(&attributes, &NSString::from_str(&url.path()))
            .unwrap();
    }
}

/// `FileHandle(forWritingTo:)`, seek to 0, write, truncate at 4, close.
fn rewrite_in_place(url: &FileUrl, bytes: &[u8]) {
    use std::io::{Seek, SeekFrom, Write};
    let mut handle = std::fs::OpenOptions::new().write(true).open(url.path()).unwrap();
    handle.seek(SeekFrom::Start(0)).unwrap();
    handle.write_all(bytes).unwrap();
    handle.set_len(4).unwrap();
}

/// `try await Task.sleep(nanoseconds: 350_000_000)`.
fn settle() {
    main_thread::sleep_pumping(Duration::from_millis(350));
}

fn expect_event_count(events: &EventCollector, count: usize) {
    for _ in 0..100 {
        if events.count() >= count {
            return;
        }
        main_thread::sleep_pumping(Duration::from_millis(10));
    }
    panic!("Timed out waiting for {count} file-watcher event(s); got {}", events.count());
}

// MARK: - Tests

/// "an acknowledged atomic own write is silent"
fn own_write_is_suppressed() {
    let fixture = Fixture::new("before");
    let events = EventCollector::default();
    let watcher = watch(&fixture.url, &events);

    let own = b"own bytes";
    watcher.suppress_own_write(0.05);
    fixture.atomic_replace(own);
    watcher.acknowledge_own_write(Some(own));
    settle();
    watcher.check_now_for_testing();
    settle();

    assert_eq!(events.count(), 0);
    watcher.stop();
}

/// "an external replacement racing an own write is delivered once"
fn external_replacement_inside_suppression_is_not_swallowed() {
    let fixture = Fixture::new("before");
    let events = EventCollector::default();
    let watcher = watch(&fixture.url, &events);

    let own = b"own bytes";
    let external = b"external";
    watcher.suppress_own_write(0.05);
    fixture.atomic_replace(own);
    fixture.atomic_replace(external);
    watcher.acknowledge_own_write(Some(own));
    settle();
    watcher.check_now_for_testing();
    expect_event_count(&events, 1);
    watcher.check_now_for_testing();
    settle();

    assert_eq!(events.count(), 1);
    assert!(events.is_changed(0));
    watcher.stop();
}

/// "an external replacement observed before the own acknowledgement survives it"
fn external_replacement_before_acknowledge_is_retained() {
    let fixture = Fixture::new("before");
    let events = EventCollector::default();
    let watcher = watch(&fixture.url, &events);

    let own = b"own bytes";
    let external = b"external";
    watcher.suppress_own_write(0.2);
    fixture.atomic_replace(external);
    watcher.check_now_for_testing();
    fixture.atomic_replace(own);
    watcher.acknowledge_own_write(Some(own));
    settle();
    watcher.check_now_for_testing();
    expect_event_count(&events, 1);

    assert_eq!(events.count(), 1);
    assert!(events.is_changed(0));
    watcher.stop();
}

/// "a settled burst reports once and repeated probes stay quiet"
fn burst_is_exactly_once() {
    let fixture = Fixture::new("before");
    let events = EventCollector::default();
    let watcher = watch(&fixture.url, &events);

    fixture.atomic_replace(b"first");
    fixture.atomic_replace(b"second");
    fixture.atomic_replace(b"settled");
    watcher.check_now_for_testing();
    expect_event_count(&events, 1);
    watcher.check_now_for_testing();
    settle();

    assert_eq!(events.count(), 1);
    assert!(events.is_changed(0));
    watcher.stop();
}

/// "same-size, same-metadata rewrites are detected by content"
fn same_metadata_rewrite_is_detected() {
    let fixture = Fixture::new("aaaa");
    let original_date = modification_date(&fixture.url);
    let events = EventCollector::default();
    let watcher = watch(&fixture.url, &events);

    rewrite_in_place(&fixture.url, b"bbbb");
    set_modification_date(&fixture.url, &original_date);
    watcher.check_now_for_testing();
    expect_event_count(&events, 1);

    assert_eq!(events.count(), 1);
    assert!(events.is_changed(0));
    watcher.stop();
}

/// "same-metadata rewrites are bounded when the poll path is used"
fn same_metadata_rewrite_is_detected_by_bounded_polling() {
    let fixture = Fixture::new("aaaa");
    let original_date = modification_date(&fixture.url);
    let events = EventCollector::default();
    let watcher = watch(&fixture.url, &events);

    // Leave the initial metadata baseline untouched, then rewrite in place
    // while preserving size and mtime. The production poll path hashes at a
    // bounded cadence rather than every ordinary poll.
    watcher.poll_now_for_testing();
    rewrite_in_place(&fixture.url, b"bbbb");
    set_modification_date(&fixture.url, &original_date);

    for _ in 0..3 {
        watcher.poll_now_for_testing();
    }
    watcher.poll_now_for_testing();
    expect_event_count(&events, 1);
    assert!(events.is_changed(0));
    watcher.stop();
}

/// "rename, removal, and restoration retain their event semantics"
fn rename_remove_restore() {
    let fixture = Fixture::new("document");
    let events = EventCollector::default();
    let watcher = watch(&fixture.url, &events);

    let renamed_url = fixture.directory.appending_path_component("renamed.md");
    std::fs::rename(fixture.url.path(), renamed_url.path()).unwrap();
    watcher.check_now_for_testing();
    expect_event_count(&events, 1);
    assert!(events.is_rename(&renamed_url, 0));

    std::fs::remove_file(renamed_url.path()).unwrap();
    watcher.check_now_for_testing();
    expect_event_count(&events, 2);
    assert!(events.is_removed(1));

    std::fs::write(renamed_url.path(), "restored").unwrap();
    watcher.check_now_for_testing();
    expect_event_count(&events, 3);
    assert!(events.is_restored(2));
    watcher.stop();
}

/// "an external atomic replacement observed mid-gap is a change, not a removal"
///
/// Regression: an external atomic save observed inside its unlink→rename
/// gap used to be delivered as `.removed` — the sibling scan looks for the
/// *old* inode, which the replacement does not have. A tentative removal now
/// waits out one bounded re-probe.
fn atomic_replacement_observed_mid_gap_is_not_a_removal() {
    let fixture = Fixture::new("document");
    let events = EventCollector::default();
    let watcher = watch(&fixture.url, &events);

    watcher.check_now_for_testing();

    // The unlink phase of an external atomic save: the watched path is
    // missing and the old inode is gone (a parked sibling would make this an
    // ordinary rename, which follows immediately).
    std::fs::remove_file(fixture.url.path()).unwrap();
    watcher.check_now_for_testing();
    assert_eq!(events.count(), 0, "the transient gap must not be reported");

    // The rename phase lands new content at the path before the probe.
    std::fs::write(fixture.url.path(), "rewritten").unwrap();
    watcher.resolve_removal_probe_for_testing();

    expect_event_count(&events, 1);
    assert!(events.is_changed(0), "the replacement must arrive as a change");
    watcher.stop();
}

/// "a genuine removal is still delivered once the re-probe resolves"
fn true_removal_is_still_delivered() {
    let fixture = Fixture::new("document");
    let events = EventCollector::default();
    let watcher = watch(&fixture.url, &events);

    watcher.check_now_for_testing();
    std::fs::remove_file(fixture.url.path()).unwrap();
    watcher.check_now_for_testing();
    assert_eq!(events.count(), 0, "the probe has not resolved yet");
    watcher.resolve_removal_probe_for_testing();

    expect_event_count(&events, 1);
    assert!(events.is_removed(0));
    watcher.stop();
}

/// "a write still unacknowledged past the old clock window stays suppressed"
///
/// A save on a slow volume can stay in flight far longer than any fixed
/// suppression window. Suppression is a state now, so the observation made
/// while the write is still unacknowledged — and the acknowledged bytes
/// themselves — must both stay silent, and only genuine later external
/// activity may deliver.
fn slow_volume_save_is_not_a_phantom_external_change() {
    let fixture = Fixture::new("before");
    let events = EventCollector::default();
    let watcher = watch(&fixture.url, &events);

    let own = b"own bytes written slowly";
    watcher.suppress_own_write(0.05);
    fixture.atomic_replace(own);

    // Outlive the caller's whole interval and the legacy 0.6 s window.
    main_thread::sleep_pumping(Duration::from_millis(700));
    watcher.check_now_for_testing();
    settle();
    assert_eq!(events.count(), 0, "an in-flight own write must not arrive as an external change");

    watcher.acknowledge_own_write(Some(own));
    watcher.check_now_for_testing();
    settle();
    assert_eq!(events.count(), 0, "the acknowledged bytes stay silent too");

    fixture.atomic_replace(b"genuinely external");
    watcher.check_now_for_testing();
    expect_event_count(&events, 1);
    assert_eq!(events.count(), 1);
    assert!(events.is_changed(0));
    watcher.stop();
}

/// "a lost acknowledgement trips the watchdog back to detection"
///
/// If an acknowledgement is somehow lost, the watchdog fails open instead of
/// leaving the watcher deaf forever.
fn lost_acknowledgement_fails_open() {
    let fixture = Fixture::new("before");
    let events = EventCollector::default();
    let watcher = watch(&fixture.url, &events);

    watcher.suppress_own_write(5.0);
    fixture.atomic_replace(b"unacknowledged bytes");
    watcher.trip_suppression_watchdog_for_testing();

    watcher.check_now_for_testing();
    expect_event_count(&events, 1);
    assert!(events.is_changed(0), "fail-open surfaces the unacknowledged write as change");

    // And the watcher keeps detecting afterwards.
    fixture.atomic_replace(b"later");
    watcher.check_now_for_testing();
    expect_event_count(&events, 2);
    watcher.stop();
}

/// "directory watch skips deliveries while own-write suppression is open"
///
/// Directory watches honor the same own-write suppression: the owner writing
/// sidecars into the watched folder must not wake an idempotent rescan, and
/// detection resumes once suppression ends.
fn directory_watch_respects_own_write_suppression() {
    let directory =
        temporary_directory().appending_path_component_is_directory(&format!("downright-dirwatch-{}", unique()), true);
    std::fs::create_dir_all(directory.path()).unwrap();
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(directory.path());
    let sibling = directory.appending_path_component("sibling.md");
    std::fs::write(sibling.path(), "first\n").unwrap();

    let events = EventCollector::default();
    let collector = events.clone();
    let watcher = FileWatcher::new(&directory, true, Some(vec!["md".to_owned()]), move |event| collector.append(event));

    watcher.suppress_own_write(5.0);
    std::fs::write(sibling.path(), "second\n").unwrap();
    watcher.deliver_directory_event_for_testing(&[sibling.path()]);
    settle();
    assert_eq!(events.count(), 0, "our own write into the folder stays silent");

    watcher.cancel_own_write_suppression();
    watcher.deliver_directory_event_for_testing(&[sibling.path()]);
    expect_event_count(&events, 1);
    assert!(events.is_changed(0));
    watcher.stop();
}

// MARK: - Additions: the production paths the Swift tests bypass

/// An external atomic save reaches the handler through FSEvents alone (no
/// test probe), coalesced into one `.changed`; after `stop()` nothing more
/// arrives.
fn fsevents_deliver_an_external_atomic_save() {
    let fixture = Fixture::new("before");
    let events = EventCollector::default();
    let watcher = watch(&fixture.url, &events);
    main_thread::sleep_pumping(Duration::from_millis(100));

    fixture.atomic_replace(b"external one");
    fixture.atomic_replace(b"external two");
    assert!(main_thread::pump_until(|| events.count() >= 1, Duration::from_secs(5)), "FSEvents never delivered");
    settle();
    assert_eq!(events.count(), 1);
    assert!(events.is_changed(0));

    watcher.stop();
    fixture.atomic_replace(b"after stop");
    main_thread::sleep_pumping(Duration::from_millis(2_000));
    assert_eq!(events.count(), 1, "a stopped watcher still delivered");
}

/// A rename in the directory, noticed by FSEvents, re-attaches the watcher.
fn fsevents_follow_a_rename() {
    let fixture = Fixture::new("document");
    let events = EventCollector::default();
    let watcher = watch(&fixture.url, &events);
    main_thread::sleep_pumping(Duration::from_millis(100));

    let renamed = fixture.directory.appending_path_component("moved.md");
    std::fs::rename(fixture.url.path(), renamed.path()).unwrap();
    assert!(main_thread::pump_until(|| events.count() >= 1, Duration::from_secs(5)), "FSEvents never delivered");
    assert!(events.is_rename(&renamed, 0));
    assert_eq!(watcher.url().standardized_file_url(), renamed.standardized_file_url());

    std::fs::write(renamed.path(), "edited after the move").unwrap();
    assert!(main_thread::pump_until(|| events.count() >= 2, Duration::from_secs(5)), "the re-attached watch is deaf");
    assert!(events.is_changed(1));
    watcher.stop();
}

fn main() {
    main_thread::run(&[
        ("own_write_is_suppressed", own_write_is_suppressed),
        ("external_replacement_inside_suppression_is_not_swallowed", external_replacement_inside_suppression_is_not_swallowed),
        ("external_replacement_before_acknowledge_is_retained", external_replacement_before_acknowledge_is_retained),
        ("burst_is_exactly_once", burst_is_exactly_once),
        ("same_metadata_rewrite_is_detected", same_metadata_rewrite_is_detected),
        ("same_metadata_rewrite_is_detected_by_bounded_polling", same_metadata_rewrite_is_detected_by_bounded_polling),
        ("rename_remove_restore", rename_remove_restore),
        ("atomic_replacement_observed_mid_gap_is_not_a_removal", atomic_replacement_observed_mid_gap_is_not_a_removal),
        ("true_removal_is_still_delivered", true_removal_is_still_delivered),
        ("slow_volume_save_is_not_a_phantom_external_change", slow_volume_save_is_not_a_phantom_external_change),
        ("lost_acknowledgement_fails_open", lost_acknowledgement_fails_open),
        ("directory_watch_respects_own_write_suppression", directory_watch_respects_own_write_suppression),
        ("fsevents_deliver_an_external_atomic_save", fsevents_deliver_an_external_atomic_save),
        ("fsevents_follow_a_rename", fsevents_follow_a_rename),
    ]);
}
