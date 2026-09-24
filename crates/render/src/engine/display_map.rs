//! Port of `Engine/DisplayMap.swift`.
//!
//! The single highest-risk piece of arithmetic in the app (§6.1).
//!
//! §3.1 forbids ever mutating characters, so hiding a marker cannot mean
//! deleting it. It means handing TextKit 2 a *different attributed string* for
//! the paragraph that contains it. The physical fallback uses
//! `NSTextContentStorageDelegate.textContentStorage(_:textParagraphWith:)`,
//! while grouped prose uses `MarkdownContentStorage` to vend source-length
//! elements. Both keep the backing source untouched and coordinate-safe.
//!
//! Substitution creates two coordinate spaces that must never be confused:
//!
//!   * **Source offsets** — UTF-16 offsets into `NSTextStorage`. The only
//!     truth. Every public API in this package speaks source offsets.
//!
//!   * **TextKit offsets** — what TextKit 2 reports back. When the layout
//!     manager resolves a point or a caret inside a substituted paragraph it
//!     forms the location as
//!     `contentManager.location(element.elementRange.location, offsetBy: i)`
//!     where `i` indexes the *substituted* string. So a TextKit offset is
//!     "source offset of the paragraph start, plus the display index inside
//!     it". It is neither a source offset nor a document-wide display offset.
//!
//! Two consequences worth stating out loud, because both are easy to get wrong:
//!
//!   1. The TextKit space is **not contiguous**. Paragraph `n`'s TextKit
//!      offsets stop short of paragraph `n+1`'s start by exactly the number of
//!      characters paragraph `n` lost. Never do length arithmetic in the
//!      TextKit space — convert both endpoints of a range independently.
//!
//!   2. The map is monotonic non-decreasing in both directions, which is what
//!      keeps selection order and hit testing stable.

use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::Arc;

use objc2::rc::Retained;
use objc2_foundation::{NSAttributedString, NSMutableAttributedString, NSString};
use upleft_core::NSRange;

use super::ns_range;

// MARK: - Paragraph index

/// Start offsets of every paragraph of the source text, where "paragraph"
/// means what `NSTextContentStorage` means by it: a run terminated by `\n`,
/// `\r`, `\r\n`, U+0085, U+2028, or U+2029, with the terminator belonging to
/// the paragraph it ends.
///
/// `starts` is shared, so copying an index is as cheap as copying the Swift
/// struct's copy-on-write array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParagraphIndex {
    /// Ascending, always begins with 0.
    pub starts: Arc<Vec<isize>>,
    /// UTF-16 length of the text this index was built from.
    pub length: isize,
}

unsafe extern "C" {
    fn CFStringGetCharactersPtr(string: *const NSString) -> *const u16;
}

impl ParagraphIndex {
    pub fn new(starts: Vec<isize>, length: isize) -> ParagraphIndex {
        ParagraphIndex {
            starts: Arc::new(if starts.is_empty() { vec![0] } else { starts }),
            length,
        }
    }

    /// `ParagraphIndex.empty`.
    pub fn empty() -> ParagraphIndex {
        ParagraphIndex::new(vec![0], 0)
    }

    /// `init(text:)`: a single pass over the UTF-16 buffer.
    ///
    /// Rebuilt on every text change and never on a caret move, so it sits on
    /// the keystroke path (§12). That is why it takes the contiguous-buffer
    /// fast path when `CFString` can hand one over, and falls back to chunked
    /// copies otherwise.
    pub fn from_text(text: &NSString) -> ParagraphIndex {
        let n = text.length() as isize;
        let mut starts: Vec<isize> = Vec::with_capacity((n / 24 + 8) as usize);
        starts.push(0);
        let mut pending_cr = false;

        // SAFETY: `text` is a live CFString (toll-free bridged).
        let contiguous = if n > 0 {
            unsafe { CFStringGetCharactersPtr(text) }
        } else {
            std::ptr::null()
        };
        if n > 0 && !contiguous.is_null() {
            // SAFETY: CFStringGetCharactersPtr returned `n` units that live as
            // long as `text` is unmutated, which it is for this call.
            let buffer = unsafe { std::slice::from_raw_parts(contiguous, n as usize) };
            ParagraphIndex::scan(buffer, 0, &mut starts, &mut pending_cr);
        } else if n > 0 {
            const CHUNK_SIZE: isize = 8192;
            let mut buffer = vec![0u16; CHUNK_SIZE as usize];
            let mut base = 0isize;
            while base < n {
                let count = CHUNK_SIZE.min(n - base);
                // SAFETY: `buffer` holds CHUNK_SIZE >= count units.
                unsafe {
                    text.getCharacters_range(
                        NonNull::new(buffer.as_mut_ptr()).unwrap(),
                        ns_range(NSRange::new(base, count)),
                    );
                }
                ParagraphIndex::scan(&buffer[..count as usize], base, &mut starts, &mut pending_cr);
                base += count;
            }
        }
        if pending_cr {
            starts.push(n);
        }
        // A terminator at the very end leaves an empty final paragraph, which
        // is real to TextKit, so it is kept.
        ParagraphIndex::new(starts, n)
    }

