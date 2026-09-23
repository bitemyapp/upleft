//! `// MARK: - Construction` and `// MARK: - Interface` of
//! `DocumentWindowController.swift`: `init()`, `makeStyleSheet`,
//! `renderConfiguration`, `buildInterface`, `installFloatingOverlayHost`,
//! `installFloatingSurfaceRetargeting`, `buildToolbar`, `wireDocument`,
//! `observeTheme`, the lazy gesture coordinators, and `deinit`.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::ptr::NonNull;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use block2::RcBlock;
use dispatch2::{DispatchQoS, DispatchQueue, DispatchQueueAttr};
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, available, msg_send, sel};
use objc2_app_kit::{
    NSAppearance, NSApplication, NSBackingStoreType, NSGlassEffectContainerView, NSLayoutConstraint, NSScreen,
    NSSplitViewController, NSSplitViewItem, NSStackView, NSStackViewDistribution, NSTitlebarSeparatorStyle,
    NSToolbar, NSToolbarDisplayMode, NSToolbarSizeMode, NSUserInterfaceLayoutOrientation, NSView, NSViewController,
    NSWindowDidResizeNotification, NSWindowStyleMask, NSWindowTabbingMode, NSWindowTitleVisibility,
    NSWindowToolbarStyle, NSLayoutAttribute,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSEdgeInsets, NSNotification, NSNotificationCenter, NSOperationQueue, NSPoint, NSSize, NSString};
use upleft_core::{DirtySet, ParsedDocument, TextEdit};
use upleft_render::appkit_compat::rect;
use upleft_render::render_contracts::{MarkdownRenderConfiguration, MarkdownRevealPolicy, RenderMode};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;
use upleft_render::view::density_gutter_view::{DensityGutterDelegate, DensityGutterView};
use upleft_render::view::markdown_container_view::MarkdownContainerView;
use upleft_render::view::markdown_text_view_delegate::{MarkdownTextViewDelegate, ScrollPosition};
use upleft_render::render_contracts::Theme;

use objc2::DefinedClass as _;
use objc2::MainThreadOnly as _;
use objc2_app_kit::NSAppearanceCustomization as _;
use upleft_render::appkit_compat::RectExt as _;
use super::{
    DocumentRootView, DocumentWindowController, DocumentWindowControllerDelegates, DocumentWindowControllerIvars,
    FloatingOverlayHostView, default_sibling_search_runner,
};
use crate::ai::markdown_document::{DocumentError, ExternalEvent, MarkdownDocument, PresentationState};
use crate::app::document_scroll_gestures::{ScrollGestureChain, ScrollGestureHandler};
use crate::app::document_window::DocumentWindow;
use crate::app::history_swipe::{self, Direction, HistorySwipeCoordinator};
use crate::app::presentation_swipe::{self, PresentationSwipeCoordinator};
use crate::app::scroll_zoom::{self, ScrollZoomCoordinator};
use crate::app::themed_window_appearance::ThemedWindowAppearance;
use crate::app::toolbar_glass_band::ToolbarGlassBand;
use crate::panels::activity_indicator_view::ActivityIndicatorView;
use crate::panels::appkit_support::activate;
use crate::panels::breadcrumb_view::{BreadcrumbDelegate, BreadcrumbView};
use crate::panels::document_status_bar_view::DocumentStatusBarView;
use crate::panels::task_progress_ring::TaskProgressRing;
use crate::support::commands::Command;
use crate::support::find_engine::FindSession;
use crate::support::jump_history::JumpHistory;
use crate::support::preferences::{self, Preferences};

// MARK: - Main-thread handles for `Send` callbacks

thread_local! {
    /// Live controllers by id, main thread only: the `[weak self]` of
    /// callbacks that must be `Send` (theme observation, path warming).
    static CONTROLLERS: RefCell<HashMap<usize, ObjcWeak<DocumentWindowController>>> =
        RefCell::new(HashMap::new());
    static NEXT_CONTROLLER_ID: Cell<usize> = const { Cell::new(1) };
}

/// A `Send` stand-in for `[weak self]`, resolved on the main thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ControllerHandle(usize);

impl ControllerHandle {
    /// The controller, when called on the main thread while it is alive.
    pub(crate) fn load(self) -> Option<Retained<DocumentWindowController>> {
        MainThreadMarker::new()?;
        CONTROLLERS.with(|controllers| controllers.borrow().get(&self.0).and_then(ObjcWeak::load))
    }
}

impl DocumentWindowController {
    /// A `Send` handle on `self` for callbacks Swift closes over
    /// `[weak self]` that run on other queues before hopping to main.
    pub(crate) fn handle(&self) -> ControllerHandle {
        ControllerHandle(self.ivars().id)
    }
}

