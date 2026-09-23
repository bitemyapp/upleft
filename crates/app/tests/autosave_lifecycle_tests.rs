//! Port of `Tests/DownrightAppTests/AutosaveLifecycleTests.swift` ("Autosave
//! and close lifetime", `.serialized`): the document half of the save
//! boundary.
//!
//! Not ported (they need `DocumentWindowController`, a window, which arrives
//! with the UI port): `occlusionSaveBelongsToTheAutosaveSetting`,
//! `pathTokenActionsStayInertForMissingPaths`.

mod document_support;
mod main_thread;

use document_support::{Fixture, document, read_text, whole};
use upleft_app::ai::markdown_document::SaveIntent;

fn closed_document_refuses_implicit_and_explicit_saves() {
    let fixture = Fixture::new("downright-autosave-lifetime", "note.md", "before\n");
    let document = document();
    document.open(&fixture.url).unwrap();
    assert!(document.replace(whole(&document), "after\n", Some("Replace")));
    document.close();

    // A straggler implicit save (queued autosave work, occlusion during
    // teardown) must be a silent no-op, not a resurrection of the buffer.
    assert!(document.save_if_needed(SaveIntent::Normal).is_ok(), "saving a closed document must not surface an error");
    assert_eq!(read_text(&fixture.url), "before\n");

    // Even a direct save call finds no owner to consent to the write.
    document.save(SaveIntent::Normal).unwrap();
    assert_eq!(read_text(&fixture.url), "before\n");
}

fn main() {
    document_support::sandbox();
    main_thread::run(&[("closed_document_refuses_implicit_and_explicit_saves", closed_document_refuses_implicit_and_explicit_saves)]);
    document_support::remove_sandbox();
}
