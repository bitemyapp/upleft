//! Port of `Panels/WorkspaceSidebarView.swift`: the workspace panel (files,
//! search results and backlinks for the folder around the document).
//!
//! The panel is its table's data source and delegate and its search field's
//! delegate, as in Swift; the private `WorkspaceSidebarRowView` is a
//! `define_class!` `NSView` subclass with the same Objective-C name.
//!
//! Files are grouped by `URL(fileURLWithPath: relativePath)
//! .deletingLastPathComponent().path`, which resolves the relative path
//! against the process's working directory (reproduced with `FileUrl`):
//! in an app launched from Finder that is `/`, so root-level files group
//! under "/" and the `"."` → "This folder" branch never matches.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSColor, NSControlTextEditingDelegate, NSEventModifierFlags, NSLayoutConstraintOrientation,
    NSLayoutPriorityDefaultLow, NSLineBreakMode, NSResponder, NSScrollView, NSSearchField, NSSearchFieldDelegate,
    NSTableColumn, NSTableRowView, NSTableView, NSTableViewDataSource, NSTableViewDelegate, NSTextAlignment,
    NSTextField, NSTextFieldDelegate, NSUserInterfaceItemIdentification, NSView,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSRect, NSString};
use upleft_core::NSRange;
use upleft_foundation::url::FileUrl;
use upleft_render::theme::style_sheet::StyleSheet;

use super::activity_indicator_view::ActivityIndicatorView;
use super::appkit_support::{activate, label, ns_string, object, role, set_label, set_role, set_value};
use super::panel_chrome::{
    PanelBackdrop, PanelEmptyStateView, PanelFont, PanelGroupRowView, PanelList, PanelMetrics, PanelSegmentedControl,
    PanelSurface, PanelTableView, element_at, install_backdrop, panel_title,
};
use crate::support::commands::Command;
use crate::workspace::workspace_index::WorkspaceIndexEntry;
use crate::workspace::workspace_link_graph::WorkspaceBacklink;
use crate::workspace::workspace_search::{WorkspaceSearchQuery, WorkspaceSearchResult};

/// `WorkspaceSidebarViewDelegate`.
pub trait WorkspaceSidebarViewDelegate {
    fn workspace_sidebar_did_select(
        &self,
        view: &WorkspaceSidebarView,
        url: &FileUrl,
        range: Option<NSRange>,
        in_new_window: bool,
    );
    fn workspace_sidebar_did_search(&self, view: &WorkspaceSidebarView, query: &WorkspaceSearchQuery);
}

/// `WorkspaceSidebarView.WorkspaceSidebarTab`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WorkspaceSidebarTab {
    Files,
    Search,
    Backlinks,
}

impl WorkspaceSidebarTab {
    pub const ALL_CASES: [WorkspaceSidebarTab; 3] =
        [WorkspaceSidebarTab::Files, WorkspaceSidebarTab::Search, WorkspaceSidebarTab::Backlinks];

    pub fn raw_value(self) -> &'static str {
        match self {
            WorkspaceSidebarTab::Files => "Files",
            WorkspaceSidebarTab::Search => "Search",
            WorkspaceSidebarTab::Backlinks => "Backlinks",
        }
    }
}

/// `WorkspaceSidebarView.Row` (private).
#[derive(Clone, Debug, PartialEq)]
enum Row {
    Group(String),
    File(usize),
    Result(usize),
    Backlink(usize),
}

