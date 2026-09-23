//! Port of `App/DocumentWindowController+Workspace.swift`: optional folder
//! workspace hooks. The normal single-file path stays unchanged until the
//! user summons this panel.
//!
//! The scan and searches run off the main thread in `WorkspaceIndex` and
//! `WorkspaceSearchSession`, as in Swift; the panel shows its scanning state
//! meanwhile. The link graph is built on the main thread when a snapshot
//! lands, as Swift builds it.
//!
//! `WorkspaceSidebarViewDelegate` is implemented on the controller's
//! delegate proxy and forwards to the methods below.

use std::rc::{Rc, Weak};

use objc2::MainThreadMarker;
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2_foundation::{NSFileManager, NSString};
use upleft_core::NSRange;
use upleft_foundation::url::{FileUrl, current_directory_path};
use upleft_render::view::markdown_text_view_delegate::ScrollPosition;
use upleft_swift_text as swift;

use crate::app::document_window_controller::{DocumentWindowController, DocumentWindowControllerDelegates};
use crate::app::document_window_controller_asset_doctor::make_first_responder;
use crate::app::document_window_controller_command_palette::app_delegate;
use crate::panels::panel_chrome::panel_title;
use crate::panels::workspace_sidebar_view::{WorkspaceSidebarTab, WorkspaceSidebarView, WorkspaceSidebarViewDelegate};
use crate::support::commands::Command;
use crate::support::quick_open_providers::{
    QuickOpenAction, QuickOpenProvider, QuickOpenProviderKind, QuickOpenResult, WorkspaceQuickOpenProvider,
};
use crate::workspace::workspace_index::{WorkspaceIndex, WorkspaceIndexPolicy, WorkspaceIndexSnapshot};
use crate::workspace::workspace_link_graph::{WorkspaceLinkGraph, WorkspaceLinkGraphBuilder};
use crate::workspace::workspace_search::{WorkspaceSearchQuery, WorkspaceSearchResult, WorkspaceSearchSession};

/// The extension's associated-object state (`workspaceIndex`,
/// `workspaceSidebar`, `workspaceSearch`, `workspaceGraph`), held by the
/// controller as `workspace_state()`.
#[derive(Default)]
pub struct WorkspaceState {
    /// `workspaceIndex`.
    pub(crate) workspace_index: Option<Rc<WorkspaceIndex>>,
    /// `workspaceSidebar`.
    pub(crate) workspace_sidebar: Option<Retained<WorkspaceSidebarView>>,
    /// `workspaceSearch`.
    pub(crate) workspace_search: Option<Rc<WorkspaceSearchSession>>,
    /// `workspaceGraph` (`.empty` until a snapshot lands).
    pub(crate) workspace_graph: WorkspaceLinkGraph,
}

impl DocumentWindowController {
    fn workspace_index(&self) -> Option<Rc<WorkspaceIndex>> {
        self.workspace_state().borrow().workspace_index.clone()
    }

    fn set_workspace_index(&self, index: Option<Rc<WorkspaceIndex>>) {
        let previous = std::mem::replace(&mut self.workspace_state().borrow_mut().workspace_index, index);
        drop(previous);
    }

    fn workspace_sidebar(&self) -> Option<Retained<WorkspaceSidebarView>> {
        self.workspace_state().borrow().workspace_sidebar.clone()
    }

    fn set_workspace_sidebar(&self, sidebar: Option<Retained<WorkspaceSidebarView>>) {
        self.workspace_state().borrow_mut().workspace_sidebar = sidebar;
    }

    fn workspace_search(&self) -> Option<Rc<WorkspaceSearchSession>> {
        self.workspace_state().borrow().workspace_search.clone()
    }

    fn set_workspace_search(&self, search: Option<Rc<WorkspaceSearchSession>>) {
        let previous = std::mem::replace(&mut self.workspace_state().borrow_mut().workspace_search, search);
        drop(previous);
    }

    fn set_workspace_graph(&self, graph: WorkspaceLinkGraph) {
        self.workspace_state().borrow_mut().workspace_graph = graph;
    }

