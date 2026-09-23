//! Port of `Tests/MarkdownRenderTests/ReflowAndRhythmTests.swift`:
//! regressions for §6.1's soft-break handling and §11.1's vertical rhythm.
//!
//! The view cases (the `drHidden` mirror, grouped layout elements, overlay
//! and inline-code-pill geometry) and the table-layout cases need the text
//! view and the table fragment, which are ported on the view branch.

mod common;

use common::*;
use objc2::rc::Retained;
use objc2_app_kit::{NSColor, NSParagraphStyle, NSTextStorage};
use objc2_foundation::{NSString, NSUserDefaults};
use upleft_core::parser::MarkdownParser;
use upleft_core::{BlockContent, DirtySet, InlineKind, NSRange, TableRow};
use upleft_render::engine::block_style::BlockStyleFactory;
use upleft_render::engine::decoration_engine::DecorationEngine;
use upleft_render::engine::display_map::{DisplayMap, DisplaySubstitution, RangeSet};
use upleft_render::engine::hard_wrap_reflow::{HardWrapReflow, Plan};
use upleft_render::engine::render_metrics;
use upleft_render::render_contracts::{RenderMode, Theme};
use upleft_render::theme::style_sheet::{ColorResolver, StyleSheet};
use upleft_render::theme::theme_store::ThemeStore;

fn sheet() -> StyleSheet {
    StyleSheet::new(Theme::fallback(), &appearance(true), None)
}

fn live_engine() -> DecorationEngine {
    let mut engine = DecorationEngine::new(sheet());
    engine.set_policy(RenderMode::Live.policy());
    engine
}

fn decorated(source: &str) -> Retained<NSTextStorage> {
    let storage = storage(source);
    live_engine().decorate(
        &storage,
        &MarkdownParser::parse(source),
        &DirtySet::new(vec![NSRange::new(0, length(source))], true),
    );
    storage
}

/// What a hidden marker looks like in a laid-out element.
const JOINER: &str = "\u{2060}";
/// What the padding of a collapsed soft break looks like.
const PAD: &str = "\u{200B}";

/// The plan plus the map a grouped element is actually built against:
/// hidden runs as length-preserving word joiners.
fn plan(source: &str) -> (Plan, DisplayMap) {
    let document = MarkdownParser::parse(source);
    let hidden = live_engine().hidden_ranges(&document, None, &[]);
    let made = HardWrapReflow::plan(&document, &utf16(source), &hidden, &[], true);
    let mut substitutions: Vec<DisplaySubstitution> = hidden
        .iter()
        .map(|range| {
            DisplaySubstitution::new(
                *range,
                range.length,
                Some(attributed(&JOINER.repeat(range.length as usize))),
                true,
                false,
                true,
            )
        })
        .collect();
    substitutions.extend(made.substitutions.iter().filter(|s| s.is_hard_wrap_reflow).cloned());
    let map = DisplayMap::new(paragraphs(source), substitutions);
    (made, map)
}

fn style(storage: &NSTextStorage, at: isize) -> Option<Retained<NSParagraphStyle>> {
    paragraph_style(storage, at)
}

// MARK: - Soft breaks inside containers

const WRAPPED_LIST_ITEM: &str = "3. **No WebView. Anywhere.** Math via SwiftMath, diagrams via
   beautiful-mermaid-swift, code via a native lexer, everything else through
   Core Text.

";

#[test]
fn a_list_items_continuation_indent_is_not_an_explicit_break_marker() {
    let document = MarkdownParser::parse(WRAPPED_LIST_ITEM);
    let mut kinds: Vec<bool> = Vec::new();
    document.root.walk(&mut |block| {
        for span in &block.inlines {
            span.walk(&mut |inline| match inline.kind {
                InlineKind::SoftBreak => kinds.push(true),
                InlineKind::LineBreak => kinds.push(false),
                _ => {}
            });
        }
    });
    assert_eq!(kinds, vec![true, true]);
}

#[test]
fn hard_wrapped_list_prose_joins_with_single_spaces() {
    let (made, map) = plan(WRAPPED_LIST_ITEM);
    assert_eq!(made.ranges.len(), 1);
    let group = made.ranges[0];
    let shown = map
        .display_string_for_source_range(group, &attributed(WRAPPED_LIST_ITEM))
        .expect("the list item produced no display string")
        .string()
        .to_string();
    assert_eq!(shown.encode_utf16().count() as isize, group.length);
    assert!(!shown[..shown.len() - 1].contains('\n'));
    assert!(!shown.contains("  "));
    assert!(shown.contains(&format!("diagrams via {PAD}{PAD}{PAD}beautiful-mermaid-swift")));
}

