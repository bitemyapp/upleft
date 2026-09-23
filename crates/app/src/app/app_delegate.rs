//! Port of `App/AppDelegate.swift`: the application delegate (launch, open
//! routing, session restore, the start and setup windows, Settings, themes,
//! warnings, the Dock menu, application-level commands), plus
//! `DocumentOpenDisposition`, `FileIdentity` and `WelcomeDocument`.
//!
//! Swift's `@MainActor` class is a `define_class!` `NSObject` subclass named
//! `AppDelegate`; its `@objc` menu actions keep their selectors, because the
//! main menu reaches them through the responder chain (`takeTour:`,
//! `openRecentDocument:`, `performDownrightCommand:` …).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use block2::RcBlock;
use dispatch2::MainThreadBound;
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertStyle, NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua,
    NSApplication, NSApplicationDelegate, NSApplicationTerminateReply, NSEventModifierFlags, NSMenu, NSMenuItem,
    NSMenuItemValidation, NSModalResponseOK, NSOpenPanel, NSSavePanel, NSScreen, NSWindow,
    NSWindowDidBecomeKeyNotification, NSWindowOrderingMode, NSWindowStyleMask, NSWindowWillCloseNotification,
    NSWorkspace,
};
use objc2_foundation::{
    NSArray, NSDistributedNotificationCenter, NSKeyValueChangeKey, NSKeyValueObservingOptions, NSNotification,
    NSObjectNSKeyValueObserverRegistration,
    NSNotificationCenter, NSObject, NSOperationQueue, NSPoint, NSProcessInfo, NSRect, NSSize, NSString, NSURL,
    NSUserDefaults,
};
use upleft_foundation::decodable::DecodableValue;
use upleft_foundation::json_decoder;
use upleft_foundation::json_encoder::{self, JsonValue, OutputFormatting};
use upleft_foundation::url::FileUrl;
use upleft_render::appkit_compat::{WorkItem, main_async};
use upleft_render::render_contracts::RenderMode;
use upleft_render::render_contracts::ThemeAppearance;
use upleft_render::theme::theme_store::ThemeStore;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::ai::document_state_store::{DocumentStateStore, RecentDocument};
use crate::ai::snapshot_store::SnapshotStore;
use crate::app::compare_window_controller::CompareWindowController;
use crate::app::document_types;
use crate::app::document_window_controller::DocumentWindowController;
use crate::app::main_menu::MainMenu;
use crate::app::preferences_window_controller::{PreferencesWindowController, SettingsPane};
use crate::app::setup_window_controller::SetupWindowController;
use crate::app::start_window_controller::{StartGuideOffer, StartWindowController};
use crate::integrations::native_integration::{DownrightServicesProvider, IntegrationRegistry};
use crate::integrations::spotlight_metadata::SpotlightIndexer;
use crate::support::app_paths::{self, PreparationFailure};
use crate::support::commands::{Command, CommandContext};
use crate::support::keybindings::KeybindingStore;
use crate::support::preferences::{self, Load, Preferences, ThemePreferenceSlot};
use crate::support::system_integration::SystemIntegration;
use crate::support::welcome_tour::{WelcomeTour, WelcomeTourError};
use crate::updater::update_coordinator::UpdateCoordinator;

fn ns(text: &str) -> Retained<NSString> {
    NSString::from_str(text)
}

/// `DocumentOpenDisposition`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentOpenDisposition {
    /// Join the active document window's native tab group when one exists.
    Tab,
    /// Keep the document in a separate window.
    Window,
}

/// `FileIdentity`: whether two URLs name the same file.
///
/// A case-sensitive path comparison is wrong on the case-insensitive volumes
/// most Macs use: `README.md` and `readme.md` are one file. Ask the file
/// system for identity when it can answer, and fold case when it cannot.
pub struct FileIdentity;

impl FileIdentity {
    pub fn same_file(lhs: &FileUrl, rhs: &FileUrl) -> bool {
        let left = lhs.resolving_symlinks_in_path().standardized_file_url();
        let right = rhs.resolving_symlinks_in_path().standardized_file_url();
        if let (Some(a), Some(b)) = (Self::identifier(&left), Self::identifier(&right)) {
            // `a.isEqual(b)`.
            return unsafe { msg_send![&*a, isEqual: &*b] };
        }
        // `compare(_:options: .caseInsensitive) == .orderedSame`.
        ns(&left.path()).caseInsensitiveCompare(&ns(&right.path())) == objc2_foundation::NSComparisonResult::Same
    }

    /// Inode identity, available only for a file that exists right now.
    fn identifier(url: &FileUrl) -> Option<Retained<AnyObject>> {
        let nsurl = url.to_nsurl();
        let mut value: Option<Retained<AnyObject>> = None;
        // SAFETY: `getResourceValue:forKey:error:` with an out-pointer.
        let ok = unsafe {
            nsurl.getResourceValue_forKey_error(&mut value, objc2_foundation::NSURLFileResourceIdentifierKey)
        };
        if ok.is_err() {
            return None;
        }
        value
    }
}

/// `WelcomeDocument`: the bundled tour, materialised in a temporary folder
/// and opened from there, so it stays editable without touching the
/// reader's files.
pub struct WelcomeDocument;

/// Why the tour could not be prepared, as `error.localizedDescription`.
#[derive(Debug, Clone)]
pub struct WelcomeDocumentError(pub String);

impl WelcomeDocument {
    pub fn bundled() -> Option<FileUrl> {
        let bundle = objc2_foundation::NSBundle::mainBundle();
        let url = bundle.URLForResource_withExtension(Some(&ns("Welcome")), Some(&ns("md")))?;
        FileUrl::from_nsurl(&url)
    }

    pub fn is_available() -> bool {
        Self::bundled().is_some()
    }

    pub fn materialize() -> Result<FileUrl, WelcomeDocumentError> {
        let Some(bundled) = Self::bundled() else {
            // `CocoaError(.fileNoSuchFile)`.
            return Err(WelcomeDocumentError("The file doesn’t exist.".to_owned()));
        };
        let folder = FileUrl::from_path_is_directory(&objc2_foundation::NSTemporaryDirectory().to_string(), true)
            .appending_path_component_is_directory("Upleft Tour", true);
        upleft_foundation::foundation_io::create_directory(&folder, true)
            .map_err(|error| WelcomeDocumentError(error.description))?;
        let copy = folder.appending_path_component("Welcome to Upleft.md");
        // A fresh copy every time: the tour should read the same on the
        // second visit as on the first, whatever the reader typed into it.
        let _ = upleft_foundation::file_manager::remove_item(&copy);
        let bytes = std::fs::read(bundled.path()).map_err(|error| WelcomeDocumentError(error.to_string()))?;
        let source = String::from_utf8(bytes).map_err(|_| {
            WelcomeDocumentError(
                "The file “Welcome.md” couldn’t be opened because the text encoding of its contents can’t be determined."
                    .to_owned(),
            )
        })?;
        let rendered = WelcomeTour::render_with_store(&source).map_err(|error| {
            let code = match error {
                WelcomeTourError::MalformedToken(_) => 0,
                WelcomeTourError::UnknownCommand(_) => 1,
                WelcomeTourError::MissingBinding(_) => 2,
            };
            WelcomeDocumentError(format!(
                "The operation couldn’t be completed. (DownrightApp.WelcomeTour.Error error {code}.)"
            ))
        })?;
        upleft_foundation::foundation_io::write_atomically(rendered.as_bytes(), &copy)
            .map_err(|error| WelcomeDocumentError(error.description))?;
        Ok(copy)
    }
}

/// `SessionWindow`: one window of the saved session (`session.json`).
#[derive(Debug, Clone, PartialEq)]
struct SessionWindow {
    path: String,
    frame: String,
    mode: String,
    tab_group: Option<i64>,
    tab_order: Option<i64>,
    selected_tab: Option<bool>,
    full_screen: Option<bool>,
}

