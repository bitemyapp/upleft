//! The window cases of `Tests/DownrightAppTests/ShareAndCaptureTests.swift`
//! ("The responder chain and the real insertion"): a Continuity Camera
//! capture through `DocumentWindowController` (`+ContinuityCamera` reads the
//! pasteboard; `+AssetInsertion` inserts the reference).
//!
//! Runs on the main thread (`harness = false`, `main_thread`), inside the
//! support-directory sandbox, with a test `Preferences.shared`.
//!
//! `imageRequestsReachTheWindowControllerThroughTheTextView` calls
//! `controller.showWindow(nil)` in Swift. Tests never order a titled window
//! in (AppKit pulls it onto a display), so the port asks the responder chain
//! of the window that was never shown; the text view, window and controller
//! are chained by `NSWindowController` itself, not by showing the window.
//! The assertions are Swift's.

mod asset_support;
mod common;
mod document_support;
mod main_thread;

use asset_support::{Board, bitmap, named_pasteboard, representation, set_data};
use common::temporary_directory;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{NSBitmapImageFileType, NSPasteboardType, NSPasteboardTypePNG};
use upleft_app::ai::snapshot_store::SnapshotStore;
use upleft_app::app::document_window_controller::DocumentWindowController;
use upleft_app::support::preferences::Preferences;
use upleft_core::NSRange;
use upleft_foundation::url::FileUrl;
use upleft_render::render_contracts::RenderMode;
use upleft_swift_text as swift;

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("window tests run on the main thread")
}

/// `defer { controller.close() }`.
struct Closing(Retained<DocumentWindowController>);

impl Drop for Closing {
    fn drop(&mut self) {
        self.0.close();
    }
}

fn png_type() -> &'static NSPasteboardType {
    // SAFETY: an AppKit constant.
    unsafe { NSPasteboardTypePNG }
}

/// `makeController(text:at:)`.
fn make_controller(text: &str, url: &FileUrl) -> Retained<DocumentWindowController> {
    std::fs::write(url.path(), text).unwrap();
    let controller = DocumentWindowController::new(mtm());
    controller.open(url, RenderMode::Live).expect("the document opens");
    controller
}

/// `makePasteboard(_:type:)`; released globally when dropped.
fn make_pasteboard(data: &[u8], pasteboard_type: &NSPasteboardType) -> Board {
    let pasteboard = named_pasteboard("ShareAndCaptureTests");
    set_data(&pasteboard, data, pasteboard_type);
    pasteboard
}

fn png() -> Vec<u8> {
    representation(&bitmap(8, 8), NSBitmapImageFileType::PNG).expect("png")
}

/// The text view holds Markdown source, not attachments, so it declines
/// image return types and the question reaches the controller. If it ever
/// stops reaching it, Continuity Camera silently vanishes from the menu.
fn image_requests_reach_the_window_controller_through_the_text_view() {
    let (directory, _directory) = temporary_directory("ShareAndCaptureTests");
    let url = directory.appending_path_component("notes.md");
    let controller = make_controller("# Title\n", &url);
    let _closing = Closing(controller.clone());
    // Swift calls `controller.showWindow(nil)` here; see the module comment.

    let text_view = controller.primary_container().text_view().clone();
    let imports_graphics: bool = unsafe { msg_send![&*text_view, importsGraphics] };
    assert!(!imports_graphics);
    let requestor: Option<Retained<AnyObject>> = unsafe {
        msg_send![&*text_view, validRequestorForSendType: None::<&NSPasteboardType>, returnType: Some(png_type())]
    };
    let controller_object: *const AnyObject = Retained::as_ptr(&controller).cast();
    assert!(requestor.as_ref().is_some_and(|requestor| Retained::as_ptr(requestor) == controller_object));
    // A service that also wants to read a selection out of us gets nothing.
    assert!(controller.valid_requestor(Some(png_type()), Some(png_type())).is_none());
}

fn capture_writes_next_to_the_document_and_inserts_one_undoable_reference() {
    let (directory, _directory) = temporary_directory("ShareAndCaptureTests");
    let url = directory.appending_path_component("meeting notes.md");
    let source = "# Title\n\nBody text.\n";
    let controller = make_controller(source, &url);
    let _closing = Closing(controller.clone());
    controller.container_text_view().set_source_selected_ranges(&[NSRange::new(swift::utf16_count(source), 0)]);

    let png = png();
    let pasteboard = make_pasteboard(&png, png_type());

    assert!(controller.read_selection(&pasteboard));

    let asset = directory.appending_path_component("meeting-notes-photo.png");
    assert!(std::path::Path::new(&asset.path()).exists());
    assert_eq!(std::fs::read(asset.path()).unwrap(), png);
    assert_eq!(
        controller.markdown_document().text(),
        source.to_owned() + "\n![meeting-notes-photo](meeting-notes-photo.png)"
    );

    // One explicit boundary, so one Undo puts the document back exactly.
    controller.markdown_document().undo_manager().undo();
    assert_eq!(controller.markdown_document().text(), source);
}

/// The capture is triggered on a phone; whatever was selected here is out of
/// sight by the time it lands, so it must survive.
fn capture_never_consumes_the_selection() {
    let (directory, _directory) = temporary_directory("ShareAndCaptureTests");
    let url = directory.appending_path_component("notes.md");
    let source = "# Title\n\nBody text.\n";
    let controller = make_controller(source, &url);
    let _closing = Closing(controller.clone());
    let selection = NSRange::new(9, 4); // "Body"
    controller.container_text_view().set_source_selected_ranges(&[selection]);

    let png = png();
    let pasteboard = make_pasteboard(&png, png_type());
    assert!(controller.read_selection(&pasteboard));

    assert_eq!(controller.markdown_document().text(), "# Title\n\n![notes-photo](notes-photo.png)\n\nBody text.\n");
    assert!(swift::contains(&controller.markdown_document().text(), "Body text."));
}

/// The never-saved window: nothing is advertised, so AppKit never offers a
/// capture there, and the write path refuses it a second time.
fn an_untitled_window_neither_advertises_nor_accepts_a_capture() {
    let controller = DocumentWindowController::new(mtm());
    let _closing = Closing(controller.clone());
    assert!(controller.markdown_document().url().is_none());
    assert!(controller.valid_requestor(None, Some(png_type())).is_none());

    let png = png();
    let pasteboard = make_pasteboard(&png, png_type());
    let before = controller.markdown_document().text();
    assert!(!controller.read_selection(&pasteboard));
    assert_eq!(controller.markdown_document().text(), before);
}

fn main() {
    let sandbox = document_support::sandbox();
    // `Preferences.shared` inside the sandbox, publishing nothing.
    Preferences::install_shared(Preferences::for_testing(
        sandbox.appending_path_component("preferences.json"),
        Some(SnapshotStore::shared().clone()),
    ));
    main_thread::run(&[
        (
            "image_requests_reach_the_window_controller_through_the_text_view",
            image_requests_reach_the_window_controller_through_the_text_view,
        ),
        (
            "capture_writes_next_to_the_document_and_inserts_one_undoable_reference",
            capture_writes_next_to_the_document_and_inserts_one_undoable_reference,
        ),
        ("capture_never_consumes_the_selection", capture_never_consumes_the_selection),
        (
            "an_untitled_window_neither_advertises_nor_accepts_a_capture",
            an_untitled_window_neither_advertises_nor_accepts_a_capture,
        ),
    ]);
    document_support::remove_sandbox();
}
