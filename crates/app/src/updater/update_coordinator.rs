//! Port of `Sources/DownrightApp/Updater/UpdateCoordinator.swift`.
//!
//! The coordinator is a main-thread object (`Rc`, cells), as the Swift class
//! is `@MainActor`. It never holds a borrow of its own state while it calls
//! out (a capability, the engine, the panel, a notification observer), so
//! every one of those may call back into it, as they can in Swift.
//!
//! The panel (`UpdateWindowController`, `panels::update_window_controller`)
//! is reached through [`UpdatePanelController`]: every coordinator is born
//! with the factory that builds it (`update_window_controller::panel_factory`,
//! as Swift's `showPanel()` builds the controller directly), and
//! [`UpdateCoordinator::set_panel_factory`] replaces it. The
//! `#if DEBUG` `presentDemoUpdateForDebugging` is not ported: the reference
//! builds release.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use objc2::{AnyThread, MainThreadMarker};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSAlert, NSAlertStyle, NSBeep, NSWorkspace};
use objc2_foundation::{
    NSBundle, NSData, NSDate, NSDateFormatter, NSDateFormatterStyle, NSDictionary, NSError, NSNotificationCenter,
    NSObject, NSString,
};
use upleft_foundation::date::Date;

use super::downright_update_driver::{Callback, DownrightUpdateDriver, Reply, UpdateDriverHost, UpdateUserChoice};
use super::main_actor::async_main;
use super::release_watch::ReleaseWatch;
use super::update_engine::{BackgroundDownloadHandler, SparkleUpdateEngine, UpdateEngine};
use super::update_metadata::{UpdateFailure, UpdateMetadata, Url};
use super::update_state_machine::{UpdateEvent, UpdatePhase, UpdateStage, UpdateStateMachine, swift_max};

/// One-shot Sparkle capability (reply, acknowledgement, cancellation, or
/// retry closure). The entire contract of the update UI lives here: call it
/// or discard it, but never twice, and never leak it past dismissal.
pub struct Capability<T> {
    body: RefCell<Option<CapabilityBody<T>>>,
}

type CapabilityBody<T> = Box<dyn FnOnce(T)>;

impl<T> Capability<T> {
    pub fn new(body: impl FnOnce(T) + 'static) -> Capability<T> {
        Capability { body: RefCell::new(Some(Box::new(body))) }
    }

    pub fn from_box(body: Box<dyn FnOnce(T)>) -> Capability<T> {
        Capability { body: RefCell::new(Some(body)) }
    }

    pub fn is_armed(&self) -> bool {
        self.body.borrow().is_some()
    }

    /// Invoke exactly once. A second call is a no-op (and a bug).
    pub fn call(&self, value: T) {
        let body = self.body.borrow_mut().take();
        if let Some(body) = body {
            body(value);
        }
    }

    /// Drops the capability without invoking Sparkle's block.
    pub fn discard(&self) {
        let body = self.body.borrow_mut().take();
        drop(body);
    }
}

/// The minimum configuration needed before Downright asks Sparkle to run.
/// Invalid release metadata must fail closed as a normal disabled-updater
/// state, not reach a partially configured Sparkle instance.
pub struct UpdateConfiguration;

impl UpdateConfiguration {
    /// `isValid(infoDictionary:)`. `None` is Swift's `?? [:]`.
    pub fn is_valid(info_dictionary: Option<&NSDictionary<NSString, AnyObject>>) -> bool {
        let Some(info) = info_dictionary else {
            return false;
        };
        let Some(feed_string) = string_value(info, "SUFeedURL") else {
            return false;
        };
        let Some(feed_url) = Url::from_nsstring(&feed_string) else {
            return false;
        };
        if !feed_url.scheme().is_some_and(|scheme| upleft_swift_text::str_eq(&upleft_swift_text::lowercased(&scheme), "https")) {
            return false;
        }
        if feed_url.host().is_none_or(|host| host.is_empty()) {
            return false;
        }
        let Some(public_key) = string_value(info, "SUPublicEDKey") else {
            return false;
        };
        // `Data(base64Encoded:)` with no options; Foundation's decoder agrees
        // with it on padding, whitespace and alphabet (see the `updater` suite).
        let Some(public_key_data) = NSData::initWithBase64EncodedString_options(
            NSData::alloc(),
            &public_key,
            objc2_foundation::NSDataBase64DecodingOptions(0),
        ) else {
            return false;
        };
        public_key_data.length() == 32
    }
}

/// `dictionary[key] as? String`.
fn string_value(dictionary: &NSDictionary<NSString, AnyObject>, key: &str) -> Option<Retained<NSString>> {
    dictionary.objectForKey(&NSString::from_str(key))?.downcast::<NSString>().ok()
}

/// What the update status pill displays right now.
#[derive(Clone, Debug)]
pub enum UpdatePillModel {
    /// "Update Now" — one press installs. `version` is the build on offer
    /// (carried in the tooltip, the accessibility label, and the hover notes
    /// rather than on the button, which says what pressing it *does*).
    ///
    /// `is_ready` distinguishes an update already downloaded in the background
    /// — the usual case, because `SUAutomaticallyUpdate` is on, and the press
    /// is then effectively instant — from one that must be fetched first. The
    /// label is the same either way: the difference is how long the press
    /// takes, not what it means.
    UpdateNow { version: String, is_ready: bool },
    /// "Restart to Update" — installation has already begun and only a quit
    /// can finish it. Distinct from `UpdateNow` because pressing here
    /// retries termination; there is nothing left to install.
    RestartToUpdate,
    /// Progress state: label plus an optional 0–1 fraction.
    Progress(String, Option<f64>),
    /// "Update Failed" — warning treatment, opens the panel.
    Warning,
    /// Informational update: show the version, open release info.
    Informational(String),
}