pub struct WorkspaceSidebarViewIvars {
    delegate: RefCell<Option<Weak<dyn WorkspaceSidebarViewDelegate>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    entries: RefCell<Vec<WorkspaceIndexEntry>>,
    search_results: RefCell<Vec<WorkspaceSearchResult>>,
    backlinks: RefCell<Vec<WorkspaceBacklink>>,
    selected_file_id: RefCell<Option<String>>,
    selected_tab: Cell<WorkspaceSidebarTab>,
    /// Set while the host is scanning the folder for the first time.  A
    /// folder scan is work in progress, not an empty folder.
    is_scanning: Cell<bool>,
    error_message: RefCell<Option<String>>,
    backdrop: Retained<PanelBackdrop>,
    title_label: Retained<NSTextField>,
    count_label: Retained<NSTextField>,
    tab_control: Retained<PanelSegmentedControl>,
    search_field: Retained<NSSearchField>,
    spinner: Retained<ActivityIndicatorView>,
    empty_state: Retained<PanelEmptyStateView>,
    table: Retained<PanelTableView>,
    /// Swift's `lazy var scroll`.
    scroll: RefCell<Option<Retained<NSScrollView>>>,
    rows: RefCell<Vec<Row>>,
    /// True once the host has handed over a file list, so "no files" can be
    /// told apart from "not scanned yet".
    has_scanned: Cell<bool>,
    is_searching: Cell<bool>,
}

define_class!(
    /// `WorkspaceSidebarView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "WorkspaceSidebarView"]
    #[ivars = WorkspaceSidebarViewIvars]
    pub struct WorkspaceSidebarView;

    unsafe impl NSObjectProtocol for WorkspaceSidebarView {}

    impl WorkspaceSidebarView {
        /// `PanelSurface.preferredWidth`.
        #[unsafe(method(preferredWidth))]
        fn __preferred_width(&self) -> CGFloat {
            self.preferred_width()
        }

        #[unsafe(method(searchChanged:))]
        fn __search_changed(&self, _sender: Option<&AnyObject>) {
            self.search_changed();
        }

        #[unsafe(method(rowClicked:))]
        fn __row_clicked(&self, _sender: Option<&AnyObject>) {
            self.activate_selection();
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.apply_style();
        }
    }

    unsafe impl NSControlTextEditingDelegate for WorkspaceSidebarView {}

    unsafe impl NSTextFieldDelegate for WorkspaceSidebarView {}

    unsafe impl NSSearchFieldDelegate for WorkspaceSidebarView {}

    unsafe impl NSTableViewDataSource for WorkspaceSidebarView {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn __number_of_rows(&self, _table_view: &NSTableView) -> isize {
            self.ivars().rows.borrow().len() as isize
        }
    }

    unsafe impl NSTableViewDelegate for WorkspaceSidebarView {
        #[unsafe(method(tableView:heightOfRow:))]
        fn __height_of_row(&self, _table_view: &NSTableView, row: isize) -> CGFloat {
            self.height_of_row(row)
        }

        #[unsafe(method_id(tableView:rowViewForRow:))]
        fn __row_view_for_row(&self, table_view: &NSTableView, _row: isize) -> Option<Retained<NSTableRowView>> {
            let row = PanelList::selection_row(table_view, Some(object(self)), self.style_sheet(), self.mtm());
            Some(Retained::into_super(row))
        }

        #[unsafe(method(tableView:isGroupRow:))]
        fn __is_group_row(&self, _table_view: &NSTableView, row: isize) -> bool {
            self.is_group_row(row)
        }

        #[unsafe(method(tableView:shouldSelectRow:))]
        fn __should_select_row(&self, _table_view: &NSTableView, row: isize) -> bool {
            self.should_select_row(row)
        }

        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn __view_for(
            &self,
            table_view: &NSTableView,
            _table_column: Option<&NSTableColumn>,
            row: isize,
        ) -> Option<Retained<NSView>> {
            self.view_for(table_view, row)
        }
    }
);

impl PanelSurface for WorkspaceSidebarView {
    fn preferred_width(&self) -> CGFloat {
        PanelMetrics::LIST_WIDTH
    }
}

