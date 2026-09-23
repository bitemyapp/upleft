//! Port of `shareIsAvailableBeforeTheDocumentHasEverBeenSaved` from
//! `Tests/DownrightAppTests/ShareAndCaptureTests.swift`. The file's menu-bar
//! test (the ⌃⌘S item built by `MainMenu`) needs the UI port; the rest belongs
//! to the export port.

use upleft_app::support::commands::{Command, CommandContext};

/// An Untitled window is exactly where a reader most wants Share, so the
/// precondition is `.document`, not `.documentWithFile`.
#[test]
fn share_is_available_before_the_document_has_ever_been_saved() {
    let document = CommandContext { has_document: true, ..CommandContext::default() };
    assert!(Command::Share.is_enabled(&document));
    assert!(Command::ShareAsPdf.is_enabled(&document));
    assert!(!Command::Share.is_enabled(&CommandContext::application_only(false)));
    assert!(!Command::ShareAsPdf.is_enabled(&CommandContext::application_only(false)));
}