impl PartialEq for UpdatePillModel {
    fn eq(&self, other: &UpdatePillModel) -> bool {
        use upleft_swift_text::str_eq;
        match (self, other) {
            (
                UpdatePillModel::UpdateNow { version: a, is_ready: ready_a },
                UpdatePillModel::UpdateNow { version: b, is_ready: ready_b },
            ) => str_eq(a, b) && ready_a == ready_b,
            (UpdatePillModel::RestartToUpdate, UpdatePillModel::RestartToUpdate) => true,
            (UpdatePillModel::Progress(a, fraction_a), UpdatePillModel::Progress(b, fraction_b)) => {
                str_eq(a, b) && fraction_a == fraction_b
            }
            (UpdatePillModel::Warning, UpdatePillModel::Warning) => true,
            (UpdatePillModel::Informational(a), UpdatePillModel::Informational(b)) => str_eq(a, b),
            _ => false,
        }
    }
}

impl UpdatePillModel {
    /// Hidden when idle, up to date, or mid-silent-check.
    pub const HIDDEN: Option<UpdatePillModel> = None;

    /// Whether pressing this pill starts an install (as opposed to opening a
    /// window). The hover notes only appear over a pill that can act.
    pub fn offers_install(&self) -> bool {
        match self {
            UpdatePillModel::UpdateNow { .. } | UpdatePillModel::RestartToUpdate => true,
            UpdatePillModel::Progress(..) | UpdatePillModel::Warning | UpdatePillModel::Informational(_) => false,
        }
    }
}

/// How release notes are presented in the update panel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum UpdateReleaseNotesState {
    #[default]
    None,
    Loaded(Vec<u8>),
    Failed,
}

/// The calls the coordinator makes on its `UpdateWindowController`,
/// implemented over the real window controller in
/// `panels::update_window_controller`; a test can install another factory
/// with [`UpdateCoordinator::set_panel_factory`].
pub trait UpdatePanelController {
    /// `panel?.showWindow(nil)`.
    fn show_window(&self);
    /// `panel?.window?.makeKeyAndOrderFront(nil)`.
    fn make_key_and_order_front(&self);
    /// `panel?.window?.performClose(nil)`.
    fn perform_close(&self);
}

/// `UpdateWindowController(coordinator: self)`.
pub type UpdatePanelFactory = Rc<dyn Fn(&Rc<UpdateCoordinator>) -> Rc<dyn UpdatePanelController>>;

/// The single owner of the updater: `SPUUpdater` lifecycle, the user driver,
/// the state machine, every one-shot capability, the settings proxy, and the
/// windows (the panel) and pills (the document titlebars and start window).
///
/// Not a standard Sparkle setup and not a thin wrapper: Sparkle owns the
/// machinery and Downright owns every visible interaction, which is exactly
/// the architecture the spec calls for.
pub struct UpdateCoordinator {
    weak_self: Weak<UpdateCoordinator>,

    // MARK: - State
    machine: RefCell<UpdateStateMachine>,
    /// Set when a *background* (automatic) download completes and the machine
    /// itself stays idle; drives the "Restart to Update" pill until the
    /// update is installed, dismissed, or superseded.
    downloaded_update: RefCell<Option<UpdateMetadata>>,
    /// Whether the current update cycle began from a user action (menu check,
    /// palette, settings, or the pill). Automatic (scheduled) cycles keep
    /// this false so background downloads stay quiet: only the pill appears,
    /// never the panel — the panel opens when the user asks for it.
    current_cycle_is_user_initiated: Cell<bool>,
    /// Test seam: how often `showPanel()` was reached, so tests can assert
    /// that background flows never open the panel while user flows always do.
    panel_show_count: Cell<isize>,
    /// Set while a press of "Update Now" is being carried out through a fresh
    /// Sparkle cycle. The press already *is* the decision, so the cycle it
    /// starts must not stop to ask the same question in a window: every prompt
    /// Sparkle raises while this is set is answered `.install` immediately.
    /// A failure clears it and does open the panel — a press that could not be
    /// honoured owes the reader an explanation.
    is_expedited_install: Cell<bool>,
    /// Test seam: how often the release watch asked Sparkle to look.
    release_watch_trigger_count: Cell<isize>,
    /// The update this cycle is about, kept past the phases that stop carrying
    /// it themselves. `.downloading`, `.extracting`, and `.readyToRelaunch`
    /// have no metadata in them, and without this the pill's tooltip, its
    /// accessibility label, and the hover notes all go blank halfway through
    /// an install — naming a version at the start and not at the end reads as
    /// the app having lost track of what it is installing.
    current_cycle_update: RefCell<Option<UpdateMetadata>>,

    // MARK: - Infrastructure
    engine: RefCell<Option<Rc<dyn UpdateEngine>>>,
    driver: RefCell<Option<Rc<DownrightUpdateDriver>>>,
    panel: RefCell<Option<Rc<dyn UpdatePanelController>>>,
    panel_factory: RefCell<Option<UpdatePanelFactory>>,
    startup_failure: RefCell<Option<UpdateFailure>>,
    release_watch: RefCell<Option<Rc<ReleaseWatch>>>,

    /// Tests set this to keep the coordinator logic window-free.
    suppress_ui_for_testing: Cell<bool>,

    // MARK: - Capabilities
    check_cancellation: RefCell<Option<Capability<()>>>,
    download_cancellation: RefCell<Option<Capability<()>>>,
    choice_reply: RefCell<Option<Capability<UpdateUserChoice>>>,
    ready_reply: RefCell<Option<Capability<UpdateUserChoice>>>,
    retry_termination: RefCell<Option<Capability<()>>>,
    acknowledgement: RefCell<Option<Capability<()>>>,

    // MARK: - Release notes state
    release_notes: RefCell<UpdateReleaseNotesState>,

    /// The `object:` of every `stateDidChange` post. Swift posts the
    /// coordinator itself; the port's coordinator is not an Objective-C
    /// object, so it posts this stand-in, one per coordinator. Observers
    /// filter on it as they would on the coordinator.
    notification_object: Retained<NSObject>,
}

