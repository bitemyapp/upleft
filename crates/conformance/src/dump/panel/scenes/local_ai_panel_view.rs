//! `LocalAIPanelView` scenes (`Scenes/LocalAIPanelViewScene.swift`). The
//! real model is never called: results come from
//! `DeterministicLocalAIProvider`.

use std::cell::RefCell;
use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSButton, NSPopUpButton, NSView};
use serde_json::{Map, Value};
use upleft_app::ai::local_ai::{
    DeterministicLocalAIProvider, LocalAIAvailability, LocalAIError, LocalAIPreview, LocalAIRequest, LocalAIRunError,
    LocalAITask, TaskCancellation,
};
use upleft_app::panels::local_ai_panel_view::{LocalAIPanelView, LocalAIPanelViewDelegate};
use upleft_app::panels::panel_chrome::PanelSurface;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_swift_text::NSRange;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
struct Delegate {
    events: RefCell<Vec<String>>,
}

impl LocalAIPanelViewDelegate for Delegate {
    fn local_ai_panel_did_request(&self, _panel: &LocalAIPanelView, task: LocalAITask) {
        self.events.borrow_mut().push(format!("request {}", task.raw_value()));
    }

    fn local_ai_panel_did_apply(&self, _panel: &LocalAIPanelView, preview: &LocalAIPreview) {
        self.events.borrow_mut().push(format!("apply {} {}", preview.range.location, preview.range.length));
    }

    fn local_ai_panel_did_cancel(&self, _panel: &LocalAIPanelView) {
        self.events.borrow_mut().push("cancel".to_owned());
    }
}

/// Swift's `"\(error)"` for the errors the deterministic provider throws.
fn describe(error: &LocalAIRunError) -> String {
    match error {
        LocalAIRunError::LocalAI(LocalAIError::EmptyInput) => "emptyInput".to_owned(),
        LocalAIRunError::LocalAI(LocalAIError::Cancelled) => "cancelled".to_owned(),
        LocalAIRunError::LocalAI(LocalAIError::Unavailable(availability)) => format!("unavailable({availability:?})"),
        LocalAIRunError::Cancellation => "CancellationError()".to_owned(),
        LocalAIRunError::Other(text) => text.clone(),
    }
}

#[derive(Default)]
pub struct LocalAIPanelViewScene {
    panel: Option<Retained<LocalAIPanelView>>,
    delegate: Option<Rc<Delegate>>,
    error: Option<String>,
}

impl PanelScene for LocalAIPanelViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let panel = if scenario.bool("current") {
            LocalAIPanelView::new_current(mtm)
        } else {
            LocalAIPanelView::new(style_sheet.clone(), mtm)
        };
        if scenario.bool("current") {
            panel.set_style_sheet(style_sheet);
        }
        let delegate = Rc::new(Delegate::default());
        let weak: std::rc::Weak<dyn LocalAIPanelViewDelegate> =
            Rc::downgrade(&(delegate.clone() as Rc<dyn LocalAIPanelViewDelegate>));
        panel.set_delegate(Some(weak));
        self.delegate = Some(delegate);
        match scenario.string("availability").as_deref() {
            Some("available") => panel.set_availability(LocalAIAvailability::Available),
            Some("frameworkUnavailable") => panel.set_availability(LocalAIAvailability::FrameworkUnavailable),
            Some("systemUnavailable") => panel.set_availability(LocalAIAvailability::SystemUnavailable),
            _ => {}
        }
        if scenario.bool("running") {
            panel.set_is_running(true);
        }
        if let Some(task) = scenario.string("task").and_then(|raw| LocalAITask::from_raw_value(&raw)) {
            let source = match scenario.string("source") {
                Some(source) => source,
                None => scenario.document_text().map_err(Failure::Error)?,
            };
            let selection: Vec<i64> = scenario
                .array("selection")
                .iter()
                .filter_map(|value| value.as_i64().or_else(|| value.as_f64().map(|f| f as i64)))
                .collect();
            let request = LocalAIRequest::new(
                task,
                source,
                (selection.len() == 2).then(|| NSRange::new(selection[0] as isize, selection[1] as isize)),
            );
            match DeterministicLocalAIProvider::run_now(&request, &TaskCancellation::new()) {
                Ok(result) => panel.set_result(Some(result)),
                Err(error) => self.error = Some(describe(&error)),
            }
        }
        if scenario.bool("clearResult") {
            panel.set_result(None);
        }
        let popup = panel.subviews().iter().find_map(|view| view.downcast::<NSPopUpButton>().ok());
        for index in scenario.array("pick") {
            let Some(index) = index.as_i64().or_else(|| index.as_f64().map(|f| f as i64)) else { continue };
            let Some(popup) = &popup else { break };
            popup.selectItemAtIndex(index as isize);
            unsafe { popup.sendAction_to(popup.action(), popup.target().as_deref()) };
        }
        let buttons: Vec<Retained<NSButton>> = panel
            .subviews()
            .iter()
            .filter(|view| view.downcast_ref::<NSPopUpButton>().is_none())
            .filter_map(|view| view.downcast::<NSButton>().ok())
            .collect();
        for name in scenario.strings("press") {
            let title = if name == "apply" { "Apply Preview" } else { "Close" };
            let Some(button) = buttons.iter().find(|button| button.title().to_string() == title) else { continue };
            unsafe { button.sendAction_to(button.action(), button.target().as_deref()) };
        }
        self.panel = Some(panel.clone());
        Ok(Retained::into_super(panel))
    }

    fn model(&self) -> Value {
        let Some(panel) = &self.panel else { return Value::Null };
        let result = panel.result();
        let mut map = Map::new();
        map.insert("preferredWidth".into(), double(panel.preferred_width()));
        map.insert("isRunning".into(), Value::Bool(panel.is_running()));
        map.insert("hasResult".into(), Value::Bool(result.is_some()));
        map.insert("resultText".into(), result.as_ref().map_or(Value::Null, |result| Value::String(result.text.clone())));
        map.insert("hasPreview".into(), Value::Bool(result.as_ref().is_some_and(|result| result.preview.is_some())));
        map.insert("error".into(), self.error.clone().map_or(Value::Null, Value::String));
        let events = self.delegate.as_ref().map(|delegate| delegate.events.borrow().clone()).unwrap_or_default();
        map.insert("events".into(), Value::Array(events.into_iter().map(Value::String).collect()));
        map.insert("fittingSize".into(), tree::size(panel.fittingSize()));
        Value::Object(map)
    }
}
