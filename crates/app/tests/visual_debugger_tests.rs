//! Port of `Tests/DownrightAppTests/VisualDebuggerTests.swift`
//! (`VisualDebuggerModelTests`).
//!
//! Skipped: `VisualDebuggerViewTests.exposesReadOnlySummaryAndCopiesIt`,
//! which needs `VisualDebuggerView` (Panels/, not ported yet).

mod common;

use common::ns_range_of;
use upleft_app::assets::asset_doctor::{AssetDiagnostic, AssetDiagnosticCode, AssetDoctor};
use upleft_app::assets::asset_resolver::AssetResolutionContext;
use upleft_app::debugging::visual_debugger_model::{
    VisualDebuggerInput, VisualDebuggerMapping, VisualDebuggerModel, VisualDebuggerStyleFacts,
};
use upleft_core::NSRange;
use upleft_core::compatibility::compatibility_diagnostics::MarkdownCompatibility;
use upleft_core::compatibility::render_target::{MarkdownCapability, RenderTargetProfile};
use upleft_core::parser::MarkdownParser;
use upleft_render::render_contracts::RenderMode;

#[test]
fn reports_caret_block_inline_mode_and_mapping() {
    let text = "# Title\n\nA **bold** word.\n";
    let document = MarkdownParser::parse(text);
    let offset = ns_range_of(text, "bold").location;
    let source_range = NSRange::new(offset, 0);
    let model = VisualDebuggerModel::new(&VisualDebuggerInput::with(
        document,
        source_range,
        RenderMode::Live,
        VisualDebuggerStyleFacts {
            font_family: "Test Font".into(),
            point_size: 13.0,
            paragraph_alignment: "left".into(),
            ..VisualDebuggerStyleFacts::default()
        },
        Some(VisualDebuggerMapping::with(source_range, NSRange::new(9, 0), Some(offset), Some(9), true, Vec::new())),
        None,
        Vec::new(),
    ));

    assert_eq!(model.mode, RenderMode::Live);
    assert_eq!(model.line, 3);
    assert_eq!(model.column, 5);
    assert_eq!(model.block.as_ref().map(|block| block.kind.as_str()), Some("paragraph"));
    assert_eq!(model.inline.as_ref().map(|inline| inline.kind.as_str()), Some("strong"));
    assert_eq!(model.mapping.text_kit_offset, 9);
    assert!(model.summary().contains("Inline: strong"));
    assert!(model.summary().contains("Font: Test Font"));
}

#[test]
fn filters_target_and_asset_diagnostics_at_selection() {
    let text = "![image](missing.png)\n\n~~old~~\n";
    let document = MarkdownParser::parse(text);
    let source_range = NSRange::new(0, document.length);
    let report = MarkdownCompatibility::diagnose(&document, &RenderTargetProfile::common_mark());
    let references = AssetDoctor::references(&document, &AssetResolutionContext::new(None, None));
    let diagnostics: Vec<AssetDiagnostic> = references
        .into_iter()
        .map(|reference| {
            AssetDiagnostic::new(AssetDiagnosticCode::Missing, "Asset is missing.", reference.destination_range, reference)
        })
        .collect();
    let model = VisualDebuggerModel::new(&VisualDebuggerInput::with(
        document,
        source_range,
        RenderMode::Read,
        VisualDebuggerStyleFacts::default(),
        None,
        Some(report),
        diagnostics,
    ));

    assert_eq!(model.render_target.as_ref().map(|profile| profile.name.as_str()), Some("CommonMark"));
    assert!(model.render_diagnostics.iter().any(|diagnostic| diagnostic.capability == MarkdownCapability::Strikethrough));
    assert_eq!(model.asset_diagnostics.len(), 1);
}

#[test]
fn summary_is_stable_and_includes_hidden_coordinates() {
    let document = MarkdownParser::parse("# Heading\n");
    let model = VisualDebuggerModel::new(&VisualDebuggerInput::with(
        document,
        NSRange::new(0, 0),
        RenderMode::Read,
        VisualDebuggerStyleFacts::default(),
        Some(VisualDebuggerMapping::with(
            NSRange::new(0, 0),
            NSRange::new(0, 0),
            Some(0),
            Some(0),
            false,
            vec![NSRange::new(0, 2)],
        )),
        None,
        Vec::new(),
    ));
    assert!(model.summary().contains("Canonical source offset: no"));
    assert!(model.summary().contains("Hidden source ranges: 0..<2 (length 2)"));
    assert!(model.summary().contains("Block content range:"));
}
