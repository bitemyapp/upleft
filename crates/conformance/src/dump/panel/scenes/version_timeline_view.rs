//! `VersionTimelineView` scenes (`Scenes/VersionTimelineViewScene.swift`).

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::NSView;
use serde_json::{Map, Value};
use upleft_app::ai::snapshot_store::{SnapshotKind, VersionRecord};
use upleft_app::panels::version_timeline_view::{VersionTimelineDelegate, VersionTimelineView};
use upleft_foundation::date::Date;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

/// `VersionTimelineViewScene.versions(_:)`.
pub fn versions(scenario: &PanelScenario) -> Vec<VersionRecord> {
    let base = scenario.double_or("base", 650_280_600.0);
    scenario
        .array("versions")
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let object = value.as_object()?;
            let at = object.get("at").and_then(Value::as_f64).unwrap_or(0.0);
            let kind = object
                .get("kind")
                .and_then(Value::as_str)
                .map_or(Some(SnapshotKind::Local), SnapshotKind::from_raw_value)
                .unwrap_or(SnapshotKind::Local);
            Some(VersionRecord {
                hash: object.get("hash").and_then(Value::as_str).map_or_else(|| format!("hash-{index}"), str::to_owned),
                date: Date::from_reference(base + at),
                byte_count: object
                    .get("bytes")
                    .and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|f| f as i64)))
                    .unwrap_or(100) as isize,
                kind,
            })
        })
        .collect()
}

/// `step > 0 ? accessibilityPerformIncrement() : accessibilityPerformDecrement()`.
pub fn perform_steps(timeline: &VersionTimelineView, scenario: &PanelScenario) {
    for step in scenario.array("steps") {
        let Some(step) = step.as_i64().or_else(|| step.as_f64().map(|f| f as i64)) else { continue };
        let _: bool = if step > 0 {
            unsafe { msg_send![timeline, accessibilityPerformIncrement] }
        } else {
            unsafe { msg_send![timeline, accessibilityPerformDecrement] }
        };
    }
}

struct Delegate {
    scrubs: Rc<RefCell<Vec<String>>>,
}

impl VersionTimelineDelegate for Delegate {
    fn version_timeline_did_scrub_to(&self, _view: &VersionTimelineView, record: &VersionRecord) {
        self.scrubs.borrow_mut().push(record.hash.clone());
    }

    fn version_timeline_did_request_restore(&self, _view: &VersionTimelineView, _record: &VersionRecord) {}
}

#[derive(Default)]
pub struct VersionTimelineViewScene {
    timeline: Option<Retained<VersionTimelineView>>,
    scrubs: Rc<RefCell<Vec<String>>>,
    delegate: Option<Rc<dyn VersionTimelineDelegate>>,
}

impl PanelScene for VersionTimelineViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let timeline = if scenario.bool("current") {
            VersionTimelineView::new_current(mtm)
        } else {
            VersionTimelineView::new(style_sheet.clone(), mtm)
        };
        if scenario.bool("current") {
            timeline.set_style_sheet(style_sheet);
        }
        let delegate: Rc<dyn VersionTimelineDelegate> = Rc::new(Delegate { scrubs: self.scrubs.clone() });
        timeline.set_delegate(Some(Rc::downgrade(&delegate)));
        self.delegate = Some(delegate);
        timeline.set_versions(versions(scenario));
        if let Some(index) = scenario.int("selectedIndex") {
            timeline.set_selected_index(index as isize);
        }
        perform_steps(&timeline, scenario);
        self.timeline = Some(timeline.clone());
        Ok(Retained::into_super(timeline))
    }

    fn model(&self) -> Value {
        let Some(timeline) = &self.timeline else { return Value::Null };
        let mut map = Map::new();
        map.insert("selectedIndex".into(), Value::from(timeline.selected_index() as i64));
        map.insert(
            "selectedHash".into(),
            timeline.selected_record().map_or(Value::Null, |record| Value::String(record.hash)),
        );
        map.insert("versionCount".into(), Value::from(timeline.versions().len() as i64));
        map.insert("intrinsicContentSize".into(), tree::size(timeline.intrinsicContentSize()));
        map.insert("isFlipped".into(), Value::Bool(timeline.isFlipped()));
        map.insert("acceptsFirstResponder".into(), Value::Bool(timeline.acceptsFirstResponder()));
        map.insert("focusRingMaskBounds".into(), tree::rect(timeline.focusRingMaskBounds()));
        map.insert("scrubs".into(), Value::Array(self.scrubs.borrow().iter().cloned().map(Value::String).collect()));
        Value::Object(map)
    }
}