type CapabilitySlot<T> = RefCell<Option<Capability<T>>>;

fn is_armed<T>(slot: &CapabilitySlot<T>) -> bool {
    slot.borrow().as_ref().is_some_and(Capability::is_armed)
}

/// `capability?.discard(); capability = nil`.
fn discard<T>(slot: &CapabilitySlot<T>) {
    let capability = slot.borrow_mut().take();
    if let Some(capability) = capability {
        capability.discard();
    }
}

/// `let pending = capability; capability = nil; pending?.call(value)`.
fn consume<T>(slot: &CapabilitySlot<T>, value: T) {
    let pending = slot.borrow_mut().take();
    if let Some(pending) = pending {
        pending.call(value);
    }
}

thread_local! {
    static SHARED: Rc<UpdateCoordinator> = UpdateCoordinator::make(None);
}

impl UpdateCoordinator {
    /// `Notification.Name("Upleft.UpdateCoordinator.stateDidChange")`.
    /// Fired whenever the machine phase, pill model, or release-notes state
    /// changes. Pills and the panel observe this; nothing polls.
    pub const STATE_DID_CHANGE: &'static str = "Upleft.UpdateCoordinator.stateDidChange";

    /// `static let shared`, built with the private `init()` (no engine until
    /// `start()`).
    pub fn shared(_mtm: MainThreadMarker) -> Rc<UpdateCoordinator> {
        SHARED.with(Rc::clone)
    }

    /// `init(engine:)`: test seam, a coordinator wired to a deterministic
    /// fake engine. The production singleton uses `start()` instead, which
    /// builds the Sparkle engine and driver itself.
    pub fn new(engine: Option<Rc<dyn UpdateEngine>>) -> Rc<UpdateCoordinator> {
        let coordinator = UpdateCoordinator::make(engine.clone());
        if let Some(engine) = engine {
            engine.set_on_background_download_completed(Some(coordinator.background_download_handler()));
        }
        coordinator
    }

    fn make(engine: Option<Rc<dyn UpdateEngine>>) -> Rc<UpdateCoordinator> {
        Rc::new_cyclic(|weak_self| UpdateCoordinator {
            weak_self: weak_self.clone(),
            machine: RefCell::new(UpdateStateMachine::new()),
            downloaded_update: RefCell::new(None),
            current_cycle_is_user_initiated: Cell::new(false),
            panel_show_count: Cell::new(0),
            is_expedited_install: Cell::new(false),
            release_watch_trigger_count: Cell::new(0),
            current_cycle_update: RefCell::new(None),
            engine: RefCell::new(engine),
            driver: RefCell::new(None),
            panel: RefCell::new(None),
            panel_factory: RefCell::new(Some(crate::panels::update_window_controller::panel_factory())),
            startup_failure: RefCell::new(None),
            release_watch: RefCell::new(None),
            suppress_ui_for_testing: Cell::new(false),
            check_cancellation: RefCell::new(None),
            download_cancellation: RefCell::new(None),
            choice_reply: RefCell::new(None),
            ready_reply: RefCell::new(None),
            retry_termination: RefCell::new(None),
            acknowledgement: RefCell::new(None),
            release_notes: RefCell::new(UpdateReleaseNotesState::None),
            notification_object: NSObject::new(),
        })
    }

    /// `{ [weak self] version in … self?.backgroundDownloadCompleted(version) }`.
    /// Swift hops to the main actor when Sparkle calls from another thread;
    /// the port's handler is not `Send`, so it only ever runs on the
    /// coordinator's own (main) thread and calls straight through.
    fn background_download_handler(&self) -> BackgroundDownloadHandler {
        let weak = self.weak_self.clone();
        Rc::new(move |version: &str| {
            if let Some(coordinator) = weak.upgrade() {
                coordinator.background_download_completed(version);
            }
        })
    }

    fn engine(&self) -> Option<Rc<dyn UpdateEngine>> {
        self.engine.borrow().clone()
    }

    // MARK: - State accessors

    pub fn machine(&self) -> UpdateStateMachine {
        self.machine.borrow().clone()
    }

    pub fn phase(&self) -> UpdatePhase {
        self.machine.borrow().phase().clone()
    }

    fn reduce(&self, event: UpdateEvent) {
        self.machine.borrow_mut().reduce(&event);
    }

    pub fn downloaded_update(&self) -> Option<UpdateMetadata> {
        self.downloaded_update.borrow().clone()
    }

    pub fn current_cycle_is_user_initiated(&self) -> bool {
        self.current_cycle_is_user_initiated.get()
    }

    pub fn panel_show_count(&self) -> isize {
        self.panel_show_count.get()
    }

    pub fn is_expedited_install(&self) -> bool {
        self.is_expedited_install.get()
    }

    pub fn release_watch_trigger_count(&self) -> isize {
        self.release_watch_trigger_count.get()
    }

    pub fn current_cycle_update(&self) -> Option<UpdateMetadata> {
        self.current_cycle_update.borrow().clone()
    }

    pub fn release_notes(&self) -> UpdateReleaseNotesState {
        self.release_notes.borrow().clone()
    }

    pub fn is_running(&self) -> bool {
        self.engine().is_some_and(|engine| engine.is_running())
    }

    pub fn suppress_ui_for_testing(&self) -> bool {
        self.suppress_ui_for_testing.get()
    }

    pub fn set_suppress_ui_for_testing(&self, value: bool) {
        self.suppress_ui_for_testing.set(value);
    }

    /// The object every `stateDidChange` notification carries.
    pub fn notification_object(&self) -> &NSObject {
        &self.notification_object
    }

    /// How `showPanel()` builds its `UpdateWindowController` (by default,
    /// `update_window_controller::panel_factory()`; `None` leaves
    /// `showPanel()` counting only).
    pub fn set_panel_factory(&self, factory: Option<UpdatePanelFactory>) {
        *self.panel_factory.borrow_mut() = factory;
    }

    // MARK: - Settings (bound directly to Sparkle's persisted properties)

