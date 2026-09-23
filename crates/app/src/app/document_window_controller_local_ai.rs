//! Port of `App/DocumentWindowController+LocalAI.swift`: the on-device AI
//! panel's host.
//!
//! `LocalAIPanelViewDelegate` is implemented on the controller's delegate
//! proxy and forwards to the methods below. The model call itself runs off
//! the main thread in `LocalAILatestWinsController`, as in Swift.

use std::rc::{Rc, Weak};
use std::sync::Arc;

use objc2::MainThreadMarker;
use objc2::rc::{Retained, Weak as ObjcWeak};

use crate::ai::local_ai::{
    AppleOnDeviceAIProvider, LocalAIEditValidator, LocalAIError, LocalAILatestWinsController, LocalAIPreview,
    LocalAIProvider, LocalAIRequest, LocalAIResult, LocalAIRunError, LocalAITask,
};
use crate::app::document_window_controller::{DocumentWindowController, DocumentWindowControllerDelegates};
use crate::panels::local_ai_panel_view::{LocalAIPanelView, LocalAIPanelViewDelegate};

/// The extension's associated-object state (`localAIPanel`,
/// `localAIProvider`, `localAICoordinator`), held by the controller as
/// `local_ai_state()`. The provider and the coordinator are created on
/// first use, as Swift's getters do.
#[derive(Default)]
pub struct LocalAIState {
    /// `localAIPanel`.
    pub(crate) local_ai_panel: Option<Retained<LocalAIPanelView>>,
    /// `localAIProvider`.
    pub(crate) local_ai_provider: Option<Arc<dyn LocalAIProvider>>,
    /// `localAICoordinator`.
    pub(crate) local_ai_coordinator: Option<Rc<LocalAILatestWinsController>>,
}

impl DocumentWindowController {
    fn local_ai_panel(&self) -> Option<Retained<LocalAIPanelView>> {
        self.local_ai_state().borrow().local_ai_panel.clone()
    }

    fn set_local_ai_panel(&self, panel: Option<Retained<LocalAIPanelView>>) {
        self.local_ai_state().borrow_mut().local_ai_panel = panel;
    }

    /// `localAIProvider`: `AppleOnDeviceAIProvider()`, made on first use.
    fn local_ai_provider(&self) -> Arc<dyn LocalAIProvider> {
        if let Some(provider) = self.local_ai_state().borrow().local_ai_provider.clone() {
            return provider;
        }
        let provider: Arc<dyn LocalAIProvider> = Arc::new(AppleOnDeviceAIProvider::new());
        self.local_ai_state().borrow_mut().local_ai_provider = Some(provider.clone());
        provider
    }

    /// `localAICoordinator`, made on first use.
    fn local_ai_coordinator(&self) -> Rc<LocalAILatestWinsController> {
        if let Some(coordinator) = self.local_ai_state().borrow().local_ai_coordinator.clone() {
            return coordinator;
        }
        let coordinator =
            Rc::new(LocalAILatestWinsController::new(self.local_ai_provider(), MainThreadMarker::from(self)));
        self.local_ai_state().borrow_mut().local_ai_coordinator = Some(coordinator.clone());
        coordinator
    }

    /// `showLocalAIPanel()`: a toggle.
    pub fn show_local_ai_panel(&self) {
        if let Some(local_ai_panel) = self.local_ai_panel() {
            self.dismiss_trailing(&local_ai_panel);
            self.set_local_ai_panel(None);
            self.local_ai_coordinator().cancel();
            return;
        }
        let panel = LocalAIPanelView::new(self.active_style_sheet(), MainThreadMarker::from(self));
        let delegates = self.delegates();
        panel.set_delegate(Some(Rc::downgrade(&delegates) as Weak<dyn LocalAIPanelViewDelegate>));
        panel.set_availability(self.local_ai_provider().availability());
        self.set_local_ai_panel(Some(panel.clone()));
        self.install_trailing(&panel, None);
    }

    /// `localAIPanel(_:didRequest:)`.
    pub fn local_ai_panel_did_request(&self, panel: &LocalAIPanelView, task: LocalAITask) {
        let selection = self.container_text_view().source_selected_range();
        let range = if selection.length > 0 { Some(selection) } else { None };
        let request = LocalAIRequest::new(task, self.markdown_document().text(), range);
        panel.set_is_running(true);
        panel.set_result(None);
        let weak_self: ObjcWeak<DocumentWindowController> = ObjcWeak::from(self);
        let weak_panel: ObjcWeak<LocalAIPanelView> = ObjcWeak::from(panel);
        self.local_ai_coordinator().submit(request, move |result| {
            let (Some(this), Some(panel)) = (weak_self.load(), weak_panel.load()) else { return };
            if !this.local_ai_panel().is_some_and(|current| Retained::as_ptr(&current) == Retained::as_ptr(&panel)) {
                return;
            }
            panel.set_is_running(false);
            match result {
                Ok(value) => panel.set_result(Some(value)),
                Err(error) => {
                    panel.set_result(Some(LocalAIResult { task, text: Self::local_ai_message(&error), preview: None }))
                }
            }
        });
    }

    /// `localAIPanel(_:didApply:)`.
    pub fn local_ai_panel_did_apply(&self, panel: &LocalAIPanelView, preview: &LocalAIPreview) {
        let Some(edit) = LocalAIEditValidator::edit(preview, &self.markdown_document().text()) else {
            panel.set_result(Some(LocalAIResult {
                task: LocalAITask::ImproveClarity,
                text: "The source changed. Run the task again.".to_owned(),
                preview: None,
            }));
            return;
        };
        let summary = edit.summary.clone();
        self.markdown_document().apply(&[edit], &summary, None);
        self.markdown_document().reparse_now(false);
        panel.set_result(None);
    }

    /// `localAIPanelDidCancel(_:)`.
    pub fn local_ai_panel_did_cancel(&self, panel: &LocalAIPanelView) {
        self.local_ai_coordinator().cancel();
        self.dismiss_trailing(panel);
        if self.local_ai_panel().is_some_and(|current| std::ptr::eq(&*current, panel)) {
            self.set_local_ai_panel(None);
        }
    }

    /// `message(for:)`.
    fn local_ai_message(error: &LocalAIRunError) -> String {
        if let LocalAIRunError::LocalAI(error) = error {
            return match error {
                LocalAIError::EmptyInput => "Select or open text first.",
                LocalAIError::Cancelled => "Cancelled.",
                LocalAIError::Unavailable(_) => "On-device AI is not available on this Mac.",
            }
            .to_owned();
        }
        "Local AI could not complete this task.".to_owned()
    }
}

// MARK: - LocalAIPanelViewDelegate

impl LocalAIPanelViewDelegate for DocumentWindowControllerDelegates {
    fn local_ai_panel_did_request(&self, panel: &LocalAIPanelView, task: LocalAITask) {
        if let Some(controller) = self.controller() {
            controller.local_ai_panel_did_request(panel, task);
        }
    }

    fn local_ai_panel_did_apply(&self, panel: &LocalAIPanelView, preview: &LocalAIPreview) {
        if let Some(controller) = self.controller() {
            controller.local_ai_panel_did_apply(panel, preview);
        }
    }

    fn local_ai_panel_did_cancel(&self, panel: &LocalAIPanelView) {
        if let Some(controller) = self.controller() {
            controller.local_ai_panel_did_cancel(panel);
        }
    }
}
