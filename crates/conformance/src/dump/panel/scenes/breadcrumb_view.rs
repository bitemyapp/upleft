//! `BreadcrumbView` scenes (`Scenes/BreadcrumbViewScene.swift`).

use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use serde_json::{Map, Value};
use upleft_app::panels::breadcrumb_view::{BreadcrumbView, Crumb};
use upleft_core::contracts::ZoomLevel;
use upleft_core::model::HeadingNode;
use upleft_core::parser::MarkdownParser;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
pub struct BreadcrumbViewScene {
    crumb: Option<Retained<BreadcrumbView>>,
    headings: Vec<HeadingNode>,
}

impl BreadcrumbViewScene {
    /// The ancestor chain of a heading, as
    /// `DocumentWindowController.refreshBreadcrumb` builds it.
    fn trail(&self, raw: i64) -> Vec<Crumb> {
        let count = self.headings.len() as i64;
        let position = if raw < 0 { count + raw } else { raw };
        if position < 0 || position >= count {
            return Vec::new();
        }
        let mut index = position as isize;
        let mut trail = Vec::new();
        loop {
            let heading = &self.headings[index as usize];
            trail.insert(0, Crumb::new(index, &heading.title, heading.level));
            let Some(parent) = heading.parent_index else { break };
            index = parent;
        }
        trail
    }
}

impl PanelScene for BreadcrumbViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let text = scenario.document_text().map_err(Failure::Error)?;
        self.headings = MarkdownParser::parse(&text).headings.clone();
        let crumb = if scenario.bool("current") {
            BreadcrumbView::new_current(mtm)
        } else {
            BreadcrumbView::new(style_sheet.clone(), mtm)
        };
        if scenario.bool("current") {
            crumb.set_style_sheet(style_sheet);
        }
        if let Some(heading) = scenario.int("heading") {
            crumb.set_trail(self.trail(heading));
        }
        let explicit: Vec<Vec<Value>> =
            scenario.array("trail").iter().filter_map(|entry| entry.as_array().cloned()).collect();
        if !explicit.is_empty() {
            crumb.set_trail(
                explicit
                    .iter()
                    .map(|entry| {
                        let int = |value: &Value| value.as_i64().or_else(|| value.as_f64().map(|f| f as i64));
                        Crumb::new(
                            int(&entry[0]).unwrap_or(0) as isize,
                            entry[1].as_str().unwrap_or(""),
                            int(&entry[2]).unwrap_or(1) as isize,
                        )
                    })
                    .collect(),
            );
        }
        if let Some(zoom) = scenario.int("zoom")
            && let Some(level) = ZoomLevel::from_raw_value(zoom as isize)
        {
            crumb.set_zoom_level(level);
        }
        if scenario.bool("present") {
            crumb.show_current_section();
        }
        if let Some(next) = scenario.int("next") {
            crumb.set_trail(self.trail(next));
        }
        if scenario.bool("hide") {
            crumb.hide_current_section();
        }
        if scenario.bool("clear") {
            crumb.set_trail(Vec::new());
        }
        self.crumb = Some(crumb.clone());
        Ok(Retained::into_super(crumb))
    }

    fn model(&self) -> Value {
        let Some(crumb) = &self.crumb else { return Value::Null };
        let menu = crumb.make_path_menu();
        let trail = crumb.trail();
        let mut map = Map::new();
        map.insert("presented".into(), Value::Bool(crumb.is_presented_for_testing()));
        map.insert("currentTitleOrigin".into(), double(crumb.current_title_origin()));
        map.insert(
            "trail".into(),
            Value::Array(
                trail
                    .iter()
                    .map(|crumb| {
                        Value::Array(vec![crumb.index.into(), Value::String(crumb.title.clone()), crumb.level.into()])
                    })
                    .collect(),
            ),
        );
        map.insert("zoomLevel".into(), crumb.zoom_level().raw_value().into());
        map.insert(
            "pathMenu".into(),
            Value::Array(
                menu.itemArray()
                    .iter()
                    .map(|item| {
                        Value::Array(vec![
                            Value::String(item.title().to_string()),
                            item.indentationLevel().into(),
                            item.state().into(),
                        ])
                    })
                    .collect(),
            ),
        );
        map.insert("sameTrailSelf".into(), Value::Bool(BreadcrumbView::same_trail(&trail, &trail)));
        map.insert("intrinsicContentSize".into(), tree::size(crumb.intrinsicContentSize()));
        Value::Object(map)
    }
}
