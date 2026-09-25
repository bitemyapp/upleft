//! The display-map producers of `View/MarkdownTextView.swift`:
//! `rebuildBaseDisplayMap(document:)` and the private helpers it calls —
//! `safeHTMLLineBreakSubstitutions`, `removingOverlaps`,
//! `containsHardWrappedParagraph`, `layoutDisplayMap(from:)`,
//! `layoutSubstitution`, `layoutFiller` and `wordJoiners`.
//!
//! They are pure functions of the view's state, so they are ported here as
//! free functions over that state; the text view calls
//! [`rebuild_base_display_map`] where Swift calls its method and stores the
//! result in the same five properties.

use std::collections::HashMap;

use objc2::rc::Retained;
use objc2_foundation::{NSAttributedString, NSMutableAttributedString, NSString};
use upleft_core::safe_html::SafeHTMLKind;
use upleft_core::{BlockContent, BlockRef, NSRange, ParsedDocument};

use crate::engine::decoration_engine::DecorationEngine;
use crate::engine::display_map::{DisplayMap, DisplaySubstitution, ParagraphIndex, RangeSet};
use crate::engine::hard_wrap_reflow::{HardWrapReflow, Plan};
use crate::engine::{keys, ns_range};
use crate::fragments::footnote_reference_display::FootnoteReferenceDisplay;
use crate::fragments::inline_math_display::InlineMathDisplay;
use crate::render_contracts::{DecorationPolicy, SourceFocus};
use crate::theme::style_sheet::StyleSheet;

/// What `rebuildBaseDisplayMap(document:)` assigns.
#[derive(Debug, Clone)]
pub struct BaseDisplayMaps {
    pub base_hidden_ranges: Vec<NSRange>,
    pub hard_wrap_ranges: Vec<NSRange>,
    pub hard_wrap_substitutions: Vec<DisplaySubstitution>,
    pub base_display_map: DisplayMap,
    /// Also what `displayMap` is set to.
    pub base_layout_map: DisplayMap,
}

/// The view state `rebuildBaseDisplayMap(document:)` reads.
pub struct BaseDisplayMapInputs<'a> {
    pub document: &'a ParsedDocument,
    /// Supplies `hiddenRanges(document:caret:selections:)` under its policy.
    pub engine: &'a DecorationEngine,
    pub effective_policy: DecorationPolicy,
    pub source_focus: SourceFocus,
    pub reflow_hard_wrapped_paragraphs: bool,
    pub style_sheet: &'a StyleSheet,
    /// The text view's storage (`textStorage`).
    pub storage: &'a NSAttributedString,
    pub paragraph_index: &'a ParagraphIndex,
}

/// `wordJoinerRuns`: U+2060 runs by length, built once each.
#[derive(Debug, Default)]
pub struct WordJoinerRuns {
    runs: HashMap<isize, Retained<NSString>>,
}

impl WordJoinerRuns {
    pub fn word_joiners(&mut self, length: isize) -> Retained<NSString> {
        if let Some(cached) = self.runs.get(&length) {
            return cached.clone();
        }
        let run: String = std::iter::repeat_n('\u{2060}', 0.max(length) as usize).collect();
        let run = NSString::from_str(&run);
        self.runs.insert(length, run.clone());
        run
    }
}

