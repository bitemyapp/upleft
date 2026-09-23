//! Port of `Sources/DownrightApp/AI/DocumentStateStore.swift`.
//!
//! Per-document reading state (§8.2, §9.3).
//!
//! Long documents behave like books: you get your place back. The position is
//! stored as a **heading anchor plus an offset into that section**, never as a
//! byte offset — an agent that inserts two paragraphs at the top of the file
//! would otherwise land you two paragraphs off every time.
//!
//! State files and `recents.json` are written by a `JSONEncoder` without
//! `.sortedKeys` (Swift's key order changes from run to run; the port writes
//! `CodingKeys` order), and the three `Set` fields are written in whatever
//! order Swift's per-process hash seed gives; the port writes them sorted.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use upleft_core::contracts::ZoomLevel;
use upleft_core::metrics::Metrics;
use upleft_core::model::ParsedDocument;
use upleft_foundation::date::Date;
use upleft_foundation::decodable::{self, DecodableValue, DecodingError, Value};
use upleft_foundation::file_manager;
use upleft_foundation::json_encoder::{self, JsonValue, OutputFormatting};
use upleft_foundation::url::FileUrl;
use upleft_render::render_contracts::RenderMode;

use crate::ai::change_tracker::PersistedMark;
use crate::ai::snapshot_store::SnapshotStore;
use crate::support::app_paths;

/// `ScrollAnchor`.
#[derive(Clone, Debug, PartialEq)]
pub struct ScrollAnchor {
    /// Slug of the nearest heading at or above the viewport top.
    pub heading_slug: String,
    /// Index of that heading, as a tiebreak when slugs repeat.
    pub heading_index: isize,
    /// Fraction of the way through that section, 0…1.
    pub fraction_through_section: f64,
}

impl ScrollAnchor {
    /// `ScrollAnchor.top`.
    pub fn top() -> ScrollAnchor {
        ScrollAnchor { heading_slug: String::new(), heading_index: 0, fraction_through_section: 0.0 }
    }

    pub fn encode(&self) -> JsonValue {
        JsonValue::object([
            ("headingSlug", JsonValue::from(self.heading_slug.as_str())),
            ("headingIndex", JsonValue::Int(self.heading_index as i64)),
            ("fractionThroughSection", JsonValue::Double(self.fraction_through_section)),
        ])
    }

    pub fn decode(value: &Value) -> Result<ScrollAnchor, DecodingError> {
        let keyed = value.keyed_container()?;
        Ok(ScrollAnchor {
            heading_slug: keyed.decode("headingSlug", Value::string_value)?,
            heading_index: keyed.decode("headingIndex", Value::int_value)? as isize,
            fraction_through_section: keyed.decode("fractionThroughSection", Value::double_value)?,
        })
    }
}

/// A Swift `Set<String>`: members are compared with Swift's `==`, which is
/// canonical equivalence (`"é"` and `"e\u{301}"` are one member), and the
/// first spelling inserted is the one kept. Iteration is in insertion order;
/// encoding writes the members sorted by their UTF-8 bytes (Swift writes them
/// in its per-process hash order).
#[derive(Clone, Debug, Default)]
pub struct StringSet {
    members: Vec<String>,
}

impl StringSet {
    pub fn new() -> StringSet {
        StringSet::default()
    }

    /// `insert(_:)`: `false`, and no change, when an equal member exists.
    pub fn insert(&mut self, member: impl Into<String>) -> bool {
        let member = member.into();
        if self.contains(&member) {
            return false;
        }
        self.members.push(member);
        true
    }

    pub fn contains(&self, member: &str) -> bool {
        self.members.iter().any(|existing| upleft_swift_text::str_eq(existing, member))
    }

    /// `remove(_:)`.
    pub fn remove(&mut self, member: &str) -> Option<String> {
        let index = self.members.iter().position(|existing| upleft_swift_text::str_eq(existing, member))?;
        Some(self.members.remove(index))
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, String> {
        self.members.iter()
    }

    /// The members sorted by their UTF-8 bytes.
    pub fn sorted(&self) -> Vec<&String> {
        let mut sorted: Vec<&String> = self.members.iter().collect();
        sorted.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
        sorted
    }
}

impl PartialEq for StringSet {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.members.iter().all(|member| other.contains(member))
    }
}

impl<S: Into<String>> FromIterator<S> for StringSet {
    fn from_iter<I: IntoIterator<Item = S>>(iter: I) -> StringSet {
        let mut set = StringSet::new();
        for member in iter {
            set.insert(member);
        }
        set
    }
}

