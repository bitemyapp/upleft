//! Rust side of the `formats` suite; mirrors
//! `oracle/app/Sources/downright-app-oracle/FormatsDump.swift` step for step
//! (see its doc comment for how files are dumped and what is normalised).
//!
//! # Script format
//!
//! A script (`corpus/formats/*.json`) is an object:
//!
//! ```json
//! { "supportDirectory": "support", "parsed": ["profiles.json"], "steps": [ {"op": "…", …}, … ] }
//! ```
//!
//! * `supportDirectory` (optional) sets `DOWNRIGHT_SUPPORT_DIRECTORY` to that
//!   sandbox path before any step; `TrustStore.shared` (`trust.open` with
//!   `"store": "shared"`) requires it, so nothing reaches the real home.
//! * `parsed` (optional) lists more files to dump as parsed JSON.
//! * Paths in steps are relative to the sandbox (absolute paths are used as
//!   written). In file text, decoded JSON and resolver tokens, `<root>` is
//!   replaced with the sandbox path. Stores, trackers, resolvers and histories are named by
//!   `store`, `tracker`, `resolver`, `history` (default `"main"`).
//!
//! Operations (fields in parentheses):
//!
//! * files: `write` (path, text | hex), `mkdir` (path), `symlink` (path,
//!   target), `remove` (path), `chmod` (path, mode as a decimal number),
//!   `touch` (path, date: ISO 8601 modification date), `truncate` (path,
//!   length; default half), `exists` (path).
//! * `SnapshotStore`: `snapshot.open` (history), `snapshot.limits`
//!   (maximumAge, maximumBytes, maximumBytesPerDocument), `snapshot.writeIndex`
//!   (doc, text: written to the document's index file),
//!   `snapshot.touchObject` (hash, date: the object's modification date),
//!   `snapshot.record`
//!   (doc, text, kind), `snapshot.wait`, `snapshot.prune` (one generation),
//!   `snapshot.versions` (doc), `snapshot.content` (text | hash | doc +
//!   version), `snapshot.forget` (doc), `snapshot.totalBytes`,
//!   `snapshot.inventory`, `snapshot.hash` (text), `snapshot.documentKey`
//!   (doc).
//! * `DocumentStateStore`: `state.open` (support), `state.writeFile` (doc,
//!   text: written to the document's state file), `state.get` (doc),
//!   `state.save` (doc, base: "current" | "new", set: {field: value};
//!   `marksFromTracker` copies a tracker's persisted marks), `state.decode`
//!   (json), `state.recents` (limit), `state.noteOpened` (doc, text),
//!   `state.removeRecent` (path), `state.clearRecents`, `state.canonicalPath`
//!   (path).
//! * `ScrollAnchoring`: `anchor.make` (text, offset), `anchor.offset` (text,
//!   anchor).
//! * `ChangeTracker`: `tracker.new` (lifetime, dwell), `tracker.apply` (old,
//!   new, hunks?, replacing), `tracker.marks`, `tracker.adjust` (range,
//!   delta), `tracker.next` / `tracker.previous` / `tracker.markAt` (offset),
//!   `tracker.visit` (index), `tracker.noteVisible` (range, now),
//!   `tracker.restore` (marks | from | fromState + doc, textLength, now),
//!   `tracker.merge` (same sources), `tracker.dropExpired` (now),
//!   `tracker.clear`, `tracker.reset`, `tracker.persisted`, `tracker.ranges`.
//! * `Preferences.Values`: `prefs.defaults` / `prefs.decode` (json), each
//!   with optional selectTheme {name, slot} and write (path): the pretty,
//!   sorted encoding, the effective typography, the large-file threshold and
//!   whether the encoding decodes back equal.
//! * reader profiles: `profiles.builtIns`, `profiles.make` (memberwise
//!   fields), `profiles.custom` (name, id?), `profiles.save` (path, profiles),
//!   `profiles.load` (path).
//! * review sidecars: `review.anchor` (text, range, contextLength),
//!   `review.resolve` (anchor: {text, range} | explicit fields, text),
//!   `review.fingerprint` (text), `review.make` (kind, text, range, body,
//!   replacement, name), `review.item` (name, id?, kind, anchor, body,
//!   replacement, state), `review.setState` (review, state), `review.apply`
//!   (review, text), `review.save` (doc, reviews, version), `review.load`
//!   (doc), `review.sidecarURL` (doc), `review.critic` (text).
//! * trust: `trust.open` (store: "shared" | a name, grants), `trust.grant`
//!   (scope, path, effects, externalURL), `trust.revoke` (scope, path),
//!   `trust.state` (doc), `trust.decide` (effect, displayName, canonicalPath,
//!   externalURL, doc, state, grants?), `trust.canonical` (path),
//!   `trust.isWithin` (child, root), `trust.folderScope` (path, isDirectory),
//!   `trust.decodeGrants` (json), `trust.titles`.
//! * `PathResolver` and `ExternalEditor`: `resolver.open` (doc?),
//!   `resolver.resolve` (raw, line), `resolver.invalidate`, `resolver.gitRoot`
//!   (path), `editor.url` (editor, path, line), `editor.all`.
//! * `JumpHistory`: `jump.record` (from?, to: {url?, offset, label}),
//!   `jump.back`, `jump.forward`, `jump.clear`.

use std::cell::Cell;
use std::collections::{BTreeSet, HashMap};
use std::os::unix::fs::PermissionsExt;
use std::rc::Rc;

use serde_json::{Map, Value};
use upleft_app::ai::change_tracker::{ChangeTracker, Mark, PersistedMark, PersistedRange, change_kind_from_raw_value};
use upleft_app::ai::document_state_store::{
    DocumentState, DocumentStateStore, RecentDocument, ScrollAnchor, ScrollAnchoring,
};
use upleft_app::ai::path_resolver::{ExternalEditor, PathResolver};
use upleft_app::ai::snapshot_store::{Content, SnapshotKind, SnapshotStore, VersionRecord};
use upleft_app::review::review_anchor_resolver::ReviewAnchorResolver;
use upleft_app::review::review_sidecar::{
    LocalReviewSidecarStore, ReviewAnchor, ReviewApplyResult, ReviewItem, ReviewKind, ReviewSidecar,
    ReviewSidecarEngine, ReviewSidecarStore, ReviewState,
};
use upleft_app::security::document_trust::{
    DocumentTrust, DocumentTrustState, TrustEffect, TrustGrant, TrustRequest, TrustScope, TrustTarget,
};
use upleft_app::security::trust_store::{InMemoryTrustStorePersistence, TrustStore};
use upleft_app::support::jump_history::{Entry, JumpHistory};
use upleft_app::support::preferences::{ThemePreferenceSlot, Values, effective_typography};
use upleft_app::support::reader_profiles::{
    JSONReaderProfileStore, ReaderChromeDensity, ReaderMotionPreference, ReaderProfile, ReaderProfileStore,
    ReaderTypographyScale,
};
use upleft_core::contracts::{ChangeHunk, ChangeKind, Uuid, ZoomLevel};
use upleft_core::model::PathToken;
use upleft_core::ns_range::NSRange;
use upleft_core::parser::MarkdownParser;
use upleft_core::text_diff::TextDiff;
use upleft_foundation::date::Date;
use upleft_foundation::decodable::{self, DecodableValue, parse_iso8601, parse_uuid, uuid_string};
use upleft_foundation::file_manager;
use upleft_foundation::json_serialization::{self, AnyJson, ReadingOptions, WritingOptions};
use upleft_foundation::url::FileUrl;
use upleft_render::render_contracts::{RenderMode, TypographyConfig};

use super::json::{self as dump_json, Object};
use super::{Failure, Request};

pub fn run(request: &Request) -> Result<(), Failure> {
    let data = std::fs::read(&request.input)?;
    // Read through `JSONSerialization`, as the Swift side reads it (which,
    // for one, drops a string's leading U+FEFF).
    let script = json_serialization::json_object(&data, ReadingOptions::default()).map(any_to_value).map_err(Failure::Error)?;
    let Some(steps) = script.get("steps").and_then(Value::as_array) else {
        return Err(Failure::Error("formats: a script is an object with \"steps\"".into()));
    };
    let mut runner = Runner::new(&script)?;
    let outcome = (|| -> Result<Value, Failure> {
        let mut results = Vec::new();
        for (index, step) in steps.iter().enumerate() {
            let op = step.get("op").and_then(Value::as_str).unwrap_or("").to_owned();
            let value = runner.perform(&op, step)?;
            results.push(Object::new().with("step", index).with("op", op).with("value", value).build());
        }
        runner.drain();
        let files = runner.dump_files();
        Ok(runner.normalize(Object::new().with("results", Value::Array(results)).with("files", Value::Array(files)).build()))
    })();
    runner.cleanup();
    let dump = outcome?;
    Ok(dump_json::write(&dump, &request.output)?)
}

