//! View-level tests for ReviewPanelView, WorkspaceSidebarView,
//! VersionTimelineView, HistoryInspectorView, TrustPromptView,
//! ReaderProfilePickerView, LocalAIPanelView, VisualDebuggerView (ported from
//! DownrightAppTests). Runs on the main thread; no window is created.
//!
//! Ported, with the Swift names:
//! - `ReaderProfileTests`: `pickerListsBuiltInsAndSendsLivePreview`,
//!   `savingCustomProfileUsesInjectedStore`;
//! - `TrustPromptViewTests`: `promptReportsExactTypedDecisionAndVoiceOverLabels`,
//!   `promptNamesTheFileItsGrantWillPersist`;
//! - `VisualDebuggerViewTests.exposesReadOnlySummaryAndCopiesIt` (into a
//!   named pasteboard, never the general one);
//! - `WorkspaceTests.sidebarShowsTreeSearchAndAccessibleRows`.
//!
//! Skipped: `TrustPromptViewTests.linkClassificationSeparatesWebFilesAndAutomationSchemes`
//! tests `MarkdownLinkDestination` (`App/DocumentWindowController+Delegates.swift`,
//! not a panel). `PanelAccessibilityTests` covers none of these panels.
//!
//! Sandbox: every panel reads `PanelFont`, which reads `Preferences.shared`;
//! its load publishes the Quick Look appearance to the global preferences
//! domain, which `CFFIXED_USER_HOME` does not redirect. So `main` installs a
//! `Preferences::for_testing` instance as the shared one before anything
//! reads it, points `DOWNRIGHT_SUPPORT_DIRECTORY` at a temporary folder
//! (`tests/document_support/mod.rs`), and points `HOME` and
//! `CFFIXED_USER_HOME` into it (`ThemeStore.shared`, which every
//! `StyleSheet` reads, creates its user-themes folder under Application
//! Support).

#[path = "document_support/mod.rs"]
mod document_support;
#[path = "main_thread/mod.rs"]
mod main_thread;

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString, NSTableColumn, NSTableView, NSView};
use objc2_foundation::NSString;
use upleft_app::debugging::visual_debugger_model::{VisualDebuggerInput, VisualDebuggerModel};
use upleft_app::panels::appkit_support::accessibility_label;
use upleft_app::panels::reader_profile_picker_view::{ReaderProfilePickerDelegate, ReaderProfilePickerView};
use upleft_app::panels::trust_prompt_view::{TrustPromptDecision, TrustPromptView, TrustPromptViewDelegate};
use upleft_app::panels::visual_debugger_view::VisualDebuggerView;
use upleft_app::panels::workspace_sidebar_view::{
    WorkspaceSidebarTab, WorkspaceSidebarView, WorkspaceSidebarViewDelegate,
};
use upleft_app::security::document_trust::{TrustEffect, TrustRequest, TrustTarget};
use upleft_app::support::preferences::Preferences;
use upleft_app::support::reader_profiles::{InMemoryReaderProfileStore, ReaderProfile, ReaderProfileID};
use upleft_app::workspace::workspace_index::WorkspaceIndexEntry;
use upleft_app::workspace::workspace_search::WorkspaceSearchQuery;
use upleft_core::NSRange;
use upleft_core::parser::MarkdownParser;
use upleft_foundation::url::FileUrl;
use upleft_render::render_contracts::RenderMode;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("main-thread tests run on the main thread")
}

/// `StyleSheet.current`.
fn current(mtm: MainThreadMarker) -> Rc<StyleSheet> {
    Rc::new(StyleSheet::current(mtm))
}

// MARK: - ReaderProfileTests

#[derive(Default)]
struct ReaderProfileDelegateSpy {
    previewed: RefCell<Vec<ReaderProfile>>,
}

impl ReaderProfilePickerDelegate for ReaderProfileDelegateSpy {
    fn reader_profile_picker_did_preview(&self, _picker: &ReaderProfilePickerView, profile: &ReaderProfile) {
        self.previewed.borrow_mut().push(profile.clone());
    }

    fn reader_profile_picker_did_select(&self, _picker: &ReaderProfilePickerView, _profile: &ReaderProfile) {}
}

fn picker_lists_built_ins_and_sends_live_preview() {
    let mtm = mtm();
    let store = Rc::new(InMemoryReaderProfileStore::default());
    let picker = ReaderProfilePickerView::new(store, current(mtm), mtm);
    let delegate = Rc::new(ReaderProfileDelegateSpy::default());
    let weak: std::rc::Weak<dyn ReaderProfilePickerDelegate> =
        Rc::downgrade(&(delegate.clone() as Rc<dyn ReaderProfilePickerDelegate>));
    picker.set_delegate(Some(weak));

    picker.select_profile(ReaderProfileID::Presentation.raw_value());

    assert_eq!(picker.profiles().len(), 6);
    assert_eq!(picker.selected_profile().name, "Presentation");
    assert_eq!(
        delegate.previewed.borrow().last().map(|profile| profile.id.clone()).as_deref(),
        Some(ReaderProfileID::Presentation.raw_value())
    );
}