/// `RenderMode` as `Codable` sees it: its raw string.
pub fn decode_render_mode(value: &Value) -> Result<RenderMode, DecodingError> {
    value.raw_string_enum(RenderMode::from_raw_value)
}

/// `ZoomLevel` as `Codable` sees it: its raw `Int`.
pub fn decode_zoom_level(value: &Value) -> Result<ZoomLevel, DecodingError> {
    value.raw_int_enum(|raw| isize::try_from(raw).ok().and_then(ZoomLevel::from_raw_value))
}

/// `DocumentState`.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentState {
    pub path: String,
    /// Content hash of what was on disk when the document was last closed.
    pub last_seen_hash: String,
    /// Content hash of the document as the reader last *finished reviewing*
    /// it. Empty means "never reviewed".
    pub review_baseline_hash: String,
    /// The unreviewed mark set, re-anchored on reopen (§8.2).
    pub marks: Vec<PersistedMark>,
    pub anchor: ScrollAnchor,
    pub mode: RenderMode,
    pub zoom_level: ZoomLevel,
    /// Heading slugs whose sections are folded.
    pub folded_headings: StringSet,
    /// Source offsets of code blocks the user explicitly expanded or
    /// collapsed, overriding the auto-collapse rule (§5.1).
    pub expanded_code_blocks: BTreeSet<isize>,
    pub collapsed_code_blocks: BTreeSet<isize>,
    pub last_opened: Date,
    pub sidebar_visible: bool,
    pub selection_location: isize,
    pub selection_length: isize,
    pub split_view_enabled: bool,
}

impl DocumentState {
    /// `DocumentState(path:)`.
    pub fn new(path: &str) -> DocumentState {
        DocumentState {
            path: path.to_owned(),
            last_seen_hash: String::new(),
            review_baseline_hash: String::new(),
            marks: Vec::new(),
            anchor: ScrollAnchor::top(),
            mode: RenderMode::Live,
            zoom_level: ZoomLevel::Everything,
            folded_headings: StringSet::new(),
            expanded_code_blocks: BTreeSet::new(),
            collapsed_code_blocks: BTreeSet::new(),
            last_opened: Date::now(),
            sidebar_visible: false,
            selection_location: 0,
            selection_length: 0,
            split_view_enabled: false,
        }
    }

    /// Synthesized `encode(to:)` under `.iso8601`.
    pub fn encode(&self) -> JsonValue {
        JsonValue::object([
            ("path", JsonValue::from(self.path.as_str())),
            ("lastSeenHash", JsonValue::from(self.last_seen_hash.as_str())),
            ("reviewBaselineHash", JsonValue::from(self.review_baseline_hash.as_str())),
            ("marks", JsonValue::Array(self.marks.iter().map(PersistedMark::encode).collect())),
            ("anchor", self.anchor.encode()),
            ("mode", JsonValue::from(self.mode.raw_value())),
            ("zoomLevel", JsonValue::Int(self.zoom_level.raw_value() as i64)),
            (
                "foldedHeadings",
                JsonValue::Array(self.folded_headings.sorted().into_iter().map(|slug| JsonValue::from(slug.as_str())).collect()),
            ),
            (
                "expandedCodeBlocks",
                JsonValue::Array(self.expanded_code_blocks.iter().map(|offset| JsonValue::Int(*offset as i64)).collect()),
            ),
            (
                "collapsedCodeBlocks",
                JsonValue::Array(self.collapsed_code_blocks.iter().map(|offset| JsonValue::Int(*offset as i64)).collect()),
            ),
            ("lastOpened", JsonValue::from(self.last_opened.iso8601())),
            ("sidebarVisible", JsonValue::Bool(self.sidebar_visible)),
            ("selectionLocation", JsonValue::Int(self.selection_location as i64)),
            ("selectionLength", JsonValue::Int(self.selection_length as i64)),
            ("splitViewEnabled", JsonValue::Bool(self.split_view_enabled)),
        ])
    }