    pub fn automatically_checks_for_updates(&self) -> bool {
        self.engine().is_some_and(|engine| engine.automatically_checks_for_updates())
    }

    pub fn set_automatically_checks_for_updates(&self, new_value: bool) {
        let Some(engine) = self.engine() else {
            return;
        };
        engine.set_automatically_checks_for_updates(new_value);
        // The watch is a second automatic check, so it answers to the same
        // switch. A setting that stopped Sparkle's schedule and left this
        // one polling would be a narrower promise than the one it makes.
        if new_value {
            self.start_release_watch();
        } else {
            let watch = self.release_watch.borrow().clone();
            if let Some(watch) = watch {
                watch.stop();
            }
            *self.release_watch.borrow_mut() = None;
        }
        self.notify_state_changed();
    }

    pub fn automatically_downloads_updates(&self) -> bool {
        self.engine().is_some_and(|engine| engine.automatically_downloads_updates())
    }

    pub fn set_automatically_downloads_updates(&self, new_value: bool) {
        let Some(engine) = self.engine() else {
            return;
        };
        engine.set_automatically_downloads_updates(new_value);
        self.notify_state_changed();
    }

    pub fn allows_automatic_updates(&self) -> bool {
        self.engine().is_some_and(|engine| engine.allows_automatic_updates())
    }

    pub fn last_update_check_date(&self) -> Option<Date> {
        self.engine().and_then(|engine| engine.last_update_check_date())
    }

    pub fn can_check_for_updates(&self) -> bool {
        if !self.is_configured() {
            return false;
        }
        match self.engine() {
            Some(engine) => engine.can_check_for_updates(),
            None => self.startup_failure.borrow().is_some(),
        }
    }

    /// Whether this bundle carries the production Sparkle configuration at all
    /// (used by the Updates settings pane to explain a disabled updater).
    pub fn is_update_configuration_present(&self) -> bool {
        self.is_configured()
    }

    /// Whether this bundle carries the production Sparkle configuration.
    /// Dev/ad-hoc bundles omit `SUFeedURL` (and the whole Sparkle block) from
    /// their Info.plist, which disables the updater without a compile flag.
    fn is_configured(&self) -> bool {
        UpdateConfiguration::is_valid(NSBundle::mainBundle().infoDictionary().as_deref())
    }

    // MARK: - Lifecycle

    /// Called from `applicationDidFinishLaunching`. A dev bundle, or a
    /// production bundle whose configuration is still missing, simply stays
    /// quiet — the pill, panel, menu, and palette all report "not available".
    pub fn start(&self) {
        if self.engine.borrow().is_some() {
            return;
        }
        if !self.is_configured() {
            // Updater disabled for this bundle. Nothing to tear down later.
            return;
        }
        let driver = DownrightUpdateDriver::new(None);
        *self.driver.borrow_mut() = Some(driver.clone());
        let host: Weak<dyn UpdateDriverHost> = self.weak_self.clone();
        driver.set_host(Some(host));
        let Some(engine) = SparkleUpdateEngine::new(driver) else {
            // Misconfigured in a way the plist check missed; stay disabled.
            *self.driver.borrow_mut() = None;
            return;
        };
        let engine: Rc<dyn UpdateEngine> = Rc::new(engine);
        *self.engine.borrow_mut() = Some(engine.clone());
        engine.set_on_background_download_completed(Some(self.background_download_handler()));
        match engine.start() {
            Ok(()) => {
                let had_startup_failure = self.startup_failure.borrow().is_some();
                if had_startup_failure {
                    self.reduce(UpdateEvent::Dismissed);
                }
                *self.startup_failure.borrow_mut() = None;
                self.start_release_watch();
                // MainMenu is built before Sparkle starts. Revalidate its command
                // now that the live updater can answer capability checks; without
                // this notification the menu can remain inert for the whole launch.
                self.notify_state_changed();
            }
            Err(error) => {
                // Startup failure (bad feed, missing signing key): surface once,
                // quietly — the pill carries the warning until the app restarts.
                *self.engine.borrow_mut() = None;
                *self.driver.borrow_mut() = None;
                let failure = UpdateFailure::from_error(&error);
                *self.startup_failure.borrow_mut() = Some(failure.clone());
                self.reduce(UpdateEvent::UpdaterError(failure));
                self.notify_state_changed();
            }
        }
    }

    /// Shuts the coordinator down cleanly (used by tests; in production the
    /// process lives for the app's lifetime). Any pending capability is
    /// discarded, which is the only acceptable way for a teardown to happen.
    pub fn tear_down_for_testing(&self) {
        self.discard_all_capabilities();
        if let Some(engine) = self.engine() {
            engine.set_on_background_download_completed(None);
        }
        let watch = self.release_watch.borrow().clone();
        if let Some(watch) = watch {
            watch.stop();
        }
        *self.release_watch.borrow_mut() = None;
        *self.machine.borrow_mut() = UpdateStateMachine::new();
        *self.downloaded_update.borrow_mut() = None;
        *self.current_cycle_update.borrow_mut() = None;
        *self.panel.borrow_mut() = None;
        *self.engine.borrow_mut() = None;
        *self.driver.borrow_mut() = None;
        *self.startup_failure.borrow_mut() = None;
        self.is_expedited_install.set(false);
        *self.release_notes.borrow_mut() = UpdateReleaseNotesState::None;
        self.notify_state_changed();
    }

    // MARK: - Release watch

