//! Port of `Sources/DownrightApp/AI/SiblingScanner.swift`.
//!
//! Sibling files (§8.7).
//!
//! Agents don't write one file, they write six into the same folder.  So on
//! open we scan the containing directory — plus one level into `docs/`,
//! `plans/`, `.claude/` and friends — and keep the result to hand.
//!
//! Explicitly **not** an index and **not** a vault (§2).  No database, no
//! crawl, no "open folder" ceremony: one shallow directory listing, sorted by
//! modification time, recomputed when the directory changes.
//!
//! Main-thread type, like the Swift class. Background rescans run on a serial
//! utility queue (`com.ezzy.downright.sibling-scan`) and land on the main
//! queue; the scanner is found again there by id, the way Swift's
//! `[weak self]` capture finds it. As in Swift, the initial scan lists the
//! directory synchronously on the caller's thread (no hashing).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};

use dispatch2::{DispatchQoS, DispatchQueue, DispatchQueueAttr, DispatchRetained};
use upleft_core::document_io::DocumentIO;
use upleft_foundation::date::Date;
use upleft_foundation::file_manager;
use upleft_foundation::url::FileUrl;

use super::document_state_store::DocumentStateStore;
use super::file_watcher::FileWatcher;

/// `Date.distantPast`.
const DISTANT_PAST: Date = Date { time_interval_since_reference_date: -63_114_076_800.0 };

const MARKDOWN_EXTENSIONS: [&str; 8] = ["md", "markdown", "mdown", "mkd", "mdx", "mdc", "qmd", "rmd"];
/// A directory full of agent output can be large; a hard cap keeps the
/// sidebar a glance rather than a file browser.
const LIMIT: usize = 200;
const CONTENT_HASH_CACHE_LIMIT: usize = 256;
/// Files above this size are not content-hashed for the "changed since you
/// last looked" dot: reading and SHA-256-ing them on open would stall the
/// first frame for a dot that carries no real signal.
const CHANGE_HASH_MAX_BYTES: isize = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Sibling {
    pub url: FileUrl,
    pub display_name: String,
    pub modified: Date,
    pub byte_count: isize,
    /// Set when the file changed since the user last looked at it — the dot
    /// in the sidebar.
    pub has_unseen_changes: bool,
    /// Relative label for files found one level down, e.g. "docs".
    pub group: Option<String>,
    pub is_current: bool,
}

impl Sibling {
    /// `Identifiable.id`.
    pub fn id(&self) -> String {
        self.url.path()
    }
}

/// `ContentFingerprint`: path, modification date (by its bits, as `Date`
/// hashes its value) and byte count.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ContentFingerprint {
    path: String,
    modified: u64,
    byte_count: isize,
}

#[derive(Clone, Default)]
struct HashCache {
    hashes: HashMap<ContentFingerprint, String>,
    order: Vec<ContentFingerprint>,
}

/// `SiblingScanner`. Owned by one main-thread holder; dropping it is Swift's
/// `deinit`.
pub struct SiblingScanner {
    inner: Rc<Inner>,
}

struct Inner {
    id: u64,
    siblings: RefCell<Vec<Sibling>>,
    on_change: RefCell<Option<Rc<dyn Fn()>>>,
    document_url: FileUrl,
    /// Readable so the window can tell whether a settings change actually
    /// invalidated this scanner before rebuilding it.
    extra_directories: Vec<String>,
    watcher: RefCell<Option<FileWatcher>>,
    cache: RefCell<HashCache>,
    scan_queue: DispatchRetained<DispatchQueue>,
    scan_generation: Cell<u64>,
    /// `DocumentStateStore.shared`, or an isolated store for tests.
    document_state_store: &'static DocumentStateStore,
}

impl Drop for Inner {
    /// `deinit { watcher?.stop() }`.
    fn drop(&mut self) {
        if let Some(watcher) = self.watcher.borrow_mut().take() {
            watcher.stop();
        }
        let id = self.id;
        let _ = SCANNERS.try_with(|scanners| {
            if let Ok(mut scanners) = scanners.try_borrow_mut() {
                scanners.remove(&id);
            }
        });
    }
}

thread_local! {
    /// Live scanners by id, main thread only (`[weak self]`).
    static SCANNERS: RefCell<HashMap<u64, Weak<Inner>>> = RefCell::new(HashMap::new());
}

static NEXT_SCANNER_ID: AtomicU64 = AtomicU64::new(1);

fn scanner_for(id: u64) -> Option<Rc<Inner>> {
    SCANNERS.with(|scanners| scanners.borrow().get(&id).and_then(Weak::upgrade))
}

impl SiblingScanner {
    /// `init(documentURL:extraDirectories:)`.
    pub fn new(document_url: &FileUrl, extra_directories: Vec<String>) -> SiblingScanner {
        Self::with_document_state_store(document_url, extra_directories, DocumentStateStore::shared())
    }

