//! Port of `Tests/MarkdownRenderTests/DecorationTests.swift`: the decoration
//! engine's contract — §3.1 (characters are never touched), §6.1 (the
//! source↔display map and the marker rules), §6.1a (the gutter), §14's
//! four-way interaction, and §12's keystroke budget.
//!
//! Cases that need the text view, the content storage, or a fragment
//! (`TableLayout`, `ListOrnamentFragment`, `CodeBlockFragment`,
//! `TableCellPresentation`) belong to the view port. Where a view case only
//! reads the display map `update(document:dirty:)` builds, it is ported
//! against `view::base_display_map`, which is that map's producer.

mod common;

use std::collections::HashSet;
use std::time::Instant;

use common::*;
use objc2_app_kit::{NSColor, NSFont, NSParagraphStyle};
use objc2_foundation::{NSAttributedString, NSNumber, NSString};
use upleft_core::parser::MarkdownParser;
use upleft_core::{DirtySet, NSRange, ZoomLevel};
use upleft_render::engine::display_map::{DisplayMap, DisplaySubstitution, ParagraphIndex, RangeSet};
use upleft_render::engine::elision_plan::ElisionPlan;
use upleft_render::engine::hard_wrap_reflow::HardWrapReflow;
use upleft_render::engine::keys;
use upleft_render::engine::marker_policy::MarkerPolicy;
use upleft_render::engine::render_metrics;
use upleft_render::fragments::footnote_reference_display::FootnoteReferenceDisplay;
use upleft_render::fragments::inline_math_display::InlineMathDisplay;
use upleft_render::render_contracts::{
    FragmentPayload, MarkdownRenderConfiguration, MarkdownRevealPolicy, RenderMode, SourceFocus, attribute_keys,
};
use upleft_render::view::base_display_map::{BaseDisplayMapInputs, WordJoinerRuns, rebuild_base_display_map};

/// Deliberately full of the things agents actually emit.
const SAMPLE_MARKDOWN: &str = "---
title: Sample document
author: agent
---

# Heading one

