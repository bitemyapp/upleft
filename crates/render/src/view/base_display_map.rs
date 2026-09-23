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
use upleft_core::{BlockContent, NSRange, ParsedDocument};

use crate::engine::decoration_engine::DecorationEngine;
use crate::engine::display_map::{DisplayMap, DisplaySubstitution, ParagraphIndex};
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
    let mut found = false;
    document.root.walk(&mut |block| {
        if found || !matches!(block.content, BlockContent::Paragraph) || block.range.length <= 0 {
            return;
        }
        // `rangeOfCharacter(from:options:range:)` over the block's range.
        let lo = block.range.location.max(0) as usize;
        let hi = (block.range.upper_bound().max(0) as usize).min(text.len());
        found = text[lo.min(hi)..hi]
            .iter()
            .any(|&unit| (0x0A..=0x0D).contains(&unit) || unit == 0x85 || unit == 0x2028 || unit == 0x2029);
    });
    found
}

/// `safeHTMLLineBreakSubstitutions(in:excluding:)`: source-preserving
/// display replacements for README tags that break lines.
pub fn safe_html_line_break_substitutions(document: &ParsedDocument, excluded: Option<NSRange>) -> Vec<DisplaySubstitution> {
    let mut substitutions = Vec::new();
    document.root.walk(&mut |block| {
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
    });
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
    let inline_object = !substitution.is_hidden
        && substitution.replacement.as_ref().is_some_and(|replacement| {
            (replacement.length() as isize) < substitution.source_range.length
                && has_attachment_at_start(replacement)
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
