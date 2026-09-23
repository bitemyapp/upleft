//! Rust counterpart of `oracle/Sources/downright-oracle/DisplayMapDump.swift`
//! (`displaymap`): the coordinate maps built for a document and every
//! conversion through them, run-length encoded.
//!
//! The `view` part reproduces what `MarkdownTextView.update(document:dirty:)`
//! publishes for a freshly opened document with the reveal policy `.never`:
//! a wholesale decoration under the view's effective policy, the paragraph
//! index, then `rebuildBaseDisplayMap` — whose layout map is `currentDisplayMap`.

use std::ops::Range;

use objc2::runtime::AnyObject;
use objc2_foundation::{NSAttributedString, NSAttributedStringEnumerationOptions, NSDictionary, NSString};
use serde_json::Value;
use upleft_core::parser::MarkdownParser;
use upleft_core::{DirtySet, NSRange};
use upleft_render::engine::display_map::{DisplayMap, DisplaySubstitution, ParagraphIndex};
use upleft_render::engine::hard_wrap_reflow::HardWrapReflow;
use upleft_render::engine::marker_policy::MarkerPolicy;
use upleft_render::fragments::footnote_reference_display::FootnoteReferenceDisplay;
use upleft_render::fragments::inline_math_display::InlineMathDisplay;
use upleft_render::render_contracts::{RenderMode, SourceFocus};
use upleft_render::view::base_display_map::{BaseDisplayMapInputs, WordJoinerRuns, rebuild_base_display_map};

use super::decorate::{self, engine, text_storage};
use super::json::Object;
use super::{Failure, Request};

fn ranges(ranges: &[NSRange]) -> Value {
    Value::Array(ranges.iter().map(|r| decorate::range(*r)).collect())
}

fn optional_range(range: Option<NSRange>) -> Value {
    range.map_or(Value::Null, decorate::range)
}

/// Each attribute run's range and sorted keys.
fn attribute_keys(string: &NSAttributedString) -> Value {
    let runs = std::cell::RefCell::new(Vec::new());
    let block = block2::RcBlock::new(
        |attributes: std::ptr::NonNull<NSDictionary<NSString, AnyObject>>,
         range: objc2_foundation::NSRange,
         _stop: std::ptr::NonNull<objc2::runtime::Bool>| {
            // SAFETY: the enumeration hands over a live dictionary.
            let attributes = unsafe { attributes.as_ref() };
            let mut keys: Vec<String> = attributes.allKeys().iter().map(|key| key.to_string()).collect();
            keys.sort();
            runs.borrow_mut().push(
                Object::new()
                    .with("range", super::json::range(range.location, range.length))
                    .with("keys", Value::Array(keys.into_iter().map(Value::String).collect()))
                    .build(),
            );
        },
    );
    string.enumerateAttributesInRange_options_usingBlock(
        objc2_foundation::NSRange::new(0, string.length()),
        NSAttributedStringEnumerationOptions::empty(),
        &block,
    );
    drop(block);
    Value::Array(runs.into_inner())
}

fn substitution(sub: &DisplaySubstitution) -> Value {
    Object::new()
        .with("sourceRange", decorate::range(sub.source_range))
        .with("displayLength", sub.display_length as i64)
        .with("isHidden", sub.is_hidden)
        .with("isHardWrapReflow", sub.is_hard_wrap_reflow)
        .with("preservesSourceOffsets", sub.preserves_source_offsets)
        .with(
            "replacement",
            sub.replacement.as_ref().map_or(Value::Null, |replacement| {
                Object::new()
                    .with("string", replacement.string().to_string())
                    .with(
                        "attributes",
                        if sub.is_hidden { attribute_keys(replacement) } else { decorate::storage(replacement) },
                    )
                    .build()
            }),
        )
        .build()
}

/// `[offset, f(offset) - offset]` wherever the difference changes.
fn runs(offsets: Range<isize>, f: impl Fn(isize) -> isize) -> Value {
    let mut out = Vec::new();
    let mut last = None;
    for offset in offsets {
        let delta = f(offset) - offset;
        if Some(delta) != last {
            out.push(Value::Array(vec![(offset as i64).into(), (delta as i64).into()]));
            last = Some(delta);
        }
    }
    Value::Array(out)
}

/// `[offset, value]` wherever a Boolean changes.
fn flags(offsets: Range<isize>, f: impl Fn(isize) -> bool) -> Value {
    let mut out = Vec::new();
    let mut last = None;
    for offset in offsets {
        let value = f(offset);
        if Some(value) != last {
            out.push(Value::Array(vec![(offset as i64).into(), value.into()]));
            last = Some(value);
        }
    }
    Value::Array(out)
}

