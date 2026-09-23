//! Port of `Sources/DownrightApp/Workspace/WorkspaceLinkGraph.swift`:
//! outgoing links, backlinks and unresolved links over a workspace snapshot.
//!
//! The Swift keys its dictionaries by `String`, so lookups match by canonical
//! equivalence: a link written `café.md` (composed) finds the file whose
//! standardized path spells it decomposed. The maps here are keyed by each
//! key's NFC form ([`upleft_swift_text::string_key`]) to keep that.
//!
//! Reproduced as Downright has it: `normalize` trims `.` and `/` but never
//! resolves `..`, so a relative link that climbs out of its folder resolves
//! only through the stem fallback; wikilinks resolve relative to the linking
//! file's folder; and the stem keys come from `URL(fileURLWithPath:)` of the
//! relative path, which is resolved against the current directory (`/` in a
//! launched app).

use std::collections::HashMap;

use objc2::rc::autoreleasepool;
use upleft_core::NSRange;
use upleft_foundation::url::FileUrl;
use upleft_swift_text::{self as swift, CharSet};

use super::workspace_index::{WorkspaceIndexEntry, WorkspaceIndexSnapshot};
use super::workspace_search::{WorkspaceSearch, WorkspaceSearchMention};
use crate::support::find_engine::{ns_string, string};

#[derive(Clone, Debug)]
pub struct WorkspaceLinkTarget {
    pub source_file: String,
    pub source_range: NSRange,
    pub destination: String,
    pub target_file: Option<String>,
}