    /// The index of `text` after an edit that left its first `edit_floor`
    /// UTF-16 units unchanged (an Upleft extension for hosted streaming).
    /// Paragraphs up to the one holding the unit before the edit are kept;
    /// the rest of `text` is scanned again. The result equals `from_text`.
    pub fn rebuilt_after_edit(&self, text: &NSString, edit_floor: isize) -> ParagraphIndex {
        let n = text.length() as isize;
        let floor = edit_floor.max(0).min(self.length.min(n));
        // The unit before the edit may be a `\r` the edit completes into `\r\n`,
        // so that paragraph is scanned again too.
        let keep = self.index_containing((floor - 1).max(0));
        let rescan_from = self.starts[keep];
        let mut starts: Vec<isize> = self.starts[..=keep].to_vec();
        let mut pending_cr = false;
        const CHUNK_SIZE: isize = 8192;
        let mut buffer = vec![0u16; CHUNK_SIZE.min((n - rescan_from).max(1)) as usize];
        let mut base = rescan_from;
        while base < n {
            let count = (buffer.len() as isize).min(n - base);
            // SAFETY: `buffer` holds at least `count` units.
            unsafe {
                text.getCharacters_range(
                    NonNull::new(buffer.as_mut_ptr()).unwrap(),
                    ns_range(NSRange::new(base, count)),
                );
            }
            ParagraphIndex::scan(&buffer[..count as usize], base, &mut starts, &mut pending_cr);
            base += count;
        }
        if pending_cr {
            starts.push(n);
        }
        ParagraphIndex::new(starts, n)
    }

    /// `init(text:)` over UTF-16 units already in hand.
    pub fn from_utf16(units: &[u16]) -> ParagraphIndex {
        let n = units.len() as isize;
        let mut starts: Vec<isize> = Vec::with_capacity((n / 24 + 8) as usize);
        starts.push(0);
        let mut pending_cr = false;
        if n > 0 {
            ParagraphIndex::scan(units, 0, &mut starts, &mut pending_cr);
        }
        if pending_cr {
            starts.push(n);
        }
        ParagraphIndex::new(starts, n)
    }

    #[inline]
    fn scan(buffer: &[u16], base: isize, starts: &mut Vec<isize>, pending_cr: &mut bool) {
        let count = buffer.len();
        let mut i = 0usize;
        while i < count {
            // The overwhelmingly common character is none of the terminators;
            // skip whole runs of them before the per-unit logic.
            if !*pending_cr {
                while i < count {
                    let c = buffer[i];
                    if !(0x0A..=0x0D).contains(&c) && c != 0x0085 && c != 0x2028 && c != 0x2029 {
                        i += 1;
                    } else {
                        break;
                    }
                }
                if i >= count {
                    break;
                }
            }
            let c = buffer[i];
            if !(0x0A..=0x0D).contains(&c) {
                if *pending_cr {
                    *pending_cr = false;
                    starts.push(base + i as isize);
                }
                if c == 0x0085 || c == 0x2028 || c == 0x2029 {
                    starts.push(base + i as isize + 1);
                }
                i += 1;
                continue;
            }
            if *pending_cr {
                *pending_cr = false;
                // `\r\n` is one terminator: the paragraph starts after the
                // `\n`, not between the two.
                if c == 0x0A {
                    starts.push(base + i as isize + 1);
                    i += 1;
                    continue;
                }
                starts.push(base + i as isize);
            }
            if c == 0x0D {
                *pending_cr = true;
            } else if c == 0x0A {
                starts.push(base + i as isize + 1);
            }
            i += 1;
        }
    }

    /// Index of the paragraph containing `offset`, clamped into range.
    pub fn index_containing(&self, offset: isize) -> usize {
        if offset <= 0 {
            return 0;
        }
        let starts = &self.starts;
        let (mut lo, mut hi, mut best) = (0isize, starts.len() as isize - 1, 0isize);
        while lo <= hi {
            let mid = (lo + hi) / 2;
            if starts[mid as usize] <= offset {
                best = mid;
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        }
        best as usize
    }

    pub fn start_containing(&self, offset: isize) -> isize {
        self.starts[self.index_containing(offset)]
    }

    pub fn end_of_paragraph_at(&self, index: usize) -> isize {
        if index + 1 < self.starts.len() {
            self.starts[index + 1]
        } else {
            self.length
        }
    }

    pub fn range_at(&self, index: usize) -> NSRange {
        let s = self.starts[index];
        NSRange::new(s, self.end_of_paragraph_at(index) - s)
    }

    /// Range of the paragraph containing `offset`.
    pub fn paragraph_range_containing(&self, offset: isize) -> NSRange {
        self.range_at(self.index_containing(offset))
    }
}

// MARK: - Range normalisation

pub struct RangeSet;

#[inline]
fn location_then_length(a: &NSRange, b: &NSRange) -> std::cmp::Ordering {
    (a.location, a.length).cmp(&(b.location, b.length))
}

impl RangeSet {
    /// Ascending, non-overlapping, zero-length ranges dropped. Every hidden
    /// and elided range list in this package passes through here so downstream
    /// code can assume the invariant instead of re-checking it.
    pub fn normalized(ranges: &[NSRange]) -> Vec<NSRange> {
        if ranges.len() <= 1 {
            return ranges.iter().copied().filter(|r| r.length > 0).collect();
        }
        let mut sorted: Vec<NSRange> = ranges.iter().copied().filter(|r| r.length > 0).collect();
        sorted.sort_by(location_then_length);
        let mut out: Vec<NSRange> = Vec::with_capacity(sorted.len());
        for r in sorted {
            if let Some(last) = out.last_mut()
                && r.location <= last.upper_bound()
            {
                if r.upper_bound() > last.upper_bound() {
                    last.length = r.upper_bound() - last.location;
                }
            } else {
                out.push(r);
            }
        }
        out
    }

