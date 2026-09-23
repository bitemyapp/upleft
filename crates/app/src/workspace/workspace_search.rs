//! Port of `Sources/DownrightApp/Workspace/WorkspaceSearch.swift`: regex and
//! literal search over a workspace snapshot, unlinked mentions, and the
//! main-thread [`WorkspaceSearchSession`] that runs searches off the main
//! thread and publishes only the latest.
//!
//! Matching is `NSRegularExpression`'s through objc2. Unlike `FindEngine`,
//! whole-word mode wraps the pattern without a group (`\b…\b`) and
//! zero-length matches are kept, as in the Swift.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use dispatch2::{DispatchQoS, DispatchQueue, GlobalQueueIdentifier, MainThreadBound};
use objc2::MainThreadMarker;
use objc2::rc::autoreleasepool;
use objc2_foundation::{NSMatchingOptions, NSRegularExpressionOptions, NSString};
use upleft_core::NSRange;
use upleft_foundation::url::FileUrl;
use upleft_swift_text as swift;

use super::workspace_index::{
    CancellationToken, WorkspaceIndexEntry, WorkspaceIndexSnapshot, read_up_to_count, string_from_utf8_data,
};
use super::workspace_link_graph::{StringMap, WorkspaceLinkGraph};
use crate::support::find_engine::{Utf16Text, escaped_pattern, frange, range_of, regular_expression, string};

#[derive(Clone, Debug, Default)]
pub struct WorkspaceSearchQuery {
    pub text: String,
    pub is_regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
}

impl PartialEq for WorkspaceSearchQuery {
    fn eq(&self, other: &Self) -> bool {
        swift::str_eq(&self.text, &other.text)
            && self.is_regex == other.is_regex
            && self.case_sensitive == other.case_sensitive
            && self.whole_word == other.whole_word
    }
}

impl WorkspaceSearchQuery {
    /// `WorkspaceSearchQuery(text:)` with the defaults.
    pub fn new(text: impl Into<String>) -> WorkspaceSearchQuery {
        WorkspaceSearchQuery { text: text.into(), ..WorkspaceSearchQuery::default() }
    }

    pub fn pattern(&self) -> Option<String> {
        if self.text.is_empty() {
            return None;
        }
        let escaped = if self.is_regex { self.text.clone() } else { escaped_pattern(&self.text) };
        Some(if self.whole_word { format!("\\b{escaped}\\b") } else { escaped })
    }

    fn options(&self) -> NSRegularExpressionOptions {
        if self.case_sensitive { NSRegularExpressionOptions::empty() } else { NSRegularExpressionOptions::CaseInsensitive }
    }
}

#[derive(Clone, Debug)]
pub struct WorkspaceSearchResult {
    pub file_id: String,
    pub url: FileUrl,
    pub relative_path: String,
    pub range: NSRange,
    pub context_range: NSRange,
    pub context_text: String,
    pub line: isize,
    pub heading: Option<String>,
}

impl WorkspaceSearchResult {
    /// `id`: `"\(fileID):\(range.location):\(range.length)"`.
    pub fn id(&self) -> String {
        format!("{}:{}:{}", self.file_id, self.range.location, self.range.length)
    }
}

#[derive(Clone, Debug)]
pub struct WorkspaceSearchMention {
    pub file_id: String,
    pub range: NSRange,
    pub target: String,
}

impl WorkspaceSearchMention {
    /// `id`: `"\(fileID):\(range.location):\(target)"`.
    pub fn id(&self) -> String {
        format!("{}:{}:{}", self.file_id, self.range.location, self.target)
    }
}

impl PartialEq for WorkspaceSearchMention {
    fn eq(&self, other: &Self) -> bool {
        swift::str_eq(&self.file_id, &other.file_id) && self.range == other.range && swift::str_eq(&self.target, &other.target)
    }
}

pub struct WorkspaceSearch;

impl WorkspaceSearch {
    /// `search(_:in:)` with the default `limitPerFile: 100`.
    pub fn search(query: &WorkspaceSearchQuery, snapshot: &WorkspaceIndexSnapshot) -> Vec<WorkspaceSearchResult> {
        Self::search_limited(query, snapshot, 100)
    }

    pub fn search_limited(
        query: &WorkspaceSearchQuery,
        snapshot: &WorkspaceIndexSnapshot,
        limit_per_file: usize,
    ) -> Vec<WorkspaceSearchResult> {
        Self::search_entries(query, &snapshot.entries, limit_per_file)
    }

