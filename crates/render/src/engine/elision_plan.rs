//! Port of `Engine/ElisionPlan.swift`.
//!
//! Structural zoom (§5.2) × heading folding (§7.1) × find (§7.2, §9.4) ×
//! hidden markers (§6.1) — the four-way interaction §14 says to specify before
//! implementing. This file *is* that specification, and it is the only place
//! the four are combined.
//!
//! THE RULE
//!
//!  1. Elision and marker hiding are orthogonal mechanisms over disjoint
//!     ranges. Markers are omitted from the display string (`DisplayMap`);
//!     elided ranges keep every character in the display string and are
//!     collapsed to zero height by `ElidedFragment`.
//!
//!  2. The elision candidates are the union of the zoom plan's elided ranges
//!     and the section range (heading line excluded) of every folded heading.
//!
//!  3. **A search hit, the caret, or a selection inside a candidate forces
//!     that whole candidate visible.** Whole, never partially.
//!
//!  4. Rule 3 is a *render-time* override, not a state change.

use std::collections::HashSet;

use upleft_core::structural_zoom::StructuralZoom;
use upleft_core::{HeadingNode, NSRange, ParsedDocument, ZoomLevel};

use super::display_map::RangeSet;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ElisionPlan {
    /// Ranges collapsed to zero height, ascending and non-overlapping.
    pub elided_ranges: Vec<NSRange>,
    /// Candidates that rule 3 forced back into view, for the "N paragraphs
    /// hidden" affordance and for tests.
    pub forced_visible_ranges: Vec<NSRange>,
}

impl ElisionPlan {
    /// `ElisionPlan.none`.
    pub fn none() -> ElisionPlan {
        ElisionPlan::new(Vec::new(), Vec::new())
    }

    pub fn new(elided_ranges: Vec<NSRange>, forced_visible_ranges: Vec<NSRange>) -> ElisionPlan {
        ElisionPlan { elided_ranges, forced_visible_ranges }
    }

    pub fn is_identity(&self) -> bool {
        self.elided_ranges.is_empty()
    }

    pub fn is_elided(&self, offset: isize) -> bool {
        RangeSet::covers(&self.elided_ranges, offset)
    }

    pub fn range_containing(&self, offset: isize) -> Option<NSRange> {
        self.elided_ranges.iter().copied().find(|range| range.contains(offset))
    }

    // MARK: - Construction

    pub fn make(
        document: &ParsedDocument,
        zoom: ZoomLevel,
        folded_heading_slugs: &HashSet<String>,
        search_hits: &[NSRange],
        caret: Option<isize>,
        selections: &[NSRange],
    ) -> ElisionPlan {
        let mut candidates: Vec<NSRange> = Vec::new();

        if zoom != ZoomLevel::Everything {
            candidates.extend(StructuralZoom::plan(document, zoom).elided_ranges);
        }
        if !folded_heading_slugs.is_empty() {
            for heading in &document.headings {
                if folded_heading_slugs.contains(&heading.slug) {
                    let body = ElisionPlan::body_range(heading);
                    if body.length > 0 {
                        candidates.push(body);
                    }
                }
            }
        }
        let candidates = RangeSet::normalized(&candidates);
        if candidates.is_empty() {
            return ElisionPlan::none();
        }

        let mut probes: Vec<NSRange> = search_hits.to_vec();
        if let Some(caret) = caret {
            probes.push(NSRange::new(caret, 0));
        }
        probes.extend_from_slice(selections);
        if probes.is_empty() {
            return ElisionPlan::new(candidates, Vec::new());
        }

        let mut kept = Vec::new();
        let mut forced = Vec::new();
        for candidate in candidates {
            if probes.iter().any(|probe| probe_touches(*probe, candidate)) {
                forced.push(candidate);
            } else {
                kept.push(candidate);
            }
        }
        ElisionPlan::new(kept, forced)
    }

    /// A folded heading hides its body, never its own line — otherwise there is
    /// nothing left to click to unfold (§7.1).
    pub fn body_range(heading: &HeadingNode) -> NSRange {
        let start = heading.range.upper_bound().max(heading.section_range.location);
        let end = heading.section_range.upper_bound();
        NSRange::new(start, 0.max(end - start))
    }
}

fn probe_touches(probe: NSRange, candidate: NSRange) -> bool {
    if probe.length == 0 {
        return candidate.contains(probe.location);
    }
    probe.location < candidate.upper_bound() && candidate.location < probe.upper_bound()
}
