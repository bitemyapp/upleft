//! `UpdateWindowController` scenes (`Scenes/UpdateWindowControllerScene.swift`):
//! the titled update window in each state of a coordinator over a
//! `FakeUpdateEngine` (started, UI suppressed, so nothing but this window is
//! ever built).
//!
//! The window is the scene's own (`own_window`): titled, so the harness has
//! already made `constrainFrameRect:toScreen:` the identity; it moves the
//! window to (-30000, -30000) before ordering it in, and `showWindow` is
//! never called. The header shows the application icon, which the scene
//! sets to the bundle's `AppIcon.icns` (`use_bundle_icon`). The panel draws
//! from its own style sheet (the current theme, which the scene selects,
//! against the window's appearance, with the system Reduce Motion); nothing
//! in it animates except progress indicators, which `before_settle_check`
//! stops where they stand so the capture settles.

use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSView, NSWindow};
use serde_json::{Map, Value};
use upleft_app::panels::appkit_support::{downcast, object};
use upleft_app::panels::update_window_controller::{
    Kind, UpdateFailureView, UpdatePanelContent, UpdateWindowController,
};
use upleft_app::updater::update_coordinator::UpdateCoordinator;
use upleft_app::updater::update_engine::{FakeUpdateEngine, UpdateEngine};
use upleft_app::updater::update_state_machine::UpdatePhase;
use upleft_render::theme::style_sheet::StyleSheet;

use super::update_status_pill::{pill_json, run, select_theme, stop_indicators, use_bundle_icon};
use crate::dump::Failure;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
pub struct UpdateWindowControllerScene {
    controller: Option<Retained<UpdateWindowController>>,
    coordinator: Option<Rc<UpdateCoordinator>>,
    engine: Option<Rc<FakeUpdateEngine>>,
}

fn failure_view(view: &NSView) -> Option<Retained<UpdateFailureView>> {
    if let Some(failure) = downcast::<UpdateFailureView>(object(view)) {
        return Some(failure);
    }
    for subview in view.subviews().iter() {
        if let Some(failure) = failure_view(&subview) {
            return Some(failure);
        }
    }
    None
}

/// Swift's `"\(kind)"` for `UpdatePanelContent.Kind`.
fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Checking => "checking",
        Kind::Available => "available",
        Kind::Downloading => "downloading",
        Kind::Extracting => "extracting",
        Kind::Ready => "ready",
        Kind::Waiting => "waiting",
        Kind::Installing => "installing",
        Kind::Informational => "informational",
        Kind::UpToDate => "upToDate",
        Kind::Failed => "failed",
    }
}

impl PanelScene for UpdateWindowControllerScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        _style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        select_theme(&scenario.theme);
        use_bundle_icon(mtm);
        let engine = FakeUpdateEngine::new();
        let coordinator = UpdateCoordinator::new(Some(engine.clone() as Rc<dyn UpdateEngine>));
        coordinator.set_suppress_ui_for_testing(true);
        let _ = engine.start();
        self.engine = Some(engine.clone());
        self.coordinator = Some(coordinator.clone());
        run(&scenario.array("steps"), &coordinator, Some(&engine));
        let controller = UpdateWindowController::new(&coordinator, mtm);
        self.controller = Some(controller.clone());
        run(&scenario.array("stepsAfter"), &coordinator, Some(&engine));
        let content = controller.window().and_then(|window| window.contentView()).expect("the panel view");
        if scenario.bool("showDetails")
            && let Some(failure) = failure_view(&content)
        {
            failure.toggle_detail_for_testing();
        }
        Ok(content)
    }

    fn own_window(&mut self, _panel: &NSView) -> Option<Retained<NSWindow>> {
        self.controller.as_ref().and_then(|controller| controller.window())
    }

    fn before_settle_check(&mut self) {
        if let Some(content) = self.controller.as_ref().and_then(|controller| controller.window()).and_then(|window| window.contentView()) {
            stop_indicators(&content);
        }
    }

    fn model(&self) -> Value {
        let (Some(controller), Some(coordinator)) = (&self.controller, &self.coordinator) else { return Value::Null };
        let Some(panel_view) = controller.panel_view() else { return Value::Null };
        let footer = panel_view.footer();
        let buttons: Vec<Value> = footer
            .buttons()
            .iter()
            .map(|button| {
                let mut map = Map::new();
                map.insert("title".into(), Value::String(button.button_title()));
                map.insert("primary".into(), Value::Bool(button.is_primary()));
                Value::Object(map)
            })
            .collect();
        let (leading, trailing) = footer.stacks();
        let content = panel_view.content_container().subviews().firstObject();
        let diagnostics = match coordinator.phase() {
            UpdatePhase::Failed(failure, _) => Value::String(UpdatePanelContent::diagnostics(coordinator, &failure)),
            _ => Value::Null,
        };
        let (title, versions) = panel_view.header_text();
        let mut map = Map::new();
        map.insert("windowFrame".into(), tree::rect(controller.window().expect("window").frame()));
        map.insert("title".into(), Value::String(title));
        map.insert("versions".into(), Value::String(versions));
        map.insert(
            "lastKind".into(),
            panel_view.last_kind().map_or(Value::Null, |kind| Value::String(kind_name(kind).to_owned())),
        );
        map.insert(
            "content".into(),
            content.map_or(Value::Null, |content| Value::String(tree::class_name(object(&*content)))),
        );
        map.insert("buttons".into(), Value::Array(buttons));
        map.insert("leading".into(), Value::from(leading.arrangedSubviews().count() as i64));
        map.insert("trailing".into(), Value::from(trailing.arrangedSubviews().count() as i64));
        map.insert("pill".into(), pill_json(coordinator.pill_model().as_ref()));
        map.insert("diagnostics".into(), diagnostics);
        Value::Object(map)
    }
}