impl Drop for DocumentWindowControllerIvars {
    /// `deinit`: remove the resize observer; forget the handle.
    fn drop(&mut self) {
        if let Some(token) = self.floating_resize_token.borrow_mut().take() {
            // SAFETY: the token came from `addObserverForName:…`.
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(token.as_ref()) };
        }
        let id = self.id;
        let _ = CONTROLLERS.try_with(|controllers| {
            if let Ok(mut controllers) = controllers.try_borrow_mut() {
                controllers.remove(&id);
            }
        });
    }
}

// MARK: - Construction

impl DocumentWindowController {
    /// `private static func makeStyleSheet(theme:appearance:)`.
    pub(super) fn make_style_sheet(theme: Theme, appearance: &NSAppearance) -> Rc<StyleSheet> {
        let mut configured_theme = theme;
        configured_theme.typography = Preferences::shared().effective_typography();
        Rc::new(StyleSheet::new(configured_theme, appearance, None))
    }

    /// `private var renderConfiguration`.
    pub(super) fn render_configuration(&self) -> MarkdownRenderConfiguration {
        let values = Preferences::shared().values();
        let show_invisibles = values.show_invisibles;
        let reveal_policy = if Preferences::shared().values().reveal_markers_at_all_cursors {
            MarkdownRevealPolicy::AllCursors
        } else {
            MarkdownRevealPolicy::PrimaryCaret
        };
        MarkdownRenderConfiguration::new(
            show_invisibles,
            reveal_policy,
            Preferences::shared().values().typographic_substitution,
            Preferences::shared().values().typewriter_scrolling,
            Preferences::shared().values().reflow_hard_wrapped_paragraphs,
            Preferences::shared().values().code_block_collapse_threshold,
            Preferences::shared().values().large_file_threshold_megabytes,
        )
    }

