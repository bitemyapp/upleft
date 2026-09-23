//! Port of `App/DocumentWindowController+Diagnostics.swift`. Not ported yet.

/// The extension's associated-object state (`healthPanel`, `renderTargetsPanel`), held by the
/// controller as `diagnostics_state()`.
// PORT: DocumentWindowController+Diagnostics.swift fills this in.
#[derive(Default)]
pub struct DiagnosticsState {}

impl crate::app::document_window_controller::DocumentWindowController {
    /// `refreshDiagnosticsPanels()`.
    pub fn refresh_diagnostics_panels(&self) {
        // PORT: DocumentWindowController+Diagnostics.swift
    }
}
