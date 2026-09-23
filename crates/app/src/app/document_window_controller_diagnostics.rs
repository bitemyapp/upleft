//! Port of `App/DocumentWindowController+Diagnostics.swift`: window hooks for
//! the two source-first diagnostics surfaces. The panels never mutate the
//! document. This extension owns validation, one-step undo, and source
//! selection.
//!
//! `DocumentHealthViewDelegate` and `RenderTargetsViewDelegate` are
//! implemented on the controller's delegate proxy and forward to the methods
//! below.

use std::rc::{Rc, Weak};

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use upleft_core::NSRange;
use upleft_core::compatibility::compatibility_diagnostics::CompatibilityDiagnostic;
use upleft_core::compatibility::render_target::RenderTargetProfile;
use upleft_core::contracts::TextEdit;
use upleft_core::health::document_health::{DocumentHealth, DocumentHealthDiagnostic};
use upleft_render::render_contracts::RenderMode;
use upleft_render::view::markdown_text_view_delegate::ScrollPosition;

use crate::app::document_window_controller::{DocumentWindowController, DocumentWindowControllerDelegates};
use crate::app::document_window_controller_asset_doctor::make_first_responder;
use crate::panels::document_health_view::{DocumentHealthView, DocumentHealthViewDelegate};
use crate::panels::render_targets_view::{RenderTargetsView, RenderTargetsViewDelegate};

/// The extension's associated-object state (`healthPanel`,
/// `renderTargetsPanel`), held by the controller as `diagnostics_state()`.
#[derive(Default)]
pub struct DiagnosticsState {
    /// `healthPanel`.
    pub(crate) health_panel: Option<Retained<DocumentHealthView>>,
    /// `renderTargetsPanel`.
    pub(crate) render_targets_panel: Option<Retained<RenderTargetsView>>,
}

impl DocumentWindowController {
    fn health_panel(&self) -> Option<Retained<DocumentHealthView>> {
        self.diagnostics_state().borrow().health_panel.clone()
    }

    fn set_health_panel(&self, panel: Option<Retained<DocumentHealthView>>) {
        self.diagnostics_state().borrow_mut().health_panel = panel;
    }

    fn render_targets_panel(&self) -> Option<Retained<RenderTargetsView>> {
        self.diagnostics_state().borrow().render_targets_panel.clone()
    }

    fn set_render_targets_panel(&self, panel: Option<Retained<RenderTargetsView>>) {
        self.diagnostics_state().borrow_mut().render_targets_panel = panel;
    }

    /// `toggleDocumentHealthPanel()`.
    pub fn toggle_document_health_panel(&self) {
        if let Some(panel) = self.health_panel() {
            self.dismiss_trailing(&panel);
            self.set_health_panel(None);
            return;
        }
        let panel = DocumentHealthView::new(self.active_style_sheet(), MainThreadMarker::from(self));
        self.set_health_panel(Some(panel.clone()));
        self.configure_document_health(&panel);
        self.install_trailing(&panel, None);
    }

    /// `configureDocumentHealth(_:)`.
    pub fn configure_document_health(&self, panel: &DocumentHealthView) {
        let delegates = self.delegates();
        panel.set_delegate(Some(Rc::downgrade(&delegates) as Weak<dyn DocumentHealthViewDelegate>));
        panel.set_style_sheet(self.active_style_sheet());
        panel.set_source_text(&self.markdown_document().text());
        panel.set_diagnostics(DocumentHealth::analyze_document(&self.markdown_document().parsed()));
    }

    /// `toggleRenderTargetsPanel()`.
    pub fn toggle_render_targets_panel(&self) {
        if let Some(panel) = self.render_targets_panel() {
            self.dismiss_trailing(&panel);
            self.set_render_targets_panel(None);
            return;
        }
        let panel = RenderTargetsView::new(self.active_style_sheet(), MainThreadMarker::from(self));
        self.set_render_targets_panel(Some(panel.clone()));
        self.configure_render_targets(&panel);
        self.install_trailing(&panel, None);
    }

    /// `configureRenderTargets(_:)`.
    pub fn configure_render_targets(&self, panel: &RenderTargetsView) {
        let delegates = self.delegates();
        panel.set_delegate(Some(Rc::downgrade(&delegates) as Weak<dyn RenderTargetsViewDelegate>));
        panel.set_style_sheet(self.active_style_sheet());
        panel.set_source_text(&self.markdown_document().text());
        panel.set_document(self.markdown_document().parsed());
    }

    /// `documentHealthView(_:didSelect:)`.
    pub fn document_health_view_did_select(&self, _view: &DocumentHealthView, diagnostic: &DocumentHealthDiagnostic) {
        self.select_diagnostic_range(diagnostic.range);
    }

    /// `documentHealthView(_:didApply:)`.
    pub fn document_health_view_did_apply(&self, view: &DocumentHealthView, fixes: &[TextEdit]) {
        self.apply_diagnostic_edits(fixes, "Apply health fixes");
        self.configure_document_health(view);
    }

    /// `documentHealthViewWantsSourceMode(_:)`.
    pub fn document_health_view_wants_source_mode(&self, _view: &DocumentHealthView) {
        self.open_source_mode();
    }

