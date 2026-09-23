//! Port of `App/DocumentWindowController+Commands.swift`. Not ported yet.
//!
//! The Objective-C entry points of this extension (`validateMenuItem:`,
//! `performDownrightCommand:`) are declared in
//! `document_window_controller.rs`'s `define_class!` and forward to the
//! methods below.

use objc2::runtime::AnyObject;
use objc2_app_kit::NSMenuItem;

use crate::app::document_window_controller::DocumentWindowController;

impl DocumentWindowController {
    /// `validateMenuItem(_:)` (`NSMenuItemValidation`).
    pub fn validate_menu_item(&self, _menu_item: &NSMenuItem) -> bool {
        // PORT: DocumentWindowController+Commands.swift
        false
    }

    /// `@objc performDownrightCommand(_:)` (`CommandResponder`).
    pub fn perform_downright_command(&self, _sender: Option<&AnyObject>) {
        // PORT: DocumentWindowController+Commands.swift
    }
}
