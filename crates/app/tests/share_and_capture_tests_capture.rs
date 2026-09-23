//! The Continuity Camera cases of
//! `Tests/DownrightAppTests/ShareAndCaptureTests.swift` that exercise
//! `CapturedImage` ("Decoding a capture", "Naming the written asset", "The
//! inserted Markdown").
//!
//! Skipped:
//! * `imageRequestsReachTheWindowControllerThroughTheTextView`,
//!   `captureWritesNextToTheDocumentAndInsertsOneUndoableReference`,
//!   `captureNeverConsumesTheSelection` and
//!   `anUntitledWindowNeitherAdvertisesNorAcceptsACapture` drive
//!   `DocumentWindowController` (not ported yet).
//! * `shareIsAFileMenuCommandWithItsOwnChord` and
//!   `editMenuCarriesTheImportFromDevicePlaceholder` test `MainMenu` (the
//!   menu port).
//!
//! The Share cases are in `share_and_capture_tests_share.rs` and
//! `share_and_capture_tests_commands.rs`.

mod asset_support;
mod common;

use std::collections::HashSet;

use asset_support::{bitmap, named_pasteboard, representation, set_data};
use common::temporary_directory;
use objc2::AnyThread;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSPasteboardTypePNG, NSPasteboardTypeString, NSPasteboardTypeTIFF};
use objc2_foundation::{NSData, NSString};
use upleft_app::assets::asset_doctor::AssetDoctor;
use upleft_app::assets::asset_resolver::{AssetMetadata, AssetProbe, AssetReferenceKind, AssetResolutionContext};
use upleft_app::assets::captured_image::CapturedImage;
use upleft_core::parser::MarkdownParser;
use upleft_render::fragments::local_asset_policy::LocalAssetPolicy;
use upleft_swift_text as swift;

fn png() -> Vec<u8> {
    representation(&bitmap(8, 8), NSBitmapImageFileType::PNG).expect("png")
}

// MARK: - Decoding a capture

#[test]
fn png_captures_are_written_verbatim() {
    let png = png();
    let pasteboard = named_pasteboard("ShareAndCaptureTests");
    // SAFETY: an AppKit constant.
    set_data(&pasteboard, &png, unsafe { NSPasteboardTypePNG });
    let payload = CapturedImage::payload(&pasteboard).expect("a payload");
    assert_eq!(payload.file_extension, "png");
    assert_eq!(payload.data, png);
}

/// TIFF is not a supported extension, so passing it through would hand the
/// Asset Doctor a diagnostic on arrival.
#[test]
fn unsupported_capture_formats_are_reencoded_as_png() {
    let tiff = representation(&bitmap(8, 8), NSBitmapImageFileType::TIFF).expect("tiff");
    let pasteboard = named_pasteboard("ShareAndCaptureTests");
    // SAFETY: an AppKit constant.
    set_data(&pasteboard, &tiff, unsafe { NSPasteboardTypeTIFF });
    let payload = CapturedImage::payload(&pasteboard).expect("a payload");
    assert_eq!(payload.file_extension, "png");
    assert!(NSBitmapImageRep::initWithData(NSBitmapImageRep::alloc(), &NSData::with_bytes(&payload.data)).is_some());
    assert!(AssetResolutionContext::default().supported_extensions.contains(&payload.file_extension));
}

#[test]
fn non_image_pasteboards_are_declined() {
    let pasteboard = named_pasteboard("ShareAndCaptureTests");
    // SAFETY: an AppKit constant.
    pasteboard.setString_forType(&NSString::from_str("not an image"), unsafe { NSPasteboardTypeString });
    assert_eq!(CapturedImage::payload(&pasteboard), None);
    // SAFETY: AppKit constants.
    assert!(CapturedImage::accepts_return_type(&unsafe { NSPasteboardTypePNG }.to_string()));
    assert!(!CapturedImage::accepts_return_type(&unsafe { NSPasteboardTypeString }.to_string()));
}

// MARK: - Naming the written asset

/// The written name and the parsed destination have to be the same string.
#[test]
fn asset_names_survive_the_destination_parser() {
    assert_eq!(CapturedImage::slug("meeting notes"), "meeting-notes");
    assert_eq!(CapturedImage::slug("q3 #plan (final) 50%"), "q3-plan-final-50");
    assert_eq!(CapturedImage::slug("Reuni\u{f3}n"), "Reuni\u{f3}n");
    assert_eq!(CapturedImage::slug("../etc/passwd"), "etcpasswd");
    assert_eq!(CapturedImage::slug("   "), "");
}

