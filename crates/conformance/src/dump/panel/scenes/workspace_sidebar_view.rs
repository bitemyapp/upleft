//! `WorkspaceSidebarView` scenes (`Scenes/WorkspaceSidebarViewScene.swift`),
//! over the sample workspace indexed as the `workspace` suite does.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSScrollView, NSTableView, NSView};
use objc2_foundation::NSIndexSet;
use serde_json::{Map, Value};
use upleft_app::panels::panel_chrome::PanelSurface;
use upleft_app::panels::workspace_sidebar_view::{WorkspaceSidebarTab, WorkspaceSidebarView, WorkspaceSidebarViewDelegate};
use upleft_app::workspace::workspace_index::{WorkspaceIndexPolicy, WorkspaceIndexPolicyInit, WorkspaceIndexSnapshot};
use upleft_app::workspace::workspace_link_graph::WorkspaceLinkGraphBuilder;
use upleft_app::workspace::workspace_search::{WorkspaceSearch, WorkspaceSearchQuery};
use upleft_core::NSRange;
use upleft_foundation::url::FileUrl;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, repository_root, tree};

thread_local! {
    static SNAPSHOTS: RefCell<HashMap<String, WorkspaceIndexSnapshot>> = RefCell::new(HashMap::new());
}

#[derive(Default)]
struct Delegate {
    events: RefCell<Vec<String>>,
}

impl WorkspaceSidebarViewDelegate for Delegate {
    fn workspace_sidebar_did_select(
        &self,
        _view: &WorkspaceSidebarView,
        url: &FileUrl,
        range: Option<NSRange>,
        in_new_window: bool,
    ) {
        let range_text = range.map_or("nil".to_owned(), |range| format!("{} {}", range.location, range.length));
        self.events.borrow_mut().push(format!("select {} {range_text} {in_new_window}", url.last_path_component()));
    }

    fn workspace_sidebar_did_search(&self, _view: &WorkspaceSidebarView, query: &WorkspaceSearchQuery) {
        self.events.borrow_mut().push(format!("search {}", query.text));
    }
}

fn tab(name: &str) -> Option<WorkspaceSidebarTab> {
    match name {
        "files" => Some(WorkspaceSidebarTab::Files),
        "search" => Some(WorkspaceSidebarTab::Search),
        "backlinks" => Some(WorkspaceSidebarTab::Backlinks),
        _ => None,
    }
}

#[derive(Default)]
pub struct WorkspaceSidebarViewScene {
    sidebar: Option<Retained<WorkspaceSidebarView>>,
    delegate: Option<Rc<Delegate>>,
}

impl PanelScene for WorkspaceSidebarViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let root_path = repository_root().join(scenario.string_or("workspace", "corpus/workspace"));
        let root = FileUrl::from_path_is_directory(&root_path.to_string_lossy(), true).standardized_file_url();
        std::env::set_current_dir("/").map_err(|error| Failure::Error(format!("cannot move to /: {error}")))?;
        let key = root.path();
        let cached = SNAPSHOTS.with(|snapshots| snapshots.borrow().get(&key).cloned());
        let snapshot = match cached {
            Some(snapshot) => snapshot,
            None => {
                let updates = crate::dump::workspace::index(&root, WorkspaceIndexPolicy::new(WorkspaceIndexPolicyInit::default()))?;
                let snapshot = updates[1].clone();
                SNAPSHOTS.with(|snapshots| snapshots.borrow_mut().insert(key, snapshot.clone()));
                snapshot
            }
        };

        let sidebar = if scenario.bool("current") {
            WorkspaceSidebarView::new_current(mtm)
        } else {
            WorkspaceSidebarView::new(style_sheet.clone(), mtm)
        };
        if scenario.bool("current") {
            sidebar.set_style_sheet(style_sheet);
        }
        let delegate = Rc::new(Delegate::default());
        let weak: std::rc::Weak<dyn WorkspaceSidebarViewDelegate> =
            Rc::downgrade(&(delegate.clone() as Rc<dyn WorkspaceSidebarViewDelegate>));
        sidebar.set_delegate(Some(weak));
        self.delegate = Some(delegate);
        if let Some(error) = scenario.string("error") {
            sidebar.set_is_scanning(false);
            sidebar.set_error_message(Some(error));
        } else {
            sidebar.set_is_scanning(true);
            if !scenario.bool("stillScanning") {
                sidebar.set_is_scanning(false);
                let graph = WorkspaceLinkGraphBuilder::build(&snapshot);
                match scenario.int("entryLimit") {
                    Some(limit) => {
                        sidebar.set_entries(snapshot.entries.iter().take(limit.max(0) as usize).cloned().collect())
                    }
                    None => sidebar.set_entries(snapshot.entries.clone()),
                }
                sidebar.set_search_results(Vec::new());
                let current_id = scenario
                    .string("currentFile")
                    .map(|file| root.appending_path_component(&file).standardized_file_url().path());
                sidebar.set_selected_file_id(current_id.clone());
                sidebar.set_backlinks(current_id.map(|id| graph.links_to(&id)).unwrap_or_default());
            }
        }
        if let Some(name) = scenario.string("tab")
            && let Some(tab) = tab(&name)
        {
            sidebar.set_selected_tab(tab);
        }
        if let Some(text) = scenario.string("search") {
            sidebar.set_search_text_for_testing(&text);
            if !scenario.bool("searchPending") {
                sidebar.set_search_results(WorkspaceSearch::search(&WorkspaceSearchQuery::new(text), &snapshot));
            }
        }
        let table = sidebar.subviews().iter().find_map(|view| {
            let scroll = view.downcast::<NSScrollView>().ok()?;
            scroll.documentView()?.downcast::<NSTableView>().ok()
        });
        if let Some(row) = scenario.int("select")
            && let Some(table) = &table
        {
            table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row as usize), false);
        }
        if scenario.bool("activate")
            && let Some(table) = &table
        {
            unsafe { table.sendAction_to(table.action(), table.target().as_deref()) };
        }
        self.sidebar = Some(sidebar.clone());
        Ok(Retained::into_super(sidebar))
    }

    fn model(&self) -> Value {
        let Some(sidebar) = &self.sidebar else { return Value::Null };
        let mut map = Map::new();
        map.insert("preferredWidth".into(), double(sidebar.preferred_width()));
        map.insert("selectedTab".into(), Value::String(sidebar.selected_tab().raw_value().into()));
        map.insert("entryCount".into(), Value::from(sidebar.entries().len() as i64));
        map.insert("searchResultCount".into(), Value::from(sidebar.search_results().len() as i64));
        map.insert("backlinkCount".into(), Value::from(sidebar.backlinks().len() as i64));
        map.insert("isScanning".into(), Value::Bool(sidebar.is_scanning()));
        let events = self.delegate.as_ref().map(|delegate| delegate.events.borrow().clone()).unwrap_or_default();
        map.insert("events".into(), Value::Array(events.into_iter().map(Value::String).collect()));
        map.insert("fittingSize".into(), tree::size(sidebar.fittingSize()));
        Value::Object(map)
    }
}
