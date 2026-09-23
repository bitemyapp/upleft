//! Port of `Tests/MarkdownRenderTests/SafeHTMLLineBreakTests.swift`.
//!
//! The Swift cases read `MarkdownTextView.currentDisplayMap` after
//! `update(document:dirty: .wholesale)`. That map is the layout map
//! `rebuildBaseDisplayMap` builds, which `view::base_display_map` ports, so the
//! cases run against it directly: the decoration, the paragraph index and the
//! producer, in the order `update` calls them. `focusSource(in:)` is expressed
//! as the scoped `SourceFocus` it installs.

mod common;

use common::*;
use upleft_core::parser::MarkdownParser;
use upleft_core::safe_html::SafeHTMLKind;
use upleft_core::{DirtySet, NSRange};
use upleft_render::engine::display_map::{DisplayMap, ParagraphIndex};
use upleft_render::render_contracts::{RenderMode, SourceFocus};
use upleft_render::view::base_display_map::{BaseDisplayMapInputs, WordJoinerRuns, rebuild_base_display_map};

fn current_display_map(source: &str, mode: RenderMode, focus: SourceFocus) -> DisplayMap {
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
    .base_layout_map
}

fn first_char(map: &DisplayMap, range: NSRange) -> Option<char> {
    map.substitutions_in(range)
        .first()
        .and_then(|s| s.replacement.as_ref())
        .and_then(|r| r.string().to_string().chars().next())
}

#[test]
fn safe_br_tags_render_as_breaks_without_changing_source_coordinates() {
    let source = "<p>first<br>second</p>";
    let map = current_display_map(source, RenderMode::Live, SourceFocus::None);
    let break_range = find(source, "<br>");
    let entry = map.substitutions_in(break_range).into_iter().next().expect("entry");
    assert_eq!(entry.source_range, break_range);
    assert_eq!(entry.display_length, break_range.length);
    assert_eq!(first_char(&map, break_range), Some('\n'));
    assert!(!entry.is_hidden);
    assert!(entry.preserves_source_offsets);
}

#[test]
fn source_mode_leaves_safe_html_tags_literal() {
    let source = "<p>first<br>second</p>";
    // `mode = .source` installs document-wide Source Focus.
    let map = current_display_map(source, RenderMode::Source, SourceFocus::Document);
    assert!(map.substitutions_in(find(source, "<br>")).is_empty());
}

#[test]
fn scoped_source_focus_reveals_a_safe_br_tag_literally() {
    let source = "<p>first<br>second</p>";
    let break_range = find(source, "<br>");
    let map = current_display_map(source, RenderMode::Live, SourceFocus::Scoped(break_range));
    assert!(map.substitutions_in(break_range).is_empty());
}

#[test]
fn details_preserves_source_offsets_and_shows_its_authored_disclosure_state() {
    let source = "<details><summary>More</summary>Body</details>";
    let document = MarkdownParser::parse(source);
    let map = current_display_map(source, RenderMode::Live, SourceFocus::None);
    let details = document.root.children[0]
        .safe_html
        .as_ref()
        .unwrap()
        .annotations
        .iter()
        .find(|a| matches!(a.kind, SafeHTMLKind::Details { .. }))
        .expect("details")
        .clone();
    let opening = details.tag_ranges[0];
    let substitution = map.substitutions_in(opening).into_iter().next().expect("substitution");
    assert_eq!(substitution.source_range, opening);
    assert_eq!(substitution.display_length, opening.length);
    assert_eq!(first_char(&map, opening), Some('▸'));
    assert!(substitution.preserves_source_offsets);
}

#[test]
fn multiline_details_keeps_all_disclosure_tags_out_of_document_display() {
    let source = "<details>\n<summary>More</summary>\n\nBody\n\n</details>";
    let map = current_display_map(source, RenderMode::Live, SourceFocus::None);
    for tag in ["<details>", "<summary>", "</summary>", "</details>"] {
        assert!(!map.substitutions_in(find(source, tag)).is_empty(), "missing substitution for {tag}");
    }
}

#[test]
fn html_table_rows_receive_source_preserving_line_breaks() {
    let source = "<table><tr><td>A</td><td>B</td></tr><tr><td>C</td><td>D</td></tr></table>";
    let document = MarkdownParser::parse(source);
    let map = current_display_map(source, RenderMode::Live, SourceFocus::None);
    let rows: Vec<_> = document.root.children[0]
        .safe_html
        .as_ref()
        .map(|html| html.annotations.iter().filter(|a| matches!(a.kind, SafeHTMLKind::TableRow)).cloned().collect())
        .unwrap_or_default();
    assert_eq!(rows.len(), 2);
    for row in rows {
        let closing = *row.tag_ranges.last().unwrap();
        let substitution = map.substitutions_in(closing).into_iter().next().expect("substitution");
        assert_eq!(substitution.source_range, closing);
        assert_eq!(first_char(&map, closing), Some('\n'));
        assert!(substitution.preserves_source_offsets);
    }
}
