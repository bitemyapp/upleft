//! Port of `Sources/DownrightApp/AI/ChangeTracker.swift`.
//!
//! Change marks for "what changed while I was reading" (§8.1).
//!
//! Marks are **reviewable state, not notifications**:
//!
//! - Visiting a mark dims it; only the user finishing review removes it.
//! - A mark is visited on *departure* — when it leaves the viewport, or after a
//!   dwell on screen — because scrolling past something is not reading it.
//! - The whole set survives a close/reopen cycle (`persisted_marks`), so
//!   closing a window is not the same as saying "I read all twelve of those".
//!
//! Marks are expressed as ranges (UTF-16 offsets) in the current buffer and
//! shifted as the user types, because a mark that drifts is worse than no mark
//! at all.
//!
//! Like the Swift class, a tracker belongs to the main thread: its fade timer
//! is scheduled on the main run loop and captures the tracker weakly. The
//! `on_change` and `on_reviewed` callbacks run with no borrow of the tracker
//! held, so they may read it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::NonNull;
use std::rc::{Rc, Weak};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes, NSTimer};
use upleft_core::contracts::{ChangeHunk, ChangeKind, Uuid};
use upleft_core::ns_range::{NSRange, ns_intersection_range};
use upleft_core::text_diff::TextDiff;
use upleft_foundation::date::Date;
use upleft_foundation::decodable::{self, DecodableValue, DecodingError, Value};
use upleft_foundation::json_encoder::JsonValue;

/// `ChangeKind(rawValue:)`.
pub fn change_kind_from_raw_value(raw: &str) -> Option<ChangeKind> {
    match raw {
        "inserted" => Some(ChangeKind::Inserted),
        "deleted" => Some(ChangeKind::Deleted),
        "modified" => Some(ChangeKind::Modified),
        _ => None,
    }
}

/// `uuid.uuidString`.
pub fn uuid_string(uuid: &Uuid) -> String {
    decodable::uuid_string(uuid.as_bytes())
}

/// `ChangeTracker.Mark`.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub id: Uuid,
    pub kind: ChangeKind,
    /// Range in the current buffer. Never empty for a deletion: see
    /// `TextDiff::anchor_range`.
    pub range: NSRange,
    /// Word-level ranges inside `range` that differ.
    pub word_ranges: Vec<NSRange>,
    /// The bytes the write removed. Non-empty only for `.deleted`.
    pub deleted_text: String,
    pub created: Date,
    /// Dimmed rather than removed. Still drawn, still navigable.
    pub visited: bool,
    /// First time this mark was reported inside the viewport, for the dwell
    /// rule. Transient: not persisted.
    pub first_seen: Option<Date>,
}

impl Mark {
    /// `Mark(kind:range:)` with every other argument defaulted: a fresh
    /// UUID, created now, unvisited.
    pub fn new(kind: ChangeKind, range: NSRange) -> Mark {
        Mark {
            id: Uuid::new_v4(),
            kind,
            range,
            word_ranges: Vec::new(),
            deleted_text: String::new(),
            created: Date::now(),
            visited: false,
            first_seen: None,
        }
    }
}

/// `ChangeTracker.PersistedRange`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PersistedRange {
    pub location: isize,
    pub length: isize,
}

impl PersistedRange {
    pub fn new(range: NSRange) -> PersistedRange {
        PersistedRange { location: range.location, length: range.length }
    }

    pub fn range(&self) -> NSRange {
        NSRange::new(self.location, self.length)
    }

    pub fn encode(&self) -> JsonValue {
        JsonValue::object([
            ("location", JsonValue::Int(self.location as i64)),
            ("length", JsonValue::Int(self.length as i64)),
        ])
    }

    pub fn decode(value: &Value) -> Result<PersistedRange, DecodingError> {
        let keyed = value.keyed_container()?;
        Ok(PersistedRange {
            location: keyed.decode("location", Value::int_value)? as isize,
            length: keyed.decode("length", Value::int_value)? as isize,
        })
    }
}

/// `ChangeTracker.PersistedMark`: a mark on disk. `kind` travels as its raw
/// string.
#[derive(Clone, Debug, PartialEq)]
pub struct PersistedMark {
    pub id: Uuid,
    pub kind: String,
    pub range: PersistedRange,
    pub word_ranges: Vec<PersistedRange>,
    pub deleted_text: String,
    pub created: Date,
    pub visited: bool,
}