/// A `JSONSerialization` tree as a `serde_json` value: an `NSNumber` holding
/// a `double` or `float` becomes a float, any other an integer.
fn any_to_value(value: AnyJson) -> Value {
    match value {
        AnyJson::Null => Value::Null,
        AnyJson::Bool(flag) => Value::Bool(flag),
        AnyJson::Int(number) => Value::from(number),
        AnyJson::Double(number) => dump_json::double(number),
        AnyJson::Number(number) => match json_serialization::number_objc_type(&number).as_str() {
            "d" | "f" => dump_json::double(number.doubleValue()),
            _ => Value::from(number.longLongValue()),
        },
        AnyJson::String(text) => Value::String(text),
        AnyJson::Array(values) => Value::Array(values.into_iter().map(any_to_value).collect()),
        AnyJson::Object(members) => Value::Object(members.into_iter().map(|(key, value)| (key, any_to_value(value))).collect()),
    }
}

/// `text` or, for texts a script cannot spell in JSON (a leading U+FEFF),
/// `textHex`: `String(decoding: bytes, as: UTF8.self)`.
fn text_field(step: &Step, key: &str) -> String {
    if let Some(hex) = string(step, &format!("{key}Hex")) {
        let bytes: Vec<u8> = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("00"), 16).unwrap_or(0))
            .collect();
        return String::from_utf8_lossy(&bytes).into_owned();
    }
    string(step, key).unwrap_or("").to_owned()
}

fn error<T>(message: impl Into<String>) -> Result<T, Failure> {
    Err(Failure::Error(message.into()))
}

fn require<T>(value: Option<T>, what: &str) -> Result<T, Failure> {
    value.ok_or_else(|| Failure::Error(format!("formats: missing {what}")))
}

/// Swift's `Array(a.utf8).lexicographicallyPrecedes(Array(b.utf8))`.
fn utf8_order(a: &str, b: &str) -> std::cmp::Ordering {
    a.as_bytes().cmp(b.as_bytes())
}

enum TrustHandle {
    Shared(&'static TrustStore),
    Owned(TrustStore),
}

impl TrustHandle {
    fn store(&self) -> &TrustStore {
        match self {
            TrustHandle::Shared(store) => store,
            TrustHandle::Owned(store) => store,
        }
    }
}

struct Runner {
    root: FileUrl,
    root_path: String,
    unresolved_root_path: String,
    run_start: Date,
    parsed_paths: Vec<String>,
    has_support_directory: bool,
    snapshot_stores: HashMap<String, SnapshotStore>,
    history_directories: HashMap<String, FileUrl>,
    state_stores: HashMap<String, DocumentStateStore>,
    support_directories: HashMap<String, FileUrl>,
    trackers: HashMap<String, ChangeTracker>,
    reviews: HashMap<String, ReviewItem>,
    trust_stores: HashMap<String, TrustHandle>,
    resolvers: HashMap<String, PathResolver>,
    histories: HashMap<String, JumpHistory>,
    /// Swift's `Set<String>`: the first spelling of each canonically
    /// equivalent path.
    mentioned_paths: Vec<String>,
}

type Step = Value;

fn string<'a>(step: &'a Step, key: &str) -> Option<&'a str> {
    step.get(key).and_then(Value::as_str)
}

/// `(step[key] as? NSNumber)?.intValue`.
fn int(step: &Step, key: &str) -> Option<i64> {
    step.get(key).and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|number| number as i64)))
}

fn double(step: &Step, key: &str) -> Option<f64> {
    step.get(key).and_then(Value::as_f64)
}

fn boolean(step: &Step, key: &str) -> Option<bool> {
    step.get(key).and_then(Value::as_bool)
}

fn range(value: Option<&Value>) -> NSRange {
    let pair: Vec<isize> = value
        .and_then(Value::as_array)
        .map(|values| values.iter().map(|value| value.as_i64().unwrap_or(0) as isize).collect())
        .unwrap_or_else(|| vec![0, 0]);
    NSRange::new(pair[0], pair[1])
}

fn name(step: &Step, key: &str) -> String {
    string(step, key).unwrap_or("main").to_owned()
}

fn json_range(range: NSRange) -> Value {
    Value::Array(vec![range.location.into(), range.length.into()])
}

fn ranges(values: &[NSRange]) -> Value {
    Value::Array(values.iter().copied().map(json_range).collect())
}

fn optional_string(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |text| Value::String(text.to_owned()))
}

fn hex(data: &[u8]) -> Value {
    Object::new().with("hex", data.iter().map(|byte| format!("{byte:02x}")).collect::<String>()).build()
}

fn lines(data: &[u8]) -> Value {
    if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return hex(data);
    }
    match std::str::from_utf8(data) {
        Ok(text) => Object::new().with("lines", text.split('\n').map(|line| Value::String(line.to_owned())).collect::<Vec<_>>()).build(),
        Err(_) => hex(data),
    }
}

fn error_code(code: isize) -> Value {
    Object::new().with("error", code).build()
}

fn parse_date(text: &str) -> Option<Date> {
    parse_iso8601(text)
}

impl Runner {
    fn new(script: &Value) -> Result<Runner, Failure> {
        let base = FileUrl::from_path(&file_manager::temporary_directory()).appending_path_component_is_directory(
            &format!("upleft-formats-{}", Uuid::new_v4().hyphenated().to_string().to_lowercase()),
            true,
        );
        file_manager::create_directory(&base, true).map_err(Failure::Error)?;
        let root = base.resolving_symlinks_in_path();
        let mut runner = Runner {
            root_path: root.path(),
            root,
            unresolved_root_path: base.path(),
            run_start: Date::now(),
            parsed_paths: script
                .get("parsed")
                .and_then(Value::as_array)
                .map(|paths| paths.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                .unwrap_or_default(),
            has_support_directory: script.get("supportDirectory").is_some_and(Value::is_string),
            snapshot_stores: HashMap::new(),
            history_directories: HashMap::new(),
            state_stores: HashMap::new(),
            support_directories: HashMap::new(),
            trackers: HashMap::new(),
            reviews: HashMap::new(),
            trust_stores: HashMap::new(),
            resolvers: HashMap::new(),
            histories: HashMap::new(),
            mentioned_paths: Vec::new(),
        };
        if let Some(support) = script.get("supportDirectory").and_then(Value::as_str) {
            let path = runner.url(support).path();
            // SAFETY: the oracle is single-threaded here; nothing else reads
            // the environment concurrently.
            unsafe { std::env::set_var("DOWNRIGHT_SUPPORT_DIRECTORY", path) };
        }
        Ok(runner)
    }

    /// A script path: relative to the sandbox, or absolute as written.
    fn url(&mut self, relative: &str) -> FileUrl {
        if relative.starts_with('/') {
            return FileUrl::from_path(relative);
        }
        if !self.mentioned_paths.iter().any(|existing| upleft_swift_text::str_eq(existing, relative)) {
            self.mentioned_paths.push(relative.to_owned());
        }
        if relative.is_empty() { self.root.clone() } else { self.root.appending_path_component(relative) }
    }

    fn path(&mut self, step: &Step, key: &str) -> Result<FileUrl, Failure> {
        let relative = require(string(step, key), key)?.to_owned();
        Ok(self.url(&relative))
    }

    /// A script string with `<root>` spelled out as the sandbox path.
    fn expand(&self, text: Option<&str>) -> String {
        text.unwrap_or("").replace("<root>", &self.root_path)
    }

    fn write_text(text: &str, target: &FileUrl) -> Result<(), Failure> {
        file_manager::create_directory(&target.deleting_last_path_component(), true).map_err(Failure::Error)?;
        file_manager::write(text.as_bytes(), target).map_err(Failure::Error)
    }

    fn cleanup(&self) {
        fn open(path: &std::path::Path) {
            let Ok(metadata) = std::fs::symlink_metadata(path) else { return };
            if !metadata.is_dir() {
                return;
            }
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
            for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
                let child = entry.path();
                if std::fs::symlink_metadata(&child).is_ok_and(|metadata| metadata.is_file()) {
                    let _ = std::fs::set_permissions(&child, std::fs::Permissions::from_mode(0o644));
                }
                open(&child);
            }
        }
        open(std::path::Path::new(&self.root_path));
        let _ = std::fs::remove_dir_all(&self.root_path);
    }

    fn drain(&self) {
        for store in self.snapshot_stores.values() {
            store.wait_for_pending_writes();
        }
    }

    // MARK: Values

    fn stamp(&self, date: Date) -> Value {
        if date.time_interval_since(self.run_start).abs() < 2.0 * 86_400.0 {
            Value::String("<now>".into())
        } else {
            Value::String(date.iso8601())
        }
    }

    fn record(&self, record: Option<&VersionRecord>) -> Value {
        let Some(record) = record else { return Value::Null };
        Object::new()
            .with("hash", record.hash.clone())
            .with("date", self.stamp(record.date))
            .with("byteCount", record.byte_count)
            .with("kind", record.kind.raw_value())
            .build()
    }

