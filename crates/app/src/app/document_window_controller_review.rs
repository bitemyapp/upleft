//! Port of `App/DocumentWindowController+Review.swift`. Not ported yet.

/// The extension's associated-object state (`reviewPanel`, `reviewStore`), held by the
/// controller as `review_state()`.
// PORT: DocumentWindowController+Review.swift fills this in.
#[derive(Default)]
pub struct ReviewState {}

impl crate::app::document_window_controller::DocumentWindowController {
    /// `refreshReviewPanelIfVisible()`.
    pub fn refresh_review_panel_if_visible(&self) {
        // PORT: DocumentWindowController+Review.swift
    }

    /// `ensureReviewPanelVisible()`.
    pub fn ensure_review_panel_visible(&self) {
        // PORT: DocumentWindowController+Review.swift
    }
}