    /// The custom `init(from:)`: older state files won't have every key, so
    /// every key is optional. A key that is present must still decode.
    pub fn decode(value: &Value) -> Result<DocumentState, DecodingError> {
        let c = value.keyed_container()?;
        let path = c.decode_if_present("path", Value::string_value)?.unwrap_or_default();
        let last_seen_hash = c.decode_if_present("lastSeenHash", Value::string_value)?.unwrap_or_default();
        // A state file written before the review baseline existed has only
        // the disk hash to offer; adopting it shows nothing new, which is
        // exactly what the old build already showed.
        let review_baseline_hash = match c.decode_if_present("reviewBaselineHash", Value::string_value)? {
            Some(hash) => hash,
            None => last_seen_hash.clone(),
        };
        let marks = c.decode_if_present("marks", |value| value.array_of(PersistedMark::decode))?.unwrap_or_default();
        let anchor = c.decode_if_present("anchor", ScrollAnchor::decode)?.unwrap_or_else(ScrollAnchor::top);
        // Mode used to persist Read/Source. The adaptive Document surface is
        // now the only restorable state; Source Focus is always transient.
        let _ = c.decode_if_present("mode", decode_render_mode)?;
        // Structural zoom belonged to the old read-only mode; always reopen a
        // complete document.
        let _ = c.decode_if_present("zoomLevel", decode_zoom_level)?;
        let folded_headings = c
            .decode_if_present("foldedHeadings", |value| value.array_of(|element| element.string_value()))?
            .unwrap_or_default()
            .into_iter()
            .collect();
        let offsets = |key: &str| -> Result<BTreeSet<isize>, DecodingError> {
            Ok(c.decode_if_present(key, |value| value.array_of(|element| element.int_value().map(|offset| offset as isize)))?
                .unwrap_or_default()
                .into_iter()
                .collect())
        };
        let expanded_code_blocks = offsets("expandedCodeBlocks")?;
        let collapsed_code_blocks = offsets("collapsedCodeBlocks")?;
        let last_opened = c.decode_if_present("lastOpened", Value::date_iso8601)?.unwrap_or_else(Date::now);
        Ok(DocumentState {
            path,
            last_seen_hash,
            review_baseline_hash,
            marks,
            anchor,
            mode: RenderMode::Live,
            zoom_level: ZoomLevel::Everything,
            folded_headings,
            expanded_code_blocks,
            collapsed_code_blocks,
            last_opened,
            sidebar_visible: c.decode_if_present("sidebarVisible", Value::bool_value)?.unwrap_or(false),
            selection_location: c.decode_if_present("selectionLocation", Value::int_value)?.unwrap_or(0) as isize,
            selection_length: c.decode_if_present("selectionLength", Value::int_value)?.unwrap_or(0) as isize,
            split_view_enabled: c.decode_if_present("splitViewEnabled", Value::bool_value)?.unwrap_or(false),
        })
    }

    /// `JSONEncoder.snapshotEncoder.encode(state)`.
    pub fn encoded(&self) -> Vec<u8> {
        json_encoder::encode(&self.encode(), OutputFormatting::DEFAULT)
    }

    /// `JSONDecoder.snapshotDecoder.decode(DocumentState.self, from: data)`.
    pub fn decoded(data: &[u8]) -> Result<DocumentState, DecodingError> {
        DocumentState::decode(&decodable::parse(data)?)
    }
}

/// `RecentDocument`: an entry in the recents list, kept with enough detail to
/// draw a rendered thumbnail without opening the file (§9.3).
#[derive(Clone, Debug, PartialEq)]
pub struct RecentDocument {
    pub path: String,
    pub display_name: String,
    pub first_heading: String,
    pub last_opened: Date,
    pub word_count: isize,
}

impl RecentDocument {
    /// `Identifiable.id`.
    pub fn id(&self) -> &str {
        &self.path
    }

    pub fn encode(&self) -> JsonValue {
        JsonValue::object([
            ("path", JsonValue::from(self.path.as_str())),
            ("displayName", JsonValue::from(self.display_name.as_str())),
            ("firstHeading", JsonValue::from(self.first_heading.as_str())),
            ("lastOpened", JsonValue::from(self.last_opened.iso8601())),
            ("wordCount", JsonValue::Int(self.word_count as i64)),
        ])
    }

    pub fn decode(value: &Value) -> Result<RecentDocument, DecodingError> {
        let keyed = value.keyed_container()?;
        Ok(RecentDocument {
            path: keyed.decode("path", Value::string_value)?,
            display_name: keyed.decode("displayName", Value::string_value)?,
            first_heading: keyed.decode("firstHeading", Value::string_value)?,
            last_opened: keyed.decode("lastOpened", Value::date_iso8601)?,
            word_count: keyed.decode("wordCount", Value::int_value)? as isize,
        })
    }
}

fn encode_recents(list: &[RecentDocument]) -> Vec<u8> {
    json_encoder::encode(&JsonValue::Array(list.iter().map(RecentDocument::encode).collect()), OutputFormatting::DEFAULT)
}

struct Cache {
    states: HashMap<String, DocumentState>,
    order: Vec<String>,
}

const MAXIMUM_CACHED_STATES: usize = 256;