impl SessionWindow {
    /// Synthesised `Encodable`: `CodingKeys` order, optionals with
    /// `encodeIfPresent`.
    fn encode(&self) -> JsonValue {
        let mut members = vec![
            ("path".to_owned(), JsonValue::String(self.path.clone())),
            ("frame".to_owned(), JsonValue::String(self.frame.clone())),
            ("mode".to_owned(), JsonValue::String(self.mode.clone())),
        ];
        JsonValue::push_if_present(&mut members, "tabGroup", self.tab_group.map(JsonValue::Int));
        JsonValue::push_if_present(&mut members, "tabOrder", self.tab_order.map(JsonValue::Int));
        JsonValue::push_if_present(&mut members, "selectedTab", self.selected_tab.map(JsonValue::Bool));
        JsonValue::push_if_present(&mut members, "fullScreen", self.full_screen.map(JsonValue::Bool));
        JsonValue::Object(members)
    }

    /// Synthesised `Decodable`.
    fn decode(value: &json_decoder::Value) -> Result<SessionWindow, json_decoder::DecodingError> {
        let keyed = value.keyed_container()?;
        Ok(SessionWindow {
            path: keyed.decode("path", |value| value.string_value())?,
            frame: keyed.decode("frame", |value| value.string_value())?,
            mode: keyed.decode("mode", |value| value.string_value())?,
            tab_group: keyed.decode_if_present("tabGroup", |value| value.int_value())?,
            tab_order: keyed.decode_if_present("tabOrder", |value| value.int_value())?,
            selected_tab: keyed.decode_if_present("selectedTab", |value| value.bool_value())?,
            full_screen: keyed.decode_if_present("fullScreen", |value| value.bool_value())?,
        })
    }
}

/// `AppDelegate.Warning`: something the user needs to know that must never
/// stop them reading.
#[derive(Debug, Clone)]
struct Warning {
    title: String,
    message: String,
}

pub struct AppDelegateIvars {
    window_controllers: RefCell<Vec<Retained<DocumentWindowController>>>,
    preferences_window: RefCell<Option<Retained<PreferencesWindowController>>>,
    start_window: RefCell<Option<Retained<StartWindowController>>>,
    setup_window: RefCell<Option<Retained<SetupWindowController>>>,
    services_provider: Retained<DownrightServicesProvider>,
    /// Set by `down --edit`; applies to the documents opened in this launch only.
    launch_mode: Cell<Option<RenderMode>>,
    /// Set by `down --line`.
    launch_line: Cell<Option<isize>>,
    /// Set by `down --review`.
    launch_review: Cell<bool>,
    /// Debug-only `--downright-demo-update` (Swift `#if DEBUG`; parsed, and
    /// inert, as in a release build).
    launch_demo_update: Cell<bool>,
    has_finished_launching: Cell<bool>,
    pending_open_urls: RefCell<Vec<FileUrl>>,
    appearance_observation: RefCell<Option<Retained<AppearanceObservation>>>,
    warning_window_observer: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
    session_save_work_item: RefCell<Option<WorkItem>>,
    pending_warnings: RefCell<Vec<Warning>>,
    is_presenting_warning: Cell<bool>,
    comparison_windows: RefCell<Vec<Retained<CompareWindowController>>>,
}

/// `AppleInterfaceThemeChangedNotification`.
const SYSTEM_THEME_CHANGED: &str = "AppleInterfaceThemeChangedNotification";

define_class!(
    // SAFETY: `init` is forwarded in `new` after the ivars are set; the
    // overrides keep AppKit's signatures.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "AppDelegate"]
    #[ivars = AppDelegateIvars]
    pub struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationWillFinishLaunching:))]
        fn application_will_finish_launching(&self, _notification: &NSNotification) {
            self.will_finish_launching();
        }

        #[unsafe(method(applicationDidFinishLaunching:))]
        fn application_did_finish_launching(&self, _notification: &NSNotification) {
            self.did_finish_launching();
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn application_should_terminate_after_last_window_closed(&self, _sender: &NSApplication) -> bool {
            false
        }

        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn application_should_handle_reopen(&self, _sender: &NSApplication, has_visible_windows: bool) -> bool {
            if !has_visible_windows && self.ivars().window_controllers.borrow().is_empty() {
                self.show_start_window();
            }
            true
        }

        #[unsafe(method(applicationWillTerminate:))]
        fn application_will_terminate(&self, _notification: &NSNotification) {
            self.will_terminate();
        }

        #[unsafe(method(applicationShouldTerminate:))]
        fn application_should_terminate(&self, _sender: &NSApplication) -> NSApplicationTerminateReply {
            self.should_terminate()
        }

        #[unsafe(method(application:openURLs:))]
        fn application_open_urls(&self, _application: &NSApplication, urls: &NSArray<NSURL>) {
            let urls: Vec<FileUrl> = urls.iter().filter_map(|url| FileUrl::from_nsurl(&url)).collect();
            if !self.ivars().has_finished_launching.get() {
                self.ivars().pending_open_urls.borrow_mut().extend(urls);
                return;
            }
            for url in urls {
                self.open_default(&url);
            }
        }

        #[unsafe(method(application:openFile:))]
        fn application_open_file(&self, _sender: &NSApplication, filename: &NSString) -> objc2::runtime::Bool {
            let url = FileUrl::from_path(&filename.to_string());
            if !self.ivars().has_finished_launching.get() {
                self.ivars().pending_open_urls.borrow_mut().push(url);
                return objc2::runtime::Bool::YES;
            }
            objc2::runtime::Bool::new(self.open_default(&url).is_some())
        }

        #[unsafe(method(application:openFiles:))]
        fn application_open_files(&self, _sender: &NSApplication, filenames: &NSArray<NSString>) {
            let urls: Vec<FileUrl> = filenames.iter().map(|name| FileUrl::from_path(&name.to_string())).collect();
            if !self.ivars().has_finished_launching.get() {
                self.ivars().pending_open_urls.borrow_mut().extend(urls);
                return;
            }
            for url in urls {
                self.open_default(&url);
            }
        }

        #[unsafe(method_id(applicationDockMenu:))]
        fn application_dock_menu(&self, _sender: &NSApplication) -> Option<Retained<NSMenu>> {
            Some(self.dock_menu())
        }
    }

    unsafe impl NSMenuItemValidation for AppDelegate {
        /// The app delegate is the last responder to see a command item, so
        /// what it answers is the no-document state: Save and Print… must be
        /// disabled with only the start window up, not enabled and inert.
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, menu_item: &NSMenuItem) -> bool {
            let can = UpdateCoordinator::shared(self.mtm()).can_check_for_updates();
            MainMenu::validate(menu_item, &CommandContext::application_only(can))
        }
    }

    impl AppDelegate {
        /// Help → Take the Tour.
        #[unsafe(method(takeTour:))]
        fn take_tour(&self, _sender: Option<&AnyObject>) {
            self.open_welcome_document();
        }

        #[unsafe(method(openRecentDocument:))]
        fn open_recent_document(&self, sender: &NSMenuItem) {
            let Some(path) = sender.representedObject().and_then(|object| object.downcast::<NSString>().ok()) else {
                return;
            };
            self.open_default(&FileUrl::from_path(&path.to_string()));
        }

        #[unsafe(method(clearRecentDocuments:))]
        fn clear_recent_documents(&self, _sender: Option<&AnyObject>) {
            self.clear_recents();
        }

        #[unsafe(method(selectLightTheme:))]
        fn select_light_theme(&self, sender: &NSMenuItem) {
            Self::select_theme(sender, ThemePreferenceSlot::Light);
        }

        #[unsafe(method(selectDarkTheme:))]
        fn select_dark_theme(&self, sender: &NSMenuItem) {
            Self::select_theme(sender, ThemePreferenceSlot::Dark);
        }

        #[unsafe(method(toggleFollowSystemAppearance:))]
        fn toggle_follow_system_appearance(&self, _sender: Option<&AnyObject>) {
            Preferences::shared().update(|values| values.follows_system_appearance = !values.follows_system_appearance);
        }

        #[unsafe(method(importTheme:))]
        fn import_theme(&self, _sender: Option<&AnyObject>) {
            self.run_import_theme();
        }

        #[unsafe(method(revealThemesFolder:))]
        fn reveal_themes_folder(&self, _sender: Option<&AnyObject>) {
            let directory = app_paths::ensure(app_paths::themes_directory());
            NSWorkspace::sharedWorkspace().selectFile_inFileViewerRootedAtPath(None, &ns(&directory.path()));
        }

        #[unsafe(method(openProjectPage:))]
        fn open_project_page(&self, _sender: Option<&AnyObject>) {
            let Some(url) = NSURL::URLWithString(&ns("https://github.com/bitemyapp/upleft")) else { return };
            NSWorkspace::sharedWorkspace().openURL(&url);
        }

        #[unsafe(method(showPreferences:))]
        fn show_preferences_action(&self, _sender: Option<&AnyObject>) {
            self.show_preferences(None);
        }

        #[unsafe(method(systemAppearanceDidChange:))]
        fn system_appearance_did_change(&self, _notification: &NSNotification) {
            if !Preferences::shared().values().follows_system_appearance {
                return;
            }
            NSApplication::sharedApplication(self.mtm()).setAppearance(None);
            self.apply_selected_theme();
        }

        #[unsafe(method(preferencesDidChange))]
        fn preferences_did_change(&self) {
            self.apply_selected_theme();
            if let Some(menu) = NSApplication::sharedApplication(self.mtm()).mainMenu() {
                MainMenu::refresh_key_equivalents(&menu);
            }
        }

        #[unsafe(method(comparisonWindowWillClose:))]
        fn comparison_window_will_close(&self, notification: &NSNotification) {
            let Some(window) = notification.object().and_then(|object| object.downcast::<NSWindow>().ok()) else {
                return;
            };
            unsafe {
                NSNotificationCenter::defaultCenter().removeObserver_name_object(
                    self,
                    Some(NSWindowWillCloseNotification),
                    Some(&window),
                );
            }
            self.ivars()
                .comparison_windows
                .borrow_mut()
                .retain(|controller| controller.window().as_deref() != Some(&*window));
        }

        /// `CommandResponder`.
        #[unsafe(method(performDownrightCommand:))]
        fn perform_downright_command(&self, sender: Option<&AnyObject>) {
            let Some(item) = sender.and_then(|sender| sender.downcast_ref::<NSMenuItem>()) else { return };
            let Some(command) = MainMenu::command(item) else { return };
            if self.handle_application_command(command) {
                return;
            }
            let Some(window) = self.active_document_window() else { return };
            let Some(controller) = Self::document_controller(&window) else { return };
            let _ = controller.perform(command);
        }
    }
);

