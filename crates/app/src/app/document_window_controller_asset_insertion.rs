//! Port of `App/DocumentWindowController+AssetInsertion.swift`. Not ported yet.

use crate::app::document_window_controller::DocumentWindowController;
use crate::assets::captured_image::InsertionEdit;

impl DocumentWindowController {
    /// `insertAssetMarkdown(_:actionName:)`: Swift's
    /// `(replacement:origin:caret:)` tuple is an [`InsertionEdit`].
    pub fn insert_asset_markdown(&self, _insertion: InsertionEdit, _action_name: &str) {
        // PORT: DocumentWindowController+AssetInsertion.swift
    }
}
