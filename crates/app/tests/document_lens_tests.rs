//! Port of `Tests/DownrightAppTests/DocumentLensTests.swift`
//! (`DocumentLensModelTests`).
//!
//! Skipped: `DocumentLensViewTests.tabSwitchAndReturnSelectsTheExactItem`,
//! which needs `DocumentLensView` (Panels/, not ported yet).

use upleft_app::assets::asset_doctor::AssetDoctor;
use upleft_app::assets::asset_resolver::{AssetProbe, AssetResolutionContext};
use upleft_app::lens::document_lens_model::{
    DocumentLensChange, DocumentLensInput, DocumentLensItemKind, DocumentLensModel, DocumentLensTab,
};
use upleft_core::compatibility::compatibility_diagnostics::MarkdownCompatibility;
use upleft_core::compatibility::render_target::RenderTargetProfile;
use upleft_core::health::document_health::DocumentHealth;
use upleft_core::parser::MarkdownParser;
use upleft_core::{ChangeKind, NSRange};
use upleft_foundation::url::FileUrl;
use upleft_swift_text::ns::{NSStringExt, utf16};

#[test]
fn builds_every_tab_with_exact_source_ranges() {
    let text = "# Title\n\n[Guide](https://example.com)\n\n- [ ] Ship it\n\n~~old~~";
    let document = MarkdownParser::parse(text);
    let health = DocumentHealth::analyze_document(&document);
    let report = MarkdownCompatibility::diagnose(&document, &RenderTargetProfile::common_mark());
    let mut input = DocumentLensInput::new(document.clone());
    input.health = health;
    input.render_target = Some(report);
    input.changes = vec![DocumentLensChange::new("one", ChangeKind::Inserted, NSRange::new(0, 7), Vec::new())];
    let model = DocumentLensModel::new(&input);

    assert_eq!(model.sections.iter().map(|section| section.tab).collect::<Vec<_>>(), DocumentLensTab::ALL_CASES.to_vec());
    let ns = utf16(text);
    let structure = model.section(DocumentLensTab::Structure);
    let title =
        structure.items().into_iter().find(|item| item.kind == DocumentLensItemKind::Heading).expect("a heading item");
    assert_eq!(ns.as_slice().substring(title.range), "# Title");
    let link = model
        .section(DocumentLensTab::Links)
        .items()
        .into_iter()
        .find(|item| item.kind == DocumentLensItemKind::Link)
        .expect("a link item");
    assert_eq!(ns.as_slice().substring(link.range), "[Guide](https://example.com)");
    let task = model.section(DocumentLensTab::Tasks).items().into_iter().next().expect("a task item");
    assert_eq!(ns.as_slice().substring(task.range), "Ship it\n");
    let change = model.section(DocumentLensTab::Changes).items().into_iter().next().expect("a change item");
    assert_eq!(change.range, NSRange::new(0, 7));
    assert!(model.section(DocumentLensTab::RenderTarget).count() > 0);
}

#[test]
fn groups_health_and_assets_without_changing_order() {
    let document = MarkdownParser::parse("# A\n\n![alt](missing.png)\n");
    let asset = AssetDoctor::diagnose(
        &document,
        &AssetResolutionContext::with_maximum_bytes(
            Some(FileUrl::from_path("/tmp/readme.md")),
            Some(FileUrl::from_path("/tmp")),
            1,
        ),
        Some(&AssetProbe::new(|_| None)),
    );
    let mut input = DocumentLensInput::new(document);
    input.assets = asset.clone();
    let model = DocumentLensModel::new(&input);
    assert_eq!(model.section(DocumentLensTab::Assets).count(), asset.len());
    assert!(model.section(DocumentLensTab::Assets).items().iter().all(|item| item.range.length > 0));
    assert!(model.section(DocumentLensTab::Health).groups.iter().all(|group| !group.title.is_empty()));
}