#[test]
fn a_reflowed_list_item_keeps_its_marker_on_the_same_row() {
    let (made, _) = plan(WRAPPED_LIST_ITEM);
    assert_eq!(made.ranges.first().map(|r| r.location), Some(0));
}

#[test]
fn an_explicit_two_space_break_still_survives_reflow() {
    let (made, _) = plan("line one with an explicit break  \nline two after it\n");
    assert!(made.ranges.is_empty());
}

#[test]
fn a_backslash_break_still_survives_reflow() {
    let (made, _) = plan("line one with a backslash break\\\nline two after it\n");
    assert!(made.ranges.is_empty());
}

#[test]
fn a_stale_document_longer_than_the_buffer_does_not_overrun_indents() {
    let source = "line one\n   line two\n";
    let document = MarkdownParser::parse(source);
    let truncated = &utf16(source)[..12]; // "line one\n   "
    let made = HardWrapReflow::plan(&document, truncated, &[], &[], true);
    assert!(made.substitutions.iter().all(|s| s.source_range.upper_bound() <= truncated.len() as isize));
    assert!(made.substitutions.iter().any(|s| s.source_range == NSRange::new(9, 3)));
}

#[test]
fn a_callout_body_does_not_open_with_a_space() {
    let source = "> [!NOTE]
> Agents emit these constantly, so they get a real treatment: a coloured left
> rule and an icon, never a filled box.

";
    let (made, map) = plan(source);
    let group = *made.ranges.first().expect("the callout body produced no group");
    let shown = map
        .display_string_for_source_range(group, &attributed(source))
        .expect("the callout body produced no group")
        .string()
        .to_string();
    assert!(!shown.starts_with(' '));
    assert!(shown.starts_with(PAD), "the terminator that opens the element contributes no space");
    assert!(shown.contains(&format!("coloured left {JOINER}{JOINER}rule")));
}

// MARK: - Marker identity

#[test]
fn adjacent_block_and_inline_markers_keep_separate_ranges() {
    let source = "3. **No WebView.** Math via SwiftMath.\n";
    let hidden = live_engine().hidden_ranges(&MarkdownParser::parse(source), None, &[]);
    assert!(hidden.contains(&NSRange::new(0, 3)));
    assert!(hidden.contains(&NSRange::new(3, 2)));
    assert!(!hidden.contains(&NSRange::new(0, 5)));
}

#[test]
fn range_set_disjoint_fuses_overlaps_and_keeps_neighbours() {
    let fused = RangeSet::disjoint(&[NSRange::new(0, 3), NSRange::new(3, 2), NSRange::new(4, 4)]);
    assert_eq!(fused, vec![NSRange::new(0, 3), NSRange::new(3, 5)]);
}

#[test]
fn a_nested_items_container_indent_is_hidden() {
    let source = "- [x] Notarise and publish\n  - [ ] Sparkle appcast\n";
    let hidden = live_engine().hidden_ranges(&MarkdownParser::parse(source), None, &[]);
    let indent = NSRange::new(27, 2);
    assert!(hidden.iter().any(|r| upleft_core::ns_range::ns_intersection_range(*r, indent).length == indent.length));
}

// MARK: - Vertical rhythm

#[test]
fn a_hard_wrapped_paragraph_keeps_the_air_after_it() {
    let source = "Reading position persists per file regardless of whether the bytes changed, so
long documents behave like books.

The density gutter shows the whole shape of a document at a glance.

";
    let storage = decorated(source);
    let opening = style(&storage, 0).unwrap();
    let closing = style(&storage, find(source, "long documents").location).unwrap();
    assert_eq!(opening.paragraphSpacing(), 0.0); // a join, not a close
    assert!(closing.paragraphSpacing() > 0.0); // the close keeps its air
    assert_eq!(closing.paragraphSpacingBefore(), 0.0); // and adds none of its own
}

#[test]
fn line_height_lands_on_an_even_point_at_the_default_body() {
    let style = sheet();
    assert_eq!(style.body_font().pointSize(), 16.0);
    assert_eq!(style.line_height, 26.0);
    assert_eq!(style.baseline_grid, 6.5);
}