impl WorkspaceSidebarView {
    /// `WorkspaceSidebarView()`.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<WorkspaceSidebarView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<WorkspaceSidebarView> {
        let title_label = label(&panel_title(Command::Workspace), mtm);
        let count_label = label("", mtm);
        let search_field = NSSearchField::new(mtm);
        let spinner = ActivityIndicatorView::new(mtm);
        let empty_state = PanelEmptyStateView::new(mtm);
        let table = PanelList::make_table_view("workspaceSidebar", mtm);
        let backdrop = PanelBackdrop::new_default(style_sheet.clone(), mtm);
        let items: Vec<&str> = WorkspaceSidebarTab::ALL_CASES.iter().map(|tab| tab.raw_value()).collect();
        let tab_control = PanelSegmentedControl::new(&items, 0, style_sheet.clone(), mtm);
        let this = Self::alloc(mtm).set_ivars(WorkspaceSidebarViewIvars {
            delegate: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            entries: RefCell::new(Vec::new()),
            search_results: RefCell::new(Vec::new()),
            backlinks: RefCell::new(Vec::new()),
            selected_file_id: RefCell::new(None),
            selected_tab: Cell::new(WorkspaceSidebarTab::Files),
            is_scanning: Cell::new(false),
            error_message: RefCell::new(None),
            backdrop: backdrop.clone(),
            title_label: title_label.clone(),
            count_label: count_label.clone(),
            tab_control: tab_control.clone(),
            search_field: search_field.clone(),
            spinner: spinner.clone(),
            empty_state: empty_state.clone(),
            table: table.clone(),
            scroll: RefCell::new(None),
            rows: RefCell::new(Vec::new()),
            has_scanned: Cell::new(false),
            is_searching: Cell::new(false),
        });
        let this: Retained<WorkspaceSidebarView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };

        install_backdrop(&this, &backdrop);
        title_label.setFont(Some(&PanelFont::header()));
        title_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        title_label.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );
        this.addSubview(&title_label);
        count_label.setFont(Some(&PanelFont::secondary()));
        count_label.setAlignment(NSTextAlignment::Right);
        count_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        count_label.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow - 1.0,
            NSLayoutConstraintOrientation::Horizontal,
        );
        count_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&count_label);

        spinner.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&spinner);

        // Sized from its labels and free to compress, so the tab strip cannot
        // out-measure a narrow inspector the way three 76pt segments did.
        let weak: ObjcWeak<WorkspaceSidebarView> = ObjcWeak::from(&*this);
        tab_control.set_on_change(Some(Rc::new(move |index: isize| {
            let Some(tab) = element_at(&WorkspaceSidebarTab::ALL_CASES, index).copied() else { return };
            if let Some(this) = weak.load() {
                this.set_selected_tab(tab);
            }
        })));
        set_label(&*tab_control, "Workspace sections");
        tab_control.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );
        tab_control.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&tab_control);

        search_field.setPlaceholderString(Some(&ns_string("Search workspace")));
        search_field.setFont(Some(&PanelFont::row()));
        unsafe {
            // SAFETY: the field is the panel's own subview; its delegate is
            // weak, as in Swift.
            search_field.setDelegate(Some(ProtocolObject::from_ref(&*this)));
            search_field.setTarget(Some(object(&*this)));
            search_field.setAction(Some(sel!(searchChanged:)));
        }
        set_label(&*search_field, "Search workspace files");
        search_field.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&search_field);

        // SAFETY: the table is the panel's own subview and never outlives it
        // (the data source and delegate are weak in AppKit, as in Swift).
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(&*this)));
            table.setDelegate(Some(ProtocolObject::from_ref(&*this)));
            table.setTarget(Some(object(&*this)));
            table.setAction(Some(sel!(rowClicked:)));
        }
        let weak: ObjcWeak<WorkspaceSidebarView> = ObjcWeak::from(&*this);
        table.set_on_activate(Some(Rc::new(move || {
            if let Some(this) = weak.load() {
                this.activate_selection();
            }
        })));
        set_label(&*table, "Workspace results");
        let scroll = this.scroll(mtm);
        this.addSubview(&scroll);
        empty_state.install(&this, &scroll, 1.0);

        activate(&[
            title_label.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), PanelMetrics::INSET),
            title_label
                .topAnchor()
                .constraintEqualToAnchor_constant(&this.topAnchor(), PanelMetrics::HEADER_TOP_PADDING),
            spinner.leadingAnchor().constraintEqualToAnchor_constant(&title_label.trailingAnchor(), 6.0),
            spinner.centerYAnchor().constraintEqualToAnchor(&title_label.centerYAnchor()),
            spinner.widthAnchor().constraintEqualToConstant(14.0),
            spinner.heightAnchor().constraintEqualToConstant(14.0),
            count_label.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -PanelMetrics::INSET),
            count_label.centerYAnchor().constraintEqualToAnchor(&title_label.centerYAnchor()),
            count_label.leadingAnchor().constraintGreaterThanOrEqualToAnchor_constant(&spinner.trailingAnchor(), 6.0),
            tab_control.leadingAnchor().constraintEqualToAnchor(&title_label.leadingAnchor()),
            tab_control
                .trailingAnchor()
                .constraintLessThanOrEqualToAnchor_constant(&this.trailingAnchor(), -PanelMetrics::INSET),
            tab_control.topAnchor().constraintEqualToAnchor_constant(&title_label.bottomAnchor(), 6.0),
            tab_control.heightAnchor().constraintEqualToConstant(PanelSegmentedControl::CONTROL_HEIGHT),
            search_field.leadingAnchor().constraintEqualToAnchor(&title_label.leadingAnchor()),
            search_field.trailingAnchor().constraintEqualToAnchor(&count_label.trailingAnchor()),
            search_field.topAnchor().constraintEqualToAnchor_constant(&tab_control.bottomAnchor(), 5.0),
            scroll.leadingAnchor().constraintEqualToAnchor(&this.leadingAnchor()),
            scroll.trailingAnchor().constraintEqualToAnchor(&this.trailingAnchor()),
            scroll.topAnchor().constraintEqualToAnchor_constant(&search_field.bottomAnchor(), 6.0),
            scroll.bottomAnchor().constraintEqualToAnchor(&this.bottomAnchor()),
        ]);

        set_role(&*this, role::group());
        set_label(&*this, "Workspace");
        this.sync_tab();
        this.apply_style();
        this.reload();
        this
    }

    /// `lazy var scroll = PanelList.makeScrollView(documentView: table)`.
    fn scroll(&self, mtm: MainThreadMarker) -> Retained<NSScrollView> {
        if let Some(scroll) = self.ivars().scroll.borrow().as_ref() {
            return scroll.clone();
        }
        let scroll = PanelList::make_scroll_view(&self.ivars().table, mtm);
        *self.ivars().scroll.borrow_mut() = Some(scroll.clone());
        scroll
    }

    // MARK: - Properties

    pub fn delegate(&self) -> Option<Rc<dyn WorkspaceSidebarViewDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn WorkspaceSidebarViewDelegate>>) {
        *self.ivars().delegate.borrow_mut() = delegate;
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet.clone();
        self.ivars().backdrop.set_style_sheet(style_sheet);
        self.apply_style();
    }

    pub fn entries(&self) -> Vec<WorkspaceIndexEntry> {
        self.ivars().entries.borrow().clone()
    }

    pub fn set_entries(&self, entries: Vec<WorkspaceIndexEntry>) {
        *self.ivars().entries.borrow_mut() = entries;
        self.ivars().has_scanned.set(true);
        self.reload();
    }

    pub fn search_results(&self) -> Vec<WorkspaceSearchResult> {
        self.ivars().search_results.borrow().clone()
    }

    /// Results arriving is what ends a search.  Until they do the panel must
    /// not claim there are none (§11.4).
    pub fn set_search_results(&self, results: Vec<WorkspaceSearchResult>) {
        *self.ivars().search_results.borrow_mut() = results;
        self.ivars().is_searching.set(false);
        self.reload();
    }

    pub fn backlinks(&self) -> Vec<WorkspaceBacklink> {
        self.ivars().backlinks.borrow().clone()
    }

    pub fn set_backlinks(&self, backlinks: Vec<WorkspaceBacklink>) {
        *self.ivars().backlinks.borrow_mut() = backlinks;
        self.reload();
    }

    pub fn selected_file_id(&self) -> Option<String> {
        self.ivars().selected_file_id.borrow().clone()
    }

    pub fn set_selected_file_id(&self, id: Option<String>) {
        *self.ivars().selected_file_id.borrow_mut() = id;
        self.reload();
    }

    pub fn selected_tab(&self) -> WorkspaceSidebarTab {
        self.ivars().selected_tab.get()
    }

    pub fn set_selected_tab(&self, tab: WorkspaceSidebarTab) {
        let old = self.ivars().selected_tab.replace(tab);
        if tab == old {
            return;
        }
        self.sync_tab();
        self.reload();
    }

    pub fn is_scanning(&self) -> bool {
        self.ivars().is_scanning.get()
    }

    pub fn set_is_scanning(&self, scanning: bool) {
        self.ivars().is_scanning.set(scanning);
        self.reload();
    }

    pub fn error_message(&self) -> Option<String> {
        self.ivars().error_message.borrow().clone()
    }

    pub fn set_error_message(&self, message: Option<String>) {
        let changed = {
            let old = self.ivars().error_message.borrow();
            match (&*old, &message) {
                (Some(old), Some(new)) => !upleft_swift_text::str_eq(old, new),
                (None, None) => false,
                _ => true,
            }
        };
        *self.ivars().error_message.borrow_mut() = message;
        if changed {
            self.reload();
        }
    }

    pub fn set_search_text_for_testing(&self, text: &str) {
        self.ivars().search_field.setStringValue(&ns_string(text));
        self.search_changed();
    }

    // MARK: - State

    fn sync_tab(&self) {
        let tab = self.selected_tab();
        let Some(index) = WorkspaceSidebarTab::ALL_CASES.iter().position(|candidate| *candidate == tab) else { return };
        self.ivars().tab_control.set_selected_index(index as isize, true);
        self.ivars().search_field.setHidden(tab == WorkspaceSidebarTab::Backlinks);
    }

    /// True while the tab on screen is still waiting for its content.
    /// Nothing may say "none" until this is false.
    fn is_busy(&self) -> bool {
        let ivars = self.ivars();
        match self.selected_tab() {
            WorkspaceSidebarTab::Files => {
                ivars.error_message.borrow().is_none() && (ivars.is_scanning.get() || !ivars.has_scanned.get())
            }
            WorkspaceSidebarTab::Search => ivars.is_searching.get(),
            WorkspaceSidebarTab::Backlinks => false,
        }
    }

    fn search_text(&self) -> String {
        self.ivars().search_field.stringValue().to_string()
    }

    fn reload(&self) {
        let ivars = self.ivars();
        ivars.rows.borrow_mut().clear();
        let count_text = match self.selected_tab() {
            WorkspaceSidebarTab::Files => {
                let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
                let mut order: Vec<String> = Vec::new();
                let count = {
                    let entries = ivars.entries.borrow();
                    for (index, entry) in entries.iter().enumerate() {
                        let folder = FileUrl::from_path(&entry.relative_path).deleting_last_path_component().path();
                        if upleft_swift_text::dict_get(&groups, &folder).is_none() {
                            order.push(folder.clone());
                        }
                        let mut indices = upleft_swift_text::dict_get(&groups, &folder).cloned().unwrap_or_default();
                        indices.push(index);
                        upleft_swift_text::dict_insert(&mut groups, folder, indices);
                    }
                    entries.len()
                };
                {
                    let mut rows = ivars.rows.borrow_mut();
                    for folder in &order {
                        if order.len() > 1 {
                            rows.push(Row::Group(if folder == "." { "This folder".to_owned() } else { folder.clone() }));
                        }
                        let indices = upleft_swift_text::dict_get(&groups, folder).cloned().unwrap_or_default();
                        rows.extend(indices.into_iter().map(Row::File));
                    }
                }
                if self.is_busy() {
                    "Scanning…".to_owned()
                } else {
                    format!("{count} file{}", if count == 1 { "" } else { "s" })
                }
            }
            WorkspaceSidebarTab::Search => {
                let count = ivars.search_results.borrow().len();
                *ivars.rows.borrow_mut() = (0..count).map(Row::Result).collect();
                if self.is_busy() {
                    "Searching…".to_owned()
                } else if count == 0 {
                    "No matches".to_owned()
                } else {
                    format!("{count} match{}", if count == 1 { "" } else { "es" })
                }
            }
            WorkspaceSidebarTab::Backlinks => {
                let count = ivars.backlinks.borrow().len();
                *ivars.rows.borrow_mut() = (0..count).map(Row::Backlink).collect();
                if count == 0 {
                    "No backlinks".to_owned()
                } else {
                    format!("{count} backlink{}", if count == 1 { "" } else { "s" })
                }
            }
        };
        ivars.count_label.setStringValue(&ns_string(&count_text));
        set_label(&*ivars.count_label, &ivars.count_label.stringValue().to_string());
        if self.is_busy() {
            ivars.spinner.begin();
        } else {
            ivars.spinner.end();
        }
        ivars.table.reloadData();
        self.update_empty_state();
        set_value(self, &ivars.count_label.stringValue().to_string());
    }

    fn update_empty_state(&self) {
        let ivars = self.ivars();
        let scroll = ivars.scroll.borrow().clone();
        if !ivars.rows.borrow().is_empty() {
            ivars.empty_state.setHidden(true);
            if let Some(scroll) = &scroll {
                scroll.setHidden(false);
            }
            return;
        }
        let error_message = ivars.error_message.borrow().clone();
        let (symbol, title, subtitle): (&str, &str, String) = if let Some(error_message) = error_message {
            ("exclamationmark.triangle", "Workspace unavailable", error_message)
        } else {
            match (self.selected_tab(), self.is_busy()) {
                (WorkspaceSidebarTab::Files, true) => {
                    ("folder", "Scanning the folder", "Listing the Markdown files\nnext to this document.".to_owned())
                }
                (WorkspaceSidebarTab::Files, false) => {
                    ("folder", "No Markdown files", "This workspace folder has no\nother Markdown documents.".to_owned())
                }
                (WorkspaceSidebarTab::Search, true) => {
                    ("magnifyingglass", "Searching", "Looking through the workspace\nfiles for that text.".to_owned())
                }
                (WorkspaceSidebarTab::Search, false) if self.search_text().is_empty() => (
                    "magnifyingglass",
                    "Search the workspace",
                    "Type above to search every\nMarkdown file in this folder.".to_owned(),
                ),
                (WorkspaceSidebarTab::Search, false) => (
                    "magnifyingglass",
                    "No matches",
                    format!("Nothing in this workspace\ncontains “{}”.", self.search_text()),
                ),
                (WorkspaceSidebarTab::Backlinks, _) => {
                    ("arrow.triangle.branch", "No backlinks", "No workspace file links to\nthis document yet.".to_owned())
                }
            }
        };
        ivars.empty_state.configure(symbol, title, &subtitle, &self.style_sheet());
        ivars.empty_state.setHidden(false);
        if let Some(scroll) = &scroll {
            scroll.setHidden(true);
        }
    }

    fn search_changed(&self) {
        let text = self.search_text();
        if !(self.selected_tab() == WorkspaceSidebarTab::Search || !text.is_empty()) {
            return;
        }
        self.ivars().is_searching.set(!text.is_empty());
        self.set_selected_tab(WorkspaceSidebarTab::Search);
        self.reload();
        if let Some(delegate) = self.delegate() {
            delegate.workspace_sidebar_did_search(self, &WorkspaceSearchQuery::new(self.search_text()));
        }
    }

    fn activate_selection(&self) {
        let ivars = self.ivars();
        let clicked = ivars.table.clickedRow();
        let row = if clicked >= 0 { clicked } else { ivars.table.selectedRow() };
        let Some(target) = ({
            let rows = ivars.rows.borrow();
            if row >= 0 && (row as usize) < rows.len() { Some(rows[row as usize].clone()) } else { None }
        }) else {
            return;
        };
        let in_new_window = NSApplication::sharedApplication(self.mtm())
            .currentEvent()
            .is_some_and(|event| event.modifierFlags().contains(NSEventModifierFlags::Command));
        let selection: Option<(FileUrl, Option<NSRange>)> = match target {
            Row::Group(_) => return,
            Row::File(index) => ivars.entries.borrow().get(index).map(|entry| (entry.url.clone(), None)),
            Row::Result(index) => {
                ivars.search_results.borrow().get(index).map(|result| (result.url.clone(), Some(result.range)))
            }
            Row::Backlink(index) => ivars
                .backlinks
                .borrow()
                .get(index)
                .map(|backlink| (FileUrl::from_path(&backlink.source_file), Some(backlink.source_range))),
        };
        let Some((url, range)) = selection else { return };
        if let Some(delegate) = self.delegate() {
            delegate.workspace_sidebar_did_select(self, &url, range, in_new_window);
        }
    }

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        ivars.title_label.setTextColor(Some(&style_sheet.text_secondary));
        ivars.count_label.setTextColor(Some(&style_sheet.text_faint));
        ivars.tab_control.set_style_sheet(style_sheet);
        ivars.table.reloadData();
        self.update_empty_state();
    }

    // MARK: - Table

    fn row(&self, row: isize) -> Option<Row> {
        let rows = self.ivars().rows.borrow();
        // Swift guards `row < rows.count` only; a negative row traps there
        // and panics here.
        if !(row < rows.len() as isize) {
            return None;
        }
        Some(rows[row as usize].clone())
    }

    fn height_of_row(&self, row: isize) -> CGFloat {
        match self.row(row) {
            None => PanelMetrics::DETAIL_ROW_HEIGHT,
            Some(Row::Group(_)) => PanelMetrics::GROUP_ROW_HEIGHT,
            Some(_) => PanelMetrics::DETAIL_ROW_HEIGHT,
        }
    }

    fn is_group_row(&self, row: isize) -> bool {
        matches!(self.row(row), Some(Row::Group(_)))
    }

    fn should_select_row(&self, row: isize) -> bool {
        match self.row(row) {
            None => false,
            Some(Row::Group(_)) => false,
            Some(_) => true,
        }
    }

    fn view_for(&self, table_view: &NSTableView, row: isize) -> Option<Retained<NSView>> {
        let row = self.row(row)?;
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        match row {
            Row::Group(title) => {
                let id = NSString::from_str("workspaceGroup");
                let reused = unsafe { table_view.makeViewWithIdentifier_owner(&id, Some(object(self))) }
                    .and_then(|view| view.downcast::<PanelGroupRowView>().ok());
                let cell = reused.unwrap_or_else(|| PanelGroupRowView::new(&id, self.mtm()));
                cell.configure(&upleft_swift_text::uppercased(&title), &style_sheet.text_faint);
                Some(Retained::into_super(cell))
            }
            Row::File(index) => {
                let entry = ivars.entries.borrow().get(index).cloned()?;
                Some(self.make_row(
                    table_view,
                    "workspaceFile",
                    &entry.relative_path,
                    &Self::heading_summary(&entry),
                    &style_sheet.text_secondary,
                ))
            }
            Row::Result(index) => {
                let result = ivars.search_results.borrow().get(index).cloned()?;
                Some(self.make_row(
                    table_view,
                    "workspaceSearch",
                    &result.relative_path,
                    &format!("Line {} · {}", result.line, result.context_text),
                    &style_sheet.text_secondary,
                ))
            }
            Row::Backlink(index) => {
                let backlink = ivars.backlinks.borrow().get(index).cloned()?;
                Some(self.make_row(
                    table_view,
                    "workspaceBacklink",
                    &FileUrl::from_path(&backlink.source_file).last_path_component(),
                    &backlink.destination,
                    &style_sheet.text_secondary,
                ))
            }
        }
    }

    fn heading_summary(entry: &WorkspaceIndexEntry) -> String {
        match entry.headings.first() {
            Some(heading) => heading.title.clone(),
            None => "Markdown file".to_owned(),
        }
    }

    /// Reuse rather than a fresh cell per `viewFor`: this table reloads on
    /// every keystroke in the search field.
    fn make_row(&self, table_view: &NSTableView, id: &str, title: &str, detail: &str, color: &NSColor) -> Retained<NSView> {
        let identifier = NSString::from_str(id);
        let reused = unsafe { table_view.makeViewWithIdentifier_owner(&identifier, Some(object(self))) }
            .and_then(|view| view.downcast::<WorkspaceSidebarRowView>().ok());
        let view = reused.unwrap_or_else(|| WorkspaceSidebarRowView::new(&identifier, self.mtm()));
        view.configure(title, detail, color);
        Retained::into_super(view)
    }

    /// The table (tests and the conformance scene select and click rows).
    pub fn table_for_testing(&self) -> Retained<PanelTableView> {
        self.ivars().table.clone()
    }

    /// `numberOfRows(in:)`, as the Swift test calls it.
    pub fn number_of_rows_for_testing(&self) -> isize {
        self.ivars().rows.borrow().len() as isize
    }

    /// `tableView(_:viewFor:row:)`, as the Swift test calls it.
    pub fn view_for_row_for_testing(&self, table_view: &NSTableView, row: isize) -> Option<Retained<NSView>> {
        self.view_for(table_view, row)
    }

    pub fn spinner_for_testing(&self) -> Retained<ActivityIndicatorView> {
        self.ivars().spinner.clone()
    }
}