    /// `toggleWorkspaceSidebar()`.
    pub fn toggle_workspace_sidebar(&self) {
        if let Some(panel) = self.workspace_sidebar() {
            self.dismiss_trailing(&panel);
            self.set_workspace_sidebar(None);
            if let Some(index) = self.workspace_index() {
                index.cancel();
            }
            if let Some(search) = self.workspace_search() {
                search.cancel();
            }
            self.set_workspace_index(None);
            self.set_workspace_search(None);
            return;
        }
        let mtm = MainThreadMarker::from(self);
        let panel = WorkspaceSidebarView::new(self.active_style_sheet(), mtm);
        let delegates = self.delegates();
        panel.set_delegate(Some(Rc::downgrade(&delegates) as Weak<dyn WorkspaceSidebarViewDelegate>));
        self.set_workspace_sidebar(Some(panel.clone()));
        let index = Rc::new(WorkspaceIndex::new(WorkspaceIndexPolicy::default()));
        self.set_workspace_index(Some(index.clone()));
        let search = Rc::new(WorkspaceSearchSession::new());
        self.set_workspace_search(Some(search.clone()));
        let weak_self: ObjcWeak<DocumentWindowController> = ObjcWeak::from(self);
        let weak_panel: ObjcWeak<WorkspaceSidebarView> = ObjcWeak::from(&*panel);
        index.set_on_update(Some(Box::new(move |snapshot: &WorkspaceIndexSnapshot| {
            let (Some(this), Some(panel)) = (weak_self.load(), weak_panel.load()) else { return };
            panel.set_is_scanning(false);
            this.set_workspace_graph(WorkspaceLinkGraphBuilder::build(snapshot));
            panel.set_entries(snapshot.entries.clone());
            panel.set_search_results(Vec::new());
            let current_id = this.markdown_document().url().map(|url| url.standardized_file_url().path());
            panel.set_selected_file_id(current_id.clone());
            let backlinks = current_id.map(|id| this.workspace_state().borrow().workspace_graph.links_to(&id));
            panel.set_backlinks(backlinks.unwrap_or_default());
            let symbols: Vec<QuickOpenResult> = snapshot
                .entries
                .iter()
                .flat_map(|entry| {
                    entry.headings.iter().map(move |heading| {
                        QuickOpenResult::new(
                            format!("workspace-heading:{}:{}", entry.id, heading.range.location),
                            QuickOpenProviderKind::Symbol,
                            heading.title.clone(),
                            QuickOpenAction::OpenAt(entry.url.clone(), heading.range),
                        )
                        .with_subtitle(entry.relative_path.clone())
                    })
                })
                .collect();
            this.set_quick_open_providers(vec![Rc::new(WorkspaceQuickOpenProvider {
                files: snapshot.entries.iter().map(|entry| entry.url.clone()).collect(),
                symbols,
            }) as Rc<dyn QuickOpenProvider>]);
        })));
        let weak_panel: ObjcWeak<WorkspaceSidebarView> = ObjcWeak::from(&*panel);
        search.set_on_update(Some(Box::new(move |results: &[WorkspaceSearchResult]| {
            if let Some(panel) = weak_panel.load() {
                panel.set_search_results(results.to_vec());
            }
        })));
        self.install_trailing(&panel, Some(&panel_title(Command::Workspace)));
        let root = self
            .markdown_document()
            .url()
            .map(|url| url.deleting_last_path_component())
            .unwrap_or_else(|| FileUrl::from_path(&current_directory_path()));
        if !NSFileManager::defaultManager().isReadableFileAtPath(&NSString::from_str(&root.path())) {
            panel.set_is_scanning(false);
            panel.set_error_message(Some(
                "Upleft cannot read this folder. Check its permissions and try again.".to_owned(),
            ));
            return;
        }
        // The first scan can take a moment on a large folder; the panel says
        // so rather than showing an empty file list that reads as "no files".
        panel.set_is_scanning(true);
        index.start(&root);
    }

    /// `resetWorkspaceState(for:)`.
    pub fn reset_workspace_state(&self, document_url: &FileUrl) {
        if let Some(search) = self.workspace_search() {
            search.cancel();
        }
        self.set_workspace_graph(WorkspaceLinkGraph::empty());
        let Some(index) = self.workspace_index() else { return };

        if let Some(sidebar) = self.workspace_sidebar() {
            sidebar.set_entries(Vec::new());
        }
        if let Some(sidebar) = self.workspace_sidebar() {
            sidebar.set_search_results(Vec::new());
        }
        if let Some(sidebar) = self.workspace_sidebar() {
            sidebar.set_backlinks(Vec::new());
        }
        if let Some(sidebar) = self.workspace_sidebar() {
            sidebar.set_selected_file_id(Some(document_url.standardized_file_url().path()));
        }
        index.reroot(&document_url.deleting_last_path_component());
    }

    /// `workspaceSidebar(_:didSearch:)`.
    pub fn workspace_sidebar_did_search(&self, view: &WorkspaceSidebarView, query: &WorkspaceSearchQuery) {
        let Some(index) = self.workspace_index() else { return };
        view.set_selected_tab(WorkspaceSidebarTab::Search);
        if let Some(search) = self.workspace_search() {
            search.start(query.clone(), index.snapshot());
        }
    }

    /// `workspaceSidebar(_:didSelect:range:inNewWindow:)`.
    pub fn workspace_sidebar_did_select(
        &self,
        _view: &WorkspaceSidebarView,
        url: &FileUrl,
        range: Option<NSRange>,
        in_new_window: bool,
    ) {
        if in_new_window {
            if let Some(delegate) = app_delegate(MainThreadMarker::from(self)) {
                delegate.open_default(url);
            }
        } else {
            self.open_in_place(url);
        }
        let Some(range) = range else { return };
        let same_file = match self.markdown_document().url() {
            Some(current) => {
                swift::str_eq(&url.standardized_file_url().path(), &current.standardized_file_url().path())
            }
            None => false,
        };
        if !same_file {
            return;
        }
        self.container_text_view().set_source_selected_ranges(&[range]);
        self.container_text_view().scroll_to_offset(range.location, ScrollPosition::Visible, true);
        if let Some(window) = self.window() {
            make_first_responder(&window, &self.container_text_view());
        }
    }
}

// MARK: - WorkspaceSidebarViewDelegate

impl WorkspaceSidebarViewDelegate for DocumentWindowControllerDelegates {
    fn workspace_sidebar_did_select(
        &self,
        view: &WorkspaceSidebarView,
        url: &FileUrl,
        range: Option<NSRange>,
        in_new_window: bool,
    ) {
        if let Some(controller) = self.controller() {
            controller.workspace_sidebar_did_select(view, url, range, in_new_window);
        }
    }

    fn workspace_sidebar_did_search(&self, view: &WorkspaceSidebarView, query: &WorkspaceSearchQuery) {
        if let Some(controller) = self.controller() {
            controller.workspace_sidebar_did_search(view, query);
        }
    }
}
