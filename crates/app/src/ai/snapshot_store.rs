//! Port of `Sources/DownrightApp/AI/SnapshotStore.swift`.
//!
//! Local time-travel (§8.3).
//!
//! Agents don't commit, and git doesn't help you here: by the time you notice
//! the agent replaced a section you wanted, the previous text exists nowhere.
//! So every external write is snapshotted into a content-addressed store,
//! deduplicated by hash — an agent that rewrites a file four times with the
//! same content costs one object.
//!
//! Store layout:
//! ```text
//! history/objects/<ab>/<hash>       zlib-compressed UTF-8 content
//! history/index/<docKey>.json       ordered version list for one document
//! ```
//!
//! Threading follows Swift: object and index writes and pruning run on one
//! serial dispatch queue (`com.ezzy.downright.history`, utility QoS); the
//! reservation cache and the limits live behind one lock. Swift's
//! `waitForPendingWrites()` and `pruneOneGenerationForTesting()` are `async`;
//! here they block the caller until the queue drains, so they must never be
//! called on the main thread ([`SnapshotStore::after_pending_writes`] is the
//! non-blocking form).
//!
//! The index file is written by a `JSONEncoder` without `.sortedKeys`, so
//! Swift's key order changes from run to run; the port writes `CodingKeys`
//! order.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use dispatch2::{DispatchQoS, DispatchQueue, DispatchQueueAttr, DispatchRetained, GlobalQueueIdentifier};
use upleft_core::document_io::DocumentIO;
use upleft_foundation::date::Date;
use upleft_foundation::decodable::{self, DecodableValue, DecodingError, Value};
use upleft_foundation::file_manager;
use upleft_foundation::json_encoder::{self, JsonValue, OutputFormatting};
use upleft_foundation::url::FileUrl;

use crate::support::app_paths;

/// `Date.distantPast`.
pub const DISTANT_PAST: Date = Date { time_interval_since_reference_date: -63_114_076_800.0 };

/// `SnapshotStore.SnapshotKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SnapshotKind {
    /// Written by something outside the app — the interesting case.
    External,
    /// Written by us, kept so the timeline is continuous.
    Local,
    /// The state at the moment the document was first opened.
    Baseline,
}

impl SnapshotKind {
    pub fn raw_value(self) -> &'static str {
        match self {
            SnapshotKind::External => "external",
            SnapshotKind::Local => "local",
            SnapshotKind::Baseline => "baseline",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<SnapshotKind> {
        match raw {
            "external" => Some(SnapshotKind::External),
            "local" => Some(SnapshotKind::Local),
            "baseline" => Some(SnapshotKind::Baseline),
            _ => None,
        }
    }
}

/// `SnapshotStore.VersionRecord`. Equality is Swift's: hash and date only.
#[derive(Clone, Debug)]
pub struct VersionRecord {
    pub hash: String,
    pub date: Date,
    pub byte_count: isize,
    pub kind: SnapshotKind,
}

impl PartialEq for VersionRecord {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash && self.date == other.date
    }
}

impl VersionRecord {
    /// `Identifiable.id`.
    pub fn id(&self) -> &str {
        &self.hash
    }

    /// Synthesized `encode(to:)` under `.iso8601`.
    pub fn encode(&self) -> JsonValue {
        JsonValue::object([
            ("hash", JsonValue::from(self.hash.as_str())),
            ("date", JsonValue::from(self.date.iso8601())),
            ("byteCount", JsonValue::Int(self.byte_count as i64)),
            ("kind", JsonValue::from(self.kind.raw_value())),
        ])
    }

    /// Synthesized `init(from:)` under `.iso8601`.
    pub fn decode(value: &Value) -> Result<VersionRecord, DecodingError> {
        let keyed = value.keyed_container()?;
        Ok(VersionRecord {
            hash: keyed.decode("hash", Value::string_value)?,
            date: keyed.decode("date", Value::date_iso8601)?,
            byte_count: keyed.decode("byteCount", Value::int_value)? as isize,
            kind: keyed.decode("kind", |value| value.raw_string_enum(SnapshotKind::from_raw_value))?,
        })
    }
}

