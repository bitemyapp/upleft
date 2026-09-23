//! `SearchInspectorView` scenes (`Scenes/SearchInspectorViewScene.swift`):
//! the same state, applied with the same calls in the same order; see the
//! Swift file for the keys.

use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use serde_json::{Map, Value};
use upleft_app::panels::appkit_support::{ns_string, superview};
use upleft_app::panels::search_inspector_view::SearchInspectorView;
use upleft_app::panels::search_results_panel_view::SearchResultsPanelView;
use upleft_app::support::find_engine::FindSession;
use upleft_render::theme::style_sheet::StyleSheet;

use super::find_bar_view::{apply_find_state, text_field};
use super::search_results_panel_view::search;
use crate::dump::Failure;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
pub struct SearchInspectorViewScene {
    inspector: Option<Retained<SearchInspectorView>>,
    results: Option<Retained<SearchResultsPanelView>>,
    session: Option<FindSession>,
}

impl PanelScene for SearchInspectorViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let inspector = if scenario.bool("current") {
            SearchInspectorView::new_current(mtm)
        } else {
            SearchInspectorView::new(style_sheet.clone(), mtm)
        };
        if scenario.bool("current") {
            inspector.set_style_sheet(style_sheet.clone());
        }
        let bar = inspector.find_bar();
        self.session = apply_find_state(&bar, scenario, mtm)?;
        if scenario.bool("showsReplace") {
            inspector.set_shows_replace(true);
        }
        if let Some(replacement) = scenario.string("replacement")
            && let Some(field) = text_field("Replace with", &bar)
        {
            field.setStringValue(&ns_string(&replacement));
        }
        if scenario.bool("results") {
            let results = SearchResultsPanelView::new(style_sheet.clone(), mtm);
            search(&results, scenario, Some(bar.current_query()))?;
            inspector.set_results(Some(&results));
            if scenario.bool("resultsTwice") {
                inspector.set_results(Some(&results));
            }
            self.results = Some(results);
        }
        if scenario.bool("clearResults") {
            inspector.set_results(None);
        }
        self.inspector = Some(inspector.clone());
        Ok(Retained::into_super(inspector))
    }

    fn model(&self) -> Value {
        let Some(inspector) = &self.inspector else { return Value::Null };
        let bar = inspector.find_bar();
        let mut map = Map::new();
        map.insert("showsReplace".into(), Value::Bool(inspector.shows_replace()));
        map.insert("findBarShowsReplace".into(), Value::Bool(bar.shows_replace()));
        map.insert("findBarFrame".into(), tree::rect(bar.frame()));
        map.insert("findBarWantsLayer".into(), Value::Bool(bar.wantsLayer()));
        map.insert("statusText".into(), Value::String(bar.status_text()));
        map.insert("isQueryValid".into(), Value::Bool(bar.is_query_valid()));
        map.insert("queryText".into(), Value::String(bar.current_query().text));
        map.insert(
            "resultsAttached".into(),
            Value::Bool(self.results.as_ref().is_some_and(|results| superview(results).is_some())),
        );
        map.insert(
            "resultsHitCount".into(),
            Value::from(self.results.as_ref().map_or(-1, |results| results.hits().len() as i64)),
        );
        if let Some(session) = &self.session {
            map.insert("matchCount".into(), Value::from(session.count() as i64));
        }
        Value::Object(map)
    }
}
