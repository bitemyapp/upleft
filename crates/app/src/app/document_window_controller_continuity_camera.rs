//! Port of `App/DocumentWindowController+ContinuityCamera.swift`.
//!
//! The Objective-C entry points (`validRequestorForSendType:returnType:`,
//! `readSelectionFromPasteboard:`) are declared in
//! `document_window_controller.rs`'s `define_class!` and forward here.

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::NSPasteboard;
use objc2_foundation::NSString;

use crate::app::document_window_controller::DocumentWindowController;

impl DocumentWindowController {
    /// `validRequestor(forSendType:returnType:)`.
    pub fn valid_requestor(
        &self,
        send_type: Option<&NSString>,
        return_type: Option<&NSString>,
    ) -> Option<Retained<AnyObject>> {
        // PORT: body (phase 2); unhandled goes to super.
        unsafe { msg_send![super(self), validRequestorForSendType: send_type, returnType: return_type] }
    }

    /// `readSelection(from:)`.
    pub fn read_selection(&self, _pasteboard: &NSPasteboard) -> bool {
        // PORT: body (phase 2).
        false
    }
}
