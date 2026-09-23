//! Port of `App/DocumentWindowController+Commands.swift`. Not ported yet.
//!
//! The Objective-C entry points of this extension (`validateMenuItem:`,
//! `performDownrightCommand:`) are declared in
//! `document_window_controller.rs`'s `define_class!` and forward to the
//! methods below.

use objc2::runtime::AnyObject;
use objc2_app_kit::NSMenuItem;
use objc2_core_foundation::CGFloat;
use upleft_render::view::markdown_text_view::MarkdownTextView;

use crate::app::document_window_controller::DocumentWindowController;
use crate::support::commands::Command;

impl DocumentWindowController {
    /// `wireKeyEventHandler(_:)`.
    pub fn wire_key_event_handler(&self, _text_view: &MarkdownTextView) {
        // PORT: DocumentWindowController+Commands.swift
    }

    /// `perform(_:) -> Bool`.
    pub fn perform(&self, _command: Command) -> bool {
        // PORT: DocumentWindowController+Commands.swift
        false
    }

    /// `adjustTextSize(by:)`.
    pub fn adjust_text_size(&self, _delta: CGFloat) {
        // PORT: DocumentWindowController+Commands.swift
    }

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
