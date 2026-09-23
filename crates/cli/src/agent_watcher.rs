//! Port of `Sources/drdownright/AgentWatcher.swift`: a directory watcher for
//! `down watch` — the fallback path for agents that have no hook system.
//!
//! This deliberately mirrors the app's own `FileWatcher` in the one decision
//! that matters: **watch the parent directory, never the file**. Every agent
//! CLI writes atomically — temp file, then `rename()` over the target — which
//! unlinks the original inode and silently kills any watch held against it.
//! A vnode watch on the file would work perfectly in testing and then stop
//! firing the moment a real agent touched it.
//!
//! Coalescing is not an optimisation here either. A single agent edit
//! routinely produces several FSEvents (write, rename, attribute change), and
//! one `open` process per event would spawn a handful of launches for one
//! logical change.
//!
//! The stream is the same CoreServices FSEvents stream with the same flags,
//! latency and dispatch queue (a serial `.utility` queue labelled
//! `com.ezzy.downright.agentwatcher`); a cancelled `DispatchWorkItem` is a
//! generation counter here, which has the same effect.

use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, Weak};

use dispatch2::{DispatchQoS, DispatchQueue, DispatchQueueAttr, DispatchRetained, DispatchTime};
use objc2_core_foundation::{CFArray, CFString};
use objc2_core_services::{
    ConstFSEventStreamRef, FSEventStreamContext, FSEventStreamCreate, FSEventStreamEventFlags, FSEventStreamEventId,
    FSEventStreamInvalidate, FSEventStreamRef, FSEventStreamRelease, FSEventStreamSetDispatchQueue,
    FSEventStreamStart, FSEventStreamStop, kFSEventStreamCreateFlagFileEvents, kFSEventStreamCreateFlagNoDefer,
    kFSEventStreamEventIdSinceNow,
};
use upleft_foundation::foundation_io;
use upleft_foundation::url::FileUrl;
use upleft_swift_text as swift_text;

use crate::markdown_cli;

/// Enough of a file to tell "the agent rewrote this" from "something touched
/// its metadata". Deliberately *not* a hash: this runs on every event in a
/// directory that may be churning, and reading file contents to decide
/// whether to read file contents is the wrong trade.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Signature {
    /// Modification date, seconds since the reference date.
    pub modified: f64,
    pub size: i64,
}

impl Signature {
    /// `Signature(path:)`: `attributesOfItem(atPath:)` (which does not follow
    /// a final symbolic link), `nil` when either attribute is missing.
    pub fn new(path: &str) -> Option<Signature> {
        let attributes = foundation_io::attributes_of_item(path).ok()?;
        Some(Signature { modified: attributes.modification_date?, size: attributes.size_int? })
    }
}

/// A `Set<String>`: members compare by Swift `==` (canonical equivalence).
#[derive(Clone, Debug, Default)]
pub struct StringSet {
    members: HashMap<String, String>,
}

impl StringSet {
    pub fn new() -> StringSet {
        StringSet::default()
    }

    /// `insert(_:).inserted`.
    pub fn insert(&mut self, value: &str) -> bool {
        let key = swift_text::string_key(value);
        if self.members.contains_key(&key) {
            return false;
        }
        self.members.insert(key, value.to_owned());
        true
    }

    pub fn contains(&self, value: &str) -> bool {
        self.members.contains_key(&swift_text::string_key(value))
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn clear(&mut self) {
        self.members.clear();
    }
}

impl PartialEq for StringSet {
    fn eq(&self, other: &Self) -> bool {
        self.members.len() == other.members.len() && self.members.keys().all(|key| other.members.contains_key(key))
    }
}

impl<const N: usize> From<[&str; N]> for StringSet {
    fn from(values: [&str; N]) -> Self {
        let mut set = StringSet::new();
        for value in values {
            set.insert(value);
        }
        set
    }
}

/// The directories FSEvents is asked to watch, and the file filter derived
/// from the roots. Split out from `start()` because it is the part worth
/// reasoning about: a root that is a file contributes its parent directory
/// to the watch set and its own path to the allow-list, while a root that is
/// a directory contributes itself and allows everything beneath it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WatchPlan {
    pub directories: Vec<String>,
    /// Exact file paths to allow. Empty means "allow anything under the
    /// watched directories".
    pub allowed_files: StringSet,
}