    /// `convenience init()`.
    pub fn new(mtm: MainThreadMarker) -> Retained<DocumentWindowController> {
        let window = DocumentWindow::new(
            rect(0.0, 0.0, 1020.0, 780.0),
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
                | NSWindowStyleMask::Resizable
                | NSWindowStyleMask::FullSizeContentView,
            NSBackingStoreType::Buffered,
            false,
            mtm,
        );
        window.setTitlebarAppearsTransparent(true);
        window.setTitlebarSeparatorStyle(NSTitlebarSeparatorStyle::None);
        window.setTabbingMode(NSWindowTabbingMode::Preferred);
        // Session restoration is owned by DocumentStateStore. AppKit window
        // archives can resurrect obsolete toolbar item views across releases.
        window.setRestorable(false);
        window.setMinSize(NSSize::new(520.0, 400.0));

        // `self.init(window:)`: the stored properties' initial values, in
        // declaration order, then `super.init(window:)`.
        let markdown_document = MarkdownDocument::new(mtm);
        let breadcrumb_view = BreadcrumbView::new_current(mtm);
        let density_gutter_view = DensityGutterView::new_current(mtm);
        let activity_indicator = ActivityIndicatorView::new(mtm);
        let attribute = DispatchQueueAttr::with_qos_class(DispatchQueueAttr::SERIAL, DispatchQoS::UserInitiated, 0);
        let sibling_search_queue = DispatchQueue::new("com.bitemyapp.upleft.sibling-search", Some(&attribute));
        let resolves_path_tokens = Preferences::shared().values().resolve_path_tokens;
        let find_session = FindSession::new();
        let jump_history = JumpHistory::new();
        let active_style_sheet = Self::make_style_sheet(
            ThemeStore::shared().current(),
            &NSApplication::sharedApplication(mtm).effectiveAppearance(),
        );
        let progress_ring = TaskProgressRing::new_current(mtm);
        let id = NEXT_CONTROLLER_ID.with(|next| {
            let id = next.get();
            next.set(id + 1);
            id
        });

        let this = Self::alloc(mtm).set_ivars(DocumentWindowControllerIvars {
            id,
            markdown_document,
            mode: Cell::new(RenderMode::Live),
            on_close: RefCell::new(None),
            primary_container: RefCell::new(None),
            split_container: RefCell::new(None),
            split_view_container: RefCell::new(None),
            breadcrumb_view,
            density_gutter_view,
            status_bar_view: RefCell::new(None),
            activity_indicator,
            task_panel: RefCell::new(None),
            find_bar: RefCell::new(None),
            conflict_bar: RefCell::new(None),
            change_summary_bar: RefCell::new(None),
            change_summary_top_constraint: RefCell::new(None),
            change_summary_dismiss_work_item: RefCell::new(None),
            search_results: RefCell::new(None),
            sibling_search_generation: Cell::new(0),
            sibling_search_queue,
            sibling_search_cancellation: RefCell::new(None),
            sibling_search_runner: RefCell::new(default_sibling_search_runner()),
            search_inspector: RefCell::new(None),
            history_inspector: RefCell::new(None),
            inspector_host: RefCell::new(None),
            floating_surface: RefCell::new(None),
            floating_panel_window: RefCell::new(None),
            floating_surface_frame: Cell::new(rect(0.0, 0.0, 0.0, 0.0)),
            floating_control_anchor_frame: Cell::new(rect(0.0, 0.0, 0.0, 0.0)),
            floating_focus_restore_view: RefCell::new(ObjcWeak::default()),
            floating_presentation_needs_focus: Cell::new(false),
            floating_dismiss_viewport_repairs: RefCell::new(Vec::new()),
            floating_resize_token: RefCell::new(None),
            floating_activation_observer: RefCell::new(None),
            front_matter_editor: RefCell::new(None),
            asset_doctor_panel: RefCell::new(None),
            tidy_sheet_window: RefCell::new(None),
            table_editor_window: RefCell::new(None),
            auxiliary_windows: RefCell::new(Vec::new()),
            root_view: RefCell::new(None),
            bar_stack: RefCell::new(None),
            window_split_controller: RefCell::new(None),
            floating_overlay_host: RefCell::new(None),
            scanner: RefCell::new(None),
            path_resolver: RefCell::new(None),
            resolves_path_tokens: Cell::new(resolves_path_tokens),
            is_clearing_disabled_path_state: Cell::new(false),
            find_session: RefCell::new(find_session),
            jump_history: RefCell::new(jump_history),
            active_style_sheet: RefCell::new(active_style_sheet),
            theme_observation: RefCell::new(None),
            progress_ring,
            is_pinned: Cell::new(false),
            focus_mode_applied: Cell::new(false),
            discard_changes_on_close: Cell::new(false),
            implicit_save_suppressed: Cell::new(false),
            focus_dimming_views: RefCell::new(Vec::new()),
            is_synchronizing_panes: Cell::new(false),
            pending_initial_restore_offset: Cell::new(None),
            deferred_initial_restore_offset: Cell::new(None),
            is_opening_document: Cell::new(false),
            initial_restore_viewport_y: Cell::new(None),
            initial_restore_generation: Cell::new(0),
            pending_conflict: RefCell::new(None),
            toolbar_presentation_control: RefCell::new(ObjcWeak::default()),
            presentation_swipe: OnceCell::new(),
            scroll_zoom: OnceCell::new(),
            history_swipe: OnceCell::new(),
            document_scroll_gestures: OnceCell::new(),
            toolbar_document_identity_view: RefCell::new(ObjcWeak::default()),
            toolbar_find_button: RefCell::new(ObjcWeak::default()),
            toolbar_glass_band: RefCell::new(None),
            toolbar_overflow_button: RefCell::new(ObjcWeak::default()),
            update_status_pill: RefCell::new(None),
            last_breadcrumb_heading_index: Cell::new(isize::MIN),
            derived_ui_refresh_work_item: RefCell::new(None),
            find_refresh_work_item: RefCell::new(None),
            exiting_find_bar: RefCell::new(None),
            exiting_search_inspector: RefCell::new(None),
            find_bar_exit_generation: Cell::new(0),
            cached_metrics_document_id: Cell::new(None),
            cached_section_metrics: RefCell::new(Vec::new()),
            cached_word_count: Cell::new(0),
            autosave_work_item: RefCell::new(None),
            save_recovery_alert: RefCell::new(None),
            command_palette_state: RefCell::default(),
            diagnostics_state: RefCell::default(),
            document_lens_state: RefCell::default(),
            local_ai_state: RefCell::default(),
            reader_profiles_state: RefCell::default(),
            review_state: RefCell::default(),
            share_state: RefCell::default(),
            speech_state: RefCell::default(),
            trust_state: RefCell::default(),
            visual_debugger_state: RefCell::default(),
            workspace_state: RefCell::default(),
            delegates: OnceCell::new(),
        });
        let this: Retained<DocumentWindowController> =
            unsafe { msg_send![super(this), initWithWindow: Some(&**window)] };
        let _ = this
            .ivars()
            .delegates
            .set(Rc::new(DocumentWindowControllerDelegates { controller: ObjcWeak::from_retained(&this) }));
        CONTROLLERS.with(|controllers| controllers.borrow_mut().insert(id, ObjcWeak::from_retained(&this)));

        window.apply_theme_appearance(&ThemeStore::shared().current());
        window.setBackgroundColor(Some(&this.active_style_sheet().background));
        window.setDelegate(Some(ProtocolObject::from_ref(&*this)));
        this.build_interface();
        window.setContentSize(NSSize::new(1020.0, 728.0));
        if let Some(screen) = window.screen().or_else(|| NSScreen::mainScreen(mtm)) {
            let visible = screen.visibleFrame();
            let _ = window.cascadeTopLeftFromPoint(NSPoint::new(visible.min_x() + 80.0, visible.max_y() - 60.0));
        } else {
            window.center();
        }
        this.wire_document();
        this.observe_theme();
        this
    }