#[test]
fn repeated_captures_never_overwrite_an_earlier_one() {
    let mut taken: HashSet<String> = HashSet::new();
    let first = CapturedImage::unique_file_name_for_document("notes", "png", |name| taken.contains(name));
    assert_eq!(first, "notes-photo.png");
    taken.insert(first);
    let second = CapturedImage::unique_file_name_for_document("notes", "png", |name| taken.contains(name));
    assert_eq!(second, "notes-photo-2.png");
    taken.insert(second);
    let third = CapturedImage::unique_file_name_for_document("notes", "jpg", |name| taken.contains(name));
    assert_eq!(third, "notes-photo.jpg");
}

// MARK: - The inserted Markdown

#[test]
fn insertion_pads_itself_into_its_own_paragraph() {
    // Mid-paragraph: needs a blank line on both sides.
    let middle = CapturedImage::insertion("a.png", "a", "one two\n", 3);
    assert_eq!(middle.replacement, "\n\n![a](a.png)\n\n");
    assert_eq!(middle.caret, 3 + swift::utf16_count("\n\n![a](a.png)"));

    // Already on a blank line between two paragraphs: pad nothing.
    let blank = CapturedImage::insertion("a.png", "a", "one\n\n\n\ntwo\n", 5);
    assert_eq!(blank.replacement, "![a](a.png)");

    // One newline short on the trailing side: pad exactly that one.
    let nearly_blank = CapturedImage::insertion("a.png", "a", "one\n\n\n\ntwo\n", 6);
    assert_eq!(nearly_blank.replacement, "![a](a.png)\n");

    // Start of an empty document.
    assert_eq!(CapturedImage::insertion("a.png", "a", "", 0).replacement, "![a](a.png)");

    // End of a document that ends in a single newline.
    let end = CapturedImage::insertion("a.png", "a", "body\n", 5);
    assert_eq!(end.replacement, "\n![a](a.png)");
}

#[test]
fn insertion_clamps_an_out_of_range_caret_rather_than_trapping() {
    let insertion = CapturedImage::insertion("a.png", "a", "abc", 9_999);
    assert!(insertion.caret <= swift::utf16_count("abc") + swift::utf16_count(&insertion.replacement));
    assert!(swift::has_suffix(&insertion.replacement, "![a](a.png)"));
}

/// The reference has to come back out of the parser pointing at the exact
/// file that was written, as a *safe relative* asset.
#[test]
fn the_inserted_reference_resolves_back_to_the_written_file() {
    let (directory, _cleanup) = temporary_directory("ShareAndCaptureTests");
    let document_url = directory.appending_path_component("meeting notes.md");
    let file_name = CapturedImage::unique_file_name_in(&directory, "meeting notes", "png");
    assert_eq!(file_name, "meeting-notes-photo.png");
    let asset = directory.appending_path_component(&file_name);
    std::fs::write(asset.path(), png()).unwrap();

    let insertion = CapturedImage::insertion(&file_name, &CapturedImage::alt_text_for_file_named(&file_name), "", 0);
    let text = insertion.replacement;
    let context = AssetResolutionContext::new(Some(document_url.clone()), Some(directory.clone()));
    let reference = AssetDoctor::references(&MarkdownParser::parse(&text), &context).into_iter().next().expect("a reference");
    assert_eq!(reference.kind, AssetReferenceKind::RelativeLocal);
    assert_eq!(reference.url.map(|url| url.standardized_file_url()), Some(asset.standardized_file_url()));

    let request = LocalAssetPolicy::request(&reference.source, Some(&document_url.to_nsurl())).expect("a local request");
    assert!(request.is_safe_relative);

    // And it arrives with no diagnostics of its own.
    let probe = AssetProbe::new(|url| {
        let exists = upleft_foundation::foundation_io::file_exists(&url.path());
        Some(AssetMetadata::new(
            exists,
            false,
            std::fs::read(url.path()).ok().map(|data| data.len() as i64),
            Some(url.path_extension()),
        ))
    });
    let diagnostics = AssetDoctor::diagnose(&MarkdownParser::parse(&text), &context, Some(&probe));
    assert!(diagnostics.is_empty(), "a fresh capture must not arrive pre-diagnosed");
}
