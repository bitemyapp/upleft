//! Port of `Sources/DownrightApp/AI/FileWatcher.swift`.
//!
//! Watches a single file for external rewrites (§8.1).
//!
//! The implementation gotcha the spec calls out is the whole reason this type
//! exists in this shape: **every agent CLI writes atomically** — write a temp
//! file, then `rename()` it over the target.  The original inode is unlinked,
//! so a vnode watch on the file silently stops firing and the app quietly stops
//! noticing changes.  So we watch the *parent directory* with FSEvents and
//! match on filename, and we re-stat the file after every event rather than
//! trusting any handle we held before it.
//!
//! A slow mtime poll runs alongside as a safety net.  FSEvents is reliable on
//! local volumes but degrades on network and virtualised filesystems, and
//! people keep agent output in Dropbox folders whether or not we sync (§2).
//!
//! The same mechanics as the Swift: a serial utility-QoS dispatch queue
//! (`com.ezzy.downright.filewatcher`) that owns all state, identified by a
//! queue-specific key; an FSEvents stream delivered on that queue; a dispatch
//! timer source for the poll; `DispatchWorkItem`s (`dispatch_block_create`d
//! blocks, cancelled with `dispatch_block_cancel`) scheduled with
//! `dispatch_after`; events delivered with `DispatchQueue.main.async`.
//! Swift's queue confinement is a mutex here, taken once per queue entry.
//!
//! Like the Swift, `new`, `retarget` and `acknowledge_own_write` read and hash
//! the file on the calling thread (the latter two synchronously on the queue).

use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use block2::RcBlock;
use dispatch2::{DispatchObject, DispatchQoS, DispatchQueue, DispatchQueueAttr, DispatchRetained, DispatchSource, DispatchTime};
use objc2_foundation::NSString;
use sha2::{Digest, Sha256};
use upleft_foundation::date::Date;
use upleft_foundation::url::FileUrl;

use crate::app::document_types;

/// `Date.distantPast`.
const DISTANT_PAST: Date = Date { time_interval_since_reference_date: -63_114_076_800.0 };

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub modified: Date,
    pub size: isize,
    pub inode: u64,
    /// A content signature closes the gap where an editor rewrites a file
    /// in place without changing its size or filesystem timestamp.
    pub content_digest: Option<[u8; 32]>,
}

impl Snapshot {
    pub const MISSING: Snapshot = Snapshot { modified: DISTANT_PAST, size: -1, inode: 0, content_digest: None };