    /// `renderTargetsView(_:didSelect:)` for a profile.
    pub fn render_targets_view_did_select_profile(&self, view: &RenderTargetsView, _profile: &RenderTargetProfile) {
        view.set_document(self.markdown_document().parsed());
    }

    /// `renderTargetsView(_:didSelect:)` for a diagnostic.
    pub fn render_targets_view_did_select_diagnostic(
        &self,
        _view: &RenderTargetsView,
        diagnostic: &CompatibilityDiagnostic,
    ) {
        self.select_diagnostic_range(diagnostic.range);
    }

    /// `renderTargetsView(_:didApply:)`.
    pub fn render_targets_view_did_apply(&self, view: &RenderTargetsView, fixes: &[TextEdit]) {
        self.apply_diagnostic_edits(fixes, "Apply render target fixes");
        self.configure_render_targets(view);
    }

    /// `renderTargetsViewWantsSourceMode(_:)`.
    pub fn render_targets_view_wants_source_mode(&self, _view: &RenderTargetsView) {
        self.open_source_mode();
    }

    /// `refreshDiagnosticsPanels()`: call after a parse when a diagnostics
    /// panel is visible.
    pub fn refresh_diagnostics_panels(&self) {
        if let Some(health_panel) = self.health_panel() {
            self.configure_document_health(&health_panel);
        }
        if let Some(render_targets_panel) = self.render_targets_panel() {
            self.configure_render_targets(&render_targets_panel);
        }
    }

    /// `selectDiagnosticRange(_:)`.
    fn select_diagnostic_range(&self, range: NSRange) {
        if !(range.location >= 0 && range.upper_bound() <= self.markdown_document().storage().length() as isize) {
            return;
        }
        self.container_text_view().set_source_selected_ranges(&[range]);
        self.container_text_view().scroll_to_offset(range.location, ScrollPosition::Visible, true);
        if let Some(window) = self.window() {
            make_first_responder(&window, &self.container_text_view());
        }
    }

    /// `openSourceMode()`.
    fn open_source_mode(&self) {
        self.apply_mode(RenderMode::Source);
        if let Some(window) = self.window() {
            make_first_responder(&window, &self.container_text_view());
        }
    }

    /// `applyDiagnosticEdits(_:actionName:)`.
    fn apply_diagnostic_edits(&self, edits: &[TextEdit], action_name: &str) {
        let length = self.markdown_document().storage().length() as isize;
        let mut safe: Vec<TextEdit> = edits
            .iter()
            .filter(|edit| edit.range.location >= 0 && edit.range.upper_bound() <= length)
            .cloned()
            .collect();
        // `sorted { $0.range.location > $1.range.location }`: Swift's sort is
        // stable, as `sort_by` is.
        safe.sort_by(|a, b| b.range.location.cmp(&a.range.location));
        if safe.is_empty() {
            return;
        }

        let mut last_start = isize::MAX;
        let non_overlapping: Vec<TextEdit> = safe
            .into_iter()
            .filter(|edit| {
                if !(edit.range.upper_bound() <= last_start) {
                    return false;
                }
                last_start = edit.range.location;
                true
            })
            .collect();
        if non_overlapping.is_empty() {
            return;
        }
        self.markdown_document().apply(&non_overlapping, action_name, None);
        self.markdown_document().reparse_now(false);
        self.refresh_derived_ui();
        self.refresh_diagnostics_panels();
    }
}

// MARK: - DocumentHealthViewDelegate, RenderTargetsViewDelegate

impl DocumentHealthViewDelegate for DocumentWindowControllerDelegates {
    fn document_health_view_did_select(&self, view: &DocumentHealthView, diagnostic: &DocumentHealthDiagnostic) {
        if let Some(controller) = self.controller() {
            controller.document_health_view_did_select(view, diagnostic);
        }
    }

    fn document_health_view_did_apply(&self, view: &DocumentHealthView, fixes: &[TextEdit]) {
        if let Some(controller) = self.controller() {
            controller.document_health_view_did_apply(view, fixes);
        }
    }

    fn document_health_view_wants_source_mode(&self, view: &DocumentHealthView) {
        if let Some(controller) = self.controller() {
            controller.document_health_view_wants_source_mode(view);
        }
    }
}

impl RenderTargetsViewDelegate for DocumentWindowControllerDelegates {
    fn render_targets_view_did_select_profile(&self, view: &RenderTargetsView, profile: &RenderTargetProfile) {
        if let Some(controller) = self.controller() {
            controller.render_targets_view_did_select_profile(view, profile);
        }
    }

    fn render_targets_view_did_select_diagnostic(
        &self,
        view: &RenderTargetsView,
        diagnostic: &CompatibilityDiagnostic,
    ) {
        if let Some(controller) = self.controller() {
            controller.render_targets_view_did_select_diagnostic(view, diagnostic);
        }
    }

    fn render_targets_view_did_apply(&self, view: &RenderTargetsView, fixes: &[TextEdit]) {
        if let Some(controller) = self.controller() {
            controller.render_targets_view_did_apply(view, fixes);
        }
    }

    fn render_targets_view_wants_source_mode(&self, view: &RenderTargetsView) {
        if let Some(controller) = self.controller() {
            controller.render_targets_view_wants_source_mode(view);
        }
    }
}