    // MARK: - Interface

    pub(super) fn build_interface(&self) {
        let mtm = self.mtm();
        let document = self.markdown_document().clone();
        let primary = MarkdownContainerView::with_storage(document.storage(), mtm);
        self.set_primary_container(Some(primary.clone()));
        let text_view_delegate: Weak<dyn MarkdownTextViewDelegate> = Rc::downgrade(&self.delegates()) as _;
        primary.text_view().set_markdown_delegate(Some(text_view_delegate));
        self.wire_key_event_handler(primary.text_view());
        // Style through the container so `scrollView.backgroundColor` follows
        // the theme — styling the text view directly left the scroll surface on
        // the fallback colour and showed a seam beside the document map (§8.6).
        let sheet = self.active_style_sheet();
        primary.set_style_sheet(sheet.clone());
        primary.set_top_accessory(Some(Retained::into_super(self.breadcrumb_view().clone())));
        // The current-section cue lives in a stable orientation lane. A
        // reader must never trade the first line of prose for navigation.
        primary.set_top_accessory_overlays_content(false);
        // The document map is navigation, not a second scrollbar. Keep it in
        // the leading lane where the outline it expands into belongs.
        primary.set_leading_accessory(Some(Retained::into_super(Retained::into_super(
            self.density_gutter_view().clone(),
        ))));
        let breadcrumb_delegate: Weak<dyn BreadcrumbDelegate> = Rc::downgrade(&self.delegates()) as _;
        self.breadcrumb_view().set_delegate(Some(breadcrumb_delegate));
        let weak = ObjcWeak::new(self);
        self.breadcrumb_view().set_on_zoom_change(Some(Rc::new(move |level| {
            if let Some(this) = weak.load() {
                this.set_shared_zoom(level);
            }
        })));
        self.breadcrumb_view().set_style_sheet(sheet.clone());
        let gutter_delegate: Weak<dyn DensityGutterDelegate> = Rc::downgrade(&self.delegates()) as _;
        self.density_gutter_view().set_delegate(Some(gutter_delegate));
        self.density_gutter_view().set_style_sheet(sheet.clone());
        self.progress_ring().set_style_sheet(sheet.clone());
        if let Some(control) = self.toolbar_presentation_control() {
            control.set_style_sheet(sheet.clone());
        }

        let root_view = DocumentRootView::new(&sheet.background, mtm);
        let root: Retained<NSView> = Retained::into_super(root_view);
        *self.ivars().root_view.borrow_mut() = Some(root.clone());
        let toolbar_glass_band = ToolbarGlassBand::new(sheet.clone(), mtm);
        self.set_toolbar_glass_band(Some(toolbar_glass_band.clone()));
        let bar_stack = NSStackView::new(mtm);
        self.set_bar_stack(Some(bar_stack.clone()));
        bar_stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        bar_stack.setSpacing(0.0);
        bar_stack.setDistribution(NSStackViewDistribution::Fill);
        bar_stack.setAlignment(NSLayoutAttribute::CenterX);

        let status_bar_view = DocumentStatusBarView::new(sheet.clone(), mtm);
        self.set_status_bar_view(Some(status_bar_view.clone()));
        status_bar_view.set_is_visible(Preferences::shared().values().show_status_bar);

        let views: [&NSView; 4] = [&toolbar_glass_band, &primary, &bar_stack, &status_bar_view];
        for view in views {
            view.setTranslatesAutoresizingMaskIntoConstraints(false);
            root.addSubview(view);
        }
        // Keep transient bars (conflict, find) off the very top edge so a
        // centred find bar does not feel nailed to the toolbar.
        bar_stack.setEdgeInsets(NSEdgeInsets { top: 14.0, left: 0.0, bottom: 0.0, right: 0.0 });

        let document_controller = NSViewController::new(mtm);
        document_controller.setView(&root);

        // The document is the only split pane. Inspector sections are floating
        // surfaces now; retaining a collapsed inspector item would leave a
        // second presentation system and keep resize state alive for dead UI.
        let split = NSSplitViewController::new(mtm);
        let document_item = NSSplitViewItem::splitViewItemWithViewController(&document_controller);
        split.addSplitViewItem(&document_item);
        *self.ivars().window_split_controller.borrow_mut() = Some(split.clone());
        if let Some(window) = self.window() {
            window.setContentViewController(Some(&split));
        }
        // Layer 3: the split and the floating lane share one window stage.
        // The surface itself is never mounted on the split view.
        self.install_floating_overlay_host();
        self.install_floating_surface_retargeting();

        let safe_top = root.safeAreaLayoutGuide().topAnchor();
        activate(&[
            primary.leadingAnchor().constraintEqualToAnchor(&root.leadingAnchor()),
            primary.trailingAnchor().constraintEqualToAnchor(&root.trailingAnchor()),
            primary.topAnchor().constraintEqualToAnchor(&bar_stack.bottomAnchor()),
            primary.bottomAnchor().constraintEqualToAnchor(&status_bar_view.topAnchor()),
            status_bar_view.leadingAnchor().constraintEqualToAnchor(&primary.leadingAnchor()),
            status_bar_view.trailingAnchor().constraintEqualToAnchor(&primary.trailingAnchor()),
            status_bar_view.bottomAnchor().constraintEqualToAnchor(&root.bottomAnchor()),
            bar_stack.leadingAnchor().constraintEqualToAnchor(&primary.leadingAnchor()),
            bar_stack.trailingAnchor().constraintEqualToAnchor(&primary.trailingAnchor()),
            bar_stack.topAnchor().constraintEqualToAnchor(&safe_top),
            toolbar_glass_band.leadingAnchor().constraintEqualToAnchor(&root.leadingAnchor()),
            toolbar_glass_band.trailingAnchor().constraintEqualToAnchor(&root.trailingAnchor()),
            toolbar_glass_band.topAnchor().constraintEqualToAnchor(&root.topAnchor()),
            toolbar_glass_band.bottomAnchor().constraintEqualToAnchor(&root.safeAreaLayoutGuide().topAnchor()),
        ]);

        self.build_toolbar();
    }