    /// Starts watching the appcast so a build published while the app is open
    /// reaches the pill in about a minute rather than on the next scheduled
    /// check. Only production bundles get here: `start()` has already refused
    /// to build an engine for a bundle without the Sparkle configuration.
    fn start_release_watch(&self) {
        if self.release_watch.borrow().is_some() {
            return;
        }
        if !self.engine().is_some_and(|engine| engine.automatically_checks_for_updates()) {
            return;
        }
        let feed = NSBundle::mainBundle()
            .infoDictionary()
            .and_then(|info| string_value(&info, "SUFeedURL"))
            .and_then(|feed| Url::from_nsstring(&feed));
        let Some(feed) = feed else {
            return;
        };
        let watch = ReleaseWatch::new(feed, None);
        let weak = self.weak_self.clone();
        watch.set_on_feed_changed(Some(Rc::new(move || {
            if let Some(coordinator) = weak.upgrade() {
                coordinator.release_feed_did_change();
            }
        })));
        *self.release_watch.borrow_mut() = Some(watch.clone());
        watch.start(true);
    }

    /// The appcast moved. All this does is ask Sparkle to look — the watch
    /// never parses the feed, so this is the full extent of its authority.
    ///
    /// A cycle already in flight is left alone: the reader is watching a
    /// download or reading a prompt, and restarting underneath them would
    /// replace what they are looking at with the same answer.
    pub fn release_feed_did_change(&self) {
        let Some(engine) = self.engine() else {
            return;
        };
        if !engine.is_running() || !engine.automatically_checks_for_updates() {
            return;
        }
        if !matches!(self.phase(), UpdatePhase::Idle) || self.downloaded_update.borrow().is_some() {
            return;
        }
        self.release_watch_trigger_count.set(self.release_watch_trigger_count.get() + 1);
        engine.check_for_updates_in_background();
    }

    // MARK: - User actions

    /// "Check for Updates…" from the menu, palette, settings, or the pill.
    pub fn check_for_updates(&self) {
        if !self.is_configured() {
            self.present_disabled_alert();
            return;
        }
        if self.engine().is_none() {
            self.start();
        }
        let Some(engine) = self.engine() else {
            if self.startup_failure.borrow().is_some() {
                self.show_panel();
            }
            return;
        };
        engine.check_for_updates();
    }

    pub fn show_panel(&self) {
        self.panel_show_count.set(self.panel_show_count.get() + 1);
        if self.suppress_ui_for_testing.get() {
            return;
        }
        // panel = panel ?? UpdateWindowController(coordinator: self)
        if self.panel.borrow().is_none() {
            let factory = self.panel_factory.borrow().clone();
            let created = match (factory, self.weak_self.upgrade()) {
                (Some(factory), Some(coordinator)) => Some(factory(&coordinator)),
                _ => None,
            };
            *self.panel.borrow_mut() = created;
        }
        let panel = self.panel.borrow().clone();
        if let Some(panel) = &panel {
            panel.show_window();
        }
        if let Some(panel) = &panel {
            panel.make_key_and_order_front();
        }
    }

    pub fn close_panel(&self) {
        if self.suppress_ui_for_testing.get() {
            return;
        }
        let panel = self.panel.borrow().clone();
        if let Some(panel) = panel {
            panel.perform_close();
        }
    }

    // MARK: Update panel actions (routed to the armed capability)

    /// The pill's one press. Everything the panel's Update button does, minus
    /// the panel: a reader who clicked a control labelled "Update Now" has
    /// already answered the question the panel would ask them.
    ///
    /// Unsaved work is deliberately *not* handled here. Sparkle's install
    /// asks the app to terminate, which runs `applicationShouldTerminate` and
    /// the one policy this app has for dirty buffers — ask, and cancel the
    /// quit if the reader cancels. A second flush here would be a second
    /// policy for the same moment, and the state machine already carries
    /// `.waitingForTermination` for the cancelled case.
    pub fn user_did_press_update_now(&self) {
        match self.phase() {
            UpdatePhase::WaitingForTermination => {
                // The install is underway; only the quit is outstanding.
                self.user_did_retry_termination();
            }
            UpdatePhase::Failed(..) => self.show_panel(),
            UpdatePhase::Downloading { .. } | UpdatePhase::Extracting { .. } | UpdatePhase::Checking { .. } => {
                // Already in flight. The panel is where the cancel button lives.
                self.show_panel();
            }
            UpdatePhase::Informational(metadata) => self.user_did_request_learn_more(&metadata),
            _ => {
                if is_armed(&self.ready_reply) || is_armed(&self.choice_reply) {
                    self.user_did_choose_install();
                } else if self.pending_update().is_some()
                    && let Some(engine) = self.engine()
                    && engine.is_running()
                {
                    // A background cycle finished without leaving a prompt armed.
                    // Re-entering Sparkle presents the same build for install, and
                    // `isExpeditedInstall` keeps that cycle from opening a window
                    // to ask what the press already answered.
                    //
                    // Straight to the engine rather than through `checkForUpdates`:
                    // that path exists for the *menu*, and gates on the bundle's
                    // updater configuration so it can explain a disabled updater.
                    // A pill only exists when an update is already pending, so a
                    // press can never be the case that alert is for.
                    self.is_expedited_install.set(true);
                    engine.check_for_updates();
                }
            }
        }
        self.notify_state_changed();
    }

    /// Update / Update & Relaunch — "do it now".
    pub fn user_did_choose_install(&self) {
        if is_armed(&self.ready_reply) {
            consume(&self.ready_reply, UpdateUserChoice::Install);
        } else if is_armed(&self.choice_reply) {
            consume(&self.choice_reply, UpdateUserChoice::Install);
        } else if self.downloaded_update.borrow().is_some() {
            // A background download with no pending prompt: asking Sparkle to
            // check again presents the already-downloaded update for install.
            self.check_for_updates();
        }
    }

    /// Retry after a failed check/download. The failed cycle must finish
    /// (acknowledge) before Sparkle will allow a new one, so the new check is
    /// scheduled for the next runloop turn.
    pub fn user_did_retry(&self) {
        if !matches!(self.phase(), UpdatePhase::Failed(..)) {
            return;
        }
        consume(&self.acknowledgement, ());
        self.discard_all_capabilities();
        self.reduce(UpdateEvent::Dismissed);
        *self.downloaded_update.borrow_mut() = None;
        let weak = self.weak_self.clone();
        async_main(move || {
            if let Some(coordinator) = weak.upgrade() {
                coordinator.check_for_updates();
            }
        });
    }