Some **bold** and *italic* and `code` and a [link](https://example.com) plus ~~strike~~ text.

## Heading two

- [ ] first task
- [x] second task
- plain item

1. numbered one
2. numbered two

> quoted line

> [!WARNING]
> A callout with **emphasis** inside it.

```swift
let x = 1
print(x)
```

| Name | Count |
|------|------:|
| a    | 1     |
| b    | 22    |

$$
E = mc^2
$$

***

Trailing paragraph mentioning `src/auth/session.ts:42` and $a^2$ inline.
";

// MARK: - §3.1 Raw text is the only source of truth

#[test]
fn decoration_never_mutates_characters_in_any_mode() {
    let document = MarkdownParser::parse(SAMPLE_MARKDOWN);
    for mode in RenderMode::ALL_CASES {
        let storage = storage(SAMPLE_MARKDOWN);
        let mut engine = engine(mode);

        engine.decorate(&storage, &document, &DirtySet::wholesale());
        assert_eq!(storage.string().to_string(), SAMPLE_MARKDOWN, "wholesale decoration changed the text in {mode:?}");

        // The incremental path has to hold the same guarantee.
        let slice = NSRange::new(0, 120.min(storage.length() as isize));
        engine.decorate(&storage, &document, &DirtySet::new(vec![slice], false));
        assert_eq!(storage.string().to_string(), SAMPLE_MARKDOWN, "incremental decoration changed the text in {mode:?}");
        assert_eq!(storage.length() as isize, length(SAMPLE_MARKDOWN));
    }
}

#[test]
fn renderer_configuration_is_bounded_and_independent_of_the_app_layer() {
    let mut configuration = MarkdownRenderConfiguration::new(true, MarkdownRevealPolicy::Never, false, true, true, 0, 5);
    assert!(configuration.show_invisibles);
    assert_eq!(configuration.reveal_policy, MarkdownRevealPolicy::Never);
    assert!(configuration.typewriter_scrolling);
    assert_eq!(configuration.code_collapse_threshold(), 1);
    configuration.set_code_collapse_threshold(99_999);
    assert_eq!(configuration.code_collapse_threshold(), 10_000);
}

#[test]
fn hard_wrapped_paragraphs_reflow_without_changing_source() {
    let source = "A physical line\ncontinues here without a blank paragraph.\n";
    let document = MarkdownParser::parse(source);
    let storage = storage(source);

    engine(RenderMode::Live).decorate(&storage, &document, &DirtySet::wholesale());

    assert_eq!(storage.string().to_string(), source);
    let paragraph = document.root.children.first().expect("paragraph");
    let style = paragraph_style(&storage, paragraph.range.location).expect("the hard-wrapped paragraph did not receive a paragraph style");
    assert_eq!(style.lineBreakMode(), objc2_app_kit::NSLineBreakMode::ByWordWrapping);
    assert_eq!(style.paragraphSpacing(), 0.0);

    let index = paragraphs(source);
    let hidden = engine(RenderMode::Live).hidden_ranges(&document, None, &[]);
    let plan = HardWrapReflow::plan(&document, &utf16(source), &hidden, &[], true);
    assert_eq!(plan.ranges.len(), 1);
    let grouped = plan.ranges[0];
    let ordinary_hidden: Vec<NSRange> = hidden
        .iter()
        .copied()
        .filter(|range| range.location < grouped.location || range.upper_bound() > grouped.upper_bound())
        .collect();
    let mut substitutions: Vec<DisplaySubstitution> = ordinary_hidden.into_iter().map(DisplaySubstitution::hide).collect();
    substitutions.extend(plan.substitutions.iter().cloned());
    let map = DisplayMap::new(index, substitutions);
    let displayed = map.display_string_for_source_range(grouped, &attributed(source)).expect("display string");
    assert_eq!(displayed.length() as isize, grouped.length);
    assert_eq!(displayed.string().to_string(), "A physical line continues here without a blank paragraph.\n");
    assert_eq!(map.hidden_ranges(), hidden);
    assert_eq!(storage.string().to_string(), source);
}

#[test]
fn explicit_markdown_breaks_stay_breaks_when_soft_wrap_reflow_is_on() {
    for source in ["one  \ntwo\n", "one\\\ntwo\n"] {
        let document = MarkdownParser::parse(source);
        let plan = HardWrapReflow::plan(&document, &utf16(source), &[], &[], true);
        assert!(plan.ranges.is_empty());
        assert!(plan.substitutions.is_empty());
    }
}

#[test]
fn hard_wrap_reflow_preserves_every_supported_line_ending_length() {
    for ending in ["\n", "\r\n", "\r", "\u{0085}", "\u{2028}", "\u{2029}"] {
        let source = format!("first{ending}second{ending}");
        let document = MarkdownParser::parse(&source);
        let plan = HardWrapReflow::plan(&document, &utf16(&source), &[], &[], true);
        let Some(&grouped) = plan.ranges.first() else {
            panic!("a supported line ending ({ending:?}) did not produce a reflowable paragraph");
        };
        let map = DisplayMap::new(paragraphs(&source), plan.substitutions.clone());
        let displayed = map.display_string_for_source_range(grouped, &attributed(&source)).expect("display string");
        assert_eq!(displayed.length() as isize, grouped.length);
        assert_eq!(map.text_kit_offset_for_source(grouped.upper_bound()), grouped.upper_bound());
    }
}

#[test]
fn hard_wrap_reflow_leaves_literal_inline_newlines_on_physical_path() {
    let source = "before `code\ninside` after\n";
    let plan = HardWrapReflow::plan(&MarkdownParser::parse(source), &utf16(source), &[], &[], true);
    assert!(plan.ranges.is_empty());
    assert!(plan.substitutions.is_empty());
}

/// `physicalTextKitFallbackStillHidesOrdinaryMarkers`, through the map
/// `ParagraphSubstitution` hands TextKit (the delegate itself is view code).
#[test]
fn physical_text_kit_fallback_still_hides_ordinary_markers() {
    let source = "**bold** tail\n";
    let index = paragraphs(source);
    let map = DisplayMap::with_hidden(index.clone(), &[NSRange::new(0, 2), NSRange::new(6, 2)]);
    let paragraph = map
        .display_string_for_paragraph(index.range_at(0), &attributed(source), true)
        .expect("the physical fallback leaked or rejected an ordinary hidden-marker paragraph");
    assert_eq!(paragraph.string().to_string(), "bold tail\n");
}

#[test]
fn decoration_applies_real_attributes() {
    let document = MarkdownParser::parse(SAMPLE_MARKDOWN);
    let storage = storage(SAMPLE_MARKDOWN);
    let sheet = style_sheet(false);
    let mut engine = engine(RenderMode::Read);
    let result = engine.decorate(&storage, &document, &DirtySet::wholesale());

    assert!(result.attribute_ranges > 0);
    assert!(result.fragment_count > 0);

    let heading_offset = find(SAMPLE_MARKDOWN, "# Heading one").location + 3;
    let heading_font = attribute(&storage, keys::font(), heading_offset).and_then(|v| v.downcast::<NSFont>().ok());
    assert!(heading_font.map_or(0.0, |font| font.pointSize()) > sheet.body_font().pointSize());
    let level = attribute(&storage, attribute_keys::dr_heading(), heading_offset).and_then(|v| v.downcast::<NSNumber>().ok());
    assert_eq!(level.map(|n| n.integerValue()), Some(1));

    let code_offset = find(SAMPLE_MARKDOWN, "print(x)").location;
    let code_font = attribute(&storage, keys::font(), code_offset).and_then(|v| v.downcast::<NSFont>().ok());
    assert_eq!(code_font.map(|font| font.isFixedPitch()), Some(true));

    let link_offset = find(SAMPLE_MARKDOWN, "[link](").location + 1;
    let link = attribute(&storage, attribute_keys::dr_link(), link_offset).and_then(|v| v.downcast::<NSString>().ok());
    assert_eq!(link.map(|s| s.to_string()).as_deref(), Some("https://example.com"));
}

// MARK: - §6.1 The source ⇄ display index map

#[test]
fn paragraph_index_handles_every_terminator() {
    let index = paragraphs("a\nb\r\nc\rd");
    assert_eq!(*index.starts, vec![0, 2, 5, 7]);
    assert_eq!(index.paragraph_range_containing(0), NSRange::new(0, 2));
    assert_eq!(index.paragraph_range_containing(4), NSRange::new(2, 3));
    assert_eq!(index.paragraph_range_containing(7), NSRange::new(7, 1));
    // The UTF-16 entry point agrees with the NSString one.
    assert_eq!(ParagraphIndex::from_utf16(&utf16("a\nb\r\nc\rd")), index);
    let unusual = "x\u{0085}y\u{2028}z\u{2029}\r";
    assert_eq!(ParagraphIndex::from_utf16(&utf16(unusual)), paragraphs(unusual));
    assert_eq!(*paragraphs(unusual).starts, vec![0, 2, 4, 6, 7]);
}

#[test]
fn hidden_runs_resolve_on_the_side_that_makes_typing_correct() {
    let text = "**bold** tail";
    let map = DisplayMap::with_hidden(paragraphs(text), &[NSRange::new(0, 2), NSRange::new(6, 2)]);

    // Source → TextKit collapses the hidden run onto its start.
    assert_eq!(map.text_kit_offset_for_source(0), 0);
    assert_eq!(map.text_kit_offset_for_source(1), 0);
    assert_eq!(map.text_kit_offset_for_source(2), 0);
    assert_eq!(map.text_kit_offset_for_source(6), 4);
    assert_eq!(map.text_kit_offset_for_source(8), 4);

    // §6.1b: at the visible start of the span the caret is *inside* the
    // emphasis, at its visible end it is outside.
    assert_eq!(map.source_offset_for_text_kit(0), 2);
    assert_eq!(map.source_offset_for_text_kit(4), 8);
    assert_eq!(map.source_offset_for_text_kit(2), 4);

    let display = map.display_string_for_paragraph(NSRange::new(0, length(text)), &attributed(text), true);
    assert_eq!(display.map(|d| d.string().to_string()).as_deref(), Some("bold tail"));
}

#[test]
fn layout_preserving_hidden_markers_still_skip_past_for_typing() {
    let text = "**bold** tail";
    let joiners = |range: NSRange| {
        DisplaySubstitution::new(
            range,
            range.length,
            Some(attributed(&"\u{2060}".repeat(range.length as usize))),
            true,
            false,
            true,
        )
    };
    let map = DisplayMap::new(paragraphs(text), vec![joiners(NSRange::new(0, 2)), joiners(NSRange::new(6, 2))]);

    assert_eq!(map.text_kit_offset_for_source(2), 2);
    assert_eq!(map.text_kit_offset_for_source(8), 8);
    assert_eq!(map.source_offset_for_text_kit(0), 2);
    assert_eq!(map.source_offset_for_text_kit(1), 2);
    assert_eq!(map.source_offset_for_text_kit(2), 2);
    assert_eq!(map.source_offset_for_text_kit(6), 8);
    assert_eq!(map.source_range_for_text_kit(NSRange::new(2, 4)), NSRange::new(2, 4));
    assert_eq!(map.source_upper_bound_for_text_kit(7), 6);
}

#[test]
fn a_selection_covers_exactly_the_source_it_looks_like() {
    let text = "**bold** tail";
    let map = DisplayMap::with_hidden(paragraphs(text), &[NSRange::new(0, 2), NSRange::new(6, 2)]);
    // Display "bold" is TextKit 0..<4.
    assert_eq!(map.source_range_for_text_kit(NSRange::new(0, 4)), NSRange::new(2, 4));
    // A caret keeps the forward rule and stays a caret.
    assert_eq!(map.source_range_for_text_kit(NSRange::new(0, 0)), NSRange::new(2, 0));
    assert_eq!(map.source_range_for_text_kit(NSRange::new(4, 0)), NSRange::new(8, 0));
    // Selecting through to the end still reaches the end.
    assert_eq!(map.source_range_for_text_kit(NSRange::new(0, 9)), NSRange::new(2, 11));
}

#[test]
fn inline_object_substitution_keeps_the_map_exact() {
    let text = "before $x^2$ after";
    let index = paragraphs(text);
    let math = NSRange::new(7, 5); // "$x^2$"
    let map = DisplayMap::new(index.clone(), vec![DisplaySubstitution::replace(math, attributed("\u{FFFC}"))]);

    assert_eq!(map.text_kit_offset_for_source(7), 7);
    assert_eq!(map.text_kit_offset_for_source(12), 8);
    // Before the object, and after it — never inside.
    assert_eq!(map.source_offset_for_text_kit(7), 7);
    assert_eq!(map.source_offset_for_text_kit(8), 12);
    let display = map.display_string_for_paragraph(index.range_at(0), &attributed(text), true);
    assert_eq!(display.map(|d| d.string().to_string()).as_deref(), Some("before \u{FFFC} after"));
}

fn has_attachment(string: &NSAttributedString) -> bool {
    attribute(string, keys::attachment(), 0).is_some()
}

#[test]
fn inline_math_produces_an_attachment_and_compacts_the_logical_display_map() {
    let source = "Euler $e^{i\\pi} + 1 = 0$ and $\\sum_{k=1}^{n} k = \\frac{n(n+1)}{2}$.\n";
    let document = MarkdownParser::parse(source);
    let substitutions = InlineMathDisplay::substitutions(&document, &style_sheet(false), None);

    assert_eq!(substitutions.len(), 2);
    for substitution in &substitutions {
        assert_eq!(substitution.display_length, 1);
        assert!(has_attachment(substitution.replacement.as_ref().unwrap()));
    }

    let removed: isize = substitutions.iter().map(|s| s.source_range.length - s.display_length).sum();
    let index = paragraphs(source);
    let map = DisplayMap::new(index.clone(), substitutions);
    let displayed = map.display_string_for_paragraph(index.range_at(0), &attributed(source), true).unwrap();
    assert_eq!(displayed.length() as isize, length(source) - removed);
}

/// `textViewInstallsInlineMathInItsLiveDisplayMap`, against the map
/// `MarkdownTextView.update` publishes (`rebuildBaseDisplayMap`'s layout map).
#[test]
fn text_view_installs_inline_math_in_its_live_display_map() {
    let source = "Euler $e^{i\\pi} + 1 = 0$ and $\\sum_{k=1}^{n} k = \\frac{n(n+1)}{2}$.\n";
    let maps = view_maps(source, RenderMode::Live, SourceFocus::None);
    let math = maps
        .base_layout_map
        .substitutions()
        .into_iter()
        .filter(|s| s.replacement.as_ref().is_some_and(|r| has_attachment(r)))
        .count();
    assert_eq!(math, 2);
}

/// What `update(document:dirty: .wholesale)` builds for `source` in `mode`.
pub fn view_maps(source: &str, mode: RenderMode, focus: SourceFocus) -> upleft_render::view::base_display_map::BaseDisplayMaps {
    let document = MarkdownParser::parse(source);
    let mut engine = engine(mode);
    let storage = storage(source);
    engine.decorate(&storage, &document, &DirtySet::wholesale());
    let index = ParagraphIndex::from_text(&storage.string());
    let sheet = engine.style_sheet().clone();
    let policy = engine.policy();
    rebuild_base_display_map(
        &BaseDisplayMapInputs {
            document: &document,
            engine: &engine,
            effective_policy: policy,
            source_focus: focus,
            reflow_hard_wrapped_paragraphs: true,
            style_sheet: &sheet,
            storage: &storage,
            paragraph_index: &index,
        },
        &mut WordJoinerRuns::default(),
    )
}

#[test]
fn display_map_round_trips_every_offset_of_a_real_document() {
    let text = SAMPLE_MARKDOWN;
    let document = MarkdownParser::parse(text);
    let hidden = engine(RenderMode::Read).hidden_ranges(&document, None, &[]);
    assert!(!hidden.is_empty(), "a document this full of markers must hide some");

    let index = paragraphs(text);
    let map = DisplayMap::with_hidden(index.clone(), &hidden);
    let sanitized = map.hidden_ranges();
    let total = length(text);

    // 1. Exact right inverse for every TextKit offset inside a paragraph.
    let last_paragraph = index.starts.len() - 1;
    for paragraph in 0..index.starts.len() {
        let start = index.starts[paragraph];
        let end = map.text_kit_end_of_paragraph_at(paragraph);
        let upper = if paragraph == last_paragraph { end } else { end - 1 };
        if upper < start {
            continue;
        }
        for text_kit in start..=upper {
            let source = map.source_offset_for_text_kit(text_kit);
            assert_eq!(map.text_kit_offset_for_source(source), text_kit, "TextKit offset {text_kit} did not survive the round trip");
        }
    }

    // 1b. A paragraph break has two TextKit spellings, and both name the
    //     same source position.
    for paragraph in 0..last_paragraph {
        let via_end = map.source_offset_for_text_kit(map.text_kit_end_of_paragraph_at(paragraph));
        let via_next_start = map.source_offset_for_text_kit(index.starts[paragraph + 1]);
        assert_eq!(via_end, via_next_start, "paragraph {paragraph}'s break resolved two ways");
        assert!(via_end >= index.starts[paragraph + 1]);
    }

    // 2. Canonical source offsets are exactly those no hidden run covers.
    for source in 0..=total {
        let covered = RangeSet::covers(&sanitized, source);
        assert_eq!(map.is_canonical(source), !covered, "offset {source}");
        if !covered {
            assert_eq!(map.source_offset_for_text_kit(map.text_kit_offset_for_source(source)), source);
        }
    }

    // 3. Monotone in both directions.
    let mut previous = -1;
    for source in 0..=total {
        let text_kit = map.text_kit_offset_for_source(source);
        assert!(text_kit >= previous);
        previous = text_kit;
    }

    // 4. Each paragraph's display string loses exactly what is hidden in it.
    let attributed = attributed(text);
    for paragraph in 0..index.starts.len() {
        let range = index.range_at(paragraph);
        let removed: isize = RangeSet::intersecting(&sanitized, range).iter().map(|r| r.length).sum();
        let substituted = map.display_string_for_paragraph(range, &attributed, true);
        assert_eq!(substituted.map_or(range.length, |s| s.length() as isize), range.length - removed);
        assert_eq!(map.text_kit_end_of_paragraph_at(paragraph), range.location + range.length - removed);
    }
}

#[test]
fn substitutions_never_cross_a_paragraph_boundary() {
    let map = DisplayMap::with_hidden(paragraphs("line one\nline two"), &[NSRange::new(5, 8)]);
    assert!(map.is_identity());
}

// MARK: - §6.1a/b Hidden ranges per mode and per caret

#[test]
fn hidden_ranges_follow_the_policy() {
    let text = "Some **bold** and *italic* text.\n";
    let document = MarkdownParser::parse(text);

    let read = engine(RenderMode::Read).hidden_ranges(&document, None, &[]);
    assert_eq!(display_text(text, &read), "Some bold and italic text.\n");

    let source = engine(RenderMode::Source).hidden_ranges(&document, None, &[]);
    assert!(source.is_empty());
    assert_eq!(display_text(text, &source), text);

    let live = engine(RenderMode::Live).hidden_ranges(&document, None, &[]);
    assert_eq!(display_text(text, &live), "Some bold and italic text.\n");
}

#[test]
fn caret_reveals_one_span_and_leaves_its_siblings_collapsed() {
    let text = "Some **bold** and *italic* text.\n";
    let document = MarkdownParser::parse(text);
    let engine = engine(RenderMode::Live);

    let inside_bold = find(text, "bold").location + 1;
    assert_eq!(display_text(text, &engine.hidden_ranges(&document, Some(inside_bold), &[])), "Some **bold** and italic text.\n");
    let inside_italic = find(text, "italic").location + 1;
    assert_eq!(display_text(text, &engine.hidden_ranges(&document, Some(inside_italic), &[])), "Some bold and *italic* text.\n");
    let in_plain = find(text, "Some").location + 1;
    assert_eq!(display_text(text, &engine.hidden_ranges(&document, Some(in_plain), &[])), "Some bold and italic text.\n");
}

#[test]
fn selection_does_not_reveal_markers() {
    let text = "Some **bold** and *italic* text.\n";
    let document = MarkdownParser::parse(text);
    let selection = find(text, "bold");
    let hidden = engine(RenderMode::Live).hidden_ranges(&document, None, &[selection]);
    assert_eq!(display_text(text, &hidden), "Some bold and italic text.\n");
    assert!(MarkerPolicy::revealed_marker_ranges(&document, RenderMode::Live.policy(), None, &[selection]).is_empty());
}

#[test]
fn revealed_ranges_are_exactly_what_the_caret_aware_run_leaves_out() {
    let text = "Some **bold** and *italic* and `code` here.\n";
    let document = MarkdownParser::parse(text);
    let policy = RenderMode::Live.policy();
    let collapsed = MarkerPolicy::hidden_ranges(&document, policy, None, &[]);

    for caret in 0..=length(text) {
        let with_caret = MarkerPolicy::hidden_ranges(&document, policy, Some(caret), &[]);
        let revealed = MarkerPolicy::revealed_marker_ranges(&document, policy, Some(caret), &[]);
        let derived: Vec<NSRange> = collapsed.iter().copied().filter(|candidate| !revealed.contains(candidate)).collect();
        assert_eq!(derived, with_caret, "caret {caret}");
    }
}

#[test]
fn all_cursor_reveal_policy_includes_secondary_insertion_cursors() {
    let text = "Some **bold** and *italic* text.\n";
    let document = MarkdownParser::parse(text);
    let mut policy = RenderMode::Live.policy();
    policy.reveals_at_all_cursors = true;

    let primary = find(text, "bold").location + 1;
    let secondary = find(text, "italic").location + 1;
    let carets = [NSRange::new(primary, 0), NSRange::new(secondary, 0)];
    let primary_only = MarkerPolicy::revealed_marker_ranges(&document, policy, Some(primary), &[]);
    let all = MarkerPolicy::revealed_marker_ranges(&document, policy, Some(primary), &carets);
    assert!(all.len() > primary_only.len());
    assert!(
        MarkerPolicy::hidden_ranges(&document, policy, Some(primary), &carets).len()
            < MarkerPolicy::hidden_ranges(&document, policy, None, &[]).len()
    );
}

#[test]
fn block_markers_are_never_revealed_inline() {
    let text = "## Heading\n\n- [ ] task\n";
    let document = MarkdownParser::parse(text);
    let engine = engine(RenderMode::Live);
    let heading_marker = NSRange::new(0, 3);
    for caret in [0, 3, 5, find(text, "task").location] {
        let hidden = engine.hidden_ranges(&document, Some(caret), &[]);
        assert!(
            hidden.iter().any(|r| r.location <= heading_marker.location && r.upper_bound() >= 2),
            "the heading marker was revealed with the caret at {caret}"
        );
    }
}

#[test]
fn callout_block_markers_stay_hidden_without_moving_source_coordinates() {
    let text = "> [!WARNING] Build carefully\n> The source stays byte-identical.\n";
    let document = MarkdownParser::parse(text);
    let callout = document.root.children.first().expect("callout");
    let marker = callout.marker_range.expect("the parser did not retain the callout marker range");
    let units = utf16(text);

    for mode in [RenderMode::Read, RenderMode::Live] {
        let hidden = engine(mode).hidden_ranges(&document, None, &[]);
        assert!(hidden.contains(&marker), "callout marker is visible in {mode:?}");
        let continuation = find(text, "> The source");
        assert!(
            hidden
                .iter()
                .any(|r| r.location <= continuation.location && r.upper_bound() >= continuation.location + 2),
            "callout continuation marker is visible in {mode:?}"
        );
        let map = DisplayMap::with_hidden(paragraphs(text), &hidden);
        assert_eq!(map.text_kit_offset_for_source(marker.location), map.text_kit_offset_for_source(marker.upper_bound()));
        assert_eq!(String::from_utf16_lossy(&units[marker.as_usize_range()]), "> [!WARNING] Build carefully");
        assert_eq!(map.source_offset_for_text_kit(map.text_kit_offset_for_source(marker.upper_bound())), marker.upper_bound());
    }
    assert!(engine(RenderMode::Source).hidden_ranges(&document, None, &[]).is_empty(), "Source Focus must retain every quote marker");
}

/// `resolvedDefinitionsStayOutOfRenderedProse`, its engine half (the
/// `drElided` and speech assertions are view code).
#[test]
fn resolved_definitions_stay_out_of_rendered_prose() {
    let text = "Body with [a link][ref] and a note[^1].\n\n[ref]: https://example.com\n[^1]: Hidden definition.\n";
    let document = MarkdownParser::parse(text);
    let rendered_hidden = engine(RenderMode::Live).hidden_ranges(&document, None, &[]);
    assert!(document.link_references.contains_key("ref"));
    assert!(document.footnotes.contains_key("1"));
    assert!(!rendered_hidden.is_empty());
    assert!(engine(RenderMode::Source).hidden_ranges(&document, None, &[]).is_empty());
    let shown = display_text(text, &rendered_hidden);
    assert!(!shown.contains("https://example.com"));
    assert!(!shown.contains("Hidden definition"));
}

#[test]
fn footnote_reference_is_one_semantic_superscript() {
    let text = "Body with a note[^12].\n\n[^12]: Margin note.\n";
    let document = MarkdownParser::parse(text);
    let substitutions = FootnoteReferenceDisplay::substitutions(&document, &style_sheet(false), None);
    assert_eq!(substitutions.len(), 1);
    let replacement = substitutions[0].replacement.as_ref().unwrap();
    assert_eq!(replacement.string().to_string(), "¹²");
    let reference = attribute(replacement, attribute_keys::dr_reference(), 0).and_then(|v| v.downcast::<NSString>().ok());
    assert_eq!(reference.map(|s| s.to_string()).as_deref(), Some("12"));
}

#[test]
fn document_gutter_uses_semantic_heading_controls_only() {
    let document = MarkdownParser::parse("## Heading\n\n> quote\n\n$$\nx\n$$\n");
    let markers = engine(RenderMode::Live).gutter_markers(&document);
    let texts: Vec<&str> = markers.iter().map(|(_, text, _)| text.as_str()).collect();
    assert_eq!(texts, ["H2"]);
}

#[test]
fn checked_tasks_have_a_quiet_completion_treatment() {
    let source = "- [x] shipped\n";
    let document = MarkdownParser::parse(source);
    let storage = storage(source);
    let mut renderer = engine(RenderMode::Read);
    renderer.decorate(&storage, &document, &DirtySet::wholesale());

    let content = find(source, "shipped");
    let strike = attribute(&storage, keys::strikethrough_style(), content.location).and_then(|v| v.downcast::<NSNumber>().ok());
    assert_eq!(strike.map(|n| n.integerValue()), Some(1));
    let completed = attribute(&storage, keys::foreground_color(), content.location).and_then(|v| v.downcast::<NSColor>().ok());
    assert_eq!(completed.map(|c| c.alphaComponent()), Some(0.55));
}

// MARK: - §6.1a Gutter markers

#[test]
fn gutter_markers_come_from_block_kinds() {
    let text = "# Title\n\n## Section\n\n- [ ] todo\n- [x] done\n- plain\n\n> quoted\n\n> [!WARNING]\n> careful\n\n```swift\nlet x = 1\n```";
    let document = MarkdownParser::parse(text);
    let markers = engine(RenderMode::Live).gutter_markers(&document);
    let texts: HashSet<&str> = markers.iter().map(|(_, text, _)| text.as_str()).collect();
    assert_eq!(texts, HashSet::from(["H1", "H2"]));
    let offsets: Vec<isize> = markers.iter().map(|(offset, _, _)| *offset).collect();
    let mut sorted = offsets.clone();
    sorted.sort();
    assert_eq!(offsets, sorted);
    assert_eq!(markers.iter().find(|(_, text, _)| text == "H2").map(|m| m.2), Some(2));
    assert_eq!(markers.iter().find(|(_, text, _)| text == "H1").map(|m| m.2), Some(1));
    assert!(markers.iter().all(|(offset, _, _)| *offset >= 0 && *offset <= length(text)));
}

#[test]
fn ordered_list_items_do_not_duplicate_their_number_in_the_rail() {
    let document = MarkdownParser::parse("1. one\n2. two\n");
    assert!(engine(RenderMode::Live).gutter_markers(&document).is_empty());
}

#[test]
fn every_gutter_marker_fits_the_rail() {
    let text = "# H1\n###### H6\n\n> quoted\n\n> [!WARNING]\n> careful\n\n> [!IMPORTANT]\n> really\n\n```swift\nlet x = 1\n```\n\n$$\nx = 1\n$$\n\n---";
    let document = MarkdownParser::parse(text);
    let sheet = style_sheet(false);
    let font = sheet.mono_font(Some(9f64.max(sheet.body_font().pointSize() * 0.62)));
    let available = render_metrics::GUTTER_WIDTH - 8.0;
    for (_, text, _) in engine(RenderMode::Live).gutter_markers(&document) {
        let width = upleft_render::engine::block_style::string_width(&text, &font);
        assert!(width <= available, "'{text}' needs {width}pt but the rail offers {available}pt");
    }
}

#[test]
fn list_ornaments_and_aligned_wraps_preserve_markdown_structure() {
    let source = "- [ ] a task with enough text to wrap under its text edge\n10. an ordered item\n";
    let document = MarkdownParser::parse(source);
    let storage = storage(source);
    engine(RenderMode::Live).decorate(&storage, &document, &DirtySet::wholesale());

    let list = document.root.children.first().unwrap();
    let task = list.children.first().unwrap();
    let ordered_list = document.root.children.last().unwrap();
    let ordered = ordered_list.children.first().unwrap();
    let task_style: objc2::rc::Retained<NSParagraphStyle> = paragraph_style(&storage, task.content_range.location).unwrap();
    let ordered_style = paragraph_style(&storage, ordered.content_range.location).unwrap();
    assert_eq!(task_style.headIndent(), task_style.firstLineHeadIndent());
    assert_eq!(ordered_style.headIndent(), ordered_style.firstLineHeadIndent());
    assert!(task_style.headIndent() > 0.0);
    assert!(ordered_style.headIndent() > 0.0);
    let payload = attribute(&storage, attribute_keys::dr_fragment(), task.range.location)
        .and_then(|v| v.downcast::<FragmentPayload>().ok())
        .unwrap();
    assert_eq!(payload.detail(), "task:unchecked");
    assert_eq!(storage.string().to_string(), source);
}

// MARK: - §14 zoom × folding × find × hidden markers

#[test]
fn a_search_hit_forces_its_elided_range_visible() {
    let text = "# Alpha\n\nAlpha body paragraph.\n\n# Beta\n\nBeta body with a needle inside.";
    let document = MarkdownParser::parse(text);
    let needle = find(text, "needle");
    let alpha_body = find(text, "Alpha body paragraph.");
    let none = HashSet::new();

    let zoomed = ElisionPlan::make(&document, ZoomLevel::H1, &none, &[], None, &[]);
    assert!(!zoomed.elided_ranges.is_empty());
    assert!(zoomed.is_elided(needle.location));
    assert!(zoomed.is_elided(alpha_body.location));

    let with_hit = ElisionPlan::make(&document, ZoomLevel::H1, &none, &[needle], None, &[]);
    assert!(!with_hit.is_elided(needle.location));
    assert!(!with_hit.forced_visible_ranges.is_empty());
    assert!(with_hit.is_elided(alpha_body.location));

    let slug = document.headings.iter().find(|h| h.title.contains("Beta")).unwrap().slug.clone();
    let folded_set = HashSet::from([slug.clone()]);
    let folded = ElisionPlan::make(&document, ZoomLevel::Everything, &folded_set, &[], None, &[]);
    assert!(folded.is_elided(needle.location));
    let folded_with_hit = ElisionPlan::make(&document, ZoomLevel::Everything, &folded_set, &[needle], None, &[]);
    assert!(!folded_with_hit.is_elided(needle.location));
    let caret_forced = ElisionPlan::make(&document, ZoomLevel::Everything, &folded_set, &[], Some(needle.location), &[]);
    assert!(!caret_forced.is_elided(needle.location));
    let selection_forced = ElisionPlan::make(&document, ZoomLevel::Everything, &folded_set, &[], None, &[needle]);
    assert!(!selection_forced.is_elided(needle.location));

    let heading = document.headings.iter().find(|h| h.slug == slug).unwrap();
    assert!(!folded.is_elided(heading.range.location));
}

#[test]
fn elision_and_marker_hiding_are_disjoint_mechanisms() {
    let text = "# Alpha\n\nBody with **bold** in it.\n";
    let document = MarkdownParser::parse(text);
    let plan = ElisionPlan::make(&document, ZoomLevel::H1, &HashSet::new(), &[], None, &[]);
    assert!(!plan.elided_ranges.is_empty());
    let hidden = engine(RenderMode::Read).hidden_ranges(&document, None, &[]);
    let map = DisplayMap::with_hidden(paragraphs(text), &hidden);
    for range in &plan.elided_ranges {
        for offset in range.location..range.upper_bound() {
            if map.is_canonical(offset) {
                assert_eq!(map.source_offset_for_text_kit(map.text_kit_offset_for_source(offset)), offset);
            }
        }
    }
}

// MARK: - §12 keystroke budget

fn p95(mut samples: Vec<f64>) -> f64 {
    samples.sort_by(|a, b| a.total_cmp(b));
    samples[(samples.len() - 1).min((samples.len() as f64 * 0.95) as usize)]
}

#[test]
fn keystroke_decoration_stays_under_the_budget() {
    let mut lines: Vec<String> = Vec::new();
    let mut section = 0;
    while lines.len() < 5000 {
        section += 1;
        lines.push(format!("## Section {section}"));
        lines.push(String::new());
        lines.push(format!("Paragraph {section} with **bold**, *italic*, `code`, and a [link](https://example.com/{section})."));
        lines.push(String::new());
        lines.push(format!("- [ ] task {section}a"));
        lines.push(format!("- [x] task {section}b"));
        lines.push(String::new());
        lines.push("```swift".into());
        lines.push(format!("let value{section} = {section}"));
        lines.push(format!("print(value{section})"));
        lines.push("```".into());
        lines.push(String::new());
    }
    let text = lines.join("\n");
    let storage = storage(&text);
    let mut engine = engine(RenderMode::Live);
    engine.decorate(&storage, &MarkdownParser::parse(&text), &DirtySet::wholesale());

    let seed = find(&text, "Paragraph 250 with").location + 10;
    let x = NSString::from_str("x");
    let mut samples = Vec::new();
    for caret in seed..seed + 100 {
        storage.replaceCharactersInRange_withString(objc2_foundation::NSRange::new(caret as usize, 0), &x);
        let document = MarkdownParser::parse(&storage.string().to_string());
        let dirty = DirtySet::new(vec![NSRange::new(caret, 1)], false);
        let started = Instant::now();
        engine.decorate(&storage, &document, &dirty);
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    let p95 = p95(samples);
    assert!(p95 < 8.0, "p95 keystroke decoration was {p95} ms, over the 8ms budget in §12");
    assert_eq!(storage.length() as isize, length(&text) + 100);
}

#[test]
fn caret_moves_rebuild_the_display_map_within_the_budget() {
    let lines: Vec<String> = (0..5000)
        .map(|i| {
            if i % 9 == 0 {
                format!("## Section {i}")
            } else {
                format!("Line {i} with **bold**, *italic*, `code`, and a [link](https://example.com/{i}).")
            }
        })
        .collect();
    let text = lines.join("\n");
    let document = MarkdownParser::parse(&text);
    let engine = engine(RenderMode::Live);
    let index = paragraphs(&text);
    let collapsed = engine.hidden_ranges(&document, None, &[]);
    let base = DisplayMap::with_hidden(index, &collapsed);
    let seed = find(&text, "Line 2500 with").location;
    let mut samples = Vec::new();
    for step in 0..100 {
        let caret = seed + step;
        let started = Instant::now();
        let revealed = MarkerPolicy::revealed_marker_ranges(&document, engine.policy(), Some(caret), &[]);
        let mut map = base.clone();
        if !revealed.is_empty() {
            map = base.replacing_paragraph_excluding(caret, &revealed);
        }
        std::hint::black_box(map.text_kit_offset_for_source(caret));
        samples.push(started.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(map.paragraphs.length, length(&text));
    }
    let ns = NSString::from_str(&text);
    let mut index_samples: Vec<f64> = (0..40)
        .map(|_| {
            let started = Instant::now();
            std::hint::black_box(ParagraphIndex::from_text(&ns));
            started.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    index_samples.sort_by(|a, b| a.total_cmp(b));
    let index_rebuild = index_samples[index_samples.len() / 2];
    let p95 = p95(samples);
    assert!(p95 < 8.0, "caret-move map rebuild was {p95} ms");
    assert!(index_rebuild < 8.0, "paragraph index rebuild was {index_rebuild} ms");
}

#[test]
fn a_paragraph_override_changes_only_that_paragraph() {
    let text = "**a** one\n**b** two\n**c** three\n";
    let index = paragraphs(text);
    let hidden: Vec<NSRange> = [0, 3, 10, 13, 20, 23].iter().map(|&l| NSRange::new(l, 2)).collect();
    let base = DisplayMap::with_hidden(index.clone(), &RangeSet::normalized(&hidden));

    let second = index.range_at(1);
    let revealed = base.replacing_paragraph(second.location, Vec::new());
    assert!(revealed.display_string_for_paragraph(second, &attributed(text), true).is_none());
    for offset in 0..=length(text) {
        if second.contains(offset) {
            continue;
        }
        assert_eq!(revealed.text_kit_offset_for_source(offset), base.text_kit_offset_for_source(offset), "offset {offset} moved");
    }
    assert_ne!(revealed.text_kit_offset_for_source(second.location + 3), base.text_kit_offset_for_source(second.location + 3));
}

#[test]
fn wholesale_decoration_of_a_large_document_is_affordable() {
    let lines: Vec<String> = (0..3000)
        .map(|i| if i % 12 == 0 { format!("## Section {i}") } else { format!("Line {i} with **bold** and `code` in it.") })
        .collect();
    let text = lines.join("\n");
    let storage = storage(&text);
    engine(RenderMode::Read).decorate(&storage, &MarkdownParser::parse(&text), &DirtySet::wholesale());
    assert_eq!(storage.string().to_string(), text);
}

// MARK: - Range utilities the rest of the engine assumes

#[test]
fn range_set_normalisation_holds_its_invariants() {
    let normalized = RangeSet::normalized(&[
        NSRange::new(10, 5),
        NSRange::new(0, 3),
        NSRange::new(2, 4),
        NSRange::new(40, 0),
    ]);
    assert_eq!(normalized, vec![NSRange::new(0, 6), NSRange::new(10, 5)]);
    assert!(RangeSet::covers(&normalized, 5));
    assert!(!RangeSet::covers(&normalized, 6));
    assert!(RangeSet::covers(&normalized, 10));
    assert!(!RangeSet::covers(&normalized, 15));
    assert_eq!(
        RangeSet::intersecting(&normalized, NSRange::new(4, 8)),
        vec![NSRange::new(4, 2), NSRange::new(10, 2)]
    );
}

// MARK: - Beyond the Swift suite: the paths the Swift tests exercise only
// through the view.

/// `SourceEditProjection`: untouched ranges survive, later ranges shift,
/// touched ranges expire.
#[test]
fn source_edit_projection_moves_what_follows_and_expires_what_was_touched() {
    use upleft_render::engine::display_map::SourceEditProjection;
    let old = paragraphs("one\ntwo\nthree\n");
    // Insert two characters inside "two".
    let projection = SourceEditProjection::new(NSRange::new(5, 0), 2, &old);
    assert_eq!(projection.invalidated_range, NSRange::new(4, 4));
    assert_eq!(projection.project(NSRange::new(0, 3)), Some(NSRange::new(0, 3)));
    assert_eq!(projection.project(NSRange::new(8, 5)), Some(NSRange::new(10, 5)));
    assert_eq!(projection.project(NSRange::new(4, 1)), None);
    assert_eq!(projection.project_unchanged(NSRange::new(4, 1)), Some(NSRange::new(4, 1)));
    assert_eq!(projection.project_unchanged(NSRange::new(6, 1)), Some(NSRange::new(8, 1)));
    assert_eq!(projection.project_container(NSRange::new(4, 4), true), Some(NSRange::new(4, 6)));
    assert_eq!(projection.project_container(NSRange::new(4, 4), false), None);
}

/// A caret-aware incremental commit in Source mode keeps its typography:
/// only a *wholesale* Source pass discards it (the `discardsTypography`
/// shortcut).
#[test]
fn only_a_wholesale_source_pass_discards_typography() {
    let text = "# Heading\n\nBody with **bold**.\n";
    let document = MarkdownParser::parse(text);
    let storage = storage(text);
    let mut engine = engine(RenderMode::Source);
    engine.decorate(&storage, &document, &DirtySet::wholesale());
    let bold = find(text, "bold").location;
    // The wholesale pass leaves the document base font in place.
    let base_font = attribute(&storage, keys::font(), bold).and_then(|v| v.downcast::<NSFont>().ok()).unwrap();
    assert_eq!(base_font.pointSize(), style_sheet(false).body_font().pointSize());
    assert!(!base_font.fontDescriptor().symbolicTraits().contains(objc2_app_kit::NSFontDescriptorSymbolicTraits::TraitBold));
    // An incremental pass over the same block applies the emphasis face.
    engine.decorate(&storage, &document, &DirtySet::new(vec![NSRange::new(bold, 1)], false));
    let font = attribute(&storage, keys::font(), bold).and_then(|v| v.downcast::<NSFont>().ok()).unwrap();
    assert!(font.fontDescriptor().symbolicTraits().contains(objc2_app_kit::NSFontDescriptorSymbolicTraits::TraitBold));
}

// MARK: - Object-fragment cases (ported with the fragments)

fn first_table(document: &upleft_core::ParsedDocument) -> upleft_core::model::TableData {
    match document.root.children.first().map(|block| &block.content) {
        Some(upleft_core::model::BlockContent::Table(data)) => data.clone(),
        _ => panic!("the parser did not produce a table"),
    }
}

#[test]
fn task_checkbox_uses_a_mac_sized_hit_target_around_the_drawn_box() {
    use objc2_core_foundation::CGPoint;
    use upleft_render::appkit_compat::RectExt;
    let hit = upleft_render::fragments::list_ornament_fragment::task_hit_rect(100.0, 40.0, 16.0);
    assert_eq!(hit.width(), 28.0);
    assert_eq!(hit.height(), 28.0);
    assert!(hit.contains_point(CGPoint::new(86.0, 40.0)));
    assert!(!hit.contains_point(CGPoint::new(101.0, 40.0)));
}

#[test]
fn wide_tables_stay_inside_the_text_measure_and_wrap_cells() {
    use upleft_render::fragments::table_fragment::TableLayout;
    let source = "| Alpha | Beta | Gamma | Delta |\n| --- | --- | --- | --- |\n| A long value that must wrap | another long value | third value | fourth value |";
    let document = MarkdownParser::parse(source);
    let data = first_table(&document);
    let storage = attributed(source);
    let layout = TableLayout::make(&data, &storage, 220.0, &style_sheet(false));
    let gaps = render_metrics::TABLE_COLUMN_GAP * (data.column_count() - 1) as f64;
    assert!(layout.total_width <= 220.0);
    assert!(layout.column_widths.iter().fold(0.0, |a, b| a + b) + gaps <= 220.001);
    assert!(layout.is_stacked);
    assert!(layout.row_heights.last().copied().unwrap_or(0.0) > style_sheet(false).line_height);
}

#[test]
fn rendered_table_cells_omit_inline_markdown_markers() {
    use upleft_render::fragments::table_fragment::TableCellPresentation;
    let source = "| | |\n|---|---|\n| **Rendered diff** | Updates *in place* with `code` and [links](https://example.com). |";
    let document = MarkdownParser::parse(source);
    let data = first_table(&document);
    let row = data.body_rows().first().copied().cloned().expect("a body row");
    assert_eq!(row.cells.len(), 2);
    let storage = attributed(source);
    assert_eq!(TableCellPresentation::plain_text(&row.cells[0], &storage), "Rendered diff");
    assert_eq!(TableCellPresentation::plain_text(&row.cells[1], &storage), "Updates in place with code and links.");
}

#[test]
fn one_line_table_cells_keep_their_final_glyph() {
    use objc2::AnyThread;
    use objc2_foundation::NSMutableAttributedString;
    use upleft_render::fragments::fragment_base::clipped;
    use upleft_render::fragments::table_fragment::{TableCellPresentation, TableLayout};
    let source = "| Mode | Target |\n|---|---:|\n| Read | 250 |";
    let document = MarkdownParser::parse(source);
    let data = first_table(&document);
    let row = data.body_rows().first().copied().cloned().expect("a body row");
    let style = style_sheet(false);
    let storage = attributed(source);
    let layout = TableLayout::make(&data, &storage, 500.0, &style);
    let height = layout.row_heights.last().copied().unwrap_or(0.0) - render_metrics::TABLE_ROW_PADDING;
    for (index, cell) in row.cells.iter().enumerate() {
        let text = NSMutableAttributedString::initWithAttributedString(
            NSMutableAttributedString::alloc(),
            &TableCellPresentation::attributed_content(cell, &storage),
        );
        // SAFETY: a font is the value the key expects.
        unsafe {
            text.addAttribute_value_range(keys::font(), &style.body_font(), objc2_foundation::NSRange::new(0, text.length()))
        };
        assert_eq!(
            clipped(&text, height, layout.column_widths[index]).string().to_string(),
            text.string().to_string()
        );
    }
}

#[test]
fn compact_table_labels_keep_their_natural_width_beside_prose() {
    use objc2_app_kit::NSAttributedStringNSStringDrawing;
    use upleft_render::fragments::table_fragment::TableLayout;
    let source = "| | |\n|---|---|\n| **Rendered diff** | The file is watched and updates in place while preserving the current heading anchor. |";
    let document = MarkdownParser::parse(source);
    let data = first_table(&document);
    let style = style_sheet(false);
    let storage = attributed(source);
    let layout = TableLayout::make(&data, &storage, 620.0, &style);
    let label_width = upleft_render::appkit_compat::attributed_string(
        "Rendered diff",
        &[(keys::font(), &style.body_font())],
    )
    .size()
    .width;
    assert_eq!(layout.column_widths.len(), 2);
    assert!(!layout.is_stacked);
    assert!(layout.column_widths[0] >= label_width - 0.5);
    assert!(layout.column_widths[1] > layout.column_widths[0]);
}

#[test]
fn unlabeled_code_fences_expose_the_same_copy_geometry_as_labeled_fences() {
    use upleft_render::appkit_compat::{RectExt, rect};
    use upleft_render::fragments::code_block_fragment::copy_button_rect;
    let style = style_sheet(false);
    let band = rect(12.0, 4.0, 420.0, 30.0);
    let unlabeled = copy_button_rect(band, &style, "");
    let labeled = copy_button_rect(band, &style, "swift");
    assert!(unlabeled.width() > 0.0);
    assert!(unlabeled.max_x() <= band.max_x());
    assert_eq!(labeled.width(), unlabeled.width());
    assert!(labeled.max_x() <= band.max_x());
}
