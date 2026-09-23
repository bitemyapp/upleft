//! Port of `App/DocumentWindowController+Actions.swift`. Not ported yet.
//!
//! The Objective-C entry points of this extension are declared in
//! `document_window_controller.rs`'s `define_class!` and forward to the
//! methods below.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSMenu, NSToolbar, NSToolbarItem};
use objc2_foundation::{NSArray, NSString};

use crate::app::document_window_controller::DocumentWindowController;

impl DocumentWindowController {
    /// `updateBreadcrumbAndGutter()`.
    pub fn update_breadcrumb_and_gutter(&self) {
        // PORT: DocumentWindowController+Actions.swift
    }

    /// `beginActivity()`.
    pub fn begin_activity(&self) {
        // PORT: DocumentWindowController+Actions.swift
    }

    /// `endActivity()`.
    pub fn end_activity(&self) {
        // PORT: DocumentWindowController+Actions.swift
    }

    /// `var presentationSegment`.
    pub fn presentation_segment(&self) -> isize {
        // PORT: DocumentWindowController+Actions.swift
        0
    }

    /// `var documentLineCount`.
    pub fn document_line_count(&self) -> isize {
        // PORT: DocumentWindowController+Actions.swift
        0
    }

    /// `setPresentationSegment(_:)`.
    pub fn set_presentation_segment(&self, _segment: isize) {
        // PORT: DocumentWindowController+Actions.swift
    }

    /// `changePresentation(to:)`.
    pub fn change_presentation(&self, _selected_segment: isize) {
        // PORT: DocumentWindowController+Actions.swift
    }

    /// `refreshSourceFocusToolbar()`.
    pub fn refresh_source_focus_toolbar(&self) {
        // PORT: DocumentWindowController+Actions.swift
    }

    /// `refreshToolbarSelectionState()`.
    pub fn refresh_toolbar_selection_state(&self) {
        // PORT: DocumentWindowController+Actions.swift
    }

    /// `static let modeItem = NSToolbarItem.Identifier("presentation-mode")`.
    pub const MODE_ITEM: &'static str = "presentation-mode";

    /// `toolbarDefaultItemIdentifiers(_:)`.
    pub fn toolbar_default_item_identifiers(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSString>> {
        // PORT: DocumentWindowController+Actions.swift
        NSArray::new()
    }

    /// `toolbarAllowedItemIdentifiers(_:)`.
    pub fn toolbar_allowed_item_identifiers(&self, _toolbar: &NSToolbar) -> Retained<NSArray<NSString>> {
        // PORT: DocumentWindowController+Actions.swift
        NSArray::new()
    }

    /// `toolbar(_:itemForItemIdentifier:willBeInsertedIntoToolbar:)`.
    pub fn toolbar_item_for_item_identifier(
        &self,
        _toolbar: &NSToolbar,
        _identifier: &NSString,
        _will_be_inserted: bool,
    ) -> Option<Retained<NSToolbarItem>> {
        // PORT: DocumentWindowController+Actions.swift
        None
    }

    /// `validateToolbarItem(_:)`.
    pub fn validate_toolbar_item(&self, _item: &NSToolbarItem) -> bool {
        // PORT: DocumentWindowController+Actions.swift
        false
    }

    /// `menuNeedsUpdate(_:)`.
    pub fn menu_needs_update(&self, _menu: &NSMenu) {
        // PORT: DocumentWindowController+Actions.swift
    }

    /// `@objc toolbarShowTasks(_:)`.
    pub fn toolbar_show_tasks(&self, _sender: Option<&AnyObject>) {
        // PORT: DocumentWindowController+Actions.swift
    }

    /// `@objc toolbarToggleSourceFocus(_:)`.
    pub fn toolbar_toggle_source_focus(&self, _sender: Option<&AnyObject>) {
        // PORT: DocumentWindowController+Actions.swift
    }

    /// `@objc toolbarShowHistory(_:)`.
    pub fn toolbar_show_history(&self, _sender: Option<&AnyObject>) {
        // PORT: DocumentWindowController+Actions.swift
    }

    /// `@objc toolbarShowFind(_:)`.
    pub fn toolbar_show_find(&self, _sender: Option<&AnyObject>) {
        // PORT: DocumentWindowController+Actions.swift
    }

    /// `@objc toolbarCheckForUpdates(_:)`.
    pub fn toolbar_check_for_updates(&self, _sender: Option<&AnyObject>) {
        // PORT: DocumentWindowController+Actions.swift
    }
}