    /// Sorted and non-overlapping, but *adjacent* ranges stay separate.
    ///
    /// `normalized` fuses `[0,3)` with `[3,5)`, which is right for a set of
    /// regions and wrong for a set of *markers*. A caret reveal names the
    /// marker it is un-hiding by its exact range, so fusing a list item's `3. `
    /// with the `**` beside it made the opening marker unrevealable while its
    /// closing twin revealed normally.
    pub fn disjoint(ranges: &[NSRange]) -> Vec<NSRange> {
        if ranges.len() <= 1 {
            return ranges.iter().copied().filter(|r| r.length > 0).collect();
        }
        let mut sorted: Vec<NSRange> = ranges.iter().copied().filter(|r| r.length > 0).collect();
        sorted.sort_by(location_then_length);
        let mut out: Vec<NSRange> = Vec::with_capacity(sorted.len());
        for r in sorted {
            let Some(last) = out.last_mut() else {
                out.push(r);
                continue;
            };
            // A true overlap is ambiguous and still fuses; touching at a
            // boundary is two markers standing next to each other.
            if r.location < last.upper_bound() {
                if r.upper_bound() > last.upper_bound() {
                    last.length = r.upper_bound() - last.location;
                }
            } else {
                out.push(r);
            }
        }
        out
    }

    /// End of a paragraph's content, i.e. the paragraph range minus its
    /// terminator.
    pub fn content_end_of_paragraph(range: NSRange, text: &[u16]) -> isize {
        let length = text.len() as isize;
        let mut e = range.upper_bound().min(length);
        if e <= range.location {
            return range.location;
        }
        let last = text[(e - 1) as usize];
        if last == 0x0A || last == 0x0D || last == 0x0085 || last == 0x2028 || last == 0x2029 {
            e -= 1;
            if last == 0x0A && e > range.location && text[(e - 1) as usize] == 0x0D {
                e -= 1;
            }
        }
        e
    }

    /// Sub-ranges of `ranges` that intersect `window`, clipped to it.
    /// `ranges` must be normalised.
    pub fn intersecting(ranges: &[NSRange], window: NSRange) -> Vec<NSRange> {
        let mut out = Vec::new();
        for r in ranges {
            if r.upper_bound() <= window.location {
                continue;
            }
            if r.location >= window.upper_bound() {
                break;
            }
            let lo = r.location.max(window.location);
            let hi = r.upper_bound().min(window.upper_bound());
            if hi > lo {
                out.push(NSRange::new(lo, hi - lo));
            }
        }
        out
    }

    /// True when a normalised `ranges` covers `offset`
    /// (`location <= offset < upperBound`).
    pub fn covers(ranges: &[NSRange], offset: isize) -> bool {
        let (mut lo, mut hi) = (0isize, ranges.len() as isize - 1);
        while lo <= hi {
            let mid = (lo + hi) / 2;
            let r = ranges[mid as usize];
            if offset < r.location {
                hi = mid - 1;
            } else if offset >= r.upper_bound() {
                lo = mid + 1;
            } else {
                return true;
            }
        }
        false
    }
}

/// Immutable coordinate policy for the interval between a source edit and its
/// asynchronous parse commit. Ranges outside the touched physical paragraphs
/// remain valid; later ranges move by the edit delta; touched ranges expire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceEditProjection {
    pub edit: NSRange,
    pub invalidated_range: NSRange,
    pub delta: isize,
}

impl SourceEditProjection {
    pub fn new(edit: NSRange, inserted_length: isize, old_paragraphs: &ParagraphIndex) -> Self {
        let location = 0.max(edit.location.min(old_paragraphs.length));
        let end = location.max(edit.upper_bound().min(old_paragraphs.length));
        let first = old_paragraphs.index_containing(location);
        let last = old_paragraphs.index_containing(location.max(end - 1));
        SourceEditProjection {
            edit: NSRange::new(location, end - location),
            invalidated_range: old_paragraphs.range_at(first).union(old_paragraphs.range_at(last)),
            delta: inserted_length - edit.length,
        }
    }