    fn content(content: &Content) -> Value {
        match content {
            Content::Text(text) => Object::new().with("text", text.clone()).build(),
            Content::Missing => Value::String("missing".into()),
            Content::Corrupt => Value::String("corrupt".into()),
        }
    }

    fn mark(&self, mark: Option<&Mark>) -> Value {
        let Some(mark) = mark else { return Value::Null };
        Object::new()
            .with("id", uuid_string(mark.id.as_bytes()))
            .with("kind", mark.kind.raw_value())
            .with("range", json_range(mark.range))
            .with("wordRanges", ranges(&mark.word_ranges))
            .with("deletedText", mark.deleted_text.clone())
            .with("created", self.stamp(mark.created))
            .with("visited", mark.visited)
            .with("firstSeen", mark.first_seen.map_or(Value::Null, |date| self.stamp(date)))
            .build()
    }

    fn persisted(&self, mark: &PersistedMark) -> Value {
        Object::new()
            .with("id", uuid_string(mark.id.as_bytes()))
            .with("kind", mark.kind.clone())
            .with("range", json_range(mark.range.range()))
            .with("wordRanges", ranges(&mark.word_ranges.iter().map(PersistedRange::range).collect::<Vec<_>>()))
            .with("deletedText", mark.deleted_text.clone())
            .with("created", self.stamp(mark.created))
            .with("visited", mark.visited)
            .build()
    }

    fn anchor(anchor: &ScrollAnchor) -> Value {
        Object::new()
            .with("headingSlug", anchor.heading_slug.clone())
            .with("headingIndex", anchor.heading_index)
            .with("fractionThroughSection", dump_json::double(anchor.fraction_through_section))
            .build()
    }

    fn scroll_anchor(value: Option<&Value>) -> ScrollAnchor {
        let empty = Value::Object(Map::new());
        let spec = value.filter(|value| value.is_object()).unwrap_or(&empty);
        ScrollAnchor {
            heading_slug: string(spec, "headingSlug").unwrap_or("").to_owned(),
            heading_index: int(spec, "headingIndex").unwrap_or(0) as isize,
            fraction_through_section: double(spec, "fractionThroughSection").unwrap_or(0.0),
        }
    }

    fn state(&self, state: &DocumentState) -> Value {
        let folded = state.folded_headings.sorted();
        Object::new()
            .with("path", state.path.clone())
            .with("lastSeenHash", state.last_seen_hash.clone())
            .with("reviewBaselineHash", state.review_baseline_hash.clone())
            .with("marks", state.marks.iter().map(|mark| self.persisted(mark)).collect::<Vec<_>>())
            .with("anchor", Self::anchor(&state.anchor))
            .with("mode", state.mode.raw_value())
            .with("zoomLevel", state.zoom_level.raw_value())
            .with("foldedHeadings", folded.into_iter().cloned().collect::<Vec<_>>())
            .with("expandedCodeBlocks", state.expanded_code_blocks.iter().copied().collect::<Vec<_>>())
            .with("collapsedCodeBlocks", state.collapsed_code_blocks.iter().copied().collect::<Vec<_>>())
            .with("lastOpened", self.stamp(state.last_opened))
            .with("sidebarVisible", state.sidebar_visible)
            .with("selectionLocation", state.selection_location)
            .with("selectionLength", state.selection_length)
            .with("splitViewEnabled", state.split_view_enabled)
            .build()
    }

    fn recent(&self, recent: &RecentDocument) -> Value {
        Object::new()
            .with("path", recent.path.clone())
            .with("displayName", recent.display_name.clone())
            .with("firstHeading", recent.first_heading.clone())
            .with("lastOpened", self.stamp(recent.last_opened))
            .with("wordCount", recent.word_count)
            .build()
    }

    fn typography(typography: &TypographyConfig) -> Value {
        Object::new()
            .with("preset", typography.preset.raw_value())
            .with("bodySize", dump_json::double(typography.body_size))
            .with("scaleRatio", dump_json::double(typography.scale_ratio))
            .with("lineHeightMultiple", dump_json::double(typography.line_height_multiple))
            .with("measureCharacters", dump_json::double(typography.measure_characters))
            .with("monoFamily", typography.mono_family.clone())
            .with("monoSizeAdjust", dump_json::double(typography.mono_size_adjust))
            .with("monoLigatures", typography.mono_ligatures)
            .with("opticalMargins", typography.optical_margins)
            .with("mathScale", dump_json::double(typography.math_scale))
            .build()
    }

    fn profile(profile: &ReaderProfile) -> Value {
        Object::new()
            .with("id", profile.id.clone())
            .with("name", profile.name.clone())
            .with("isBuiltIn", profile.is_built_in)
            .with("typographyScale", profile.typography_scale.raw_value())
            .with("scaleValue", dump_json::double(profile.typography_scale.value()))
            .with("scaleTitle", profile.typography_scale.title())
            .with("measureCharacters", dump_json::double(profile.measure_characters))
            .with("chromeDensity", profile.chrome_density.raw_value())
            .with("densityTitle", profile.chrome_density.title())
            .with("motionPreference", profile.motion_preference.raw_value())
            .with("motionTitle", profile.motion_preference.title())
            .build()
    }

    fn review_anchor(anchor: Option<&ReviewAnchor>) -> Value {
        let Some(anchor) = anchor else { return Value::Null };
        Object::new()
            .with("range", json_range(anchor.range))
            .with("selectedText", anchor.selected_text.clone())
            .with("beforeFingerprint", anchor.before_fingerprint.clone())
            .with("afterFingerprint", anchor.after_fingerprint.clone())
            .build()
    }

    fn review(item: Option<&ReviewItem>) -> Value {
        let Some(item) = item else { return Value::Null };
        Object::new()
            .with("id", uuid_string(item.id.as_bytes()))
            .with("kind", item.kind.raw_value())
            .with("title", item.title())
            .with("anchor", Self::review_anchor(Some(&item.anchor)))
            .with("body", item.body.clone())
            .with("replacement", optional_string(item.replacement.as_deref()))
            .with("state", item.state.raw_value())
            .build()
    }

    fn grant(grant: &TrustGrant) -> Value {
        let mut effects: Vec<&str> = grant.effects.iter().map(|effect| effect.raw_value()).collect();
        effects.sort_by(|a, b| utf8_order(a, b));
        Object::new()
            .with("scope", grant.scope.raw_value())
            .with("canonicalPath", grant.canonical_path.clone())
            .with("effects", effects)
            .with("externalURL", optional_string(grant.external_url.as_deref()))
            .build()
    }

    fn entry(entry: Option<&Entry>) -> Value {
        let Some(entry) = entry else { return Value::Null };
        Object::new()
            .with("url", entry.url.as_ref().map_or(Value::Null, |url| Value::String(url.path())))
            .with("offset", entry.offset)
            .with("label", entry.label.clone())
            .build()
    }

    // MARK: Steps

