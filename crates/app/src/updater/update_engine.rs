//! Port of `Sources/DownrightApp/Updater/UpdateEngine.swift`.
//!
//! Swift imports Sparkle in this file. The port reaches `SPUUpdater` through
//! [`SpuUpdater`], a trait that names every call the engine makes into it, so
//! that upleft-app does not need Sparkle at link time: only the app binary
//! links the framework, as only Downright's host app does.
//! [`super::sparkle`] implements the trait over the real `SPUUpdater`, and
//! [`super::sparkle::install`] hands its constructor to
//! [`set_spu_updater_factory`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2_foundation::{NSBundle, NSError, NSString};
use upleft_foundation::date::Date;

use super::downright_update_driver::{DownrightUpdateDriver, SuAppcastItem};
use super::update_coordinator::UpdateConfiguration;

/// `((String) -> Void)?`: the background-download callback.
pub type BackgroundDownloadHandler = Rc<dyn Fn(&str)>;

/// The updater behind the coordinator. Production is `SparkleUpdateEngine`;
/// tests inject `FakeUpdateEngine` so the coordinator's logic — every state
/// transition, every exactly-once reply, every settings read — runs without
/// the framework or the network.
///
/// Swift's protocol is `@MainActor` and class-bound; the port's methods take
/// `&self` and implementors keep their state in cells, as the Swift classes'
/// mutable properties are.
pub trait UpdateEngine {
    /// True once `start()` has succeeded and the engine is live.
    fn is_running(&self) -> bool;

    /// `func start() throws`. The error is the `NSError` Swift's `error as
    /// NSError` would produce.
    fn start(&self) -> Result<(), Retained<NSError>>;
    fn check_for_updates(&self);
    fn check_for_updates_in_background(&self);

    /// Whether a user-initiated check is currently allowed (menu validation).
    fn can_check_for_updates(&self) -> bool;

    // Settings. The production engine reads/writes Sparkle's *own* persisted
    // properties — Downright's JSON preferences never duplicate them.
    fn automatically_checks_for_updates(&self) -> bool;
    fn set_automatically_checks_for_updates(&self, value: bool);
    fn automatically_downloads_updates(&self) -> bool;
    fn set_automatically_downloads_updates(&self, value: bool);
    fn allows_automatic_updates(&self) -> bool;
    fn update_check_interval(&self) -> f64;
    fn set_update_check_interval(&self, value: f64);
    fn last_update_check_date(&self) -> Option<Date>;

    // Delegate callbacks the coordinator needs to surface.
    /// A background (automatic) download finished. Argument is the display version.
    fn on_background_download_completed(&self) -> Option<BackgroundDownloadHandler>;
    fn set_on_background_download_completed(&self, handler: Option<BackgroundDownloadHandler>);
}

// MARK: - Production

/// Every call `SparkleUpdateEngine` makes into its `SPUUpdater`, one method
/// per Objective-C message. [`super::sparkle`] implements it over the real
/// `SPUUpdater`; tests may install a stand-in.
pub trait SpuUpdater {
    /// `clearFeedURLFromUserDefaults()`.
    fn clear_feed_url_from_user_defaults(&self);
    /// `start() throws` (`-startUpdater:`).
    fn start(&self) -> Result<(), Retained<NSError>>;
    /// `checkForUpdates()`.
    fn check_for_updates(&self);
    /// `checkForUpdatesInBackground()`.
    fn check_for_updates_in_background(&self);
    /// `canCheckForUpdates`.
    fn can_check_for_updates(&self) -> bool;
    /// `automaticallyChecksForUpdates`.
    fn automatically_checks_for_updates(&self) -> bool;
    fn set_automatically_checks_for_updates(&self, value: bool);
    /// `automaticallyDownloadsUpdates`.
    fn automatically_downloads_updates(&self) -> bool;
    fn set_automatically_downloads_updates(&self, value: bool);
    /// `allowsAutomaticUpdates`.
    fn allows_automatic_updates(&self) -> bool;
    /// `updateCheckInterval`.
    fn update_check_interval(&self) -> f64;
    fn set_update_check_interval(&self, value: f64);
    /// `lastUpdateCheckDate`.
    fn last_update_check_date(&self) -> Option<Date>;
}

/// `SPUUpdater(hostBundle: host, applicationBundle: host, userDriver:
/// userDriver, delegate: notifier)`. It answers `None` when the framework is
/// not loaded in this process (see [`super::sparkle::make_updater`]).
pub type SpuUpdaterFactory = Box<
    dyn Fn(
        &NSBundle,
        &NSBundle,
        &Rc<DownrightUpdateDriver>,
        &Rc<BackgroundDownloadNotifier>,
    ) -> Option<Rc<dyn SpuUpdater>>,
>;

thread_local! {
    static SPU_UPDATER_FACTORY: RefCell<Option<SpuUpdaterFactory>> = const { RefCell::new(None) };
}

