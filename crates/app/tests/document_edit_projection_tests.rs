//! Port of `Tests/DownrightAppTests/DocumentEditProjectionTests.swift`
//! ("Document edits project fragment payloads", `.serialized`).
//!
//! Fragment payloads are reference values riding on NSTextStorage attribute
//! runs.  Runs move with an edit; payload ranges do not — unless every edit
//! funnel projects them.  These tests pin the document-level funnels.
//!
//! One change: the Swift tests decorate the storage through
//! `MarkdownTextView.update(document:dirty:)`, which arrives with the view
//! port. Here the code block's `FragmentPayload` is attached directly, over
//! the block's source range and with the block's range as its source range,
//! which is the part of the decoration these tests read.

mod document_support;
mod main_thread;

use std::sync::Arc;

use document_support::{Fixture, document};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use upleft_app::ai::markdown_document::MarkdownDocument;
use upleft_core::model::{BlockContent, MDBlock, ParsedDocument};
use upleft_core::parser::MarkdownParser;
use upleft_render::render_contracts::{FragmentKind, FragmentPayload, attribute_keys};
use upleft_swift_text::NSRange;

const SOURCE: &str = "Intro paragraph.\n\n```swift\nlet a = 1\n```\n\nTail paragraph.";

fn fixture() -> Fixture {
    Fixture::new("downright-payload-projection", "note.md", SOURCE)
}

fn fenced_code_block(document: &ParsedDocument) -> Option<Arc<MDBlock>> {
    let mut found: Option<Arc<MDBlock>> = None;
    document.root.walk(&mut |block| {
        if found.is_none()
            && let BlockContent::CodeBlock { language: Some(_), is_fenced: true, .. } = &block.content
        {
            found = Some(block.clone());
        }
    });
    found
}

/// Stands in for `view.update(document: document.parsed, dirty: .wholesale)`.
fn decorate(document: &MarkdownDocument) {
    let block = fenced_code_block(&document.parsed()).expect("fixture lost its code fence");
    let payload = FragmentPayload::new(FragmentKind::CodeBlock, block.range, block.identity, "swift");
    let value: &AnyObject = payload.as_ref();
    // SAFETY: `drFragment` carries a `FragmentPayload`.
    unsafe {
        document.storage().addAttribute_value_range(
            attribute_keys::dr_fragment(),
            value,
            objc2_foundation::NSRange::new(block.range.location as usize, block.range.length as usize),
        );
    }
}

fn payload_at(document: &MarkdownDocument, location: isize) -> Option<Retained<FragmentPayload>> {
    // SAFETY: an in-range attribute query.
    let value = unsafe {
        document.storage().attribute_atIndex_effectiveRange(attribute_keys::dr_fragment(), location as usize, std::ptr::null_mut())
    }?;
    value.downcast::<FragmentPayload>().ok()
}

fn fence_line() -> NSRange {
    let location = SOURCE.find("```swift").expect("fixture has a fence") as isize;
    NSRange::new(location, 8)
}

fn command_edits_shift_payload_ranges_below_the_edit() {
    let fixture = fixture();
    let document = document();
    document.open(&fixture.url).unwrap();
    decorate(&document);

    // Locate the fenced code block's payload in the decorated storage.
    let fence_line = fence_line();
    let payload_before = payload_at(&document, fence_line.location).expect("the code block must carry a fragment payload");

    // A document-level edit above the fence: two inserted characters.
    assert!(document.replace(NSRange::new(0, 0), "Hi", Some("Type")));

    // The payload must now describe where the block lives *after* the edit,
    // matching what a fresh parse of the current text says.
    let reparsed = MarkdownParser::parse(&document.text());
    let expected = fenced_code_block(&reparsed).map_or(NSRange::new(0, 0), |block| block.range);
    assert!(expected.length > 0);
    assert_eq!(payload_before.source_range().location, fence_line.location + 2);
    assert_eq!(
        payload_before.source_range().location,
        expected.location,
        "projected payload must agree with the fresh parse"
    );
    document.close();
}

fn undo_redo_keeps_payloads_aligned() {
    let fixture = fixture();
    let document = document();
    document.open(&fixture.url).unwrap();
    decorate(&document);

    let fence_line = fence_line();
    let payload = payload_at(&document, fence_line.location);
    let original_location = fence_line.location;

    assert!(document.replace(NSRange::new(0, 0), "XYZ", Some("Type")));
    assert_eq!(payload.as_ref().map(|payload| payload.source_range().location), Some(original_location + 3));

    document.undo_manager().undo();
    assert_eq!(
        payload.as_ref().map(|payload| payload.source_range().location),
        Some(original_location),
        "undo is itself an edit through replace() and must project too"
    );

    document.undo_manager().redo();
    assert_eq!(payload.as_ref().map(|payload| payload.source_range().location), Some(original_location + 3));
    document.close();
}

fn main() {
    document_support::sandbox();
    main_thread::run(&[
        ("command_edits_shift_payload_ranges_below_the_edit", command_edits_shift_payload_ranges_below_the_edit),
        ("undo_redo_keeps_payloads_aligned", undo_redo_keeps_payloads_aligned),
    ]);
    document_support::remove_sandbox();
}