    fn perform(&mut self, op: &str, step: &Step) -> Result<Value, Failure> {
        match op {
            // Files
            "write" => {
                let target = self.path(step, "path")?;
                file_manager::create_directory(&target.deleting_last_path_component(), true).map_err(Failure::Error)?;
                let bytes: Vec<u8> = match string(step, "hex") {
                    Some(hex) => hex
                        .as_bytes()
                        .chunks_exact(2)
                        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                        .collect(),
                    None => self.expand(string(step, "text")).into_bytes(),
                };
                file_manager::write(&bytes, &target).map_err(Failure::Error)?;
                Ok(Value::Null)
            }
            "mkdir" => {
                let target = self.path(step, "path")?;
                file_manager::create_directory(&target, true).map_err(Failure::Error)?;
                Ok(Value::Null)
            }
            "symlink" => {
                let link = self.path(step, "path")?;
                let target = self.path(step, "target")?;
                std::os::unix::fs::symlink(target.path(), link.path())?;
                Ok(Value::Null)
            }
            "remove" => {
                let target = self.path(step, "path")?;
                let _ = file_manager::remove_item(&target);
                Ok(Value::Null)
            }
            "chmod" => {
                let target = self.path(step, "path")?;
                let mode = require(int(step, "mode"), "mode")? as u32;
                let _ = std::fs::set_permissions(target.path(), std::fs::Permissions::from_mode(mode));
                Ok(Value::Null)
            }
            "touch" => {
                let date = require(parse_date(require(string(step, "date"), "date")?), "date")?;
                let target = self.path(step, "path")?;
                let seconds = date.time_interval_since_1970();
                let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(seconds);
                std::fs::File::options().write(true).open(target.path())?.set_modified(when)?;
                Ok(Value::Null)
            }
            "truncate" => {
                let target = self.path(step, "path")?;
                let data = std::fs::read(target.path())?;
                let length = int(step, "length").map_or(data.len() / 2, |length| length as usize).min(data.len());
                file_manager::write(&data[..length], &target).map_err(Failure::Error)?;
                Ok(Value::Null)
            }
            "exists" => {
                let target = self.path(step, "path")?;
                Ok(Value::Bool(file_manager::file_exists(&target.path())))
            }

            // SnapshotStore
            "snapshot.open" => {
                let history = self.path(step, "history")?;
                self.snapshot_stores.insert(name(step, "store"), SnapshotStore::new(history.clone()));
                self.history_directories.insert(name(step, "store"), history);
                Ok(Value::Null)
            }
            "snapshot.writeIndex" => {
                let history = require(self.history_directories.get(&name(step, "store")).cloned(), "snapshot store")?;
                let document = self.path(step, "doc")?;
                let file = history
                    .appending_path_component_is_directory("index", true)
                    .appending_path_component(&(SnapshotStore::document_key(&document) + ".json"));
                Self::write_text(&self.expand(string(step, "text")), &file)?;
                Ok(Value::Null)
            }
            "snapshot.touchObject" => {
                let history = require(self.history_directories.get(&name(step, "store")).cloned(), "snapshot store")?;
                let hash = require(string(step, "hash"), "hash")?;
                let file = history
                    .appending_path_component_is_directory("objects", true)
                    .appending_path_component_is_directory(upleft_swift_text::prefix(hash, 2), true)
                    .appending_path_component(hash);
                let date = require(parse_date(string(step, "date").unwrap_or("")), "date")?;
                let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs_f64(date.time_interval_since_1970());
                std::fs::File::options().write(true).open(file.path())?.set_modified(when)?;
                Ok(Value::Null)
            }
            "snapshot.limits" => {
                let store = self.snapshot(step)?;
                if let Some(age) = double(step, "maximumAge") {
                    store.set_maximum_age(age);
                }
                if let Some(bytes) = int(step, "maximumBytes") {
                    store.set_maximum_bytes(bytes as isize);
                }
                if let Some(per_document) = int(step, "maximumBytesPerDocument") {
                    store.set_maximum_bytes_per_document(per_document as isize);
                }
                Ok(Object::new()
                    .with("maximumAge", dump_json::double(store.maximum_age()))
                    .with("maximumBytes", store.maximum_bytes())
                    .with("maximumBytesPerDocument", store.maximum_bytes_per_document())
                    .build())
            }
            "snapshot.record" => {
                let store = self.snapshot(step)?.clone();
                let kind = require(SnapshotKind::from_raw_value(string(step, "kind").unwrap_or("external")), "kind")?;
                let document = self.path(step, "doc")?;
                let record = store.record(&text_field(step, "text"), &document, kind);
                Ok(self.record(record.as_ref()))
            }
            "snapshot.wait" => {
                self.snapshot(step)?.wait_for_pending_writes();
                Ok(Value::Null)
            }
            "snapshot.prune" => {
                self.snapshot(step)?.prune_one_generation_for_testing();
                Ok(Value::Null)
            }
            "snapshot.versions" => {
                let store = self.snapshot(step)?.clone();
                let document = self.path(step, "doc")?;
                Ok(Value::Array(store.versions(&document).iter().map(|record| self.record(Some(record))).collect()))
            }
            "snapshot.content" => {
                let store = self.snapshot(step)?.clone();
                let hash = if string(step, "text").is_some() || string(step, "textHex").is_some() {
                    SnapshotStore::hash(&text_field(step, "text"))
                } else if let Some(explicit) = string(step, "hash") {
                    explicit.to_owned()
                } else {
                    let document = self.path(step, "doc")?;
                    let versions = store.versions(&document);
                    let index = int(step, "version").unwrap_or(0) as usize;
                    let Some(version) = versions.get(index) else { return Ok(Value::Null) };
                    version.hash.clone()
                };
                Ok(Object::new()
                    .with("content", Self::content(&store.content_for_hash(&hash)))
                    .with("text", optional_string(store.text_for_hash(&hash).as_deref()))
                    .build())
            }
            "snapshot.forget" => {
                let store = self.snapshot(step)?.clone();
                let document = self.path(step, "doc")?;
                store.forget(&document);
                Ok(Value::Null)
            }
            "snapshot.totalBytes" => Ok(Value::from(self.snapshot(step)?.total_bytes())),
            "snapshot.inventory" => {
                let mut inventory = self.snapshot(step)?.object_inventory();
                inventory.sort_by(|a, b| utf8_order(&a.hash, &b.hash));
                Ok(Value::Array(
                    inventory.iter().map(|object| Object::new().with("hash", object.hash.clone()).with("size", object.size).build()).collect(),
                ))
            }
            "snapshot.hash" => Ok(Value::String(SnapshotStore::hash(string(step, "text").unwrap_or("")))),
            "snapshot.documentKey" => {
                let document = self.path(step, "doc")?;
                Ok(Value::String(SnapshotStore::document_key(&document)))
            }

            // DocumentStateStore
            "state.open" => {
                let support = self.path(step, "support")?;
                self.state_stores.insert(name(step, "store"), DocumentStateStore::new(support.clone()));
                self.support_directories.insert(name(step, "store"), support);
                Ok(Value::Null)
            }
            "state.writeFile" => {
                let support = require(self.support_directories.get(&name(step, "store")).cloned(), "state store")?;
                let document = self.path(step, "doc")?;
                let file = support
                    .appending_path_component_is_directory("state", true)
                    .appending_path_component(&(SnapshotStore::document_key(&document) + ".json"));
                Self::write_text(&self.expand(string(step, "text")), &file)?;
                Ok(Value::Null)
            }
            "state.get" => {
                let document = self.path(step, "doc")?;
                let state = self.state_store(step)?.state(&document);
                Ok(self.state(&state))
            }
            "state.save" => {
                let document = self.path(step, "doc")?;
                let mut value = if string(step, "base").unwrap_or("current") == "new" {
                    DocumentState::new(&document.path())
                } else {
                    self.state_store(step)?.state(&document)
                };
                let empty = Value::Object(Map::new());
                self.apply_state(step.get("set").unwrap_or(&empty), &mut value)?;
                self.state_store(step)?.save(&value, &document);
                Ok(self.state(&value))
            }
            "state.decode" => match DocumentState::decoded(self.expand(string(step, "json")).as_bytes()) {
                Ok(state) => Ok(self.state(&state)),
                Err(error) => Ok(error_code(error.code())),
            },
            "state.recents" => {
                let limit = int(step, "limit").unwrap_or(30) as usize;
                let recents = self.state_store(step)?.recents(limit);
                Ok(Value::Array(recents.iter().map(|recent| self.recent(recent)).collect()))
            }
            "state.noteOpened" => {
                let document = self.path(step, "doc")?;
                let parsed = MarkdownParser::parse(string(step, "text").unwrap_or(""));
                self.state_store(step)?.note_opened(&document, &parsed);
                Ok(Value::Null)
            }
            "state.removeRecent" => {
                let target = self.path(step, "path")?;
                self.state_store(step)?.remove_recent(&target.path());
                Ok(Value::Null)
            }
            "state.clearRecents" => {
                self.state_store(step)?.clear_recents();
                Ok(Value::Null)
            }
            "state.canonicalPath" => {
                let target = self.path(step, "path")?;
                Ok(Value::String(DocumentStateStore::canonical_path(&target.path())))
            }

            // ScrollAnchoring
            "anchor.make" => {
                let document = MarkdownParser::parse(string(step, "text").unwrap_or(""));
                Ok(Self::anchor(&ScrollAnchoring::anchor(int(step, "offset").unwrap_or(0) as isize, &document)))
            }
            "anchor.offset" => {
                let document = MarkdownParser::parse(string(step, "text").unwrap_or(""));
                Ok(Value::from(ScrollAnchoring::offset(&Self::scroll_anchor(step.get("anchor")), &document)))
            }

            // ChangeTracker
            "tracker.new" => {
                let tracker = ChangeTracker::new();
                if let Some(lifetime) = double(step, "lifetime") {
                    tracker.set_lifetime(lifetime);
                }
                if let Some(dwell) = double(step, "dwell") {
                    tracker.set_dwell(dwell);
                }
                self.trackers.insert(name(step, "tracker"), tracker);
                Ok(Value::Null)
            }
            "tracker.apply" => {
                let replacing = boolean(step, "replacing").unwrap_or(true);
                let old = string(step, "old").unwrap_or("");
                let new = string(step, "new").unwrap_or("");
                let hunks = match step.get("hunks").and_then(Value::as_array) {
                    Some(specs) => specs
                        .iter()
                        .map(|spec| {
                            Ok(ChangeHunk::new(
                                require(change_kind_from_raw_value(string(spec, "kind").unwrap_or("")), "kind")?,
                                range(spec.get("new")),
                                range(spec.get("old")),
                                spec.get("words")
                                    .and_then(Value::as_array)
                                    .map(|words| words.iter().map(|word| range(Some(word))).collect())
                                    .unwrap_or_default(),
                            ))
                        })
                        .collect::<Result<Vec<_>, Failure>>()?,
                    None => TextDiff::hunks(old, new),
                };
                let tracker = self.tracker(step)?;
                tracker.apply(&hunks, new, old, replacing);
                Ok(self.tracker_summary(tracker))
            }
            "tracker.marks" => Ok(self.tracker_summary(self.tracker(step)?)),
            "tracker.adjust" => {
                let tracker = self.tracker(step)?;
                tracker.adjust(range(step.get("range")), int(step, "delta").unwrap_or(0) as isize);
                Ok(self.tracker_summary(tracker))
            }
            "tracker.next" => {
                let found = self.tracker(step)?.next(int(step, "offset").unwrap_or(0) as isize);
                Ok(self.mark(found.as_ref()))
            }
            "tracker.previous" => {
                let found = self.tracker(step)?.previous(int(step, "offset").unwrap_or(0) as isize);
                Ok(self.mark(found.as_ref()))
            }
            "tracker.markAt" => {
                let found = self.tracker(step)?.mark_at(int(step, "offset").unwrap_or(0) as isize);
                Ok(self.mark(found.as_ref()))
            }
            "tracker.visit" => {
                let tracker = self.tracker(step)?;
                let index = int(step, "index").unwrap_or(0) as usize;
                let marks = tracker.marks();
                if index < marks.len() {
                    tracker.mark_visited(marks[index].id);
                }
                Ok(self.tracker_summary(tracker))
            }
            "tracker.noteVisible" => {
                let now = require(parse_date(string(step, "now").unwrap_or("")), "now")?;
                let tracker = self.tracker(step)?;
                tracker.note_visible_range(range(step.get("range")), now);
                Ok(self.tracker_summary(tracker))
            }
            "tracker.restore" => {
                let now = require(parse_date(string(step, "now").unwrap_or("")), "now")?;
                let marks = self.persisted_marks(step)?;
                let tracker = self.tracker(step)?;
                tracker.restore(&marks, int(step, "textLength").unwrap_or(0) as isize, now);
                Ok(self.tracker_summary(tracker))
            }
            "tracker.merge" => {
                let marks = self.persisted_marks(step)?;
                let tracker = self.tracker(step)?;
                tracker.merge(&marks);
                Ok(self.tracker_summary(tracker))
            }
            "tracker.dropExpired" => {
                let now = require(parse_date(string(step, "now").unwrap_or("")), "now")?;
                let tracker = self.tracker(step)?;
                let dropped = tracker.drop_expired_marks(now);
                Ok(Object::new().with("dropped", dropped).with("tracker", self.tracker_summary(tracker)).build())
            }
            "tracker.clear" => {
                let tracker = self.tracker(step)?;
                let reviewed = Rc::new(Cell::new(false));
                let flag = Rc::clone(&reviewed);
                tracker.set_on_reviewed(Some(Box::new(move || flag.set(true))));
                tracker.clear();
                tracker.set_on_reviewed(None);
                Ok(Object::new().with("reviewed", reviewed.get()).with("tracker", self.tracker_summary(tracker)).build())
            }
            "tracker.reset" => {
                let tracker = self.tracker(step)?;
                tracker.reset();
                Ok(self.tracker_summary(tracker))
            }
            "tracker.persisted" => {
                let marks = self.tracker(step)?.persisted_marks();
                Ok(Value::Array(marks.iter().map(|mark| self.persisted(mark)).collect()))
            }
            "tracker.ranges" => {
                let tracker = self.tracker(step)?;
                let mut object = Object::new();
                for kind in [ChangeKind::Inserted, ChangeKind::Deleted, ChangeKind::Modified] {
                    object = object.with(kind.raw_value(), ranges(&tracker.ranges(kind)));
                }
                Ok(object.build())
            }

            // Preferences.Values
            "prefs.defaults" => self.preferences(Values::default(), step),
            "prefs.decode" => match Values::decoded(self.expand(string(step, "json")).as_bytes()) {
                Ok(values) => self.preferences(values, step),
                Err(error) => Ok(error_code(error.code())),
            },

            // ReaderProfiles
            "profiles.builtIns" => Ok(Value::Array(ReaderProfile::built_ins().iter().map(Self::profile).collect())),
            "profiles.make" => Ok(Self::profile(&Self::reader_profile(step)?)),
            "profiles.custom" => {
                let name = string(step, "name").unwrap_or("");
                Ok(Self::profile(&match string(step, "id") {
                    Some(id) => ReaderProfile::custom_with_id(id, name),
                    None => ReaderProfile::custom(name),
                }))
            }
            "profiles.save" => {
                let target = self.path(step, "path")?;
                let profiles = step
                    .get("profiles")
                    .and_then(Value::as_array)
                    .map(|specs| specs.iter().map(Self::reader_profile).collect::<Result<Vec<_>, _>>())
                    .transpose()?
                    .unwrap_or_default();
                JSONReaderProfileStore::new(target).save_custom_profiles(&profiles);
                Ok(Value::Null)
            }
            "profiles.load" => {
                let target = self.path(step, "path")?;
                Ok(Value::Array(JSONReaderProfileStore::new(target).load_custom_profiles().iter().map(Self::profile).collect()))
            }

            // Review sidecars
            "review.anchor" => Ok(Self::review_anchor(
                ReviewAnchorResolver::make_anchor(
                    string(step, "text").unwrap_or(""),
                    range(step.get("range")),
                    int(step, "contextLength").unwrap_or(48) as isize,
                )
                .as_ref(),
            )),
            "review.resolve" => {
                let anchor = Self::anchor_spec(step.get("anchor"))?;
                let resolution = ReviewAnchorResolver::resolve(
                    &anchor,
                    string(step, "text").unwrap_or(""),
                    int(step, "contextLength").unwrap_or(48) as isize,
                );
                Ok(Object::new()
                    .with("status", resolution.status.raw_value())
                    .with("range", resolution.range.map_or(Value::Null, json_range))
                    .build())
            }
            "review.fingerprint" => Ok(Value::String(ReviewAnchorResolver::fingerprint(string(step, "text").unwrap_or("")))),
            "review.make" => {
                let item = ReviewSidecarEngine::make_review(
                    require(ReviewKind::from_raw_value(string(step, "kind").unwrap_or("")), "kind")?,
                    string(step, "text").unwrap_or(""),
                    range(step.get("range")),
                    string(step, "body").unwrap_or(""),
                    string(step, "replacement"),
                );
                if let (Some(item), Some(key)) = (&item, string(step, "name")) {
                    self.reviews.insert(key.to_owned(), item.clone());
                }
                Ok(Self::review(item.as_ref()))
            }
            "review.item" => {
                let kind = require(ReviewKind::from_raw_value(string(step, "kind").unwrap_or("")), "kind")?;
                let state = require(ReviewState::from_raw_value(string(step, "state").unwrap_or("open")), "state")?;
                let anchor = Self::anchor_spec(step.get("anchor"))?;
                let body = string(step, "body").unwrap_or("");
                let replacement = string(step, "replacement");
                let item = match string(step, "id") {
                    Some(id) => ReviewItem::with_id(
                        Uuid::from_bytes(require(parse_uuid(id), "id")?),
                        kind,
                        anchor,
                        body,
                        replacement,
                        state,
                    ),
                    None => ReviewItem::new(kind, anchor, body, replacement, state),
                };
                self.reviews.insert(require(string(step, "name"), "name")?.to_owned(), item.clone());
                Ok(Self::review(Some(&item)))
            }
            "review.setState" => {
                let key = require(string(step, "review"), "review")?.to_owned();
                let state = require(ReviewState::from_raw_value(string(step, "state").unwrap_or("")), "state")?;
                let item = require(self.reviews.get_mut(&key), "review")?;
                item.state = state;
                Ok(Self::review(Some(item)))
            }
            "review.apply" => {
                let item = require(self.reviews.get(require(string(step, "review"), "review")?), "review")?;
                Ok(match ReviewSidecarEngine::apply_suggestion(item, string(step, "text").unwrap_or("")) {
                    ReviewApplyResult::Applied(edit) => Object::new()
                        .with(
                            "applied",
                            Object::new()
                                .with("range", json_range(edit.range))
                                .with("replacement", edit.replacement)
                                .with("summary", edit.summary)
                                .build(),
                        )
                        .build(),
                    ReviewApplyResult::Stale(status) => Object::new().with("stale", status.raw_value()).build(),
                })
            }
            "review.save" => {
                let names: Vec<&str> =
                    step.get("reviews").and_then(Value::as_array).map(|names| names.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
                let reviews = names
                    .iter()
                    .map(|key| require(self.reviews.get(*key).cloned(), &format!("review {key}")))
                    .collect::<Result<Vec<_>, _>>()?;
                let sidecar = ReviewSidecar { version: int(step, "version").unwrap_or(1), reviews };
                let document = self.path(step, "doc")?;
                Ok(match LocalReviewSidecarStore::new().save(&sidecar, &document) {
                    Ok(()) => Value::String("saved".into()),
                    Err(error) => error_code(error.code().unwrap_or(0)),
                })
            }
            "review.load" => {
                let document = self.path(step, "doc")?;
                Ok(match LocalReviewSidecarStore::new().load(&document) {
                    Ok(sidecar) => Object::new()
                        .with("version", sidecar.version)
                        .with("reviews", sidecar.reviews.iter().map(|item| Self::review(Some(item))).collect::<Vec<_>>())
                        .build(),
                    Err(error) => error_code(error.code().unwrap_or(0)),
                })
            }
            "review.sidecarURL" => {
                let document = self.path(step, "doc")?;
                Ok(Value::String(LocalReviewSidecarStore::sidecar_url(&document).path()))
            }
            "review.critic" => Ok(ranges(&ReviewSidecarEngine::critic_markup_ranges(string(step, "text").unwrap_or("")))),

            // Trust
            "trust.open" => {
                let key = name(step, "store");
                let handle = if key == "shared" {
                    if !self.has_support_directory {
                        return error("formats: TrustStore.shared needs \"supportDirectory\"");
                    }
                    TrustHandle::Shared(TrustStore::shared())
                } else {
                    let grants = self.grant_specs(step.get("grants"))?;
                    TrustHandle::Owned(TrustStore::new(Box::new(InMemoryTrustStorePersistence::new(grants))))
                };
                self.trust_stores.insert(key, handle);
                Ok(Value::Array(self.trust_store(step)?.grants().iter().map(Self::grant).collect()))
            }
            "trust.grant" => {
                let effects = Self::effects(step.get("effects"))?;
                let scope = require(TrustScope::from_raw_value(string(step, "scope").unwrap_or("")), "scope")?;
                let target = self.path(step, "path")?;
                let store = self.trust_store(step)?;
                let granted = store.grant(scope, &target, effects, string(step, "externalURL"));
                Ok(Object::new()
                    .with("granted", granted)
                    .with("grants", store.grants().iter().map(Self::grant).collect::<Vec<_>>())
                    .build())
            }
            "trust.revoke" => {
                let scope = require(TrustScope::from_raw_value(string(step, "scope").unwrap_or("")), "scope")?;
                let target = self.path(step, "path")?;
                let store = self.trust_store(step)?;
                let revoked = store.revoke(scope, &target);
                Ok(Object::new()
                    .with("revoked", revoked)
                    .with("grants", store.grants().iter().map(Self::grant).collect::<Vec<_>>())
                    .build())
            }
            "trust.state" => {
                let document = string(step, "doc").map(str::to_owned).map(|document| self.url(&document));
                Ok(Value::String(self.trust_store(step)?.state(document.as_ref()).raw_value().to_owned()))
            }
            "trust.decide" => {
                let effect = require(TrustEffect::from_raw_value(string(step, "effect").unwrap_or("")), "effect")?;
                let canonical = string(step, "canonicalPath").map(str::to_owned).map(|path| self.url(&path).path());
                let document = string(step, "doc").map(str::to_owned).map(|document| self.url(&document));
                let request = TrustRequest::new(
                    effect,
                    TrustTarget::new(string(step, "displayName").unwrap_or(""), canonical.as_deref(), string(step, "externalURL")),
                    document.as_ref(),
                );
                let trust_state =
                    require(DocumentTrustState::from_raw_value(string(step, "state").unwrap_or("standard")), "state")?;
                let policy = if step.get("grants").is_some_and(Value::is_array) {
                    DocumentTrust::new(trust_state, self.grant_specs(step.get("grants"))?)
                } else {
                    self.trust_store(step)?.policy(trust_state)
                };
                Ok(Object::new()
                    .with("documentPath", optional_string(request.document_path.as_deref()))
                    .with("decision", policy.decision(&request).raw_value())
                    .build())
            }
            "trust.canonical" => {
                let target = self.path(step, "path")?;
                Ok(DocumentTrust::canonical_file_path(&target).map_or(Value::Null, |url| Value::String(url.path())))
            }
            "trust.isWithin" => {
                let child = self.path(step, "child")?;
                let root = self.path(step, "root")?;
                Ok(Value::Bool(DocumentTrust::is_within(&child, &root)))
            }
            "trust.folderScope" => {
                let target = self.path(step, "path")?;
                Ok(Value::String(DocumentTrust::folder_scope(&target, boolean(step, "isDirectory").unwrap_or(false)).path()))
            }
            "trust.decodeGrants" => {
                match decodable::parse(self.expand(string(step, "json")).as_bytes()).and_then(|value| value.array_of(TrustGrant::decode)) {
                    Ok(grants) => Ok(Value::Array(grants.iter().map(Self::grant).collect())),
                    Err(error) => Ok(error_code(error.code())),
                }
            }
            "trust.titles" => Ok(Object::new()
                .with(
                    "states",
                    DocumentTrustState::ALL_CASES.iter().map(|state| Value::from(vec![state.raw_value(), state.title()])).collect::<Vec<_>>(),
                )
                .with(
                    "effects",
                    TrustEffect::ALL_CASES.iter().map(|effect| Value::from(vec![effect.raw_value(), effect.title()])).collect::<Vec<_>>(),
                )
                .with("scopes", TrustScope::ALL_CASES.iter().map(|scope| scope.raw_value()).collect::<Vec<_>>())
                .build()),

            // PathResolver and ExternalEditor
            "resolver.open" => {
                let document = string(step, "doc").map(str::to_owned).map(|document| self.url(&document));
                self.resolvers.insert(name(step, "resolver"), PathResolver::new(document.as_ref()));
                Ok(Value::Null)
            }
            "resolver.resolve" => {
                let resolver = require(self.resolvers.get(&name(step, "resolver")), "resolver")?;
                let token = PathToken::new(self.expand(string(step, "raw")), int(step, "line").map(|line| line as isize), None);
                let resolution = resolver.resolve(&token);
                Ok(Object::new()
                    .with("url", resolution.url.as_ref().map_or(Value::Null, |url| Value::String(url.path())))
                    .with("urlIsDirectory", resolution.url.as_ref().map_or(Value::Null, |url| Value::Bool(url.has_directory_path())))
                    .with("exists", resolution.exists)
                    .with("isDirectory", resolution.is_directory)
                    .with("line", resolution.line.map_or(Value::Null, Value::from))
                    .build())
            }
            "resolver.invalidate" => {
                require(self.resolvers.get(&name(step, "resolver")), "resolver")?.invalidate();
                Ok(Value::Null)
            }
            "resolver.gitRoot" => {
                let target = self.path(step, "path")?;
                Ok(PathResolver::find_git_root(&target).map_or(Value::Null, |url| Value::String(url.path())))
            }
            "editor.url" => {
                let editor = require(ExternalEditor::from_raw_value(string(step, "editor").unwrap_or("")), "editor")?;
                let target = self.path(step, "path")?;
                Ok(editor
                    .url(&target, int(step, "line").map(|line| line as isize))
                    .and_then(|url| url.absoluteString())
                    .map_or(Value::Null, |text| Value::String(text.to_string())))
            }
            "editor.all" => Ok(Value::Array(
                ExternalEditor::ALL_CASES
                    .iter()
                    .map(|editor| {
                        Object::new()
                            .with("raw", editor.raw_value())
                            .with("title", editor.title())
                            .with("bundleIdentifier", optional_string(editor.bundle_identifier()))
                            .build()
                    })
                    .collect(),
            )),

            // JumpHistory
            "jump.record" => {
                let from = self.entry_spec(step.get("from"));
                let to = require(self.entry_spec(step.get("to")), "to")?;
                let history = self.histories.entry(name(step, "history")).or_default();
                history.record(from, to);
                Ok(Self::jump_state(history))
            }
            "jump.back" => {
                let history = self.histories.entry(name(step, "history")).or_default();
                let result = history.go_back();
                Ok(Object::new().with("entry", Self::entry(result.as_ref())).with("state", Self::jump_state(history)).build())
            }
            "jump.forward" => {
                let history = self.histories.entry(name(step, "history")).or_default();
                let result = history.go_forward();
                Ok(Object::new().with("entry", Self::entry(result.as_ref())).with("state", Self::jump_state(history)).build())
            }
            "jump.clear" => {
                let history = self.histories.entry(name(step, "history")).or_default();
                history.clear();
                Ok(Self::jump_state(history))
            }

            _ => error(format!("formats: unknown op {op}")),
        }
    }

    // MARK: Step helpers

    fn snapshot(&self, step: &Step) -> Result<&SnapshotStore, Failure> {
        let key = name(step, "store");
        require(self.snapshot_stores.get(&key), &format!("snapshot store {key}"))
    }

    fn state_store(&self, step: &Step) -> Result<&DocumentStateStore, Failure> {
        let key = name(step, "store");
        require(self.state_stores.get(&key), &format!("state store {key}"))
    }

    fn tracker(&self, step: &Step) -> Result<&ChangeTracker, Failure> {
        require(self.trackers.get(&name(step, "tracker")), "tracker")
    }

    fn trust_store(&self, step: &Step) -> Result<&TrustStore, Failure> {
        let key = name(step, "store");
        require(self.trust_stores.get(&key), &format!("trust store {key}")).map(TrustHandle::store)
    }

    fn jump_state(history: &JumpHistory) -> Value {
        Object::new()
            .with("entries", history.entries().iter().map(|entry| Self::entry(Some(entry))).collect::<Vec<_>>())
            .with("canGoBack", history.can_go_back())
            .with("canGoForward", history.can_go_forward())
            .with("current", Self::entry(history.current()))
            .build()
    }

    fn entry_spec(&mut self, value: Option<&Value>) -> Option<Entry> {
        let spec = value.filter(|value| value.is_object())?;
        let url = string(spec, "url").map(str::to_owned).map(|path| self.url(&path));
        Some(Entry::new(url, int(spec, "offset").unwrap_or(0) as isize, string(spec, "label").unwrap_or("")))
    }

    fn tracker_summary(&self, tracker: &ChangeTracker) -> Value {
        Object::new()
            .with("count", tracker.count())
            .with("unread", tracker.unread_count())
            .with("decorated", tracker.decorated_marks().len())
            .with("marks", tracker.marks().iter().map(|mark| self.mark(Some(mark))).collect::<Vec<_>>())
            .build()
    }

    fn persisted_marks(&mut self, step: &Step) -> Result<Vec<PersistedMark>, Failure> {
        if let Some(source) = string(step, "from") {
            return Ok(require(self.trackers.get(source), &format!("tracker {source}"))?.persisted_marks());
        }
        if let Some(store_name) = string(step, "fromState") {
            let document = self.path(step, "doc")?;
            return Ok(require(self.state_stores.get(store_name), "state store")?.state(&document).marks);
        }
        step.get("marks")
            .and_then(Value::as_array)
            .map(|specs| specs.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|spec| {
                Ok(PersistedMark {
                    id: Uuid::from_bytes(require(parse_uuid(string(spec, "id").unwrap_or("")), "mark id")?),
                    kind: string(spec, "kind").unwrap_or("").to_owned(),
                    range: PersistedRange::new(range(spec.get("range"))),
                    word_ranges: spec
                        .get("wordRanges")
                        .and_then(Value::as_array)
                        .map(|words| words.iter().map(|word| PersistedRange::new(range(Some(word)))).collect())
                        .unwrap_or_default(),
                    deleted_text: string(spec, "deletedText").unwrap_or("").to_owned(),
                    created: require(parse_date(string(spec, "created").unwrap_or("")), "created")?,
                    visited: boolean(spec, "visited").unwrap_or(false),
                })
            })
            .collect()
    }

    fn apply_state(&mut self, fields: &Value, state: &mut DocumentState) -> Result<(), Failure> {
        let Some(members) = fields.as_object() else { return Ok(()) };
        let mut keys: Vec<&String> = members.keys().collect();
        keys.sort_by(|a, b| utf8_order(a, b));
        for key in keys {
            let value = &members[key];
            match key.as_str() {
                "lastSeenHash" => state.last_seen_hash = value.as_str().unwrap_or("").to_owned(),
                "reviewBaselineHash" => state.review_baseline_hash = value.as_str().unwrap_or("").to_owned(),
                "anchor" => state.anchor = Self::scroll_anchor(Some(value)),
                "mode" => state.mode = require(RenderMode::from_raw_value(value.as_str().unwrap_or("")), "mode")?,
                "zoomLevel" => {
                    state.zoom_level = require(ZoomLevel::from_raw_value(value.as_i64().unwrap_or(0) as isize), "zoomLevel")?
                }
                "foldedHeadings" => {
                    state.folded_headings = value
                        .as_array()
                        .map(|values| values.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                        .unwrap_or_default()
                }
                "expandedCodeBlocks" | "collapsedCodeBlocks" => {
                    let offsets: BTreeSet<isize> = value
                        .as_array()
                        .map(|values| values.iter().filter_map(Value::as_i64).map(|offset| offset as isize).collect())
                        .unwrap_or_default();
                    if key == "expandedCodeBlocks" {
                        state.expanded_code_blocks = offsets;
                    } else {
                        state.collapsed_code_blocks = offsets;
                    }
                }
                "lastOpened" => state.last_opened = require(parse_date(value.as_str().unwrap_or("")), "lastOpened")?,
                "sidebarVisible" => state.sidebar_visible = value.as_bool().unwrap_or(false),
                "selectionLocation" => state.selection_location = value.as_i64().unwrap_or(0) as isize,
                "selectionLength" => state.selection_length = value.as_i64().unwrap_or(0) as isize,
                "splitViewEnabled" => state.split_view_enabled = value.as_bool().unwrap_or(false),
                "marksFromTracker" => {
                    state.marks = require(self.trackers.get(value.as_str().unwrap_or("")), "tracker")?.persisted_marks()
                }
                other => return error(format!("formats: unknown state field {other}")),
            }
        }
        Ok(())
    }

    fn preferences(&mut self, initial: Values, step: &Step) -> Result<Value, Failure> {
        let mut values = initial;
        if let Some(theme) = step.get("selectTheme").filter(|theme| theme.is_object()) {
            let slot = if string(theme, "slot") == Some("dark") { ThemePreferenceSlot::Dark } else { ThemePreferenceSlot::Light };
            values.select_theme(string(theme, "name").unwrap_or(""), slot);
        }
        let data = values.persisted_data();
        if let Some(write) = string(step, "write") {
            let target = self.url(write);
            file_manager::create_directory(&target.deleting_last_path_component(), true).map_err(Failure::Error)?;
            file_manager::write_atomic(&data, &target).map_err(Failure::Error)?;
        }
        let round_trip = Values::decoded(&data).map_err(|error| Failure::Error(error.to_string()))?;
        Ok(Object::new()
            .with("encoded", lines(&data))
            .with("effectiveTypography", Self::typography(&effective_typography(&values)))
            .with("largeFileThresholdBytes", values.large_file_threshold_megabytes * 1024 * 1024)
            .with("roundTripEqual", round_trip == values)
            .build())
    }

    fn reader_profile(spec: &Value) -> Result<ReaderProfile, Failure> {
        Ok(ReaderProfile::with(
            string(spec, "id").unwrap_or(""),
            string(spec, "name").unwrap_or(""),
            boolean(spec, "isBuiltIn").unwrap_or(false),
            require(ReaderTypographyScale::from_raw_value(string(spec, "typographyScale").unwrap_or("standard")), "typographyScale")?,
            double(spec, "measureCharacters").unwrap_or(70.0),
            require(ReaderChromeDensity::from_raw_value(string(spec, "chromeDensity").unwrap_or("comfortable")), "chromeDensity")?,
            require(
                ReaderMotionPreference::from_raw_value(string(spec, "motionPreference").unwrap_or("follow-system")),
                "motionPreference",
            )?,
        ))
    }

    fn anchor_spec(value: Option<&Value>) -> Result<ReviewAnchor, Failure> {
        let spec = require(value.filter(|value| value.is_object()), "anchor")?;
        if let Some(text) = string(spec, "text") {
            return require(ReviewAnchorResolver::make_anchor(text, range(spec.get("range")), 48), "anchor");
        }
        Ok(ReviewAnchor {
            range: range(spec.get("range")),
            selected_text: string(spec, "selectedText").unwrap_or("").to_owned(),
            before_fingerprint: string(spec, "beforeFingerprint").unwrap_or("").to_owned(),
            after_fingerprint: string(spec, "afterFingerprint").unwrap_or("").to_owned(),
        })
    }

    fn effects(value: Option<&Value>) -> Result<Vec<TrustEffect>, Failure> {
        value
            .and_then(Value::as_array)
            .map(|values| values.as_slice())
            .unwrap_or_default()
            .iter()
            .map(|effect| require(TrustEffect::from_raw_value(effect.as_str().unwrap_or("")), "effect"))
            .collect()
    }

    fn grant_specs(&mut self, value: Option<&Value>) -> Result<Vec<TrustGrant>, Failure> {
        let specs = value.and_then(Value::as_array).cloned().unwrap_or_default();
        specs
            .iter()
            .map(|spec| {
                let scope = require(TrustScope::from_raw_value(string(spec, "scope").unwrap_or("")), "scope")?;
                let path = self.url(string(spec, "path").unwrap_or("")).path();
                Ok(TrustGrant::new(scope, &path, Self::effects(spec.get("effects"))?, string(spec, "externalURL")))
            })
            .collect()
    }

    // MARK: Files

    /// Each document key the script can have produced, with its label.
    fn key_labels(&mut self) -> Vec<(String, String)> {
        let mut mentioned: Vec<String> = self.mentioned_paths.iter().cloned().collect();
        mentioned.sort_by(|a, b| utf8_order(a, b));
        mentioned
            .iter()
            .map(|relative| (SnapshotStore::document_key(&self.url(relative)), format!("<key:{relative}>")))
            .collect()
    }

    fn dump_files(&mut self) -> Vec<Value> {
        fn walk(root: &str, relative: &str, out: &mut Vec<String>) {
            let directory = if relative.is_empty() { root.to_owned() } else { format!("{root}/{relative}") };
            let Ok(entries) = std::fs::read_dir(&directory) else {
                return;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let child = if relative.is_empty() { name } else { format!("{relative}/{name}") };
                out.push(child.clone());
                if std::fs::symlink_metadata(format!("{root}/{child}")).is_ok_and(|metadata| metadata.is_dir()) {
                    walk(root, &child, out);
                }
            }
        }
        let mut relatives = Vec::new();
        walk(&self.root_path, "", &mut relatives);
        // Sorted as the dump will spell them: a document key depends on the
        // sandbox path, so the raw names sort differently from run to run.
        let labels = self.key_labels();
        let spelled = |relative: &str| labels.iter().fold(relative.to_owned(), |text, (key, label)| text.replace(key.as_str(), label));
        relatives.sort_by(|a, b| utf8_order(&spelled(a), &spelled(b)));
        let mut out = Vec::new();
        for relative in relatives {
            let full = format!("{}/{relative}", self.root_path);
            let Ok(metadata) = std::fs::symlink_metadata(&full) else { continue };
            if metadata.is_dir() {
                out.push(Object::new().with("path", relative).with("kind", "directory").build());
            } else if metadata.file_type().is_symlink() {
                let destination = std::fs::read_link(&full).map(|path| path.to_string_lossy().into_owned()).unwrap_or_default();
                out.push(Object::new().with("path", relative).with("kind", "symlink").with("target", destination).build());
            } else {
                let data = std::fs::read(&full).unwrap_or_default();
                let content = self.file_content(&relative, &data);
                out.push(Object::new().with("path", relative).with("kind", "file").with("content", content).build());
            }
        }
        out
    }

    fn is_parsed(&self, relative: &str) -> bool {
        if self.parsed_paths.iter().any(|path| path == relative) {
            return true;
        }
        let components: Vec<&str> = relative.split('/').collect();
        if components.last() == Some(&"recents.json") {
            return true;
        }
        if !relative.ends_with(".json") || components.len() < 2 {
            return false;
        }
        let parent = components[components.len() - 2];
        if parent == "state" {
            return true;
        }
        parent == "index" && components.len() >= 3 && components[components.len() - 3] == "history"
    }

    fn file_content(&self, relative: &str, data: &[u8]) -> Value {
        let components: Vec<&str> = relative.split('/').collect();
        if let Some(objects) = components.iter().position(|component| *component == "objects")
            && objects > 0
            && components[objects - 1] == "history"
        {
            return hex(data);
        }
        if self.is_parsed(relative)
            && let Ok(object) = json_serialization::json_object(data, ReadingOptions { fragments_allowed: true })
        {
            let normalized = self.normalize_parsed(object, None);
            let options = WritingOptions {
                pretty_printed: true,
                sorted_keys: true,
                fragments_allowed: true,
                without_escaping_slashes: true,
            };
            if let Ok(text) = json_serialization::data(&normalized, options)
                && let Ok(text) = String::from_utf8(text)
            {
                return Object::new().with("json", text.split('\n').map(|line| Value::String(line.to_owned())).collect::<Vec<_>>()).build();
            }
        }
        if components.last() == Some(&"trust.json")
            && !data.starts_with(&[0xEF, 0xBB, 0xBF])
            && let Ok(text) = std::str::from_utf8(data)
        {
            let lines = sort_effect_blocks(text.split('\n').collect());
            return Object::new().with("lines", lines).build();
        }
        lines(data)
    }

    fn normalize_parsed(&self, value: AnyJson, key: Option<&str>) -> AnyJson {
        const TIMESTAMP_KEYS: [&str; 4] = ["date", "created", "lastOpened", "firstSeen"];
        const SET_KEYS: [&str; 3] = ["foldedHeadings", "expandedCodeBlocks", "collapsedCodeBlocks"];
        match value {
            AnyJson::Object(members) => AnyJson::Object(
                members.into_iter().map(|(member, element)| {
                    let normalized = self.normalize_parsed(element, Some(&member));
                    (member, normalized)
                }).collect(),
            ),
            AnyJson::Array(values) => {
                let mut elements: Vec<AnyJson> = values.into_iter().map(|element| self.normalize_parsed(element, None)).collect();
                if key.is_some_and(|key| SET_KEYS.contains(&key)) {
                    elements.sort_by(|a, b| match (a, b) {
                        (AnyJson::String(x), AnyJson::String(y)) => utf8_order(x, y),
                        _ => {
                            let number = |value: &AnyJson| value.number().map(|number| number.doubleValue());
                            match (number(a), number(b)) {
                                (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                                _ => std::cmp::Ordering::Equal,
                            }
                        }
                    });
                }
                AnyJson::Array(elements)
            }
            AnyJson::String(text)
                if key.is_some_and(|key| TIMESTAMP_KEYS.contains(&key))
                    && parse_date(&text).is_some_and(|date| date.time_interval_since(self.run_start).abs() < 2.0 * 86_400.0) =>
            {
                AnyJson::String("<now>".into())
            }
            other => other,
        }
    }

    // MARK: Normalisation

    fn normalize(&mut self, dump: Value) -> Value {
        let keys = self.key_labels();
        let spellings: Vec<String> = [self.root_path.clone(), self.unresolved_root_path.clone()]
            .into_iter()
            .flat_map(|path| {
                let escaped = path.replace('/', "\\/");
                [path, escaped]
            })
            .collect();
        let mut uuids: HashMap<String, String> = HashMap::new();
        let mut rewrite = |text: &str| -> String {
            let mut text = text.to_owned();
            for (key, label) in &keys {
                if text.contains(key.as_str()) {
                    text = text.replace(key.as_str(), label);
                }
            }
            for spelling in &spellings {
                if text.contains(spelling.as_str()) {
                    text = text.replace(spelling.as_str(), "<root>");
                }
            }
            replace_uuids(&text, &mut uuids)
        };
        fn walk(value: Value, rewrite: &mut dyn FnMut(&str) -> String) -> Value {
            match value {
                Value::String(text) => Value::String(rewrite(&text)),
                Value::Array(values) => Value::Array(values.into_iter().map(|value| walk(value, rewrite)).collect()),
                Value::Object(members) => {
                    Value::Object(members.into_iter().map(|(key, value)| (key, walk(value, rewrite))).collect())
                }
                other => other,
            }
        }
        walk(dump, &mut rewrite)
    }
}

/// Sorts the element lines of each `"effects" : [` block, re-placing the
/// commas: Swift writes a `Set` in per-process hash order.
fn sort_effect_blocks(lines: Vec<&str>) -> Vec<String> {
    let mut out = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        out.push(line.to_owned());
        index += 1;
        if !line.ends_with("\"effects\" : [") {
            continue;
        }
        let mut block: Vec<String> = Vec::new();
        while index < lines.len() && !lines[index].trim_matches([' ', '\t']).starts_with(']') {
            block.push(lines[index].strip_suffix(',').unwrap_or(lines[index]).to_owned());
            index += 1;
        }
        block.sort_by(|a, b| utf8_order(a, b));
        let count = block.len();
        for (position, element) in block.into_iter().enumerate() {
            out.push(if position + 1 < count { element + "," } else { element });
        }
    }
    out
}

/// Replaces each upper-case `UUID.uuidString` with `<uuid-N>`, numbered by
/// first appearance.
fn replace_uuids(text: &str, table: &mut HashMap<String, String>) -> String {
    let bytes = text.as_bytes();
    if bytes.len() < 36 {
        return text.to_owned();
    }
    let is_hex = |byte: u8| byte.is_ascii_digit() || (b'A'..=b'F').contains(&byte);
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if index + 36 <= bytes.len() {
            let candidate = &bytes[index..index + 36];
            let matches = candidate.iter().enumerate().all(|(offset, &byte)| {
                if matches!(offset, 8 | 13 | 18 | 23) { byte == b'-' } else { is_hex(byte) }
            });
            if matches {
                let uuid = String::from_utf8_lossy(candidate).into_owned();
                let next = table.len() + 1;
                let label = table.entry(uuid).or_insert_with(|| format!("<uuid-{next}>")).clone();
                out.extend_from_slice(label.as_bytes());
                index += 36;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(out).unwrap_or_default()
}