impl PersistedMark {
    /// Synthesized `encode(to:)` under `.iso8601`.
    pub fn encode(&self) -> JsonValue {
        JsonValue::object([
            ("id", JsonValue::from(uuid_string(&self.id))),
            ("kind", JsonValue::from(self.kind.as_str())),
            ("range", self.range.encode()),
            ("wordRanges", JsonValue::Array(self.word_ranges.iter().map(PersistedRange::encode).collect())),
            ("deletedText", JsonValue::from(self.deleted_text.as_str())),
            ("created", JsonValue::from(self.created.iso8601())),
            ("visited", JsonValue::Bool(self.visited)),
        ])
    }

    /// Synthesized `init(from:)` under `.iso8601`.
    pub fn decode(value: &Value) -> Result<PersistedMark, DecodingError> {
        let keyed = value.keyed_container()?;
        Ok(PersistedMark {
            id: Uuid::from_bytes(keyed.decode("id", Value::uuid_value)?),
            kind: keyed.decode("kind", Value::string_value)?,
            range: keyed.decode("range", PersistedRange::decode)?,
            word_ranges: keyed.decode("wordRanges", |value| value.array_of(PersistedRange::decode))?,
            deleted_text: keyed.decode("deletedText", Value::string_value)?,
            created: keyed.decode("created", Value::date_iso8601)?,
            visited: keyed.decode("visited", Value::bool_value)?,
        })
    }
}

/// Identity used to carry review progress across a re-diff: the same kind of
/// edit over the same bytes.
#[derive(Clone, PartialEq, Eq, Hash)]
struct MarkIdentity {
    kind: &'static str,
    location: isize,
    length: isize,
}

impl MarkIdentity {
    fn new(mark: &Mark) -> MarkIdentity {
        MarkIdentity { kind: mark.kind.raw_value(), location: mark.range.location, length: mark.range.length }
    }
}

type Callback = Rc<RefCell<Option<Box<dyn FnMut()>>>>;

/// Calls a callback with no borrow held, so it may call back into the
/// tracker (and even replace itself).
fn fire(callback: &Callback) {
    let taken = callback.borrow_mut().take();
    if let Some(mut function) = taken {
        function();
        let mut slot = callback.borrow_mut();
        if slot.is_none() {
            *slot = Some(function);
        }
    }
}

struct State {
    lifetime: f64,
    dwell: f64,
    marks: Vec<Mark>,
    fade_timer: Option<Retained<NSTimer>>,
}

/// `ChangeTracker`.
pub struct ChangeTracker {
    state: Rc<RefCell<State>>,
    on_change: Callback,
    on_reviewed: Callback,
}

impl Default for ChangeTracker {
    fn default() -> Self {
        ChangeTracker::new()
    }
}

impl Drop for ChangeTracker {
    fn drop(&mut self) {
        if let Some(timer) = self.state.borrow_mut().fade_timer.take() {
            timer.invalidate();
        }
    }
}

impl ChangeTracker {
    pub fn new() -> ChangeTracker {
        ChangeTracker {
            state: Rc::new(RefCell::new(State { lifetime: 600.0, dwell: 1.5, marks: Vec::new(), fade_timer: None })),
            on_change: Rc::new(RefCell::new(None)),
            on_reviewed: Rc::new(RefCell::new(None)),
        }
    }

    /// How long a mark stays on the page before it expires (§8.1).
    pub fn lifetime(&self) -> f64 {
        self.state.borrow().lifetime
    }

    pub fn set_lifetime(&self, value: f64) {
        self.state.borrow_mut().lifetime = value;
    }

    /// How long a mark has to sit in the viewport before it counts as read.
    pub fn dwell(&self) -> f64 {
        self.state.borrow().dwell
    }

    pub fn set_dwell(&self, value: f64) {
        self.state.borrow_mut().dwell = value;
    }

    /// Everything the tracker holds, unread and visited alike.
    pub fn marks(&self) -> Vec<Mark> {
        self.state.borrow().marks.clone()
    }

    /// Fires when the mark set changes in a way that needs a redraw.
    pub fn set_on_change(&self, callback: Option<Box<dyn FnMut()>>) {
        *self.on_change.borrow_mut() = callback;
    }

