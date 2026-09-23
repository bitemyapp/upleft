//! Port of `Fragments/InlineMathDisplay.swift`, the document-walking half:
//! inline math ranges and their display substitutions. The attachment itself
//! (`MathRenderer.inlineAttachment`) is `upleft_math::downright::inline_math_display`.

use upleft_core::{InlineKind, NSRange, ParsedDocument};

use crate::engine::display_map::DisplaySubstitution;
use crate::theme::style_sheet::StyleSheet;

pub struct InlineMathDisplay;

impl InlineMathDisplay {
    pub fn ranges(document: &ParsedDocument) -> Vec<NSRange> {
        let mut result = Vec::new();
        document.root.walk(&mut |block| {
            for span in &block.inlines {
                span.walk(&mut |inline| {
                    if let InlineKind::InlineMath { .. } = inline.kind
                        && inline.range.length > 0
                    {
                        result.push(inline.range);
                    }
                });
            }
        });
        result.sort_by(|a, b| a.location.cmp(&b.location));
        result
    }

    /// `ranges(in:touching:)`.
    pub fn ranges_touching(document: &ParsedDocument, offset: isize) -> Vec<NSRange> {
        Self::ranges(document).into_iter().filter(|range| range.touches(offset)).collect()
    }

    pub fn substitutions(
        document: &ParsedDocument,
        style_sheet: &StyleSheet,
        excluded_range: Option<NSRange>,
    ) -> Vec<DisplaySubstitution> {
        Self::ranges(document)
            .into_iter()
            .filter_map(|range| {
                if let Some(excluded) = excluded_range
                    && upleft_core::ns_range::ns_intersection_range(range, excluded).length > 0
                {
                    return None;
                }
                let clipped = range.intersection(NSRange::new(0, document.length)).unwrap_or(range);
                let latex = document.substring(clipped);
                let replacement = upleft_math::downright::inline_math_display::inline_attachment(
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
