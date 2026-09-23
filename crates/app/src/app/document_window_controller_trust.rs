//! Port of `App/DocumentWindowController+Trust.swift`.

use std::rc::Rc;

use objc2::rc::Retained;
use upleft_render::theme::theme_store::ThemeObservation;

use crate::panels::trust_prompt_view::TrustPromptView;
use crate::security::trust_store::TrustStore;

/// The extension's associated objects: `trustStore` (cached on first use),
/// `trustPrompt`, `pendingTrustAction` and the prompt's theme observation.
#[derive(Default)]
pub struct TrustState {
    pub(crate) trust_store: Option<&'static TrustStore>,
    pub(crate) trust_prompt: Option<Retained<TrustPromptView>>,
    pub(crate) pending_trust_action: Option<Rc<dyn Fn()>>,
    pub(crate) trust_theme_observation: Option<ThemeObservation>,
}
