//! `RenderTargetsView` scenes (`Scenes/RenderTargetsViewScene.swift`).

use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use serde_json::{Map, Value};
use upleft_app::panels::render_targets_view::RenderTargetsView;
use upleft_core::compatibility::render_target::{
    BuiltInRenderTarget, MarkdownCapabilities, MarkdownCapability, RenderTargetProfile,
};
use upleft_core::parser::MarkdownParser;
use upleft_render::theme::style_sheet::StyleSheet;

use super::document_health_view::table_rows;
use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene};

#[derive(Default)]
pub struct RenderTargetsViewScene {
    view: Option<Retained<RenderTargetsView>>,
}

/// `RenderTargetsViewScene.profile(_:)`.
pub fn profile(value: &Value) -> Option<RenderTargetProfile> {
    if let Some(id) = value.as_str() {
        return BuiltInRenderTarget::from_raw_value(id).map(|built_in| built_in.profile());
    }
    let object = value.as_object()?;
    let name = object.get("name")?.as_str()?;
    let mut capabilities = MarkdownCapabilities::EMPTY;
    for raw in object.get("capabilities").and_then(Value::as_array).into_iter().flatten() {
        let Some(capability) = raw.as_str().and_then(MarkdownCapability::from_raw_value) else { continue };
        capabilities.insert(MarkdownCapabilities::from_capability(capability));
    }
    Some(RenderTargetProfile::custom(name, capabilities))
}

impl PanelScene for RenderTargetsViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let text = scenario.document_text().map_err(Failure::Error)?;
        let view = RenderTargetsView::new(style_sheet.clone(), mtm);
        let profiles: Vec<RenderTargetProfile> = scenario.array("profiles").iter().filter_map(profile).collect();
        if !profiles.is_empty() {
            view.set_profiles(profiles);
        }
        view.set_source_text(&text);
        view.set_document(MarkdownParser::parse(&text));
        if let Some(index) = scenario.int("profile") {
            let profiles = view.profiles();
            if index >= 0 && (index as usize) < profiles.len() {
                view.set_selected_profile(profiles[index as usize].clone());
            }
        }
        if let Some(select) = scenario.int("select") {
            view.select_finding_for_testing(select as isize);
        }
        if scenario.bool("apply") {
            view.apply_safe_fixes_for_testing();
        }
        if scenario.bool("restyle") {
            view.set_style_sheet(style_sheet);
        }
        self.view = Some(view.clone());
        Ok(Retained::into_super(view))
    }

    fn model(&self) -> Value {
        let Some(view) = &self.view else { return Value::Null };
        let report = view.report();
        let mut map = Map::new();
        map.insert("preferredWidth".into(), double(view.preferred_width()));
        map.insert(
            "profiles".into(),
            Value::Array(view.profiles().into_iter().map(|profile| Value::String(profile.id)).collect()),
        );
        map.insert("selectedProfile".into(), Value::String(view.selected_profile().id));
        map.insert("reportProfile".into(), Value::String(report.profile.id.clone()));
        map.insert(
            "findings".into(),
            Value::Array(report.diagnostics.iter().map(|diagnostic| Value::String(diagnostic.id.clone())).collect()),
        );
        map.insert("rows".into(), table_rows(view));
        Value::Object(map)
    }
}