/// `SnapshotStore.Index` (private in Swift).
#[derive(Clone, Debug)]
struct Index {
    path: String,
    versions: Vec<VersionRecord>,
}

impl Index {
    fn encode(&self) -> JsonValue {
        JsonValue::object([
            ("path", JsonValue::from(self.path.as_str())),
            ("versions", JsonValue::Array(self.versions.iter().map(VersionRecord::encode).collect())),
        ])
    }

    fn decode(data: &[u8]) -> Result<Index, DecodingError> {
        let value = decodable::parse(data)?;
        let keyed = value.keyed_container()?;
        Ok(Index {
            path: keyed.decode("path", Value::string_value)?,
            versions: keyed.decode("versions", |value| value.array_of(VersionRecord::decode))?,
        })
    }

    /// `JSONEncoder.snapshotEncoder.encode(index)`.
    fn encoded(&self) -> Vec<u8> {
        json_encoder::encode(&self.encode(), OutputFormatting::DEFAULT)
    }
}

/// `SnapshotStore.Content`: the result of asking for a historical version.
///
/// "Not there" and "there but unreadable" are different answers and the
/// timeline has to say which: silently handing back mojibake presents a
/// truncated object as a real version of the user's document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Content {
    Text(String),
    /// No object with that hash — pruned by age or size, or never written.
    Missing,
    /// The object exists but does not decompress, does not decode as UTF-8,
    /// or does not hash back to the name it is filed under.
    Corrupt,
}

/// `SnapshotStore.PruneReport`: what a prune removed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PruneReport {
    /// Versions dropped, keyed by document key.
    pub dropped_versions: HashMap<String, isize>,
    /// Bytes reclaimed from the object store.
    pub freed_bytes: isize,
}

impl PruneReport {
    pub fn is_empty(&self) -> bool {
        self.dropped_versions.is_empty()
    }
}

/// One object file, as `objectInventory()` reports it.
#[derive(Clone, Debug)]
pub struct ObjectEntry {
    pub url: FileUrl,
    pub size: isize,
    pub date: Date,
    pub hash: String,
}

#[derive(Clone, Debug, Default)]
struct DocumentState {
    newest_hash: Option<String>,
    is_loaded: bool,
}

/// Everything `pendingLock` owns.
struct Pending {
    stored_maximum_age: f64,
    stored_maximum_bytes: isize,
    stored_maximum_bytes_per_document: isize,
    /// A hash is reserved before the disk write is queued, so a second
    /// record call cannot race the first index update.
    state_by_document: HashMap<String, DocumentState>,
    state_order: Vec<String>,
    /// Accumulates across prunes until a document reads and acknowledges
    /// its own count.
    evicted_versions_by_document: HashMap<String, isize>,
    last_report: PruneReport,
}

const MAXIMUM_CACHED_DOCUMENTS: usize = 512;

/// Pruning is primarily a launch job. This interval is only a backstop for
/// unusually long sessions that can cross the configured size caps.
const PRUNE_INTERVAL: f64 = 30.0 * 60.0;

struct Inner {
    queue: DispatchRetained<DispatchQueue>,
    pending: Mutex<Pending>,
    /// Only touched on `queue`.
    last_prune: Mutex<Date>,
    history_directory: FileUrl,
}

/// `SnapshotStore`. Cloning shares the store, like a Swift class reference.
#[derive(Clone)]
pub struct SnapshotStore {
    inner: Arc<Inner>,
}

/// The three answers an index file can give. "Absent" and "present but
/// undecodable" must stay distinct: overwriting the latter replaces every
/// version it references with a one-entry index, and the following prune
/// garbage-collects those now-unreferenced objects.
enum IndexLoad {
    Loaded(Index),
    Missing,
    Unreadable,
}

