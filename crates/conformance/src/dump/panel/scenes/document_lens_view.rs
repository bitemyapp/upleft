//! `DocumentLensView` scenes (`Scenes/DocumentLensViewScene.swift`).

use std::cell::RefCell;
use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use serde_json::{Map, Value};
use upleft_app::assets::asset_doctor::AssetDoctor;
use upleft_app::assets::asset_resolver::AssetResolutionContext;
use upleft_app::lens::document_lens_model::{
    DocumentLensChange, DocumentLensInput, DocumentLensItem, DocumentLensModel, DocumentLensTab,
};
use upleft_app::panels::document_lens_view::{DocumentLensView, DocumentLensViewDelegate};
use upleft_core::compatibility::compatibility_diagnostics::MarkdownCompatibility;
use upleft_core::compatibility::render_target::{BuiltInRenderTarget, RenderTargetProfile};
use upleft_core::health::document_health::DocumentHealth;
use upleft_core::parser::MarkdownParser;
use upleft_core::{ChangeKind, NSRange};
use upleft_render::theme::style_sheet::StyleSheet;

use super::document_health_view::{document_url, probe, table_rows};
use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene};

#[derive(Default)]
pub struct DocumentLensViewScene {
    view: Option<Retained<DocumentLensView>>,
    selections: Vec<Value>,
}

fn change_kind(raw: &str) -> Option<ChangeKind> {
    [ChangeKind::Inserted, ChangeKind::Deleted, ChangeKind::Modified].into_iter().find(|kind| kind.raw_value() == raw)
}

impl PanelScene for DocumentLensViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let text = scenario.document_text().map_err(Failure::Error)?;
        let view = DocumentLensView::new(style_sheet.clone(), mtm);
        if let Some(target) = scenario.string("target")
            && let Some(built_in) = BuiltInRenderTarget::from_raw_value(&target)
        {
            view.set_render_target_profile(built_in.profile());
        }
        if !scenario.bool("empty") {
            let parsed = MarkdownParser::parse(&text);
            let health = DocumentHealth::analyze_document(&parsed);
            let document_url = document_url(scenario);
            let workspace_root = document_url.as_ref().map(|url| url.deleting_last_path_component());
            let context = AssetResolutionContext::new(document_url, workspace_root);
            let references = AssetDoctor::references(&parsed, &context);
            let assets = AssetDoctor::diagnose(&parsed, &context, Some(&probe()));
            let report = MarkdownCompatibility::diagnose(&parsed, &view.render_target_profile());
            let mut changes = Vec::new();
            for (index, value) in scenario.array("changes").iter().enumerate() {
                let Some(object) = value.as_object() else { continue };
                let (Some(kind), Some(location), Some(length)) = (
                    object.get("kind").and_then(Value::as_str).and_then(change_kind),
                    object.get("location").and_then(Value::as_i64),
                    object.get("length").and_then(Value::as_i64),
                ) else {
                    continue;
                };
                changes.push(DocumentLensChange::new(
                    format!("{index}:{location}"),
                    kind,
                    NSRange::new(location as isize, length as isize),
                    Vec::new(),
                ));
            }
            view.set_source_text(&text);
            let mut input = DocumentLensInput::new(parsed);
            input.health = health;
            input.asset_references = references;
            input.assets = assets;
            input.render_target = Some(report);
            input.changes = changes;
            view.set_model(DocumentLensModel::new(&input));
        }
        if let Some(tab) = scenario.string("tab")
            && let Some(selected) = DocumentLensTab::from_raw_value(&tab)
        {
            view.set_selected_tab(selected);
        }
        let delegate = Rc::new(RecordingLensSceneDelegate::default());
        let weak: std::rc::Weak<dyn DocumentLensViewDelegate> = Rc::downgrade(&(delegate.clone() as Rc<dyn DocumentLensViewDelegate>));
        view.set_delegate(Some(weak));
        if let Some(select) = scenario.int("select") {
            view.select_item_for_testing(select as isize);
        }
        self.selections = delegate.selections.borrow().clone();
        view.set_delegate(None);
        if scenario.bool("restyle") {
            view.set_style_sheet(style_sheet);
        }
        self.view = Some(view.clone());
        Ok(Retained::into_super(view))
    }

    fn model(&self) -> Value {
        let Some(view) = &self.view else { return Value::Null };
        let model = view.model();
        let mut map = Map::new();
        map.insert("preferredWidth".into(), double(view.preferred_width()));
        map.insert("selectedTab".into(), Value::String(view.selected_tab().raw_value().to_owned()));
        map.insert("renderTargetProfile".into(), Value::String(view.render_target_profile().id));
        map.insert(
            "sectionCounts".into(),
            Value::Array(DocumentLensTab::ALL_CASES.iter().map(|tab| Value::from(model.section(*tab).count() as i64)).collect()),
        );
        map.insert("selections".into(), Value::Array(self.selections.clone()));
        map.insert("rows".into(), table_rows(view));
        Value::Object(map)
    }
}

#[derive(Default)]
struct RecordingLensSceneDelegate {
    selections: RefCell<Vec<Value>>,
}

impl DocumentLensViewDelegate for RecordingLensSceneDelegate {
    fn document_lens_did_select(&self, _view: &DocumentLensView, range: NSRange, item: &DocumentLensItem) {
        let mut map = Map::new();
        map.insert("range".into(), Value::Array(vec![Value::from(range.location as i64), Value::from(range.length as i64)]));
        map.insert("id".into(), Value::String(item.id.clone()));
        self.selections.borrow_mut().push(Value::Object(map));
    }

    fn document_lens_did_select_render_target(&self, _view: &DocumentLensView, _profile: &RenderTargetProfile) {}
}