    pub fn with_content_digest(&self, digest: [u8; 32]) -> Snapshot {
        let mut copy = self.clone();
        copy.content_digest = Some(digest);
        copy
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// The file's bytes changed on disk.
    Changed,
    /// The file went away — deleted, and not found again under a new name.
    Removed,
    /// It came back after having been removed.
    Restored,
    /// The file was renamed or moved within its directory.  The watcher has
    /// already re-attached; the URL is where it lives now.
    Renamed(FileUrl),
}

/// Agents write a file two to five times in a few seconds — a plan, then a
/// section, then a fix to that section.  120 ms was short enough that each
/// of those arrived as its own event, and the reader watched the document
/// rebuild five times.  300 ms holds a burst together without making a
/// single deliberate write feel late.  The document layer adds a second,
/// trailing quiet period on top of this; see
/// `MarkdownDocument.handleExternalWrite`.
const COALESCE_INTERVAL: f64 = 0.30;
const POLL_INTERVAL: f64 = 1.5;
/// `.milliseconds(400)`.
const POLL_LEEWAY_NANOSECONDS: u64 = 400_000_000;
/// Metadata is the cheap path for ordinary polls. Every few polls we still
/// hash unchanged metadata so an in-place rewrite that preserves inode,
/// size, and timestamps is detected even on filesystems whose event stream
/// is unavailable. FSEvents and test probes force a hash.
const CONTENT_DIGEST_POLL_STRIDE: u64 = 4;
/// `suppressOwnWrite(for:)`'s default interval.
pub const DEFAULT_SUPPRESSION_INTERVAL: f64 = 0.6;

pub struct FileWatcher {
    inner: Arc<Inner>,
}

struct Inner {
    me: Weak<Inner>,
    /// `private(set) var url`, readable from any thread.
    url: Mutex<FileUrl>,
    watches_directory: bool,
    handler: Arc<dyn Fn(Event) + Send + Sync>,
    /// Extensions a directory watch reports.  A folder of agent output churns
    /// constantly — lockfiles, build artefacts, `.DS_Store` — and matching on
    /// the path prefix alone woke a full sibling rescan for every one of them.
    interesting_extensions: Vec<String>,
    queue: DispatchRetained<DispatchQueue>,
    /// `DispatchSpecificKey<Void>`: its address is the key.
    queue_key: Box<u8>,
    state: Mutex<State>,
}

struct State {
    stream: Option<StreamRef>,
    stream_context: Option<Arc<StreamContext>>,
    poll_timer: Option<DispatchRetained<DispatchSource>>,
    last_snapshot: Snapshot,
    coalesce_work_item: Option<WorkItem>,
    /// A filesystem event is a strong signal that content should be checked
    /// immediately. Keep that request when a polling tick happens to
    /// coalesce with the event.
    force_content_digest_on_next_check: bool,
    /// Writes we made ourselves must not come back to us as external changes.
    ///
    /// Suppression is a *state*, not a clock: it opens with
    /// `suppress_own_write()` and closes at `acknowledge_own_write(contents:)`
    /// or `cancel_own_write_suppression()`, however long the write takes.
    /// Two bounded clocks remain, neither load-bearing: a watchdog that fails
    /// open if an acknowledgement is somehow lost, and a short
    /// post-acknowledge grace that keeps late FSEvents from the same write
    /// from being reconciled as externals before the state pass sees them.
    own_write_in_flight: bool,
    own_write_watchdog_deadline: Date,
    settle_grace_until: Date,
    /// The last snapshot observed while an own-write suppression window was
    /// open.  This is deliberately separate from `last_snapshot`: advancing
    /// the baseline before deciding whether a snapshot is ours is the race
    /// that used to swallow an external atomic replacement.
    suppressed_snapshots: Vec<Snapshot>,
    suppression_baseline: Option<Snapshot>,
    expected_own_snapshot: Option<Snapshot>,
    own_write_generation: u64,
    /// A tentative removal waits out one bounded re-probe before being
    /// delivered. External atomic saves pass through a window where the path
    /// does not exist (unlink, then rename into place); our own writes filter
    /// that transient through suppression state, and an external writer gets
    /// this probe instead of a phantom "the file is gone".
    pending_removal_probe: Option<WorkItem>,
    /// Shorter than the document layer's quiet period, longer than any local
    /// unlink→rename gap, so a genuine deletion is reported promptly.
    removal_probe_interval: f64,
    poll_probe_count: u64,
    /// Bumped on every `stop()` so in-flight coalesced work becomes a no-op.
    generation: u64,
}

impl FileWatcher {
    /// `init(url:watchesDirectory:fileExtensions:handler:)`. `handler` runs
    /// on the main queue.
    pub fn new(
        url: &FileUrl,
        watches_directory: bool,
        file_extensions: Option<Vec<String>>,
        handler: impl Fn(Event) + Send + Sync + 'static,
    ) -> FileWatcher {
        let url = url.resolving_symlinks_in_path();
        let interesting_extensions = file_extensions.unwrap_or_else(|| {
            document_types::FILE_EXTENSIONS.iter().map(|extension| upleft_swift_text::lowercased(extension)).collect()
        });
        let last_snapshot = snapshot(&url, true);
        let attribute = DispatchQueueAttr::with_qos_class(DispatchQueueAttr::SERIAL, DispatchQoS::Utility, 0);
        let queue = DispatchQueue::new("com.bitemyapp.upleft.filewatcher", Some(&attribute));
        let inner = Arc::new_cyclic(|me| Inner {
            me: me.clone(),
            url: Mutex::new(url),
            watches_directory,
            handler: Arc::new(handler),
            interesting_extensions,
            queue,
            queue_key: Box::new(0),
            state: Mutex::new(State {
                stream: None,
                stream_context: None,
                poll_timer: None,
                last_snapshot,
                coalesce_work_item: None,
                force_content_digest_on_next_check: false,
                own_write_in_flight: false,
                own_write_watchdog_deadline: DISTANT_PAST,
                settle_grace_until: DISTANT_PAST,
                suppressed_snapshots: Vec::new(),
                suppression_baseline: None,
                expected_own_snapshot: None,
                own_write_generation: 0,
                pending_removal_probe: None,
                removal_probe_interval: 0.35,
                poll_probe_count: 0,
                generation: 0,
            }),
        });
        inner.queue.set_specific(inner.key(), || {});
        {
            let mut state = inner.lock();
            inner.start(&mut state);
        }
        FileWatcher { inner }
    }

    /// `url`: where the watched file lives now.
    pub fn url(&self) -> FileUrl {
        self.inner.url()
    }

    pub fn stop(&self) {
        self.inner.on_queue(|inner, state| inner.stop_on_queue(state));
    }

    /// Point the watcher at a different file (Save As, or following a rename).
    pub fn retarget(&self, new_url: &FileUrl) {
        let resolved = new_url.resolving_symlinks_in_path();
        self.inner.on_queue(move |inner, state| inner.retarget_on_queue(state, resolved));
    }