/// `DocumentStateStore`.
pub struct DocumentStateStore {
    cache: Mutex<Cache>,
    support_directory: FileUrl,
    state_directory: FileUrl,
}

static SHARED: OnceLock<DocumentStateStore> = OnceLock::new();

impl DocumentStateStore {
    /// `DocumentStateStore.shared`.
    pub fn shared() -> &'static DocumentStateStore {
        SHARED.get_or_init(|| DocumentStateStore::new(app_paths::support_directory()))
    }

    /// `init(supportDirectory:)`: injectable so tests cannot touch the user's
    /// real reading positions or recents.
    pub fn new(support_directory: FileUrl) -> DocumentStateStore {
        let support_directory = support_directory.standardized_file_url();
        let state_directory = support_directory.appending_path_component_is_directory("state", true);
        app_paths::ensure(state_directory.clone());
        DocumentStateStore {
            cache: Mutex::new(Cache { states: HashMap::new(), order: Vec::new() }),
            support_directory,
            state_directory,
        }
    }

    // MARK: Per-document state

    pub fn state(&self, url: &FileUrl) -> DocumentState {
        let key = SnapshotStore::document_key(url);
        let mut cache = self.cache.lock().unwrap();
        if let Some(cached) = cache.states.get(&key).cloned() {
            touch_cache_key(&mut cache, &key);
            return cached;
        }
        let file_url = self.state_directory.appending_path_component(&(key.clone() + ".json"));
        let mut state = match file_manager::data_contents_of(&file_url).map(|data| DocumentState::decoded(&data)) {
            Some(Ok(decoded)) => decoded,
            _ => DocumentState::new(&url.path()),
        };
        state.path = url.path();
        store_in_cache(&mut cache, state.clone(), &key);
        state
    }

    pub fn save(&self, state: &DocumentState, url: &FileUrl) {
        let key = SnapshotStore::document_key(url);
        let data = state.encoded();
        let file_url = self.state_directory.appending_path_component(&(key.clone() + ".json"));
        let mut cache = self.cache.lock().unwrap();
        store_in_cache(&mut cache, state.clone(), &key);
        let _ = file_manager::write_atomic(&data, &file_url);
    }

    // MARK: Recents

    fn recents_url(&self) -> FileUrl {
        self.support_directory.appending_path_component("recents.json")
    }

    /// `recents(limit:)` (Swift's default limit is 30).
    pub fn recents(&self, limit: usize) -> Vec<RecentDocument> {
        let Some(data) = file_manager::data_contents_of(&self.recents_url()) else {
            return Vec::new();
        };
        let Ok(list) = decodable::parse(&data).and_then(|value| value.array_of(RecentDocument::decode)) else {
            return Vec::new();
        };
        let mut seen: HashSet<String> = HashSet::new();
        let mut live: Vec<RecentDocument> = list
            .into_iter()
            .filter_map(|recent| {
                let canonical = Self::canonical_path(&recent.path);
                if !file_manager::file_exists(&canonical) {
                    return None;
                }
                if !seen.insert(canonical.clone()) {
                    return None;
                }
                Some(RecentDocument { path: canonical, ..recent })
            })
            .collect();
        // Swift's `sorted(by:)` is stable, as `sort_by` is.
        live.sort_by(|a, b| {
            if a.last_opened > b.last_opened {
                std::cmp::Ordering::Less
            } else if b.last_opened > a.last_opened {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        live.truncate(limit);
        live
    }

    /// Resolves a stored path the same way `AppDelegate.open` identifies a
    /// window, so a file reached through a symlink is the same recent entry
    /// everywhere instead of a duplicate.
    pub fn canonical_path(path: &str) -> String {
        FileUrl::from_path(path).resolving_symlinks_in_path().standardized_file_url().path()
    }

    pub fn note_opened(&self, url: &FileUrl, document: &ParsedDocument) {
        let canonical = Self::canonical_path(&url.path());
        let mut list: Vec<RecentDocument> =
            self.recents(200).into_iter().filter(|recent| Self::canonical_path(&recent.path) != canonical).collect();
        list.insert(
            0,
            RecentDocument {
                path: canonical,
                display_name: url.deleting_path_extension().last_path_component(),
                first_heading: document.headings.first().map(|heading| heading.title.clone()).unwrap_or_default(),
                last_opened: Date::now(),
                // The caller already parsed this text.
                word_count: Metrics::document_word_count(document),
            },
        );
        list.truncate(60);
        let _ = file_manager::write_atomic(&encode_recents(&list), &self.recents_url());
    }

    /// Drops one entry and leaves the rest alone, matched on the canonical
    /// path.
    pub fn remove_recent(&self, path: &str) {
        let canonical = Self::canonical_path(path);
        let list: Vec<RecentDocument> =
            self.recents(200).into_iter().filter(|recent| Self::canonical_path(&recent.path) != canonical).collect();
        let _ = file_manager::write_atomic(&encode_recents(&list), &self.recents_url());
    }

    pub fn clear_recents(&self) {
        let _ = file_manager::remove_item(&self.recents_url());
    }
}

fn store_in_cache(cache: &mut Cache, state: DocumentState, key: &str) {
    cache.states.insert(key.to_owned(), state);
    touch_cache_key(cache, key);
    while cache.order.len() > MAXIMUM_CACHED_STATES {
        let evicted = cache.order.remove(0);
        cache.states.remove(&evicted);
    }
}

fn touch_cache_key(cache: &mut Cache, key: &str) {
    cache.order.retain(|existing| existing != key);
    cache.order.push(key.to_owned());
}

// MARK: - Anchoring

/// `ScrollAnchoring`.
pub struct ScrollAnchoring;

impl ScrollAnchoring {
    /// Builds an anchor for a source offset. Used when the buffer is about to
    /// be replaced under the reader (§8.1) and when closing the document.
    pub fn anchor(offset: isize, document: &ParsedDocument) -> ScrollAnchor {
        let first = document.headings.first();
        let Some(first_heading) = first.filter(|heading| offset >= heading.range.location) else {
            let preamble_length = first.map(|heading| heading.range.location).unwrap_or(document.length);
            let fraction = if preamble_length > 0 {
                swift_min(1.0, swift_max(0.0, offset as f64 / preamble_length as f64))
            } else {
                0.0
            };
            return ScrollAnchor { heading_slug: String::new(), heading_index: 0, fraction_through_section: fraction };
        };
        let _ = first_heading;

        let mut index = 0usize;
        for (i, heading) in document.headings.iter().enumerate() {
            if heading.range.location <= offset {
                index = i;
            }
        }
        let heading = &document.headings[index];
        let span = 1.max(heading.section_range.length);
        let within = 0.max(offset - heading.section_range.location).min(span);
        ScrollAnchor {
            heading_slug: heading.slug.clone(),
            heading_index: index as isize,
            fraction_through_section: within as f64 / span as f64,
        }
    }

    /// Resolves an anchor back to a source offset in a possibly-rewritten
    /// document. Matching by slug first is what makes the position survive an
    /// agent inserting a whole new section above where you were reading.
    pub fn offset(anchor: &ScrollAnchor, document: &ParsedDocument) -> isize {
        if document.headings.is_empty() {
            return int(anchor.fraction_through_section * document.length as f64);
        }
        if anchor.heading_slug.is_empty() {
            let preamble_length = document.headings.first().map(|heading| heading.range.location).unwrap_or(document.length);
            return int(anchor.fraction_through_section * preamble_length as f64);
        }

        let candidates: Vec<(isize, &upleft_core::model::HeadingNode)> = document
            .headings
            .iter()
            .enumerate()
            .filter(|(_, heading)| heading.slug == anchor.heading_slug)
            .map(|(offset, heading)| (offset as isize, heading))
            .collect();
        let heading = if let Some((_, exact)) = candidates.iter().find(|(offset, _)| *offset == anchor.heading_index) {
            *exact
        } else if let Some((_, nearest)) = candidates.iter().fold(None::<&(isize, &upleft_core::model::HeadingNode)>, |best, candidate| {
            match best {
                Some(best) if !((candidate.0 - anchor.heading_index).abs() < (best.0 - anchor.heading_index).abs()) => Some(best),
                _ => Some(candidate),
            }
        }) {
            *nearest
        } else if anchor.heading_index < document.headings.len() as isize {
            // The heading is gone entirely — fall back to positional.
            &document.headings[usize::try_from(anchor.heading_index).expect("Index out of range")]
        } else {
            &document.headings[document.headings.len() - 1]
        };

        let offset = heading.section_range.location
            + int(anchor.fraction_through_section * heading.section_range.length as f64);
        0.max(offset).min(document.length)
    }
}

/// Swift's `min(_:_:)`: `y < x ? y : x`.
fn swift_min(x: f64, y: f64) -> f64 {
    if y < x { y } else { x }
}

/// Swift's `max(_:_:)`: `y >= x ? y : x`.
fn swift_max(x: f64, y: f64) -> f64 {
    if y >= x { y } else { x }
}

/// `Int(x)`: truncates, and traps where Swift traps.
fn int(x: f64) -> isize {
    upleft_render::swift_compat::int_truncating(x) as isize
}