fn saving_custom_profile_uses_injected_store() {
    let mtm = mtm();
    let store = Rc::new(InMemoryReaderProfileStore::default());
    let picker = ReaderProfilePickerView::new(store.clone(), current(mtm), mtm);
    picker.save_custom_profile_for_testing("Team");

    let profiles = store.profiles.lock().unwrap().clone();
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].name, "Team");
    assert!(!profiles[0].is_built_in);
}

// MARK: - TrustPromptViewTests

#[derive(Default)]
struct RecordingTrustDelegate {
    decision: RefCell<Option<TrustPromptDecision>>,
    request: RefCell<Option<TrustRequest>>,
}

impl TrustPromptViewDelegate for RecordingTrustDelegate {
    fn trust_prompt_did_choose(&self, _view: &TrustPromptView, decision: TrustPromptDecision, request: &TrustRequest) {
        *self.decision.borrow_mut() = Some(decision);
        *self.request.borrow_mut() = Some(request.clone());
    }
}

fn prompt_reports_exact_typed_decision_and_voice_over_labels() {
    let view = TrustPromptView::new_current(mtm());
    let request = TrustRequest::new(
        TrustEffect::LaunchPathOrEditor,
        TrustTarget::new("/workspace/scripts/build.sh", Some("/workspace/scripts/build.sh"), None),
        Some(&FileUrl::from_path("/workspace/README.md")),
    );
    let delegate = Rc::new(RecordingTrustDelegate::default());
    let weak: std::rc::Weak<dyn TrustPromptViewDelegate> =
        Rc::downgrade(&(delegate.clone() as Rc<dyn TrustPromptViewDelegate>));
    view.set_delegate(Some(weak));
    view.set_request(Some(request.clone()));
    view.choose_for_testing(TrustPromptDecision::Deny);

    assert_eq!(*delegate.decision.borrow(), Some(TrustPromptDecision::Deny));
    assert_eq!(*delegate.request.borrow(), Some(request));
    assert_eq!(accessibility_label(&*view).as_deref(), Some("Permission Needed"));
}

fn prompt_names_the_file_its_grant_will_persist() {
    let local = TrustRequest::new(
        TrustEffect::LaunchPathOrEditor,
        TrustTarget::new("/workspace/scripts/build.sh", Some("/workspace/scripts/build.sh"), None),
        Some(&FileUrl::from_path("/workspace/README.md")),
    );
    let web = TrustRequest::new(
        TrustEffect::OpenExternalLink,
        TrustTarget::new("https://example.com", None, Some("https://example.com")),
        Some(&FileUrl::from_path("/workspace/README.md")),
    );

    assert_eq!(TrustPromptView::file_grant_name(&local).as_deref(), Some("build.sh"));
    assert_eq!(TrustPromptView::file_grant_name(&web).as_deref(), Some("README.md"));
}

// MARK: - VisualDebuggerViewTests

fn exposes_read_only_summary_and_copies_it() {
    let view = VisualDebuggerView::new_current(mtm());
    view.set_model(VisualDebuggerModel::new(&VisualDebuggerInput::new(
        MarkdownParser::parse("# Title\n"),
        NSRange::new(0, 0),
        RenderMode::Read,
    )));
    assert_eq!(accessibility_label(&*view).as_deref(), Some("Visual Debugger"));
    assert!(view.summary_text_for_testing().contains("Visual Debugger"));

    // Copy into a named pasteboard: the view must route through the
    // injectable target, so nothing it does reaches the user's real
    // clipboard by construction.
    let scratch = NSPasteboard::pasteboardWithName(&NSString::from_str("downright.test.visual-debugger"));
    scratch.clearContents();
    scratch.setString_forType(&NSString::from_str("sentinel"), unsafe { NSPasteboardTypeString });
    let general = view.pasteboard_for_testing();
    view.set_pasteboard_for_testing(scratch.clone());
    view.copy_summary_for_testing();
    view.set_pasteboard_for_testing(general);
    let copied = scratch.stringForType(unsafe { NSPasteboardTypeString }).map(|string| string.to_string());
    // Not in the Swift test: the scratch pasteboard is released afterwards.
    let _: () = unsafe { msg_send![&*scratch, releaseGlobally] };
    assert!(copied.is_some_and(|copied| copied.contains("Visual Debugger")));
}

// MARK: - WorkspaceTests

#[derive(Default)]
struct RecordingWorkspaceDelegate {
    queries: RefCell<Vec<WorkspaceSearchQuery>>,
}

impl WorkspaceSidebarViewDelegate for RecordingWorkspaceDelegate {
    fn workspace_sidebar_did_select(
        &self,
        _view: &WorkspaceSidebarView,
        _url: &FileUrl,
        _range: Option<NSRange>,
        _in_new_window: bool,
    ) {
    }

    fn workspace_sidebar_did_search(&self, _view: &WorkspaceSidebarView, query: &WorkspaceSearchQuery) {
        self.queries.borrow_mut().push(query.clone());
    }
}

