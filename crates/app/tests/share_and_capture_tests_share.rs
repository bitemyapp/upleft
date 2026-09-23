//! The Share cases of `Tests/DownrightAppTests/ShareAndCaptureTests.swift`
//! ("What Share actually sends").
//!
//! Not here: `shareIsAFileMenuCommandWithItsOwnChord` and
//! `shareIsAvailableBeforeTheDocumentHasEverBeenSaved` test `Command`,
//! `KeybindingDefaults` and `MainMenu` (the commands port), and the
//! Continuity Camera cases test capture and the window controller.

mod common;

use common::temporary_directory;
use upleft_app::export::document_share::{DocumentShareSource, DocumentShareStaging};
use upleft_core::{ByteFidelity, LineEnding, TextEncodingKind};
use upleft_foundation::url::FileUrl;

#[test]
fn saved_and_unmodified_document_shares_its_own_file() {
    let url = FileUrl::from_path("/tmp/notes/design.md");
    let source = DocumentShareSource::choose(Some(&url), false, "design");
    assert_eq!(source, DocumentShareSource::DocumentFile(url));
}

/// The case that would otherwise be a silent lie.
#[test]
fn modified_document_shares_the_buffer_not_the_stale_file() {
    let (directory, _cleanup) = temporary_directory("ShareAndCaptureTests");
    let url = directory.appending_path_component("design.md");
    std::fs::write(url.path(), "on disk\n").unwrap();

    let source = DocumentShareSource::choose(Some(&url), true, "design");
    assert_eq!(source, DocumentShareSource::BufferSnapshot { file_name: "design.md".into() });

    let staged =
        DocumentShareStaging::file_url(&source, || "in the buffer\n".into(), ByteFidelity::DEFAULT, &directory).unwrap();
    assert_ne!(staged, url);
    // The receiver sees the document's own filename, not a staging name.
    assert_eq!(staged.last_path_component(), "design.md");
    assert_eq!(std::fs::read_to_string(staged.path()).unwrap(), "in the buffer\n");
    // Share is not a save: the original must be exactly as it was.
    assert_eq!(std::fs::read_to_string(url.path()).unwrap(), "on disk\n");
}

#[test]
fn never_saved_document_shares_a_named_markdown_file() {
    let (directory, _cleanup) = temporary_directory("ShareAndCaptureTests");

    let source = DocumentShareSource::choose(None, true, "Untitled");
    assert_eq!(source, DocumentShareSource::BufferSnapshot { file_name: "Untitled.md".into() });

    let staged = DocumentShareStaging::file_url(&source, || "# Draft\n".into(), ByteFidelity::DEFAULT, &directory).unwrap();
    assert_eq!(staged.path_extension(), "md");
    assert!(std::path::Path::new(&staged.path()).exists());
}

/// A shared copy is a copy (§3.1).
#[test]
fn snapshot_keeps_the_documents_byte_fidelity() {
    let (directory, _cleanup) = temporary_directory("ShareAndCaptureTests");
    let fidelity = ByteFidelity::new(TextEncodingKind::Utf8, false, LineEnding::Crlf, true);
    let staged = DocumentShareStaging::file_url(
        &DocumentShareSource::BufferSnapshot { file_name: "crlf.md".into() },
        || "one\ntwo\n".into(),
        fidelity,
        &directory,
    )
    .unwrap();
    let data = std::fs::read(staged.path()).unwrap();
    assert_eq!(String::from_utf8_lossy(&data), "one\r\ntwo\r\n");
}

/// A display name is arbitrary text. `/` in it would redirect the write.
#[test]
fn staged_names_cannot_escape_the_staging_directory() {
    let (directory, _cleanup) = temporary_directory("ShareAndCaptureTests");
    let source = DocumentShareSource::choose(None, true, "../../etc/passwd");
    assert_eq!(source, DocumentShareSource::BufferSnapshot { file_name: "etc-passwd.md".into() });
    let staged = DocumentShareStaging::file_url(&source, || "x\n".into(), ByteFidelity::DEFAULT, &directory).unwrap();
    assert_eq!(staged.deleting_last_path_component().deleting_last_path_component().last_path_component(), "Downright-Share");

    let pdf = DocumentShareStaging::pdf_url("notes/../secret", &directory).unwrap();
    assert_eq!(pdf.last_path_component(), "notes-..-secret.pdf");
}

/// Two shares in the same second must not collide on one path.
#[test]
fn every_share_gets_its_own_staging_directory() {
    let (directory, _cleanup) = temporary_directory("ShareAndCaptureTests");
    let snapshot = DocumentShareSource::BufferSnapshot { file_name: "a.md".into() };
    let first = DocumentShareStaging::file_url(&snapshot, || "one\n".into(), ByteFidelity::DEFAULT, &directory).unwrap();
    let second = DocumentShareStaging::file_url(&snapshot, || "two\n".into(), ByteFidelity::DEFAULT, &directory).unwrap();
    assert_ne!(first, second);
    assert_eq!(std::fs::read_to_string(first.path()).unwrap(), "one\n");
    assert_eq!(std::fs::read_to_string(second.path()).unwrap(), "two\n");
}