/// Installs the `SPUUpdater` constructor ([`super::sparkle::install`] does it
/// at start-up). Until something installs one, or while it answers `None`,
/// `SparkleUpdateEngine::new` answers `None` even for a valid configuration,
/// so the coordinator stays in its ordinary disabled state.
pub fn set_spu_updater_factory(factory: Option<SpuUpdaterFactory>) {
    SPU_UPDATER_FACTORY.with(|slot| *slot.borrow_mut() = factory);
}

/// Owns an `SPUUpdater` configured with the app's Info.plist feed settings and
/// Downright's custom user driver. All calls must be made on the main thread.
///
/// The updater's delegate is `BackgroundDownloadNotifier`, not the engine
/// itself: `SPUUpdater` takes its delegate at construction, before `self` is
/// usable, and with automatic downloads enabled that delegate is the *only*
/// channel that learns a silent background cycle finished — Sparkle routes
/// those cycles through `SPUAutomaticUpdateDriver`, which never presents
/// through the user driver at all. Without this wiring a staged update would
/// install on quit with no pill and no panel, which is exactly the silence
/// the coordinator's `downloadedUpdate` state exists to prevent.
pub struct SparkleUpdateEngine {
    updater: Rc<dyn SpuUpdater>,
    #[allow(dead_code)] // Swift keeps the driver alive for the updater's lifetime.
    driver: Rc<DownrightUpdateDriver>,
    notifier: Rc<BackgroundDownloadNotifier>,
    is_running: Cell<bool>,
}

impl SparkleUpdateEngine {
    /// `init?(userDriver:)`.
    pub fn new(user_driver: Rc<DownrightUpdateDriver>) -> Option<SparkleUpdateEngine> {
        let host = NSBundle::mainBundle();
        // A dev/ad-hoc bundle, or a release bundle with incomplete updater
        // metadata, cannot be constructed usefully. Refuse to hand partial
        // configuration to Sparkle; the coordinator stays in its normal
        // disabled state and the production bundle gate catches the release
        // mistake before users ever receive it.
        if !UpdateConfiguration::is_valid(host.infoDictionary().as_deref()) {
            return None;
        }
        let notifier = Rc::new(BackgroundDownloadNotifier::default());
        let updater = SPU_UPDATER_FACTORY
            .with(|slot| slot.borrow().as_ref().and_then(|factory| factory(&host, &host, &user_driver, &notifier)))?;
        Some(SparkleUpdateEngine { updater, driver: user_driver, notifier, is_running: Cell::new(false) })
    }
}

impl UpdateEngine for SparkleUpdateEngine {
    fn is_running(&self) -> bool {
        self.is_running.get()
    }

    fn start(&self) -> Result<(), Retained<NSError>> {
        if self.is_running.get() {
            return Ok(());
        }
        self.updater.clear_feed_url_from_user_defaults(); // never override the Info.plist feed
        self.updater.start()?;
        self.is_running.set(true);
        Ok(())
    }

    fn check_for_updates(&self) {
        if !self.is_running.get() {
            return;
        }
        self.updater.check_for_updates();
    }

    fn check_for_updates_in_background(&self) {
        if !self.is_running.get() {
            return;
        }
        self.updater.check_for_updates_in_background();
    }

    fn can_check_for_updates(&self) -> bool {
        self.updater.can_check_for_updates()
    }

    fn automatically_checks_for_updates(&self) -> bool {
        self.updater.automatically_checks_for_updates()
    }

    fn set_automatically_checks_for_updates(&self, value: bool) {
        self.updater.set_automatically_checks_for_updates(value);
    }

    fn automatically_downloads_updates(&self) -> bool {
        self.updater.automatically_downloads_updates()
    }

    fn set_automatically_downloads_updates(&self, value: bool) {
        self.updater.set_automatically_downloads_updates(value);
    }

    fn allows_automatic_updates(&self) -> bool {
        self.updater.allows_automatic_updates()
    }

    fn update_check_interval(&self) -> f64 {
        self.updater.update_check_interval()
    }

    fn set_update_check_interval(&self, value: f64) {
        self.updater.set_update_check_interval(value);
    }

    fn last_update_check_date(&self) -> Option<Date> {
        self.updater.last_update_check_date()
    }

    fn on_background_download_completed(&self) -> Option<BackgroundDownloadHandler> {
        self.notifier.handler.borrow().clone()
    }

    fn set_on_background_download_completed(&self, handler: Option<BackgroundDownloadHandler>) {
        *self.notifier.handler.borrow_mut() = handler;
    }
}

/// The one `SPUUpdaterDelegate` callback Downright needs. A download finished
/// — including the silent background ones that never reach the user driver —
/// so the coordinator can raise the "Restart to Update" pill on every surface.
#[derive(Default)]
pub struct BackgroundDownloadNotifier {
    pub handler: RefCell<Option<BackgroundDownloadHandler>>,
}