    pub fn project(&self, range: NSRange) -> Option<NSRange> {
        if range.upper_bound() <= self.invalidated_range.location {
            return Some(range);
        }
        if range.location >= self.invalidated_range.upper_bound() {
            return Some(NSRange::new(range.location + self.delta, range.length));
        }
        None
    }

    /// Projects a coordinate-stable substitution. Only a substitution whose
    /// own source bytes were edited expires; siblings in the same paragraph
    /// remain safe and must not flash back to literal Markdown.
    pub fn project_unchanged(&self, range: NSRange) -> Option<NSRange> {
        if range.upper_bound() <= self.edit.location {
            return Some(range);
        }
        if range.location >= self.edit.upper_bound() {
            return Some(NSRange::new(range.location + self.delta, range.length));
        }
        None
    }

    /// Projects a grouped layout range through an ordinary character edit.
    /// Adding or removing a paragraph separator changes element topology, so
    /// callers must opt out and use the conservative paragraph invalidation.
    pub fn project_container(&self, range: NSRange, preserving_structure: bool) -> Option<NSRange> {
        if range.upper_bound() <= self.edit.location {
            return Some(range);
        }
        if range.location >= self.edit.upper_bound() {
            return Some(NSRange::new(range.location + self.delta, range.length));
        }
        if !(preserving_structure
            && self.edit.location > range.location
            && self.edit.upper_bound() < range.upper_bound()
            && range.length + self.delta > 0)
        {
            return None;
        }
        Some(NSRange::new(range.location, range.length + self.delta))
    }
}

// MARK: - Substitutions

/// One source range replaced in the display string.
///
/// A zero display length is the ordinary hidden-marker representation (§6.1).
/// A positive length is an inline object — today inline math, which becomes a
/// single attachment character carrying the typeset image (§11.3), or a
/// same-length hard-wrap/hidden replacement used by grouped TextKit elements.
#[derive(Debug, Clone)]
pub struct DisplaySubstitution {
    pub source_range: NSRange,
    pub display_length: isize,
    /// Attributed replacement; `None` means "omit", and its length must equal
    /// `display_length`.
    pub replacement: Option<Retained<NSAttributedString>>,
    /// True when the source is semantically hidden, even if a grouped TextKit
    /// element has to carry a same-length replacement for coordinate safety.
    pub is_hidden: bool,
    /// True only for a soft Markdown break replaced inside a grouped element.
    /// Physical paragraph fallback must retain that element's separator.
    pub is_hard_wrap_reflow: bool,
    /// True when every display position in the replacement has the same
    /// source position. This is required for same-length zero-width content;
    /// inline objects still intentionally collapse to their leading edge.
    pub preserves_source_offsets: bool,
}

impl PartialEq for DisplaySubstitution {
    /// Swift's synthesised `==`: the replacement compares with `isEqual:`.
    fn eq(&self, other: &Self) -> bool {
        let same_replacement = match (&self.replacement, &other.replacement) {
            (None, None) => true,
            (Some(a), Some(b)) => a.isEqualToAttributedString(b),
            _ => false,
        };
        self.source_range == other.source_range
            && self.display_length == other.display_length
            && same_replacement
            && self.is_hidden == other.is_hidden
            && self.is_hard_wrap_reflow == other.is_hard_wrap_reflow
            && self.preserves_source_offsets == other.preserves_source_offsets
    }
}

#[inline]
fn replacement_length(replacement: &Option<Retained<NSAttributedString>>) -> isize {
    replacement.as_ref().map_or(0, |r| r.length() as isize)
}

impl DisplaySubstitution {
    pub fn new(
        source_range: NSRange,
        display_length: isize,
        replacement: Option<Retained<NSAttributedString>>,
        is_hidden: bool,
        is_hard_wrap_reflow: bool,
        preserves_source_offsets: bool,
    ) -> Self {
        DisplaySubstitution {
            source_range,
            display_length,
            replacement,
            is_hidden,
            is_hard_wrap_reflow,
            preserves_source_offsets,
        }
    }

    pub fn hide(range: NSRange) -> Self {
        DisplaySubstitution::new(range, 0, None, true, false, false)
    }

    /// Builds an expanding substitution. `display_length` must never exceed
    /// `source_range.length` (see `DisplayMap::source_offset_for_text_kit`).
    pub fn replace(range: NSRange, string: Retained<NSAttributedString>) -> Self {
        let length = string.length() as isize;
        DisplaySubstitution::new(range, length, Some(string), false, false, false)
    }

    /// Same-length hidden content used by grouped hard-wrap elements.
    pub fn replace_hidden(range: NSRange, string: Retained<NSAttributedString>) -> Self {
        let length = string.length() as isize;
        DisplaySubstitution::new(range, length, Some(string), true, false, true)
    }

    /// Same-length display-only space used for an intra-block soft break.
    pub fn replace_hard_wrap(range: NSRange, string: Retained<NSAttributedString>) -> Self {
        let length = string.length() as isize;
        DisplaySubstitution::new(range, length, Some(string), false, true, true)
    }
}