    // MARK: Morph chip: Layer 3 container

    /// Builds the window's two drawing lanes without putting glass on the
    /// split view itself. The split controller remains a real child
    /// controller, so AppKit still owns pane layout; the overlay is its
    /// sibling in this root.
    fn install_floating_overlay_host(&self) {
        let mtm = self.mtm();
        let Some(host_window) = self.window() else { return };
        let Some(split) = self.ivars().window_split_controller.borrow().clone() else { return };

        let stage = NSView::new(mtm);
        stage.setWantsLayer(false);
        let split_view = split.view();
        split_view.setTranslatesAutoresizingMaskIntoConstraints(false);
        let overlay = FloatingOverlayHostView::new(mtm);
        overlay.setWantsLayer(false);
        overlay.setTranslatesAutoresizingMaskIntoConstraints(false);

        let holder = NSViewController::new(mtm);
        holder.setView(&stage);
        holder.addChildViewController(&split);
        *self.ivars().floating_overlay_host.borrow_mut() = Some(overlay.clone());

        if available!(macos = 26.0) {
            // Document pixels and floating glass share one native container.
            // The overlay remains inside its content host so AppKit composes
            // NSGlassEffectView against the document below it.
            let glass_stage = NSView::new(mtm);
            glass_stage.addSubview(&split_view);
            glass_stage.addSubview(&overlay);
            activate(&[
                split_view.leadingAnchor().constraintEqualToAnchor(&glass_stage.leadingAnchor()),
                split_view.trailingAnchor().constraintEqualToAnchor(&glass_stage.trailingAnchor()),
                split_view.topAnchor().constraintEqualToAnchor(&glass_stage.topAnchor()),
                split_view.bottomAnchor().constraintEqualToAnchor(&glass_stage.bottomAnchor()),
                overlay.leadingAnchor().constraintEqualToAnchor(&glass_stage.leadingAnchor()),
                overlay.trailingAnchor().constraintEqualToAnchor(&glass_stage.trailingAnchor()),
                overlay.topAnchor().constraintEqualToAnchor(&glass_stage.topAnchor()),
                overlay.bottomAnchor().constraintEqualToAnchor(&glass_stage.bottomAnchor()),
            ]);
            let container = NSGlassEffectContainerView::new(mtm);
            container.setTranslatesAutoresizingMaskIntoConstraints(false);
            container.setContentView(Some(&glass_stage));
            stage.addSubview(&container);
            activate(&[
                container.leadingAnchor().constraintEqualToAnchor(&stage.leadingAnchor()),
                container.trailingAnchor().constraintEqualToAnchor(&stage.trailingAnchor()),
                container.topAnchor().constraintEqualToAnchor(&stage.topAnchor()),
                container.bottomAnchor().constraintEqualToAnchor(&stage.bottomAnchor()),
            ]);
        } else {
            stage.addSubview(&split_view);
            stage.addSubview(&overlay);
            activate(&[
                split_view.leadingAnchor().constraintEqualToAnchor(&stage.leadingAnchor()),
                split_view.trailingAnchor().constraintEqualToAnchor(&stage.trailingAnchor()),
                split_view.topAnchor().constraintEqualToAnchor(&stage.topAnchor()),
                split_view.bottomAnchor().constraintEqualToAnchor(&stage.bottomAnchor()),
                overlay.leadingAnchor().constraintEqualToAnchor(&stage.leadingAnchor()),
                overlay.trailingAnchor().constraintEqualToAnchor(&stage.trailingAnchor()),
                overlay.topAnchor().constraintEqualToAnchor(&stage.topAnchor()),
                overlay.bottomAnchor().constraintEqualToAnchor(&stage.bottomAnchor()),
            ]);
        }
        host_window.setContentViewController(Some(&holder));
    }

