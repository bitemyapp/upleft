//! Port of `App/DocumentWindowController.swift`: one window over one
//! document.
//!
//! The window owns the document, the text surface, and every transient panel.
//! §11.4 is the layout rule: nothing is resident. The outline, tasks,
//! siblings, find, and the conflict bar are all summoned and dismissed; what
//! stays on screen is the document, the density gutter, and a breadcrumb.
//!
//! `DocumentWindowController` is a `define_class!` `NSWindowController`
//! subclass of that name. Every Swift stored property is an ivar, reached
//! only through the accessors below; every Objective-C entry point of the
//! class, including those whose bodies live in the `+Extension` modules, is
//! declared in the one `define_class!` here and forwards to a Rust method in
//! the owning module. The extensions that keep state in associated objects
//! each own one `<ext>_state` ivar (`RefCell<…State>`), defined in their
//! module.
//!
//! The private Swift classes of the file are in [`views`]
//! (`FloatingOverlayHostView`, `FocusDimmingView`, `DocumentRootView`);
//! [`SiblingSearchCancellationToken`] and [`SiblingSearchRunner`] are here.
//!
//! # Contract (for the `+Extension` modules)
//!
//! Other modules use only these accessors, never the ivars. `let` and lazy
//! properties return `&`/`Retained`/`Rc`; `var`s have `foo()`/`set_foo(…)`,
//! and `set_foo` runs the Swift `didSet` where there is one. Swift's
//! `private` members are private to this module (and its child modules).
//!
//! | Swift | Rust |
//! |---|---|
//! | `let markdownDocument` | `markdown_document() -> &Retained<MarkdownDocument>` |
//! | `var mode` | `mode()`, `set_mode(RenderMode)` |
//! | `var onClose` | `on_close()`, `set_on_close(Option<Rc<dyn Fn()>>)` |
//! | `var primaryContainer: MarkdownContainerView!` | `primary_container() -> Retained<…>` (panics while nil, as the IUO traps), `primary_container_opt()`, `set_primary_container(Option<…>)` |
//! | `var splitContainer` | `split_container()`, `set_split_container(Option<Retained<MarkdownContainerView>>)` |
//! | `var splitViewContainer` | `split_view_container()`, `set_split_view_container(Option<Retained<ThemedSplitView>>)` |
//! | `let breadcrumbView` | `breadcrumb_view() -> &Retained<BreadcrumbView>` |
//! | `let densityGutterView` | `density_gutter_view() -> &Retained<DensityGutterView>` |
//! | `var statusBarView: DocumentStatusBarView!` | `status_bar_view() -> Retained<…>`, `set_status_bar_view(Option<…>)` |
//! | `let activityIndicator` | `activity_indicator() -> &Retained<ActivityIndicatorView>` |
//! | `var taskPanel` | `task_panel()`, `set_task_panel(Option<Retained<TaskPanelView>>)` |
//! | `var findBar` | `find_bar()`, `set_find_bar(Option<Retained<FindBarView>>)` |
//! | `var conflictBar` | `conflict_bar()`, `set_conflict_bar(Option<Retained<ConflictBarView>>)` |
//! | `var changeSummaryBar` | `change_summary_bar()`, `set_change_summary_bar(Option<Retained<ChangeSummaryBarView>>)` |
//! | `var changeSummaryTopConstraint` | `change_summary_top_constraint()`, `set_change_summary_top_constraint(…)` |
//! | `var searchResults` | `search_results()`, `set_search_results(Option<Retained<SearchResultsPanelView>>)` |
//! | `var siblingSearchActive` (computed) | `sibling_search_active() -> bool` |
//! | `var siblingSearchGeneration` | `sibling_search_generation()`, `set_sibling_search_generation(isize)` |
//! | `let siblingSearchQueue` | `sibling_search_queue() -> &DispatchRetained<DispatchQueue>` |
//! | `var siblingSearchCancellation` | `sibling_search_cancellation()`, `set_sibling_search_cancellation(Option<Arc<SiblingSearchCancellationToken>>)` |
//! | `var siblingSearchRunner` | `sibling_search_runner()`, `set_sibling_search_runner(SiblingSearchRunner)` |
//! | `var searchInspector` | `search_inspector()`, `set_search_inspector(Option<Retained<SearchInspectorView>>)` |
//! | `var historyInspector` | `history_inspector()`, `set_history_inspector(Option<Retained<HistoryInspectorView>>)` |
//! | `var inspectorHost` | `inspector_host()`, `set_inspector_host(Option<Retained<InspectorHostView>>)` |
//! | `private(set) var floatingSurface` | `floating_surface() -> Option<Retained<FloatingPanelSurface>>` |
//! | `var isTaskPanelFloating` (computed) | `is_task_panel_floating() -> bool` |
//! | `var frontMatterEditor` | `front_matter_editor()`, `set_front_matter_editor(Option<Retained<FrontMatterEditorView>>)` |
//! | `var assetDoctorPanel` | `asset_doctor_panel()`, `set_asset_doctor_panel(Option<Retained<AssetDoctorView>>)` |
//! | `var tidySheetWindow` | `tidy_sheet_window()`, `set_tidy_sheet_window(Option<Retained<NSWindow>>)` |
//! | `var tableEditorWindow` | `table_editor_window()`, `set_table_editor_window(Option<Retained<NSWindow>>)` |
//! | `var barStack: NSStackView!` | `bar_stack() -> Retained<NSStackView>`, `set_bar_stack(Option<…>)` |
//! | `var scanner { didSet }` | `scanner() -> Option<Rc<SiblingScanner>>`, `set_scanner(Option<Rc<SiblingScanner>>)` (runs `didSet`; identity is `Rc::ptr_eq`) |
//! | `var pathResolver` | `path_resolver() -> Option<PathResolver>` (a clone shares it), `set_path_resolver(Option<PathResolver>)` |
//! | `let findSession` (a class) | `find_session() -> &RefCell<FindSession>` (borrow for one statement) |
//! | `let jumpHistory` (a class) | `jump_history() -> &RefCell<JumpHistory>` |
//! | `var activeStyleSheet` | `active_style_sheet() -> Rc<StyleSheet>`, `set_active_style_sheet(Rc<StyleSheet>)` |
//! | `let progressRing` | `progress_ring() -> &Retained<TaskProgressRing>` |
//! | `var isOpeningDocument` | `is_opening_document()`, `set_is_opening_document(bool)` |
//! | `var isFocusModeEnabled` (computed) | `is_focus_mode_enabled() -> bool` |
//! | `var pendingConflict` | `pending_conflict() -> Option<Conflict>`, `set_pending_conflict(Option<Conflict>)` |
//! | `weak var toolbarPresentationControl` | `toolbar_presentation_control()`, `set_toolbar_presentation_control(Option<&ToolbarPresentationControl>)` |
//! | `lazy var presentationSwipe` | `presentation_swipe() -> Rc<PresentationSwipeCoordinator>` |
//! | `lazy var scrollZoom` | `scroll_zoom() -> Rc<ScrollZoomCoordinator>` |
//! | `lazy var historySwipe` | `history_swipe() -> Rc<HistorySwipeCoordinator>` |
//! | `lazy var documentScrollGestures` | `document_scroll_gestures() -> Rc<ScrollGestureChain>` |
//! | `weak var toolbarDocumentIdentityView` | `toolbar_document_identity_view()`, `set_toolbar_document_identity_view(Option<&ToolbarDocumentIdentityView>)` |
//! | `weak var toolbarFindButton` | `toolbar_find_button()`, `set_toolbar_find_button(Option<&ToolbarActionButton>)` |
//! | `var toolbarGlassBand` | `toolbar_glass_band()`, `set_toolbar_glass_band(Option<Retained<ToolbarGlassBand>>)` |
//! | `weak var toolbarOverflowButton` | `toolbar_overflow_button()`, `set_toolbar_overflow_button(Option<&ToolbarMenuButton>)` |
//! | `var updateStatusPill` | `update_status_pill()`, `set_update_status_pill(Option<Retained<UpdateStatusPill>>)` |
//! | `var saveRecoveryAlert` | `save_recovery_alert()`, `set_save_recovery_alert(Option<Retained<NSAlert>>)` |
//! | `var isWindowPinned` (computed) | `is_window_pinned() -> bool` |
//! | `var documentPanes` (computed) | `document_panes() -> Vec<Retained<MarkdownContainerView>>` |
//! | `var currentFindQuery` (computed) | `current_find_query() -> FindQuery` |
//! | associated-object state of `+X` | `<x>_state() -> &RefCell<crate::app::document_window_controller_<x>::<X>State>` for CommandPalette, Diagnostics, DocumentLens, LocalAI, ReaderProfiles, Review, Share, Speech, Trust, VisualDebugger, Workspace |
//! | `self` handed to a view as its delegate | `delegates() -> Rc<DocumentWindowControllerDelegates>`; pass `Rc::downgrade(&…) as Weak<dyn XDelegate>`; the proxy's `controller()` gives the controller back. Each `XDelegate` trait is implemented on the proxy in the module where Swift declares the conformance. |
//!
//! Construction and the rest of this file's methods: `new(mtm)` (`init()`),
//! `open(&FileUrl, RenderMode) -> Result<(), DocumentError>`,
//! `reset_transient_chrome`, `dump_layout_if_requested`,
//! `adopt(text, title)`, `apply_status_bar_preference`, `apply_style_sheet`,
//! `apply_mode`, `synchronize_panes(source, selection, viewport)`,
//! `set_shared_zoom`, `set_shared_folds(folded, Option<&MarkdownTextView>)`,
//! `schedule_derived_ui_refresh(immediate)`, `schedule_find_refresh`,
//! `schedule_find_query`, `refresh_derived_ui`, `note_visible_change_marks`,
//! `refresh_density_bands(Option<&[ReadingMetrics]>)`, `refresh_breadcrumb`,
//! `visible_heading_index(at)`, `refresh_change_decorations`,
//! `mark_changes_reviewed`, `refresh_change_summary_top_inset`,
//! `show_change_summary(Option<&str>)`, `show_conflict_bar(&str)`,
//! `dismiss_conflict_bar`, `dismiss_change_summary`, `floating_frame`,
//! `refit_floating_surface_after_content_change`, `toggle_task_panel`,
//! `handle_new_document_command`, `close_task_panel`,
//! `install_trailing(&NSView, Option<&str>)`, `dismiss_trailing`,
//! `show_in_inspector(&NSView, InspectorSection)`,
//! `close_inspector(restoring_focus)`, `retain_timeline`,
//! `show_find_bar(replace, Option<FindQuery>)`, `show_find_inspector`,
//! `dismiss_find_bar`, `apply_find_query`,
//! `run_find(query, scroll_to_match, highlight_all)`, `advance_find`,
//! `record_jump`, `jump(to, label, animated)`, `go_back`, `go_forward`,
//! `toggle_split_view`, `update_focus_dimming_views`,
//! `confirm_pending_changes_before_close(mark_discard_for_window_close)`,
//! `document_will_close`, `toggle_pin`, `toggle_focus_mode`,
//! `apply_focus_mode(enabled, animated)`.
//!
//! # Objective-C entry points
//!
//! | Selector | Forwards to | Module |
//! |---|---|---|
//! | `windowDidBecomeKey:` … `windowDidChangeOcclusionState:` (NSWindowDelegate) | `window_did_become_key(&NSNotification)` etc. | this file |
//! | `preferencesDidChange`, `accessibilityDisplayOptionsDidChange`, `auxiliaryWindowWillClose:` | `preferences_did_change`, `accessibility_display_options_did_change`, `auxiliary_window_will_close` | this file |
//! | `validateMenuItem:` | `validate_menu_item(&NSMenuItem) -> bool` | `+Commands` |
//! | `performDownrightCommand:` | `perform_downright_command(Option<&AnyObject>)` | `+Commands` |
//! | `toolbarDefaultItemIdentifiers:` / `toolbarAllowedItemIdentifiers:` | `toolbar_default_item_identifiers(&NSToolbar)` / `toolbar_allowed_item_identifiers(&NSToolbar)` `-> Retained<NSArray<NSString>>` | `+Actions` |
//! | `toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:` | `toolbar_item_for_item_identifier(&NSToolbar, &NSString, bool) -> Option<Retained<NSToolbarItem>>` | `+Actions` |
//! | `validateToolbarItem:` | `validate_toolbar_item(&NSToolbarItem) -> bool` | `+Actions` |
//! | `menuNeedsUpdate:` | `menu_needs_update(&NSMenu)` | `+Actions` |
//! | `toolbarShowTasks:` `toolbarToggleSourceFocus:` `toolbarShowHistory:` `toolbarShowFind:` `toolbarCheckForUpdates:` | `toolbar_show_tasks(Option<&AnyObject>)` etc. | `+Actions` |
//! | `validRequestorForSendType:returnType:` (override) | `valid_requestor(Option<&NSString>, Option<&NSString>) -> Option<Retained<AnyObject>>` | `+ContinuityCamera` |
//! | `readSelectionFromPasteboard:` | `read_selection(&NSPasteboard) -> bool` | `+ContinuityCamera` |
//! | `sharingServicePicker:delegateForSharingService:` | `sharing_service_picker_delegate_for(&NSSharingServicePicker, &NSSharingService) -> Option<Retained<AnyObject>>` | `+Share` |
//! | `sharingServicePicker:didChooseSharingService:` | `sharing_service_picker_did_choose(&NSSharingServicePicker, Option<&NSSharingService>)` | `+Share` |
//! | `sharingService:sourceWindowForShareItems:sharingContentScope:` | `sharing_service_source_window(&NSSharingService, &NSArray, NonNull<NSSharingContentScope>) -> Option<Retained<NSWindow>>` | `+Share` |