// MARK: - Display map

/// Source ⇄ TextKit offset conversion for a given set of substitutions.
///
/// Immutable and cheap to copy: its arrays are shared, like the Swift struct's
/// copy-on-write storage.
#[derive(Debug, Clone)]
pub struct DisplayMap {
    pub paragraphs: ParagraphIndex,
    /// Ascending, non-overlapping, never crossing a paragraph boundary and
    /// never covering a paragraph terminator.
    base: Rc<Vec<DisplaySubstitution>>,
    /// Index into `base` of the first entry in each paragraph, so a conversion
    /// only ever touches its own paragraph's entries.
    first_in_paragraph: Arc<Vec<usize>>,
    /// One paragraph whose entries replace the base's (§6.1c).
    override_paragraph: Option<usize>,
    override_entries: Rc<Vec<DisplaySubstitution>>,
    /// Hidden ranges from `base`, cached while the map is built.
    normalized_base_hidden_ranges: Arc<Vec<NSRange>>,
}

impl DisplayMap {
    /// `DisplayMap.identity`.
    pub fn identity() -> DisplayMap {
        DisplayMap::new(ParagraphIndex::empty(), Vec::new())
    }

    /// Sanitisation and the paragraph index are built in one merge pass over
    /// two already-ordered sequences.
    pub fn new(paragraphs: ParagraphIndex, substitutions: Vec<DisplaySubstitution>) -> DisplayMap {
        let ordered = DisplayMap::ordered(substitutions);
        let paragraph_count = paragraphs.starts.len();
        let mut kept: Vec<DisplaySubstitution> = Vec::with_capacity(ordered.len());
        let mut first = vec![0usize; paragraph_count + 1];
        let mut seen = vec![false; paragraph_count + 1];
        let mut paragraph = 0usize;
        let mut last_end = 0isize;

        for sub in ordered {
            let range = sub.source_range;
            if !(range.length > 0
                && range.location >= last_end
                && range.upper_bound() <= paragraphs.length
                && sub.display_length >= 0
                && replacement_length(&sub.replacement) == sub.display_length)
            {
                continue;
            }
            while paragraph + 1 < paragraph_count && paragraphs.starts[paragraph + 1] <= range.location {
                paragraph += 1;
            }
            // A substitution crossing a paragraph terminator would leave the
            // element's range and its content permanently out of step, so it
            // is refused rather than clipped.
            if range.upper_bound() > paragraphs.end_of_paragraph_at(paragraph) {
                continue;
            }
            if !seen[paragraph] {
                seen[paragraph] = true;
                first[paragraph] = kept.len();
            }
            last_end = range.upper_bound();
            kept.push(sub);
        }
        let hidden: Vec<NSRange> = kept
            .iter()
            .filter(|sub| sub.is_hidden)
            .map(|sub| sub.source_range)
            .collect();

        // Paragraphs with nothing of their own point at the next paragraph's
        // first entry, so `entries(inParagraphAt:)` is defined everywhere.
        let mut running = kept.len();
        for p in (0..paragraph_count).rev() {
            if seen[p] {
                running = first[p];
            } else {
                first[p] = running;
            }
        }
        first[paragraph_count] = kept.len();
        DisplayMap {
            paragraphs,
            base: Rc::new(kept),
            first_in_paragraph: Arc::new(first),
            override_paragraph: None,
            override_entries: Rc::new(Vec::new()),
            normalized_base_hidden_ranges: Arc::new(hidden),
        }
    }

    /// Convenience for the common case of pure marker hiding.
    pub fn with_hidden(paragraphs: ParagraphIndex, hidden: &[NSRange]) -> DisplayMap {
        DisplayMap::new(paragraphs, hidden.iter().copied().map(DisplaySubstitution::hide).collect())
    }

    /// Projects an ordinary character edit without rebuilding the paragraph
    /// lookup table. Structural edits fall back to the validating initializer.
    pub fn projecting_stable_topology(
        &self,
        paragraphs: ParagraphIndex,
        substitutions: Vec<DisplaySubstitution>,
        hidden_ranges: Vec<NSRange>,
    ) -> DisplayMap {
        if !(self.override_paragraph.is_none()
            && paragraphs.starts.len() == self.paragraphs.starts.len()
            && substitutions.len() == self.base.len())
        {
            return DisplayMap::new(paragraphs, substitutions);
        }
        DisplayMap {
            paragraphs,
            base: Rc::new(substitutions),
            first_in_paragraph: self.first_in_paragraph.clone(),
            override_paragraph: None,
            override_entries: Rc::new(Vec::new()),
            normalized_base_hidden_ranges: Arc::new(hidden_ranges),
        }
    }