    /// Call immediately before writing the file ourselves.  Our own write
    /// would otherwise arrive back as an external change and re-mark the whole
    /// document — the toggle-a-checkbox case (§8.5) makes this obvious fast.
    ///
    /// Suppression stays open until the matching acknowledgement (or cancel),
    /// regardless of how long the write takes; `interval` (Swift's default:
    /// [`DEFAULT_SUPPRESSION_INTERVAL`]) only sizes the lost-acknowledgement
    /// watchdog.
    pub fn suppress_own_write(&self, interval: f64) {
        self.inner.on_queue(move |_, state| {
            state.own_write_generation = state.own_write_generation.wrapping_add(1);
            state.suppression_baseline = Some(state.last_snapshot.clone());
            state.expected_own_snapshot = None;
            state.suppressed_snapshots.clear();
            state.own_write_in_flight = true;
            state.settle_grace_until = DISTANT_PAST;
            // A save that never acknowledged must not mute the watcher
            // forever: fail open after a generous multiple of the caller's
            // expectation, floored well above any honest slow volume.
            state.own_write_watchdog_deadline = Date::now().adding(f64::max(5.0, interval * 4.0));
        });
    }

    /// Call after writing so the baseline can reconcile the replacement.  When
    /// supplied, `contents` are the exact bytes Downright intended to write;
    /// retaining them is what distinguishes a racing external replacement from
    /// a successful own save even if no filesystem event arrived yet.
    pub fn acknowledge_own_write(&self, contents: Option<&[u8]>) {
        let digest = contents.map(content_digest);
        self.inner.on_queue(move |inner, state| {
            let actual = snapshot(&inner.url(), true);
            let expected = match digest {
                Some(digest) => actual.with_content_digest(digest),
                None => actual.clone(),
            };
            state.expected_own_snapshot = Some(expected.clone());

            if actual == expected {
                state.last_snapshot = actual;
            } else if actual != state.last_snapshot {
                // The file on disk is not the payload we just wrote.  Retain it
                // as an external observation for the reconciliation pass.
                record_suppressed_snapshot(state, actual);
            }
            // The write has landed; the very next observation reconciles.
            state.own_write_in_flight = false;
            state.settle_grace_until = Date::now().adding(0.25);
        });
    }

    /// A guarded save failed before committing our payload. End suppression
    /// immediately so the restored external generation is observed normally.
    pub fn cancel_own_write_suppression(&self) {
        self.inner.on_queue(|inner, state| {
            state.expected_own_snapshot = None;
            state.suppressed_snapshots.clear();
            state.own_write_in_flight = false;
            state.settle_grace_until = DISTANT_PAST;
            state.force_content_digest_on_next_check = true;
            inner.schedule_check(state, true);
        });
    }

    /// Synchronous filesystem probe used by deterministic integration tests.
    /// Production notifications still arrive through FSEvents and the polling
    /// safety net; this hook only avoids making race tests depend on scheduler
    /// timing.
    pub fn check_now_for_testing(&self) {
        self.inner.on_queue(|inner, state| inner.check(state, true));
    }

    /// A production-shaped metadata-only probe used by deterministic tests to
    /// exercise the bounded same-metadata fallback without waiting 1.5s per
    /// poll. Unlike `check_now_for_testing`, this does not force a content read.
    pub fn poll_now_for_testing(&self) {
        self.inner.on_queue(|inner, state| inner.check(state, false));
    }

    /// Forces the lost-acknowledgement watchdog to fire on the next check, so
    /// the fail-open path is deterministic instead of waiting out its floor.
    pub fn trip_suppression_watchdog_for_testing(&self) {
        self.inner.on_queue(|_, state| state.own_write_watchdog_deadline = DISTANT_PAST);
    }

    /// Runs the tentative-removal resolution synchronously, so race tests can
    /// pin both outcomes of the removal grace without waiting out its interval.
    pub fn resolve_removal_probe_for_testing(&self) {
        self.inner.on_queue(|inner, state| inner.resolve_tentative_removal(state));
    }