mod construction;
mod derived_ui;
mod find;
mod floating;
mod lifecycle;
mod navigation;
mod opening;
mod presentation;
mod views;

pub(crate) use construction::ControllerHandle;

use std::cell::{Cell, OnceCell, RefCell};
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class};
use objc2_app_kit::{
    NSAlert, NSMenu, NSMenuDelegate, NSMenuItem, NSMenuItemValidation, NSPasteboard, NSResponder,
    NSServicesMenuRequestor, NSSharingContentScope, NSSharingService, NSSharingServiceDelegate,
    NSSharingServicePicker, NSSharingServicePickerDelegate, NSSplitViewController, NSStackView, NSToolbar,
    NSToolbarDelegate, NSToolbarItem, NSToolbarItemValidation, NSView, NSWindow, NSWindowController,
    NSWindowDelegate, NSLayoutConstraint,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSArray, NSNotification, NSRect, NSString, NSUndoManager};
use upleft_core::ReadingMetrics;
use upleft_foundation::url::FileUrl;
use upleft_render::appkit_compat::WorkItem;
use upleft_render::render_contracts::RenderMode;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeObservation;
use upleft_render::view::density_gutter_view::DensityGutterView;
use upleft_render::view::markdown_container_view::MarkdownContainerView;

use crate::ai::markdown_document::{Conflict, MarkdownDocument};
use crate::ai::path_resolver::PathResolver;
use crate::ai::sibling_scanner::SiblingScanner;
use crate::app::document_scroll_gestures::ScrollGestureChain;
use crate::app::document_window_controller_command_palette::CommandPaletteState;
use crate::app::document_window_controller_diagnostics::DiagnosticsState;
use crate::app::document_window_controller_document_lens::DocumentLensState;
use crate::app::document_window_controller_local_ai::LocalAIState;
use crate::app::document_window_controller_reader_profiles::ReaderProfilesState;
use crate::app::document_window_controller_review::ReviewState;
use crate::app::document_window_controller_share::ShareState;
use crate::app::document_window_controller_speech::SpeechState;
use crate::app::document_window_controller_trust::TrustState;
use crate::app::document_window_controller_visual_debugger::VisualDebuggerState;
use crate::app::document_window_controller_workspace::WorkspaceState;
use crate::app::history_swipe::HistorySwipeCoordinator;
use crate::app::presentation_swipe::PresentationSwipeCoordinator;
use crate::app::scroll_zoom::ScrollZoomCoordinator;
use crate::app::themed_split_view::ThemedSplitView;
use crate::app::toolbar_controls::{
    ToolbarActionButton, ToolbarDocumentIdentityView, ToolbarMenuButton, ToolbarPresentationControl,
};
use crate::app::toolbar_glass_band::ToolbarGlassBand;
use crate::panels::activity_indicator_view::ActivityIndicatorView;
use crate::panels::asset_doctor_view::AssetDoctorView;
use crate::panels::breadcrumb_view::BreadcrumbView;
use crate::panels::change_summary_bar_view::ChangeSummaryBarView;
use crate::panels::conflict_bar_view::ConflictBarView;
use crate::panels::document_status_bar_view::DocumentStatusBarView;
use crate::panels::find_bar_view::FindBarView;
use crate::panels::floating_panel_surface::{FloatingPanelSurface, FloatingPanelWindow};
use crate::panels::front_matter_editor_view::FrontMatterEditorView;
use crate::panels::history_inspector_view::HistoryInspectorView;
use crate::panels::inspector_host_view::{InspectorHostView, InspectorSection};
use crate::panels::search_inspector_view::SearchInspectorView;
use crate::panels::search_results_panel_view::SearchResultsPanelView;
use crate::panels::task_panel_view::TaskPanelView;
use crate::panels::task_progress_ring::TaskProgressRing;
use crate::panels::update_status_pill::UpdateStatusPill;
use crate::support::find_engine::{FindQuery, FindSession, SiblingHit, SiblingSearch};
use crate::support::jump_history::JumpHistory;
use crate::support::preferences::Preferences;

