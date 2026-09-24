//! Port of `Fragments/InlineMathDisplay.swift`: inline math substitutions
//! built without changing the source text.
//!
//! The logical display map replaces the source span with one attachment
//! character; the layout map expands that replacement back to the source
//! span's length with word joiners. The attachment itself
//! (`MathRenderer.inlineAttachment`) lives in `upleft-math`.

use upleft_core::{BlockRef, InlineKind, NSRange, ParsedDocument};
use upleft_math::downright::inline_math_display::inline_attachment;

use crate::engine::display_map::DisplaySubstitution;
use crate::theme::style_sheet::StyleSheet;

pub struct InlineMathDisplay;

impl InlineMathDisplay {
    pub fn ranges(document: &ParsedDocument) -> Vec<NSRange> {
        InlineMathDisplay::ranges_in(std::slice::from_ref(&document.root))
    }

    /// `ranges(document:)` over `blocks` and their descendants only.
    pub fn ranges_in(blocks: &[BlockRef]) -> Vec<NSRange> {
        let mut result = Vec::new();
        let mut visit = |block: &BlockRef| {
            for span in &block.inlines {
                span.walk(&mut |inline| {
                    if matches!(inline.kind, InlineKind::InlineMath { .. }) && inline.range.length > 0 {
                        result.push(inline.range);
                    }
                });
            }
        };
        for block in blocks {
            block.walk(&mut visit);
        }
        // Swift's sort is stable, and so is this one.
        result.sort_by_key(|range| range.location);
        result
    }

    pub fn ranges_touching(document: &ParsedDocument, offset: isize) -> Vec<NSRange> {
        InlineMathDisplay::ranges(document)
            .into_iter()
            .filter(|range| range.touches(offset))
            .collect()
    }

    pub fn substitutions(
        document: &ParsedDocument,
        style_sheet: &StyleSheet,
        excluded_range: Option<NSRange>,
    ) -> Vec<DisplaySubstitution> {
        InlineMathDisplay::substitutions_for(document, InlineMathDisplay::ranges(document), style_sheet, excluded_range)
    }

    /// `substitutions` for ranges already found (`ranges_in`).
    pub fn substitutions_for(
        document: &ParsedDocument,
        ranges: Vec<NSRange>,
        style_sheet: &StyleSheet,
        excluded_range: Option<NSRange>,
    ) -> Vec<DisplaySubstitution> {
        ranges
            .into_iter()
            .filter_map(|range| {
                if let Some(excluded) = excluded_range
                    && upleft_core::ns_range::ns_intersection_range(range, excluded).length > 0
                {
                    return None;
                }
                let bounded = range.intersection(NSRange::new(0, document.length)).unwrap_or(range);
                let latex = document.substring(bounded);
                let replacement = inline_attachment(
                    &latex,
                    style_sheet.math_point_size,
                    &style_sheet.text,
                    &style_sheet.body_font(),
                )?;
                Some(DisplaySubstitution::replace(range, replacement))
            })
            .collect()
    }
}
