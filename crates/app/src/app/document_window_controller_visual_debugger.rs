//! Port of `App/DocumentWindowController+VisualDebugger.swift`. Not ported yet.

/// The extension's associated-object state (`visualDebuggerPanel` and its theme observation), held by the
/// controller as `visual_debugger_state()`.
// PORT: DocumentWindowController+VisualDebugger.swift fills this in.
#[derive(Default)]
pub struct VisualDebuggerState {}

impl crate::app::document_window_controller::DocumentWindowController {
    /// `refreshVisualDebuggerIfVisible()`.
    pub fn refresh_visual_debugger_if_visible(&self) {
        // PORT: DocumentWindowController+VisualDebugger.swift
    }
}