static SHARED: OnceLock<SnapshotStore> = OnceLock::new();

impl SnapshotStore {
    /// `SnapshotStore.shared`.
    pub fn shared() -> &'static SnapshotStore {
        SHARED.get_or_init(|| SnapshotStore::new(app_paths::history_directory()))
    }

    /// `init(historyDirectory:)`: injectable so tests never traverse the
    /// user's production store.
    pub fn new(history_directory: FileUrl) -> SnapshotStore {
        let history_directory = history_directory.standardized_file_url();
        let utility = DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(DispatchQoS::Utility));
        let queue = DispatchQueue::new_with_target("com.ezzy.downright.history", DispatchQueueAttr::SERIAL, Some(&utility));
        let inner = Inner {
            queue,
            pending: Mutex::new(Pending {
                stored_maximum_age: 30.0 * 24.0 * 60.0 * 60.0,
                stored_maximum_bytes: 500 * 1024 * 1024,
                stored_maximum_bytes_per_document: 32 * 1024 * 1024,
                state_by_document: HashMap::new(),
                state_order: Vec::new(),
                evicted_versions_by_document: HashMap::new(),
                last_report: PruneReport::default(),
            }),
            last_prune: Mutex::new(DISTANT_PAST),
            history_directory,
        };
        app_paths::ensure(inner.history_directory.clone());
        app_paths::ensure(inner.objects_directory());
        app_paths::ensure(inner.index_directory());
        SnapshotStore { inner: Arc::new(inner) }
    }

    // MARK: Limits

    pub fn maximum_age(&self) -> f64 {
        self.inner.pending.lock().unwrap().stored_maximum_age
    }

    pub fn set_maximum_age(&self, value: f64) {
        self.inner.pending.lock().unwrap().stored_maximum_age = value;
    }

    pub fn maximum_bytes(&self) -> isize {
        self.inner.pending.lock().unwrap().stored_maximum_bytes
    }

    pub fn set_maximum_bytes(&self, value: isize) {
        self.inner.pending.lock().unwrap().stored_maximum_bytes = value;
    }

    /// Per-document size cap.
    ///
    /// A single global cap lets one 400 MB document's history evict every
    /// other document's. Each document is trimmed against its own budget
    /// first, and the global cap is only a backstop.
    pub fn maximum_bytes_per_document(&self) -> isize {
        self.inner.pending.lock().unwrap().stored_maximum_bytes_per_document
    }

    pub fn set_maximum_bytes_per_document(&self, value: isize) {
        self.inner.pending.lock().unwrap().stored_maximum_bytes_per_document = value;
    }

    // MARK: Recording

    /// Records `text` as a version of `url`. Returns `None` when the content
    /// is identical to the newest version already recorded, which is the
    /// common case for a save that changed nothing.
    pub fn record(&self, text: &str, url: &FileUrl, kind: SnapshotKind) -> Option<VersionRecord> {
        let hash = Self::hash(text);
        let document_key = Self::document_key(url);
        let mut pending = self.inner.pending.lock().unwrap();

        let mut state = pending.state_by_document.get(&document_key).cloned().unwrap_or_default();
        if !state.is_loaded {
            if let IndexLoad::Loaded(existing) = self.inner.load_index_state(url) {
                state.newest_hash = existing.versions.last().map(|version| version.hash.clone());
            }
            state.is_loaded = true;
        }
        if state.newest_hash.as_deref() == Some(hash.as_str()) {
            store_state(&mut pending, state, &document_key);
            return None;
        }
        state.newest_hash = Some(hash.clone());
        store_state(&mut pending, state, &document_key);

        let data = text.as_bytes().to_vec();
        let record = VersionRecord { hash: hash.clone(), date: Date::now(), byte_count: data.len() as isize, kind };

        let inner = Arc::clone(&self.inner);
        let url = url.clone();
        let queued = record.clone();
        self.inner.queue.exec_async(move || {
            inner.write_object_if_needed(&hash, &data);
            // The object above is safe regardless of index health: while any
            // present-but-undecodable index exists, pruning fails closed and
            // will not treat it as unreferenced. The index itself is only
            // rewritten when it is readable or genuinely absent.
            match inner.load_index_state(&url) {
                IndexLoad::Loaded(mut index) => {
                    if index.versions.last().map(|version| version.hash.as_str()) == Some(hash.as_str()) {
                        return;
                    }
                    index.versions.push(queued);
                    inner.save_index(&index, &url);
                }
                IndexLoad::Missing => {
                    inner.save_index(&Index { path: url.path(), versions: vec![queued] }, &url);
                }
                IndexLoad::Unreadable => {}
            }
            inner.prune_if_due();
        });
        drop(pending);
        Some(record)
    }

    // MARK: Reading

    pub fn versions(&self, url: &FileUrl) -> Vec<VersionRecord> {
        match self.inner.load_index_state(url) {
            IndexLoad::Loaded(index) => index.versions,
            _ => Vec::new(),
        }
    }

    pub fn text(&self, record: &VersionRecord) -> Option<String> {
        self.text_for_hash(&record.hash)
    }

    /// Convenience for callers that treat "gone" and "broken" alike. Prefer
    /// [`SnapshotStore::content_for_hash`] anywhere the difference is visible.
    pub fn text_for_hash(&self, hash: &str) -> Option<String> {
        match self.content_for_hash(hash) {
            Content::Text(text) => Some(text),
            _ => None,
        }
    }

    pub fn content(&self, record: &VersionRecord) -> Content {
        self.content_for_hash(&record.hash)
    }

    /// Reads an object and verifies it. The file name *is* the checksum: the
    /// only trustworthy test of "did this survive" is hashing what came back.
    /// That is also what separates a legitimate pre-compression object (raw
    /// UTF-8, hashes correctly) from a truncated compressed one.
    ///
    /// `String(data:encoding: .utf8)` drops a leading byte-order mark, so a
    /// text that starts with U+FEFF never hashes back and reads as corrupt,
    /// in Swift and here alike.
    pub fn content_for_hash(&self, hash: &str) -> Content {
        if hash.is_empty() {
            return Content::Missing;
        }
        let url = self.inner.object_url(hash);
        let Some(stored) = file_manager::data_contents_of(&url) else {
            return Content::Missing;
        };
        if let Some(raw) = file_manager::decompressed_zlib(&stored)
            && let Some(text) = file_manager::string_from_utf8_data(&raw)
            && Self::hash(&text) == hash
        {
            return Content::Text(text);
        }
        // Objects written before compression are raw UTF-8.
        if let Some(text) = file_manager::string_from_utf8_data(&stored)
            && Self::hash(&text) == hash
        {
            return Content::Text(text);
        }
        Content::Corrupt
    }

    /// Waits for all writes queued before this call. Blocks the caller.
    pub fn wait_for_pending_writes(&self) {
        self.inner.queue.exec_sync(|| {});
    }

    /// Runs `completion` on the history queue once every write queued before
    /// this call has finished: the non-blocking form of
    /// [`SnapshotStore::wait_for_pending_writes`].
    pub fn after_pending_writes(&self, completion: impl FnOnce() + Send + 'static) {
        self.inner.queue.exec_async(completion);
    }

    /// Runs pruning on the same serial executor as object and index writes,
    /// so launch maintenance cannot delete an object between its write and
    /// the matching index append.
    pub fn schedule_prune(&self) {
        let inner = Arc::clone(&self.inner);
        self.inner.queue.exec_async(move || {
            *inner.last_prune.lock().unwrap() = Date::now();
            let _ = inner.prune();
        });
    }

    /// Executes exactly one prune generation and waits for it. Blocks the
    /// caller.
    pub fn prune_one_generation_for_testing(&self) {
        let inner = Arc::clone(&self.inner);
        self.inner.queue.exec_sync(move || {
            let _ = inner.prune();
        });
    }

    /// Total bytes held by the store, for the preferences pane.
    pub fn total_bytes(&self) -> isize {
        let Some(urls) = file_manager::enumerate(&self.inner.objects_directory()) else {
            return 0;
        };
        urls.iter()
            .map(|url| file_manager::resource_values(url).and_then(|values| values.file_size).unwrap_or(0) as isize)
            .sum()
    }

    /// Drops one document's entire history — the "Forget this document's
    /// history" action. The index goes immediately; the objects go on the
    /// sweep that follows, because another document may share them.
    pub fn forget(&self, url: &FileUrl) {
        let document_key = Self::document_key(url);
        {
            let mut pending = self.inner.pending.lock().unwrap();
            // Keep a loaded empty state until the queued deletion runs. A new
            // record immediately after forget must not read the old index.
            store_state(&mut pending, DocumentState { newest_hash: None, is_loaded: true }, &document_key);
            pending.evicted_versions_by_document.remove(&document_key);
        }
        let inner = Arc::clone(&self.inner);
        let url = url.clone();
        self.inner.queue.exec_async(move || {
            let _ = file_manager::remove_item(&inner.index_url(&url));
            {
                let mut pending = inner.pending.lock().unwrap();
                if pending.state_by_document.get(&document_key).is_some_and(|state| state.newest_hash.is_none()) {
                    pending.state_by_document.remove(&document_key);
                    pending.state_order.retain(|key| *key != document_key);
                }
            }
            // Deleting the index only unreferences the objects. Sweep now so
            // "forget" actually reclaims the disk the user asked us to give
            // back.
            inner.collect_unreferenced_objects();
        });
    }

    /// `objectInventory()`.
    pub fn object_inventory(&self) -> Vec<ObjectEntry> {
        self.inner.object_inventory()
    }

    /// The report of the last prune generation (private in Swift; the
    /// conformance dump reads it).
    pub fn last_prune_report(&self) -> PruneReport {
        self.inner.pending.lock().unwrap().last_report.clone()
    }

    // MARK: Hashing

    pub fn hash(text: &str) -> String {
        DocumentIO::content_hash(text)
    }

    pub fn hash_data(data: &[u8]) -> String {
        DocumentIO::content_hash_data(data)
    }

    pub fn document_key(url: &FileUrl) -> String {
        Self::hash(&url.resolving_symlinks_in_path().path())
    }

    /// Where the store lives.
    pub fn history_directory(&self) -> &FileUrl {
        &self.inner.history_directory
    }
}

