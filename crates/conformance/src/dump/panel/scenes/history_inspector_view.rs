//! `HistoryInspectorView` scenes (`Scenes/HistoryInspectorViewScene.swift`).

use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use serde_json::{Map, Value};
use upleft_app::panels::history_inspector_view::HistoryInspectorView;
use upleft_app::panels::version_timeline_view::VersionTimelineView;
use upleft_render::theme::style_sheet::StyleSheet;

use super::version_timeline_view::{perform_steps, versions};
use crate::dump::Failure;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
pub struct HistoryInspectorViewScene {
    inspector: Option<Retained<HistoryInspectorView>>,
    timeline: Option<Retained<VersionTimelineView>>,
}

impl PanelScene for HistoryInspectorViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let inspector = if scenario.bool("current") {
            HistoryInspectorView::new_current(mtm)
        } else {
            HistoryInspectorView::new(style_sheet.clone(), mtm)
        };
        if scenario.bool("current") {
            inspector.set_style_sheet(style_sheet);
        }
        inspector.set_versions(versions(scenario));
        let timeline = inspector
            .subviews()
            .iter()
            .find_map(|view| view.downcast::<VersionTimelineView>().ok());
        if let Some(timeline) = &timeline {
            perform_steps(timeline, scenario);
        }
        self.inspector = Some(inspector.clone());
        self.timeline = timeline;
        Ok(Retained::into_super(inspector))
    }

    fn model(&self) -> Value {
        let Some(inspector) = &self.inspector else { return Value::Null };
        let mut map = Map::new();
        map.insert("versionCount".into(), Value::from(inspector.versions().len() as i64));
        map.insert(
            "selectedIndex".into(),
            self.timeline.as_ref().map_or(Value::Null, |timeline| Value::from(timeline.selected_index() as i64)),
        );
        map.insert(
            "selectedHash".into(),
            self.timeline
                .as_ref()
                .and_then(|timeline| timeline.selected_record())
                .map_or(Value::Null, |record| Value::String(record.hash)),
        );
        map.insert("fittingSize".into(), tree::size(inspector.fittingSize()));
        Value::Object(map)
    }
}