impl BackgroundDownloadNotifier {
    /// `updater(_:didDownloadUpdate:)`. The Objective-C `SPUUpdaterDelegate`
    /// (`super::sparkle::BackgroundDownloadNotifierObject`) forwards here, on
    /// the main thread.
    pub fn updater_did_download_update(&self, item: &dyn SuAppcastItem) {
        let handler = self.handler.borrow().clone();
        if let Some(handler) = handler {
            handler(&item.display_version_string());
        }
    }
}

/// `enum UpdateStartError: Error`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateStartError {
    UpdaterRefusedToStart,
}

impl UpdateStartError {
    /// Swift's `error as NSError` for this enum: domain
    /// `"<module>.UpdateStartError"`, code the case index, no user info. The
    /// module is `DownrightApp`.
    pub fn to_ns_error(self) -> Retained<NSError> {
        let code = match self {
            UpdateStartError::UpdaterRefusedToStart => 0,
        };
        NSError::new(code, &NSString::from_str("DownrightApp.UpdateStartError"))
    }
}

// MARK: - Test fake

/// Deterministic stand-in for Sparkle. The coordinator is tested against this:
/// scripts emit driver callbacks and settings reads/writes exactly like the
/// framework would, with no network and no binary framework in the loop.
pub struct FakeUpdateEngine {
    is_running: Cell<bool>,
    pub start_throws: Cell<bool>,

    pub on_background_download_completed: RefCell<Option<BackgroundDownloadHandler>>,

    pub _can_check_for_updates: Cell<bool>,

    pub automatically_checks_for_updates: Cell<bool>,
    pub automatically_downloads_updates: Cell<bool>,
    pub allows_automatic_updates: Cell<bool>,
    pub update_check_interval: Cell<f64>,
    pub last_update_check_date: Cell<Option<Date>>,

    background_check_count: Cell<isize>,
    foreground_check_count: Cell<isize>,
}

impl Default for FakeUpdateEngine {
    fn default() -> FakeUpdateEngine {
        FakeUpdateEngine {
            is_running: Cell::new(false),
            start_throws: Cell::new(false),
            on_background_download_completed: RefCell::new(None),
            _can_check_for_updates: Cell::new(true),
            automatically_checks_for_updates: Cell::new(true),
            automatically_downloads_updates: Cell::new(true),
            allows_automatic_updates: Cell::new(true),
            update_check_interval: Cell::new(86_400.0),
            last_update_check_date: Cell::new(None),
            background_check_count: Cell::new(0),
            foreground_check_count: Cell::new(0),
        }
    }
}

impl FakeUpdateEngine {
    pub fn new() -> Rc<FakeUpdateEngine> {
        Rc::new(FakeUpdateEngine::default())
    }

    pub fn background_check_count(&self) -> isize {
        self.background_check_count.get()
    }

    pub fn foreground_check_count(&self) -> isize {
        self.foreground_check_count.get()
    }

    // Test scripting helpers.

    pub fn complete_background_download(&self, display_version: &str) {
        let handler = self.on_background_download_completed.borrow().clone();
        if let Some(handler) = handler {
            handler(display_version);
        }
    }
}

impl UpdateEngine for FakeUpdateEngine {
    fn is_running(&self) -> bool {
        self.is_running.get()
    }

    fn start(&self) -> Result<(), Retained<NSError>> {
        if self.start_throws.get() {
            return Err(UpdateStartError::UpdaterRefusedToStart.to_ns_error());
        }
        self.is_running.set(true);
        Ok(())
    }

    fn check_for_updates(&self) {
        if !self.is_running.get() {
            return;
        }
        self.foreground_check_count.set(self.foreground_check_count.get() + 1);
    }

    fn check_for_updates_in_background(&self) {
        if !self.is_running.get() {
            return;
        }
        self.background_check_count.set(self.background_check_count.get() + 1);
    }

    fn can_check_for_updates(&self) -> bool {
        self._can_check_for_updates.get()
    }

    fn automatically_checks_for_updates(&self) -> bool {
        self.automatically_checks_for_updates.get()
    }

    fn set_automatically_checks_for_updates(&self, value: bool) {
        self.automatically_checks_for_updates.set(value);
    }

    fn automatically_downloads_updates(&self) -> bool {
        self.automatically_downloads_updates.get()
    }

    fn set_automatically_downloads_updates(&self, value: bool) {
        self.automatically_downloads_updates.set(value);
    }

    fn allows_automatic_updates(&self) -> bool {
        self.allows_automatic_updates.get()
    }

    fn update_check_interval(&self) -> f64 {
        self.update_check_interval.get()
    }

    fn set_update_check_interval(&self, value: f64) {
        self.update_check_interval.set(value);
    }

    fn last_update_check_date(&self) -> Option<Date> {
        self.last_update_check_date.get()
    }

    fn on_background_download_completed(&self) -> Option<BackgroundDownloadHandler> {
        self.on_background_download_completed.borrow().clone()
    }

    fn set_on_background_download_completed(&self, handler: Option<BackgroundDownloadHandler>) {
        *self.on_background_download_completed.borrow_mut() = handler;
    }
}