    pub fn user_did_choose_later(&self) {
        if is_armed(&self.choice_reply) {
            // The update is dismissed until Sparkle reminds the user later;
            // the machine leaves `.available` so the pill stops offering it.
            consume(&self.choice_reply, UpdateUserChoice::Later);
            self.reduce(UpdateEvent::Dismissed);
            self.close_panel();
        } else if is_armed(&self.ready_reply) {
            // Already downloaded: it still installs on quit, so the pill keeps
            // its "Restart to Update" state and only the panel goes away.
            consume(&self.ready_reply, UpdateUserChoice::Later);
            self.close_panel();
        } else if matches!(self.phase(), UpdatePhase::UpToDate) {
            // Keep the no-update result visible until the user has had a
            // chance to read it. This acknowledgement is deliberately delayed
            // until the explicit OK action, or until window dismissal handles
            // the close-button path below.
            self.user_did_acknowledge();
            self.close_panel();
        } else {
            self.close_panel();
        }
        self.notify_state_changed();
    }

    pub fn user_did_choose_skip(&self) {
        consume(&self.choice_reply, UpdateUserChoice::Skip);
        self.reduce(UpdateEvent::Dismissed);
        self.close_panel();
        self.notify_state_changed();
    }

    pub fn user_did_cancel_check(&self) {
        consume(&self.check_cancellation, ());
        self.reduce(UpdateEvent::CheckCancelled);
        self.close_panel();
        self.notify_state_changed();
    }

    pub fn user_did_cancel_download(&self) {
        consume(&self.download_cancellation, ());
        self.reduce(UpdateEvent::DownloadCancelled);
        self.close_panel();
        self.notify_state_changed();
    }

    pub fn user_did_retry_termination(&self) {
        consume(&self.retry_termination, ());
    }

    pub fn user_did_acknowledge(&self) {
        consume(&self.acknowledgement, ());
    }

    pub fn user_did_request_learn_more(&self, metadata: &UpdateMetadata) {
        let url = metadata.info_url.as_ref().filter(|url| url.scheme().is_some_and(|scheme| upleft_swift_text::str_eq(&scheme, "https")));
        let Some(url) = url else {
            NSBeep();
            return;
        };
        NSWorkspace::sharedWorkspace().openURL(url.as_nsurl());
    }

    /// Panel closed by the user without a button. Where a choice is pending,
    /// closing means "later"; otherwise it is a pure dismissal. A pending
    /// check or download cancellation is invoked so Sparkle's cycle is never
    /// left with a capability it will wait on forever.
    pub fn user_did_dismiss_panel(&self) {
        if is_armed(&self.choice_reply) {
            consume(&self.choice_reply, UpdateUserChoice::Later);
            self.reduce(UpdateEvent::Dismissed);
        } else if is_armed(&self.ready_reply) {
            consume(&self.ready_reply, UpdateUserChoice::Later);
        } else if is_armed(&self.acknowledgement) {
            consume(&self.acknowledgement, ());
        } else if is_armed(&self.retry_termination) {
            // An install-in-progress keeps going; the panel can go away.
            discard(&self.retry_termination);
        } else if is_armed(&self.check_cancellation) {
            // Closing the panel mid-check cancels it; Sparkle calls nothing
            // after a cancelled check, so the machine leaves `.checking` here.
            consume(&self.check_cancellation, ());
            self.reduce(UpdateEvent::CheckCancelled);
        } else if is_armed(&self.download_cancellation) {
            // Closing the panel mid-download cancels it.
            consume(&self.download_cancellation, ());
            self.reduce(UpdateEvent::DownloadCancelled);
        } else if matches!(self.phase(), UpdatePhase::ReadyToRelaunch) {
            // Keep background downloadedUpdate so the restart titlebar pill remains visible.
        } else {
            *self.downloaded_update.borrow_mut() = None;
        }
        self.notify_state_changed();
    }

    // MARK: - Capabilities

    fn discard_all_capabilities(&self) {
        discard(&self.check_cancellation);
        discard(&self.download_cancellation);
        discard(&self.choice_reply);
        discard(&self.ready_reply);
        discard(&self.retry_termination);
        discard(&self.acknowledgement);
    }

    // MARK: - Background downloads

    fn background_download_completed(&self, display_version: &str) {
        // The machine stays idle; remember the download so the pill can offer
        // "Restart to Update" until the update is installed or dismissed.
        let differs = match &*self.downloaded_update.borrow() {
            Some(update) => !upleft_swift_text::str_eq(&update.display_version_string, display_version),
            None => true,
        };
        if differs {
            *self.downloaded_update.borrow_mut() = Some(UpdateMetadata {
                version_string: display_version.to_owned(),
                display_version_string: display_version.to_owned(),
                title: None,
                item_description: None,
                release_notes_url: None,
                info_url: None,
                content_length: 0,
                is_information_only: false,
                is_major_upgrade: false,
                is_critical: false,
                minimum_system_version: None,
            });
        }
        self.notify_state_changed();
    }

    // MARK: - Derived UI models

