//! Port of `Tests/DownrightAppTests/DocumentDropTests.swift`: which path a
//! dropped file ends up with, pushed back through the real parser and the
//! real `LocalAssetPolicy` to prove it still names the dropped file.
//!
//! Skipped (they drive `DocumentWindowController`, not ported yet; "The real
//! window"): `droppingAnImageInsertsOneUndoableReferenceAtTheDropPoint`,
//! `droppingPixelsWritesAFileNamedAfterTheDocument`,
//! `aNeverSavedWindowTakesFilesButNotLooseImageBytes`,
//! `aDroppedSiblingBecomesALinkThatResolves`,
//! `aFailedWriteInsertsNothingAndLeavesNoDebris`.
//!
//! `aFileFromOutsideIsLinkedByFileURL` checks the classification through the
//! definition of `MarkdownLinkDestination.classify` (a destination `URL(string:)`
//! parses with scheme `file` is `.localFile(url)`), because that enum lives in
//! `App/DocumentWindowController+Delegates.swift`, which is not ported yet.
//!
//! Named pasteboards are released when a test ends (the Swift suite leaves
//! them to the pasteboard server); nothing reaches the general pasteboard.

mod asset_support;
mod common;

use std::collections::HashSet;

use asset_support::{named_pasteboard, png_data, set_data, write_files};
use common::temporary_directory;
use objc2_app_kit::{NSPasteboardTypePNG, NSPasteboardTypeString};
use objc2_foundation::{NSString, NSURL};
use upleft_app::assets::asset_doctor::{AssetDiagnostic, AssetDoctor};
use upleft_app::assets::asset_resolver::{AssetMetadata, AssetProbe, AssetReferenceKind, AssetResolutionContext};
use upleft_app::assets::captured_image;
use upleft_app::assets::dropped_asset::{Destination, DroppedAsset, Insertion, Payload, Write, WriteContents};
use upleft_core::parser::MarkdownParser;
use upleft_foundation::url::FileUrl;
use upleft_render::fragments::local_asset_policy::LocalAssetPolicy;
use upleft_swift_text as swift;

fn png_type() -> &'static NSString {
    // SAFETY: an AppKit constant.
    unsafe { NSPasteboardTypePNG }
}

fn write(url: &FileUrl, bytes: &[u8]) {
    std::fs::write(url.path(), bytes).unwrap();
}

fn pasteboard_with_files(files: &[FileUrl]) -> asset_support::Board {
    let board = named_pasteboard("DocumentDropTests");
    write_files(&board, files);
    board
}

fn pasteboard_with_png(png: &[u8]) -> asset_support::Board {
    let board = named_pasteboard("DocumentDropTests");
    set_data(&board, png, png_type());
    board
}

// MARK: - Reading the drag

#[test]
fn a_file_drag_is_read_as_files_even_when_it_also_carries_pixels() {
    let (directory, _cleanup) = temporary_directory("DocumentDropTests");
    let file = directory.appending_path_component("shot.png");
    write(&file, &png_data());

    // Finder and Preview both put an image representation on the pasteboard
    // alongside the file URL.
    let board = named_pasteboard("DocumentDropTests");
    write_files(&board, std::slice::from_ref(&file));
    set_data(&board, &png_data(), png_type());
    assert_eq!(DroppedAsset::payload(&board), Some(Payload::Files(vec![file])));
}

#[test]
fn raw_pixels_are_read_when_there_is_no_file() {
    let png = png_data();
    let payload = DroppedAsset::payload(&pasteboard_with_png(&png)).expect("a payload");
    assert_eq!(payload, Payload::ImageData(captured_image::Payload::new(png, "png")));
}

#[test]
fn a_text_drag_is_not_an_asset_drop() {
    let board = named_pasteboard("DocumentDropTests");
    // SAFETY: an AppKit constant.
    board.setString_forType(&NSString::from_str("just some words"), unsafe { NSPasteboardTypeString });
    assert_eq!(DroppedAsset::payload(&board), None);
}

/// A dragged file that has since been deleted is not a drop target.
#[test]
fn a_vanished_file_is_not_a_payload() {
    let (directory, _cleanup) = temporary_directory("DocumentDropTests");
    let board = pasteboard_with_files(&[directory.appending_path_component("gone.png")]);
    assert_eq!(DroppedAsset::payload(&board), None);
}

// MARK: - Which destination a file gets

