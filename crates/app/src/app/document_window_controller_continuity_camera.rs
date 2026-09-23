//! Port of `App/DocumentWindowController+ContinuityCamera.swift`:
//! Continuity Camera's "Take Photo" and "Scan Documents", served by a nearby
//! iPhone or iPad straight into the document at the caret.
//!
//! The machinery is the Services one. AppKit inserts the two menu items into
//! `MainMenu`'s Insert submenu only when something in the responder chain
//! says it can accept an image, and delivers the capture by calling
//! `readSelectionFromPasteboard:` on whatever said so. The window controller
//! is that responder, not the text view (its `importsGraphics` is off).
//!
//! The Objective-C entry points (`validRequestorForSendType:returnType:`,
//! `readSelectionFromPasteboard:`) are declared in
//! `document_window_controller.rs`'s `define_class!` and forward here.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::NSPasteboard;
use objc2_foundation::{NSData, NSDataWritingOptions, NSString};

use crate::app::document_window_controller::DocumentWindowController;
use crate::assets::captured_image::CapturedImage;

impl DocumentWindowController {
    /// `validRequestor(forSendType:returnType:)`: advertises the image
    /// return types Continuity Camera looks for, gated on the document
    /// having a file (an untitled window has no folder to make an image path
    /// relative to).
    pub fn valid_requestor(
        &self,
        send_type: Option<&NSString>,
        return_type: Option<&NSString>,
    ) -> Option<Retained<AnyObject>> {
        // A send type means the service also wants to *read* a selection out
        // of this object, which it cannot: nothing here writes to a pasteboard.
        let wants_nothing_from_us = send_type.is_none_or(|send_type| send_type.length() == 0);
        if let Some(return_type) = return_type
            && wants_nothing_from_us
            && self.markdown_document().url().is_some()
            && CapturedImage::accepts_return_type(&return_type.to_string())
        {
            return Some(objc2::Message::retain(self.as_ref() as &AnyObject));
        }
        unsafe { msg_send![super(self), validRequestorForSendType: send_type, returnType: return_type] }
    }

    /// `readSelection(from:)`: writes the image next to the document and
    /// inserts one Markdown reference at the caret as a single explicit
    /// source mutation with its own undo boundary. Returning false leaves the
    /// capture unconsumed, so AppKit reports the failure itself.
    pub fn read_selection(&self, pasteboard: &NSPasteboard) -> bool {
        // `validRequestor` already refuses an untitled window; this enforces
        // the same invariant at the point of the write.
        let Some(document_url) = self.markdown_document().url() else { return false };
        let Some(payload) = CapturedImage::payload(pasteboard) else { return false };

        let directory = document_url.deleting_last_path_component();
        let file_name = CapturedImage::unique_file_name_in(
            &directory,
            &document_url.deleting_path_extension().last_path_component(),
            &payload.file_extension,
        );
        // `.withoutOverwriting` closes the gap between picking a free name
        // and using it.
        let data = NSData::with_bytes(&payload.data);
        let destination = directory.appending_path_component(&file_name).to_nsurl();
        if let Err(error) = data.writeToURL_options_error(&destination, NSDataWritingOptions::WithoutOverwriting) {
            self.present_operation_error(
                "Couldn\u{2019}t save the captured image",
                &error.localizedDescription().to_string(),
            );
            return false;
        }

        self.insert_captured_image_reference(&file_name);
        true
    }

    /// One insertion at the caret, with an undo boundary, and nothing else.
    /// Deliberately inserts at the *start* of the selection instead of
    /// replacing it: the capture is triggered on a phone, and whatever was
    /// selected is out of mind by the time it arrives.
    fn insert_captured_image_reference(&self, file_name: &str) {
        // Source coordinates throughout: `sourceSelectedRange` is already in
        // the document's own UTF-16 space, never a TextKit display offset.
        self.insert_asset_markdown(
            CapturedImage::insertion(
                file_name,
                &CapturedImage::alt_text_for_file_named(file_name),
                &self.markdown_document().text(),
                self.container_text_view().source_selected_range().location,
            ),
            "Insert Photo",
        );
    }
}