    /// Fires when the user finished reviewing (`clear()`).
    pub fn set_on_reviewed(&self, callback: Option<Box<dyn FnMut()>>) {
        *self.on_reviewed.borrow_mut() = callback;
    }

    pub fn is_empty(&self) -> bool {
        self.state.borrow().marks.is_empty()
    }

    pub fn count(&self) -> usize {
        self.state.borrow().marks.len()
    }

    /// **The decorator's input**, and what navigation walks: every mark still
    /// inside its lifetime, visited and unread alike.
    pub fn decorated_marks(&self) -> Vec<Mark> {
        let state = self.state.borrow();
        let cutoff = Date::now().adding(-state.lifetime);
        state.marks.iter().filter(|mark| mark.created > cutoff).cloned().collect()
    }

    /// Marks the reader has not looked at yet. For counts, not for drawing.
    pub fn unread_marks(&self) -> Vec<Mark> {
        self.decorated_marks().into_iter().filter(|mark| !mark.visited).collect()
    }

    /// Historical name for `unread_marks`.
    pub fn visible_marks(&self) -> Vec<Mark> {
        self.unread_marks()
    }

    pub fn unread_count(&self) -> usize {
        self.unread_marks().len()
    }

    /// Number of changed *blocks*, which is what the conflict bar reports.
    pub fn changed_block_count(&self) -> usize {
        self.count()
    }

    // MARK: Applying a diff

    /// Replaces the mark set from a fresh diff. `new_text` and `old_text` are
    /// the two sides of that diff (`""` when unknown). Review progress is
    /// carried across: a mark whose kind and range are unchanged keeps its
    /// identity, its creation date, and its visited flag.
    pub fn apply(&self, hunks: &[ChangeHunk], new_text: &str, old_text: &str, replacing_existing: bool) {
        let now = Date::now();
        let new_length = upleft_swift_text::utf16_count(new_text);
        let old_units: Vec<u16> = old_text.encode_utf16().collect();
        {
            let mut state = self.state.borrow_mut();
            let mut carried: HashMap<MarkIdentity, Mark> = HashMap::new();
            for mark in &state.marks {
                carried.entry(MarkIdentity::new(mark)).or_insert_with(|| mark.clone());
            }
            let fresh: Vec<Mark> = hunks
                .iter()
                .map(|hunk| {
                    let range =
                        if new_text.is_empty() { hunk.new_range } else { TextDiff::anchor_range(hunk, new_length) };
                    let deleted = if hunk.kind == ChangeKind::Deleted
                        && hunk.old_range.upper_bound() <= old_units.len() as isize
                    {
                        String::from_utf16_lossy(&old_units[hunk.old_range.as_usize_range()])
                    } else {
                        String::new()
                    };
                    let mut mark = Mark {
                        word_ranges: hunk.word_ranges.clone(),
                        deleted_text: deleted,
                        created: now,
                        ..Mark::new(hunk.kind, range)
                    };
                    if let Some(previous) = carried.get(&MarkIdentity::new(&mark)) {
                        mark.id = previous.id;
                        mark.created = previous.created;
                        mark.visited = previous.visited;
                        mark.first_seen = previous.first_seen;
                    }
                    mark
                })
                .collect();
            if replacing_existing {
                state.marks = fresh;
            } else {
                state.marks.extend(fresh);
            }
            state.marks.sort_by_key(|mark| mark.range.location);
        }
        self.schedule_fade_check();
        fire(&self.on_change);
    }

    /// The user finished reviewing: drop every mark and let the document move
    /// its review baseline forward.
    pub fn clear(&self) {
        let had_marks = {
            let mut state = self.state.borrow_mut();
            let had_marks = !state.marks.is_empty();
            state.marks.clear();
            if let Some(timer) = state.fade_timer.take() {
                timer.invalidate();
            }
            had_marks
        };
        if had_marks {
            fire(&self.on_change);
        }
        fire(&self.on_reviewed);
    }

    /// Drop every mark because the *document* went away. Never advances the
    /// review baseline: the reader still has not read those changes.
    pub fn reset(&self) {
        {
            let mut state = self.state.borrow_mut();
            if state.marks.is_empty() {
                return;
            }
            state.marks.clear();
            if let Some(timer) = state.fade_timer.take() {
                timer.invalidate();
            }
        }
        fire(&self.on_change);
    }