/// `rebuildBaseDisplayMap(document:)`.
pub fn rebuild_base_display_map(inputs: &BaseDisplayMapInputs, joiners: &mut WordJoinerRuns) -> BaseDisplayMaps {
    let document = inputs.document;
    let focus = inputs.source_focus.range();
    let intersects_focus =
        |range: NSRange| focus.is_some_and(|focus| upleft_core::ns_range::ns_intersection_range(range, focus).length > 0);

    let mut hidden = inputs.engine.hidden_ranges(document, None, &[]);
    if focus.is_some() {
        hidden.retain(|range| !intersects_focus(*range));
    }
    let line_break_substitutions = if inputs.effective_policy.renders_fragments {
        safe_html_line_break_substitutions(document, focus)
    } else {
        Vec::new()
    };
    if !line_break_substitutions.is_empty() {
        let exclusions: Vec<NSRange> = line_break_substitutions.iter().map(|sub| sub.source_range).collect();
        hidden = removing_overlaps(hidden, &exclusions);
    }

    let math_ranges: Vec<NSRange> = if inputs.effective_policy.renders_fragments {
        InlineMathDisplay::ranges(document)
            .into_iter()
            .filter(|range| focus.is_none() || !intersects_focus(*range))
            .collect()
    } else {
        Vec::new()
    };
    // The math replacement owns its delimiters and content as one visual
    // object.
    hidden = removing_overlaps(hidden, &math_ranges);
    let math_substitutions = if inputs.effective_policy.renders_fragments {
        InlineMathDisplay::substitutions(document, inputs.style_sheet, focus)
    } else {
        Vec::new()
    };

    let footnote_references: Vec<NSRange> = if inputs.effective_policy.hides_inline_markers {
        FootnoteReferenceDisplay::references(document)
            .into_iter()
            .filter(|reference| focus.is_none() || !intersects_focus(reference.range))
            .map(|reference| reference.range)
            .collect()
    } else {
        Vec::new()
    };
    hidden = removing_overlaps(hidden, &footnote_references);
    let footnote_substitutions = if inputs.effective_policy.hides_inline_markers {
        FootnoteReferenceDisplay::substitutions(document, inputs.style_sheet, focus)
    } else {
        Vec::new()
    };

    let base_hidden_ranges = hidden.clone();
    let plan = if inputs.reflow_hard_wrapped_paragraphs {
        let source = utf16_of(&inputs.storage.string());
        if contains_hard_wrapped_paragraph(document, &source) {
            HardWrapReflow::plan(document, &source, &hidden, &[], true)
        } else {
            Plan::default()
        }
    } else {
        // Most generated large documents use one physical line per paragraph;
        // skipping the plan avoids a whole-document walk that cannot publish.
        Plan::default()
    };
    let hard_wrap_ranges = plan.ranges;
    let hard_wrap_substitutions: Vec<DisplaySubstitution> =
        plan.substitutions.into_iter().filter(|sub| sub.is_hard_wrap_reflow).collect();

    let mut substitutions: Vec<DisplaySubstitution> = hidden.into_iter().map(DisplaySubstitution::hide).collect();
    substitutions.extend(math_substitutions);
    substitutions.extend(footnote_substitutions);
    substitutions.extend(line_break_substitutions);
    substitutions.extend(hard_wrap_substitutions.iter().cloned());
    let base_display_map = DisplayMap::new(inputs.paragraph_index.clone(), substitutions);
    // Selection / hit-testing speak TextKit coordinates from the layout map
    // (length-preserving joiners).
    let base_layout_map = layout_display_map(&base_display_map, inputs.paragraph_index, inputs.storage, joiners);
    BaseDisplayMaps {
        base_hidden_ranges,
        hard_wrap_ranges,
        hard_wrap_substitutions,
        base_display_map,
        base_layout_map,
    }
}

fn utf16_of(string: &NSString) -> Vec<u16> {
    let length = string.length();
    let mut units = vec![0u16; length];
    if length > 0 {
        // SAFETY: `units` holds `length` code units.
        unsafe {
            string.getCharacters_range(
                std::ptr::NonNull::new(units.as_mut_ptr()).unwrap(),
                objc2_foundation::NSRange::new(0, length),
            );
        }
    }
    units
}

/// `containsHardWrappedParagraph(in:text:)`: whether any paragraph's source
/// holds a character of `CharacterSet.newlines`.
pub fn contains_hard_wrapped_paragraph(document: &ParsedDocument, text: &[u16]) -> bool {
    contains_hard_wrapped_paragraph_in(std::slice::from_ref(&document.root), text)
}