    /// The same, reading "last seen" hashes from `document_state_store`
    /// instead of `DocumentStateStore.shared`.
    pub fn with_document_state_store(
        document_url: &FileUrl,
        extra_directories: Vec<String>,
        document_state_store: &'static DocumentStateStore,
    ) -> SiblingScanner {
        let attribute = DispatchQueueAttr::with_qos_class(DispatchQueueAttr::SERIAL, DispatchQoS::Utility, 0);
        let inner = Rc::new(Inner {
            id: NEXT_SCANNER_ID.fetch_add(1, Ordering::Relaxed),
            siblings: RefCell::new(Vec::new()),
            on_change: RefCell::new(None),
            document_url: document_url.resolving_symlinks_in_path(),
            extra_directories,
            watcher: RefCell::new(None),
            cache: RefCell::new(HashCache::default()),
            scan_queue: DispatchQueue::new("com.bitemyapp.upleft.sibling-scan", Some(&attribute)),
            scan_generation: Cell::new(0),
            document_state_store,
        });
        SCANNERS.with(|scanners| scanners.borrow_mut().insert(inner.id, Rc::downgrade(&inner)));
        let scanner = SiblingScanner { inner };
        // First pass: pure directory listing, no content hashing — instant for
        // the first paint.  The second pass computes unseen-change dots off
        // the scan queue so a folder of agent output never blocks opening the
        // file.
        scanner.scan(true, false);
        scanner.scan(false, true);
        scanner.start_watching();
        scanner
    }

    pub fn siblings(&self) -> Vec<Sibling> {
        self.inner.siblings.borrow().clone()
    }

    pub fn extra_directories(&self) -> &[String] {
        &self.inner.extra_directories
    }

    pub fn set_on_change(&self, callback: Option<impl Fn() + 'static>) {
        *self.inner.on_change.borrow_mut() = callback.map(|callback| Rc::new(callback) as Rc<dyn Fn()>);
    }

    // MARK: - Scanning

    /// Directory listing; `compute_changes: true` additionally reads and
    /// hashes each sibling for the "changed since you last looked" dot.
    /// Watcher-driven rescans run off the main thread; the initial open path
    /// stays synchronous so the sidebar has rows before the first paint, but
    /// *listing only*. Swift's defaults are `synchronously: false,
    /// computeChanges: true`.
    pub fn scan(&self, synchronously: bool, compute_changes: bool) {
        self.inner.scan(synchronously, compute_changes);
    }

    // MARK: - Watching

    /// Watching the *directory* rather than each file means a newly written
    /// sibling appears without a rescan timer — which is the case that
    /// matters, since the sixth file usually arrives after you have opened
    /// the first.
    fn start_watching(&self) {
        let id = self.inner.id;
        let watcher = FileWatcher::new(&self.inner.document_url.deleting_last_path_component(), true, None, move |_| {
            // Delivered on the main queue.
            if let Some(inner) = scanner_for(id) {
                inner.scan(false, true);
            }
        });
        *self.inner.watcher.borrow_mut() = Some(watcher);
    }

    // MARK: - Cycling (§8.7, ⌥⌘← / ⌥⌘→)

    pub fn neighbour(&self, url: &FileUrl, forward: bool) -> Option<FileUrl> {
        let siblings = self.inner.siblings.borrow();
        if siblings.len() <= 1 {
            return None;
        }
        let wanted = url.resolving_symlinks_in_path();
        let Some(index) = siblings.iter().position(|sibling| sibling.url.resolving_symlinks_in_path() == wanted) else {
            return siblings.first().map(|sibling| sibling.url.clone());
        };
        let count = siblings.len();
        let next = if forward { (index + 1) % count } else { (index + count - 1) % count };
        Some(siblings[next].url.clone())
    }

    /// Groups for the sidebar's section headers, preserving scan order.
    pub fn grouped(&self) -> Vec<(Option<String>, Vec<Sibling>)> {
        let mut groups: Vec<(Option<String>, Vec<Sibling>)> = Vec::new();
        for sibling in self.inner.siblings.borrow().iter() {
            let existing = groups.iter_mut().find(|(group, _)| match (group, &sibling.group) {
                (None, None) => true,
                (Some(a), Some(b)) => upleft_swift_text::str_eq(a, b),
                _ => false,
            });
            match existing {
                Some((_, items)) => items.push(sibling.clone()),
                None => groups.push((sibling.group.clone(), vec![sibling.clone()])),
            }
        }
        groups
    }
}