/// `AgentWatcher`.
pub struct AgentWatcher {
    inner: Arc<Inner>,
}

struct Inner {
    roots: Vec<FileUrl>,
    debounce: f64,
    handler: Box<dyn Fn(Vec<FileUrl>) + Send + Sync>,
    queue: DispatchRetained<DispatchQueue>,
    state: Mutex<State>,
}

struct State {
    stream: Option<StreamPointer>,
    stream_context: Option<Arc<StreamContext>>,
    /// Resolved once at `start()`. Recomputing it per event would stat the
    /// file system on every write in a busy directory to answer a question
    /// whose answer cannot change while the stream is running.
    watch_plan: WatchPlan,
    /// Paths accumulated since the last flush, in first-seen order.
    pending: Vec<String>,
    pending_seen: StringSet,
    /// The live `DispatchWorkItem`: a scheduled flush runs only while its
    /// generation is current, so bumping it cancels.
    flush: Option<u64>,
    flush_generation: u64,
    /// Content signature of every path already reported, so a
    /// metadata-only event cannot report the same unchanged bytes twice.
    /// Bounded below. Keyed by Swift `String` equality.
    reported: HashMap<String, Signature>,
}

struct StreamPointer(FSEventStreamRef);

// SAFETY: the stream is only created, started, stopped and released on the
// watcher's serial queue.
unsafe impl Send for StreamPointer {}

struct StreamContext {
    watcher: Mutex<Weak<Inner>>,
}

/// `queueKey`: `dispatch_queue_set_specific` marks the watcher's queue.
static QUEUE_KEY: u8 = 0;

impl AgentWatcher {
    /// How long to wait for a burst of events to settle before reporting.
    /// 300ms is comfortably longer than the write→rename gap of an atomic
    /// save and comfortably shorter than a human noticing a delay.
    pub const DEFAULT_DEBOUNCE: f64 = 0.3;

    /// Ceiling on the signature table for a watch left running over a large
    /// tree. Clearing it is safe: a forgotten path is reported once more,
    /// which is a duplicate open at worst, never a missed change.
    const SIGNATURE_LIMIT: usize = 4096;

    /// - `roots`: files or directories to watch. A file root is watched
    ///   through its parent directory and filtered back down to that one path.
    /// - `debounce`: quiet period before a burst is reported.
    /// - `handler`: called on the watcher's queue with the coalesced Markdown
    ///   files that changed. Never called with an empty array.
    pub fn new(roots: Vec<FileUrl>, debounce: f64, handler: impl Fn(Vec<FileUrl>) + Send + Sync + 'static) -> AgentWatcher {
        let attributes = DispatchQueueAttr::with_qos_class(None, DispatchQoS::Utility, 0);
        let queue = DispatchQueue::new("com.ezzy.downright.agentwatcher", Some(&attributes));
        queue.set_specific(NonNull::from(&QUEUE_KEY).cast(), || {});
        AgentWatcher {
            inner: Arc::new(Inner {
                roots: roots.iter().map(FileUrl::standardized_file_url).collect(),
                debounce,
                handler: Box::new(handler),
                queue,
                state: Mutex::new(State {
                    stream: None,
                    stream_context: None,
                    watch_plan: WatchPlan::default(),
                    pending: Vec::new(),
                    pending_seen: StringSet::new(),
                    flush: None,
                    flush_generation: 0,
                    reported: HashMap::new(),
                }),
            }),
        }
    }

    /// `AgentWatcher.plan(for:)`.
    pub fn plan(roots: &[FileUrl]) -> WatchPlan {
        let mut directories: Vec<String> = Vec::new();
        let mut allowed_files = StringSet::new();
        let mut saw_directory = false;
        for root in roots {
            let (exists, is_directory) = foundation_io::file_exists_is_directory(&root.path());
            if exists && is_directory {
                saw_directory = true;
                directories.push(root.path());
            } else {
                // A file that does not exist yet is still a legitimate target —
                // agents create files as well as rewrite them — so this does
                // not require existence, only that the parent directory is real.
                directories.push(root.deleting_last_path_component().path());
                allowed_files.insert(&root.path());
            }
        }
        let mut seen = StringSet::new();
        directories.retain(|directory| seen.insert(directory));
        // A directory root subsumes any file root under it; mixing the two
        // would otherwise let the allow-list suppress everything the
        // directory wanted.
        WatchPlan { directories, allowed_files: if saw_directory { StringSet::new() } else { allowed_files } }
    }