    // MARK: Persistence (§8.2)

    pub fn persisted_marks(&self) -> Vec<PersistedMark> {
        self.state
            .borrow()
            .marks
            .iter()
            .map(|mark| PersistedMark {
                id: mark.id,
                kind: mark.kind.raw_value().to_owned(),
                range: PersistedRange::new(mark.range),
                word_ranges: mark.word_ranges.iter().copied().map(PersistedRange::new).collect(),
                deleted_text: mark.deleted_text.clone(),
                created: mark.created,
                visited: mark.visited,
            })
            .collect()
    }

    /// Re-anchors persisted marks onto a freshly computed set: a stored mark
    /// claims a computed one when both describe the same kind of edit over the
    /// same bytes.
    pub fn merge(&self, persisted: &[PersistedMark]) {
        let changed = {
            let mut state = self.state.borrow_mut();
            if persisted.is_empty() || state.marks.is_empty() {
                return;
            }
            let mut stored: HashMap<String, &PersistedMark> = HashMap::new();
            for mark in persisted {
                stored
                    .entry(format!("{}:{}:{}", mark.kind, mark.range.location, mark.range.length))
                    .or_insert(mark);
            }
            let mut changed = false;
            for mark in state.marks.iter_mut() {
                let key = format!("{}:{}:{}", mark.kind.raw_value(), mark.range.location, mark.range.length);
                let Some(found) = stored.get(&key) else {
                    continue;
                };
                mark.id = found.id;
                mark.created = found.created;
                mark.visited = found.visited;
                changed = changed || found.visited;
            }
            changed
        };
        if changed {
            fire(&self.on_change);
        }
    }

    /// Re-anchors persisted marks into a document of `text_length` UTF-16
    /// units. A mark that no longer fits, or that has outlived `lifetime`, is
    /// dropped rather than clamped.
    pub fn restore(&self, persisted: &[PersistedMark], text_length: isize, now: Date) {
        {
            let mut state = self.state.borrow_mut();
            let cutoff = now.adding(-state.lifetime);
            let mut marks: Vec<Mark> = persisted
                .iter()
                .filter_map(|stored| {
                    let kind = change_kind_from_raw_value(&stored.kind)?;
                    if !(stored.created > cutoff) {
                        return None;
                    }
                    let range = stored.range.range();
                    if !(range.location >= 0 && range.upper_bound() <= text_length) {
                        return None;
                    }
                    Some(Mark {
                        id: stored.id,
                        kind,
                        range,
                        word_ranges: stored
                            .word_ranges
                            .iter()
                            .map(PersistedRange::range)
                            .filter(|range| range.upper_bound() <= text_length)
                            .collect(),
                        deleted_text: stored.deleted_text.clone(),
                        created: stored.created,
                        visited: stored.visited,
                        first_seen: None,
                    })
                })
                .collect();
            marks.sort_by_key(|mark| mark.range.location);
            state.marks = marks;
        }
        self.schedule_fade_check();
        fire(&self.on_change);
    }

    // MARK: Staying valid under editing

    /// Adjusts marks for a local edit so they keep pointing at the same text.
    /// A mark whose range the edit overlaps is dropped.
    pub fn adjust(&self, edit_range: NSRange, delta: isize) {
        let changed = {
            let mut state = self.state.borrow_mut();
            if state.marks.is_empty() {
                return;
            }
            let before = state.marks.len();
            let marks = std::mem::take(&mut state.marks);
            state.marks = marks
                .into_iter()
                .filter_map(|mut mark| {
                    if mark.range.upper_bound() <= edit_range.location {
                        return Some(mark);
                    }
                    if mark.range.location >= edit_range.upper_bound() {
                        mark.range.location += delta;
                        mark.word_ranges = mark
                            .word_ranges
                            .iter()
                            .map(|range| NSRange::new(range.location + delta, range.length))
                            .collect();
                        return Some(mark);
                    }
                    None // overlapped by the edit
                })
                .collect();
            state.marks.len() != before
        };
        if changed {
            fire(&self.on_change);
        }
    }

    // MARK: Navigation (§7.2 `[` / `]`, ⌥↑ / ⌥↓)

    pub fn next(&self, after: isize) -> Option<Mark> {
        let marks = self.decorated_marks();
        marks.iter().find(|mark| mark.range.location > after).or(marks.first()).cloned()
    }