impl AppDelegate {
    pub fn new(mtm: MainThreadMarker) -> Retained<AppDelegate> {
        let this = Self::alloc(mtm).set_ivars(AppDelegateIvars {
            window_controllers: RefCell::new(Vec::new()),
            preferences_window: RefCell::new(None),
            start_window: RefCell::new(None),
            setup_window: RefCell::new(None),
            services_provider: DownrightServicesProvider::new(mtm),
            launch_mode: Cell::new(None),
            launch_line: Cell::new(None),
            launch_review: Cell::new(false),
            launch_demo_update: Cell::new(false),
            has_finished_launching: Cell::new(false),
            pending_open_urls: RefCell::new(Vec::new()),
            appearance_observation: RefCell::new(None),
            warning_window_observer: RefCell::new(None),
            session_save_work_item: RefCell::new(None),
            pending_warnings: RefCell::new(Vec::new()),
            is_presenting_warning: Cell::new(false),
            comparison_windows: RefCell::new(Vec::new()),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn weak(&self) -> Weak<AppDelegate> {
        Weak::from(self)
    }

    // MARK: - Lifecycle

    fn will_finish_launching(&self) {
        let mtm = self.mtm();
        self.ivars().has_finished_launching.set(false);
        self.report_unavailable_storage(app_paths::prepare_all());
        // Counted before anything can read it, so first-run affordances all
        // agree about which launch this is.
        Preferences::shared().update(|values| values.launch_count += 1);
        self.parse_launch_arguments();
        NSApplication::sharedApplication(mtm).setMainMenu(Some(&MainMenu::build(mtm)));
        self.apply_selected_theme();
        let weak = self.weak();
        IntegrationRegistry::shared(mtm).set_open_handler(Some(Rc::new(move |url: &FileUrl| {
            if let Some(this) = weak.load() {
                let _ = this.open_default(url);
            }
        })));
        // SAFETY: registers this process's services provider under its name.
        unsafe {
            objc2_app_kit::NSRegisterServicesProvider(Some(&self.ivars().services_provider), &ns("Upleft"));
        }
    }

    fn did_finish_launching(&self) {
        let mtm = self.mtm();
        // Start the Sparkle updater after launch, as the spec requires. Dev
        // bundles without the Sparkle Info.plist block make this a no-op.
        UpdateCoordinator::shared(mtm).start();
        // `--downright-demo-update` is `#if DEBUG` in Swift; release builds
        // never present the demo pill.
        let _ = self.ivars().launch_demo_update.get();
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                self,
                sel!(preferencesDidChange),
                Some(&ns(preferences::DID_CHANGE)),
                None,
            );
        }
        // "Follow system appearance" has to mean *while running*.
        let observation = AppearanceObservation::observe(self, mtm);
        *self.ivars().appearance_observation.borrow_mut() = Some(observation);
        // The distributed system event is the authoritative edge when macOS
        // changes Light/Dark mode.
        unsafe {
            NSDistributedNotificationCenter::defaultCenter().addObserver_selector_name_object(
                self,
                sel!(systemAppearanceDidChange:),
                Some(&ns(SYSTEM_THEME_CHANGED)),
                None,
            );
        }
        let weak = MainThreadBound::new(self.weak(), mtm);
        Preferences::shared().set_on_load_fault(Some(Box::new(move |load: &Load| {
            let Load::Recovered { backup } = load else { return };
            let Some(mtm) = MainThreadMarker::new() else { return };
            if let Some(this) = weak.get(mtm).load() {
                this.report_settings_recovery(backup.as_ref());
            }
        })));
        let weak = std::sync::Arc::new(MainThreadBound::new(self.weak(), mtm));
        Preferences::shared().set_on_persistence_failure(Some(Box::new(move |error: &str| {
            let error = error.to_owned();
            let weak = weak.clone();
            dispatch2::DispatchQueue::main().exec_async(move || {
                let Some(mtm) = MainThreadMarker::new() else { return };
                if let Some(this) = weak.get(mtm).load() {
                    this.report_settings_write_failure(&error);
                }
            });
        })));
        // A launch-time fault can be queued before any application window is
        // visible. Retry when the first window appears.
        let weak = self.weak();
        let block = RcBlock::new(move |_notification: std::ptr::NonNull<NSNotification>| {
            let weak = weak.clone();
            main_async(move || {
                if let Some(this) = weak.load() {
                    this.flush_warnings();
                }
            });
        });
        let observer = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(NSWindowDidBecomeKeyNotification),
                None,
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
        };
        *self.ivars().warning_window_observer.borrow_mut() = Some(observer);
        // The store refuses to overwrite a file it could not read, so the
        // user is the only one who can fix it.
        let weak = MainThreadBound::new(self.weak(), mtm);
        KeybindingStore::shared().set_on_load_failure(Some(Box::new(move |error| {
            let Some(mtm) = MainThreadMarker::new() else { return };
            if let Some(this) = weak.get(mtm).load() {
                this.report_keybindings_load_failure(&error.localized_description());
            }
        })));
        // History pruning at launch rather than on a timer (§8.3).
        SnapshotStore::shared().schedule_prune();

        self.ivars().has_finished_launching.set(true);

        // The one-time setup panel.
        let showing_setup = self.present_setup_if_needed();

        let finish = |this: &AppDelegate| {
            this.flush_warnings();
            // The panel is re-raised once the rest of the launch has settled.
            if showing_setup {
                let setup = this.ivars().setup_window.borrow().clone();
                if let Some(window) = setup.and_then(|setup| setup.window()) {
                    window.makeKeyAndOrderFront(None);
                }
            }
        };
        if self.drain_pending_open_urls() || !self.ivars().window_controllers.borrow().is_empty() {
            finish(self);
            return;
        }
        // Paths on the command line, for running straight out of the build
        // directory during development.
        if self.open_command_line_files() {
            finish(self);
            return;
        }
        if Preferences::shared().values().restore_session && self.restore_session() {
            finish(self);
            return;
        }
        self.show_start_window();
        finish(self);
    }

