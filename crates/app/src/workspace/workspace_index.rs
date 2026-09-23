//! Port of `Sources/DownrightApp/Workspace/WorkspaceIndex.swift`: the
//! optional, local-only workspace index.
//!
//! Threading follows the Swift. [`WorkspaceIndex`] is main-thread state
//! (`@MainActor`): `start` bumps the revision, cancels the scan in flight and
//! hands a new one to a user-initiated global queue (`Task.detached(priority:
//! .userInitiated)`); the scan reads files on a small worker pool
//! (`DispatchQueue.concurrentPerform`) and the finished snapshot hops back to
//! the main queue, where it is published only if its task was not cancelled
//! and its revision is still the latest. Nothing here blocks the main thread.
//!
//! Paths follow Swift's `URL` ([`FileUrl`]); the default enumerator is
//! `FileManager`'s own, called through objc2, so the order files are visited
//! in (which decides what `maximumFiles` keeps) is Foundation's.

use std::cell::RefCell;
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use dispatch2::{DispatchQoS, DispatchQueue, GlobalQueueIdentifier, MainThreadBound};
use objc2::MainThreadMarker;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2_foundation::{
    NSArray, NSComparisonResult, NSDirectoryEnumerationOptions, NSFileManager, NSNumber, NSString, NSURL,
    NSURLIsDirectoryKey, NSURLIsRegularFileKey, NSURLFileSizeKey, NSURLResourceKey,
};
use upleft_core::parser::MarkdownParser;
use upleft_core::{InlineKind, NSRange, ParseOptions, ParsedDocument};
use upleft_foundation::url::FileUrl;
use upleft_swift_text as swift;

use crate::support::find_engine::file_size;

// MARK: - Cancellation

/// `WorkspaceCancellationToken`.
#[derive(Default)]
pub struct CancellationToken {
    cancelled: AtomicBool,
}

impl CancellationToken {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
}

// MARK: - Swift `Set<String>`

/// A `Set<String>`: membership by canonical equivalence (Swift's `String ==`
/// and hashing), keyed by each string's NFC form.
#[derive(Clone, Debug, Default)]
pub struct StringSet {
    keys: HashSet<String>,
}

impl StringSet {
    pub fn new<I: IntoIterator<Item = S>, S: AsRef<str>>(items: I) -> StringSet {
        StringSet { keys: items.into_iter().map(|item| swift::string_key(item.as_ref())).collect() }
    }

    pub fn contains(&self, value: &str) -> bool {
        if value.is_ascii() {
            return self.keys.contains(value);
        }
        self.keys.contains(&swift::string_key(value))
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

impl PartialEq for StringSet {
    fn eq(&self, other: &Self) -> bool {
        self.keys == other.keys
    }
}

impl Eq for StringSet {}

// MARK: - Policy

/// `WorkspaceIndexPolicy.init`'s arguments, with its defaults.
#[derive(Clone, Debug)]
pub struct WorkspaceIndexPolicyInit {
    pub markdown_extensions: Vec<String>,
    pub ignored_directory_names: Vec<String>,
    pub ignores_hidden_directories: bool,
    pub maximum_files: isize,
    pub maximum_bytes_per_file: i64,
    pub maximum_total_bytes: i64,
    pub read_concurrency: isize,
}

impl Default for WorkspaceIndexPolicyInit {
    fn default() -> Self {
        WorkspaceIndexPolicyInit {
            markdown_extensions: ["md", "markdown", "mdown", "mkd", "mdx", "mdc", "qmd", "rmd"]
                .map(str::to_owned)
                .to_vec(),
            ignored_directory_names: [".git", ".build", "build", "vendor", "node_modules", "DerivedData", ".swiftpm", "Pods"]
                .map(str::to_owned)
                .to_vec(),
            ignores_hidden_directories: true,
            maximum_files: 10_000,
            maximum_bytes_per_file: 10 * 1024 * 1024,
            maximum_total_bytes: 100 * 1024 * 1024,
            read_concurrency: 4,
        }
    }
}

/// Files that can be indexed by the optional workspace surface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceIndexPolicy {
    pub markdown_extensions: StringSet,
    pub ignored_directory_names: StringSet,
    pub ignores_hidden_directories: bool,
    pub maximum_files: isize,
    pub maximum_bytes_per_file: i64,
    pub maximum_total_bytes: i64,
    pub read_concurrency: isize,
}

impl Default for WorkspaceIndexPolicy {
    /// `WorkspaceIndexPolicy.default`.
    fn default() -> Self {
        WorkspaceIndexPolicy::new(WorkspaceIndexPolicyInit::default())
    }
}

impl WorkspaceIndexPolicy {
    pub fn new(init: WorkspaceIndexPolicyInit) -> WorkspaceIndexPolicy {
        WorkspaceIndexPolicy {
            markdown_extensions: StringSet::new(init.markdown_extensions.iter().map(|e| swift::lowercased(e))),
            ignored_directory_names: StringSet::new(&init.ignored_directory_names),
            ignores_hidden_directories: init.ignores_hidden_directories,
            maximum_files: init.maximum_files.max(1),
            maximum_bytes_per_file: init.maximum_bytes_per_file.max(1),
            maximum_total_bytes: init.maximum_total_bytes.max(1),
            read_concurrency: init.read_concurrency.max(1).min(16),
        }
    }