pub use views::{DocumentRootView, FloatingOverlayHostView, FocusDimmingView};

// MARK: - Sibling search

/// `typealias SiblingSearchRunner`: `(query, urls, shouldCancel) -> hits`,
/// run on the sibling-search queue.
pub type SiblingSearchRunner =
    Arc<dyn Fn(&FindQuery, &[FileUrl], &(dyn Fn() -> bool + Sync)) -> Vec<SiblingHit> + Send + Sync>;

/// The default runner: `SiblingSearch.search(query, in: urls, shouldCancel:)`.
pub fn default_sibling_search_runner() -> SiblingSearchRunner {
    Arc::new(|query, urls, should_cancel| SiblingSearch::search(query, urls, 20, should_cancel))
}

/// A filesystem pass cannot be interrupted while Foundation is inside one
/// read, but it can stop before parsing that file or opening the next one.
/// The lock is the whole synchronization contract for this cross-queue flag.
#[derive(Debug, Default)]
pub struct SiblingSearchCancellationToken {
    cancelled: Mutex<bool>,
}

impl SiblingSearchCancellationToken {
    pub fn new() -> SiblingSearchCancellationToken {
        SiblingSearchCancellationToken::default()
    }

    /// `isCancelled`.
    pub fn is_cancelled(&self) -> bool {
        *self.cancelled.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// `cancel()`.
    pub fn cancel(&self) {
        *self.cancelled.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
    }
}

// MARK: - Delegate proxy

/// The controller as the delegate of its views. Views hold delegates weakly
/// as `std::rc::Weak<dyn XDelegate>`; the controller owns this proxy (an
/// `Rc`) and hands out `Rc::downgrade`s of it. Each delegate trait is
/// implemented on the proxy in the module where Swift declares the
/// conformance.
pub struct DocumentWindowControllerDelegates {
    controller: ObjcWeak<DocumentWindowController>,
}

impl DocumentWindowControllerDelegates {
    /// The controller, while it is alive (Swift's `self`).
    pub fn controller(&self) -> Option<Retained<DocumentWindowController>> {
        self.controller.load()
    }
}

// MARK: - Class

/// Swift's stored properties, in declaration order, then the extensions'
/// associated-object state.
pub struct DocumentWindowControllerIvars {
    /// Key of this controller in the main-thread registry behind
    /// [`ControllerHandle`].
    id: usize,
    markdown_document: Retained<MarkdownDocument>,
    mode: Cell<RenderMode>,
    on_close: RefCell<Option<Rc<dyn Fn()>>>,

    // Text surface.
    primary_container: RefCell<Option<Retained<MarkdownContainerView>>>,
    split_container: RefCell<Option<Retained<MarkdownContainerView>>>,
    split_view_container: RefCell<Option<Retained<ThemedSplitView>>>,

    // Persistent chrome.
    breadcrumb_view: Retained<BreadcrumbView>,
    density_gutter_view: Retained<DensityGutterView>,
    status_bar_view: RefCell<Option<Retained<DocumentStatusBarView>>>,
    activity_indicator: Retained<ActivityIndicatorView>,