impl PartialEq for WorkspaceLinkTarget {
    fn eq(&self, other: &Self) -> bool {
        swift::str_eq(&self.source_file, &other.source_file)
            && self.source_range == other.source_range
            && swift::str_eq(&self.destination, &other.destination)
            && match (&self.target_file, &other.target_file) {
                (Some(a), Some(b)) => swift::str_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

#[derive(Clone, Debug)]
pub struct WorkspaceBacklink {
    pub source_file: String,
    pub source_range: NSRange,
    pub target_file: String,
    pub destination: String,
}

impl WorkspaceBacklink {
    /// `id`: `"\(sourceFile):\(sourceRange.location):\(targetFile)"`.
    pub fn id(&self) -> String {
        format!("{}:{}:{}", self.source_file, self.source_range.location, self.target_file)
    }
}

impl PartialEq for WorkspaceBacklink {
    fn eq(&self, other: &Self) -> bool {
        swift::str_eq(&self.source_file, &other.source_file)
            && self.source_range == other.source_range
            && swift::str_eq(&self.target_file, &other.target_file)
            && swift::str_eq(&self.destination, &other.destination)
    }
}

/// A `[String: V]` with Swift's key semantics (canonical equivalence),
/// remembering each key as first spelled.
#[derive(Clone, Debug)]
pub struct StringMap<V> {
    map: HashMap<String, (String, V)>,
}

impl<V> Default for StringMap<V> {
    fn default() -> Self {
        StringMap { map: HashMap::new() }
    }
}

impl<V: PartialEq> PartialEq for StringMap<V> {
    fn eq(&self, other: &Self) -> bool {
        self.map.len() == other.map.len()
            && self.map.iter().all(|(key, (_, value))| other.map.get(key).is_some_and(|(_, other)| other == value))
    }
}

fn key(value: &str) -> std::borrow::Cow<'_, str> {
    if value.is_ascii() { std::borrow::Cow::Borrowed(value) } else { std::borrow::Cow::Owned(swift::string_key(value)) }
}

impl<V> StringMap<V> {
    pub fn get(&self, name: &str) -> Option<&V> {
        self.map.get(key(name).as_ref()).map(|(_, value)| value)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut V> {
        self.map.get_mut(key(name).as_ref()).map(|(_, value)| value)
    }

    pub fn insert(&mut self, name: &str, value: V) {
        match self.map.get_mut(key(name).as_ref()) {
            Some(slot) => slot.1 = value,
            None => {
                self.map.insert(key(name).into_owned(), (name.to_owned(), value));
            }
        }
    }

    /// `dictionary[key, default: …]`.
    pub fn entry_or(&mut self, name: &str, default: impl FnOnce() -> V) -> &mut V {
        let normalized = key(name).into_owned();
        &mut self.map.entry(normalized).or_insert_with(|| (name.to_owned(), default())).1
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// The pairs, keys as first spelled, in no particular order (Swift's
    /// `Dictionary` order is per-process).
    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.map.values().map(|(name, value)| (name.as_str(), value))
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct WorkspaceLinkGraph {
    pub outgoing: StringMap<Vec<WorkspaceLinkTarget>>,
    pub backlinks: StringMap<Vec<WorkspaceBacklink>>,
    pub unresolved: Vec<WorkspaceLinkTarget>,
    pub unlinked_mentions: StringMap<Vec<WorkspaceSearchMention>>,
}

impl WorkspaceLinkGraph {
    /// `WorkspaceLinkGraph.empty`.
    pub fn empty() -> WorkspaceLinkGraph {
        WorkspaceLinkGraph::default()
    }

    pub fn links_to(&self, file_id: &str) -> Vec<WorkspaceBacklink> {
        self.backlinks.get(file_id).cloned().unwrap_or_default()
    }
}

pub struct WorkspaceLinkGraphBuilder;

impl WorkspaceLinkGraphBuilder {
    /// `build(snapshot:)` without unlinked mentions.
    pub fn build(snapshot: &WorkspaceIndexSnapshot) -> WorkspaceLinkGraph {
        Self::build_with(snapshot, false)
    }

    pub fn build_with(snapshot: &WorkspaceIndexSnapshot, include_unlinked_mentions: bool) -> WorkspaceLinkGraph {
        // `normalize` folds distinct files together (`a\b.md` vs `a/b.md`,
        // and a hidden `.x.md` vs `x.md`). Prefer the entry whose relative
        // path is already normalized; among ties the sorted scan order
        // decides.
        let mut by_path: StringMap<&WorkspaceIndexEntry> = StringMap::default();
        for entry in &snapshot.entries {
            let key = normalize(&entry.relative_path);
            if let Some(existing) = by_path.get(&key) {
                if !swift::str_eq(&existing.relative_path, &key) && swift::str_eq(&entry.relative_path, &key) {
                    by_path.insert(&key, entry);
                }
                continue;
            }
            by_path.insert(&key, entry);
        }
        let mut by_stem: StringMap<Vec<&WorkspaceIndexEntry>> = StringMap::default();
        for entry in &snapshot.entries {
            let stem = normalize(&FileUrl::from_path(&entry.relative_path).deleting_path_extension().path());
            by_stem.entry_or(&stem, Vec::new).push(entry);
        }
        let mut outgoing: StringMap<Vec<WorkspaceLinkTarget>> = StringMap::default();
        let mut backlinks: StringMap<Vec<WorkspaceBacklink>> = StringMap::default();
        let mut unresolved = Vec::new();

        for entry in &snapshot.entries {
            for link in &entry.links {
                let target = resolve(&link.destination, entry, &by_path, &by_stem);
                let item = WorkspaceLinkTarget {
                    source_file: entry.id.clone(),
                    source_range: link.range,
                    destination: link.destination.clone(),
                    target_file: target.map(|target| target.id.clone()),
                };
                outgoing.entry_or(&entry.id, Vec::new).push(item.clone());
                if let Some(target) = target {
                    backlinks.entry_or(&target.id, Vec::new).push(WorkspaceBacklink {
                        source_file: entry.id.clone(),
                        source_range: link.range,
                        target_file: target.id.clone(),
                        destination: link.destination.clone(),
                    });
                } else if is_local(&link.destination) {
                    unresolved.push(item);
                }
            }
        }

        let unlinked_mentions = if include_unlinked_mentions {
            WorkspaceSearch::unlinked_mentions(snapshot, None)
        } else {
            StringMap::default()
        };
        WorkspaceLinkGraph { outgoing, backlinks, unresolved, unlinked_mentions }
    }
}

fn resolve<'a>(
    destination: &str,
    source: &WorkspaceIndexEntry,
    by_path: &StringMap<&'a WorkspaceIndexEntry>,
    by_stem: &StringMap<Vec<&'a WorkspaceIndexEntry>>,
) -> Option<&'a WorkspaceIndexEntry> {
    if !is_local(destination) {
        return None;
    }
    let raw = swift::trimming(swift::trim_whitespaces_and_newlines(destination), CharSet::Chars("<>"));
    let without_fragment = swift::split(raw, '#', 1, false).into_iter().next().unwrap_or(raw);
    let without_query = swift::split(without_fragment, '?', 1, false).into_iter().next().unwrap_or(without_fragment);
    let decoded = removing_percent_encoding(without_query).unwrap_or_else(|| without_query.to_owned());
    if destination.is_empty() {
        return None;
    }
    let path = if swift::has_prefix(destination, "/") {
        normalize(swift::drop_first(&decoded, 1))
    } else if swift::has_prefix(raw, "[[") {
        normalize(&decoded)
    } else {
        let source_directory = deleting_last_path_component(&source.relative_path);
        let combined = if source_directory == "." { decoded } else { format!("{source_directory}/{decoded}") };
        normalize(&combined)
    };
    if let Some(exact) = by_path.get(&path) {
        return Some(exact);
    }
    let with_markdown = if swift::has_suffix(&swift::lowercased(&path), ".md") { path.clone() } else { format!("{path}.md") };
    if let Some(exact) = by_path.get(&with_markdown) {
        return Some(exact);
    }
    match by_stem.get(&path) {
        Some(stem) if stem.len() == 1 => Some(stem[0]),
        _ => match by_stem.get(&with_markdown) {
            Some(stem) if stem.len() == 1 => Some(stem[0]),
            _ => None,
        },
    }
}

/// `isLocal(_:)`.
pub(crate) fn is_local(destination: &str) -> bool {
    let value = swift::trim_whitespaces_and_newlines(destination);
    if value.is_empty() {
        return false;
    }
    if swift::has_prefix(value, "#") {
        return false;
    }
    if swift::contains(value, "://") {
        return false;
    }
    let lower = swift::lowercased(value);
    !["mailto:", "javascript:", "data:", "file:"].iter().any(|scheme| swift::has_prefix(&lower, scheme))
}

/// `normalize(_:)`: backslashes become slashes, then `.` and `/` are trimmed
/// from both ends.
pub(crate) fn normalize(path: &str) -> String {
    swift::trimming(&swift::replacing_occurrences(path, "\\", "/"), CharSet::Chars("./")).to_owned()
}

/// `String.removingPercentEncoding` (the same as `NSString`'s, probed).
pub(crate) fn removing_percent_encoding(value: &str) -> Option<String> {
    autoreleasepool(|_| ns_string(value).stringByRemovingPercentEncoding().map(|decoded| string(&decoded)))
}

/// `(path as NSString).deletingLastPathComponent`.
pub(crate) fn deleting_last_path_component(path: &str) -> String {
    autoreleasepool(|_| string(&ns_string(path).stringByDeletingLastPathComponent()))
}
