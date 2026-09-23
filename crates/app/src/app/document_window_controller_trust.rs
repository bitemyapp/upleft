//! Port of `App/DocumentWindowController+Trust.swift`: trust hooks for
//! external effects. Existing link, asset, and path handlers call
//! `authorize_trust`; this extension owns the decision UI only.

use std::rc::{Rc, Weak};

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::AnyObject;
use objc2_app_kit::NSAppearanceCustomization;
use objc2_foundation::{NSNumber, NSURL, NSURLIsDirectoryKey};
use upleft_foundation::url::FileUrl;
use upleft_render::fragments::fragment_base::LocalAssetAuthorizer;
use upleft_render::render_contracts::RenderMode;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::{ThemeObservation, ThemeStore};
use upleft_render::view::markdown_text_view::MarkdownTextView;

use crate::app::document_window_controller::{DocumentWindowController, DocumentWindowControllerDelegates};
use crate::app::document_window_controller_support::cocoa_error_description;
use crate::panels::trust_prompt_view::{TrustPromptDecision, TrustPromptView, TrustPromptViewDelegate};
use crate::security::document_trust::{
    DocumentTrust, DocumentTrustState, TrustDecision, TrustEffect, TrustRequest, TrustScope, TrustTarget,
};
use crate::security::trust_store::TrustStore;

/// `CocoaError.Code.fileWriteUnknown`.
const FILE_WRITE_UNKNOWN_ERROR: isize = 512;

/// The extension's associated objects: `trustStore` (cached on first use),
/// `trustPrompt`, `pendingTrustAction` and the prompt's theme observation.
#[derive(Default)]
pub struct TrustState {
    pub(crate) trust_store: Option<&'static TrustStore>,
    pub(crate) trust_prompt: Option<Retained<TrustPromptView>>,
    pub(crate) pending_trust_action: Option<Rc<dyn Fn()>>,
    pub(crate) trust_theme_observation: Option<ThemeObservation>,
}

impl DocumentWindowController {
    /// `configureLocalAssetAccess(for:documentURL:)`.
    pub fn configure_local_asset_access(&self, text_view: &MarkdownTextView, document_url: Option<&FileUrl>) {
        text_view.set_document_url(document_url.map(FileUrl::path));
        let weak = ObjcWeak::new(self);
        let authorizer: LocalAssetAuthorizer = Rc::new(move |path: &str| {
            weak.load().map(|this| this.allows_local_asset(&FileUrl::from_path(path))).unwrap_or(false)
        });
        text_view.set_local_asset_authorizer(Some(authorizer));
    }

    /// Render-time trust check. `.ask` is intentionally treated as blocked;
    /// prompts are only legal from an explicit user action, never from draw.
    fn allows_local_asset(&self, url: &FileUrl) -> bool {
        let Some(canonical) = DocumentTrust::canonical_file_path(url) else { return false };
        let path = canonical.path();
        let request = TrustRequest::new(
            TrustEffect::ReadLocalAsset,
            TrustTarget::new(&path, Some(&path), None),
            self.markdown_document().url().as_ref(),
        );
        self.trust_decision(&request) == TrustDecision::Allow
    }