    /// True when an event path should be reported, given a plan.
    pub fn accepts(path: &str, plan: &WatchPlan) -> bool {
        if !markdown_cli::is_markdown_path(path) {
            return false;
        }
        if plan.allowed_files.is_empty() {
            return true;
        }
        plan.allowed_files.contains(path)
    }

    /// `start()`: returns whether the stream started.
    pub fn start(&self) -> bool {
        self.stop();
        let inner = self.inner.clone();
        on_queue(&self.inner.queue, move || start_on_queue(&inner))
    }

    pub fn stop(&self) {
        let inner = self.inner.clone();
        on_queue(&self.inner.queue, move || stop_on_queue(&inner));
    }
}

impl Drop for AgentWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

fn is_on_queue() -> bool {
    !unsafe { dispatch2::dispatch_get_specific(NonNull::from(&QUEUE_KEY).cast()) }.is_null()
}

/// `onQueue(_:)`: runs `body` synchronously on the watcher's queue.
fn on_queue<T: Send>(queue: &DispatchQueue, body: impl FnOnce() -> T + Send) -> T {
    if is_on_queue() {
        return body();
    }
    let mut result = None;
    queue.exec_sync(|| result = Some(body()));
    result.expect("exec_sync ran the body")
}

fn start_on_queue(inner: &Arc<Inner>) -> bool {
    let plan = AgentWatcher::plan(&inner.roots);
    if plan.directories.is_empty() {
        return false;
    }
    let directories = plan.directories.clone();
    let mut state = inner.state.lock().unwrap();
    state.watch_plan = plan;

    let context_box = Arc::new(StreamContext { watcher: Mutex::new(Arc::downgrade(inner)) });
    state.stream_context = Some(context_box.clone());
    let mut context = FSEventStreamContext {
        version: 0,
        info: Arc::as_ptr(&context_box) as *mut c_void,
        retain: Some(retain_context),
        release: Some(release_context),
        copyDescription: None,
    };
    // Without `kFSEventStreamCreateFlagUseCFTypes` the callback receives a
    // plain `char **`. That is deliberate: the CFTypes form hands back a
    // `CFArrayRef`, and reading one as a C array does not fail loudly — it
    // decodes garbage that quietly fails the Markdown filter, so the watcher
    // runs forever and simply never reports anything.
    //
    // `FileEvents` is what makes the callback report individual paths rather
    // than the directory; without it every event would arrive as the folder
    // and the filter below could not tell Markdown from build output.
    let flags = kFSEventStreamCreateFlagFileEvents | kFSEventStreamCreateFlagNoDefer;
    let paths: Vec<objc2_core_foundation::CFRetained<CFString>> =
        directories.iter().map(|directory| CFString::from_str(directory)).collect();
    let array = CFArray::from_retained_objects(&paths);
    let stream = unsafe {
        FSEventStreamCreate(
            None,
            Some(stream_callback),
            &mut context,
            (*array).as_ref(),
            kFSEventStreamEventIdSinceNow,
            inner.debounce / 2.0,
            flags,
        )
    };
    if stream.is_null() {
        return false;
    }
    state.stream = Some(StreamPointer(stream));
    unsafe {
        FSEventStreamSetDispatchQueue(stream, Some(&inner.queue));
        FSEventStreamStart(stream)
    }
}

fn stop_on_queue(inner: &Arc<Inner>) {
    let mut state = inner.state.lock().unwrap();
    if let Some(StreamPointer(stream)) = state.stream.take() {
        unsafe {
            FSEventStreamStop(stream);
            FSEventStreamInvalidate(stream);
            FSEventStreamRelease(stream);
        }
    }
    if let Some(context) = state.stream_context.take() {
        *context.watcher.lock().unwrap() = Weak::new();
    }
    state.flush = None;
    state.flush_generation += 1;
    state.pending = Vec::new();
    state.pending_seen.clear();
}