    // Transient panels (§11.4).
    task_panel: RefCell<Option<Retained<TaskPanelView>>>,
    find_bar: RefCell<Option<Retained<FindBarView>>>,
    conflict_bar: RefCell<Option<Retained<ConflictBarView>>>,
    change_summary_bar: RefCell<Option<Retained<ChangeSummaryBarView>>>,
    change_summary_top_constraint: RefCell<Option<Retained<NSLayoutConstraint>>>,
    change_summary_dismiss_work_item: RefCell<Option<WorkItem>>,
    search_results: RefCell<Option<Retained<SearchResultsPanelView>>>,
    sibling_search_generation: Cell<isize>,
    sibling_search_queue: DispatchRetained<DispatchQueue>,
    sibling_search_cancellation: RefCell<Option<Arc<SiblingSearchCancellationToken>>>,
    sibling_search_runner: RefCell<SiblingSearchRunner>,
    search_inspector: RefCell<Option<Retained<SearchInspectorView>>>,
    history_inspector: RefCell<Option<Retained<HistoryInspectorView>>>,
    inspector_host: RefCell<Option<Retained<InspectorHostView>>>,
    floating_surface: RefCell<Option<Retained<FloatingPanelSurface>>>,
    floating_panel_window: RefCell<Option<Retained<FloatingPanelWindow>>>,
    floating_surface_frame: Cell<NSRect>,
    floating_control_anchor_frame: Cell<NSRect>,
    floating_focus_restore_view: RefCell<ObjcWeak<NSView>>,
    floating_presentation_needs_focus: Cell<bool>,
    floating_dismiss_viewport_repairs: RefCell<Vec<Box<dyn Fn()>>>,
    floating_resize_token: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
    floating_activation_observer: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
    front_matter_editor: RefCell<Option<Retained<FrontMatterEditorView>>>,
    asset_doctor_panel: RefCell<Option<Retained<AssetDoctorView>>>,
    tidy_sheet_window: RefCell<Option<Retained<NSWindow>>>,
    table_editor_window: RefCell<Option<Retained<NSWindow>>>,
    auxiliary_windows: RefCell<Vec<Retained<NSWindowController>>>,

    // Layout containers.
    root_view: RefCell<Option<Retained<NSView>>>,
    bar_stack: RefCell<Option<Retained<NSStackView>>>,
    window_split_controller: RefCell<Option<Retained<NSSplitViewController>>>,
    floating_overlay_host: RefCell<Option<Retained<FloatingOverlayHostView>>>,

    // State.
    scanner: RefCell<Option<Rc<SiblingScanner>>>,
    path_resolver: RefCell<Option<PathResolver>>,
    resolves_path_tokens: Cell<bool>,
    is_clearing_disabled_path_state: Cell<bool>,
    find_session: RefCell<FindSession>,
    jump_history: RefCell<JumpHistory>,
    active_style_sheet: RefCell<Rc<StyleSheet>>,
    theme_observation: RefCell<Option<ThemeObservation>>,
    progress_ring: Retained<TaskProgressRing>,
    is_pinned: Cell<bool>,
    focus_mode_applied: Cell<bool>,
    discard_changes_on_close: Cell<bool>,
    implicit_save_suppressed: Cell<bool>,
    focus_dimming_views: RefCell<Vec<Retained<FocusDimmingView>>>,
    is_synchronizing_panes: Cell<bool>,
    pending_initial_restore_offset: Cell<Option<isize>>,
    deferred_initial_restore_offset: Cell<Option<isize>>,
    is_opening_document: Cell<bool>,
    initial_restore_viewport_y: Cell<Option<CGFloat>>,
    initial_restore_generation: Cell<usize>,
    pending_conflict: RefCell<Option<Conflict>>,
    toolbar_presentation_control: RefCell<ObjcWeak<ToolbarPresentationControl>>,
    presentation_swipe: OnceCell<Rc<PresentationSwipeCoordinator>>,
    scroll_zoom: OnceCell<Rc<ScrollZoomCoordinator>>,
    history_swipe: OnceCell<Rc<HistorySwipeCoordinator>>,
    document_scroll_gestures: OnceCell<Rc<ScrollGestureChain>>,
    toolbar_document_identity_view: RefCell<ObjcWeak<ToolbarDocumentIdentityView>>,
    toolbar_find_button: RefCell<ObjcWeak<ToolbarActionButton>>,
    toolbar_glass_band: RefCell<Option<Retained<ToolbarGlassBand>>>,
    toolbar_overflow_button: RefCell<ObjcWeak<ToolbarMenuButton>>,
    update_status_pill: RefCell<Option<Retained<UpdateStatusPill>>>,
    last_breadcrumb_heading_index: Cell<isize>,
    derived_ui_refresh_work_item: RefCell<Option<WorkItem>>,
    find_refresh_work_item: RefCell<Option<WorkItem>>,
    exiting_find_bar: RefCell<Option<Retained<FindBarView>>>,
    exiting_search_inspector: RefCell<Option<Retained<SearchInspectorView>>>,
    find_bar_exit_generation: Cell<isize>,
    cached_metrics_document_id: Cell<Option<usize>>,
    cached_section_metrics: RefCell<Vec<ReadingMetrics>>,
    cached_word_count: Cell<isize>,
    autosave_work_item: RefCell<Option<WorkItem>>,
    save_recovery_alert: RefCell<Option<Retained<NSAlert>>>,

    // Associated-object state of the extensions.
    command_palette_state: RefCell<CommandPaletteState>,
    diagnostics_state: RefCell<DiagnosticsState>,
    document_lens_state: RefCell<DocumentLensState>,
    local_ai_state: RefCell<LocalAIState>,
    reader_profiles_state: RefCell<ReaderProfilesState>,
    review_state: RefCell<ReviewState>,
    share_state: RefCell<ShareState>,
    speech_state: RefCell<SpeechState>,
    trust_state: RefCell<TrustState>,
    visual_debugger_state: RefCell<VisualDebuggerState>,
    workspace_state: RefCell<WorkspaceState>,

