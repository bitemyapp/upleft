//! `ConflictBarView` scenes (`Scenes/ConflictBarViewScene.swift`).

use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use serde_json::{Map, Value};
use upleft_app::panels::conflict_bar_view::ConflictBarView;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
pub struct ConflictBarViewScene {
    bar: Option<Retained<ConflictBarView>>,
}

impl PanelScene for ConflictBarViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let bar = if scenario.bool("current") {
            ConflictBarView::new_current(mtm)
        } else {
            ConflictBarView::new(style_sheet.clone(), mtm)
        };
        if scenario.bool("current") {
            bar.set_style_sheet(style_sheet);
        }
        if let Some(status) = scenario.string("status") {
            bar.set_status(&status);
        }
        if let Some(message) = scenario.string("message") {
            bar.set_message(&message);
        }
        self.bar = Some(bar.clone());
        Ok(Retained::into_super(Retained::into_super(bar)))
    }

    fn model(&self) -> Value {
        let Some(bar) = &self.bar else { return Value::Null };
        let mut map = Map::new();
        map.insert("message".into(), Value::String(bar.message()));
        map.insert("fittedWidth".into(), double(bar.fitted_width()));
        map.insert("intrinsicContentSize".into(), tree::size(bar.intrinsicContentSize()));
        Value::Object(map)
    }
}