/// This cache only coalesces writes. Evicting an old entry is safe because the
/// next write reloads its compact index from disk.
fn store_state(pending: &mut Pending, state: DocumentState, key: &str) {
    pending.state_by_document.insert(key.to_owned(), state);
    pending.state_order.retain(|existing| existing != key);
    pending.state_order.push(key.to_owned());
    while pending.state_order.len() > MAXIMUM_CACHED_DOCUMENTS {
        let evicted = pending.state_order.remove(0);
        pending.state_by_document.remove(&evicted);
    }
}

impl Inner {
    fn objects_directory(&self) -> FileUrl {
        self.history_directory.appending_path_component_is_directory("objects", true)
    }

    fn index_directory(&self) -> FileUrl {
        self.history_directory.appending_path_component_is_directory("index", true)
    }

    // MARK: Object storage

    fn object_url(&self, hash: &str) -> FileUrl {
        self.objects_directory()
            .appending_path_component_is_directory(upleft_swift_text::prefix(hash, 2), true)
            .appending_path_component(hash)
    }

    fn write_object_if_needed(&self, hash: &str, data: &[u8]) {
        let url = self.object_url(hash);
        if file_manager::file_exists(&url.path()) {
            return;
        }
        app_paths::ensure(url.deleting_last_path_component());
        let payload = file_manager::compressed_zlib(data).unwrap_or_else(|| data.to_vec());
        let _ = file_manager::write_atomic(&payload, &url);
    }