    /// `self` as its views' delegate.
    delegates: OnceCell<Rc<DocumentWindowControllerDelegates>>,
}

define_class!(
    /// `DocumentWindowController`.
    // SAFETY: `initWithWindow:` is forwarded in `new` after the ivars are
    // set; every method below keeps AppKit's signature for its selector.
    #[unsafe(super(NSWindowController, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "DocumentWindowController"]
    #[ivars = DocumentWindowControllerIvars]
    pub struct DocumentWindowController;

    unsafe impl NSObjectProtocol for DocumentWindowController {}

    impl DocumentWindowController {
        // Notification observers (this file).

        #[unsafe(method(preferencesDidChange))]
        fn __preferences_did_change(&self) {
            self.preferences_did_change();
        }

        #[unsafe(method(accessibilityDisplayOptionsDidChange))]
        fn __accessibility_display_options_did_change(&self) {
            self.accessibility_display_options_did_change();
        }

        #[unsafe(method(auxiliaryWindowWillClose:))]
        fn __auxiliary_window_will_close(&self, notification: &NSNotification) {
            self.auxiliary_window_will_close(notification);
        }

        // `+Commands` (`CommandResponder`).

        #[unsafe(method(performDownrightCommand:))]
        fn __perform_downright_command(&self, sender: Option<&AnyObject>) {
            self.perform_downright_command(sender);
        }

        // `+Actions`: toolbar targets.

        #[unsafe(method(toolbarShowTasks:))]
        fn __toolbar_show_tasks(&self, sender: Option<&AnyObject>) {
            self.toolbar_show_tasks(sender);
        }

        #[unsafe(method(toolbarToggleSourceFocus:))]
        fn __toolbar_toggle_source_focus(&self, sender: Option<&AnyObject>) {
            self.toolbar_toggle_source_focus(sender);
        }

        #[unsafe(method(toolbarShowHistory:))]
        fn __toolbar_show_history(&self, sender: Option<&AnyObject>) {
            self.toolbar_show_history(sender);
        }

        #[unsafe(method(toolbarShowFind:))]
        fn __toolbar_show_find(&self, sender: Option<&AnyObject>) {
            self.toolbar_show_find(sender);
        }

        #[unsafe(method(toolbarCheckForUpdates:))]
        fn __toolbar_check_for_updates(&self, sender: Option<&AnyObject>) {
            self.toolbar_check_for_updates(sender);
        }

        // `+ContinuityCamera`: the `NSResponder` override.

        #[unsafe(method_id(validRequestorForSendType:returnType:))]
        fn __valid_requestor(
            &self,
            send_type: Option<&NSString>,
            return_type: Option<&NSString>,
        ) -> Option<Retained<AnyObject>> {
            self.valid_requestor(send_type, return_type)
        }
    }

    unsafe impl NSWindowDelegate for DocumentWindowController {
        #[unsafe(method(windowDidBecomeKey:))]
        fn __window_did_become_key(&self, notification: &NSNotification) {
            self.window_did_become_key(notification);
        }

        #[unsafe(method(windowDidResignKey:))]
        fn __window_did_resign_key(&self, notification: &NSNotification) {
            self.window_did_resign_key(notification);
        }

        #[unsafe(method_id(windowWillReturnUndoManager:))]
        fn __window_will_return_undo_manager(&self, window: &NSWindow) -> Option<Retained<NSUndoManager>> {
            self.window_will_return_undo_manager(window)
        }

        #[unsafe(method(windowDidResize:))]
        fn __window_did_resize(&self, notification: &NSNotification) {
            self.window_did_resize(notification);
        }

        #[unsafe(method(windowDidEnterFullScreen:))]
        fn __window_did_enter_full_screen(&self, notification: &NSNotification) {
            self.window_did_enter_full_screen(notification);
        }

        #[unsafe(method(windowDidExitFullScreen:))]
        fn __window_did_exit_full_screen(&self, notification: &NSNotification) {
            self.window_did_exit_full_screen(notification);
        }

        #[unsafe(method(windowDidDeminiaturize:))]
        fn __window_did_deminiaturize(&self, notification: &NSNotification) {
            self.window_did_deminiaturize(notification);
        }

        #[unsafe(method(windowDidChangeBackingProperties:))]
        fn __window_did_change_backing_properties(&self, notification: &NSNotification) {
            self.window_did_change_backing_properties(notification);
        }

        #[unsafe(method(windowShouldClose:))]
        fn __window_should_close(&self, sender: &NSWindow) -> bool {
            self.window_should_close(sender)
        }

        #[unsafe(method(windowWillClose:))]
        fn __window_will_close(&self, notification: &NSNotification) {
            self.window_will_close(notification);
        }

        #[unsafe(method(windowDidChangeOcclusionState:))]
        fn __window_did_change_occlusion_state(&self, notification: &NSNotification) {
            self.window_did_change_occlusion_state(notification);
        }
    }

    unsafe impl NSMenuItemValidation for DocumentWindowController {
        #[unsafe(method(validateMenuItem:))]
        fn __validate_menu_item(&self, menu_item: &NSMenuItem) -> bool {
            self.validate_menu_item(menu_item)
        }
    }

    unsafe impl NSToolbarDelegate for DocumentWindowController {
        #[unsafe(method_id(toolbarDefaultItemIdentifiers:))]
        fn __toolbar_default_item_identifiers(&self, toolbar: &NSToolbar) -> Retained<NSArray<NSString>> {
            self.toolbar_default_item_identifiers(toolbar)
        }

        #[unsafe(method_id(toolbarAllowedItemIdentifiers:))]
        fn __toolbar_allowed_item_identifiers(&self, toolbar: &NSToolbar) -> Retained<NSArray<NSString>> {
            self.toolbar_allowed_item_identifiers(toolbar)
        }

        #[unsafe(method_id(toolbar:itemForItemIdentifier:willBeInsertedIntoToolbar:))]
        fn __toolbar_item_for_item_identifier(
            &self,
            toolbar: &NSToolbar,
            identifier: &NSString,
            will_be_inserted: bool,
        ) -> Option<Retained<NSToolbarItem>> {
            self.toolbar_item_for_item_identifier(toolbar, identifier, will_be_inserted)
        }
    }

    unsafe impl NSMenuDelegate for DocumentWindowController {
        #[unsafe(method(menuNeedsUpdate:))]
        fn __menu_needs_update(&self, menu: &NSMenu) {
            self.menu_needs_update(menu);
        }
    }

    unsafe impl NSToolbarItemValidation for DocumentWindowController {
        #[unsafe(method(validateToolbarItem:))]
        fn __validate_toolbar_item(&self, item: &NSToolbarItem) -> bool {
            self.validate_toolbar_item(item)
        }
    }

    unsafe impl NSServicesMenuRequestor for DocumentWindowController {
        #[unsafe(method(readSelectionFromPasteboard:))]
        fn __read_selection_from_pasteboard(&self, pasteboard: &NSPasteboard) -> bool {
            self.read_selection(pasteboard)
        }
    }

    unsafe impl NSSharingServicePickerDelegate for DocumentWindowController {
        #[unsafe(method_id(sharingServicePicker:delegateForSharingService:))]
        fn __sharing_service_picker_delegate_for(
            &self,
            picker: &NSSharingServicePicker,
            service: &NSSharingService,
        ) -> Option<Retained<AnyObject>> {
            self.sharing_service_picker_delegate_for(picker, service)
        }

        #[unsafe(method(sharingServicePicker:didChooseSharingService:))]
        fn __sharing_service_picker_did_choose(
            &self,
            picker: &NSSharingServicePicker,
            service: Option<&NSSharingService>,
        ) {
            self.sharing_service_picker_did_choose(picker, service);
        }
    }

    unsafe impl NSSharingServiceDelegate for DocumentWindowController {
        #[unsafe(method_id(sharingService:sourceWindowForShareItems:sharingContentScope:))]
        fn __sharing_service_source_window(
            &self,
            service: &NSSharingService,
            items: &NSArray,
            scope: NonNull<NSSharingContentScope>,
        ) -> Option<Retained<NSWindow>> {
            self.sharing_service_source_window(service, items, scope)
        }
    }
);

// MARK: - Accessors

impl DocumentWindowController {
    pub(crate) fn mtm(&self) -> MainThreadMarker {
        MainThreadMarker::from(self)
    }

    /// `let markdownDocument`.
    pub fn markdown_document(&self) -> &Retained<MarkdownDocument> {
        &self.ivars().markdown_document
    }

