//! `VisualDebuggerView` scenes (`Scenes/VisualDebuggerViewScene.swift`).

use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use serde_json::{Map, Value};
use upleft_app::debugging::visual_debugger_model::{VisualDebuggerInput, VisualDebuggerModel, VisualDebuggerStyleFacts};
use upleft_app::panels::panel_chrome::PanelSurface;
use upleft_app::panels::visual_debugger_view::VisualDebuggerView;
use upleft_core::NSRange;
use upleft_core::parser::MarkdownParser;
use upleft_render::render_contracts::RenderMode;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
pub struct VisualDebuggerViewScene {
    view: Option<Retained<VisualDebuggerView>>,
}

impl PanelScene for VisualDebuggerViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let view = if scenario.bool("current") {
            VisualDebuggerView::new_current(mtm)
        } else {
            VisualDebuggerView::new(style_sheet.clone(), mtm)
        };
        if scenario.bool("current") {
            view.set_style_sheet(style_sheet);
        }
        if !scenario.bool("noModel") {
            let document = MarkdownParser::parse(&scenario.document_text().map_err(Failure::Error)?);
            let selection: Vec<i64> = scenario
                .array("selection")
                .iter()
                .filter_map(|value| value.as_i64().or_else(|| value.as_f64().map(|f| f as i64)))
                .collect();
            let style = scenario.object("style");
            let text = |key: &str| style.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
            let number = |key: &str| style.get(key).and_then(Value::as_f64).unwrap_or(0.0);
            let attributes: Vec<String> = style
                .get("attributes")
                .and_then(Value::as_array)
                .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
                .unwrap_or_default();
            view.set_model(VisualDebuggerModel::new(&VisualDebuggerInput::with(
                document,
                if selection.len() == 2 {
                    NSRange::new(selection[0] as isize, selection[1] as isize)
                } else {
                    NSRange::new(0, 0)
                },
                RenderMode::from_raw_value(&scenario.string_or("mode", "live")).unwrap_or(RenderMode::Live),
                VisualDebuggerStyleFacts::new(
                    text("fontFamily"),
                    number("pointSize"),
                    text("foregroundColor"),
                    text("paragraphAlignment"),
                    number("lineHeight"),
                    number("lineSpacing"),
                    attributes,
                ),
                None,
                None,
                Vec::new(),
            )));
        }
        self.view = Some(view.clone());
        Ok(Retained::into_super(view))
    }

    fn model(&self) -> Value {
        let Some(view) = &self.view else { return Value::Null };
        let model = view.model();
        let mut map = Map::new();
        map.insert("preferredWidth".into(), double(view.preferred_width()));
        map.insert("line".into(), Value::from(model.line as i64));
        map.insert("column".into(), Value::from(model.column as i64));
        map.insert("summary".into(), Value::String(view.summary_text_for_testing()));
        map.insert("acceptsFirstResponder".into(), Value::Bool(view.acceptsFirstResponder()));
        map.insert("fittingSize".into(), tree::size(view.fittingSize()));
        Value::Object(map)
    }
}