/// `containsHardWrappedParagraph(in:text:)` over `blocks` and their
/// descendants only.
pub fn contains_hard_wrapped_paragraph_in(blocks: &[BlockRef], text: &[u16]) -> bool {
    let mut found = false;
    let mut visit = |block: &BlockRef| {
        if found || !matches!(block.content, BlockContent::Paragraph) || block.range.length <= 0 {
            return;
        }
        // `rangeOfCharacter(from:options:range:)` over the block's range.
        let lo = block.range.location.max(0) as usize;
        let hi = (block.range.upper_bound().max(0) as usize).min(text.len());
        found = text[lo.min(hi)..hi]
            .iter()
            .any(|&unit| (0x0A..=0x0D).contains(&unit) || unit == 0x85 || unit == 0x2028 || unit == 0x2029);
    };
    for block in blocks {
        block.walk(&mut visit);
    }
    found
}

/// `safeHTMLLineBreakSubstitutions(in:excluding:)`: source-preserving
/// display replacements for README tags that break lines.
pub fn safe_html_line_break_substitutions(document: &ParsedDocument, excluded: Option<NSRange>) -> Vec<DisplaySubstitution> {
    safe_html_line_break_substitutions_in(std::slice::from_ref(&document.root), excluded)
}

/// `safeHTMLLineBreakSubstitutions(in:excluding:)` over `blocks` and their
/// descendants only.
pub fn safe_html_line_break_substitutions_in(blocks: &[BlockRef], excluded: Option<NSRange>) -> Vec<DisplaySubstitution> {
    let mut substitutions = Vec::new();
    let mut visit = |block: &BlockRef| {
        let Some(html) = &block.safe_html else { return };
        if !html.is_safe {
            return;
        }
        for annotation in &html.annotations {
            let (range, text) = match annotation.kind {
                SafeHTMLKind::LineBreak => {
                    if annotation.range.length <= 0 {
                        continue;
                    }
                    (annotation.range, "\n")
                }
                SafeHTMLKind::Details { open } => {
                    // Only the opening tag: a stable native disclosure marker.
                    let Some(opening) = annotation.tag_ranges.first() else { continue };
                    if opening.length <= 0 {
                        continue;
                    }
                    (*opening, if open { "▾" } else { "▸" })
                }
                SafeHTMLKind::TableRow => {
                    // A break on the closing tag keeps rows from concatenating.
                    let Some(closing) = annotation.tag_ranges.last() else { continue };
                    if closing.length <= 0 {
                        continue;
                    }
                    (*closing, "\n")
                }
                _ => continue,
            };
            if let Some(excluded) = excluded
                && upleft_core::ns_range::ns_intersection_range(range, excluded).length > 0
            {
                continue;
            }
            let mut replacement = String::from(text);
            for _ in 0..range.length - 1 {
                replacement.push('\u{200B}');
            }
            let replacement = NSAttributedString::from_nsstring(&NSString::from_str(&replacement));
            substitutions.push(DisplaySubstitution::new(range, range.length, Some(replacement), false, false, true));
        }
    };
    for block in blocks {
        block.walk(&mut visit);
    }
    substitutions.sort_by_key(|sub| sub.source_range.location);
    substitutions
}

/// `removingOverlaps(_:with:)`: drops ranges intersecting an ordered exclusion
/// set, in one merge pass.
pub fn removing_overlaps(ranges: Vec<NSRange>, exclusions: &[NSRange]) -> Vec<NSRange> {
    if ranges.is_empty() || exclusions.is_empty() {
        return ranges;
    }
    let mut ordered_ranges = ranges;
    ordered_ranges.sort_by_key(|range| range.location);
    let mut ordered_exclusions = exclusions.to_vec();
    ordered_exclusions.sort_by_key(|range| range.location);
    let mut kept = Vec::with_capacity(ordered_ranges.len());
    let mut exclusion_index = 0usize;
    for range in ordered_ranges {
        while exclusion_index < ordered_exclusions.len()
            && ordered_exclusions[exclusion_index].upper_bound() <= range.location
        {
            exclusion_index += 1;
        }
        let overlaps = exclusion_index < ordered_exclusions.len()
            && ordered_exclusions[exclusion_index].location < range.upper_bound()
            && ordered_exclusions[exclusion_index].upper_bound() > range.location;
        if !overlaps {
            kept.push(range);
        }
    }
    kept
}