    /// A map identical to this one except that the entries of the paragraph
    /// containing `offset` are replaced. O(entries in that paragraph).
    ///
    /// `entries` must be ascending, non-overlapping, and wholly inside that
    /// paragraph's content; anything else is refused and the receiver is
    /// returned unchanged. Only one paragraph may be overridden at a time.
    pub fn replacing_paragraph(&self, offset: isize, entries: Vec<DisplaySubstitution>) -> DisplayMap {
        let p = self.paragraphs.index_containing(0.max(offset.min(self.paragraphs.length)));
        if !(self.override_paragraph.is_none() || self.override_paragraph == Some(p)) {
            return self.clone();
        }
        let bounds = self.paragraphs.range_at(p);
        let mut previous_end = bounds.location;
        for entry in &entries {
            let r = entry.source_range;
            if !(r.length > 0 && r.location >= previous_end && r.upper_bound() <= bounds.upper_bound()) {
                return self.clone();
            }
            previous_end = r.upper_bound();
        }
        DisplayMap {
            paragraphs: self.paragraphs.clone(),
            base: self.base.clone(),
            first_in_paragraph: self.first_in_paragraph.clone(),
            override_paragraph: Some(p),
            override_entries: Rc::new(entries),
            normalized_base_hidden_ranges: self.normalized_base_hidden_ranges.clone(),
        }
    }

    /// A paragraph-local reveal.
    pub fn replacing_paragraph_excluding(&self, offset: isize, source_ranges: &[NSRange]) -> DisplayMap {
        if source_ranges.is_empty() {
            return self.clone();
        }
        let p = self.paragraphs.index_containing(0.max(offset.min(self.paragraphs.length)));
        let kept: Vec<DisplaySubstitution> = self
            .entries_in_paragraph(p)
            .iter()
            .filter(|entry| !source_ranges.contains(&entry.source_range))
            .cloned()
            .collect();
        self.replacing_paragraph(offset, kept)
    }

    /// Hidden substitutions in one paragraph.
    pub fn hidden_ranges_in_paragraph_containing(&self, offset: isize) -> Vec<NSRange> {
        let p = self.paragraphs.index_containing(0.max(offset.min(self.paragraphs.length)));
        self.entries_in_paragraph(p)
            .iter()
            .filter(|entry| entry.is_hidden)
            .map(|entry| entry.source_range)
            .collect()
    }

    /// All substitutions in one physical paragraph.
    pub fn substitutions_in_paragraph_containing(&self, offset: isize) -> Vec<DisplaySubstitution> {
        let p = self.paragraphs.index_containing(0.max(offset.min(self.paragraphs.length)));
        self.entries_in_paragraph(p).to_vec()
    }

    /// Substitutions intersecting a source range, preserving map order.
    pub fn substitutions_in(&self, source_range: NSRange) -> Vec<DisplaySubstitution> {
        let lower = 0.max(source_range.location.min(self.paragraphs.length));
        let upper = lower.max(source_range.upper_bound().min(self.paragraphs.length));
        if !(upper > lower && (!self.base.is_empty() || !self.override_entries.is_empty())) {
            return Vec::new();
        }
        let first = self.paragraphs.index_containing(lower);
        let last = self.paragraphs.index_containing(lower.max(upper - 1));
        let mut result = Vec::new();
        for paragraph in first..=last {
            result.extend(
                self.entries_in_paragraph(paragraph)
                    .iter()
                    .filter(|sub| sub.source_range.location < upper && sub.source_range.upper_bound() > lower)
                    .cloned(),
            );
        }
        result
    }

    /// Ascending by location. Already-ordered input skips the sort after one
    /// linear check. Swift's sort is stable, and so is this one.
    fn ordered(mut subs: Vec<DisplaySubstitution>) -> Vec<DisplaySubstitution> {
        let is_sorted = subs
            .windows(2)
            .all(|pair| pair[0].source_range.location <= pair[1].source_range.location);
        if !is_sorted {
            subs.sort_by_key(|sub| sub.source_range.location);
        }
        subs
    }

    pub fn is_identity(&self) -> bool {
        self.base.is_empty() && self.override_entries.is_empty()
    }

    /// Every substitution in document order.
    pub fn substitutions(&self) -> Vec<DisplaySubstitution> {
        if self.override_paragraph.is_none() {
            return (*self.base).clone();
        }
        let mut out = Vec::with_capacity(self.base.len());
        for p in 0..self.paragraphs.starts.len() {
            out.extend(self.entries_in_paragraph(p).iter().cloned());
        }
        out
    }

    /// The base substitutions, borrowed (`substitutions` without the copy
    /// when no override is in force).
    pub fn base_substitutions(&self) -> &[DisplaySubstitution] {
        &self.base
    }

    /// Effective hidden ranges for edit projection.
    pub fn hidden_ranges_for_edit_projection(&self) -> Vec<NSRange> {
        let Some(override_paragraph) = self.override_paragraph else {
            return (*self.normalized_base_hidden_ranges).clone();
        };
        let overridden = self.paragraphs.range_at(override_paragraph);
        let mut result = Vec::with_capacity(self.normalized_base_hidden_ranges.len());
        for range in self.normalized_base_hidden_ranges.iter() {
            if range.upper_bound() <= overridden.location {
                result.push(*range);
            }
        }
        result.extend(
            self.override_entries
                .iter()
                .filter(|entry| entry.is_hidden)
                .map(|entry| entry.source_range),
        );
        for range in self.normalized_base_hidden_ranges.iter() {
            if range.location >= overridden.upper_bound() {
                result.push(*range);
            }
        }
        result
    }