    /// `var mode`.
    pub fn mode(&self) -> RenderMode {
        self.ivars().mode.get()
    }

    pub fn set_mode(&self, mode: RenderMode) {
        self.ivars().mode.set(mode);
    }

    /// `var onClose`.
    pub fn on_close(&self) -> Option<Rc<dyn Fn()>> {
        self.ivars().on_close.borrow().clone()
    }

    pub fn set_on_close(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_close.borrow_mut() = handler;
    }

    /// `primaryContainer` read through its implicitly unwrapped optional:
    /// panics while it is nil, as Swift traps.
    pub fn primary_container(&self) -> Retained<MarkdownContainerView> {
        self.primary_container_opt().expect("primaryContainer is set by buildInterface")
    }

    /// `primaryContainer?`.
    pub fn primary_container_opt(&self) -> Option<Retained<MarkdownContainerView>> {
        self.ivars().primary_container.borrow().clone()
    }

    pub fn set_primary_container(&self, container: Option<Retained<MarkdownContainerView>>) {
        *self.ivars().primary_container.borrow_mut() = container;
    }

    /// `var splitContainer`.
    pub fn split_container(&self) -> Option<Retained<MarkdownContainerView>> {
        self.ivars().split_container.borrow().clone()
    }

    pub fn set_split_container(&self, container: Option<Retained<MarkdownContainerView>>) {
        *self.ivars().split_container.borrow_mut() = container;
    }

    /// `var splitViewContainer`.
    pub fn split_view_container(&self) -> Option<Retained<ThemedSplitView>> {
        self.ivars().split_view_container.borrow().clone()
    }

    pub fn set_split_view_container(&self, split: Option<Retained<ThemedSplitView>>) {
        *self.ivars().split_view_container.borrow_mut() = split;
    }

    /// `let breadcrumbView`.
    pub fn breadcrumb_view(&self) -> &Retained<BreadcrumbView> {
        &self.ivars().breadcrumb_view
    }

    /// `let densityGutterView`.
    pub fn density_gutter_view(&self) -> &Retained<DensityGutterView> {
        &self.ivars().density_gutter_view
    }

    /// `statusBarView` through its implicitly unwrapped optional.
    pub fn status_bar_view(&self) -> Retained<DocumentStatusBarView> {
        self.ivars().status_bar_view.borrow().clone().expect("statusBarView is set by buildInterface")
    }

    pub fn set_status_bar_view(&self, view: Option<Retained<DocumentStatusBarView>>) {
        *self.ivars().status_bar_view.borrow_mut() = view;
    }

    /// `let activityIndicator`.
    pub fn activity_indicator(&self) -> &Retained<ActivityIndicatorView> {
        &self.ivars().activity_indicator
    }

    /// `var taskPanel`.
    pub fn task_panel(&self) -> Option<Retained<TaskPanelView>> {
        self.ivars().task_panel.borrow().clone()
    }

    pub fn set_task_panel(&self, panel: Option<Retained<TaskPanelView>>) {
        *self.ivars().task_panel.borrow_mut() = panel;
    }

    /// `var findBar`.
    pub fn find_bar(&self) -> Option<Retained<FindBarView>> {
        self.ivars().find_bar.borrow().clone()
    }

    pub fn set_find_bar(&self, bar: Option<Retained<FindBarView>>) {
        *self.ivars().find_bar.borrow_mut() = bar;
    }

    /// `var conflictBar`.
    pub fn conflict_bar(&self) -> Option<Retained<ConflictBarView>> {
        self.ivars().conflict_bar.borrow().clone()
    }

    pub fn set_conflict_bar(&self, bar: Option<Retained<ConflictBarView>>) {
        *self.ivars().conflict_bar.borrow_mut() = bar;
    }

    /// `var changeSummaryBar`.
    pub fn change_summary_bar(&self) -> Option<Retained<ChangeSummaryBarView>> {
        self.ivars().change_summary_bar.borrow().clone()
    }

    pub fn set_change_summary_bar(&self, bar: Option<Retained<ChangeSummaryBarView>>) {
        *self.ivars().change_summary_bar.borrow_mut() = bar;
    }

    /// `var changeSummaryTopConstraint`: held for layout tests and the
    /// transient toast's fixed corner inset.
    pub fn change_summary_top_constraint(&self) -> Option<Retained<NSLayoutConstraint>> {
        self.ivars().change_summary_top_constraint.borrow().clone()
    }

    pub fn set_change_summary_top_constraint(&self, constraint: Option<Retained<NSLayoutConstraint>>) {
        *self.ivars().change_summary_top_constraint.borrow_mut() = constraint;
    }

    fn change_summary_dismiss_work_item(&self) -> Option<WorkItem> {
        self.ivars().change_summary_dismiss_work_item.borrow().clone()
    }

    fn set_change_summary_dismiss_work_item(&self, item: Option<WorkItem>) {
        *self.ivars().change_summary_dismiss_work_item.borrow_mut() = item;
    }

    /// `var searchResults`.
    pub fn search_results(&self) -> Option<Retained<SearchResultsPanelView>> {
        self.ivars().search_results.borrow().clone()
    }

    pub fn set_search_results(&self, panel: Option<Retained<SearchResultsPanelView>>) {
        *self.ivars().search_results.borrow_mut() = panel;
    }

    /// `var siblingSearchActive`: sibling search is a presentation state, not
    /// a mode to keep in sync. Deriving it from the visible inspector also
    /// lets an in-place document hop rebind the retained search surface to
    /// the replacement scanner.
    pub fn sibling_search_active(&self) -> bool {
        self.search_results().is_some()
            && self.search_inspector().is_some()
            && self.find_bar().is_some()
            && self.scanner().is_some()
            && self.inspector_host().and_then(|host| host.selected_section()) == Some(InspectorSection::Search)
            && self.floating_surface().map(|surface| surface.is_dismissing()) == Some(false)
    }

    /// `var siblingSearchGeneration`.
    pub fn sibling_search_generation(&self) -> isize {
        self.ivars().sibling_search_generation.get()
    }

    pub fn set_sibling_search_generation(&self, generation: isize) {
        self.ivars().sibling_search_generation.set(generation);
    }

    /// `let siblingSearchQueue`.
    pub fn sibling_search_queue(&self) -> &DispatchRetained<DispatchQueue> {
        &self.ivars().sibling_search_queue
    }

    /// `var siblingSearchCancellation`.
    pub fn sibling_search_cancellation(&self) -> Option<Arc<SiblingSearchCancellationToken>> {
        self.ivars().sibling_search_cancellation.borrow().clone()
    }

    pub fn set_sibling_search_cancellation(&self, token: Option<Arc<SiblingSearchCancellationToken>>) {
        *self.ivars().sibling_search_cancellation.borrow_mut() = token;
    }

    /// `var siblingSearchRunner`.
    pub fn sibling_search_runner(&self) -> SiblingSearchRunner {
        self.ivars().sibling_search_runner.borrow().clone()
    }