fn sidebar_shows_tree_search_and_accessible_rows() {
    let mtm = mtm();
    let entry = WorkspaceIndexEntry::new(
        FileUrl::from_path("/workspace/readme.md"),
        "readme.md",
        "# Readme",
        Vec::new(),
        Vec::new(),
        Vec::new(),
        8,
    );
    let view = WorkspaceSidebarView::new_current(mtm);
    let delegate = Rc::new(RecordingWorkspaceDelegate::default());
    let weak: std::rc::Weak<dyn WorkspaceSidebarViewDelegate> =
        Rc::downgrade(&(delegate.clone() as Rc<dyn WorkspaceSidebarViewDelegate>));
    view.set_delegate(Some(weak));
    view.set_entries(vec![entry]);
    // `view.numberOfRows(in: NSTableView())` and
    // `view.tableView(NSTableView(), viewFor: nil, row: 0)`, sent as AppKit
    // sends them.
    let rows: isize = unsafe { msg_send![&*view, numberOfRowsInTableView: &*NSTableView::new(mtm)] };
    assert_eq!(rows, 1);
    let row: Option<Retained<NSView>> = unsafe {
        msg_send![
            &*view,
            tableView: &*NSTableView::new(mtm),
            viewForTableColumn: Option::<&NSTableColumn>::None,
            row: 0isize
        ]
    };
    let row = row.expect("a row view");
    assert!(accessibility_label(&*row).is_some_and(|label| label.contains("readme.md")));
    view.set_selected_tab(WorkspaceSidebarTab::Search);
    view.set_search_text_for_testing("readme");
    assert_eq!(delegate.queries.borrow().first().map(|query| query.text.clone()).as_deref(), Some("readme"));
    assert_eq!(accessibility_label(&*view).as_deref(), Some("Workspace"));
}

// MARK: - Class names

/// Not in the Swift suite: every panel class this group registers keeps its
/// Swift name. (The private row views, `ReviewRowView` and
/// `WorkspaceSidebarRowView`, register when a table first asks for a row;
/// the `panel` dumps compare their names.)
fn panel_classes_keep_their_swift_names() {
    let mtm = mtm();
    let names = [
        "ReviewPanelView",
        "WorkspaceSidebarView",
        "VersionTimelineView",
        "HistoryInspectorView",
        "TrustPromptView",
        "ReaderProfilePickerView",
        "LocalAIPanelView",
        "VisualDebuggerView",
    ];
    // Building each panel registers its classes.
    let _ = upleft_app::panels::review_panel_view::ReviewPanelView::new_current(mtm);
    let _ = WorkspaceSidebarView::new_current(mtm);
    let _ = upleft_app::panels::history_inspector_view::HistoryInspectorView::new_current(mtm);
    let _ = TrustPromptView::new_current(mtm);
    let _ = upleft_app::panels::local_ai_panel_view::LocalAIPanelView::new_current(mtm);
    let _ = VisualDebuggerView::new_current(mtm);
    let _ = ReaderProfilePickerView::new(Rc::new(InMemoryReaderProfileStore::default()), current(mtm), mtm);
    for name in names {
        let name = std::ffi::CString::new(name).unwrap();
        assert!(objc2::runtime::AnyClass::get(&name).is_some(), "{name:?} is not registered");
    }
}

/// Sandboxes the support folder, the home folder and `Preferences.shared`.
/// Call first thing in `main`, before any thread starts.
fn sandbox() {
    let root = document_support::sandbox();
    let home = root.appending_path_component_is_directory("home", true);
    std::fs::create_dir_all(home.path()).unwrap();
    // SAFETY: called from `main` before any other thread exists.
    unsafe {
        std::env::set_var("CFFIXED_USER_HOME", home.path());
        std::env::set_var("HOME", home.path());
    }
    let preferences = Preferences::for_testing(root.appending_path_component("preferences.json"), None);
    assert!(Preferences::install_shared_for_testing(preferences), "Preferences.shared was read before the sandbox");
    let themes = ThemeStore::user_themes_directory().and_then(|url| url.path()).unwrap().to_string();
    assert!(themes.starts_with(&home.path()), "the themes folder {themes} escaped the sandbox");
}

fn main() {
    sandbox();
    main_thread::run_off_screen(&[
        ("picker_lists_built_ins_and_sends_live_preview", picker_lists_built_ins_and_sends_live_preview),
        ("saving_custom_profile_uses_injected_store", saving_custom_profile_uses_injected_store),
        (
            "prompt_reports_exact_typed_decision_and_voice_over_labels",
            prompt_reports_exact_typed_decision_and_voice_over_labels,
        ),
        ("prompt_names_the_file_its_grant_will_persist", prompt_names_the_file_its_grant_will_persist),
        ("exposes_read_only_summary_and_copies_it", exposes_read_only_summary_and_copies_it),
        ("sidebar_shows_tree_search_and_accessible_rows", sidebar_shows_tree_search_and_accessible_rows),
        ("panel_classes_keep_their_swift_names", panel_classes_keep_their_swift_names),
    ]);
    document_support::remove_sandbox();
}