#[test]
fn a_file_inside_the_documents_folder_is_referenced_where_it_stands() {
    let (directory, _cleanup) = temporary_directory("DocumentDropTests");
    std::fs::create_dir_all(directory.appending_path_component("assets").path()).unwrap();
    let nested = directory.appending_path_component("assets/diagram.png");
    assert_eq!(DroppedAsset::destination(&nested, Some(&directory)), Destination::Relative("assets/diagram.png".into()));
}

/// Percent-encoding is *not* the answer: the renderer treats the destination
/// as a literal path.
#[test]
fn a_space_in_the_name_uses_the_angle_bracket_form_rather_than_percent_encoding() {
    let (directory, _cleanup) = temporary_directory("DocumentDropTests");
    let spaced = directory.appending_path_component("meeting notes.png");
    assert_eq!(DroppedAsset::destination(&spaced, Some(&directory)), Destination::Angled("meeting notes.png".into()));
    assert_eq!(Destination::Angled("meeting notes.png".into()).markdown_text(), "<meeting notes.png>");
    assert!(!Destination::Angled("meeting notes.png".into()).markdown_text().contains('%'));
}

#[test]
fn a_file_outside_the_folder_has_no_relative_form() {
    let (directory, _cleanup) = temporary_directory("DocumentDropTests");
    let (elsewhere, _cleanup_elsewhere) = temporary_directory("DocumentDropTests");
    let outside = elsewhere.appending_path_component("diagram.png");
    assert_eq!(DroppedAsset::relative_path(&outside, &directory), None);
    assert!(
        matches!(DroppedAsset::destination(&outside, Some(&directory)), Destination::Absolute(_)),
        "a file outside the folder must not be given a relative destination"
    );
}

/// A `..` path is *not* used for a file one directory up.
#[test]
fn a_sibling_folder_does_not_get_a_traversal_path() {
    let (root, _cleanup) = temporary_directory("DocumentDropTests");
    let documents = root.appending_path_component_is_directory("docs", true);
    let images = root.appending_path_component_is_directory("images", true);
    std::fs::create_dir_all(documents.path()).unwrap();
    std::fs::create_dir_all(images.path()).unwrap();
    let asset = images.appending_path_component("diagram.png");
    assert_eq!(DroppedAsset::relative_path(&asset, &documents), None);
}

/// `/tmp` is a symlink to `/private/tmp`, and a document opened through one
/// with an image dragged from the other must not look like two folders.
#[test]
fn symlinked_folders_still_resolve_as_the_same_folder() {
    let (directory, _cleanup) = temporary_directory("DocumentDropTests");
    let resolved = directory.resolving_symlinks_in_path();
    let asset = resolved.appending_path_component("diagram.png");
    assert_eq!(DroppedAsset::relative_path(&asset, &directory).as_deref(), Some("diagram.png"));
}

// MARK: - What a drop becomes

fn plan(payload: &Payload, directory: Option<&FileUrl>, base: &str, taken: &[&str]) -> Vec<Insertion> {
    let taken: HashSet<String> = taken.iter().map(|name| (*name).to_owned()).collect();
    DroppedAsset::plan(payload, directory, base, |name| taken.contains(name))
}

#[test]
fn an_image_already_beside_the_document_is_not_copied() {
    let (directory, _cleanup) = temporary_directory("DocumentDropTests");
    let asset = directory.appending_path_component("diagram.png");
    write(&asset, &png_data());

    let insertions = plan(&Payload::Files(vec![asset]), Some(&directory), "notes", &[]);
    assert_eq!(insertions.len(), 1);
    assert_eq!(insertions[0].write, None, "a file already in the folder must not be duplicated");
    assert_eq!(insertions[0].markdown, "![diagram](diagram.png)");
    assert!(insertions[0].is_block);
}

#[test]
fn an_image_from_outside_is_copied_in_and_referenced_relatively() {
    let (directory, _cleanup) = temporary_directory("DocumentDropTests");
    let (elsewhere, _cleanup_elsewhere) = temporary_directory("DocumentDropTests");
    let asset = elsewhere.appending_path_component("Screen Shot 2026.png");
    write(&asset, &png_data());

    let insertions = plan(&Payload::Files(vec![asset.clone()]), Some(&directory), "notes", &[]);
    assert_eq!(insertions[0].write, Some(Write::new("Screen-Shot-2026.png", WriteContents::CopyOf(asset))));
    // The alt text keeps the name the reader recognises.
    assert_eq!(insertions[0].markdown, "![Screen Shot 2026](Screen-Shot-2026.png)");
}