    /// Drives directory-watch event matching directly, without depending on
    /// FSEvents delivery timing.
    pub fn deliver_directory_event_for_testing(&self, paths: &[String]) {
        let paths = paths.to_vec();
        self.inner.on_queue(move |inner, state| inner.handle_stream_events(state, &paths));
    }
}

impl Drop for FileWatcher {
    /// `deinit { stop() }`.
    fn drop(&mut self) {
        self.stop();
    }
}

// MARK: - Queue confinement

impl Inner {
    fn key(&self) -> NonNull<()> {
        NonNull::from(&*self.queue_key).cast()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn url(&self) -> FileUrl {
        self.url.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
    }

    fn set_url(&self, url: FileUrl) {
        *self.url.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = url;
    }

    /// `onQueue`: runs `body` on the watcher queue, synchronously, or in place
    /// when already on it.
    fn on_queue<T: Send>(&self, body: impl FnOnce(&Inner, &mut State) -> T + Send) -> T {
        // SAFETY: the key is only compared, never dereferenced.
        if !unsafe { dispatch2::dispatch_get_specific(self.key().cast()) }.is_null() {
            let mut state = self.lock();
            return body(self, &mut state);
        }
        let mut result = None;
        self.queue.exec_sync(|| {
            let mut state = self.lock();
            result = Some(body(self, &mut state));
        });
        result.expect("dispatch_sync ran the block")
    }

    /// Runs `body` on the queue later, if the watcher is still alive.
    fn upgrade_and_lock(weak: &Weak<Inner>, body: impl FnOnce(&Inner, &mut State)) {
        let Some(inner) = weak.upgrade() else { return };
        let mut state = inner.lock();
        body(&inner, &mut state);
    }

    // MARK: Lifecycle

    fn start(&self, state: &mut State) {
        self.start_stream(state);
        self.start_polling(state);
    }

    fn stop_on_queue(&self, state: &mut State) {
        state.generation = state.generation.wrapping_add(1);
        if let Some(stream) = state.stream.take() {
            // SAFETY: a stream this watcher created and has not released.
            unsafe {
                FSEventStreamStop(stream.0);
                FSEventStreamInvalidate(stream.0);
                FSEventStreamRelease(stream.0);
            }
        }
        if let Some(context) = state.stream_context.take() {
            *context.watcher.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Weak::new();
        }
        if let Some(timer) = state.poll_timer.take() {
            timer.cancel();
        }
        if let Some(item) = state.coalesce_work_item.take() {
            item.cancel();
        }
        if let Some(item) = state.pending_removal_probe.take() {
            item.cancel();
        }
        state.force_content_digest_on_next_check = false;
    }

    fn retarget_on_queue(&self, state: &mut State, resolved: FileUrl) {
        if resolved == self.url() {
            return;
        }
        self.stop_on_queue(state);
        self.set_url(resolved.clone());
        state.last_snapshot = snapshot(&resolved, true);
        state.poll_probe_count = 0;
        self.start(state);
    }

    // MARK: FSEvents

    fn start_stream(&self, state: &mut State) {
        let url = self.url();
        let directory = if self.watches_directory { url.path() } else { url.deleting_last_path_component().path() };
        let context_box = Arc::new(StreamContext { watcher: Mutex::new(self.me.clone()) });
        state.stream_context = Some(context_box.clone());
        let mut context = FSEventStreamContext {
            version: 0,
            info: Arc::as_ptr(&context_box) as *mut c_void,
            retain: Some(stream_context_retain),
            release: Some(stream_context_release),
            copy_description: None,
        };
        let flags = K_FS_EVENT_STREAM_CREATE_FLAG_FILE_EVENTS
            | K_FS_EVENT_STREAM_CREATE_FLAG_NO_DEFER
            | K_FS_EVENT_STREAM_CREATE_FLAG_WATCH_ROOT;
        let paths = objc2_foundation::NSArray::from_retained_slice(&[NSString::from_str(&directory)]);
        // SAFETY: the context is copied by FSEventStreamCreate, which retains
        // `info` through the callbacks above; `paths` is a CFArray of
        // CFStrings (toll-free bridged).
        let stream = unsafe {
            FSEventStreamCreate(
                std::ptr::null(),
                stream_callback,
                &mut context,
                objc2::rc::Retained::as_ptr(&paths) as *const c_void,
                K_FS_EVENT_STREAM_EVENT_ID_SINCE_NOW,
                0.05,
                flags,
            )
        };
        if stream.is_null() {
            return;
        }
        // SAFETY: a fresh stream, delivered on this watcher's queue.
        unsafe {
            FSEventStreamSetDispatchQueue(stream, &*self.queue as *const DispatchQueue as *mut c_void);
            FSEventStreamStart(stream);
        }
        state.stream = Some(StreamRef(stream));
    }

    fn handle_stream_events(&self, state: &mut State, paths: &[String]) {
        let url = self.url();
        if self.watches_directory {
            let url_path = url.path();
            let root = if upleft_swift_text::has_suffix(&url_path, "/") { url_path.clone() } else { format!("{url_path}/") };
            let matches = paths.iter().any(|path| {
                if !(upleft_swift_text::str_eq(path, &url_path) || upleft_swift_text::has_prefix(path, &root)) {
                    return false;
                }
                self.is_interesting(path, &url_path)
            });
            if !matches {
                return;
            }
            self.schedule_directory_change(state);
            return;
        }

        // Match on filename, not on identity: after an atomic write the path
        // is the same file to the user and a different inode to the kernel.
        let target = url.standardized_file_url().path();
        let matches = paths
            .iter()
            .any(|path| upleft_swift_text::str_eq(&FileUrl::from_path(path).standardized_file_url().path(), &target));
        if !matches {
            return;
        }
        self.record_current_snapshot_if_suppressed(state);
        self.schedule_check(state, true);
    }

    /// Whether a path under a watched directory is worth waking the owner for.
    ///
    /// Markdown files always are.  So is anything with no extension: a new
    /// `docs/` folder, or a `README` written without one.  Everything else —
    /// `.swift`, `.png`, `.tmp`, `.lock` — is noise to a sibling list, and
    /// `.DS_Store` and `.git` churn hardest of all.
    fn is_interesting(&self, path: &str, url_path: &str) -> bool {
        if upleft_swift_text::str_eq(path, url_path) {
            return true;
        }
        let name = NSString::from_str(path).lastPathComponent();
        let name_text = name.to_string();
        if upleft_swift_text::str_eq(&name_text, ".DS_Store") {
            return false;
        }
        if upleft_swift_text::contains(path, "/.git/") || upleft_swift_text::str_eq(&name_text, ".git") {
            return false;
        }
        let extension = upleft_swift_text::lowercased(&name.pathExtension().to_string());
        extension.is_empty()
            || self.interesting_extensions.iter().any(|interesting| upleft_swift_text::str_eq(interesting, &extension))
    }

    /// Delivers a coalesced `.changed` for directory watches.
    ///
    /// Contract: a directory watch reports *any* interesting churn under the
    /// root — it has no single file to attribute events to. While own-write
    /// suppression is open (the owner writing sidecars or review notes into
    /// the watched folder), deliveries are skipped so the app's own writes
    /// cannot wake an idempotent rescan; the next genuine change re-triggers
    /// one. Any future owner that treats these events as content-to-reload
    /// inherits this filter automatically and must not rely on receiving
    /// events for its own writes.
    fn schedule_directory_change(&self, state: &mut State) {
        if is_suppressing_own_writes(state, Date::now()) {
            return;
        }
        if let Some(item) = state.coalesce_work_item.take() {
            item.cancel();
        }
        let generation = state.generation;
        let weak = self.me.clone();
        let item = WorkItem::new(move || {
            Inner::upgrade_and_lock(&weak, |inner, state| {
                if state.generation != generation {
                    return;
                }
                let handler = inner.handler.clone();
                DispatchQueue::main().exec_async(move || handler(Event::Changed));
            });
        });
        item.schedule_after(&self.queue, COALESCE_INTERVAL);
        state.coalesce_work_item = Some(item);
    }

    // MARK: Polling safety net

    fn start_polling(&self, state: &mut State) {
        // SAFETY: a timer source (no handle, no mask) targeting our queue.
        let timer = unsafe {
            DispatchSource::new(
                (&raw const dispatch2::_dispatch_source_type_timer).cast_mut(),
                0,
                0,
                Some(&self.queue),
            )
        };
        let now = DispatchTime::NOW.time(0);
        timer.set_timer(now.time(nanoseconds(POLL_INTERVAL)), (POLL_INTERVAL * 1e9) as u64, POLL_LEEWAY_NANOSECONDS);
        let weak = self.me.clone();
        let handler = RcBlock::new(move || {
            Inner::upgrade_and_lock(&weak, |inner, state| inner.schedule_check(state, false));
        });
        // SAFETY: libdispatch copies the block.
        unsafe { timer.set_event_handler_with_block(RcBlock::as_ptr(&handler)) };
        timer.resume();
        state.poll_timer = Some(timer);
    }

    // MARK: Change detection

    fn schedule_check(&self, state: &mut State, force_content_digest: bool) {
        if force_content_digest {
            state.force_content_digest_on_next_check = true;
        }
        if let Some(item) = state.coalesce_work_item.take() {
            item.cancel();
        }
        let generation = state.generation;
        let write_generation = state.own_write_generation;
        let weak = self.me.clone();
        let item = WorkItem::new(move || {
            Inner::upgrade_and_lock(&weak, |inner, state| {
                if state.generation != generation || state.own_write_generation != write_generation {
                    return;
                }
                let should_force_content_digest = state.force_content_digest_on_next_check;
                state.force_content_digest_on_next_check = false;
                inner.check(state, should_force_content_digest);
            });
        });
        item.schedule_after(&self.queue, COALESCE_INTERVAL);
        state.coalesce_work_item = Some(item);
    }

    fn check(&self, state: &mut State, force_content_digest: bool) {
        state.poll_probe_count = state.poll_probe_count.wrapping_add(1);
        // Lost acknowledgement: fail open rather than stay deaf forever.
        if state.own_write_in_flight && Date::now() >= state.own_write_watchdog_deadline {
            state.own_write_in_flight = false;
            state.force_content_digest_on_next_check = true;
        }
        let url = self.url();
        let metadata = metadata(&url).unwrap_or(Snapshot::MISSING);
        let metadata_changed = !metadata_matches(&metadata, &state.last_snapshot);
        let periodic_content_probe = state.poll_probe_count % CONTENT_DIGEST_POLL_STRIDE == 0;
        if !(force_content_digest
            || metadata_changed
            || periodic_content_probe
            || !state.suppressed_snapshots.is_empty()
            || state.expected_own_snapshot.is_some())
        {
            return;
        }

        let now = snapshot(&url, true);

        if is_suppressing_own_writes(state, Date::now()) {
            if now == state.last_snapshot {
                return;
            }
            if state.expected_own_snapshot.as_ref() == Some(&now) {
                // Consume our own replacement, but do not discard an external
                // snapshot observed earlier in the same generation.
                state.last_snapshot = now;
            } else {
                record_suppressed_snapshot(state, now);
            }
            self.schedule_check(state, false);
            return;
        }

        if !state.suppressed_snapshots.is_empty() || state.expected_own_snapshot.is_some() {
            self.reconcile_suppressed_change(state, now);
            return;
        }

        if now == state.last_snapshot {
            return;
        }
        let previous = state.last_snapshot.clone();

        if now.size < 0 && previous.size >= 0 {
            // Rename following stays immediate — the old inode is findable
            // right now, and deferring it would stall Save As / rename UX.
            if previous.inode != 0
                && let Some(relocated) = locate(&previous, &url.deleting_last_path_component())
            {
                self.retarget_on_queue(state, relocated.resolving_symlinks_in_path());
                let new_url = self.url();
                self.deliver(Event::Renamed(new_url));
                return;
            }
            self.schedule_removal_reprobe(state);
            return;
        }

        if let Some(item) = state.pending_removal_probe.take() {
            item.cancel();
        }
        state.last_snapshot = now.clone();
        let event = self.event(state, &now, &previous);
        self.deliver(event);
    }

    /// One bounded re-probe before committing a removal. When the probe runs,
    /// the same snapshot-and-classify logic decides: the file came back (an
    /// external atomic replacement) and is reported as the change it became,
    /// or it is still gone and `.removed` is delivered.
    fn schedule_removal_reprobe(&self, state: &mut State) {
        if state.pending_removal_probe.is_some() {
            return;
        }
        let generation = state.generation;
        let weak = self.me.clone();
        let item = WorkItem::new(move || {
            Inner::upgrade_and_lock(&weak, |inner, state| {
                if state.generation != generation {
                    return;
                }
                state.pending_removal_probe = None;
                inner.resolve_tentative_removal(state);
            });
        });
        item.schedule_after(&self.queue, state.removal_probe_interval);
        state.pending_removal_probe = Some(item);
    }

    fn resolve_tentative_removal(&self, state: &mut State) {
        let now = snapshot(&self.url(), true);
        if now == state.last_snapshot {
            return;
        }
        let previous = std::mem::replace(&mut state.last_snapshot, now.clone());
        let event = self.event(state, &now, &previous);
        self.deliver(event);
    }

    /// Reconciles observations made while an own-write window was open.  The
    /// final snapshot is always committed, including when an external change
    /// preceded our own bytes; this prevents a later poll from re-reporting
    /// the suppressed own write.  Only the external snapshot is delivered.
    fn reconcile_suppressed_change(&self, state: &mut State, final_snapshot: Snapshot) {
        let previous = state.suppression_baseline.clone().unwrap_or_else(|| state.last_snapshot.clone());
        let expected = state.expected_own_snapshot.clone();
        let mut candidates: Vec<Snapshot> = state
            .suppressed_snapshots
            .iter()
            .filter(|snapshot| Some(*snapshot) != expected.as_ref() && **snapshot != previous)
            .cloned()
            .collect();
        if final_snapshot != previous && Some(&final_snapshot) != expected.as_ref() {
            candidates.push(final_snapshot.clone());
        }
        // Atomic replacement can expose a brief missing path between unlink
        // and rename.  Once the expected own bytes are present, that transient
        // is part of our save rather than an external removal event.
        if Some(&final_snapshot) == expected.as_ref()
            && !candidates.is_empty()
            && candidates.iter().all(|snapshot| snapshot.size < 0)
        {
            candidates.clear();
        }
        let observed_external = candidates.last().cloned();

        state.suppression_baseline = None;
        state.expected_own_snapshot = None;
        state.suppressed_snapshots.clear();
        state.own_write_in_flight = false;
        state.settle_grace_until = DISTANT_PAST;
        state.last_snapshot = final_snapshot;

        if let Some(observed_external) = observed_external {
            let event = self.event(state, &observed_external, &previous);
            self.deliver(event);
        }
    }

    fn record_current_snapshot_if_suppressed(&self, state: &mut State) {
        if !is_suppressing_own_writes(state, Date::now()) {
            return;
        }
        let snapshot = snapshot(&self.url(), true);
        if snapshot == state.last_snapshot {
            return;
        }
        if state.expected_own_snapshot.as_ref() == Some(&snapshot) {
            state.last_snapshot = snapshot;
        } else {
            record_suppressed_snapshot(state, snapshot);
        }
    }

    fn deliver(&self, event: Event) {
        let handler = self.handler.clone();
        DispatchQueue::main().exec_async(move || handler(event));
    }

    fn event(&self, state: &mut State, now: &Snapshot, previous: &Snapshot) -> Event {
        // An atomic save deletes the original file before renaming the new one
        // in; if we act on the deletion we would close the document or report
        // it gone.  Check if a file with the same inode/size exists in the
        // directory before treating this as a true removal.  (This is also what
        // lets a file renamed in Finder keep its open window.)
        //
        // Sibling scan: if the path went away, scan the parent directory for a
        // file matching the *old* inode and size.  This catches external renames
        // (`mv doc.md archive.md`) that would otherwise leave the watcher
        // resolving and the app used to stop watching for good.  The inode is
        // still there under a new name, so look for it before declaring the
        // document gone.
        if now.size < 0
            && previous.inode != 0
            && previous.size >= 0
            && let Some(relocated) = locate(previous, &self.url().deleting_last_path_component())
        {
            self.retarget_on_queue(state, relocated.resolving_symlinks_in_path());
            return Event::Renamed(self.url());
        }

        if now.size < 0 {
            Event::Removed
        } else if previous.size < 0 {
            Event::Restored
        } else {
            Event::Changed
        }
    }
}

/// Whether own-write suppression is currently absorbing observations.
/// In-flight covers the whole save transaction; the settle grace only keeps
/// late events from the just-landed write from racing the state
/// reconciliation. Correctness never depends on the grace expiring.
fn is_suppressing_own_writes(state: &State, now: Date) -> bool {
    state.own_write_in_flight || now < state.settle_grace_until
}

fn record_suppressed_snapshot(state: &mut State, snapshot: Snapshot) {
    if state.suppression_baseline.as_ref() == Some(&snapshot) || state.suppressed_snapshots.contains(&snapshot) {
        return;
    }
    state.suppressed_snapshots.push(snapshot);
}

// MARK: - Snapshots

/// Finds the file that used to be at the watched path, shallowly, in one
/// directory.
///
/// Matched on the *whole* snapshot — inode, size, modification time, and
/// content digest — not on the inode alone: a rename changes none of these,
/// while an inode number freed by a genuine deletion can be handed straight
/// back to an unrelated new file.
///
/// Only ever runs when the watched file has just disappeared, so the cost is
/// paid once per removal rather than once per event.  Capped because "agent
/// output folder" and "ten thousand files" are not mutually exclusive.
fn locate(wanted: &Snapshot, directory: &FileUrl) -> Option<FileUrl> {
    // `FileManager.contentsOfDirectory(atPath:)`: directory order, without
    // `.` and `..`.
    let entries = std::fs::read_dir(directory.path()).ok()?;
    for entry in entries.take(4096) {
        let Ok(entry) = entry else { continue };
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
        let candidate = directory.appending_path_component(&name);
        let Some(metadata) = metadata(&candidate) else { continue };
        if !(metadata.modified == wanted.modified && metadata.size == wanted.size && metadata.inode == wanted.inode) {
            continue;
        }
        // Only hash a candidate whose inode/metadata could actually be the
        // relocated file; a busy sibling directory should stay cheap.
        if wanted.content_digest.is_none() || snapshot(&candidate, true) == *wanted {
            return Some(candidate);
        }
    }
    None
}

fn snapshot(url: &FileUrl, include_content_digest: bool) -> Snapshot {
    let path = url.path();
    for _ in 0..3 {
        let Some(before) = metadata(url) else { return Snapshot::MISSING };
        if !(include_content_digest || before != Snapshot::MISSING) {
            return Snapshot::MISSING;
        }
        if !include_content_digest {
            return before;
        }
        let Ok(data) = std::fs::read(&path) else { return before };
        let Some(after) = metadata(url) else { return Snapshot::MISSING };
        if before != after {
            continue;
        }
        return before.with_content_digest(content_digest(&data));
    }

    // A file that is being rewritten continuously is still represented by
    // its latest metadata.  The next FSEvents/poll pass will retry the
    // content signature once the writer settles.
    metadata(url).unwrap_or(Snapshot::MISSING)
}

fn metadata_matches(lhs: &Snapshot, rhs: &Snapshot) -> bool {
    lhs.modified == rhs.modified && lhs.size == rhs.size && lhs.inode == rhs.inode
}

fn metadata(url: &FileUrl) -> Option<Snapshot> {
    let path = CString::new(url.path()).ok()?;
    // SAFETY: `stat` fills the zeroed struct for a valid C path.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::stat(path.as_ptr(), &mut st) } != 0 {
        return None;
    }
    let seconds = st.st_mtime as f64;
    let nanos = st.st_mtime_nsec as f64 / 1_000_000_000.0;
    Some(Snapshot {
        modified: Date::from_1970(seconds + nanos),
        size: st.st_size as isize,
        inode: st.st_ino,
        content_digest: None,
    })
}

fn content_digest(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// `Int64(seconds * Double(NSEC_PER_SEC))`, as `DispatchTime + Double` clamps it.
fn nanoseconds(seconds: f64) -> i64 {
    (seconds * 1_000_000_000.0) as i64
}

// MARK: - DispatchWorkItem

/// `DispatchWorkItem(block:)`: a `dispatch_block_create_with_qos_class`
/// block (no flags, unspecified QoS), so `cancel()` is `dispatch_block_cancel`.
/// Shared with `markdown_document`, whose debounce items are main-queue work
/// items.
pub(crate) struct WorkItem {
    block: RcBlock<dyn Fn()>,
}

// SAFETY: dispatch blocks are thread-safe objects; the closures captured
// here hold only `Weak<Inner>` and plain values, which are `Send + Sync`.
unsafe impl Send for WorkItem {}

impl WorkItem {
    pub(crate) fn new(body: impl Fn() + Send + 'static) -> WorkItem {
        let body = RcBlock::new(body);
        // SAFETY: `dispatch_block_create_with_qos_class` copies the block and
        // returns a new one we own. The flags are `[]`: dispatch2 does not
        // export `dispatch_block_flags_t`, a transparent `c_ulong`, so the
        // zero value is built with `zeroed`.
        let created = unsafe {
            dispatch2::dispatch_block_create_with_qos_class(
                std::mem::zeroed(),
                DispatchQoS::Unspecified,
                0,
                RcBlock::as_ptr(&body),
            )
        };
        // SAFETY: a +1 block from `dispatch_block_create*`.
        let block = unsafe { RcBlock::from_raw(created) }.expect("dispatch_block_create returned a block");
        WorkItem { block }
    }

    /// `queue.asyncAfter(deadline: .now() + seconds, execute: item)`.
    pub(crate) fn schedule_after(&self, queue: &DispatchQueue, seconds: f64) {
        let deadline = DispatchTime::NOW.time(0).time(nanoseconds(seconds));
        // SAFETY: `dispatch_after` copies (retains) the block.
        unsafe { DispatchQueue::exec_after_with_block(deadline, queue, RcBlock::as_ptr(&self.block)) };
    }

    pub(crate) fn cancel(&self) {
        // SAFETY: a block made by `dispatch_block_create*`.
        unsafe { dispatch2::dispatch_block_cancel(RcBlock::as_ptr(&self.block)) };
    }
}

// MARK: - FSEvents

/// Weak box so FSEvents callbacks never touch a deallocated watcher after
/// `stop()` — the stream may still deliver one last burst on its queue.
struct StreamContext {
    watcher: Mutex<Weak<Inner>>,
}

struct StreamRef(*mut c_void);

// SAFETY: FSEventStream calls are made only under the state mutex.
unsafe impl Send for StreamRef {}

#[repr(C)]
struct FSEventStreamContext {
    version: isize,
    info: *mut c_void,
    retain: Option<extern "C" fn(*const c_void) -> *const c_void>,
    release: Option<extern "C" fn(*const c_void)>,
    copy_description: Option<extern "C" fn(*const c_void) -> *const c_void>,
}

type FSEventStreamCallback = extern "C" fn(
    stream: *const c_void,
    info: *mut c_void,
    num_events: usize,
    event_paths: *mut c_void,
    event_flags: *const u32,
    event_ids: *const u64,
);

const K_FS_EVENT_STREAM_CREATE_FLAG_NO_DEFER: u32 = 0x0000_0002;
const K_FS_EVENT_STREAM_CREATE_FLAG_WATCH_ROOT: u32 = 0x0000_0004;
const K_FS_EVENT_STREAM_CREATE_FLAG_FILE_EVENTS: u32 = 0x0000_0010;
const K_FS_EVENT_STREAM_EVENT_ID_SINCE_NOW: u64 = 0xFFFF_FFFF_FFFF_FFFF;

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn FSEventStreamCreate(
        allocator: *const c_void,
        callback: FSEventStreamCallback,
        context: *mut FSEventStreamContext,
        paths_to_watch: *const c_void,
        since_when: u64,
        latency: f64,
        flags: u32,
    ) -> *mut c_void;
    fn FSEventStreamSetDispatchQueue(stream: *mut c_void, queue: *mut c_void);
    fn FSEventStreamStart(stream: *mut c_void) -> u8;
    fn FSEventStreamStop(stream: *mut c_void);
    fn FSEventStreamInvalidate(stream: *mut c_void);
    fn FSEventStreamRelease(stream: *mut c_void);
}

extern "C" fn stream_context_retain(info: *const c_void) -> *const c_void {
    if !info.is_null() {
        // SAFETY: `info` is an `Arc<StreamContext>` pointer.
        unsafe { Arc::increment_strong_count(info as *const StreamContext) };
    }
    info
}

extern "C" fn stream_context_release(info: *const c_void) {
    if !info.is_null() {
        // SAFETY: balances `stream_context_retain`.
        unsafe { Arc::decrement_strong_count(info as *const StreamContext) };
    }
}

extern "C" fn stream_callback(
    _stream: *const c_void,
    info: *mut c_void,
    count: usize,
    event_paths: *mut c_void,
    _flags: *const u32,
    _ids: *const u64,
) {
    if info.is_null() {
        return;
    }
    // SAFETY: the stream holds a retain on the context for its lifetime.
    let context = unsafe { &*(info as *const StreamContext) };
    let weak = context.watcher.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
    let Some(watcher) = weak.upgrade() else { return };
    // Without `kFSEventStreamCreateFlagUseCFTypes`, `eventPaths` is a C array
    // of UTF-8 path pointers.
    let pointers = event_paths as *const *const c_char;
    let mut paths = Vec::with_capacity(count);
    for index in 0..count {
        // SAFETY: FSEvents passes `count` path pointers.
        let pointer = unsafe { *pointers.add(index) };
        if pointer.is_null() {
            continue;
        }
        // SAFETY: a NUL-terminated path; `String(cString:)` repairs invalid UTF-8.
        paths.push(unsafe { CStr::from_ptr(pointer) }.to_string_lossy().into_owned());
    }
    let mut state = watcher.lock();
    watcher.handle_stream_events(&mut state, &paths);
}