impl Inner {
    fn scan(&self, synchronously: bool, compute_changes: bool) {
        if synchronously {
            self.scan_generation.set(self.scan_generation.get().wrapping_add(1));
            let mut cache = self.cache.borrow().clone();
            let found = build_siblings(
                &self.document_url,
                &self.extra_directories,
                compute_changes,
                &mut cache,
                self.document_state_store,
            );
            *self.cache.borrow_mut() = cache;
            *self.siblings.borrow_mut() = found;
            self.notify();
            return;
        }

        self.scan_generation.set(self.scan_generation.get().wrapping_add(1));
        let generation = self.scan_generation.get();
        let document_url = self.document_url.clone();
        let extra_directories = self.extra_directories.clone();
        let mut cache = self.cache.borrow().clone();
        let store = self.document_state_store;
        let id = self.id;
        self.scan_queue.exec_async(move || {
            let found = build_siblings(&document_url, &extra_directories, compute_changes, &mut cache, store);
            DispatchQueue::main().exec_async(move || {
                let Some(inner) = scanner_for(id) else { return };
                if inner.scan_generation.get() != generation {
                    return;
                }
                *inner.cache.borrow_mut() = cache;
                *inner.siblings.borrow_mut() = found;
                inner.notify();
            });
        });
    }

    fn notify(&self) {
        let callback = self.on_change.borrow().clone();
        if let Some(callback) = callback {
            callback();
        }
    }
}

fn build_siblings(
    document_url: &FileUrl,
    extra_directories: &[String],
    compute_changes: bool,
    cache: &mut HashCache,
    store: &DocumentStateStore,
) -> Vec<Sibling> {
    let directory = document_url.deleting_last_path_component();
    let mut found: Vec<Sibling> = Vec::new();
    found.extend(markdown_files(&directory, None, document_url, compute_changes, cache, store));

    for name in extra_directories {
        let sub = directory.appending_path_component_is_directory(name, true);
        if file_manager::file_exists_is_directory(&sub.path()) != Some(true) {
            continue;
        }
        found.extend(markdown_files(&sub, Some(name.clone()), document_url, compute_changes, cache, store));
    }

    found.sort_by(|a, b| {
        if a.is_current != b.is_current {
            return if a.is_current { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
        }
        if a.modified > b.modified {
            std::cmp::Ordering::Less
        } else if b.modified > a.modified {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    found.truncate(LIMIT);
    found
}

fn markdown_files(
    directory: &FileUrl,
    group: Option<String>,
    document_url: &FileUrl,
    compute_changes: bool,
    cache: &mut HashCache,
    store: &DocumentStateStore,
) -> Vec<Sibling> {
    let Ok(entries) = file_manager::contents_of_directory(directory) else { return Vec::new() };

    entries
        .into_iter()
        .filter_map(|url| {
            let extension = upleft_swift_text::lowercased(&url.path_extension());
            if !MARKDOWN_EXTENSIONS.contains(&extension.as_str()) {
                return None;
            }
            let values = file_manager::resource_values(&url)?;
            if !values.is_regular_file {
                return None;
            }

            let modified = values.content_modification_date.unwrap_or(DISTANT_PAST);
            let state = store.state(&url);
            let size = values.file_size.unwrap_or(0) as isize;
            let unseen = if state.last_seen_hash.is_empty() || !compute_changes || size > CHANGE_HASH_MAX_BYTES {
                false
            } else if let Some(hash) = content_hash(&url, modified, size, cache) {
                !upleft_swift_text::str_eq(&hash, &state.last_seen_hash)
            } else {
                false
            };

            let resolved = url.resolving_symlinks_in_path();
            Some(Sibling {
                display_name: url.deleting_path_extension().last_path_component(),
                modified,
                byte_count: size,
                has_unseen_changes: unseen && resolved != *document_url,
                group: group.clone(),
                is_current: resolved == *document_url,
                url,
            })
        })
        .collect()
}

fn content_hash(url: &FileUrl, modified: Date, byte_count: isize, cache: &mut HashCache) -> Option<String> {
    let key = ContentFingerprint {
        path: url.standardized_file_url().path(),
        modified: modified.time_interval_since_reference_date.to_bits(),
        byte_count,
    };
    if let Some(cached) = cache.hashes.get(&key).cloned() {
        cache.order.retain(|entry| *entry != key);
        cache.order.push(key);
        return Some(cached);
    }

    let data = std::fs::read(url.path()).ok()?;
    file_manager::string_from_utf8_data(&data)?;
    let hash = DocumentIO::content_hash_data(&data);
    cache.hashes.insert(key.clone(), hash.clone());
    cache.order.retain(|entry| *entry != key);
    cache.order.push(key);
    while cache.order.len() > CONTENT_HASH_CACHE_LIMIT {
        let oldest = cache.order.remove(0);
        cache.hashes.remove(&oldest);
    }
    Some(hash)
}