    /// The pill model across every document titlebar and the start window.
    pub fn pill_model(&self) -> Option<UpdatePillModel> {
        match self.phase() {
            UpdatePhase::Idle => {
                let downloaded = self.downloaded_update.borrow().clone()?;
                Some(UpdatePillModel::UpdateNow { version: downloaded.display_version_string, is_ready: true })
            }
            UpdatePhase::Checking { .. } => None,
            UpdatePhase::Available(metadata, stage) => Some(UpdatePillModel::UpdateNow {
                version: metadata.display_version_string,
                is_ready: stage == UpdateStage::Downloaded || stage == UpdateStage::Installing,
            }),
            UpdatePhase::Downloading { received, expected } => {
                let fraction = expected.map(|expected| received as f64 / swift_max(1, expected) as f64);
                Some(UpdatePillModel::Progress(self.update_label(), fraction))
            }
            UpdatePhase::Extracting { progress } => Some(UpdatePillModel::Progress("Updating…".into(), progress)),
            UpdatePhase::ReadyToRelaunch => {
                Some(UpdatePillModel::UpdateNow { version: self.ready_version_string(), is_ready: true })
            }
            // The install has begun and the app has to quit to finish it.
            UpdatePhase::WaitingForTermination => Some(UpdatePillModel::RestartToUpdate),
            UpdatePhase::Installing => UpdatePillModel::HIDDEN, // the app is quitting; nothing to click
            UpdatePhase::Informational(metadata) => Some(UpdatePillModel::Informational(metadata.display_version_string)),
            UpdatePhase::UpToDate => UpdatePillModel::HIDDEN,
            UpdatePhase::Failed(..) => Some(UpdatePillModel::Warning),
        }
    }

    /// The update the pill is currently offering, for the hover notes. Mirrors
    /// what the panel resolves so both surfaces describe the same build.
    pub fn pending_update(&self) -> Option<UpdateMetadata> {
        match self.phase() {
            UpdatePhase::Available(metadata, _) | UpdatePhase::Informational(metadata) => Some(metadata),
            _ => self.downloaded_update.borrow().clone().or_else(|| self.current_cycle_update.borrow().clone()),
        }
    }

    /// The version to name once extraction has finished and the metadata is no
    /// longer carried by the phase itself.
    fn ready_version_string(&self) -> String {
        self.pending_update().map(|update| update.display_version_string).unwrap_or_default()
    }

    /// Short label for progress states: the target version once known, else
    /// a generic "Updating…".
    fn update_label(&self) -> String {
        if let UpdatePhase::Available(metadata, _) = self.phase() {
            return format!("Update {}", metadata.display_version_string);
        }
        if let Some(downloaded) = &*self.downloaded_update.borrow() {
            return format!("Update {}", downloaded.display_version_string);
        }
        "Updating…".into()
    }

    /// "Version 1.0.0 (42) · Last check: …" for the Updates settings pane.
    pub fn status_line(&self) -> String {
        let bundle = NSBundle::mainBundle();
        let info_string = |key: &str| {
            bundle
                .objectForInfoDictionaryKey(&NSString::from_str(key))
                .and_then(|value| value.downcast::<NSString>().ok())
                .map(|value| value.to_string())
        };
        let version = info_string("CFBundleShortVersionString").unwrap_or_else(|| "—".into());
        let build = info_string("CFBundleVersion").unwrap_or_else(|| "—".into());
        let check = self
            .last_update_check_date()
            .map(|date| {
                let date = NSDate::dateWithTimeIntervalSinceReferenceDate(date.time_interval_since_reference_date);
                NSDateFormatter::localizedStringFromDate_dateStyle_timeStyle(
                    &date,
                    NSDateFormatterStyle::MediumStyle,
                    NSDateFormatterStyle::ShortStyle,
                )
                .to_string()
            })
            .unwrap_or_else(|| "never".into());
        format!("Version {version} ({build}) · Last check: {check}")
    }

    fn notify_state_changed(&self) {
        let center = NSNotificationCenter::defaultCenter();
        // SAFETY: a plain post; observers run synchronously on this thread.
        unsafe {
            center.postNotificationName_object(
                &NSString::from_str(UpdateCoordinator::STATE_DID_CHANGE),
                Some(&self.notification_object),
            );
        }
    }

    fn present_disabled_alert(&self) {
        if self.suppress_ui_for_testing.get() {
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str("Updates aren't available for this build."));
        alert.setInformativeText(&NSString::from_str(
            "This copy of Upleft doesn't carry the production update configuration.",
        ));
        alert.setAlertStyle(NSAlertStyle::Informational);
        alert.runModal();
    }
}

// MARK: - UpdateDriverHost

impl UpdateDriverHost for UpdateCoordinator {
    fn driver_did_begin_user_check(&self, cancellation: Callback) {
        *self.release_notes.borrow_mut() = UpdateReleaseNotesState::None;
        *self.current_cycle_update.borrow_mut() = None;
        self.current_cycle_is_user_initiated.set(true);
        discard(&self.check_cancellation);
        *self.check_cancellation.borrow_mut() = Some(Capability::new(move |()| cancellation()));
        self.reduce(UpdateEvent::UserInitiatedCheckBegan);
        if !self.is_expedited_install.get() {
            self.show_panel();
        }
        self.notify_state_changed();
    }

    fn driver_did_find_update(
        &self,
        metadata: UpdateMetadata,
        stage: UpdateStage,
        user_initiated: bool,
        reply: Reply<UpdateUserChoice>,
    ) {
        *self.release_notes.borrow_mut() = UpdateReleaseNotesState::None;
        discard(&self.check_cancellation);
        discard(&self.choice_reply);
        discard(&self.ready_reply);
        *self.choice_reply.borrow_mut() = Some(Capability::from_box(reply));
        *self.downloaded_update.borrow_mut() = None;
        *self.current_cycle_update.borrow_mut() = Some(metadata.clone());
        self.current_cycle_is_user_initiated.set(user_initiated);
        self.reduce(UpdateEvent::UpdateFound(metadata, stage));
        if self.is_expedited_install.get() {
            // The press was the answer. Reply now rather than opening a window
            // to ask it again; `consume` keeps the exactly-once contract.
            consume(&self.choice_reply, UpdateUserChoice::Install);
        } else if user_initiated {
            // A scheduled/automatic presentation stays on the pill; the panel
            // opens only when the user asked for the update themselves.
            self.show_panel();
        }
        self.notify_state_changed();
    }

    fn driver_did_receive_release_notes(&self, data: Vec<u8>) {
        *self.release_notes.borrow_mut() = UpdateReleaseNotesState::Loaded(data);
        self.notify_state_changed();
    }