    pub fn set_sibling_search_runner(&self, runner: SiblingSearchRunner) {
        *self.ivars().sibling_search_runner.borrow_mut() = runner;
    }

    /// `var searchInspector`.
    pub fn search_inspector(&self) -> Option<Retained<SearchInspectorView>> {
        self.ivars().search_inspector.borrow().clone()
    }

    pub fn set_search_inspector(&self, inspector: Option<Retained<SearchInspectorView>>) {
        *self.ivars().search_inspector.borrow_mut() = inspector;
    }

    /// `var historyInspector`.
    pub fn history_inspector(&self) -> Option<Retained<HistoryInspectorView>> {
        self.ivars().history_inspector.borrow().clone()
    }

    pub fn set_history_inspector(&self, inspector: Option<Retained<HistoryInspectorView>>) {
        *self.ivars().history_inspector.borrow_mut() = inspector;
    }

    /// `var inspectorHost`.
    pub fn inspector_host(&self) -> Option<Retained<InspectorHostView>> {
        self.ivars().inspector_host.borrow().clone()
    }

    pub fn set_inspector_host(&self, host: Option<Retained<InspectorHostView>>) {
        *self.ivars().inspector_host.borrow_mut() = host;
    }

    /// `private(set) var floatingSurface`: the floating Tasks surface (§8.5's
    /// floating clause), kept for as long as it is on screen and torn down on
    /// dismissal.
    pub fn floating_surface(&self) -> Option<Retained<FloatingPanelSurface>> {
        self.ivars().floating_surface.borrow().clone()
    }

    fn set_floating_surface(&self, surface: Option<Retained<FloatingPanelSurface>>) {
        *self.ivars().floating_surface.borrow_mut() = surface;
    }

    /// Transparent boundary that lets the native panel glass sample the
    /// document while keeping the surface out of the document's glass group.
    fn floating_panel_window(&self) -> Option<Retained<FloatingPanelWindow>> {
        self.ivars().floating_panel_window.borrow().clone()
    }

    fn set_floating_panel_window(&self, window: Option<Retained<FloatingPanelWindow>>) {
        *self.ivars().floating_panel_window.borrow_mut() = window;
    }

    /// `var isTaskPanelFloating`.
    pub fn is_task_panel_floating(&self) -> bool {
        self.floating_surface().is_some()
            && self.inspector_host().and_then(|host| host.selected_section()) == Some(InspectorSection::Tasks)
    }

    /// The surface's resting frame in screen space, remembered so a retarget
    /// can still aim after the dismissal hands the glass off early.
    fn floating_surface_frame(&self) -> NSRect {
        self.ivars().floating_surface_frame.get()
    }

    fn set_floating_surface_frame(&self, frame: NSRect) {
        self.ivars().floating_surface_frame.set(frame);
    }

    /// Toolbar-space origin captured before the surface begins moving.
    fn floating_control_anchor_frame(&self) -> NSRect {
        self.ivars().floating_control_anchor_frame.get()
    }

    fn set_floating_control_anchor_frame(&self, frame: NSRect) {
        self.ivars().floating_control_anchor_frame.set(frame);
    }

    /// The responder that was active before a floating surface opened.
    fn floating_focus_restore_view(&self) -> Option<Retained<NSView>> {
        self.ivars().floating_focus_restore_view.borrow().load()
    }

    fn set_floating_focus_restore_view(&self, view: Option<&NSView>) {
        *self.ivars().floating_focus_restore_view.borrow_mut() = view.map(ObjcWeak::new).unwrap_or_default();
    }

    fn floating_presentation_needs_focus(&self) -> bool {
        self.ivars().floating_presentation_needs_focus.get()
    }

    fn set_floating_presentation_needs_focus(&self, value: bool) {
        self.ivars().floating_presentation_needs_focus.set(value);
    }

    /// `var frontMatterEditor`.
    pub fn front_matter_editor(&self) -> Option<Retained<FrontMatterEditorView>> {
        self.ivars().front_matter_editor.borrow().clone()
    }

    pub fn set_front_matter_editor(&self, editor: Option<Retained<FrontMatterEditorView>>) {
        *self.ivars().front_matter_editor.borrow_mut() = editor;
    }

    /// `var assetDoctorPanel`.
    pub fn asset_doctor_panel(&self) -> Option<Retained<AssetDoctorView>> {
        self.ivars().asset_doctor_panel.borrow().clone()
    }

    pub fn set_asset_doctor_panel(&self, panel: Option<Retained<AssetDoctorView>>) {
        *self.ivars().asset_doctor_panel.borrow_mut() = panel;
    }

    /// `var tidySheetWindow`.
    pub fn tidy_sheet_window(&self) -> Option<Retained<NSWindow>> {
        self.ivars().tidy_sheet_window.borrow().clone()
    }

    pub fn set_tidy_sheet_window(&self, window: Option<Retained<NSWindow>>) {
        *self.ivars().tidy_sheet_window.borrow_mut() = window;
    }

    /// `var tableEditorWindow`.
    pub fn table_editor_window(&self) -> Option<Retained<NSWindow>> {
        self.ivars().table_editor_window.borrow().clone()
    }

    pub fn set_table_editor_window(&self, window: Option<Retained<NSWindow>>) {
        *self.ivars().table_editor_window.borrow_mut() = window;
    }

    /// `private var rootView: NSView!`.
    fn root_view(&self) -> Retained<NSView> {
        self.ivars().root_view.borrow().clone().expect("rootView is set by buildInterface")
    }

    /// `var barStack: NSStackView!`.
    pub fn bar_stack(&self) -> Retained<NSStackView> {
        self.ivars().bar_stack.borrow().clone().expect("barStack is set by buildInterface")
    }

    pub fn set_bar_stack(&self, stack: Option<Retained<NSStackView>>) {
        *self.ivars().bar_stack.borrow_mut() = stack;
    }

    /// `var scanner`.
    pub fn scanner(&self) -> Option<Rc<SiblingScanner>> {
        self.ivars().scanner.borrow().clone()
    }

    /// `scanner = …`, running its `didSet`: rewire `onChange` to the new
    /// scanner (identity is the `Rc`), then retire sibling-search work tied
    /// to the old one.
    pub fn set_scanner(&self, scanner: Option<Rc<SiblingScanner>>) {
        let old_value = self.ivars().scanner.replace(scanner.clone());
        let same = match (&old_value, &scanner) {
            (Some(old), Some(new)) => Rc::ptr_eq(old, new),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        if let Some(old_value) = &old_value {
            old_value.set_on_change(None::<fn()>);
        }
        let scanner_id = scanner.as_ref().map(|scanner| Rc::as_ptr(scanner) as usize);
        if let Some(scanner) = &scanner {
            let weak = ObjcWeak::new(self);
            scanner.set_on_change(Some(move || {
                let weak = weak.clone();
                // `Task { @MainActor [weak self] in … }`
                upleft_render::appkit_compat::main_async(move || {
                    let Some(this) = weak.load() else { return };
                    if this.scanner().map(|scanner| Rc::as_ptr(&scanner) as usize) != scanner_id {
                        return;
                    }
                    this.sibling_search_scanner_did_change();
                });
            }));
        }
        self.sibling_search_scanner_did_change();
    }

    /// `var pathResolver`.
    pub fn path_resolver(&self) -> Option<PathResolver> {
        self.ivars().path_resolver.borrow().clone()
    }

    pub fn set_path_resolver(&self, resolver: Option<PathResolver>) {
        *self.ivars().path_resolver.borrow_mut() = resolver;
    }

    /// `let findSession` (a class in Swift): borrow it for one statement.
    pub fn find_session(&self) -> &RefCell<FindSession> {
        &self.ivars().find_session
    }

    /// `let jumpHistory` (a class in Swift): borrow it for one statement.
    pub fn jump_history(&self) -> &RefCell<JumpHistory> {
        &self.ivars().jump_history
    }

    /// `var activeStyleSheet`.
    pub fn active_style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().active_style_sheet.borrow().clone()
    }