/// `layoutDisplayMap(from:)`: hidden runs become zero-width word joiners so
/// TextKit 2 elements keep source-length ranges.
pub fn layout_display_map(
    logical: &DisplayMap,
    paragraph_index: &ParagraphIndex,
    storage: &NSAttributedString,
    joiners: &mut WordJoinerRuns,
) -> DisplayMap {
    let substitutions = logical
        .substitutions()
        .iter()
        .map(|sub| layout_substitution(sub, storage, joiners))
        .collect();
    DisplayMap::new(paragraph_index.clone(), substitutions)
}

/// `layoutSubstitution(_:)`: one logical substitution in layout space.
pub fn layout_substitution(
    substitution: &DisplaySubstitution,
    storage: &NSAttributedString,
    joiners: &mut WordJoinerRuns,
) -> DisplaySubstitution {
    // Upleft: a footnote's superscript is text, not an attachment, and is
    // shorter than its `[^id]` too. Left short, its element is shorter than
    // its range, and TextKit, laying the paragraph out again (a new width,
    // an invalidation), drops the space after it that a first layout gives.
    let inline_object = !substitution.is_hidden
        && substitution.replacement.as_ref().is_some_and(|replacement| {
            (replacement.length() as isize) < substitution.source_range.length
                && (has_attachment_at_start(replacement) || is_footnote_reference(replacement))
        });
    if !inline_object {
        if !substitution.is_hidden {
            return substitution.clone();
        }
        let length = substitution.source_range.length;
        return DisplaySubstitution::new(
            substitution.source_range,
            length,
            Some(layout_filler(length, substitution.source_range.location, storage, joiners)),
            true,
            false,
            true,
        );
    }
    let replacement = substitution.replacement.as_ref().expect("checked above");
    let filler_count = substitution.source_range.length - replacement.length() as isize;
    let layout_replacement = NSMutableAttributedString::from_attributed_nsstring(replacement);
    layout_replacement.appendAttributedString(&layout_filler(
        filler_count,
        substitution.source_range.location,
        storage,
        joiners,
    ));
    DisplaySubstitution::new(
        substitution.source_range,
        substitution.source_range.length,
        Some(Retained::into_super(layout_replacement)),
        false,
        false,
        true,
    )
}

fn is_footnote_reference(string: &NSAttributedString) -> bool {
    string.length() > 0
        // SAFETY: reading an attribute value; the effective range is not asked for.
        && unsafe {
            string.attribute_atIndex_effectiveRange(
                crate::render_contracts::attribute_keys::dr_reference(),
                0,
                std::ptr::null_mut(),
            )
        }
        .is_some()
}

fn has_attachment_at_start(string: &NSAttributedString) -> bool {
    // SAFETY: reading an attribute value; the effective range is not asked for.
    unsafe { string.attribute_atIndex_effectiveRange(keys::attachment(), 0, std::ptr::null_mut()) }.is_some()
}

/// `layoutFiller(length:styledLike:)`: a run of `length` word joiners
/// carrying the storage's own attributes at `offset`.
pub fn layout_filler(
    length: isize,
    offset: isize,
    storage: &NSAttributedString,
    joiners: &mut WordJoinerRuns,
) -> Retained<NSAttributedString> {
    let run = joiners.word_joiners(length);
    if !(offset >= 0 && offset < storage.length() as isize) {
        return NSAttributedString::from_nsstring(&run);
    }
    let styled = NSMutableAttributedString::from_attributed_nsstring(
        &storage.attributedSubstringFromRange(ns_range(NSRange::new(offset, 1))),
    );
    styled.replaceCharactersInRange_withString(objc2_foundation::NSRange::new(0, 1), &run);
    Retained::into_super(styled)
}

// MARK: - Hosted incremental rebuild (Upleft extension)

