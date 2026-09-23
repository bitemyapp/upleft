//! `TrustPromptView` scenes (`Scenes/TrustPromptViewScene.swift`).

use std::cell::RefCell;
use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use serde_json::{Map, Value};
use upleft_app::panels::panel_chrome::PanelSurface;
use upleft_app::panels::trust_prompt_view::{TrustPromptDecision, TrustPromptView, TrustPromptViewDelegate};
use upleft_app::security::document_trust::{TrustEffect, TrustRequest, TrustTarget};
use upleft_foundation::url::FileUrl;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
struct Delegate {
    decisions: RefCell<Vec<String>>,
}

/// Swift's `"\(decision)"`: the case name.
fn case_name(decision: TrustPromptDecision) -> &'static str {
    match decision {
        TrustPromptDecision::AllowOnce => "allowOnce",
        TrustPromptDecision::AllowForFile => "allowForFile",
        TrustPromptDecision::AllowForFolder => "allowForFolder",
        TrustPromptDecision::Deny => "deny",
        TrustPromptDecision::Revoke => "revoke",
    }
}

impl TrustPromptViewDelegate for Delegate {
    fn trust_prompt_did_choose(&self, _view: &TrustPromptView, decision: TrustPromptDecision, request: &TrustRequest) {
        self.decisions.borrow_mut().push(format!("{} {}", case_name(decision), request.target.display_name));
    }
}

fn decision(name: &str) -> Option<TrustPromptDecision> {
    match name {
        "allowOnce" => Some(TrustPromptDecision::AllowOnce),
        "allowForFile" => Some(TrustPromptDecision::AllowForFile),
        "allowForFolder" => Some(TrustPromptDecision::AllowForFolder),
        "deny" => Some(TrustPromptDecision::Deny),
        "revoke" => Some(TrustPromptDecision::Revoke),
        _ => None,
    }
}

#[derive(Default)]
pub struct TrustPromptViewScene {
    prompt: Option<Retained<TrustPromptView>>,
    request: Option<TrustRequest>,
    delegate: Option<Rc<Delegate>>,
}

impl PanelScene for TrustPromptViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let prompt = if scenario.bool("current") {
            TrustPromptView::new_current(mtm)
        } else {
            TrustPromptView::new(style_sheet.clone(), mtm)
        };
        if scenario.bool("current") {
            prompt.set_style_sheet(style_sheet);
        }
        let delegate = Rc::new(Delegate::default());
        let weak: std::rc::Weak<dyn TrustPromptViewDelegate> = Rc::downgrade(&(delegate.clone() as Rc<dyn TrustPromptViewDelegate>));
        prompt.set_delegate(Some(weak));
        self.delegate = Some(delegate);
        if !scenario.bool("noRequest") {
            let effect = TrustEffect::from_raw_value(&scenario.string_or("effect", "openExternalLink"))
                .unwrap_or(TrustEffect::OpenExternalLink);
            let document_url = scenario.string("documentPath").map(|path| FileUrl::from_path(&path));
            let request = TrustRequest::new(
                effect,
                TrustTarget::new(
                    &scenario.string_or("displayName", ""),
                    scenario.string("canonicalPath").as_deref(),
                    scenario.string("externalURL").as_deref(),
                ),
                document_url.as_ref(),
            );
            prompt.set_request(Some(request.clone()));
            self.request = Some(request);
        }
        for name in scenario.strings("choose") {
            let Some(decision) = decision(&name) else { continue };
            prompt.choose_for_testing(decision);
        }
        self.prompt = Some(prompt.clone());
        Ok(Retained::into_super(prompt))
    }

    fn model(&self) -> Value {
        let Some(prompt) = &self.prompt else { return Value::Null };
        let mut map = Map::new();
        map.insert("preferredWidth".into(), double(prompt.preferred_width()));
        map.insert(
            "fileGrantName".into(),
            self.request.as_ref().and_then(TrustPromptView::file_grant_name).map_or(Value::Null, Value::String),
        );
        map.insert("hasRequest".into(), Value::Bool(prompt.request().is_some()));
        map.insert("acceptsFirstResponder".into(), Value::Bool(prompt.acceptsFirstResponder()));
        let decisions = self.delegate.as_ref().map(|delegate| delegate.decisions.borrow().clone()).unwrap_or_default();
        map.insert("decisions".into(), Value::Array(decisions.into_iter().map(Value::String).collect()));
        map.insert("fittingSize".into(), tree::size(prompt.fittingSize()));
        Value::Object(map)
    }
}