    pub fn previous(&self, before: isize) -> Option<Mark> {
        let marks = self.decorated_marks();
        marks.iter().rev().find(|mark| mark.range.upper_bound() < before).or(marks.last()).cloned()
    }

    pub fn mark_at(&self, offset: isize) -> Option<Mark> {
        self.decorated_marks().into_iter().find(|mark| mark.range.touches(offset))
    }

    /// Explicit "I have read this one". **Not** to be called on arrival at a
    /// mark — see [`ChangeTracker::note_visible_range`].
    pub fn mark_visited(&self, id: Uuid) {
        {
            let mut state = self.state.borrow_mut();
            let Some(mark) = state.marks.iter_mut().find(|mark| mark.id == id) else {
                return;
            };
            if mark.visited {
                return;
            }
            mark.visited = true;
        }
        fire(&self.on_change);
    }

    /// Reports what the reader can currently see, in source offsets. A mark
    /// becomes visited when it has been on screen for `dwell`, or when it
    /// leaves the viewport having been on screen at all.
    pub fn note_visible_range(&self, visible: NSRange, now: Date) {
        let changed = {
            let mut state = self.state.borrow_mut();
            if state.marks.is_empty() {
                return;
            }
            let dwell = state.dwell;
            let mut changed = false;
            for mark in state.marks.iter_mut() {
                let is_on_screen =
                    ns_intersection_range(mark.range, visible).length > 0 || mark.range.touches(visible.location);
                if is_on_screen {
                    if mark.visited {
                        continue;
                    }
                    let Some(seen) = mark.first_seen else {
                        mark.first_seen = Some(now);
                        continue;
                    };
                    if now.time_interval_since(seen) >= dwell {
                        mark.visited = true;
                        changed = true;
                    }
                } else if mark.first_seen.is_some() && !mark.visited {
                    mark.visited = true;
                    changed = true;
                }
            }
            changed
        };
        if changed {
            fire(&self.on_change);
        }
    }

    /// Ranges to decorate for one kind. Includes visited marks.
    pub fn ranges(&self, kind: ChangeKind) -> Vec<NSRange> {
        self.decorated_marks().into_iter().filter(|mark| mark.kind == kind).map(|mark| mark.range).collect()
    }

    // MARK: Fading

    fn schedule_fade_check(&self) {
        let mut state = self.state.borrow_mut();
        if let Some(timer) = state.fade_timer.take() {
            timer.invalidate();
        }
        if state.marks.is_empty() {
            return;
        }
        let weak_state: Weak<RefCell<State>> = Rc::downgrade(&self.state);
        let weak_change: Weak<RefCell<Option<Box<dyn FnMut()>>>> = Rc::downgrade(&self.on_change);
        let block = RcBlock::new(move |_timer: NonNull<NSTimer>| {
            let (Some(state), Some(on_change)) = (weak_state.upgrade(), weak_change.upgrade()) else {
                return;
            };
            if drop_expired(&state, Date::now()) {
                fire(&on_change);
            }
            let mut state = state.borrow_mut();
            if state.marks.is_empty()
                && let Some(timer) = state.fade_timer.take()
            {
                timer.invalidate();
            }
        });
        // SAFETY: the timer fires on the main run loop, the thread that owns
        // this tracker; the block holds only weak references.
        let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(30.0, true, &block) };
        unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
        state.fade_timer = Some(timer);
    }

    /// Retires every mark past its lifetime. Deliberately not `on_reviewed`:
    /// a mark ageing out is the queue emptying itself, not the reader saying
    /// they read it.
    pub fn drop_expired_marks(&self, now: Date) -> bool {
        let dropped = drop_expired(&self.state, now);
        if dropped {
            fire(&self.on_change);
        }
        dropped
    }
}

fn drop_expired(state: &Rc<RefCell<State>>, now: Date) -> bool {
    let mut state = state.borrow_mut();
    let cutoff = now.adding(-state.lifetime);
    let before = state.marks.len();
    state.marks.retain(|mark| !(mark.created <= cutoff));
    state.marks.len() != before
}

/// `JSONDecoder` for a `[PersistedMark]` value, for callers decoding one on
/// its own.
pub fn decode_persisted_marks(value: &Value) -> Result<Vec<PersistedMark>, DecodingError> {
    value.array_of(PersistedMark::decode)
}
