//! Port of `Tests/DownrightAppTests/ExportSnapshotTests.swift`: export
//! captures the current buffer even while its async parse is pending.
//!
//! `@MainActor` and `.serialized`: this binary owns the main thread
//! (`harness = false`, `tests/main_thread`), runs in a sandbox
//! (`tests/document_window_support`), and orders no window in.

mod document_window_support;
mod main_thread;

use document_window_support::{enter_sandbox, leave_sandbox, mtm};
use objc2_foundation::{NSRange, NSString};
use upleft_app::app::document_window_controller::DocumentWindowController;

fn export_includes_edits_before_async_parsing_finishes(for_print: bool) {
    let controller = DocumentWindowController::new(mtm());
    let document = controller.markdown_document().clone();
    document.adopt("# Original\n", None);
    document.storage().replaceCharactersInRange_withString(NSRange::new(2, 8), &NSString::from_str("Current"));
    assert_ne!(document.parsed().text, document.text());

    let html = controller.exporter(for_print).html();
    assert!(html.contains(">Current</h1>"));
    assert!(!html.contains(">Original</h1>"));
    assert_eq!(document.text(), "# Current\n");
    controller.close();
}

fn main() {
    enter_sandbox();
    main_thread::run(&[
        ("export_includes_edits_before_async_parsing_finishes(false)", || {
            export_includes_edits_before_async_parsing_finishes(false)
        }),
        ("export_includes_edits_before_async_parsing_finishes(true)", || {
            export_includes_edits_before_async_parsing_finishes(true)
        }),
    ]);
    leave_sandbox();
}