/// A link is a reference, not an embed.
#[test]
fn a_markdown_file_is_linked_inline_and_never_copied() {
    let (directory, _cleanup) = temporary_directory("DocumentDropTests");
    let sibling = directory.appending_path_component("design.md");
    write(&sibling, b"# Design\n");

    let insertions = plan(&Payload::Files(vec![sibling]), Some(&directory), "notes", &[]);
    assert_eq!(insertions[0].write, None);
    assert_eq!(insertions[0].markdown, "[design](design.md)");
    assert!(!insertions[0].is_block, "a link lands where the pointer was, inline");
}

#[test]
fn a_file_from_outside_is_linked_by_file_url() {
    let (directory, _cleanup) = temporary_directory("DocumentDropTests");
    let (elsewhere, _cleanup_elsewhere) = temporary_directory("DocumentDropTests");
    let sibling = elsewhere.appending_path_component("spec.pdf");
    write(&sibling, b"%PDF-1.4\n");

    let insertions = plan(&Payload::Files(vec![sibling.clone()]), Some(&directory), "notes", &[]);
    assert_eq!(insertions[0].write, None);
    assert!(swift::has_prefix(&insertions[0].markdown, "[spec](file://"));
    // `String(markdown.drop(while: { $0 != "(" }).dropFirst().dropLast())`.
    let start = insertions[0].markdown.find('(').unwrap();
    let destination = swift::drop_last(swift::drop_first(&insertions[0].markdown[start..], 1), 1).to_owned();
    // `MarkdownLinkDestination.classify(destination)` is `.localFile(url)`
    // exactly when `URL(string:)` parses it with the scheme `file`.
    let url = NSURL::URLWithString(&NSString::from_str(&destination)).expect("a dropped outside file must classify as a local file");
    assert_eq!(url.scheme().map(|scheme| swift::lowercased(&scheme.to_string())).as_deref(), Some("file"));
    let url = FileUrl::from_path_is_directory(&url.path().unwrap().to_string(), url.hasDirectoryPath());
    assert_eq!(url.standardized_file_url().path(), sibling.standardized_file_url().path());
}

#[test]
fn dropped_pixels_are_written_beside_the_document() {
    let png = png_data();
    let (directory, _cleanup) = temporary_directory("DocumentDropTests");
    let insertions =
        plan(&Payload::ImageData(captured_image::Payload::new(png.clone(), "png")), Some(&directory), "notes", &[]);
    assert_eq!(insertions[0].write, Some(Write::new("notes-image.png", WriteContents::Data(png))));
    assert_eq!(insertions[0].markdown, "![notes-image](notes-image.png)");
}

/// Bytes have nowhere to go without a document folder.
#[test]
fn dropped_pixels_are_refused_without_a_document_folder() {
    let png = png_data();
    assert!(plan(&Payload::ImageData(captured_image::Payload::new(png, "png")), None, "notes", &[]).is_empty());
}

#[test]
fn two_drops_in_a_row_never_collide_on_one_name() {
    let (directory, _cleanup) = temporary_directory("DocumentDropTests");
    let (elsewhere, _cleanup_elsewhere) = temporary_directory("DocumentDropTests");
    let first = elsewhere.appending_path_component("shot.png");
    let second = elsewhere.appending_path_component_is_directory("nested", true);
    std::fs::create_dir_all(second.path()).unwrap();
    let second_file = second.appending_path_component("shot.png");
    write(&first, &png_data());
    write(&second_file, &png_data());

    // Both are called `shot.png`, and one is already on disk beside the
    // document: three claims on one name inside a single drop.
    let insertions = plan(&Payload::Files(vec![first, second_file]), Some(&directory), "notes", &["shot.png"]);
    assert_eq!(
        insertions.iter().filter_map(|insertion| insertion.write.as_ref().map(|write| write.file_name.clone())).collect::<Vec<_>>(),
        vec!["shot-2.png".to_owned(), "shot-3.png".to_owned()]
    );
}

// MARK: - The single source edit

#[test]
fn a_single_link_stays_inline_at_the_drop_point() {
    let insertions = [Insertion::new(None, "[a](a.md)", false)];
    let edit = DroppedAsset::edit(&insertions, "one two three", 4);
    assert_eq!(edit.as_ref().map(|edit| edit.replacement.as_str()), Some("[a](a.md)"));
    assert_eq!(edit.as_ref().map(|edit| edit.origin), Some(4));
    assert_eq!(edit.as_ref().map(|edit| edit.caret), Some(4 + 9));
}

