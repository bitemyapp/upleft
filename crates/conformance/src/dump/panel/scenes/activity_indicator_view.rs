//! `ActivityIndicatorView` scenes (`Scenes/ActivityIndicatorViewScene.swift`).

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::NSView;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use serde_json::{Map, Value};
use upleft_app::panels::activity_indicator_view::ActivityIndicatorView;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
pub struct ActivityIndicatorViewScene {
    indicator: Option<Retained<ActivityIndicatorView>>,
    visibility_changes: Rc<RefCell<Vec<bool>>>,
}

impl PanelScene for ActivityIndicatorViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        _style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let indicator = ActivityIndicatorView::new(mtm);
        let changes = self.visibility_changes.clone();
        indicator.set_on_visibility_change(Some(Rc::new(move |hidden| changes.borrow_mut().push(hidden))));
        if scenario.bool("begin") {
            indicator.begin();
        }
        if scenario.bool("end") {
            indicator.end();
        }
        self.indicator = Some(indicator.clone());
        let container = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(scenario.width, scenario.height)),
        );
        indicator.setFrame(NSRect::new(NSPoint::new(4.0, 4.0), NSSize::new(18.0, 18.0)));
        container.addSubview(&indicator);
        Ok(container)
    }

    fn model(&self) -> Value {
        let Some(indicator) = &self.indicator else { return Value::Null };
        let mut map = Map::new();
        map.insert("hidden".into(), Value::Bool(indicator.isHidden()));
        map.insert("intrinsicContentSize".into(), tree::size(indicator.intrinsicContentSize()));
        map.insert(
            "visibilityChanges".into(),
            Value::Array(self.visibility_changes.borrow().iter().map(|hidden| Value::Bool(*hidden)).collect()),
        );
        Value::Object(map)
    }
}