    fn search_entries(
        query: &WorkspaceSearchQuery,
        entries: &[WorkspaceIndexEntry],
        limit_per_file: usize,
    ) -> Vec<WorkspaceSearchResult> {
        autoreleasepool(|_| {
            let Some(pattern) = query.pattern() else { return Vec::new() };
            let Some(regex) = regular_expression(&pattern, query.options()) else { return Vec::new() };
            let mut results = Vec::new();
            for entry in entries {
                let Some(text) = resolved_text(entry) else { continue };
                let text = Utf16Text::new(&text);
                let ns = &text.ns;
                let length = text.units().len() as isize;
                let line_starts = line_start_offsets(text.units());
                let full = NSRange::new(0, length);
                let matches = regex.matchesInString_options_range(ns, NSMatchingOptions::empty(), frange(full));
                for index in 0..matches.count().min(limit_per_file) {
                    let range = range_of(&matches.objectAtIndex(index));
                    let line = line_number(range.location, &line_starts);
                    let context = range_of_line(line, &line_starts, length);
                    let heading = entry
                        .headings
                        .iter()
                        .rev()
                        .find(|heading| heading.range.location <= range.location)
                        .map(|heading| heading.title.clone());
                    results.push(WorkspaceSearchResult {
                        file_id: entry.id.clone(),
                        url: entry.url.clone(),
                        relative_path: entry.relative_path.clone(),
                        range,
                        context_range: context,
                        context_text: substring(ns, context),
                        line,
                        heading,
                    });
                }
            }
            results
        })
    }

    pub fn is_valid(query: &WorkspaceSearchQuery) -> bool {
        let Some(pattern) = query.pattern() else { return true };
        autoreleasepool(|_| regular_expression(&pattern, query.options()).is_some())
    }

    /// Mentions of each file's stem in the other files that do not already
    /// link to it.
    pub fn unlinked_mentions(
        snapshot: &WorkspaceIndexSnapshot,
        graph: Option<&WorkspaceLinkGraph>,
    ) -> StringMap<Vec<WorkspaceSearchMention>> {
        let mut linked_pairs = std::collections::HashSet::new();
        if let Some(graph) = graph {
            for (source, links) in graph.outgoing.iter() {
                for link in links {
                    if let Some(target) = &link.target_file {
                        linked_pairs.insert(swift::string_key(&format!("{source}->{target}")));
                    }
                }
            }
        }
        let mut result: StringMap<Vec<WorkspaceSearchMention>> = StringMap::default();
        // Stem → files inverted index so mentions stay O(files · hits).
        let mut stem_to_targets: StringMap<Vec<&WorkspaceIndexEntry>> = StringMap::default();
        let mut stems: Vec<String> = Vec::new();
        for target in &snapshot.entries {
            let stem = FileUrl::from_path(&target.relative_path).deleting_path_extension().last_path_component();
            if swift::count(&stem) <= 1 {
                continue;
            }
            if stem_to_targets.get(&stem).is_none() {
                stems.push(stem.clone());
            }
            stem_to_targets.entry_or(&stem, Vec::new).push(target);
        }
        // Swift walks the dictionary in its own order; each target belongs
        // to one stem, so the lists it produces do not depend on that order.
        for stem in &stems {
            let targets = stem_to_targets.get(stem).expect("stem was inserted");
            let query = WorkspaceSearchQuery { text: stem.clone(), whole_word: true, ..WorkspaceSearchQuery::default() };
            for source in &snapshot.entries {
                for target in targets {
                    if swift::str_eq(&source.id, &target.id)
                        || linked_pairs.contains(&swift::string_key(&format!("{}->{}", source.id, target.id)))
                    {
                        continue;
                    }
                    for hit in Self::search_entries(&query, std::slice::from_ref(source), 20) {
                        result.entry_or(&target.id, Vec::new).push(WorkspaceSearchMention {
                            file_id: source.id.clone(),
                            range: hit.range,
                            target: stem.clone(),
                        });
                    }
                }
            }
        }
        result
    }
}

/// `ns.substring(with:)` bridged back to a Swift `String`.
fn substring(ns: &NSString, range: NSRange) -> String {
    string(&ns.substringWithRange(frange(range)))
}