    // MARK: Index

    fn index_url(&self, url: &FileUrl) -> FileUrl {
        self.index_directory().appending_path_component(&(SnapshotStore::document_key(url) + ".json"))
    }

    fn load_index_state(&self, url: &FileUrl) -> IndexLoad {
        let file_url = self.index_url(url);
        if !file_manager::file_exists(&file_url.path()) {
            return IndexLoad::Missing;
        }
        match file_manager::data_contents_of(&file_url).map(|data| Index::decode(&data)) {
            Some(Ok(index)) => IndexLoad::Loaded(index),
            _ => IndexLoad::Unreadable,
        }
    }

    fn save_index(&self, index: &Index, url: &FileUrl) {
        let _ = file_manager::write_atomic(&index.encoded(), &self.index_url(url));
    }

    // MARK: Pruning

    fn prune_if_due(&self) {
        let mut last_prune = self.last_prune.lock().unwrap();
        if Date::now().time_interval_since(*last_prune) <= PRUNE_INTERVAL {
            return;
        }
        *last_prune = Date::now();
        drop(last_prune);
        self.prune();
    }

    /// Trims every document against the age cap and its **own** size budget,
    /// then applies the global cap as a backstop, then garbage-collects
    /// objects no index references any more.
    fn prune(&self) -> PruneReport {
        let (maximum_age, maximum_bytes, maximum_bytes_per_document) = {
            let pending = self.pending.lock().unwrap();
            (pending.stored_maximum_age, pending.stored_maximum_bytes, pending.stored_maximum_bytes_per_document)
        };
        let cutoff = Date::now().adding(-maximum_age);
        let mut report = PruneReport::default();
        let mut referenced: HashSet<String> = HashSet::new();
        let mut newest_hashes: HashSet<String> = HashSet::new();

        let Ok(indexes) = file_manager::contents_of_directory(&self.index_directory()) else {
            // No complete index inventory means no safe answer to
            // "unreferenced". Fail closed and leave every object alone.
            self.record_prune(&report);
            return report;
        };
        let mut has_incomplete_reference_knowledge = false;
        let mut live_indexes: Vec<FileUrl> = Vec::new();
        for index_file in indexes.iter().filter(|file| file.path_extension() == "json") {
            let document_key = index_file.deleting_path_extension().last_path_component();
            let Some(Ok(mut index)) = file_manager::data_contents_of(index_file).map(|data| Index::decode(&data)) else {
                has_incomplete_reference_knowledge = true;
                continue;
            };
            let before = index.versions.len();

            // Always keep the newest version even if it is older than the cap:
            // a document you haven't touched in six weeks should still have a
            // "what did it look like before" to compare against.
            let newest = index.versions.last().cloned();
            let newest_hash = newest.as_ref().map(|newest| newest.hash.clone());
            index.versions.retain(|version| !(version.date < cutoff && Some(&version.hash) != newest_hash.as_ref()));
            if index.versions.is_empty()
                && let Some(newest) = newest
            {
                index.versions = vec![newest];
            }

            // Per-document size budget, oldest first, newest always kept.
            let mut total: isize = index.versions.iter().map(|version| version.byte_count).sum();
            while total > maximum_bytes_per_document && index.versions.len() > 1 {
                total -= index.versions.remove(0).byte_count;
            }

            let dropped = before as isize - index.versions.len() as isize;
            if dropped > 0 {
                *report.dropped_versions.entry(document_key.clone()).or_insert(0) += dropped;
                let _ = file_manager::write_atomic(&index.encoded(), index_file);
            }
            live_indexes.push(index_file.clone());
            referenced.extend(index.versions.iter().map(|version| version.hash.clone()));
            if let Some(newest) = index.versions.last() {
                newest_hashes.insert(newest.hash.clone());
            }
        }

        if has_incomplete_reference_knowledge {
            self.record_prune(&report);
            return report;
        }

        let objects = self.object_inventory();
        for object in objects.iter().filter(|object| !referenced.contains(&object.hash)) {
            let _ = file_manager::remove_item(&object.url);
            report.freed_bytes += object.size;
        }

        let mut live: Vec<&ObjectEntry> = objects.iter().filter(|object| referenced.contains(&object.hash)).collect();
        let mut total: isize = live.iter().map(|object| object.size).sum();
        if total <= maximum_bytes {
            self.record_prune(&report);
            return report;
        }

        // Swift's `sort` is stable, as `sort_by` is.
        live.sort_by(|a, b| a.date.partial_cmp(&b.date).unwrap_or(std::cmp::Ordering::Equal));
        let mut dropped: HashSet<String> = HashSet::new();
        // Keep every document's newest version. The in-memory reservation
        // cache relies on the index's newest hash remaining durable.
        for object in live {
            if !(total > maximum_bytes && !newest_hashes.contains(&object.hash)) {
                continue;
            }
            let _ = file_manager::remove_item(&object.url);
            dropped.insert(object.hash.clone());
            report.freed_bytes += object.size;
            total -= object.size;
        }
        if dropped.is_empty() {
            self.record_prune(&report);
            return report;
        }

        for index_file in &live_indexes {
            let Some(Ok(mut index)) = file_manager::data_contents_of(index_file).map(|data| Index::decode(&data)) else {
                continue;
            };
            let document_key = index_file.deleting_path_extension().last_path_component();
            let before = index.versions.len();
            index.versions.retain(|version| !dropped.contains(&version.hash));
            let removed = before as isize - index.versions.len() as isize;
            if removed <= 0 {
                continue;
            }
            *report.dropped_versions.entry(document_key).or_insert(0) += removed;
            let _ = file_manager::write_atomic(&index.encoded(), index_file);
        }
        self.record_prune(&report);
        report
    }