#[test]
fn an_image_pads_itself_into_its_own_paragraph() {
    let insertions = [Insertion::new(None, "![a](a.png)", true)];
    let edit = DroppedAsset::edit(&insertions, "one two three", 4);
    assert_eq!(edit.map(|edit| edit.replacement).as_deref(), Some("\n\n![a](a.png)\n\n"));
}

/// Several files at once is a list, not a sentence.
#[test]
fn a_multiple_file_drop_is_laid_out_as_blocks() {
    let insertions = [Insertion::new(None, "[a](a.md)", false), Insertion::new(None, "[b](b.md)", false)];
    let edit = DroppedAsset::edit(&insertions, "", 0);
    assert_eq!(edit.map(|edit| edit.replacement).as_deref(), Some("[a](a.md)\n\n[b](b.md)"));
}

#[test]
fn nothing_to_insert_is_no_edit() {
    assert_eq!(DroppedAsset::edit(&[], "one", 0), None);
}

// MARK: - The reference has to survive the round trip

fn diagnostics(text: &str, document_url: &FileUrl, root: &FileUrl) -> Vec<AssetDiagnostic> {
    let probe = AssetProbe::new(|url| {
        Some(AssetMetadata::new(
            upleft_foundation::foundation_io::file_exists(&url.path()),
            false,
            std::fs::read(url.path()).ok().map(|data| data.len() as i64),
            Some(url.path_extension()),
        ))
    });
    AssetDoctor::diagnose(
        &MarkdownParser::parse(text),
        &AssetResolutionContext::new(Some(document_url.clone()), Some(root.clone())),
        Some(&probe),
    )
}

#[test]
fn a_dropped_image_resolves_back_to_the_file_that_was_dropped() {
    let (directory, _cleanup) = temporary_directory("DocumentDropTests");
    let document_url = directory.appending_path_component("notes.md");
    let asset = directory.appending_path_component("diagram.png");
    write(&asset, &png_data());

    let markdown = plan(&Payload::Files(vec![asset.clone()]), Some(&directory), "notes", &[])[0].markdown.clone();
    let reference = AssetDoctor::references(
        &MarkdownParser::parse(&markdown),
        &AssetResolutionContext::new(Some(document_url.clone()), Some(directory.clone())),
    )
    .into_iter()
    .next()
    .expect("a reference");
    assert_eq!(reference.kind, AssetReferenceKind::RelativeLocal);
    assert_eq!(reference.url.map(|url| url.standardized_file_url()), Some(asset.standardized_file_url()));
    assert!(
        LocalAssetPolicy::request(&reference.source, Some(&document_url.to_nsurl()))
            .expect("a local request")
            .is_safe_relative
    );
    assert!(diagnostics(&markdown, &document_url, &directory).is_empty());
}

/// The angle-bracket form has to survive the parser, the doctor and the
/// renderer's own resolver.
#[test]
fn an_angle_bracket_destination_survives_the_parser_and_the_renderer() {
    let (directory, _cleanup) = temporary_directory("DocumentDropTests");
    let document_url = directory.appending_path_component("notes.md");
    let asset = directory.appending_path_component("meeting notes.png");
    write(&asset, &png_data());

    let markdown = plan(&Payload::Files(vec![asset.clone()]), Some(&directory), "notes", &[])[0].markdown.clone();
    assert_eq!(markdown, "![meeting notes](<meeting notes.png>)");
    let reference = AssetDoctor::references(
        &MarkdownParser::parse(&markdown),
        &AssetResolutionContext::new(Some(document_url.clone()), Some(directory.clone())),
    )
    .into_iter()
    .next()
    .expect("a reference");
    assert_eq!(reference.source, "meeting notes.png");
    assert_eq!(reference.url.map(|url| url.standardized_file_url()), Some(asset.standardized_file_url()));
    let request = LocalAssetPolicy::request(&reference.source, Some(&document_url.to_nsurl())).expect("a local request");
    assert!(request.is_safe_relative);
    assert_eq!(request.url.lastPathComponent().map(|name| name.to_string()).as_deref(), Some("meeting notes.png"));
    assert!(diagnostics(&markdown, &document_url, &directory).is_empty());
}

/// A filename holding a bracket would otherwise end the label early.
#[test]
fn brackets_in_a_file_name_are_escaped_in_the_label() {
    assert_eq!(DroppedAsset::alt_text("a [draft] plan.md"), "a \\[draft\\] plan");
    assert_eq!(DroppedAsset::alt_text(".hidden"), ".hidden");
    assert_eq!(DroppedAsset::alt_text(""), "Image");
}