    fn open_command_line_files(&self) -> bool {
        let mut opened = 0;
        let arguments: Vec<String> =
            NSProcessInfo::processInfo().arguments().iter().skip(1).map(|argument| argument.to_string()).collect();
        let mut index = 0;
        while index < arguments.len() {
            let argument = &arguments[index];
            // `-NSDocumentRevisionsDebugMode YES` and friends arrive here too.
            if argument.starts_with('-') {
                if argument == "--mode" || argument == "--downright-line" {
                    index += 2;
                } else {
                    index += 1;
                }
                continue;
            }
            let url = FileUrl::from_path(argument).standardized_file_url();
            if !upleft_foundation::foundation_io::file_exists(&url.path()) {
                index += 1;
                continue;
            }
            let line = self.ivars().launch_line.get();
            let review = self.ivars().launch_review.get();
            if self
                .open(&url, self.ivars().launch_mode.get(), line, review, DocumentOpenDisposition::Tab, None)
                .is_some()
            {
                opened += 1;
            }
            index += 1;
        }
        opened > 0
    }

    fn will_terminate(&self) {
        if let Some(observer) = self.ivars().warning_window_observer.borrow_mut().take() {
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(observer.as_ref()) };
        }
        unsafe {
            NSDistributedNotificationCenter::defaultCenter().removeObserver_name_object(
                self,
                Some(&ns(SYSTEM_THEME_CHANGED)),
                None,
            );
            objc2_app_kit::NSUnregisterServicesProvider(&ns("Upleft"));
        }
        self.save_session();
        let controllers = self.ivars().window_controllers.borrow().clone();
        for controller in controllers {
            let _ = controller.document_will_close();
        }
    }

    fn should_terminate(&self) -> NSApplicationTerminateReply {
        // One policy for unsaved work: ask. A failed save still cancels
        // termination, or macOS tears the process down after the alert and
        // the buffer is lost.
        let controllers = self.ivars().window_controllers.borrow().clone();
        for controller in controllers {
            if !controller.markdown_document().is_dirty() {
                continue;
            }
            if let Some(window) = controller.window() {
                window.makeKeyAndOrderFront(None);
            }
            if !controller.confirm_pending_changes_before_close(true) {
                return NSApplicationTerminateReply::TerminateCancel;
            }
        }
        NSApplicationTerminateReply::TerminateNow
    }

    fn parse_launch_arguments(&self) {
        let arguments: Vec<String> =
            NSProcessInfo::processInfo().arguments().iter().map(|argument| argument.to_string()).collect();
        if let Some(index) = arguments.iter().position(|argument| argument == "--mode")
            && index + 1 < arguments.len()
        {
            self.ivars().launch_mode.set(RenderMode::from_raw_value(&arguments[index + 1]));
        }
        if let Some(index) = arguments.iter().position(|argument| argument == "--downright-line")
            && index + 1 < arguments.len()
        {
            self.ivars().launch_line.set(swift_int(&arguments[index + 1]));
        }
        self.ivars().launch_review.set(arguments.iter().any(|argument| argument == "--downright-review"));
        self.ivars().launch_demo_update.set(arguments.iter().any(|argument| argument == "--downright-demo-update"));
    }

    // MARK: - Opening

    fn drain_pending_open_urls(&self) -> bool {
        let urls = std::mem::take(&mut *self.ivars().pending_open_urls.borrow_mut());
        if urls.is_empty() {
            return false;
        }
        for url in urls {
            self.open_default(&url);
        }
        true
    }

    /// `open(_:)` with every default argument.
    pub fn open_default(&self, url: &FileUrl) -> Option<Retained<DocumentWindowController>> {
        self.open(url, None, None, false, DocumentOpenDisposition::Tab, None)
    }

    /// `open(_:mode:line:review:disposition:tabbingWith:)`.
    pub fn open(
        &self,
        url: &FileUrl,
        mode: Option<RenderMode>,
        line: Option<isize>,
        review: bool,
        disposition: DocumentOpenDisposition,
        explicit_host: Option<&NSWindow>,
    ) -> Option<Retained<DocumentWindowController>> {
        let mtm = self.mtm();
        // One window per file.
        let existing = self
            .ivars()
            .window_controllers
            .borrow()
            .iter()
            .find(|controller| {
                controller.markdown_document().url().is_some_and(|open| FileIdentity::same_file(&open, url))
            })
            .cloned();
        if let Some(existing) = existing {
            unsafe { existing.showWindow(None) };
            if let Some(window) = existing.window() {
                window.makeKeyAndOrderFront(None);
            }
            self.dismiss_start_window();
            existing.apply_command_line_open(line, review);
            return Some(existing);
        }

        let tab_host: Option<Retained<NSWindow>> = match disposition {
            DocumentOpenDisposition::Tab => {
                explicit_host.map(|host| host.retain()).or_else(|| self.active_document_window())
            }
            DocumentOpenDisposition::Window => None,
        };
        let controller = DocumentWindowController::new(mtm);
        let requested = mode
            .or(self.ivars().launch_mode.get())
            .unwrap_or_else(|| Preferences::shared().values().default_mode);
        if let Err(error) = controller.open(url, requested) {
            self.present_open_failure(&error.localized_description(), url);
            // Keep or restore the start window when nothing else is open.
            if self.ivars().window_controllers.borrow().is_empty() {
                self.show_start_window();
            }
            return None;
        }
        self.adopt(&controller);
        SpotlightIndexer::index_opened_document(url);
        self.dismiss_start_window();
        unsafe { controller.showWindow(None) };
        if let (Some(tab_host), Some(window)) = (tab_host, controller.window())
            && !std::ptr::eq(&*tab_host, &*window)
        {
            tab_host.addTabbedWindow_ordered(&window, NSWindowOrderingMode::Above);
        }
        if let Some(window) = controller.window() {
            window.makeKeyAndOrderFront(None);
        }
        controller.apply_command_line_open(line, review);
        Some(controller)
    }

    fn document_window_owning(&self, window: &NSWindow) -> Option<Retained<NSWindow>> {
        let mut candidate: Option<Retained<NSWindow>> = Some(window.retain());
        while let Some(current) = candidate {
            if self
                .ivars()
                .window_controllers
                .borrow()
                .iter()
                .any(|controller| controller.window().is_some_and(|window| std::ptr::eq(&*window, &*current)))
            {
                return Some(current);
            }
            candidate = current.parentWindow();
        }
        None
    }

    fn active_document_window(&self) -> Option<Retained<NSWindow>> {
        let app = NSApplication::sharedApplication(self.mtm());
        if let Some(key) = app.keyWindow()
            && let Some(document) = self.document_window_owning(&key)
        {
            return Some(document);
        }
        self.ivars()
            .window_controllers
            .borrow()
            .iter()
            .filter_map(|controller| controller.window())
            .filter(|window| window.isVisible())
            .last()
    }

    fn document_controller(window: &NSWindow) -> Option<Retained<DocumentWindowController>> {
        window.windowController().and_then(|controller| controller.downcast::<DocumentWindowController>().ok())
    }

    pub fn adopt(&self, controller: &DocumentWindowController) {
        self.ivars().window_controllers.borrow_mut().push(controller.retain());
        let weak_self = self.weak();
        let weak_controller: Weak<DocumentWindowController> = Weak::from(controller);
        controller.set_on_close(Some(Rc::new(move || {
            let (Some(this), Some(controller)) = (weak_self.load(), weak_controller.load()) else { return };
            this.ivars()
                .window_controllers
                .borrow_mut()
                .retain(|candidate| !std::ptr::eq(&**candidate, &*controller));
            this.schedule_session_save();
            if this.ivars().window_controllers.borrow().is_empty() {
                this.show_start_window();
            }
        })));
    }

    fn present_open_failure(&self, description: &str, url: &FileUrl) {
        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&ns(&format!("Couldn't open {}", url.last_path_component())));
        alert.setInformativeText(&ns(description));
        alert.setAlertStyle(NSAlertStyle::Warning);
        alert.runModal();
    }

    pub fn show_open_panel(&self) {
        let mtm = self.mtm();
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setAllowsMultipleSelection(true);
        panel.setCanChooseDirectories(false);
        panel.setAllowedContentTypes(&NSArray::from_retained_slice(&document_types::content_types()));
        panel.setMessage(Some(&ns("Open a Markdown document")));
        if panel.runModal() != NSModalResponseOK {
            self.raise_start_window();
            return;
        }
        for url in panel.URLs().iter() {
            if let Some(url) = FileUrl::from_nsurl(&url) {
                self.open_default(&url);
            }
        }
    }

    fn raise_start_window(&self) {
        let start = self.ivars().start_window.borrow().clone();
        if let Some(window) = start.and_then(|start| start.window()) {
            window.makeKeyAndOrderFront(None);
        }
    }

    fn show_start_window(&self) {
        let mtm = self.mtm();
        let recents = DocumentStateStore::shared().recents(StartWindowController::RECENT_DISPLAY_LIMIT);
        let existing = self.ivars().start_window.borrow().clone();
        if let Some(start_window) = existing {
            start_window.reload_recents(&recents);
            if let Some(window) = start_window.window() {
                window.setAlphaValue(1.0);
            }
            unsafe { start_window.showWindow(None) };
            if let Some(window) = start_window.window() {
                window.makeKeyAndOrderFront(None);
            }
            return;
        }
        let guide = self.guide_offer(&recents);
        let controller = StartWindowController::new(recents, guide, mtm);
        let weak = self.weak();
        controller.set_on_open(Some(Rc::new({
            let weak = weak.clone();
            move |url: FileUrl| {
                if let Some(this) = weak.load() {
                    this.open_default(&url);
                }
            }
        })));
        controller.set_on_open_panel(Some(Rc::new({
            let weak = weak.clone();
            move || {
                if let Some(this) = weak.load() {
                    this.show_open_panel();
                }
            }
        })));
        controller.set_on_new(Some(Rc::new({
            let weak = weak.clone();
            move || {
                if let Some(this) = weak.load() {
                    this.new_document();
                }
            }
        })));
        controller.set_on_open_guide(Some(Rc::new({
            let weak = weak.clone();
            move || {
                if let Some(this) = weak.load() {
                    this.open_welcome_document();
                }
            }
        })));
        controller.set_on_clear_recents(Some(Rc::new({
            let weak = weak.clone();
            move || {
                if let Some(this) = weak.load() {
                    this.clear_recents();
                }
            }
        })));
        controller.set_on_remove_recent(Some(Rc::new({
            let weak = weak.clone();
            move |path: String| {
                if let Some(this) = weak.load() {
                    this.remove_recent_document(&path);
                }
            }
        })));
        *self.ivars().start_window.borrow_mut() = Some(controller.clone());
        unsafe { controller.showWindow(None) };
    }

    /// The launch after which the start window stops offering the tour.
    pub const TOUR_RETIREMENT_LAUNCH: i64 = 3;

    /// Taken once retires it immediately; three launches retires it
    /// regardless.
    pub fn should_offer_tour(has_taken_tour: bool, launch_count: i64) -> bool {
        !has_taken_tour && launch_count <= Self::TOUR_RETIREMENT_LAUNCH
    }

    fn guide_offer(&self, recents: &[RecentDocument]) -> StartGuideOffer {
        if !WelcomeDocument::is_available() {
            return StartGuideOffer::Unavailable;
        }
        let values = Preferences::shared().values();
        if !Self::should_offer_tour(values.has_taken_tour, values.launch_count) {
            return StartGuideOffer::Unavailable;
        }
        if Preferences::shared().is_first_run() && recents.is_empty() {
            StartGuideOffer::Primary
        } else {
            StartGuideOffer::Secondary
        }
    }

    pub fn open_welcome_document(&self) {
        // Recorded on the way in, not on completion.
        Preferences::shared().update(|values| values.has_taken_tour = true);
        match WelcomeDocument::materialize() {
            Ok(url) => {
                self.open(&url, Some(RenderMode::Live), None, false, DocumentOpenDisposition::Tab, None);
            }
            Err(error) => {
                self.warn(
                    "Couldn't open the tour",
                    &format!("The welcome document couldn't be prepared. {}", error.0),
                );
                self.raise_start_window();
                self.flush_warnings();
            }
        }
    }

    // MARK: - System integration (§10)

    /// Shows the first-run setup panel, once. Returns whether the panel was
    /// put on screen.
    fn present_setup_if_needed(&self) -> bool {
        let mtm = self.mtm();
        if Preferences::shared().values().has_answered_setup {
            self.refresh_system_registration_if_moved();
            return false;
        }
        let Some(controller) = SetupWindowController::make_if_needed(mtm) else {
            Preferences::shared().update(|values| values.has_answered_setup = true);
            self.refresh_system_registration_if_moved();
            return false;
        };
        *self.ivars().setup_window.borrow_mut() = Some(controller.clone());
        let weak = self.weak();
        controller.set_on_finish(Some(Rc::new(move || {
            if let Some(this) = weak.load() {
                *this.ivars().setup_window.borrow_mut() = None;
                this.note_system_registration();
            }
        })));
        unsafe { controller.showWindow(None) };
        if let Some(window) = controller.window() {
            window.makeKeyAndOrderFront(None);
        }
        true
    }

    fn bundle_path() -> String {
        objc2_foundation::NSBundle::mainBundle().bundleURL().path().map(|path| path.to_string()).unwrap_or_default()
    }

    /// LaunchServices and `pluginkit` record an absolute path, so moving the
    /// app leaves them pointing at nothing. Gated on the path having changed.
    fn refresh_system_registration_if_moved(&self) {
        let current = Self::bundle_path();
        if Preferences::shared().values().last_registered_bundle_path == current
            || !SystemIntegration::quick_look_extensions_are_bundled()
            || !SystemIntegration::is_permanently_installed()
        {
            return;
        }
        // No cache reset here — that is only worth it the once.
        SystemIntegration::register_with_system(false, |_| {}, self.mtm());
        self.note_system_registration();
    }

    fn note_system_registration(&self) {
        let path = Self::bundle_path();
        Preferences::shared().update(|values| values.last_registered_bundle_path = path);
    }

    /// Fades the start window out in parallel with the document fade-up.
    fn dismiss_start_window(&self) {
        let Some(controller) = self.ivars().start_window.borrow_mut().take() else { return };
        let animated = !StyleSheet::current(self.mtm()).reduce_motion;
        controller.dismiss(animated, None);
    }

    // MARK: - Session restore (§9.3)

    fn save_session(&self) {
        let mut group_ids: Vec<(usize, i64)> = Vec::new();
        let mut next_group_id = 0i64;
        let controllers = self.ivars().window_controllers.borrow().clone();
        let windows: Vec<SessionWindow> = controllers
            .iter()
            .filter_map(|controller| {
                let url = controller.markdown_document().url()?;
                let window = controller.window()?;
                let frame = window.frame();
                let (group_id, tab_order, selected) = match window.tabGroup() {
                    Some(group) => {
                        let identity = Retained::as_ptr(&group) as usize;
                        let id = match group_ids.iter().find(|(key, _)| *key == identity) {
                            Some((_, id)) => *id,
                            None => {
                                group_ids.push((identity, next_group_id));
                                next_group_id += 1;
                                next_group_id - 1
                            }
                        };
                        let order = group
                            .windows()
                            .iter()
                            .position(|candidate| std::ptr::eq(&*candidate, &*window))
                            .map(|index| index as i64);
                        let selected =
                            group.selectedWindow().is_some_and(|selected| std::ptr::eq(&*selected, &*window));
                        (Some(id), order, Some(selected))
                    }
                    None => (None, None, None),
                };
                Some(SessionWindow {
                    path: url.path(),
                    frame: NSString::from_rect(frame).to_string(),
                    mode: controller.mode().raw_value().to_owned(),
                    tab_group: group_id,
                    tab_order,
                    selected_tab: selected,
                    full_screen: Some(window.styleMask().contains(NSWindowStyleMask::FullScreen)),
                })
            })
            .collect();
        let value = JsonValue::Array(windows.iter().map(SessionWindow::encode).collect());
        let data = json_encoder::encode(&value, OutputFormatting::default());
        let _ = upleft_foundation::foundation_io::write_atomically(&data, &app_paths::session_file());
    }

    /// Closing several windows in a row rewrites the whole session file each
    /// time; coalesce to one write once the burst settles.
    fn schedule_session_save(&self) {
        if let Some(item) = self.ivars().session_save_work_item.borrow_mut().take() {
            item.cancel();
        }
        let weak = self.weak();
        let work = WorkItem::new(move || {
            if let Some(this) = weak.load() {
                *this.ivars().session_save_work_item.borrow_mut() = None;
                this.save_session();
            }
        });
        *self.ivars().session_save_work_item.borrow_mut() = Some(work.clone());
        work.dispatch_main_after(0.4);
    }

    fn restore_session(&self) -> bool {
        let Ok(data) = std::fs::read(app_paths::session_file().path()) else { return false };
        let Ok(value) = json_decoder::parse(&data) else { return false };
        let Ok(windows) = value.array_of(SessionWindow::decode) else { return false };
        if windows.is_empty() {
            return false;
        }

        let mut restored = 0;
        let mut missing: Vec<String> = Vec::new();
        let mut group_hosts: Vec<(i64, Retained<NSWindow>)> = Vec::new();
        let mut selected_windows: Vec<Retained<NSWindow>> = Vec::new();
        let mut ordered = windows;
        ordered.sort_by(|a, b| {
            if a.tab_group != b.tab_group {
                return a.tab_group.unwrap_or(i64::MAX).cmp(&b.tab_group.unwrap_or(i64::MAX));
            }
            a.tab_order.unwrap_or(0).cmp(&b.tab_order.unwrap_or(0))
        });
        for entry in ordered {
            let url = FileUrl::from_path(&entry.path);
            if !upleft_foundation::foundation_io::file_exists(&url.path()) {
                missing.push(url.last_path_component());
                continue;
            }
            let host = entry
                .tab_group
                .and_then(|group| group_hosts.iter().find(|(id, _)| *id == group).map(|(_, window)| window.clone()));
            let disposition = if host.is_none() { DocumentOpenDisposition::Window } else { DocumentOpenDisposition::Tab };
            let Some(controller) =
                self.open(&url, RenderMode::from_raw_value(&entry.mode), None, false, disposition, host.as_deref())
            else {
                continue;
            };
            let frame = objc2_foundation::NSRectFromString(&ns(&entry.frame));
            if let Some(frame) = Self::reachable_frame(frame, self.mtm())
                && let Some(window) = controller.window()
            {
                window.setFrame_display(frame, true);
            }
            if entry.full_screen == Some(true)
                && let Some(restored_window) = controller.window()
            {
                main_async(move || restored_window.toggleFullScreen(None));
            }
            if let Some(group) = entry.tab_group
                && !group_hosts.iter().any(|(id, _)| *id == group)
                && let Some(window) = controller.window()
            {
                group_hosts.push((group, window));
            }
            if entry.selected_tab == Some(true)
                && let Some(window) = controller.window()
            {
                selected_windows.push(window);
            }
            restored += 1;
        }
        for window in selected_windows {
            if let Some(group) = window.tabGroup() {
                group.setSelectedWindow(Some(&window));
            }
        }
        self.report_skipped_session_files(&missing);
        restored > 0
    }

    /// A saved frame, moved onto an attached screen when the screen it was
    /// recorded on has gone away. `None` for a frame with no usable size.
    pub fn reachable_frame(frame: NSRect, mtm: MainThreadMarker) -> Option<NSRect> {
        if !(frame.size.width >= 1.0 && frame.size.height >= 1.0) {
            return None;
        }
        let screens = NSScreen::screens(mtm);
        if screens.is_empty() {
            return Some(frame);
        }
        // The title bar is the handle.
        let title_bar = NSRect::new(
            NSPoint::new(frame.origin.x, frame.origin.y + frame.size.height - 24.0),
            NSSize::new(frame.size.width, 24.0),
        );
        if screens.iter().any(|screen| rect_intersects(screen.visibleFrame(), title_bar)) {
            return Some(frame);
        }
        let target = NSScreen::mainScreen(mtm).unwrap_or_else(|| screens.objectAtIndex(0)).visibleFrame();
        let size = NSSize::new(frame.size.width.min(target.size.width), frame.size.height.min(target.size.height));
        Some(NSRect::new(
            NSPoint::new(
                target.origin.x + target.size.width / 2.0 - size.width / 2.0,
                target.origin.y + target.size.height / 2.0 - size.height / 2.0,
            ),
            size,
        ))
    }

    // MARK: - Menu actions

    fn clear_recents(&self) {
        DocumentStateStore::shared().clear_recents();
        let start = self.ivars().start_window.borrow().clone();
        if let Some(start) = start {
            start.reload_recents(&[]);
        }
    }

    /// Forgetting one file re-reads the list rather than removing a row in
    /// place: the store also drops entries whose file is gone.
    fn remove_recent_document(&self, path: &str) {
        DocumentStateStore::shared().remove_recent(path);
        let start = self.ivars().start_window.borrow().clone();
        if let Some(start) = start {
            start.reload_recents(&DocumentStateStore::shared().recents(StartWindowController::RECENT_DISPLAY_LIMIT));
        }
    }

    fn select_theme(sender: &NSMenuItem, slot: ThemePreferenceSlot) {
        let Some(name) = sender.representedObject().and_then(|object| object.downcast::<NSString>().ok()) else {
            return;
        };
        let name = name.to_string();
        Preferences::shared().update(|values| values.select_theme(&name, slot));
    }

    fn run_import_theme(&self) {
        let mtm = self.mtm();
        let panel = NSOpenPanel::openPanel(mtm);
        let json = unsafe { objc2_uniform_type_identifiers::UTTypeJSON };
        panel.setAllowedContentTypes(&NSArray::from_slice(&[json]));
        panel.setMessage(Some(&ns("Choose a VS Code or Shiki theme")));
        if panel.runModal() != NSModalResponseOK {
            return;
        }
        let Some(path) = panel.URL().and_then(|url| url.path()) else { return };
        match ThemeStore::shared().import_vscode_theme(&path.to_string()) {
            Ok(theme) => {
                let name = theme.name.clone();
                let appearance = theme.appearance;
                Preferences::shared().update(|values| match appearance {
                    ThemeAppearance::Light => values.select_theme(&name, ThemePreferenceSlot::Light),
                    ThemeAppearance::Dark => values.select_theme(&name, ThemePreferenceSlot::Dark),
                    ThemeAppearance::Auto => {
                        values.select_theme(&name, ThemePreferenceSlot::Light);
                        values.select_theme(&name, ThemePreferenceSlot::Dark);
                    }
                });
            }
            Err(error) => {
                // `NSAlert(error:)`, then the message replaced.
                let alert = NSAlert::new(mtm);
                alert.setInformativeText(&ns(&error.error_description()));
                alert.setMessageText(&ns("Couldn't import that theme"));
                alert.runModal();
            }
        }
    }

    /// Opens Settings, optionally on a named pane.
    pub fn show_preferences(&self, pane: Option<SettingsPane>) {
        let existing = self.ivars().preferences_window.borrow().clone();
        let controller = existing.unwrap_or_else(|| PreferencesWindowController::new(self.mtm()));
        *self.ivars().preferences_window.borrow_mut() = Some(controller.clone());
        if let Some(pane) = pane {
            controller.select(pane);
        }
        unsafe { controller.showWindow(None) };
        if let Some(window) = controller.window() {
            window.makeKeyAndOrderFront(None);
        }
    }

    /// The one place that decides which theme is current.
    fn apply_selected_theme(&self) {
        let app = NSApplication::sharedApplication(self.mtm());
        let appearance = if Preferences::shared().values().follows_system_appearance {
            Self::mac_os_appearance(&app)
        } else {
            app.effectiveAppearance()
        };
        ThemeStore::shared().select(&Preferences::shared().theme_name(&appearance));
    }

    /// The actual macOS preference, independent of any appearance an
    /// existing app window inherited before Follow System was enabled.
    fn mac_os_appearance(app: &NSApplication) -> Retained<NSAppearance> {
        let style = NSUserDefaults::standardUserDefaults().stringForKey(&ns("AppleInterfaceStyle"));
        let dark = style.is_some_and(|style| style.to_string() == "Dark");
        let name = unsafe { if dark { NSAppearanceNameDarkAqua } else { NSAppearanceNameAqua } };
        NSAppearance::appearanceNamed(name).unwrap_or_else(|| app.effectiveAppearance())
    }

    // MARK: - Command routing

    /// Commands with no document — everything else is handled by the window
    /// controller further down the responder chain.
    pub fn handle_application_command(&self, command: Command) -> bool {
        let mtm = self.mtm();
        match command {
            Command::NewDocument => {
                if let Some(window) = self.active_document_window()
                    && let Some(controller) = Self::document_controller(&window)
                    && controller.handle_new_document_command()
                {
                    return true;
                }
                self.new_document();
                true
            }
            Command::Open => {
                self.show_open_panel();
                true
            }
            // No document to close, so ⌘W means the window in front.
            Command::Close => {
                if let Some(window) = NSApplication::sharedApplication(mtm).keyWindow() {
                    window.performClose(None);
                }
                true
            }
            Command::Preferences => {
                self.show_preferences(None);
                true
            }
            Command::ShowKeybindings => {
                self.show_preferences(Some(SettingsPane::Keys));
                true
            }
            Command::ReloadTheme => {
                ThemeStore::shared().reload_user_themes();
                true
            }
            Command::CompareFiles => {
                self.show_compare_panel();
                true
            }
            // The palette has no menu validation in front of it.
            Command::CheckForUpdates => {
                let coordinator = UpdateCoordinator::shared(mtm);
                if !coordinator.can_check_for_updates() {
                    return true;
                }
                coordinator.check_for_updates();
                true
            }
            Command::ToggleLightDark => {
                self.toggle_light_dark_theme();
                true
            }
            Command::TaskPanel => {
                let Some(window) = self.active_document_window() else { return false };
                let Some(controller) = Self::document_controller(&window) else { return false };
                controller.toggle_task_panel();
                true
            }
            // Handled by the document window controller.
            Command::GoToLine => false,
            _ => false,
        }
    }

    pub fn new_document(&self) {
        let mtm = self.mtm();
        let panel = NSSavePanel::savePanel(mtm);
        panel.setAllowedContentTypes(&NSArray::from_retained_slice(&document_types::content_types()));
        panel.setNameFieldStringValue(&ns("Untitled.md"));
        panel.setMessage(Some(&ns("Create a Markdown document")));
        panel.setPrompt(Some(&ns("Create")));
        // Cancel leaves the start window key and unchanged.
        if panel.runModal() != NSModalResponseOK {
            self.raise_start_window();
            return;
        }
        let Some(url) = panel.URL().and_then(|url| FileUrl::from_nsurl(&url)) else {
            self.raise_start_window();
            return;
        };
        let name = url.deleting_path_extension().last_path_component();
        if let Err(error) = upleft_foundation::foundation_io::write_atomically(format!("# {name}\n\n").as_bytes(), &url)
        {
            self.present_write_failure(&error.description, &url);
            self.raise_start_window();
            return;
        }
        self.open(&url, Some(RenderMode::Live), None, false, DocumentOpenDisposition::Tab, None);
    }

    fn present_write_failure(&self, description: &str, url: &FileUrl) {
        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&ns(&format!("Couldn't create {}", url.last_path_component())));
        alert.setInformativeText(&ns(description));
        alert.setAlertStyle(NSAlertStyle::Warning);
        alert.runModal();
    }

    /// Toggles between the selected light and dark themes without opening
    /// Settings. When following system appearance, it picks the opposite
    /// half of the pair and stops following.
    fn toggle_light_dark_theme(&self) {
        let app = NSApplication::sharedApplication(self.mtm());
        let names = unsafe { NSArray::from_slice(&[NSAppearanceNameAqua, NSAppearanceNameDarkAqua]) };
        let is_dark = app
            .effectiveAppearance()
            .bestMatchFromAppearancesWithNames(&names)
            .is_some_and(|name| name.isEqualToString(unsafe { NSAppearanceNameDarkAqua }));
        let themes = ThemeStore::shared().themes();
        Preferences::shared().update(|values| {
            values.follows_system_appearance = false;
            let current = themes.iter().find(|theme| theme.name == values.theme_name).map(|theme| theme.appearance);
            if is_dark {
                // Currently dark → switch to light.
                if current == Some(ThemeAppearance::Dark) {
                    values.theme_name = "Paper Light".to_owned();
                }
            } else if current != Some(ThemeAppearance::Dark) {
                // Currently light → switch to dark.
                values.theme_name = values.dark_theme_name.clone();
            }
        });
        // applySelectedTheme picks up the change from the notification.
    }

    // MARK: - Dock menu

    fn dock_menu(&self) -> Retained<NSMenu> {
        let mtm = self.mtm();
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &ns("Upleft"));
        let recents = DocumentStateStore::shared().recents(8);
        if recents.is_empty() {
            let none = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &ns("No recent files"), None, &ns(""))
            };
            none.setEnabled(false);
            menu.addItem(&none);
        } else {
            for recent in recents.iter().take(8) {
                let item = unsafe {
                    NSMenuItem::initWithTitle_action_keyEquivalent(
                        NSMenuItem::alloc(mtm),
                        &ns(&recent.display_name),
                        Some(sel!(openRecentDocument:)),
                        &ns(""),
                    )
                };
                unsafe {
                    item.setRepresentedObject(Some(&ns(&recent.path)));
                    item.setTarget(Some(self));
                }
                menu.addItem(&item);
            }
        }
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        let new_item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &ns("New Document"),
                Some(sel!(performDownrightCommand:)),
                &ns("n"),
            )
        };
        new_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
        unsafe {
            new_item.setTarget(Some(self));
            new_item.setRepresentedObject(Some(&ns(Command::NewDocument.raw_value())));
        }
        menu.addItem(&new_item);
        menu
    }

    // MARK: - Warnings

    /// Queues a warning. Sheets are window-modal, so they inform without
    /// blocking; a fault raised before any window exists waits for one.
    fn warn(&self, title: &str, message: &str) {
        self.ivars()
            .pending_warnings
            .borrow_mut()
            .push(Warning { title: title.to_owned(), message: message.to_owned() });
        self.flush_warnings();
    }

    fn flush_warnings(&self) {
        if self.ivars().is_presenting_warning.get() || self.ivars().pending_warnings.borrow().is_empty() {
            return;
        }
        let app = NSApplication::sharedApplication(self.mtm());
        let window = app.keyWindow().or_else(|| app.windows().iter().find(|window| window.isVisible()));
        let Some(window) = window else { return };
        let warning = self.ivars().pending_warnings.borrow_mut().remove(0);
        self.ivars().is_presenting_warning.set(true);
        let alert = NSAlert::new(self.mtm());
        alert.setMessageText(&ns(&warning.title));
        alert.setInformativeText(&ns(&warning.message));
        alert.setAlertStyle(NSAlertStyle::Warning);
        alert.addButtonWithTitle(&ns("OK"));
        let weak = self.weak();
        let handler = RcBlock::new(move |_response: objc2_app_kit::NSModalResponse| {
            let Some(this) = weak.load() else { return };
            this.ivars().is_presenting_warning.set(false);
            this.flush_warnings();
        });
        alert.beginSheetModalForWindow_completionHandler(&window, Some(&handler));
    }

    fn report_settings_write_failure(&self, error: &str) {
        self.warn(
            "Upleft can't save your settings",
            &format!(
                "Your changes are in effect for now, but they won't survive a restart until Upleft can write to its settings file. {error}"
            ),
        );
    }

    fn report_keybindings_load_failure(&self, error: &str) {
        self.warn(
            "Your keyboard shortcuts file couldn't be read",
            &format!(
                "Upleft is using its default shortcuts. Your file at {} was left untouched so you can repair it; recording a shortcut in Settings replaces it. {error}",
                app_paths::keybindings_file().path()
            ),
        );
    }

    fn report_settings_recovery(&self, backup: Option<&FileUrl>) {
        let where_it_went = backup
            .map(|backup| format!("The old file is kept as {}.", backup.last_path_component()))
            .unwrap_or_else(|| "The old file couldn't be kept.".to_owned());
        self.warn("Your settings file couldn't be read", &format!("Upleft has started from its defaults. {where_it_went}"));
    }

    fn report_unavailable_storage(&self, failures: Vec<PreparationFailure>) {
        if failures.is_empty() {
            return;
        }
        let lost = failures
            .iter()
            .map(|failure| format!("• {}", failure.purpose.feature_description()))
            .collect::<Vec<_>>()
            .join("\n");
        self.warn(
            "Upleft can't use its support folder",
            &format!("Until this is fixed, Upleft can't save:\n\n{lost}\n\n{}", failures[0].error),
        );
    }

    fn report_skipped_session_files(&self, names: &[String]) {
        if names.is_empty() {
            return;
        }
        let list = names.iter().take(6).map(|name| format!("• {name}")).collect::<Vec<_>>().join("\n");
        let more = if names.len() > 6 { format!("\n• and {} more", names.len() - 6) } else { String::new() };
        self.warn(
            if names.len() == 1 {
                "One file from your last session is gone"
            } else {
                "Some files from your last session are gone"
            },
            &format!("These weren't reopened because they're no longer where they were:\n\n{list}{more}"),
        );
    }

    fn show_compare_panel(&self) {
        let mtm = self.mtm();
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setAllowsMultipleSelection(true);
        panel.setAllowedContentTypes(&NSArray::from_retained_slice(&document_types::content_types()));
        panel.setMessage(Some(&ns("Choose two files to compare")));
        if panel.runModal() != NSModalResponseOK || panel.URLs().count() != 2 {
            return;
        }
        let urls: Vec<FileUrl> = panel.URLs().iter().filter_map(|url| FileUrl::from_nsurl(&url)).collect();
        if urls.len() != 2 {
            return;
        }
        let controller = CompareWindowController::with_urls(&urls[0], &urls[1], mtm);
        unsafe { controller.showWindow(None) };
        self.ivars().comparison_windows.borrow_mut().push(controller.clone());
        // Target/action, so the observation can remove itself precisely.
        if let Some(window) = controller.window() {
            unsafe {
                NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                    self,
                    sel!(comparisonWindowWillClose:),
                    Some(NSWindowWillCloseNotification),
                    Some(&window),
                );
            }
        }
    }
}