fn conversions(map: &DisplayMap, around: Option<NSRange>) -> Value {
    let length = map.paragraphs.length;
    let (offsets, paragraph_indices) = if let Some(paragraph) = around {
        let index = map.paragraphs.index_containing(paragraph.location);
        (
            0.max(paragraph.location - 1)..(length + 2).min(paragraph.upper_bound() + 2),
            index..index + 1,
        )
    } else {
        (0..length + 2, 0..map.paragraphs.starts.len())
    };
    Object::new()
        .with("isIdentity", map.is_identity())
        .with("textKit", runs(offsets.clone(), |s| map.text_kit_offset_for_source(s)))
        .with("source", runs(offsets.clone(), |t| map.source_offset_for_text_kit(t)))
        .with("upper", runs(offsets.clone(), |t| map.source_upper_bound_for_text_kit(t)))
        .with("canonical", flags(offsets, |s| map.is_canonical(s)))
        .with(
            "textKitEnds",
            Value::Array(
                paragraph_indices
                    .map(|p| (map.text_kit_end_of_paragraph_at(p) as i64).into())
                    .collect(),
            ),
        )
        .build()
}

/// `displaymap <file.md> <out.json> [--mode M] [--theme NAME] [--dark]`.
pub fn run(request: &Request) -> Result<(), Failure> {
    let text = super::markup::read_text(&request.input)?;
    let mut engine = engine(request)?;
    let document = MarkdownParser::parse(&text);
    let units = &document.utf16;
    let length = units.len() as isize;
    let mode = RenderMode::from_raw_value(&request.mode).expect("mode validated by Request::parse");
    let policy = mode.policy();
    let paragraphs = ParagraphIndex::from_utf16(units);

    let hidden = MarkerPolicy::hidden_ranges(&document, policy, None, &[]);
    let logical = DisplayMap::with_hidden(paragraphs.clone(), &hidden);

    let mut carets = Vec::new();
    for index in 1..=8isize {
        let caret = length * index / 8;
        let selections = [NSRange::new(caret, 0)];
        let revealed = MarkerPolicy::revealed_marker_ranges(&document, policy, Some(caret), &selections);
        let caret_hidden = MarkerPolicy::hidden_ranges(&document, policy, Some(caret), &selections);
        let overridden = logical.replacing_paragraph_excluding(caret, &revealed);
        carets.push(
            Object::new()
                .with("caret", caret as i64)
                .with("revealed", ranges(&revealed))
                .with("hidden", ranges(&caret_hidden))
                .with("paragraphHidden", ranges(&logical.hidden_ranges_in_paragraph_containing(caret)))
                .with("ending", optional_range(logical.substitution_ending_at(caret).map(|s| s.source_range)))
                .with("starting", optional_range(logical.substitution_starting_at(caret).map(|s| s.source_range)))
                .with(
                    "overridden",
                    conversions(&overridden, Some(paragraphs.paragraph_range_containing(caret))),
                )
                .build(),
        );
    }

    let plan = HardWrapReflow::plan(&document, units, &hidden, &[], true);
    let footnotes = FootnoteReferenceDisplay::references(&document);

    // The view: `configuration.revealPolicy = .never`, `mode`, then
    // `update(document:dirty: .wholesale)`.
    let mut effective = policy;
    effective.reveals_at_caret = false;
    effective.reveals_at_all_cursors = false;
    engine.set_policy(effective);
    let storage = text_storage(&text);
    engine.decorate(&storage, &document, &DirtySet::wholesale());
    let paragraph_index = ParagraphIndex::from_text(&storage.string());
    let style_sheet = engine.style_sheet().clone();
    let source_focus = if mode == RenderMode::Source { SourceFocus::Document } else { SourceFocus::None };
    let maps = rebuild_base_display_map(
        &BaseDisplayMapInputs {
            document: &document,
            engine: &engine,
            effective_policy: effective,
            source_focus,
            reflow_hard_wrapped_paragraphs: true,
            style_sheet: &style_sheet,
            storage: &storage,
            paragraph_index: &paragraph_index,
        },
        &mut WordJoinerRuns::default(),
    );
    let layout = &maps.base_layout_map;

    let value = Object::new()
        .with("length", length as i64)
        .with("paragraphs", Value::Array(paragraphs.starts.iter().map(|s| (*s as i64).into()).collect()))
        .with("hidden", ranges(&hidden))
        .with("logical", conversions(&logical, None))
        .with("carets", Value::Array(carets))
        .with(
            "hardWrap",
            Object::new()
                .with("ranges", ranges(&plan.ranges))
                .with("substitutions", Value::Array(plan.substitutions.iter().map(substitution).collect()))
                .build(),
        )
        .with("inlineMath", ranges(&InlineMathDisplay::ranges(&document)))
        .with(
            "footnotes",
            Value::Array(
                footnotes
                    .iter()
                    .map(|reference| {
                        Object::new()
                            .with("range", decorate::range(reference.range))
                            .with("identifier", reference.identifier.clone())
                            .build()
                    })
                    .collect(),
            ),
        )
        .with(
            "view",
            Object::new()
                .with("substitutions", Value::Array(layout.substitutions().iter().map(substitution).collect()))
                .with("hidden", ranges(&layout.hidden_ranges()))
                .with("conversions", conversions(layout, None))
                .build(),
        )
        .build();
    super::json::write(&value, &request.output)?;
    Ok(())
}