    fn driver_did_fail_to_download_release_notes(&self, _error: &NSError) {
        *self.release_notes.borrow_mut() = UpdateReleaseNotesState::Failed;
        self.notify_state_changed();
    }

    fn driver_did_find_no_update(&self, user_initiated: bool, acknowledgement: Callback) {
        *self.release_notes.borrow_mut() = UpdateReleaseNotesState::None;
        self.is_expedited_install.set(false);
        *self.current_cycle_update.borrow_mut() = None;
        discard(&self.check_cancellation);
        discard(&self.choice_reply);
        discard(&self.ready_reply);
        self.current_cycle_is_user_initiated.set(false);
        discard(&self.acknowledgement);
        *self.acknowledgement.borrow_mut() = Some(Capability::new(move |()| acknowledgement()));
        self.reduce(UpdateEvent::UpdateNotFound { user_initiated });
        if user_initiated {
            self.show_panel();
            // Keep the result panel open until the user dismisses it. Sparkle
            // waits for this acknowledgement, which is exactly what lets the
            // custom UI communicate a useful no-update result.
        } else {
            // Quiet background check; nothing to show, acknowledge immediately.
            self.user_did_acknowledge();
        }
        self.notify_state_changed();
    }

    fn driver_did_encounter_error(&self, error: &NSError, acknowledgement: Callback) {
        *self.release_notes.borrow_mut() = UpdateReleaseNotesState::None;
        // A press that could not be honoured owes an explanation, so the
        // failure path deliberately drops back to the ordinary panel.
        self.is_expedited_install.set(false);
        let user_initiated = self.current_cycle_is_user_initiated.get();
        self.current_cycle_is_user_initiated.set(false);
        discard(&self.acknowledgement);
        *self.acknowledgement.borrow_mut() = Some(Capability::new(move |()| acknowledgement()));
        if !user_initiated {
            // A background check that could not reach the feed is a non-event.
            // Sparkle's cycle has to be finished here, or `canCheckForUpdates`
            // stays false until relaunch and every window carries an orange
            // alarm badge because a laptop was opened without wifi — which is
            // exactly the alarm DESIGN.md says not to raise.
            self.user_did_acknowledge();
            self.discard_all_capabilities();
            self.reduce(UpdateEvent::Dismissed);
            self.notify_state_changed();
            return;
        }
        self.reduce(UpdateEvent::UpdaterError(UpdateFailure::from_error(error)));
        self.show_panel();
        self.notify_state_changed();
    }

    fn driver_did_begin_download(&self, cancellation: Callback) {
        discard(&self.download_cancellation);
        *self.download_cancellation.borrow_mut() = Some(Capability::new(move |()| cancellation()));
        self.reduce(UpdateEvent::DownloadInitiated);
        // Automatic background downloads — and the one a press started —
        // proceed silently on the pill.
        if self.current_cycle_is_user_initiated.get() && !self.is_expedited_install.get() {
            self.show_panel();
        }
        self.notify_state_changed();
    }

    fn driver_did_receive_expected_length(&self, length: u64) {
        self.reduce(UpdateEvent::ExpectedLength(length));
        self.notify_state_changed();
    }

    fn driver_did_receive_data(&self, length: u64) {
        self.reduce(UpdateEvent::DataReceived(length));
        self.notify_state_changed();
    }

    fn driver_did_begin_extraction(&self) {
        self.reduce(UpdateEvent::ExtractionBegan);
        self.notify_state_changed();
    }

    fn driver_did_receive_extraction_progress(&self, progress: f64) {
        self.reduce(UpdateEvent::ExtractionProgress(progress));
        self.notify_state_changed();
    }

    fn driver_did_become_ready_to_relaunch(&self, reply: Reply<UpdateUserChoice>) {
        discard(&self.download_cancellation);
        discard(&self.choice_reply);
        discard(&self.ready_reply);
        *self.ready_reply.borrow_mut() = Some(Capability::from_box(reply));
        self.reduce(UpdateEvent::ReadyToInstallAndRelaunch);
        if self.is_expedited_install.get() {
            consume(&self.ready_reply, UpdateUserChoice::Install);
        } else if self.current_cycle_is_user_initiated.get() {
            // A background download that became ready stays on the pill; the
            // panel opens when the pill is clicked.
            self.show_panel();
        }
        self.notify_state_changed();
    }

    fn driver_did_begin_installation(&self, application_terminated: bool, retry_termination: Callback) {
        discard(&self.retry_termination);
        discard(&self.download_cancellation);
        *self.retry_termination.borrow_mut() = Some(Capability::new(move |()| retry_termination()));
        self.is_expedited_install.set(false);
        self.reduce(UpdateEvent::InstallingUpdate { application_terminated });
        if !application_terminated {
            self.show_panel(); // give the user Retry / Later for a delayed quit
        }
        self.notify_state_changed();
    }

    fn driver_did_finish_installation(&self, relaunched: bool, acknowledgement: Callback) {
        discard(&self.acknowledgement);
        *self.acknowledgement.borrow_mut() = Some(Capability::new(move |()| acknowledgement()));
        self.reduce(UpdateEvent::UpdateInstalled { relaunched });
        self.current_cycle_is_user_initiated.set(false);
        self.is_expedited_install.set(false);
        *self.current_cycle_update.borrow_mut() = None;
        *self.downloaded_update.borrow_mut() = None;
        self.user_did_acknowledge();
        self.close_panel();
        self.notify_state_changed();
    }

    fn driver_did_dismiss(&self) {
        self.discard_all_capabilities();
        self.current_cycle_is_user_initiated.set(false);
        self.is_expedited_install.set(false);
        *self.current_cycle_update.borrow_mut() = None;
        self.reduce(UpdateEvent::Dismissed);
        *self.downloaded_update.borrow_mut() = None;
        *self.release_notes.borrow_mut() = UpdateReleaseNotesState::None;
        self.close_panel();
        self.notify_state_changed();
    }

    fn driver_did_request_focus(&self) {
        self.show_panel();
    }
}