/// Swift `Int(String)`: an optional sign and decimal digits, nothing else.
fn swift_int(text: &str) -> Option<isize> {
    let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// `NSRect.intersects(_:)`.
fn rect_intersects(a: NSRect, b: NSRect) -> bool {
    a.origin.x < b.origin.x + b.size.width
        && b.origin.x < a.origin.x + a.size.width
        && a.origin.y < b.origin.y + b.size.height
        && b.origin.y < a.origin.y + a.size.height
        && a.size.width > 0.0
        && a.size.height > 0.0
        && b.size.width > 0.0
        && b.size.height > 0.0
}

// MARK: - The appearance observation

pub struct AppearanceObservationIvars {
    delegate: Weak<AppDelegate>,
}

define_class!(
    /// `NSApp.observe(\.effectiveAppearance)`: key-value observation of the
    /// application's effective appearance, options `[]`. Swift's
    /// `NSKeyValueObservation` is Foundation's own class; this observer is
    /// the port's.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpleftAppearanceObservation"]
    #[ivars = AppearanceObservationIvars]
    pub struct AppearanceObservation;

    unsafe impl NSObjectProtocol for AppearanceObservation {}

    impl AppearanceObservation {
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe_value(
            &self,
            _key_path: Option<&NSString>,
            _object: Option<&AnyObject>,
            _change: Option<&objc2_foundation::NSDictionary<NSKeyValueChangeKey, AnyObject>>,
            _context: *mut std::ffi::c_void,
        ) {
            // `Task { @MainActor [weak self] in self?.applySelectedTheme() }`.
            let delegate = self.ivars().delegate.clone();
            main_async(move || {
                if let Some(delegate) = delegate.load() {
                    delegate.apply_selected_theme();
                }
            });
        }
    }
);

impl AppearanceObservation {
    fn observe(delegate: &AppDelegate, mtm: MainThreadMarker) -> Retained<AppearanceObservation> {
        let this = Self::alloc(mtm).set_ivars(AppearanceObservationIvars { delegate: Weak::from(delegate) });
        let this: Retained<AppearanceObservation> = unsafe { msg_send![super(this), init] };
        let app = NSApplication::sharedApplication(mtm);
        unsafe {
            app.addObserver_forKeyPath_options_context(
                &this,
                &ns("effectiveAppearance"),
                NSKeyValueObservingOptions::empty(),
                std::ptr::null_mut(),
            );
        }
        this
    }
}