    /// The cached base set, without a transient caret override.
    pub fn base_hidden_ranges_for_edit_projection(&self) -> &[NSRange] {
        &self.normalized_base_hidden_ranges
    }

    /// Ranges omitted entirely — what `drHidden` marks.
    pub fn hidden_ranges(&self) -> Vec<NSRange> {
        self.hidden_ranges_for_edit_projection()
    }

    /// The entries in force for a paragraph: its override if it has one, its
    /// slice of the base otherwise.
    #[inline]
    fn entries_in_paragraph(&self, p: usize) -> &[DisplaySubstitution] {
        if Some(p) == self.override_paragraph {
            return &self.override_entries;
        }
        let start = self.first_in_paragraph[p];
        let end = self.paragraphs.end_of_paragraph_at(p);
        let mut i = start;
        while i < self.base.len() && self.base[i].source_range.location < end {
            i += 1;
        }
        &self.base[start..i]
    }

    // MARK: Source → TextKit

    /// Total function. Source offsets strictly inside a substitution collapse
    /// onto the TextKit offset of that substitution's start.
    pub fn text_kit_offset_for_source(&self, source: isize) -> isize {
        let s = self.clamp_source(source);
        let p = self.paragraphs.index_containing(s);
        let start = self.paragraphs.starts[p];
        let mut cursor = start;
        let mut display = 0isize;
        for sub in self.entries_in_paragraph(p) {
            if sub.source_range.location >= s {
                break;
            }
            display += sub.source_range.location - cursor;
            if s >= sub.source_range.upper_bound() {
                display += sub.display_length;
                cursor = sub.source_range.upper_bound();
            } else {
                if sub.preserves_source_offsets {
                    return start + display + (s - sub.source_range.location);
                }
                return start + display;
            }
        }
        start + display + (s - cursor)
    }

    /// Endpoints are converted independently: lengths are meaningless in the
    /// TextKit space.
    pub fn text_kit_range_for_source(&self, range: NSRange) -> NSRange {
        let a = self.text_kit_offset_for_source(range.location);
        let b = self.text_kit_offset_for_source(range.upper_bound());
        NSRange::new(a, 0.max(b - a))
    }

    // MARK: TextKit → Source

    /// Exact right inverse of `text_kit_offset_for_source`. Where several
    /// source offsets share a TextKit offset, a *hidden* run resolves to the
    /// offset after it (§6.1b); a positive-length replacement resolves to its
    /// start.
    pub fn source_offset_for_text_kit(&self, text_kit: isize) -> isize {
        let t = self.clamp_source(text_kit);
        let p = self.paragraphs.index_containing(t);
        let paragraph_end = self.paragraphs.end_of_paragraph_at(p);
        let mut cursor = self.paragraphs.starts[p];
        let mut remaining = t - cursor;
        for sub in self.entries_in_paragraph(p) {
            let visible = sub.source_range.location - cursor;
            if remaining < visible {
                return cursor + remaining;
            }
            remaining -= visible;
            if remaining < sub.display_length {
                // Hidden runs may carry same-length joiners for layout safety.
                // Resolve past them so typing never lands inside a marker the
                // user cannot see (§6.1b).
                if sub.is_hidden {
                    return sub.source_range.upper_bound();
                }
                return if sub.preserves_source_offsets {
                    sub.source_range.location + remaining
                } else {
                    sub.source_range.location
                };
            }
            remaining -= sub.display_length;
            cursor = sub.source_range.upper_bound();
        }
        // Landing exactly on a paragraph break puts us at the start of the next
        // paragraph, which may itself open with a hidden run. Resolve forward
        // past it so the two spellings of the break agree (§6.1a).
        let mut result = (cursor + remaining).min(paragraph_end);
        while let Some(next) = self.substitution_starting_at(result) {
            if !next.is_hidden {
                break;
            }
            result = next.source_range.upper_bound();
        }
        result
    }

    /// Mirror of `source_offset_for_text_kit` for the *end* of a range.
    pub fn source_upper_bound_for_text_kit(&self, text_kit: isize) -> isize {
        let t = self.clamp_source(text_kit);
        let p = self.paragraphs.index_containing(t);
        let paragraph_end = self.paragraphs.end_of_paragraph_at(p);
        let mut cursor = self.paragraphs.starts[p];
        let mut remaining = t - cursor;
        for sub in self.entries_in_paragraph(p) {
            let visible = sub.source_range.location - cursor;
            // `<=` rather than `<`: stop *before* the run instead of after it.
            if remaining <= visible {
                return cursor + remaining;
            }
            remaining -= visible;
            if remaining <= sub.display_length {
                // Selection ends stop *before* a hidden run, including layout
                // fillers, so ⌘C never picks up invisible marker characters.
                if sub.is_hidden {
                    return sub.source_range.location;
                }
                return if sub.preserves_source_offsets {
                    sub.source_range.location + remaining
                } else {
                    sub.source_range.upper_bound()
                };
            }
            remaining -= sub.display_length;
            cursor = sub.source_range.upper_bound();
        }
        (cursor + remaining).min(paragraph_end)
    }