    fn trust_store(&self) -> &'static TrustStore {
        if let Some(store) = self.trust_state().borrow().trust_store {
            return store;
        }
        let store = TrustStore::shared();
        self.trust_state().borrow_mut().trust_store = Some(store);
        store
    }

    fn trust_prompt(&self) -> Option<Retained<TrustPromptView>> {
        self.trust_state().borrow().trust_prompt.clone()
    }

    fn set_trust_prompt(&self, prompt: Option<Retained<TrustPromptView>>) {
        self.trust_state().borrow_mut().trust_prompt = prompt;
    }

    /// `trustDecision(for:)`.
    pub fn trust_decision(&self, request: &TrustRequest) -> TrustDecision {
        let state = if self.container_text_view().mode() == RenderMode::Source {
            DocumentTrustState::RawSource
        } else {
            self.trust_store().state(self.markdown_document().url().as_ref())
        };
        self.trust_store().policy(state).decision(request)
    }

    /// `authorizeExternalURL(_:action:)`.
    pub fn authorize_external_url(&self, url: &NSURL, action: Rc<dyn Fn()>) -> TrustDecision {
        let absolute = absolute_string(url);
        self.authorize_trust(
            TrustRequest::new(
                TrustEffect::OpenExternalLink,
                TrustTarget::new(&absolute, None, Some(&absolute)),
                self.markdown_document().url().as_ref(),
            ),
            action,
        )
    }

    /// `authorizeRemoteAssetURL(_:action:)`.
    pub fn authorize_remote_asset_url(&self, url: &NSURL, action: Rc<dyn Fn()>) -> TrustDecision {
        let absolute = absolute_string(url);
        self.authorize_trust(
            TrustRequest::new(
                TrustEffect::LoadRemoteAsset,
                TrustTarget::new(&absolute, None, Some(&absolute)),
                self.markdown_document().url().as_ref(),
            ),
            action,
        )
    }

    /// `authorizeAutomationURL(_:action:)`.
    pub fn authorize_automation_url(&self, url: &NSURL, action: Rc<dyn Fn()>) -> TrustDecision {
        let absolute = absolute_string(url);
        self.authorize_trust(
            TrustRequest::new(
                TrustEffect::AutomationAppIntent,
                TrustTarget::new(&absolute, None, Some(&absolute)),
                self.markdown_document().url().as_ref(),
            ),
            action,
        )
    }

    /// `authorizeLocalEffect(_:target:action:)`.
    pub fn authorize_local_effect(&self, effect: TrustEffect, target: &FileUrl, action: Rc<dyn Fn()>) -> TrustDecision {
        let canonical = DocumentTrust::canonical_file_path(target).unwrap_or_else(|| target.standardized_file_url());
        let path = canonical.path();
        self.authorize_trust(
            TrustRequest::new(effect, TrustTarget::new(&path, Some(&path), None), self.markdown_document().url().as_ref()),
            action,
        )
    }

    /// Runs `action` only after policy allows it. An ask is non-modal and
    /// returns `.ask`; the action runs later if the user grants it.
    pub fn authorize_trust(&self, request: TrustRequest, action: Rc<dyn Fn()>) -> TrustDecision {
        match self.trust_decision(&request) {
            TrustDecision::Allow => {
                action();
                TrustDecision::Allow
            }
            TrustDecision::Deny => TrustDecision::Deny,
            TrustDecision::Ask => {
                self.trust_state().borrow_mut().pending_trust_action = Some(action);
                self.present_trust_prompt(request);
                TrustDecision::Ask
            }
        }
    }

    /// `presentTrustPrompt(_:)`.
    pub fn present_trust_prompt(&self, request: TrustRequest) {
        if let Some(prompt) = self.trust_prompt() {
            prompt.set_request(Some(request));
            return;
        }
        let prompt = TrustPromptView::new(self.active_style_sheet(), self.mtm());
        let delegate: Weak<dyn TrustPromptViewDelegate> = Rc::downgrade(&self.delegates()) as _;
        prompt.set_delegate(Some(delegate));
        prompt.set_request(Some(request));
        self.set_trust_prompt(Some(prompt.clone()));
        self.install_trailing(&prompt, None);

        // `[weak self, weak prompt]`: the observation lives exactly as long
        // as this prompt is the one installed (`finishTrustPrompt` drops it),
        // so the installed prompt is the one it was made for.
        let handle = self.handle();
        let observation = ThemeStore::shared().observe(move |theme| {
            let Some(this) = handle.load() else { return };
            let Some(prompt) = this.trust_prompt() else { return };
            let Some(window) = this.window() else { return };
            prompt.set_style_sheet(Rc::new(StyleSheet::new(theme.clone(), &window.effectiveAppearance(), None)));
        });
        self.trust_state().borrow_mut().trust_theme_observation = Some(observation);
    }

    /// `revokeTrust(scope:path:)`.
    pub fn revoke_trust(&self, scope: TrustScope, path: &FileUrl) {
        if !self.trust_store().revoke(scope, path) {
            self.present_operation_error(
                "Couldn\u{2019}t save the trust revocation",
                &cocoa_error_description(FILE_WRITE_UNKNOWN_ERROR),
            );
        }
    }

    /// `trustPrompt(_:didChoose:request:)`.
    pub fn trust_prompt_did_choose(&self, _view: &TrustPromptView, decision: TrustPromptDecision, request: &TrustRequest) {
        match decision {
            TrustPromptDecision::AllowOnce => self.finish_trust_prompt(true),
            TrustPromptDecision::AllowForFile => {
                self.grant(request, TrustScope::File);
                self.finish_trust_prompt(true);
            }
            TrustPromptDecision::AllowForFolder => {
                self.grant(request, TrustScope::Folder);
                self.finish_trust_prompt(true);
            }
            TrustPromptDecision::Deny => self.finish_trust_prompt(false),
            TrustPromptDecision::Revoke => {
                self.revoke_matching(request);
                self.finish_trust_prompt(false);
            }
        }
    }

    fn finish_trust_prompt(&self, run_pending: bool) {
        if let Some(prompt) = self.trust_prompt() {
            self.dismiss_trailing(&prompt);
        }
        self.set_trust_prompt(None);
        let observation = self.trust_state().borrow_mut().trust_theme_observation.take();
        drop(observation);
        let action = self.trust_state().borrow_mut().pending_trust_action.take();
        if run_pending && let Some(action) = action {
            action();
        }
    }

    fn grant(&self, request: &TrustRequest, scope: TrustScope) {
        let path = if let Some(target_path) = &request.target.canonical_path {
            let target_url = FileUrl::from_path(target_path);
            Some(if scope == TrustScope::Folder { self.folder_scope_url(&target_url) } else { target_url })
        } else if let Some(document_url) = self.markdown_document().url() {
            Some(if scope == TrustScope::Folder { self.folder_scope_url(&document_url) } else { document_url })
        } else {
            None
        };
        let Some(path) = path else { return };
        if !self.trust_store().grant(scope, &path, [request.effect], request.target.external_url.as_deref()) {
            self.present_operation_error(
                "Couldn\u{2019}t save this trust decision",
                &cocoa_error_description(FILE_WRITE_UNKNOWN_ERROR),
            );
        }
    }

    fn revoke_matching(&self, request: &TrustRequest) {
        if let Some(path) = &request.target.canonical_path {
            let target_url = FileUrl::from_path(path);
            self.revoke_trust(TrustScope::File, &target_url);
            self.revoke_trust(TrustScope::Folder, &self.folder_scope_url(&target_url));
        } else if let Some(document_url) = self.markdown_document().url() {
            self.revoke_trust(TrustScope::File, &document_url);
            self.revoke_trust(TrustScope::Folder, &self.folder_scope_url(&document_url));
        }
    }

    /// The folder an "Allow for Folder" grant covers for a target. A file
    /// target grants its parent directory; a directory target grants the
    /// directory itself, so consenting on a folder never widens to its
    /// parent.
    fn folder_scope_url(&self, target: &FileUrl) -> FileUrl {
        let url = target.to_nsurl();
        let mut value: Option<Retained<AnyObject>> = None;
        // SAFETY: `NSURLIsDirectoryKey` answers an `NSNumber`.
        let found = unsafe { url.getResourceValue_forKey_error(&mut value, NSURLIsDirectoryKey) }.is_ok();
        let is_directory =
            found && value.and_then(|value| value.downcast::<NSNumber>().ok()).is_some_and(|number| number.boolValue());
        DocumentTrust::folder_scope(target, is_directory)
    }
}

impl TrustPromptViewDelegate for DocumentWindowControllerDelegates {
    fn trust_prompt_did_choose(&self, view: &TrustPromptView, decision: TrustPromptDecision, request: &TrustRequest) {
        if let Some(controller) = self.controller() {
            controller.trust_prompt_did_choose(view, decision, request);
        }
    }
}

/// `url.absoluteString`.
fn absolute_string(url: &NSURL) -> String {
    url.absoluteString().map(|string| string.to_string()).unwrap_or_default()
}