    pub fn set_active_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().active_style_sheet.borrow_mut() = style_sheet;
    }

    /// `let progressRing`.
    pub fn progress_ring(&self) -> &Retained<TaskProgressRing> {
        &self.ivars().progress_ring
    }

    /// `var isOpeningDocument`.
    pub fn is_opening_document(&self) -> bool {
        self.ivars().is_opening_document.get()
    }

    pub fn set_is_opening_document(&self, value: bool) {
        self.ivars().is_opening_document.set(value);
    }

    /// `var isFocusModeEnabled`.
    pub fn is_focus_mode_enabled(&self) -> bool {
        Preferences::shared().values().focus_mode
    }

    /// `var pendingConflict`.
    pub fn pending_conflict(&self) -> Option<Conflict> {
        self.ivars().pending_conflict.borrow().clone()
    }

    pub fn set_pending_conflict(&self, conflict: Option<Conflict>) {
        *self.ivars().pending_conflict.borrow_mut() = conflict;
    }

    /// `weak var toolbarPresentationControl`.
    pub fn toolbar_presentation_control(&self) -> Option<Retained<ToolbarPresentationControl>> {
        self.ivars().toolbar_presentation_control.borrow().load()
    }

    pub fn set_toolbar_presentation_control(&self, control: Option<&ToolbarPresentationControl>) {
        *self.ivars().toolbar_presentation_control.borrow_mut() = control.map(ObjcWeak::new).unwrap_or_default();
    }

    /// `weak var toolbarDocumentIdentityView`.
    pub fn toolbar_document_identity_view(&self) -> Option<Retained<ToolbarDocumentIdentityView>> {
        self.ivars().toolbar_document_identity_view.borrow().load()
    }

    pub fn set_toolbar_document_identity_view(&self, view: Option<&ToolbarDocumentIdentityView>) {
        *self.ivars().toolbar_document_identity_view.borrow_mut() = view.map(ObjcWeak::new).unwrap_or_default();
    }

    /// `weak var toolbarFindButton`.
    pub fn toolbar_find_button(&self) -> Option<Retained<ToolbarActionButton>> {
        self.ivars().toolbar_find_button.borrow().load()
    }

    pub fn set_toolbar_find_button(&self, button: Option<&ToolbarActionButton>) {
        *self.ivars().toolbar_find_button.borrow_mut() = button.map(ObjcWeak::new).unwrap_or_default();
    }

    /// `var toolbarGlassBand`.
    pub fn toolbar_glass_band(&self) -> Option<Retained<ToolbarGlassBand>> {
        self.ivars().toolbar_glass_band.borrow().clone()
    }

    pub fn set_toolbar_glass_band(&self, band: Option<Retained<ToolbarGlassBand>>) {
        *self.ivars().toolbar_glass_band.borrow_mut() = band;
    }

    /// `weak var toolbarOverflowButton`.
    pub fn toolbar_overflow_button(&self) -> Option<Retained<ToolbarMenuButton>> {
        self.ivars().toolbar_overflow_button.borrow().load()
    }

    pub fn set_toolbar_overflow_button(&self, button: Option<&ToolbarMenuButton>) {
        *self.ivars().toolbar_overflow_button.borrow_mut() = button.map(ObjcWeak::new).unwrap_or_default();
    }

    /// `var updateStatusPill`.
    pub fn update_status_pill(&self) -> Option<Retained<UpdateStatusPill>> {
        self.ivars().update_status_pill.borrow().clone()
    }

    pub fn set_update_status_pill(&self, pill: Option<Retained<UpdateStatusPill>>) {
        *self.ivars().update_status_pill.borrow_mut() = pill;
    }

    /// `var saveRecoveryAlert`: prevents autosave, close, and task-toggle
    /// failures from stacking several identical recovery sheets.
    pub fn save_recovery_alert(&self) -> Option<Retained<NSAlert>> {
        self.ivars().save_recovery_alert.borrow().clone()
    }

    pub fn set_save_recovery_alert(&self, alert: Option<Retained<NSAlert>>) {
        *self.ivars().save_recovery_alert.borrow_mut() = alert;
    }

    /// `var isWindowPinned`.
    pub fn is_window_pinned(&self) -> bool {
        self.ivars().is_pinned.get()
    }

    // Associated-object state of the extensions.

    pub fn command_palette_state(&self) -> &RefCell<CommandPaletteState> {
        &self.ivars().command_palette_state
    }

    pub fn diagnostics_state(&self) -> &RefCell<DiagnosticsState> {
        &self.ivars().diagnostics_state
    }

    pub fn document_lens_state(&self) -> &RefCell<DocumentLensState> {
        &self.ivars().document_lens_state
    }

    pub fn local_ai_state(&self) -> &RefCell<LocalAIState> {
        &self.ivars().local_ai_state
    }

    pub fn reader_profiles_state(&self) -> &RefCell<ReaderProfilesState> {
        &self.ivars().reader_profiles_state
    }

    pub fn review_state(&self) -> &RefCell<ReviewState> {
        &self.ivars().review_state
    }

    pub fn share_state(&self) -> &RefCell<ShareState> {
        &self.ivars().share_state
    }

    pub fn speech_state(&self) -> &RefCell<SpeechState> {
        &self.ivars().speech_state
    }

    pub fn trust_state(&self) -> &RefCell<TrustState> {
        &self.ivars().trust_state
    }

    pub fn visual_debugger_state(&self) -> &RefCell<VisualDebuggerState> {
        &self.ivars().visual_debugger_state
    }

    pub fn workspace_state(&self) -> &RefCell<WorkspaceState> {
        &self.ivars().workspace_state
    }

    /// `self` as a delegate: the proxy the views hold weakly.
    pub fn delegates(&self) -> Rc<DocumentWindowControllerDelegates> {
        self.ivars().delegates.get().expect("the delegate proxy is created in init").clone()
    }

    /// `var documentPanes`.
    pub fn document_panes(&self) -> Vec<Retained<MarkdownContainerView>> {
        [self.primary_container_opt(), self.split_container()].into_iter().flatten().collect()
    }

    /// `var currentFindQuery`.
    pub fn current_find_query(&self) -> FindQuery {
        self.find_session().borrow().query().clone()
    }
}
