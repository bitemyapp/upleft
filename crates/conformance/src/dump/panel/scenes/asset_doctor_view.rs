//! `AssetDoctorView` scenes (`Scenes/AssetDoctorViewScene.swift`).

use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use serde_json::{Map, Value};
use upleft_app::assets::asset_doctor::{AssetDiagnostic, AssetDoctor};
use upleft_app::assets::asset_resolver::AssetResolutionContext;
use upleft_app::panels::asset_doctor_view::AssetDoctorView;
use upleft_core::parser::MarkdownParser;
use upleft_render::theme::style_sheet::StyleSheet;

use super::document_health_view::{document_url, probe, table_rows};
use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene};

#[derive(Default)]
pub struct AssetDoctorViewScene {
    view: Option<Retained<AssetDoctorView>>,
}

/// `AssetDoctorViewScene.diagnostics(_:text:)`.
pub fn diagnostics(scenario: &PanelScenario, text: &str) -> Vec<AssetDiagnostic> {
    let document_url = document_url(scenario);
    let workspace_root = document_url.as_ref().map(|url| url.deleting_last_path_component());
    let context = match scenario.int("maximumBytes") {
        Some(maximum_bytes) => AssetResolutionContext::with_maximum_bytes(document_url, workspace_root, maximum_bytes),
        None => AssetResolutionContext::new(document_url, workspace_root),
    };
    AssetDoctor::diagnose(&MarkdownParser::parse(text), &context, Some(&probe()))
}

impl PanelScene for AssetDoctorViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let text = scenario.document_text().map_err(Failure::Error)?;
        let view = AssetDoctorView::new(style_sheet.clone(), mtm);
        if !scenario.bool("empty") {
            view.set_diagnostics(diagnostics(scenario, &text));
        }
        if scenario.bool("restyle") {
            view.set_style_sheet(style_sheet);
        }
        self.view = Some(view.clone());
        Ok(Retained::into_super(view))
    }

    fn model(&self) -> Value {
        let Some(view) = &self.view else { return Value::Null };
        let diagnostics = view.diagnostics();
        let mut map = Map::new();
        map.insert("preferredWidth".into(), double(view.preferred_width()));
        map.insert(
            "findings".into(),
            Value::Array(diagnostics.iter().map(|diagnostic| Value::String(diagnostic.id.clone())).collect()),
        );
        map.insert(
            "lines".into(),
            Value::Array(diagnostics.iter().map(|diagnostic| Value::from(diagnostic.reference.line as i64)).collect()),
        );
        map.insert("rows".into(), table_rows(view));
        Value::Object(map)
    }
}