    /// The location resolves forward and the upper bound backward, so a
    /// selection covers exactly the source the user can see.
    pub fn source_range_for_text_kit(&self, range: NSRange) -> NSRange {
        let a = self.source_offset_for_text_kit(range.location);
        if range.length <= 0 {
            return NSRange::new(a, 0);
        }
        let b = self.source_upper_bound_for_text_kit(range.upper_bound());
        NSRange::new(a, 0.max(b - a))
    }

    /// A source offset is *canonical* when it survives a source → TextKit →
    /// source round trip.
    pub fn is_canonical(&self, source: isize) -> bool {
        let s = self.clamp_source(source);
        self.source_offset_for_text_kit(self.text_kit_offset_for_source(s)) == s
    }

    /// Substitution whose source range ends exactly at `offset`, if any.
    pub fn substitution_ending_at(&self, offset: isize) -> Option<&DisplaySubstitution> {
        let p = self.paragraphs.index_containing(0.max(offset.min(self.paragraphs.length)));
        if let Some(sub) = self
            .entries_in_paragraph(p)
            .iter()
            .find(|sub| sub.source_range.upper_bound() == offset)
        {
            return Some(sub);
        }
        // A run ending at a paragraph's first offset belongs to the previous one.
        if p == 0 {
            return None;
        }
        self.entries_in_paragraph(p - 1)
            .iter()
            .find(|sub| sub.source_range.upper_bound() == offset)
    }

    /// Substitution whose source range starts exactly at `offset`, if any.
    pub fn substitution_starting_at(&self, offset: isize) -> Option<&DisplaySubstitution> {
        let p = self.paragraphs.index_containing(0.max(offset.min(self.paragraphs.length)));
        self.entries_in_paragraph(p)
            .iter()
            .find(|sub| sub.source_range.location == offset)
    }

    /// One past the last TextKit offset belonging to the paragraph at `p`.
    pub fn text_kit_end_of_paragraph_at(&self, p: usize) -> isize {
        let range = self.paragraphs.range_at(p);
        let mut removed = 0isize;
        for sub in self.entries_in_paragraph(p) {
            removed += sub.source_range.length - sub.display_length;
        }
        range.location + 0.max(range.length - removed)
    }

    #[inline]
    fn clamp_source(&self, offset: isize) -> isize {
        0.max(offset.min(self.paragraphs.length))
    }

    // MARK: Display strings

    /// The substituted attributed string for a paragraph, or `None` when the
    /// paragraph is untouched and TextKit should use the storage as-is.
    pub fn display_string_for_paragraph(
        &self,
        paragraph_range: NSRange,
        storage: &NSAttributedString,
        including_hard_wrap_reflow: bool,
    ) -> Option<Retained<NSAttributedString>> {
        let p = self.paragraphs.index_containing(paragraph_range.location);
        let local: Vec<&DisplaySubstitution> = self
            .entries_in_paragraph(p)
            .iter()
            .filter(|sub| including_hard_wrap_reflow || !sub.is_hard_wrap_reflow)
            .collect();
        if local.is_empty() {
            return None;
        }
        Some(Self::splice(local, paragraph_range, storage))
    }

    /// The grouped-element counterpart to `display_string_for_paragraph`.
    pub fn display_string_for_source_range(
        &self,
        source_range: NSRange,
        storage: &NSAttributedString,
    ) -> Option<Retained<NSAttributedString>> {
        let local = self.substitutions_in(source_range);
        if local.is_empty() {
            return None;
        }
        Some(Self::splice(local.iter().collect(), source_range, storage))
    }

    fn splice(
        local: Vec<&DisplaySubstitution>,
        range: NSRange,
        storage: &NSAttributedString,
    ) -> Retained<NSAttributedString> {
        let out = NSMutableAttributedString::new();
        let mut cursor = range.location;
        for sub in local {
            if !(sub.source_range.location >= cursor && sub.source_range.upper_bound() <= range.upper_bound()) {
                continue;
            }
            if sub.source_range.location > cursor {
                out.appendAttributedString(&storage.attributedSubstringFromRange(ns_range(NSRange::new(
                    cursor,
                    sub.source_range.location - cursor,
                ))));
            }
            if let Some(replacement) = &sub.replacement {
                out.appendAttributedString(replacement);
            }
            cursor = sub.source_range.upper_bound();
        }
        if cursor < range.upper_bound() {
            out.appendAttributedString(
                &storage.attributedSubstringFromRange(ns_range(NSRange::new(cursor, range.upper_bound() - cursor))),
            );
        }
        Retained::into_super(out)
    }
}
