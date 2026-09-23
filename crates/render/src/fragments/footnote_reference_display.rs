//! Port of `Fragments/FootnoteReferenceDisplay.swift`: turns `[^12]` into one
//! semantic superscript without touching source bytes.

use objc2::runtime::AnyObject;
use objc2_foundation::{NSAttributedString, NSDictionary, NSNumber, NSString};
use upleft_core::{InlineKind, NSRange, ParsedDocument, swift_text};

use crate::engine::display_map::DisplaySubstitution;
use crate::engine::keys;
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
        result.sort_by_key(|reference| reference.range.location);
        result
    }

    pub fn substitutions(
        document: &ParsedDocument,
        style_sheet: &StyleSheet,
        excluded_range: Option<NSRange>,
    ) -> Vec<DisplaySubstitution> {
        FootnoteReferenceDisplay::references(document)
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
                let attributes = NSDictionary::from_slices(
                    &[
                        keys::font(),
                        keys::foreground_color(),
                        keys::baseline_offset(),
                        attribute_keys::dr_reference(),
                    ],
                    &[
                        font.as_ref() as &AnyObject,
                        style_sheet.accent.as_ref(),
                        offset.as_ref(),
                        identifier.as_ref(),
                    ],
                );
                // SAFETY: attribute keys to attribute values.
                let string =
                    unsafe { NSAttributedString::new_with_attributes(&NSString::from_str(&value), &attributes) };
                Some(DisplaySubstitution::replace(reference.range, string))
            })
            .collect()
    }
}

/// Maps each Character through the superscript table; an empty identifier
/// becomes a bullet.
fn superscript(identifier: &str) -> String {
    let mut converted = String::with_capacity(identifier.len() * 2);
    for character in swift_text::graphemes(identifier) {
        let mapped = match character {
            "0" => "⁰",
            "1" => "¹",
            "2" => "²",
            "3" => "³",
            "4" => "⁴",
            "5" => "⁵",
            "6" => "⁶",
            "7" => "⁷",
            "8" => "⁸",
            "9" => "⁹",
            "+" => "⁺",
            "-" => "⁻",
            other => other,
        };
        converted.push_str(mapped);
    }
    if converted.is_empty() { "•".to_owned() } else { converted }
}
