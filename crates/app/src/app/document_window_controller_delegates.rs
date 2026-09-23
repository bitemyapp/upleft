//! Port of `App/DocumentWindowController+Delegates.swift`. Not ported yet.
//!
//! Every delegate conformance Swift declares here is a trait implemented on
//! `DocumentWindowControllerDelegates`, the proxy the controller hands its
//! views (see the contract at the top of `document_window_controller.rs`).
//! Until this file is ported, the conformances are empty placeholders so the
//! controller can install itself as each view's delegate.

use objc2_core_foundation::CGFloat;
use upleft_core::TextEdit;
use upleft_render::view::density_gutter_view::{DensityGutterDelegate, DensityGutterView};
use upleft_render::view::markdown_text_view_delegate::MarkdownTextViewDelegate;

use crate::app::document_window_controller::DocumentWindowControllerDelegates;
use crate::panels::breadcrumb_view::{BreadcrumbDelegate, BreadcrumbView};
use crate::panels::change_summary_bar_view::ChangeSummaryBarDelegate;
use crate::panels::conflict_bar_view::{ConflictBarDelegate, ConflictBarView};
use crate::panels::find_bar_view::FindBarDelegate;
use crate::panels::search_results_panel_view::SearchResultsDelegate;
use crate::panels::task_panel_view::TaskPanelDelegate;
use crate::panels::tidy_sheet_view::{TidySheetDelegate, TidySheetView};

// PORT: DocumentWindowController+Delegates.swift (`MarkdownTextViewDelegate`).
impl MarkdownTextViewDelegate for DocumentWindowControllerDelegates {}

// PORT: DocumentWindowController+Delegates.swift (`DensityGutterDelegate`).
impl DensityGutterDelegate for DocumentWindowControllerDelegates {
    fn density_gutter_did_request_scroll_to_fraction(&self, _gutter: &DensityGutterView, _fraction: CGFloat) {}

    fn density_gutter_preview_at_fraction(
        &self,
        _gutter: &DensityGutterView,
        _fraction: CGFloat,
    ) -> Option<(String, String, String)> {
        None
    }
}

// PORT: DocumentWindowController+Delegates.swift (`ConflictBarDelegate`).
impl ConflictBarDelegate for DocumentWindowControllerDelegates {
    fn conflict_bar_did_request_review(&self, _bar: &ConflictBarView) {}
    fn conflict_bar_did_request_keep_mine(&self, _bar: &ConflictBarView) {}
    fn conflict_bar_did_request_take_theirs(&self, _bar: &ConflictBarView) {}
    fn conflict_bar_did_request_dismiss(&self, _bar: &ConflictBarView) {}
}

// PORT: DocumentWindowController+Delegates.swift (the panels' delegates;
// their traits arrive with the panels on `port/panels`).
impl BreadcrumbDelegate for DocumentWindowControllerDelegates {
    fn breadcrumb_did_select_heading_at(&self, _view: &BreadcrumbView, _index: isize) {}
}
impl TaskPanelDelegate for DocumentWindowControllerDelegates {}
impl FindBarDelegate for DocumentWindowControllerDelegates {}
impl ChangeSummaryBarDelegate for DocumentWindowControllerDelegates {}
impl TidySheetDelegate for DocumentWindowControllerDelegates {
    fn tidy_sheet_did_apply(&self, _sheet: &TidySheetView, _edits: &[TextEdit]) {}
    fn tidy_sheet_did_cancel(&self, _sheet: &TidySheetView) {}
}
impl SearchResultsDelegate for DocumentWindowControllerDelegates {}
