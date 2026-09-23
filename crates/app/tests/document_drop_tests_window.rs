//! The window cases of `Tests/DownrightAppTests/DocumentDropTests.swift`
//! ("The real window"): a drop through `DocumentWindowController`
//! (`App/DocumentWindowController+AssetInsertion.swift`). The model cases
//! are in `document_drop_tests.rs`.
//!
//! The Swift cases are `@MainActor`: this binary owns the main thread
//! (`harness = false`), in `controller_support`'s sandbox. The window is
//! never ordered in.
//!
//! Skipped: `aFailedWriteInsertsNothingAndLeavesNoDebris`. Its failed write
//! presents the error as an `NSAlert` sheet on the document window
//! (`presentOperationError`), and a sheet begun on a titled window that was
//! never ordered in can reach a display; tests never put a window on screen.
//!
//! Named pasteboards are released when a test ends; nothing reaches the
//! general pasteboard.

mod asset_support;
mod controller_support;

use asset_support::{Board, named_pasteboard, png_data, set_data, write_files};
use controller_support::{Closing, Removing, make_controller, new_controller, range_of, temporary_directory};
use objc2::Message;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG};
use upleft_app::app::document_window_controller::DocumentWindowController;
use upleft_app::app::document_window_controller_delegates::MarkdownLinkDestination;
use upleft_core::NSRange;
use upleft_foundation::url::FileUrl;
use upleft_render::render_contracts::RenderMode;
use upleft_render::view::markdown_text_view_delegate::DocumentDrop;
use upleft_swift_text as swift;

/// `drop(_:at:on:)`.
fn drop_on(board: &NSPasteboard, offset: isize, controller: &DocumentWindowController) -> bool {
    let view = controller.container_text_view();
    controller.markdown_text_view_did_accept_drop(&view, &DocumentDrop::new(board.retain(), offset))
}

/// `pasteboard(files:)`.
fn pasteboard_with_files(files: &[FileUrl]) -> Board {
    let board = named_pasteboard("DocumentDropTests");
    write_files(&board, files);
    board
}

/// `pasteboard(png:)`.
fn pasteboard_with_png(png: &[u8]) -> Board {
    let board = named_pasteboard("DocumentDropTests");
    // SAFETY: an AppKit constant.
    set_data(&board, png, unsafe { NSPasteboardTypePNG });
    board
}

/// End to end, at a source offset the reader chose, not at the caret.
fn dropping_an_image_inserts_one_undoable_reference_at_the_drop_point() {
    let directory = temporary_directory("DocumentDropTests");
    let elsewhere = temporary_directory("DocumentDropTests");
    let _remove_directory = Removing(directory.clone());
    let _remove_elsewhere = Removing(elsewhere.clone());
    let url = directory.appending_path_component("notes.md");
    let source = "# Title\n\nFirst paragraph.\n\nSecond paragraph.\n";
    let controller = Closing(make_controller(source, &url, RenderMode::Live));
    // The caret is somewhere else entirely: the drop must ignore it.
    controller.container_text_view().set_source_selected_ranges(&[NSRange::new(0, 0)]);

    let origin = elsewhere.appending_path_component("chart.png");
    let bytes = png_data();
    std::fs::write(origin.path(), &bytes).unwrap();

    let drop_offset = range_of(source, "Second paragraph.").location;
    assert!(drop_on(&pasteboard_with_files(&[origin]), drop_offset, &controller));

    let copied = directory.appending_path_component("chart.png");
    assert_eq!(std::fs::read(copied.path()).unwrap(), bytes);
    assert_eq!(
        controller.markdown_document().text(),
        "# Title\n\nFirst paragraph.\n\n![chart](chart.png)\n\nSecond paragraph.\n"
    );
    // One boundary, so one Undo puts the document back exactly, and leaves
    // the copied file alone, which is the less destructive half.
    controller.markdown_document().undo_manager().undo();
    assert_eq!(controller.markdown_document().text(), source);
    assert!(std::path::Path::new(&copied.path()).exists());
}

fn dropping_pixels_writes_a_file_named_after_the_document() {
    let directory = temporary_directory("DocumentDropTests");
    let _remove = Removing(directory.clone());
    let url = directory.appending_path_component("meeting notes.md");
    let controller = Closing(make_controller("# Title\n", &url, RenderMode::Live));

    let bytes = png_data();
    assert!(drop_on(&pasteboard_with_png(&bytes), 8, &controller));
    let written = directory.appending_path_component("meeting-notes-image.png");
    assert_eq!(std::fs::read(written.path()).unwrap(), bytes);
    assert_eq!(controller.markdown_document().text(), "# Title\n\n![meeting-notes-image](meeting-notes-image.png)");
}

fn a_never_saved_window_takes_files_but_not_loose_image_bytes() {
    let directory = temporary_directory("DocumentDropTests");
    let _remove = Removing(directory.clone());
    let controller = Closing(new_controller());
    assert!(controller.markdown_document().url().is_none());

    let view = controller.container_text_view();
    let bytes = png_data();
    let pixels = pasteboard_with_png(&bytes);
    assert!(!controller.markdown_text_view_can_accept_drop(&view, &DocumentDrop::new(pixels.retain(), 0)));
    let before = controller.markdown_document().text();
    assert!(!drop_on(&pixels, 0, &controller));
    assert_eq!(controller.markdown_document().text(), before);

    let file = directory.appending_path_component("design.md");
    std::fs::write(file.path(), "# Design\n").unwrap();
    let board = pasteboard_with_files(&[file]);
    assert!(controller.markdown_text_view_can_accept_drop(&view, &DocumentDrop::new(board.retain(), 0)));
    assert!(drop_on(&board, 0, &controller));
    assert!(swift::contains(&controller.markdown_document().text(), "[design](file://"));
}

/// A dropped `.md` sibling lands inline, and the link the reader then clicks
/// has to open the file it names.
fn a_dropped_sibling_becomes_a_link_that_resolves() {
    let directory = temporary_directory("DocumentDropTests");
    let _remove = Removing(directory.clone());
    let url = directory.appending_path_component("notes.md");
    let controller = Closing(make_controller("See also.\n", &url, RenderMode::Live));
    let sibling = directory.appending_path_component("design.md");
    std::fs::write(sibling.path(), "# Design\n").unwrap();

    assert!(drop_on(&pasteboard_with_files(&[sibling]), 8, &controller));
    assert_eq!(controller.markdown_document().text(), "See also[design](design.md).\n");

    let MarkdownLinkDestination::Relative(relative) = MarkdownLinkDestination::classify("design.md") else {
        panic!("a sibling link must classify as relative");
    };
    assert!(std::path::Path::new(&directory.appending_path_component(&relative).path()).exists());
}

fn main() {
    controller_support::prepare();
    controller_support::main_thread::run(&[
        (
            "dropping_an_image_inserts_one_undoable_reference_at_the_drop_point",
            dropping_an_image_inserts_one_undoable_reference_at_the_drop_point,
        ),
        (
            "dropping_pixels_writes_a_file_named_after_the_document",
            dropping_pixels_writes_a_file_named_after_the_document,
        ),
        (
            "a_never_saved_window_takes_files_but_not_loose_image_bytes",
            a_never_saved_window_takes_files_but_not_loose_image_bytes,
        ),
        ("a_dropped_sibling_becomes_a_link_that_resolves", a_dropped_sibling_becomes_a_link_that_resolves),
    ]);
    controller_support::finish();
}