unsafe extern "C-unwind" fn retain_context(info: *const c_void) -> *const c_void {
    if info.is_null() {
        return std::ptr::null();
    }
    unsafe { Arc::increment_strong_count(info as *const StreamContext) };
    info
}

unsafe extern "C-unwind" fn release_context(info: *const c_void) {
    if info.is_null() {
        return;
    }
    unsafe { Arc::decrement_strong_count(info as *const StreamContext) };
}

unsafe extern "C-unwind" fn stream_callback(
    _stream: ConstFSEventStreamRef,
    info: *mut c_void,
    count: usize,
    paths: NonNull<c_void>,
    _flags: NonNull<FSEventStreamEventFlags>,
    _ids: NonNull<FSEventStreamEventId>,
) {
    if info.is_null() {
        return;
    }
    let context = unsafe { &*(info as *const StreamContext) };
    let Some(watcher) = context.watcher.lock().unwrap().upgrade() else { return };
    let raw = paths.as_ptr() as *const *const c_char;
    let mut changed = Vec::new();
    for index in 0..count {
        let entry = unsafe { *raw.add(index) };
        if entry.is_null() {
            continue;
        }
        changed.push(unsafe { CStr::from_ptr(entry) }.to_string_lossy().into_owned());
    }
    if changed.is_empty() {
        return;
    }
    absorb(&watcher, changed);
}

fn absorb(inner: &Arc<Inner>, paths: Vec<String>) {
    let weak = Arc::downgrade(inner);
    inner.queue.exec_async(move || {
        let Some(inner) = weak.upgrade() else { return };
        {
            let mut state = inner.state.lock().unwrap();
            for path in &paths {
                if !AgentWatcher::accepts(path, &state.watch_plan) {
                    continue;
                }
                let standardized = FileUrl::from_path(path).standardized_file_url().path();
                if state.pending_seen.insert(&standardized) {
                    state.pending.push(standardized);
                }
            }
            if state.pending.is_empty() {
                return;
            }
        }
        schedule_flush(&inner);
    });
}

/// `DispatchTime.now() + seconds`, clamped as Swift's `toInt64Clamped`.
fn deadline_after(seconds: f64) -> DispatchTime {
    let nanoseconds = seconds * 1_000_000_000.0;
    let delta = if nanoseconds.is_nan() || nanoseconds >= i64::MAX as f64 {
        i64::MAX
    } else if nanoseconds <= i64::MIN as f64 {
        i64::MIN
    } else {
        nanoseconds as i64
    };
    DispatchTime::NOW.time(delta)
}

fn schedule_flush(inner: &Arc<Inner>) {
    let generation = {
        let mut state = inner.state.lock().unwrap();
        state.flush_generation += 1;
        state.flush = Some(state.flush_generation);
        state.flush_generation
    };
    let weak = Arc::downgrade(inner);
    let _ = inner.queue.after(deadline_after(inner.debounce), move || {
        let Some(inner) = weak.upgrade() else { return };
        let urls = {
            let mut state = inner.state.lock().unwrap();
            if state.flush != Some(generation) {
                return;
            }
            state.flush = None;
            let batch = std::mem::take(&mut state.pending);
            state.pending_seen.clear();
            if batch.is_empty() {
                return;
            }
            // Signatures are taken at flush time, not at event time. A file
            // written and then deleted inside one debounce window has no
            // signature and drops out here, so the app is never launched for
            // a path that is already gone.
            //
            // This is also what breaks the feedback loop that makes a naive
            // version of this watcher unusable: opening a document updates
            // the file's metadata, macOS reports that as another event on the
            // same path, and the watcher opens it again — forever. A metadata
            // touch leaves modification date and size alone, so it stops here.
            if state.reported.len() > AgentWatcher::SIGNATURE_LIMIT {
                state.reported.clear();
            }
            let mut urls = Vec::new();
            for path in batch {
                let Some(signature) = Signature::new(&path) else { continue };
                let key = swift_text::string_key(&path);
                if state.reported.get(&key) == Some(&signature) {
                    continue;
                }
                state.reported.insert(key, signature);
                urls.push(FileUrl::from_path(&path));
            }
            urls
        };
        if urls.is_empty() {
            return;
        }
        (inner.handler)(urls);
    });
}
