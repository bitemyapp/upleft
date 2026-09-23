//! Port of `Tests/DownrightAppTests/AssetDoctorTests.swift` (every test).

use std::collections::HashSet;

use upleft_app::assets::asset_doctor::{AssetDiagnosticCode, AssetDoctor};
use upleft_app::assets::asset_resolver::{AssetMetadata, AssetProbe, AssetReferenceKind, AssetResolutionContext};
use upleft_core::parser::MarkdownParser;
use upleft_foundation::url::FileUrl;
use upleft_swift_text::ns::{NSStringExt, utf16};

#[test]
fn extracts_exact_unicode_destination_range() {
    let text = "# Caf\u{e9}\n\n![alt](<images/na\u{ef}ve (copy).png> \"title\")\n";
    let document = MarkdownParser::parse(text);
    let context = AssetResolutionContext::new(
        Some(FileUrl::from_path("/tmp/docs/readme.md")),
        Some(FileUrl::from_path("/tmp")),
    );
    let reference = AssetDoctor::references(&document, &context).into_iter().next().expect("a reference");
    let ns = utf16(text);
    assert_eq!(ns.as_slice().substring(reference.destination_range), "images/na\u{ef}ve (copy).png");
    assert_eq!(reference.line, 3);
}

#[test]
fn diagnoses_kinds_without_probing_remote_or_data() {
    let text = "![x](https://example.com/a.png) ![x](data:image/png;base64,AA==) ![](/Users/me/a.png)\n";
    let document = MarkdownParser::parse(text);
    let context =
        AssetResolutionContext::new(Some(FileUrl::from_path("/tmp/readme.md")), Some(FileUrl::from_path("/tmp")));
    let refs = AssetDoctor::references(&document, &context);
    assert_eq!(
        refs.iter().map(|reference| reference.kind).collect::<Vec<_>>(),
        vec![AssetReferenceKind::RemoteHttp, AssetReferenceKind::DataUrl, AssetReferenceKind::AbsoluteLocal]
    );
    let diagnostics = AssetDoctor::diagnose(&document, &context, None);
    assert!(diagnostics.iter().any(|diagnostic| diagnostic.code == AssetDiagnosticCode::AbsolutePath));
    assert!(!diagnostics.iter().any(|diagnostic| diagnostic.code == AssetDiagnosticCode::Missing));
}

#[test]
fn injected_probe_finds_missing_large_unsupported_and_duplicate() {
    let text = "![a](a.bin)\n![b](b.bin)\n![same](a.bin)\n";
    let document = MarkdownParser::parse(text);
    let root = FileUrl::from_path("/tmp/work");
    let context = AssetResolutionContext::with_maximum_bytes(Some(root.appending_path_component("readme.md")), Some(root), 10);
    let probe = AssetProbe::new(|url| {
        Some(
            AssetMetadata::new(true, false, Some(20), Some(url.path_extension()))
                .with_content_identity(Some("same-content".to_owned())),
        )
    });
    let codes: HashSet<AssetDiagnosticCode> =
        AssetDoctor::diagnose(&document, &context, Some(&probe)).iter().map(|diagnostic| diagnostic.code).collect();
    assert!(codes.contains(&AssetDiagnosticCode::Duplicate));
    assert!(codes.contains(&AssetDiagnosticCode::LargeFile));
    assert!(codes.contains(&AssetDiagnosticCode::UnsupportedFormat));
}

#[test]
fn proposal_requires_expected_source_and_can_reverse() {
    let text = "![alt](old.png)\n";
    let document = MarkdownParser::parse(text);
    let reference = AssetDoctor::references(&document, &AssetResolutionContext::default())
        .into_iter()
        .next()
        .expect("a reference");
    let proposal = AssetDoctor::relink_proposal(&reference, "new.png");
    assert_eq!(proposal.apply(text).as_deref(), Some("![alt](new.png)\n"));
    assert_eq!(proposal.apply("![alt](changed.png)\n"), None);
    assert_eq!(proposal.inverse("![alt](new.png)\n").as_deref(), Some(text));
}

#[test]
fn preserves_title_and_resolves_query_fragment_and_percent_path() {
    let text = "![alt](assets/a%20b.png?size=2#hero \"Title\")\n";
    let document = MarkdownParser::parse(text);
    let root = FileUrl::from_path("/tmp/work");
    let reference = AssetDoctor::references(
        &document,
        &AssetResolutionContext::new(Some(root.appending_path_component("readme.md")), Some(root)),
    )
    .into_iter()
    .next()
    .expect("a reference");
    assert_eq!(reference.title.as_deref(), Some("Title"));
    assert_eq!(reference.url.map(|url| url.path()).as_deref(), Some("/tmp/work/assets/a b.png"));
}

#[test]
fn rejects_unsafe_schemes_and_reports_traversal() {
    let text = "![x](javascript:alert(1))\n![x](../outside.png)\n";
    let document = MarkdownParser::parse(text);
    let root = FileUrl::from_path("/tmp/work/docs");
    let context = AssetResolutionContext::new(Some(root.appending_path_component("readme.md")), Some(root));
    let refs = AssetDoctor::references(&document, &context);
    assert!(refs.iter().any(|reference| reference.kind == AssetReferenceKind::Unsafe));
    let codes: HashSet<AssetDiagnosticCode> =
        AssetDoctor::diagnose(&document, &context, None).iter().map(|diagnostic| diagnostic.code).collect();
    assert!(codes.contains(&AssetDiagnosticCode::OutsideWorkspace));
    assert!(codes.contains(&AssetDiagnosticCode::Unsafe));
}

#[test]
fn shared_reference_proposal_edits_definition_destination() {
    let text = "![one][hero]\n![two][hero]\n\n[hero]: assets/hero.png \"Hero\"\n";
    let document = MarkdownParser::parse(text);
    let refs = AssetDoctor::references(&document, &AssetResolutionContext::default());
    assert_eq!(refs.len(), 2);
    let proposal = AssetDoctor::relink_proposal(refs.first().expect("a reference"), "assets/new.png");
    assert_eq!(
        proposal.apply(text).as_deref(),
        Some("![one][hero]\n![two][hero]\n\n[hero]: assets/new.png \"Hero\"\n")
    );
}

#[test]
fn keeps_escaped_parentheses_in_destination() {
    let document = MarkdownParser::parse("![alt](assets/a\\(b\\).png)\n");
    let reference = AssetDoctor::references(&document, &AssetResolutionContext::default())
        .into_iter()
        .next()
        .expect("a reference");
    assert_eq!(reference.source, "assets/a\\(b\\).png");
}

#[test]
fn marks_malformed_remote_destination() {
    let document = MarkdownParser::parse("![alt](https://)\n");
    let reference = AssetDoctor::references(&document, &AssetResolutionContext::default())
        .into_iter()
        .next()
        .expect("a reference");
    assert_eq!(reference.kind, AssetReferenceKind::Malformed);
}

#[test]
fn proposal_percent_encodes_markdown_delimiters() {
    let document = MarkdownParser::parse("![alt](old.png)\n");
    let reference = AssetDoctor::references(&document, &AssetResolutionContext::default())
        .into_iter()
        .next()
        .expect("a reference");
    let proposal = AssetDoctor::relink_proposal(&reference, "new file (copy).png");
    assert_eq!(proposal.apply("![alt](old.png)\n").as_deref(), Some("![alt](new%20file%20%28copy%29.png)\n"));
}