    /// Keep the real floating surface fitted while its window resizes.
    fn install_floating_surface_retargeting(&self) {
        let Some(window) = self.window() else { return };
        let weak = ObjcWeak::new(self);
        let block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
            if let Some(this) = weak.load() {
                this.refit_floating_surface(true);
            }
        });
        // SAFETY: the block runs on the main queue, where the controller
        // lives; the token is removed in `deinit`.
        let token = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(NSWindowDidResizeNotification),
                Some(&window),
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
        };
        *self.ivars().floating_resize_token.borrow_mut() = Some(token);
    }

    fn build_toolbar(&self) {
        let mtm = self.mtm();
        // Keep the document switch in the optical centre with explicit flexible
        // spaces. AppKit then owns hit testing and the layout stays stable when
        // a toolbar item is hidden or the window gets narrower.
        let toolbar =
            NSToolbar::initWithIdentifier(NSToolbar::alloc(mtm), &NSString::from_str("DownrightToolbar.v11"));
        toolbar.setDelegate(Some(ProtocolObject::from_ref(self)));
        toolbar.setDisplayMode(NSToolbarDisplayMode::IconOnly);
        toolbar.setSizeMode(NSToolbarSizeMode::Regular);
        toolbar.setCenteredItemIdentifier(Some(&NSString::from_str(Self::MODE_ITEM)));
        toolbar.setAllowsUserCustomization(false);
        toolbar.setAutosavesConfiguration(false);
        toolbar.setVisible(true);
        if let Some(window) = self.window() {
            window.setToolbar(Some(&toolbar));
        }
        if let Some(window) = self.window() {
            window.setToolbarStyle(NSWindowToolbarStyle::Unified);
        }
        // The identity item *is* the title, so AppKit must not draw a second
        // one behind it.
        if let Some(window) = self.window() {
            window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        }
    }

    fn wire_document(&self) {
        let document = self.markdown_document();
        let weak = ObjcWeak::new(self);
        document.set_on_will_apply_edits(Some(move |edits: &[TextEdit]| {
            let Some(this) = weak.load() else { return };
            for pane in this.document_panes() {
                pane.text_view().prepare_for_external_document_edits(edits);
            }
        }));
        let weak = ObjcWeak::new(self);
        document.set_on_reparse(Some(move |parsed: &Arc<ParsedDocument>, dirty: &DirtySet| {
            let Some(this) = weak.load() else { return };
            this.document_did_reparse(parsed, dirty);
        }));
        let weak = ObjcWeak::new(self);
        document.set_on_parse_activity(Some(move |busy: bool| {
            let Some(this) = weak.load() else { return };
            if busy {
                this.activity_indicator().begin();
            } else {
                this.activity_indicator().end();
            }
        }));
        let weak = ObjcWeak::new(self);
        document.set_on_external_event(Some(move |event: &ExternalEvent| {
            if let Some(this) = weak.load() {
                this.handle_external_event(event);
            }
        }));
        let weak = ObjcWeak::new(self);
        document.set_on_dirty_changed(Some(move |dirty: bool| {
            if let Some(this) = weak.load()
                && let Some(window) = this.window()
            {
                window.setDocumentEdited(dirty);
            }
            if dirty && let Some(this) = weak.load() {
                this.schedule_autosave();
            }
        }));
        let weak = ObjcWeak::new(self);
        document.set_on_presentation_state_changed(Some(move |state: &PresentationState| {
            if let Some(this) = weak.load()
                && let Some(view) = this.toolbar_document_identity_view()
            {
                view.set_document_state(state.clone());
            }
        }));
        let weak = ObjcWeak::new(self);
        document.set_on_will_apply_undo_redo(Some(move || {
            let Some(this) = weak.load() else { return };
            for pane in this.document_panes() {
                pane.text_view().preserve_viewport_across_undo_redo();
            }
            // Undoing a tick or a drag reshapes the list the same way the
            // forward edit did.
            this.refit_floating_surface_after_content_change();
        }));
        let weak = ObjcWeak::new(self);
        document.set_on_save_failure(Some(move |error: &DocumentError| {
            if let Some(this) = weak.load() {
                this.present_save_error(error);
            }
        }));
        // The reading position belongs to whichever pane the reader is in, not
        // always the primary one — in split view the anchor used to be captured
        // from and restored to the other pane.
        let weak = ObjcWeak::new(self);
        document.set_current_top_offset_provider(Some(move || {
            weak.load().map(|this| this.container_text_view().top_visible_offset()).unwrap_or(0)
        }));
        let weak = ObjcWeak::new(self);
        document.set_restore_offset_handler(Some(move |offset: isize| {
            let Some(this) = weak.load() else { return };
            for pane in this.document_panes() {
                pane.text_view().scroll_to_offset(offset, ScrollPosition::Top, false);
            }
        }));
        // An external write replaces the whole buffer, which drops the
        // selection; put it back on the same text rather than at the same
        // offset, since the offsets have moved.
        let weak = ObjcWeak::new(self);
        document.set_current_selection_provider(Some(move || {
            weak.load()
                .map(|this| this.container_text_view().source_selected_range())
                .unwrap_or(upleft_core::NSRange { location: 0, length: 0 })
        }));
        let weak = ObjcWeak::new(self);
        document.set_restore_selection_handler(Some(move |range: upleft_core::NSRange| {
            if let Some(this) = weak.load() {
                this.container_text_view().set_source_selected_ranges(&[range]);
            }
        }));
        let weak = ObjcWeak::new(self);
        document.set_on_external_write_activity(Some(move |busy: bool| {
            let Some(this) = weak.load() else { return };
            if busy {
                this.activity_indicator().begin();
            } else {
                this.activity_indicator().end();
            }
        }));
        let weak = ObjcWeak::new(self);
        document.set_on_file_renamed(Some(move |new_url: &upleft_foundation::url::FileUrl| {
            if let Some(this) = weak.load() {
                this.adopt_renamed_file(new_url);
            }
        }));
        let weak = ObjcWeak::new(self);
        document.changes().set_on_change(Some(Box::new(move || {
            if let Some(this) = weak.load() {
                this.refresh_change_decorations();
            }
        })));
    }

    /// `markdownDocument.onReparse`.
    fn document_did_reparse(&self, parsed: &Arc<ParsedDocument>, dirty: &DirtySet) {
        self.primary_container().text_view().update(parsed.clone(), dirty, true);
        if let Some(split) = self.split_container() {
            split.text_view().update(parsed.clone(), dirty, true);
        }
        let primary_text_view = self.primary_container().text_view().clone();
        self.synchronize_panes(&primary_text_view, false, false);
        if Preferences::shared().values().resolve_path_tokens {
            if let Some(resolver) = self.path_resolver() {
                let tokens: Vec<_> = parsed.path_tokens.iter().map(|token| token.token.clone()).collect();
                let handle = self.handle();
                resolver.warm(&tokens, move || {
                    let Some(this) = handle.load() else { return };
                    // Split panes share attributed storage. One bounded refresh
                    // updates both without duplicating the attribute walk.
                    for pane in this.document_panes() {
                        pane.text_view().invalidate_path_existence_cache();
                    }
                    this.primary_container().text_view().refresh_path_existence(None);
                    if let Some(split) = this.split_container() {
                        split.text_view().setNeedsDisplay(true);
                    }
                });
            }
        } else if self.ivars().is_clearing_disabled_path_state.get() {
            self.clear_disabled_path_state();
        }
        self.schedule_derived_ui_refresh(false);
        self.schedule_find_refresh();
    }

    pub(super) fn observe_theme(&self) {
        let handle = self.handle();
        let observation = ThemeStore::shared().observe(move |theme: &Theme| {
            let Some(this) = handle.load() else { return };
            let Some(window) = this.window() else { return };
            window.apply_theme_appearance(theme);
            this.set_active_style_sheet(Self::make_style_sheet(theme.clone(), &window.effectiveAppearance()));
            this.apply_style_sheet();
        });
        *self.ivars().theme_observation.borrow_mut() = Some(observation);
        let center = NSNotificationCenter::defaultCenter();
        // SAFETY: the selectors are this class's own no-argument methods; the
        // observer is removed in `documentWillClose`.
        unsafe {
            center.addObserver_selector_name_object(
                self,
                sel!(preferencesDidChange),
                Some(&NSString::from_str(preferences::DID_CHANGE)),
                None,
            );
            // Registered with the default centre, as Swift does (NSWorkspace
            // posts this on its own centre).
            center.addObserver_selector_name_object(
                self,
                sel!(accessibilityDisplayOptionsDidChange),
                Some(objc2_app_kit::NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
                None,
            );
        }
    }

    // MARK: Lazy gesture coordinators

    /// `lazy var presentationSwipe`: the two-finger Document↔Source swipe
    /// over the document surface.
    pub fn presentation_swipe(&self) -> Rc<PresentationSwipeCoordinator> {
        self.ivars()
            .presentation_swipe
            .get_or_init(|| {
                let panes = ObjcWeak::new(self);
                let style = ObjcWeak::new(self);
                let segment = ObjcWeak::new(self);
                let commit = ObjcWeak::new(self);
                let track = ObjcWeak::new(self);
                let settle = ObjcWeak::new(self);
                let lines = ObjcWeak::new(self);
                let set_segment = ObjcWeak::new(self);
                let mtm = self.mtm();
                PresentationSwipeCoordinator::new(presentation_swipe::Host {
                    panes: Box::new(move || panes.load().map(|this| this.document_panes()).unwrap_or_default()),
                    style_sheet: Box::new(move || {
                        style
                            .load()
                            .map(|this| this.active_style_sheet())
                            .unwrap_or_else(|| Rc::new(StyleSheet::current(mtm)))
                    }),
                    selected_segment: Box::new(move || {
                        segment.load().map(|this| this.presentation_segment()).unwrap_or(0)
                    }),
                    commit_segment: Box::new(move |selected| {
                        if let Some(this) = commit.load() {
                            this.change_presentation(selected);
                        }
                    }),
                    track_rail: Box::new(move |position: CGFloat| {
                        if let Some(control) = track.load().and_then(|this| this.toolbar_presentation_control()) {
                            control.track_swipe(position);
                        }
                    }),
                    settle_rail: Box::new(move |selected| {
                        if let Some(control) = settle.load().and_then(|this| this.toolbar_presentation_control()) {
                            control.settle_swipe(selected);
                        }
                    }),
                    document_lines: Box::new(move || lines.load().map(|this| this.document_line_count()).unwrap_or(0)),
                    set_segment: Box::new(move |selected| {
                        if let Some(this) = set_segment.load() {
                            this.set_presentation_segment(selected);
                        }
                    }),
                })
            })
            .clone()
    }

    /// `lazy var scrollZoom`: ⌘-scroll and ⌥-scroll over the document
    /// surface.
    pub fn scroll_zoom(&self) -> Rc<ScrollZoomCoordinator> {
        self.ivars()
            .scroll_zoom
            .get_or_init(|| {
                let text = ObjcWeak::new(self);
                let detail = ObjcWeak::new(self);
                ScrollZoomCoordinator::new(scroll_zoom::Host::new(
                    move |steps| {
                        if let Some(this) = text.load() {
                            this.adjust_text_size(steps as CGFloat);
                        }
                    },
                    move |steps| {
                        let Some(this) = detail.load() else { return };
                        if steps == 0 {
                            return;
                        }
                        // Routed through the commands rather than the zoom level,
                        // so the gesture, the chord and the View menu can never
                        // disagree about what the ends of the scale are.
                        let command = if steps > 0 { Command::ZoomIn } else { Command::ZoomOut };
                        for _ in 0..steps.unsigned_abs() {
                            this.perform(command);
                        }
                    },
                ))
            })
            .clone()
    }

    /// `lazy var historySwipe`: the ⇧ two-finger Back/Forward swipe.
    pub fn history_swipe(&self) -> Rc<HistorySwipeCoordinator> {
        self.ivars()
            .history_swipe
            .get_or_init(|| {
                let panes = ObjcWeak::new(self);
                let style = ObjcWeak::new(self);
                let can_move = ObjcWeak::new(self);
                let step = ObjcWeak::new(self);
                let mtm = self.mtm();
                HistorySwipeCoordinator::new(history_swipe::Host::new(
                    move || panes.load().map(|this| this.document_panes()).unwrap_or_default(),
                    move || {
                        style
                            .load()
                            .map(|this| this.active_style_sheet())
                            .unwrap_or_else(|| Rc::new(StyleSheet::current(mtm)))
                    },
                    move |direction| {
                        let Some(this) = can_move.load() else { return false };
                        match direction {
                            Direction::Back => this.jump_history().borrow().can_go_back(),
                            Direction::Forward => this.jump_history().borrow().can_go_forward(),
                        }
                    },
                    move |direction| match direction {
                        Direction::Back => {
                            if let Some(this) = step.load() {
                                this.go_back();
                            }
                        }
                        Direction::Forward => {
                            if let Some(this) = step.load() {
                                this.go_forward();
                            }
                        }
                    },
                ))
            })
            .clone()
    }

    /// `lazy var documentScrollGestures`: every gesture the document surface
    /// can hand a scroll event to, in the order they get to claim it.
    pub fn document_scroll_gestures(&self) -> Rc<ScrollGestureChain> {
        if let Some(chain) = self.ivars().document_scroll_gestures.get() {
            return chain.clone();
        }
        let handlers: Vec<Rc<dyn ScrollGestureHandler>> =
            vec![self.scroll_zoom(), self.history_swipe(), self.presentation_swipe()];
        self.ivars().document_scroll_gestures.get_or_init(|| Rc::new(ScrollGestureChain::new(handlers))).clone()
    }
}

#[allow(unused_imports)]
use {AnyObject as _, NSLayoutConstraint as _};