// MARK: - WorkspaceSidebarRowView

pub struct WorkspaceSidebarRowViewIvars {
    title_label: Retained<NSTextField>,
    detail_label: Retained<NSTextField>,
}

define_class!(
    /// `WorkspaceSidebarRowView` (private in Swift).
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "WorkspaceSidebarRowView"]
    #[ivars = WorkspaceSidebarRowViewIvars]
    pub struct WorkspaceSidebarRowView;

    unsafe impl NSObjectProtocol for WorkspaceSidebarRowView {}
);

impl WorkspaceSidebarRowView {
    /// `init(identifier:)`.
    fn new(identifier: &NSString, mtm: MainThreadMarker) -> Retained<WorkspaceSidebarRowView> {
        let title_label = label("", mtm);
        let detail_label = label("", mtm);
        let this = Self::alloc(mtm)
            .set_ivars(WorkspaceSidebarRowViewIvars { title_label: title_label.clone(), detail_label: detail_label.clone() });
        let this: Retained<WorkspaceSidebarRowView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setIdentifier(Some(identifier));
        title_label.setFont(Some(&PanelFont::row()));
        detail_label.setFont(Some(&PanelFont::secondary()));
        for field in [&title_label, &detail_label] {
            field.setTranslatesAutoresizingMaskIntoConstraints(false);
            field.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
            this.addSubview(field);
        }
        activate(&[
            title_label.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), PanelMetrics::INSET),
            title_label.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -PanelMetrics::INSET),
            title_label.topAnchor().constraintEqualToAnchor_constant(&this.topAnchor(), 5.0),
            detail_label.leadingAnchor().constraintEqualToAnchor(&title_label.leadingAnchor()),
            detail_label.trailingAnchor().constraintEqualToAnchor(&title_label.trailingAnchor()),
            detail_label.topAnchor().constraintEqualToAnchor_constant(&title_label.bottomAnchor(), 1.0),
        ]);
        set_role(&*this, role::button());
        this
    }

    fn configure(&self, title: &str, detail: &str, color: &NSColor) {
        let ivars = self.ivars();
        ivars.title_label.setStringValue(&ns_string(title));
        ivars.detail_label.setStringValue(&ns_string(detail));
        ivars.title_label.setTextColor(Some(color));
        ivars.detail_label.setTextColor(Some(&color.colorWithAlphaComponent(0.7)));
        set_label(self, &format!("{title}. {detail}"));
    }
}