/// What a hosted view's base display map was built from, for reuse after
/// an edit (`MarkdownTextView::new_hosted`, docs/EMBEDDING.md).
///
/// Every producer `rebuild_base_display_map` merges is local to a top-level
/// block: marker ranges, safe-HTML line breaks, inline math, footnote
/// references and hard-wrap groups all lie inside the block (a group reaches
/// only its own trailing separator and line prefix). The only cross-block
/// inputs are the reference and footnote definitions, which are cheap and
/// produced again every time. So after an edit that leaves everything before
/// some offset unchanged — an append, above all — each producer's output for
/// the top-level blocks before that offset is kept, the rest is produced
/// again, and the merge runs over the concatenation exactly as the full
/// build does. The layout map's fillers, the expensive part, are reused the
/// same way.
#[derive(Default)]
pub struct HostedBaseMap {
    /// The inputs the kept outputs were produced under.
    key: Option<HostedBaseKey>,
    /// Each producer's output for the whole document, in document order.
    block_hidden: Vec<NSRange>,
    line_breaks: Vec<DisplaySubstitution>,
    math_ranges: Vec<NSRange>,
    math_substitutions: Vec<DisplaySubstitution>,
    footnote_ranges: Vec<NSRange>,
    footnote_substitutions: Vec<DisplaySubstitution>,
    /// Start of the first paragraph with a line break inside it.
    first_hard_wrapped: Option<isize>,
    hard_wrap_ranges: Vec<NSRange>,
    /// Only the reflow substitutions, as `rebuild_base_display_map` keeps.
    hard_wrap_substitutions: Vec<DisplaySubstitution>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct HostedBaseKey {
    policy: DecorationPolicy,
    reflow: bool,
    style_token: i64,
}

impl HostedBaseMap {
    /// `rebuild_base_display_map`, reusing what the blocks before `floor`
    /// produced last time. `floor` is the first offset whose text or
    /// decoration may have changed; `None` rebuilds everything.
    /// `previous_layout` is the view's current base layout map.
    pub fn rebuild(
        &mut self,
        inputs: &BaseDisplayMapInputs,
        joiners: &mut WordJoinerRuns,
        floor: Option<isize>,
        previous_layout: &DisplayMap,
    ) -> BaseDisplayMaps {
        let document = inputs.document;
        if inputs.source_focus.range().is_some() {
            // Source focus filters every producer by the focused range; it is
            // rare in a hosted view, so it takes the full path.
            self.key = None;
            return rebuild_base_display_map(inputs, joiners);
        }
        let key = HostedBaseKey {
            policy: inputs.effective_policy,
            reflow: inputs.reflow_hard_wrapped_paragraphs,
            style_token: crate::fragments::fragment_base::StyleToken::of(inputs.style_sheet),
        };
        let children = &document.root.children;
        // The first top-level block the edit may have reached, and the cut
        // before which everything is kept.
        let (first_changed, cut) = match floor {
            Some(floor) if self.key == Some(key) => {
                let index = children.partition_point(|child| child.range.upper_bound() < floor);
                let cut = children.get(index).map_or(floor, |child| floor.min(child.range.location));
                (index, cut.max(0))
            }
            _ => (0, 0),
        };
        self.key = Some(key);
        let changed = &children[first_changed..];
        let kept = |range: NSRange| range.location < cut && range.upper_bound() <= cut;

        self.block_hidden.retain(|range| kept(*range));
        self.block_hidden.extend(inputs.engine.block_hidden_ranges(document, changed));
        let renders_fragments = inputs.effective_policy.renders_fragments;
        let hides_inline_markers = inputs.effective_policy.hides_inline_markers;
        self.line_breaks.retain(|sub| kept(sub.source_range));
        self.math_ranges.retain(|range| kept(*range));
        self.math_substitutions.retain(|sub| kept(sub.source_range));
        self.footnote_ranges.retain(|range| kept(*range));
        self.footnote_substitutions.retain(|sub| kept(sub.source_range));
        if renders_fragments {
            self.line_breaks.extend(safe_html_line_break_substitutions_in(changed, None));
            let math = InlineMathDisplay::ranges_in(changed);
            self.math_ranges.extend(math.iter().copied());
            self.math_substitutions.extend(InlineMathDisplay::substitutions_for(document, math, inputs.style_sheet, None));
        }
        if hides_inline_markers {
            let references = FootnoteReferenceDisplay::references_in(changed);
            self.footnote_ranges.extend(references.iter().map(|reference| reference.range));
            self.footnote_substitutions
                .extend(FootnoteReferenceDisplay::substitutions_for(references, inputs.style_sheet, None));
        }

        // The merge, as `rebuild_base_display_map` does it.
        let mut hidden = inputs.engine.definition_hidden_ranges(document);
        hidden.extend(self.block_hidden.iter().copied());
        let mut hidden = RangeSet::disjoint(&hidden);
        if !self.line_breaks.is_empty() {
            let exclusions: Vec<NSRange> = self.line_breaks.iter().map(|sub| sub.source_range).collect();
            hidden = removing_overlaps(hidden, &exclusions);
        }
        hidden = removing_overlaps(hidden, &self.math_ranges);
        hidden = removing_overlaps(hidden, &self.footnote_ranges);
        let base_hidden_ranges = hidden.clone();

        self.hard_wrap_ranges.retain(|range| kept(*range));
        self.hard_wrap_substitutions.retain(|sub| kept(sub.source_range));
        if inputs.reflow_hard_wrapped_paragraphs {
            let source = utf16_of(&inputs.storage.string());
            if self.first_hard_wrapped.is_some_and(|location| location >= cut) {
                self.first_hard_wrapped = None;
            }
            if self.first_hard_wrapped.is_none() {
                self.first_hard_wrapped = changed
                    .iter()
                    .find(|child| contains_hard_wrapped_paragraph_in(std::slice::from_ref(*child), &source))
                    .map(|child| child.range.location);
            }
            if self.first_hard_wrapped.is_some() {
                let plan = HardWrapReflow::plan_in(changed, &source, &hidden, &[], true);
                self.hard_wrap_ranges.extend(plan.ranges);
                self.hard_wrap_substitutions
                    .extend(plan.substitutions.into_iter().filter(|sub| sub.is_hard_wrap_reflow));
            }
        } else {
            self.first_hard_wrapped = None;
        }

        let mut substitutions: Vec<DisplaySubstitution> = hidden.into_iter().map(DisplaySubstitution::hide).collect();
        substitutions.extend(self.math_substitutions.iter().cloned());
        substitutions.extend(self.footnote_substitutions.iter().cloned());
        substitutions.extend(self.line_breaks.iter().cloned());
        substitutions.extend(self.hard_wrap_substitutions.iter().cloned());
        let base_display_map = DisplayMap::new(inputs.paragraph_index.clone(), substitutions);

        // Layout substitutions before the cut are the ones built last time.
        let mut reusable = previous_layout.base_substitutions().iter().peekable();
        let layout: Vec<DisplaySubstitution> = base_display_map
            .base_substitutions()
            .iter()
            .map(|sub| {
                if cut > 0 && kept(sub.source_range) {
                    while reusable.next_if(|old| old.source_range.location < sub.source_range.location).is_some() {}
                    if let Some(old) = reusable.peek()
                        && old.source_range == sub.source_range
                        && old.is_hidden == sub.is_hidden
                        && old.is_hard_wrap_reflow == sub.is_hard_wrap_reflow
                    {
                        return (*old).clone();
                    }
                }
                layout_substitution(sub, inputs.storage, joiners)
            })
            .collect();
        let base_layout_map = DisplayMap::new(inputs.paragraph_index.clone(), layout);
        BaseDisplayMaps {
            base_hidden_ranges,
            hard_wrap_ranges: self.hard_wrap_ranges.clone(),
            hard_wrap_substitutions: self.hard_wrap_substitutions.clone(),
            base_display_map,
            base_layout_map,
        }
    }
}
