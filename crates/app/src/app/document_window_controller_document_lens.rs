//! Port of `App/DocumentWindowController+DocumentLens.swift`. Document Lens
//! is kept in an extension so the window controller stays focused on the
//! document surface. The associated state gives the transient panel normal
//! lifetime without adding shared controller state.
//!
//! `DocumentLensViewDelegate` is implemented on the controller's delegate
//! proxy and forwards to the methods below.

use std::rc::{Rc, Weak};

use dispatch2::MainThreadBound;
use objc2::MainThreadMarker;
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2_app_kit::NSAppearanceCustomization;
use upleft_core::NSRange;
use upleft_core::compatibility::compatibility_diagnostics::MarkdownCompatibility;
use upleft_core::compatibility::render_target::RenderTargetProfile;
use upleft_core::health::document_health::DocumentHealth;
use upleft_render::render_contracts::Theme;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::{ThemeObservation, ThemeStore};
use upleft_render::view::markdown_text_view_delegate::ScrollPosition;

use crate::app::document_window_controller::{DocumentWindowController, DocumentWindowControllerDelegates};
use crate::app::document_window_controller_asset_doctor::make_first_responder;
use crate::assets::asset_doctor::AssetDoctor;
use crate::assets::asset_resolver::AssetResolutionContext;
use crate::lens::document_lens_model::{DocumentLensChange, DocumentLensInput, DocumentLensItem, DocumentLensModel};
use crate::panels::document_lens_view::{DocumentLensView, DocumentLensViewDelegate};
use crate::panels::panel_chrome::panel_title;
use crate::support::commands::Command;

/// The extension's associated-object state (`documentLensPanel` and its
/// theme observation), held by the controller as `document_lens_state()`.
#[derive(Default)]
pub struct DocumentLensState {
    /// `documentLensPanel`.
    pub(crate) document_lens_panel: Option<Retained<DocumentLensView>>,
    /// The `documentLensThemeKey` association; dropping it cancels the
    /// observation, as releasing Swift's token does.
    pub(crate) theme_observation: Option<ThemeObservation>,
}

impl DocumentWindowController {
    fn document_lens_panel(&self) -> Option<Retained<DocumentLensView>> {
        self.document_lens_state().borrow().document_lens_panel.clone()
    }

    fn set_document_lens_panel(&self, panel: Option<Retained<DocumentLensView>>) {
        self.document_lens_state().borrow_mut().document_lens_panel = panel;
    }

    /// Replaces the stored observation. The old one is dropped (cancelled)
    /// outside the borrow.
    fn set_document_lens_theme_observation(&self, observation: Option<ThemeObservation>) {
        let previous = std::mem::replace(&mut self.document_lens_state().borrow_mut().theme_observation, observation);
        drop(previous);
    }

    /// `toggleDocumentLensPanel()`.
    pub fn toggle_document_lens_panel(&self) {
        if let Some(panel) = self.document_lens_panel() {
            self.dismiss_trailing(&panel);
            self.set_document_lens_panel(None);
            self.set_document_lens_theme_observation(None);
            return;
        }

        let mtm = MainThreadMarker::from(self);
        let panel = DocumentLensView::new(self.active_style_sheet(), mtm);
        let delegates = self.delegates();
        panel.set_delegate(Some(Rc::downgrade(&delegates) as Weak<dyn DocumentLensViewDelegate>));
        self.set_document_lens_panel(Some(panel.clone()));
        self.configure_document_lens(&panel);
        self.install_trailing(&panel, Some(&panel_title(Command::DocumentLens)));

        // `[weak self, weak panel]`; the store calls observers on the main
        // queue.
        let captured = MainThreadBound::new((ObjcWeak::from(self), ObjcWeak::from(&*panel)), mtm);
        let observation = ThemeStore::shared().observe(move |theme: &Theme| {
            let Some(mtm) = MainThreadMarker::new() else { return };
            let (weak_self, weak_panel): &(ObjcWeak<DocumentWindowController>, ObjcWeak<DocumentLensView>) =
                captured.get(mtm);
            let (Some(this), Some(panel)) = (weak_self.load(), weak_panel.load()) else { return };
            let Some(window) = this.window() else { return };
            panel.set_style_sheet(Rc::new(StyleSheet::new(theme.clone(), &window.effectiveAppearance(), None)));
        });
        self.set_document_lens_theme_observation(Some(observation));
    }

    /// `configureDocumentLens(_:)`.
    pub fn configure_document_lens(&self, panel: &DocumentLensView) {
        let parsed = self.markdown_document().parsed();
        let health = DocumentHealth::analyze_document(&parsed);
        let context = AssetResolutionContext::new(
            self.markdown_document().url(),
            self.markdown_document().url().map(|url| url.deleting_last_path_component()),
        );
        let references = AssetDoctor::references(&parsed, &context);
        let assets = AssetDoctor::diagnose(&parsed, &context, Some(&self.local_asset_probe()));
        // GitHub is the most common interchange target. The panel remains
        // useful for local files because the target is injected and can later
        // be replaced by a user profile without changing the view.
        let report = MarkdownCompatibility::diagnose(&parsed, &panel.render_target_profile());
        let changes: Vec<DocumentLensChange> = self
            .markdown_document()
            .changes()
            .visible_marks()
            .iter()
            .enumerate()
            .map(|(index, mark)| {
                DocumentLensChange::new(
                    format!("{index}:{}", mark.range.location),
                    mark.kind,
                    mark.range,
                    mark.word_ranges.clone(),
                )
            })
            .collect();
        // The rows caption themselves by line number, which needs the source
        // before the model lands or they fall back to raw byte offsets.
        panel.set_source_text(&self.markdown_document().text());
        let mut input = DocumentLensInput::new(parsed);
        input.health = health;
        input.asset_references = references;
        input.assets = assets;
        input.render_target = Some(report);
        input.changes = changes;
        panel.set_model(DocumentLensModel::new(&input));
    }

    /// `documentLens(_:didSelectRenderTarget:)`.
    pub fn document_lens_did_select_render_target(&self, view: &DocumentLensView, profile: &RenderTargetProfile) {
        view.set_render_target_profile(profile.clone());
        self.configure_document_lens(view);
    }

    /// `documentLens(_:didSelect:item:)`.
    pub fn document_lens_did_select(&self, _view: &DocumentLensView, range: NSRange, _item: &DocumentLensItem) {
        if !(range.location >= 0 && range.upper_bound() <= self.markdown_document().storage().length() as isize) {
            return;
        }
        self.container_text_view().set_source_selected_ranges(&[range]);
        self.container_text_view().scroll_to_offset(range.location, ScrollPosition::Visible, true);
        if let Some(window) = self.window() {
            make_first_responder(&window, &self.container_text_view());
        }
    }

    /// `refreshDocumentLensIfVisible()`.
    pub fn refresh_document_lens_if_visible(&self) {
        let Some(document_lens_panel) = self.document_lens_panel() else { return };
        self.configure_document_lens(&document_lens_panel);
    }
}

// MARK: - DocumentLensViewDelegate

impl DocumentLensViewDelegate for DocumentWindowControllerDelegates {
    fn document_lens_did_select(&self, view: &DocumentLensView, range: NSRange, item: &DocumentLensItem) {
        if let Some(controller) = self.controller() {
            controller.document_lens_did_select(view, range, item);
        }
    }

    fn document_lens_did_select_render_target(&self, view: &DocumentLensView, profile: &RenderTargetProfile) {
        if let Some(controller) = self.controller() {
            controller.document_lens_did_select_render_target(view, profile);
        }
    }
}