#[test]
fn a_heading_following_a_heading_binds_tighter_than_one_following_prose() {
    let stacked_source = "## Section\n\n### Subsection\n\nBody text here.\n";
    let prose_source = "## Section\n\nBody text here.\n\n### Subsection\n\nMore body.\n";
    let stacked = decorated(stacked_source);
    let after_prose = decorated(prose_source);
    let stacked_before = style(&stacked, find(stacked_source, "### Subsection").location).map_or(0.0, |s| s.paragraphSpacingBefore());
    let prose_before = style(&after_prose, find(prose_source, "### Subsection").location).map_or(0.0, |s| s.paragraphSpacingBefore());
    assert!(stacked_before < prose_before);
}

// MARK: - Code rows

#[test]
fn a_wrapped_code_row_hangs_past_its_own_indent() {
    let source = "```swift
func decorate(_ storage: NSTextStorage, document: ParsedDocument) {
    let markers = hiddenRanges(document: document, caret: nil, selections: [])
}
```

";
    let storage = decorated(source);
    let outer = style(&storage, find(source, "func decorate").location).expect("a code row carried no paragraph style");
    let inner = style(&storage, find(source, "let markers").location).expect("a code row carried no paragraph style");
    assert_eq!(inner.firstLineHeadIndent(), outer.firstLineHeadIndent());
    assert!(inner.headIndent() > outer.headIndent());
    assert!(inner.defaultTabInterval() > 0.0);
}

#[test]
fn code_row_indent_columns_count_tabs_to_their_stops() {
    assert_eq!(BlockStyleFactory::indent_columns(&utf16("    let x = 1")), 4);
    assert_eq!(
        BlockStyleFactory::indent_columns(&utf16("\tlet x = 1")),
        render_metrics::CODE_TAB_COLUMNS as isize
    );
    assert_eq!(BlockStyleFactory::indent_columns(&utf16("no indent")), 0);
}

// MARK: - The task checkbox, in both places

/// A colour with alpha, resolved against what sits behind it.
fn composite(color: &NSColor, background: &NSColor) -> Retained<NSColor> {
    ColorResolver::blend(background, &color.colorWithAlphaComponent(1.0), color.alphaComponent())
}

fn bundled_themes() -> Vec<Theme> {
    let defaults = NSUserDefaults::initWithSuiteName(
        <NSUserDefaults as objc2::AnyThread>::alloc(),
        Some(&NSString::from_str("downright.tests.reflow")),
    )
    .expect("suite");
    ThemeStore::new(defaults).themes()
}

#[test]
fn the_task_checkbox_tick_carries_its_own_field_in_every_theme() {
    for theme in bundled_themes() {
        for dark in [false, true] {
            let sheet = StyleSheet::new(theme.clone(), &appearance(dark), None);
            let field = composite(&sheet.task_field_color(), &sheet.background);
            let tick = composite(&sheet.task_tick_color(), &field);
            let ratio = StyleSheet::contrast_ratio(&tick, &field);
            assert!(ratio >= 3.0, "{}/{dark}: tick on field is {ratio}:1", theme.name);
        }
    }
}

#[test]
fn an_open_checkbox_ring_stays_visible_against_the_page() {
    for theme in bundled_themes() {
        let sheet = StyleSheet::new(theme.clone(), &appearance(false), None);
        let ring = composite(&sheet.task_ring_color(false), &sheet.background);
        let ratio = StyleSheet::contrast_ratio(&ring, &sheet.background);
        assert!(ratio >= 2.9, "{}: open ring is {ratio}:1 against the page", theme.name);
    }
}

// MARK: - Table cells

#[test]
fn an_empty_table_cell_has_no_content_of_its_own() {
    let source = "| | |\n|---|---|\n| `⌘E` | Use selection for Find |\n\n";
    let document = MarkdownParser::parse(source);
    let mut header: Option<TableRow> = None;
    document.root.walk(&mut |block| {
        if let BlockContent::Table(data) = &block.content {
            header = data.header_row().cloned();
        }
    });
    let header = header.expect("the table did not parse");
    let units = utf16(source);
    for cell in &header.cells {
        assert_eq!(cell.content_range.length, 0, "an empty cell has no content");
        assert!(cell.inlines.is_empty());
        let text = String::from_utf16_lossy(&units[cell.content_range.as_usize_range()]);
        assert!(!text.contains('|'));
    }
}
