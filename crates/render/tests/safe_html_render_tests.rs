//! Port of `Tests/MarkdownRenderTests/SafeHTMLRenderTests.swift`: native
//! presentation of README-style HTML.

mod common;

use std::collections::HashSet;

use common::*;
use objc2_app_kit::{NSFont, NSFontDescriptorSymbolicTraits, NSParagraphStyle, NSTextAlignment};
use objc2_foundation::{NSNumber, NSString};
use upleft_core::DirtySet;
use upleft_core::parser::MarkdownParser;
use upleft_core::safe_html::SafeHTMLKind;
use upleft_render::engine::keys;
use upleft_render::render_contracts::{FragmentKind, FragmentPayload, RenderMode, attribute_keys};

#[test]
fn safe_tags_collapse_but_source_and_semantic_attributes_remain() {
    let source = r#"<p align="center"><strong>Gold</strong> <a href="https://example.com">standard</a></p>"#;
    let document = MarkdownParser::parse(source);
    let storage = storage(source);
    let mut renderer = engine(RenderMode::Live);
    renderer.decorate(&storage, &document, &DirtySet::wholesale());

    assert_eq!(storage.string().to_string(), source);
    let hidden: HashSet<_> = renderer.hidden_ranges(&document, None, &[]).into_iter().collect();
    let html = document.root.children[0].safe_html.as_ref().expect("safe HTML");
    let tags: HashSet<_> = html.tag_ranges().into_iter().collect();
    assert_eq!(hidden, tags);

    let gold = find(source, "Gold");
    let standard = find(source, "standard");
    let bold = attribute(&storage, keys::font(), gold.location).and_then(|v| v.downcast::<NSFont>().ok()).unwrap();
    assert!(bold.fontDescriptor().symbolicTraits().contains(NSFontDescriptorSymbolicTraits::TraitBold));
    let link = attribute(&storage, attribute_keys::dr_link(), standard.location).and_then(|v| v.downcast::<NSString>().ok());
    assert_eq!(link.map(|s| s.to_string()).as_deref(), Some("https://example.com"));
    let paragraph = attribute(&storage, keys::paragraph_style(), gold.location)
        .and_then(|v| v.downcast::<NSParagraphStyle>().ok())
        .unwrap();
    assert_eq!(paragraph.alignment(), NSTextAlignment::Center);
}

#[test]
fn caret_reveals_safe_tags_for_editing_and_source_mode_always_shows_them() {
    let source = "<strong>Text</strong>";
    let document = MarkdownParser::parse(source);
    let live = engine(RenderMode::Live);
    assert!(!live.hidden_ranges(&document, None, &[]).is_empty());
    assert!(live.hidden_ranges(&document, Some(9), &[]).is_empty());
    assert!(engine(RenderMode::Source).hidden_ranges(&document, None, &[]).is_empty());
}

#[test]
fn unsafe_html_receives_no_link_fragment_or_hidden_ranges() {
    let source = r#"<a href="javascript:alert(1)">Run</a>"#;
    let document = MarkdownParser::parse(source);
    let storage = storage(source);
    let mut renderer = engine(RenderMode::Live);
    renderer.decorate(&storage, &document, &DirtySet::wholesale());
    assert!(renderer.hidden_ranges(&document, None, &[]).is_empty());
    assert!(attribute(&storage, attribute_keys::dr_link(), 0).is_none());
    assert_eq!(storage.string().to_string(), source);
}

#[test]
fn local_html_image_uses_native_fragment_without_remote_loading() {
    let source = r#"<img src="Docs/demo.png" alt="Demo">"#;
    let document = MarkdownParser::parse(source);
    let storage = storage(source);
    engine(RenderMode::Live).decorate(&storage, &document, &DirtySet::wholesale());
    let payload = attribute(&storage, attribute_keys::dr_fragment(), 0)
        .and_then(|v| v.downcast::<FragmentPayload>().ok())
        .expect("payload");
    assert_eq!(payload.kind(), FragmentKind::Image);
    assert_eq!(payload.detail(), "Docs/demo.png");
    assert_eq!(storage.string().to_string(), source);
}

#[test]
fn details_and_table_annotations_receive_native_reading_chrome() {
    let source = "<details open><summary>More</summary>Body</details>\n<table><tr><td>A</td><td>B</td></tr></table>";
    let document = MarkdownParser::parse(source);
    let storage = storage(source);
    engine(RenderMode::Live).decorate(&storage, &document, &DirtySet::wholesale());

    let details = document.root.children[0].safe_html.as_ref().expect("details");
    let summary = details
        .annotations
        .iter()
        .find(|a| matches!(a.kind, SafeHTMLKind::Summary))
        .expect("summary");
    assert!(attribute(&storage, keys::background_color(), summary.content_range.location).is_some());

    let table = document
        .root
        .children
        .iter()
        .filter_map(|block| block.safe_html.as_ref())
        .find(|html| html.annotations.iter().any(|a| matches!(a.kind, SafeHTMLKind::TableCell { .. })))
        .expect("table");
    let cells: Vec<_> = table
        .annotations
        .iter()
        .filter(|a| matches!(a.kind, SafeHTMLKind::TableCell { .. }))
        .collect();
    assert_eq!(cells.len(), 2);
    for cell in cells {
        let last = cell.content_range.upper_bound() - 1;
        let kern = attribute(&storage, keys::kern(), last).and_then(|v| v.downcast::<NSNumber>().ok());
        assert!(kern.map_or(0.0, |n| n.doubleValue()) > 0.0);
        assert!(attribute(&storage, keys::background_color(), last).is_some());
    }
    assert_eq!(storage.string().to_string(), source);
}
