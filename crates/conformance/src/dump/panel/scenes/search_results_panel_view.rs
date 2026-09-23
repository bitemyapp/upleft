//! `SearchResultsPanelView` scenes (`Scenes/SearchResultsPanelViewScene.swift`):
//! the same state, applied with the same calls in the same order; see the
//! Swift file for the keys.

use std::cell::RefCell;
use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use objc2_foundation::NSIndexSet;
use serde_json::{Map, Value};
use upleft_app::panels::appkit_support::object;
use upleft_app::panels::search_results_panel_view::{SearchResultsDelegate, SearchResultsPanelView};
use upleft_app::support::find_engine::{FindEngine, FindQuery, SiblingHit, SiblingSearch};
use upleft_foundation::url::FileUrl;
use upleft_render::theme::style_sheet::StyleSheet;

use super::find_bar_view::range_json;
use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, repository_root};

/// `SearchResultsSceneRecorder`.
#[derive(Default)]
pub struct SearchResultsSceneRecorder {
    selected: RefCell<Vec<SiblingHit>>,
}

impl SearchResultsDelegate for SearchResultsSceneRecorder {
    fn search_results_did_select(&self, _view: &SearchResultsPanelView, hit: &SiblingHit) {
        self.selected.borrow_mut().push(hit.clone());
    }
}

#[derive(Default)]
pub struct SearchResultsPanelViewScene {
    panel: Option<Retained<SearchResultsPanelView>>,
    recorder: Rc<SearchResultsSceneRecorder>,
}

/// `SearchResultsPanelViewScene.siblingURLs(_:)`.
pub fn sibling_urls(scenario: &PanelScenario) -> Result<Vec<FileUrl>, Failure> {
    let root = repository_root();
    let files = scenario.strings("files");
    if !files.is_empty() {
        return Ok(files.iter().map(|file| FileUrl::from_path(&root.join(file).to_string_lossy())).collect());
    }
    let Some(directory) = scenario.string("directory") else { return Ok(Vec::new()) };
    let folder = root.join(directory);
    let mut names: Vec<String> = std::fs::read_dir(&folder)?
        .filter_map(|entry| entry.ok().map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .filter(|name| name.ends_with(".md"))
        .collect();
    names.sort();
    Ok(names.iter().map(|name| FileUrl::from_path(&folder.join(name).to_string_lossy())).collect())
}

/// `SearchResultsPanelViewScene.search(_:_:)`: the window controller's
/// sibling pass, run synchronously.
/// `given` is the find bar's current query when the panel sits in the
/// search inspector. (`var query = FindQuery()`, then field by field.)
#[allow(clippy::field_reassign_with_default)]
pub fn search(panel: &SearchResultsPanelView, scenario: &PanelScenario, given: Option<FindQuery>) -> Result<(), Failure> {
    if let Some(text) = scenario.string("query") {
        let options = scenario.strings("options");
        let mut query = FindQuery::default();
        query.text = text;
        query.is_regex = options.iter().any(|option| option == "regex");
        query.case_sensitive = options.iter().any(|option| option == "caseSensitive");
        query.whole_word = options.iter().any(|option| option == "wholeWord");
        if let Some(given) = given {
            query = given;
        }
        let urls = sibling_urls(scenario)?;
        panel.set_query(&query.text);
        panel.set_searched_file_count(urls.len() as isize);
        panel.set_hits(Vec::new());
        panel.set_is_searching(!query.is_empty() && FindEngine::is_valid(&query));
        if !scenario.bool("isSearching") {
            panel.set_hits(SiblingSearch::search(&query, &urls, scenario.int_or("limitPerFile", 20) as usize, &|| false));
            panel.set_is_searching(false);
        }
    }
    if let Some(count) = scenario.int("searchedFileCount") {
        panel.set_searched_file_count(count as isize);
    }
    Ok(())
}

/// `SearchResultsPanelViewScene.hitJSON(_:)`.
pub fn hit_json(hit: &SiblingHit) -> Value {
    Value::Array(vec![
        Value::String(hit.display_name.clone()),
        Value::from(hit.line_number as i64),
        range_json(Some(hit.range)),
        range_json(Some(hit.context_range)),
        hit.heading_title.clone().map_or(Value::Null, Value::String),
    ])
}

impl PanelScene for SearchResultsPanelViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let panel = if scenario.bool("current") {
            SearchResultsPanelView::new_current(mtm)
        } else {
            SearchResultsPanelView::new(style_sheet.clone(), mtm)
        };
        if scenario.bool("current") {
            panel.set_style_sheet(style_sheet);
        }
        let recorder: Rc<dyn SearchResultsDelegate> = self.recorder.clone();
        panel.set_delegate(Some(Rc::downgrade(&recorder)));
        search(&panel, scenario, None)?;
        let table = panel.table_for_testing();
        if let Some(row) = scenario.int("select") {
            table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row as usize), false);
        }
        if scenario.bool("activate") {
            table.activate();
        }
        self.panel = Some(panel.clone());
        Ok(Retained::into_super(panel))
    }

    fn model(&self) -> Value {
        let Some(panel) = &self.panel else { return Value::Null };
        let empty = panel.empty_state_for_testing();
        let hits = panel.hits();
        let value: Option<Retained<objc2_foundation::NSString>> =
            unsafe { objc2::msg_send![object(&**panel), accessibilityValue] };
        let mut map = Map::new();
        map.insert("query".into(), Value::String(panel.query()));
        map.insert("searchedFileCount".into(), Value::from(panel.searched_file_count() as i64));
        map.insert("isSearching".into(), Value::Bool(panel.is_searching()));
        map.insert(
            "preferredWidth".into(),
            double(upleft_app::panels::panel_chrome::panel_surface_preferred_width(panel).unwrap_or(0.0)),
        );
        map.insert("hitCount".into(), Value::from(hits.len() as i64));
        map.insert("hits".into(), Value::Array(hits.iter().take(40).map(hit_json).collect()));
        map.insert("emptyTitle".into(), Value::String(empty.title()));
        map.insert("emptySubtitle".into(), Value::String(empty.subtitle()));
        map.insert("accessibilityValue".into(), value.map_or(Value::Null, |value| Value::String(value.to_string())));
        map.insert("selected".into(), Value::Array(self.recorder.selected.borrow().iter().map(hit_json).collect()));
        Value::Object(map)
    }
}