    pub fn accepts(&self, url: &FileUrl, is_directory: bool) -> bool {
        if is_directory {
            let name = url.last_path_component();
            if self.ignored_directory_names.contains(&name) {
                return false;
            }
            if self.ignores_hidden_directories && swift::has_prefix(&name, ".") {
                return false;
            }
            return true;
        }
        self.markdown_extensions.contains(&swift::lowercased(&url.path_extension()))
    }
}

// MARK: - Entries

#[derive(Clone, Debug)]
pub struct WorkspaceHeading {
    pub title: String,
    pub range: NSRange,
    pub level: isize,
}

#[derive(Clone, Debug)]
pub struct WorkspaceFrontMatterField {
    pub key: String,
    pub value: String,
    pub range: NSRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorkspaceLinkKind {
    Markdown,
    Wikilink,
}

impl WorkspaceLinkKind {
    pub fn raw_value(self) -> &'static str {
        match self {
            WorkspaceLinkKind::Markdown => "markdown",
            WorkspaceLinkKind::Wikilink => "wikilink",
        }
    }
}

#[derive(Clone, Debug)]
pub struct WorkspaceLink {
    pub destination: String,
    pub range: NSRange,
    pub kind: WorkspaceLinkKind,
}

#[derive(Clone, Debug)]
pub struct WorkspaceIndexEntry {
    /// `url.standardizedFileURL.path`.
    pub id: String,
    pub url: FileUrl,
    pub relative_path: String,
    pub text: String,
    pub headings: Vec<WorkspaceHeading>,
    pub front_matter: Vec<WorkspaceFrontMatterField>,
    pub links: Vec<WorkspaceLink>,
    pub byte_count: i64,
}

impl WorkspaceIndexEntry {
    pub fn new(
        url: FileUrl,
        relative_path: impl Into<String>,
        text: impl Into<String>,
        headings: Vec<WorkspaceHeading>,
        front_matter: Vec<WorkspaceFrontMatterField>,
        links: Vec<WorkspaceLink>,
        byte_count: i64,
    ) -> WorkspaceIndexEntry {
        WorkspaceIndexEntry {
            id: url.standardized_file_url().path(),
            url,
            relative_path: relative_path.into(),
            text: text.into(),
            headings,
            front_matter,
            links,
            byte_count,
        }
    }
}

// Swift's synthesized `Equatable` compares every stored property, strings by
// canonical equivalence and URLs by their whole string.

impl PartialEq for WorkspaceHeading {
    fn eq(&self, other: &Self) -> bool {
        swift::str_eq(&self.title, &other.title) && self.range == other.range && self.level == other.level
    }
}

impl PartialEq for WorkspaceFrontMatterField {
    fn eq(&self, other: &Self) -> bool {
        swift::str_eq(&self.key, &other.key) && swift::str_eq(&self.value, &other.value) && self.range == other.range
    }
}

impl PartialEq for WorkspaceLink {
    fn eq(&self, other: &Self) -> bool {
        swift::str_eq(&self.destination, &other.destination) && self.range == other.range && self.kind == other.kind
    }
}

impl PartialEq for WorkspaceIndexEntry {
    fn eq(&self, other: &Self) -> bool {
        swift::str_eq(&self.id, &other.id)
            && self.url == other.url
            && swift::str_eq(&self.relative_path, &other.relative_path)
            && swift::str_eq(&self.text, &other.text)
            && self.headings == other.headings
            && self.front_matter == other.front_matter
            && self.links == other.links
            && self.byte_count == other.byte_count
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceIndexSnapshot {
    pub root_url: FileUrl,
    pub revision: isize,
    pub entries: Vec<WorkspaceIndexEntry>,
    pub skipped_files: isize,
}

impl WorkspaceIndexSnapshot {
    pub fn new(root_url: FileUrl, revision: isize, entries: Vec<WorkspaceIndexEntry>) -> WorkspaceIndexSnapshot {
        WorkspaceIndexSnapshot { root_url, revision, entries, skipped_files: 0 }
    }

    /// `WorkspaceIndexSnapshot.empty`.
    pub fn empty() -> WorkspaceIndexSnapshot {
        WorkspaceIndexSnapshot::new(FileUrl::from_path("/"), 0, Vec::new())
    }
}

// MARK: - Index

/// `Enumerator`: the whole list at once.
pub type Enumerator = Arc<dyn Fn(&FileUrl, &WorkspaceIndexPolicy) -> Vec<FileUrl> + Send + Sync>;
/// `StreamingEnumerator`: hands each file to `accept`, stopping when it
/// returns `false`.
pub type StreamingEnumerator =
    Arc<dyn Fn(&FileUrl, &WorkspaceIndexPolicy, &mut dyn FnMut(FileUrl) -> bool) + Send + Sync>;
/// `Reader`: the file's text and byte count, or `nil`.
pub type Reader = Arc<dyn Fn(&FileUrl) -> Option<(String, i64)> + Send + Sync>;

type UpdateHandler = Rc<dyn Fn(&WorkspaceIndexSnapshot)>;

struct IndexState {
    on_update: Option<UpdateHandler>,
    snapshot: WorkspaceIndexSnapshot,
    policy: WorkspaceIndexPolicy,
    enumerator: StreamingEnumerator,
    reader: Reader,
    /// The outer `Task`'s cancellation, checked on the main thread.
    task: Option<Arc<CancellationToken>>,
    cancellation_token: Option<Arc<CancellationToken>>,
    revision: isize,
}

/// Optional, local-only workspace index. New scans cancel the old scan, and
/// a stale revision cannot replace a newer snapshot. Main-thread only.
pub struct WorkspaceIndex {
    state: Rc<RefCell<IndexState>>,
}

impl WorkspaceIndex {
    /// `WorkspaceIndex(policy:)`, with Foundation's enumerator and a whole-file
    /// UTF-8 reader.
    pub fn new(policy: WorkspaceIndexPolicy) -> WorkspaceIndex {
        Self::with_streaming_enumerator(policy, Arc::new(default_enumerator), Arc::new(default_reader))
    }

    /// `WorkspaceIndex(policy:enumerator:reader:)`: injected file list and
    /// reader, so tests never touch the disk.
    pub fn with_enumerator(policy: WorkspaceIndexPolicy, enumerator: Enumerator, reader: Reader) -> WorkspaceIndex {
        let streaming: StreamingEnumerator = Arc::new(move |root, policy, accept| {
            for url in enumerator(root, policy) {
                if !accept(url) {
                    break;
                }
            }
        });
        Self::with_streaming_enumerator(policy, streaming, reader)
    }

    fn with_streaming_enumerator(
        policy: WorkspaceIndexPolicy,
        enumerator: StreamingEnumerator,
        reader: Reader,
    ) -> WorkspaceIndex {
        WorkspaceIndex {
            state: Rc::new(RefCell::new(IndexState {
                on_update: None,
                snapshot: WorkspaceIndexSnapshot::empty(),
                policy,
                enumerator,
                reader,
                task: None,
                cancellation_token: None,
                revision: 0,
            })),
        }
    }

    fn main_thread() -> MainThreadMarker {
        MainThreadMarker::new().expect("WorkspaceIndex is main-thread state (@MainActor)")
    }

    /// `onUpdate`.
    pub fn set_on_update(&self, handler: Option<Box<dyn Fn(&WorkspaceIndexSnapshot)>>) {
        self.state.borrow_mut().on_update = handler.map(Rc::from);
    }

    pub fn snapshot(&self) -> WorkspaceIndexSnapshot {
        self.state.borrow().snapshot.clone()
    }

    pub fn start(&self, root_url: &FileUrl) {
        let mtm = Self::main_thread();
        let mut state = self.state.borrow_mut();
        state.revision += 1;
        let current_revision = state.revision;
        if let Some(token) = &state.cancellation_token {
            token.cancel();
        }
        if let Some(task) = &state.task {
            task.cancel();
        }
        let cancellation_token = Arc::new(CancellationToken::default());
        state.cancellation_token = Some(cancellation_token.clone());
        let task = Arc::new(CancellationToken::default());
        state.task = Some(task.clone());
        let policy = state.policy.clone();
        let enumerator = state.enumerator.clone();
        let reader = state.reader.clone();
        drop(state);

        let weak = MainThreadBound::new(Rc::downgrade(&self.state), mtm);
        let root_url = root_url.clone();
        DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(DispatchQoS::UserInitiated)).exec_async(
            move || {
                let snapshot = autoreleasepool(|_| {
                    Self::build_snapshot(
                        &root_url.standardized_file_url(),
                        current_revision,
                        &policy,
                        &enumerator,
                        &reader,
                        &cancellation_token,
                    )
                });
                DispatchQueue::main().exec_async(move || {
                    let mtm = Self::main_thread();
                    let weak: &Weak<RefCell<IndexState>> = weak.get(mtm);
                    if task.is_cancelled() {
                        return;
                    }
                    let Some(state) = weak.upgrade() else { return };
                    if state.borrow().revision != current_revision {
                        return;
                    }
                    Self::publish(&state, snapshot);
                });
            },
        );
    }

    /// Stores the snapshot and calls `onUpdate` outside the borrow, so the
    /// handler may read the index.
    fn publish(state: &Rc<RefCell<IndexState>>, snapshot: WorkspaceIndexSnapshot) {
        let handler = {
            let mut state = state.borrow_mut();
            state.snapshot = snapshot.clone();
            state.on_update.clone()
        };
        if let Some(handler) = handler {
            handler(&snapshot);
        }
    }

    pub fn cancel(&self) {
        let mut state = self.state.borrow_mut();
        state.revision += 1;
        if let Some(token) = state.cancellation_token.take() {
            token.cancel();
        }
        if let Some(task) = state.task.take() {
            task.cancel();
        }
    }

    pub fn reroot(&self, root_url: &FileUrl) {
        let root_url = root_url.standardized_file_url();
        self.cancel();
        let revision = self.state.borrow().revision;
        Self::publish(&self.state, WorkspaceIndexSnapshot::new(root_url.clone(), revision, Vec::new()));
        self.start(&root_url);
    }

    /// The scan itself (`buildSnapshot`): enumerate, read on a worker pool,
    /// parse, and sort by `localizedStandardCompare`.
    pub fn build_snapshot(
        root_url: &FileUrl,
        revision: isize,
        policy: &WorkspaceIndexPolicy,
        enumerator: &StreamingEnumerator,
        reader: &Reader,
        cancellation_token: &CancellationToken,
    ) -> WorkspaceIndexSnapshot {
        struct ScanState {
            cursor: usize,
            skipped: isize,
            total_bytes: i64,
            entries: Vec<WorkspaceIndexEntry>,
        }
        let state = Mutex::new(ScanState { cursor: 0, skipped: 0, total_bytes: 0, entries: Vec::new() });
        let lock = || state.lock().unwrap_or_else(|poison| poison.into_inner());

        let mut urls: Vec<FileUrl> = Vec::new();
        enumerator(root_url, policy, &mut |url| {
            if cancellation_token.is_cancelled() {
                return false;
            }
            if !policy.accepts(&url, false) {
                return true;
            }
            if let Some(size) = file_size(Path::new(&url.path()))
                && size > policy.maximum_bytes_per_file
            {
                lock().skipped += 1;
                return true;
            }
            urls.push(url);
            (urls.len() as isize) < policy.maximum_files
        });

        // A small worker pool gives predictable memory use without one task
        // per file.
        let workers = (policy.read_concurrency as usize).min(urls.len().max(1));
        let work = || {
            loop {
                if cancellation_token.is_cancelled() {
                    return;
                }
                let url = {
                    let mut state = lock();
                    if state.cursor >= urls.len() {
                        return;
                    }
                    let url = &urls[state.cursor];
                    state.cursor += 1;
                    url
                };

                if let Some(size) = file_size(Path::new(&url.path()))
                    && size > policy.maximum_bytes_per_file
                {
                    lock().skipped += 1;
                    continue;
                }

                let Some((text, byte_count)) = reader(url).filter(|(_, count)| {
                    *count >= 0 && *count <= policy.maximum_bytes_per_file
                }) else {
                    lock().skipped += 1;
                    continue;
                };
                let fits_budget = {
                    let mut state = lock();
                    let fits = byte_count <= policy.maximum_total_bytes - state.total_bytes;
                    if fits {
                        state.total_bytes += byte_count;
                    } else {
                        state.skipped += 1;
                    }
                    fits
                };
                if !fits_budget {
                    continue;
                }

                let document = MarkdownParser::parse_with(&text, ParseOptions::WORKSPACE_INDEX);
                // Search loads text on demand; keep the index lean.
                let entry = Self::entry(url, root_url, "", &document, byte_count);
                if cancellation_token.is_cancelled() {
                    return;
                }
                lock().entries.push(entry);
            }
        };
        std::thread::scope(|scope| {
            for _ in 1..workers {
                scope.spawn(|| autoreleasepool(|_| work()));
            }
            work();
        });

        let state = state.into_inner().unwrap_or_else(|poison| poison.into_inner());
        let mut entries = state.entries;
        sort_localized_standard(&mut entries);
        WorkspaceIndexSnapshot { root_url: root_url.clone(), revision, entries, skipped_files: state.skipped }
    }

    fn entry(
        url: &FileUrl,
        root_url: &FileUrl,
        text: &str,
        document: &ParsedDocument,
        byte_count: i64,
    ) -> WorkspaceIndexEntry {
        let headings = document
            .headings
            .iter()
            .map(|heading| WorkspaceHeading { title: heading.title.clone(), range: heading.range, level: heading.level })
            .collect();
        let fields = document.front_matter.as_ref().map_or_else(Vec::new, |matter| {
            matter
                .fields
                .iter()
                .map(|field| WorkspaceFrontMatterField {
                    key: field.key.clone(),
                    value: field.value.clone(),
                    range: field.key_range.union(field.value_range),
                })
                .collect()
        });
        let mut links = Vec::new();
        document.root.walk(&mut |block| {
            for inline in &block.inlines {
                inline.walk(&mut |span| match &span.kind {
                    InlineKind::Link { destination, .. } | InlineKind::Autolink { destination } => {
                        links.push(WorkspaceLink {
                            destination: destination.clone(),
                            range: span.range,
                            kind: WorkspaceLinkKind::Markdown,
                        });
                    }
                    InlineKind::Wikilink { target, .. } => links.push(WorkspaceLink {
                        destination: target.clone(),
                        range: span.range,
                        kind: WorkspaceLinkKind::Wikilink,
                    }),
                    _ => {}
                });
            }
        });
        let root_path = root_url.standardized_file_url().path();
        let prefix = if swift::has_suffix(&root_path, "/") { root_path } else { root_path + "/" };
        let relative = swift::replacing_occurrences(&url.standardized_file_url().path(), &prefix, "");
        WorkspaceIndexEntry::new(url.clone(), relative, text, headings, fields, links, byte_count)
    }
}

impl Drop for WorkspaceIndex {
    /// `deinit`: cancels the scan in flight.
    fn drop(&mut self) {
        let state = self.state.borrow();
        if let Some(token) = &state.cancellation_token {
            token.cancel();
        }
        if let Some(task) = &state.task {
            task.cancel();
        }
    }
}

/// `entries.sort { $0.relativePath.localizedStandardCompare($1.relativePath)
/// == .orderedAscending }`.
fn sort_localized_standard(entries: &mut Vec<WorkspaceIndexEntry>) {
    autoreleasepool(|_| {
        let keys: Vec<Retained<NSString>> =
            entries.iter().map(|entry| crate::support::find_engine::ns_string(&entry.relative_path)).collect();
        let mut order: Vec<usize> = (0..entries.len()).collect();
        order.sort_by(|&a, &b| match keys[a].localizedStandardCompare(&keys[b]) {
            NSComparisonResult::Ascending => std::cmp::Ordering::Less,
            NSComparisonResult::Descending => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        });
        let mut slots: Vec<Option<WorkspaceIndexEntry>> = std::mem::take(entries).into_iter().map(Some).collect();
        *entries = order.into_iter().map(|index| slots[index].take().expect("each index once")).collect();
    });
}

/// A boolean resource value (`resourceValues(forKeys:)` then `.isDirectory`
/// / `.isRegularFile`); `nil` counts as `false`.
fn bool_resource(url: &NSURL, key: &NSURLResourceKey) -> bool {
    let mut value: Option<Retained<AnyObject>> = None;
    // SAFETY: `value` is a valid out-pointer for the call's duration.
    if unsafe { url.getResourceValue_forKey_error(&mut value, key) }.is_err() {
        return false;
    }
    value.and_then(|value| value.downcast::<NSNumber>().ok()).is_some_and(|number| number.boolValue())
}

/// `defaultEnumerator`: `FileManager.default.enumerator(at:
/// includingPropertiesForKeys: [.isDirectoryKey, .isRegularFileKey,
/// .fileSizeKey], options: [])`, skipping the descendants of directories the
/// policy refuses.
pub fn default_enumerator(root: &FileUrl, policy: &WorkspaceIndexPolicy, accept: &mut dyn FnMut(FileUrl) -> bool) {
    autoreleasepool(|_| {
        // SAFETY: the resource keys are immutable Foundation globals.
        let keys = unsafe { NSArray::from_slice(&[NSURLIsDirectoryKey, NSURLIsRegularFileKey, NSURLFileSizeKey]) };
        let Some(iterator) = NSFileManager::defaultManager().enumeratorAtURL_includingPropertiesForKeys_options_errorHandler(
            &root.to_nsurl(),
            Some(&keys),
            NSDirectoryEnumerationOptions::empty(),
            None,
        ) else {
            return;
        };
        loop {
            let keep_going = autoreleasepool(|_| {
                let Some(url) = iterator.nextObject() else { return false };
                let Some(file_url) = FileUrl::from_nsurl(&url) else { return true };
                // SAFETY: the resource keys are immutable Foundation globals.
                let (is_directory, is_regular_file) =
                    unsafe { (bool_resource(&url, NSURLIsDirectoryKey), bool_resource(&url, NSURLIsRegularFileKey)) };
                if is_directory {
                    if !policy.accepts(&file_url, true) {
                        iterator.skipDescendants();
                    }
                    return true;
                }
                !(is_regular_file && policy.accepts(&file_url, false) && !accept(file_url))
            });
            if !keep_going {
                return;
            }
        }
    });
}

/// `String(data:encoding: .utf8)`: strict UTF-8, one leading byte-order mark
/// dropped.
pub fn string_from_utf8_data(data: &[u8]) -> Option<String> {
    let body = data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(data);
    std::str::from_utf8(body).ok().map(str::to_owned)
}

/// `defaultReader`: the whole file, if it is UTF-8.
pub fn default_reader(url: &FileUrl) -> Option<(String, i64)> {
    let data = std::fs::read(url.path()).ok()?;
    let text = string_from_utf8_data(&data)?;
    Some((text, data.len() as i64))
}

/// `FileHandle(forReadingFrom:)` then `read(upToCount:)`: `nil` for an
/// unreadable file or one with no bytes at all.
pub(crate) fn read_up_to_count(url: &FileUrl, limit: usize) -> Option<Vec<u8>> {
    let file = std::fs::File::open(url.path()).ok()?;
    let mut data = Vec::new();
    file.take(limit as u64).read_to_end(&mut data).ok()?;
    if data.is_empty() { None } else { Some(data) }
}