    /// Removes objects no index mentions. Split out of `prune()` so `forget`
    /// can reclaim one document's disk without a full age/size pass.
    fn collect_unreferenced_objects(&self) {
        let mut referenced: HashSet<String> = HashSet::new();
        let indexes = file_manager::contents_of_directory(&self.index_directory()).unwrap_or_default();
        for index_file in indexes.iter().filter(|file| file.path_extension() == "json") {
            let Some(Ok(index)) = file_manager::data_contents_of(index_file).map(|data| Index::decode(&data)) else {
                // An index that cannot be decoded may be the only thing
                // referencing its objects. Same fail-closed rule as prune():
                // with incomplete reference knowledge, delete nothing.
                return;
            };
            referenced.extend(index.versions.into_iter().map(|version| version.hash));
        }
        for object in self.object_inventory() {
            if !referenced.contains(&object.hash) {
                let _ = file_manager::remove_item(&object.url);
            }
        }
    }

    fn object_inventory(&self) -> Vec<ObjectEntry> {
        let mut objects = Vec::new();
        let Some(urls) = file_manager::enumerate(&self.objects_directory()) else {
            return objects;
        };
        for url in urls {
            let Some(values) = file_manager::resource_values(&url) else {
                continue;
            };
            let name = url.last_path_component();
            if !values.is_regular_file || upleft_swift_text::count(&name) != 64 {
                continue;
            }
            let Some(size) = values.file_size else {
                continue;
            };
            objects.push(ObjectEntry {
                url,
                size: size as isize,
                date: values.content_modification_date.unwrap_or(DISTANT_PAST),
                hash: name,
            });
        }
        objects
    }

    fn record_prune(&self, report: &PruneReport) {
        let mut pending = self.pending.lock().unwrap();
        pending.last_report = report.clone();
        for (key, count) in &report.dropped_versions {
            *pending.evicted_versions_by_document.entry(key.clone()).or_insert(0) += count;
        }
    }
}