/// `resolvedText(for:)`: the entry's own text, or the file read afresh,
/// refusing a file that has grown past the indexed size.
fn resolved_text(entry: &WorkspaceIndexEntry) -> Option<String> {
    if !entry.text.is_empty() {
        return Some(entry.text.clone());
    }
    if !(entry.byte_count >= 0 && entry.byte_count < i64::MAX) {
        return None;
    }
    let limit = entry.byte_count as usize + 1;
    let data = read_up_to_count(&entry.url, limit)?;
    if data.len() as i64 > entry.byte_count {
        return None;
    }
    string_from_utf8_data(&data)
}

fn line_start_offsets(units: &[u16]) -> Vec<isize> {
    let mut starts = vec![0];
    for (index, &unit) in units.iter().enumerate() {
        if unit == 0x0A {
            starts.push(index as isize + 1);
        }
    }
    starts
}

fn line_number(offset: isize, starts: &[isize]) -> isize {
    let mut low: isize = 0;
    let mut high: isize = starts.len() as isize - 1;
    while low <= high {
        let mid = (low + high) / 2;
        if starts[mid as usize] <= offset {
            low = mid + 1;
        } else {
            high = mid - 1;
        }
    }
    (high + 1).max(1)
}

fn range_of_line(line: isize, starts: &[isize], length: isize) -> NSRange {
    let index = 0.max((starts.len() as isize - 1).min(line - 1)) as usize;
    let start = starts[index];
    let end = if index + 1 < starts.len() { starts[index + 1] - 1 } else { length };
    NSRange::new(start, 0.max(end - start))
}

// MARK: - Session

type ResultsHandler = Rc<dyn Fn(&[WorkspaceSearchResult])>;

struct SessionState {
    on_update: Option<ResultsHandler>,
    task: Option<Arc<CancellationToken>>,
    revision: isize,
}

/// Async search coordinator. A new query cancels the old one and only the
/// latest revision may call `onUpdate`. Main-thread only.
pub struct WorkspaceSearchSession {
    state: Rc<RefCell<SessionState>>,
}

impl Default for WorkspaceSearchSession {
    fn default() -> Self {
        WorkspaceSearchSession::new()
    }
}

impl WorkspaceSearchSession {
    pub fn new() -> WorkspaceSearchSession {
        WorkspaceSearchSession { state: Rc::new(RefCell::new(SessionState { on_update: None, task: None, revision: 0 })) }
    }

    fn main_thread() -> MainThreadMarker {
        MainThreadMarker::new().expect("WorkspaceSearchSession is main-thread state (@MainActor)")
    }

    /// `onUpdate`.
    pub fn set_on_update(&self, handler: Option<Box<dyn Fn(&[WorkspaceSearchResult])>>) {
        self.state.borrow_mut().on_update = handler.map(Rc::from);
    }

    pub fn start(&self, query: WorkspaceSearchQuery, snapshot: WorkspaceIndexSnapshot) {
        let mtm = Self::main_thread();
        let (current_revision, task) = {
            let mut state = self.state.borrow_mut();
            state.revision += 1;
            if let Some(task) = &state.task {
                task.cancel();
            }
            let task = Arc::new(CancellationToken::default());
            state.task = Some(task.clone());
            (state.revision, task)
        };
        let weak = MainThreadBound::new(Rc::downgrade(&self.state), mtm);
        DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(DispatchQoS::UserInitiated)).exec_async(
            move || {
                let results = WorkspaceSearch::search(&query, &snapshot);
                DispatchQueue::main().exec_async(move || {
                    let mtm = Self::main_thread();
                    let weak: &Weak<RefCell<SessionState>> = weak.get(mtm);
                    if task.is_cancelled() {
                        return;
                    }
                    let Some(state) = weak.upgrade() else { return };
                    let handler = {
                        let state = state.borrow();
                        if state.revision != current_revision {
                            return;
                        }
                        state.on_update.clone()
                    };
                    if let Some(handler) = handler {
                        handler(&results);
                    }
                });
            },
        );
    }

    pub fn cancel(&self) {
        let mut state = self.state.borrow_mut();
        state.revision += 1;
        if let Some(task) = state.task.take() {
            task.cancel();
        }
    }
}

impl Drop for WorkspaceSearchSession {
    /// `deinit { task?.cancel() }`.
    fn drop(&mut self) {
        if let Some(task) = &self.state.borrow().task {
            task.cancel();
        }
    }
}
