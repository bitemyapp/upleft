//! Port of `Fragments/FootnoteReferenceDisplay.swift`: turns `[^12]` into one
//! semantic superscript without touching source bytes.
//!
//! Ported with the view layer because `MarkdownTextView` builds its base
//! display map from it and `FootnoteMarginView` draws from it.

use objc2::runtime::AnyObject;
use objc2_foundation::{NSNumber, NSString};
use upleft_core::{InlineKind, NSRange, ParsedDocument};

use crate::appkit_compat::{attributed_string, keys};
use crate::engine::display_map::DisplaySubstitution;
use crate::render_contracts::attribute_keys;
use crate::theme::style_sheet::StyleSheet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub range: NSRange,
    pub identifier: String,
}

pub struct FootnoteReferenceDisplay;

impl FootnoteReferenceDisplay {
    pub fn references(document: &ParsedDocument) -> Vec<Reference> {
        let mut result = Vec::new();
        document.root.walk(&mut |block| {
            for span in &block.inlines {
                span.walk(&mut |inline| {
                    if let InlineKind::FootnoteReference { identifier } = &inline.kind {
                        result.push(Reference { range: inline.range, identifier: identifier.clone() });
                    }
                });
            }
        });
        // Swift's `sorted(by:)` is not stable in general, but references come
        // out of the walk already ascending, so equal keys never swap.
        result.sort_by(|a, b| a.range.location.cmp(&b.range.location));
        result
    }

    pub fn substitutions(
        document: &ParsedDocument,
        style_sheet: &StyleSheet,
        excluded_range: Option<NSRange>,
    ) -> Vec<DisplaySubstitution> {
        Self::references(document)
            .into_iter()
            .filter_map(|reference| {
                if let Some(excluded) = excluded_range
                    && upleft_core::ns_range::ns_intersection_range(reference.range, excluded).length > 0
                {
                    return None;
                }
                let value = superscript(&reference.identifier);
                let body = style_sheet.body_font();
                let font = body.fontWithSize(body.pointSize() * 0.62);
                let offset = NSNumber::new_f64(body.xHeight() * 0.42);
                let identifier = NSString::from_str(&reference.identifier);
                // SAFETY: AppKit exports the key as an immutable global.
                let baseline = unsafe { objc2_app_kit::NSBaselineOffsetAttributeName };
                let string = attributed_string(
                    &value,
                    &[
                        (keys::font(), &*font as &AnyObject),
                        (keys::foreground_color(), &*style_sheet.accent),
                        (baseline, &*offset),
                        (attribute_keys::dr_reference(), &*identifier),
                    ],
                );
                Some(DisplaySubstitution::replace(reference.range, string))
            })
            .collect()
    }
}

fn superscript(identifier: &str) -> String {
    // Swift maps each `Character`; every mapped key is a single scalar, and
    // any other character passes through unchanged.
    let converted: String = identifier
        .chars()
        .map(|c| match c {
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            '9' => '⁹',
            '+' => '⁺',
            '-' => '⁻',
            other => other,
        })
        .collect();
    if converted.is_empty() { "•".to_owned() } else { converted }
}
