//! Sparkle 2.9.6, the half of `UpdateEngine.swift` and
//! `DownrightUpdateDriver.swift` that talks to the framework.
//!
//! Downright links Sparkle in its host app only. Upleft keeps that split:
//! upleft-app never references a Sparkle symbol, so it links without the
//! framework, and only the app binary links `Sparkle.framework`. Every class
//! and protocol below is looked up in the Objective-C runtime when first
//! used (`extern_class!` resolves through `objc_getClass`, and
//! [`is_available`] checks for each one first). In a process without the
//! framework, [`make_updater`] answers `None` and the coordinator stays in its
//! ordinary disabled state.
//!
//! What lives here:
//! - the Objective-C declarations the two Swift files use: `SPUUpdater`,
//!   `SUAppcastItem`, `SPUUserUpdateState`, `SPUDownloadData`,
//!   `SPUUpdatePermissionRequest`, `SUUpdatePermissionResponse`, and the
//!   `SPUUserDriver` and `SPUUpdaterDelegate` protocols;
//! - [`SparkleUpdater`], the [`SpuUpdater`] over a real `SPUUpdater`;
//! - [`DownrightUpdateDriverObject`] (runtime name `DownrightUpdateDriver`),
//!   the `SPUUserDriver` that forwards to the ported [`DownrightUpdateDriver`];
//! - [`BackgroundDownloadNotifierObject`] (runtime name
//!   `BackgroundDownloadNotifier`), the `SPUUpdaterDelegate` that forwards to
//!   the ported [`BackgroundDownloadNotifier`];
//! - [`install`], which the app calls at start-up, before
//!   `UpdateCoordinator::shared(mtm).start()`.
//!
//! Threading: the ported driver and notifier are main-thread objects (`Rc`).
//! Sparkle calls its user driver on the main thread, and both bridge classes
//! call straight through there. A call that arrives on another thread (the
//! delegate's, which Swift handles by hopping to the main actor in the
//! coordinator's handler) is carried to the main queue first, with the
//! Objective-C arguments retained; nothing main-thread-only is touched
//! before the hop.

use std::ffi::CStr;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::DynBlock;
use dispatch2::DispatchQueue;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, NSObjectProtocol, ProtocolObject};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, extern_class, extern_methods,
    extern_protocol, msg_send,
};
use objc2_foundation::{NSArray, NSBundle, NSData, NSDate, NSDictionary, NSError, NSNumber, NSObject, NSString, NSURL};
use upleft_foundation::date::Date;

use super::downright_update_driver::{
    DownrightUpdateDriver, SpuUpdatePermissionRequest, SpuUserUpdateChoice, SpuUserUpdateState, SuAppcastItem,
    SuUpdatePermissionResponse,
};
use super::main_actor::{MainOnly, is_main_thread};
use super::update_engine::{BackgroundDownloadNotifier, SpuUpdater, set_spu_updater_factory};
use super::update_metadata::Url;

/// The version Downright pins (`exact: "2.9.6"`) and these declarations
/// follow. `scripts/sparkle-framework.sh` refuses any other.
pub const SPARKLE_VERSION: &str = "2.9.6";

// MARK: - Framework presence

/// The Sparkle classes this module sends messages to or receives.
const CLASSES: [&CStr; 6] = [
    c"SPUUpdater",
    c"SUAppcastItem",
    c"SPUUserUpdateState",
    c"SPUDownloadData",
    c"SPUUpdatePermissionRequest",
    c"SUUpdatePermissionResponse",
];

/// The Sparkle protocols the bridge classes conform to.
const PROTOCOLS: [&CStr; 2] = [c"SPUUserDriver", c"SPUUpdaterDelegate"];

/// Whether Sparkle is loaded in this process: every class and protocol this
/// module uses is registered with the Objective-C runtime.
pub fn is_available() -> bool {
    CLASSES.iter().all(|name| AnyClass::get(name).is_some())
        && PROTOCOLS.iter().all(|name| AnyProtocol::get(name).is_some())
}

/// Installs [`make_updater`] as the engine's `SPUUpdater` constructor. The
/// app binary calls this once at start-up, before
/// `UpdateCoordinator::shared(mtm).start()` (which Downright calls from
/// `applicationDidFinishLaunching`). Without the framework the factory
/// answers `None`, so installing it is always safe.
pub fn install(_mtm: MainThreadMarker) {
    set_spu_updater_factory(Some(Box::new(|host, application, driver, notifier| {
        make_updater(host, application, driver, notifier).map(|updater| updater as Rc<dyn SpuUpdater>)
    })));
}

/// `SPUUpdater(hostBundle:applicationBundle:userDriver:delegate:)`, with the
/// ported driver and notifier wrapped in their Objective-C classes. `None`
/// when Sparkle is not loaded, or off the main thread (`SPUUpdater` is a
/// main-thread class).
///
/// The bridge classes are registered with the runtime on first use, which
/// comes after the availability check, so they always pick up Sparkle's
/// protocols.
pub fn make_updater(
    host_bundle: &NSBundle,
    application_bundle: &NSBundle,
    driver: &Rc<DownrightUpdateDriver>,
    notifier: &Rc<BackgroundDownloadNotifier>,
) -> Option<Rc<SparkleUpdater>> {
    let mtm = MainThreadMarker::new()?;
    if !is_available() {
        return None;
    }
    let user_driver = DownrightUpdateDriverObject::new(mtm, driver.clone());
    let delegate = BackgroundDownloadNotifierObject::new(notifier.clone());
    let updater = SPUUpdater::initWithHostBundle_applicationBundle_userDriver_delegate(
        SPUUpdater::alloc(mtm),
        host_bundle,
        application_bundle,
        ProtocolObject::from_ref(&*user_driver),
        Some(ProtocolObject::from_ref(&*delegate)),
    );
    Some(Rc::new(SparkleUpdater { updater, user_driver, delegate }))
}

// MARK: - Sparkle's declarations (Sparkle 2.9.6 headers)

extern_protocol!(
    /// `@protocol SPUUserDriver <NSObject>` (`SPUUserDriver.h`). The methods
    /// are declared on [`DownrightUpdateDriverObject`], which implements
    /// them; Rust never sends them.
    ///
    /// # Safety
    ///
    /// The name is Sparkle's, and the protocol inherits from `NSObject`.
    pub unsafe trait SPUUserDriver: NSObjectProtocol {}
);

extern_protocol!(
    /// `@protocol SPUUpdaterDelegate <NSObject>` (`SPUUpdaterDelegate.h`).
    /// Every method is optional; Downright implements only
    /// `updater:didDownloadUpdate:`.
    ///
    /// # Safety
    ///
    /// The name is Sparkle's, and the protocol inherits from `NSObject`.
    pub unsafe trait SPUUpdaterDelegate: NSObjectProtocol {}
);

extern_class!(
    /// `SPUUpdater` (`SPUUpdater.h`): "This class must be used on the main
    /// thread."
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SPUUpdater"]
    pub struct SPUUpdater;
);

#[allow(non_snake_case)]
impl SPUUpdater {
    extern_methods!(
        /// `-initWithHostBundle:applicationBundle:userDriver:delegate:`. The
        /// updater keeps the user driver strongly and the delegate weakly.
        #[unsafe(method(initWithHostBundle:applicationBundle:userDriver:delegate:))]
        pub fn initWithHostBundle_applicationBundle_userDriver_delegate(
            this: Allocated<Self>,
            host_bundle: &NSBundle,
            application_bundle: &NSBundle,
            user_driver: &ProtocolObject<dyn SPUUserDriver>,
            delegate: Option<&ProtocolObject<dyn SPUUpdaterDelegate>>,
        ) -> Retained<Self>;

        /// `-startUpdater:`.
        #[unsafe(method(startUpdater:_))]
        pub fn startUpdater(&self) -> Result<(), Retained<NSError>>;

        #[unsafe(method(checkForUpdates))]
        pub fn checkForUpdates(&self);

        #[unsafe(method(checkForUpdatesInBackground))]
        pub fn checkForUpdatesInBackground(&self);

        #[unsafe(method(canCheckForUpdates))]
        pub fn canCheckForUpdates(&self) -> bool;

        #[unsafe(method(automaticallyChecksForUpdates))]
        pub fn automaticallyChecksForUpdates(&self) -> bool;

        #[unsafe(method(setAutomaticallyChecksForUpdates:))]
        pub fn setAutomaticallyChecksForUpdates(&self, value: bool);

        #[unsafe(method(automaticallyDownloadsUpdates))]
        pub fn automaticallyDownloadsUpdates(&self) -> bool;

        #[unsafe(method(setAutomaticallyDownloadsUpdates:))]
        pub fn setAutomaticallyDownloadsUpdates(&self, value: bool);

        #[unsafe(method(allowsAutomaticUpdates))]
        pub fn allowsAutomaticUpdates(&self) -> bool;

        /// `NSTimeInterval updateCheckInterval`.
        #[unsafe(method(updateCheckInterval))]
        pub fn updateCheckInterval(&self) -> f64;

        #[unsafe(method(setUpdateCheckInterval:))]
        pub fn setUpdateCheckInterval(&self, value: f64);

        #[unsafe(method(lastUpdateCheckDate))]
        pub fn lastUpdateCheckDate(&self) -> Option<Retained<NSDate>>;

        /// `-clearFeedURLFromUserDefaults`: the feed URL it removed, if any.
        #[unsafe(method(clearFeedURLFromUserDefaults))]
        pub fn clearFeedURLFromUserDefaults(&self) -> Option<Retained<NSURL>>;
    );
}

extern_class!(
    /// `SUAppcastItem` (`SUAppcastItem.h`), immutable and `NS_SWIFT_SENDABLE`.
    #[unsafe(super(NSObject))]
    #[name = "SUAppcastItem"]
    pub struct SUAppcastItem;
);

#[allow(non_snake_case)]
impl SUAppcastItem {
    extern_methods!(
        #[unsafe(method(versionString))]
        pub fn versionString(&self) -> Retained<NSString>;

        #[unsafe(method(displayVersionString))]
        pub fn displayVersionString(&self) -> Retained<NSString>;

        #[unsafe(method(title))]
        pub fn title(&self) -> Option<Retained<NSString>>;

        #[unsafe(method(itemDescription))]
        pub fn itemDescription(&self) -> Option<Retained<NSString>>;

        #[unsafe(method(releaseNotesURL))]
        pub fn releaseNotesURL(&self) -> Option<Retained<NSURL>>;

        #[unsafe(method(infoURL))]
        pub fn infoURL(&self) -> Option<Retained<NSURL>>;

        /// `uint64_t contentLength`.
        #[unsafe(method(contentLength))]
        pub fn contentLength(&self) -> u64;

        #[unsafe(method(isInformationOnlyUpdate))]
        pub fn isInformationOnlyUpdate(&self) -> bool;

        #[unsafe(method(isMajorUpgrade))]
        pub fn isMajorUpgrade(&self) -> bool;

        #[unsafe(method(isCriticalUpdate))]
        pub fn isCriticalUpdate(&self) -> bool;

        #[unsafe(method(minimumSystemVersion))]
        pub fn minimumSystemVersion(&self) -> Option<Retained<NSString>>;
    );
}

/// What `UpdateMetadata(appcastItem:)` and `BackgroundDownloadNotifier` read.
/// A Swift `String` bridged from these `NSString`s holds the same text.
impl SuAppcastItem for SUAppcastItem {
    fn version_string(&self) -> String {
        self.versionString().to_string()
    }

    fn display_version_string(&self) -> String {
        self.displayVersionString().to_string()
    }

    fn title(&self) -> Option<String> {
        SUAppcastItem::title(self).map(|title| title.to_string())
    }

    fn item_description(&self) -> Option<String> {
        self.itemDescription().map(|description| description.to_string())
    }

    fn release_notes_url(&self) -> Option<Url> {
        self.releaseNotesURL().map(Url::from_nsurl)
    }

    fn info_url(&self) -> Option<Url> {
        self.infoURL().map(Url::from_nsurl)
    }

    fn content_length(&self) -> u64 {
        self.contentLength()
    }

    fn is_information_only_update(&self) -> bool {
        self.isInformationOnlyUpdate()
    }

    fn is_major_upgrade(&self) -> bool {
        self.isMajorUpgrade()
    }

    fn is_critical_update(&self) -> bool {
        self.isCriticalUpdate()
    }

    fn minimum_system_version(&self) -> Option<String> {
        self.minimumSystemVersion().map(|version| version.to_string())
    }
}

extern_class!(
    /// `SPUUserUpdateState` (`SPUUserUpdateState.h`).
    #[unsafe(super(NSObject))]
    #[name = "SPUUserUpdateState"]
    pub struct SPUUserUpdateState;
);

#[allow(non_snake_case)]
impl SPUUserUpdateState {
    extern_methods!(
        /// `SPUUserUpdateStage stage` (`NS_ENUM(NSInteger)`), raw, so that a
        /// stage from a newer Sparkle reaches the driver's `@unknown default`.
        #[unsafe(method(stage))]
        pub fn stage(&self) -> isize;

        #[unsafe(method(userInitiated))]
        pub fn userInitiated(&self) -> bool;
    );
}

extern_class!(
    /// `SPUDownloadData` (`SPUDownloadData.h`).
    #[unsafe(super(NSObject))]
    #[name = "SPUDownloadData"]
    pub struct SPUDownloadData;
);

#[allow(non_snake_case)]
impl SPUDownloadData {
    extern_methods!(
        #[unsafe(method(data))]
        pub fn data(&self) -> Retained<NSData>;
    );
}

extern_class!(
    /// `SPUUpdatePermissionRequest` (`SPUUpdatePermissionRequest.h`).
    #[unsafe(super(NSObject))]
    #[name = "SPUUpdatePermissionRequest"]
    pub struct SPUUpdatePermissionRequest;
);

#[allow(non_snake_case)]
impl SPUUpdatePermissionRequest {
    extern_methods!(
        #[unsafe(method(initWithSystemProfile:))]
        pub fn initWithSystemProfile(
            this: Allocated<Self>,
            system_profile: &NSArray<NSDictionary<NSString, NSString>>,
        ) -> Retained<Self>;

        #[unsafe(method(systemProfile))]
        pub fn systemProfile(&self) -> Retained<NSArray<NSDictionary<NSString, NSString>>>;
    );
}

extern_class!(
    /// `SUUpdatePermissionResponse` (`SUUpdatePermissionResponse.h`).
    #[unsafe(super(NSObject))]
    #[name = "SUUpdatePermissionResponse"]
    pub struct SUUpdatePermissionResponse;
);

#[allow(non_snake_case)]
impl SUUpdatePermissionResponse {
    extern_methods!(
        #[unsafe(method(initWithAutomaticUpdateChecks:automaticUpdateDownloading:sendSystemProfile:))]
        pub fn initWithAutomaticUpdateChecks_automaticUpdateDownloading_sendSystemProfile(
            this: Allocated<Self>,
            automatic_update_checks: bool,
            automatic_update_downloading: Option<&NSNumber>,
            send_system_profile: bool,
        ) -> Retained<Self>;

        #[unsafe(method(automaticUpdateChecks))]
        pub fn automaticUpdateChecks(&self) -> bool;

        #[unsafe(method(automaticUpdateDownloading))]
        pub fn automaticUpdateDownloading(&self) -> Option<Retained<NSNumber>>;

        #[unsafe(method(sendSystemProfile))]
        pub fn sendSystemProfile(&self) -> bool;
    );
}

// MARK: - SPUUpdater

/// The engine's `SPUUpdater`. Swift's `SparkleUpdateEngine` holds its
/// updater, its driver and its notifier; here the Objective-C halves of the
/// driver and notifier live with the updater, since `SPUUpdater` keeps its
/// delegate only weakly ("you are responsible for keeping it alive").
pub struct SparkleUpdater {
    updater: Retained<SPUUpdater>,
    user_driver: Retained<DownrightUpdateDriverObject>,
    delegate: Retained<BackgroundDownloadNotifierObject>,
}

impl SparkleUpdater {
    pub fn updater(&self) -> &SPUUpdater {
        &self.updater
    }

    pub fn user_driver(&self) -> &DownrightUpdateDriverObject {
        &self.user_driver
    }

    pub fn delegate(&self) -> &BackgroundDownloadNotifierObject {
        &self.delegate
    }
}

impl SpuUpdater for SparkleUpdater {
    fn clear_feed_url_from_user_defaults(&self) {
        let _ = self.updater.clearFeedURLFromUserDefaults();
    }

    fn start(&self) -> Result<(), Retained<NSError>> {
        self.updater.startUpdater()
    }

    fn check_for_updates(&self) {
        self.updater.checkForUpdates();
    }

    fn check_for_updates_in_background(&self) {
        self.updater.checkForUpdatesInBackground();
    }

    fn can_check_for_updates(&self) -> bool {
        self.updater.canCheckForUpdates()
    }

    fn automatically_checks_for_updates(&self) -> bool {
        self.updater.automaticallyChecksForUpdates()
    }

    fn set_automatically_checks_for_updates(&self, value: bool) {
        self.updater.setAutomaticallyChecksForUpdates(value);
    }

    fn automatically_downloads_updates(&self) -> bool {
        self.updater.automaticallyDownloadsUpdates()
    }

    fn set_automatically_downloads_updates(&self, value: bool) {
        self.updater.setAutomaticallyDownloadsUpdates(value);
    }

    fn allows_automatic_updates(&self) -> bool {
        self.updater.allowsAutomaticUpdates()
    }

    fn update_check_interval(&self) -> f64 {
        self.updater.updateCheckInterval()
    }

    fn set_update_check_interval(&self, value: f64) {
        self.updater.setUpdateCheckInterval(value);
    }

    fn last_update_check_date(&self) -> Option<Date> {
        self.updater.lastUpdateCheckDate().map(|date| Date::from_reference(date.timeIntervalSinceReferenceDate()))
    }
}

// MARK: - Main-thread delivery

/// Objective-C arguments carried to the main queue. Everything inside is
/// either an Objective-C object or block (retained and released atomically,
/// so moving them between threads is sound) or a closure that only opens
/// them on the main thread.
struct CarriedToMain<T>(T);

// SAFETY: see the type's comment; the contents are only used by the main
// queue block that receives them.
unsafe impl<T> Send for CarriedToMain<T> {}

impl<T> CarriedToMain<T> {
    fn into_inner(self) -> T {
        self.0
    }
}

/// Runs `body` with `this` on the main thread: at once when the caller is on
/// it (always, for Sparkle's user driver), otherwise on a later turn of the
/// main queue. `this` stays retained for the call, so the body may release
/// the last other reference to it.
fn on_main<T: Message + 'static>(this: &T, body: impl FnOnce(&T) + 'static) {
    let this = this.retain();
    if is_main_thread() {
        body(&this);
        return;
    }
    let carried = CarriedToMain((this, body));
    DispatchQueue::main().exec_async(move || {
        let (this, body) = carried.into_inner();
        body(&this);
    });
}

// MARK: - SPUUserDriver

/// The ivars of [`DownrightUpdateDriverObject`].
pub struct DownrightUpdateDriverIvars {
    /// Opened only on the main thread; leaked rather than dropped elsewhere.
    driver: MainOnly<Rc<DownrightUpdateDriver>>,
}

define_class!(
    /// Downright's `SPUUserDriver`: `final class DownrightUpdateDriver:
    /// NSObject, SPUUserDriver`. Each protocol method converts Sparkle's
    /// arguments and forwards to the method of the same name on the ported
    /// [`DownrightUpdateDriver`], which hands the one-shot capability to the
    /// coordinator.
    // SAFETY: `init` is forwarded in `new` after the ivars are set; each
    // method's signature is the one in Sparkle 2.9.6's `SPUUserDriver.h`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "DownrightUpdateDriver"]
    #[ivars = DownrightUpdateDriverIvars]
    pub struct DownrightUpdateDriverObject;

    unsafe impl NSObjectProtocol for DownrightUpdateDriverObject {}

    unsafe impl SPUUserDriver for DownrightUpdateDriverObject {
        /// `show(_:reply:)`.
        #[unsafe(method(showUpdatePermissionRequest:reply:))]
        fn show_update_permission_request(
            &self,
            request: &SPUUpdatePermissionRequest,
            reply: &DynBlock<dyn Fn(NonNull<SUUpdatePermissionResponse>)>,
        ) {
            let request = permission_request(request);
            let reply = reply.copy();
            on_main(self, move |this| {
                this.driver().show_update_permission_request(
                    &request,
                    Box::new(move |response: SuUpdatePermissionResponse| {
                        let response = permission_response(&response);
                        reply.call((NonNull::from(&*response),));
                    }),
                );
            });
        }

        #[unsafe(method(showUserInitiatedUpdateCheckWithCancellation:))]
        fn show_user_initiated_update_check(&self, cancellation: &DynBlock<dyn Fn()>) {
            let cancellation = cancellation.copy();
            on_main(self, move |this| {
                this.driver().show_user_initiated_update_check(Box::new(move || cancellation.call(())));
            });
        }

        #[unsafe(method(showUpdateFoundWithAppcastItem:state:reply:))]
        fn show_update_found(
            &self,
            appcast_item: &SUAppcastItem,
            state: &SPUUserUpdateState,
            reply: &DynBlock<dyn Fn(isize)>,
        ) {
            let appcast_item = appcast_item.retain();
            let state = SpuUserUpdateState { stage: state.stage(), user_initiated: state.userInitiated() };
            let reply = reply.copy();
            on_main(self, move |this| {
                this.driver().show_update_found(
                    &*appcast_item,
                    &state,
                    Box::new(move |choice: SpuUserUpdateChoice| reply.call((choice as isize,))),
                );
            });
        }

        /// `showUpdateReleaseNotes(with:)`: `downloadData.data`.
        #[unsafe(method(showUpdateReleaseNotesWithDownloadData:))]
        fn show_update_release_notes(&self, download_data: &SPUDownloadData) {
            let data = download_data.data();
            on_main(self, move |this| this.driver().show_update_release_notes(&data.to_vec()));
        }

        #[unsafe(method(showUpdateReleaseNotesFailedToDownloadWithError:))]
        fn show_update_release_notes_failed_to_download_with_error(&self, error: &NSError) {
            let error = error.retain();
            on_main(self, move |this| this.driver().show_update_release_notes_failed_to_download_with_error(&error));
        }

        #[unsafe(method(showUpdateNotFoundWithError:acknowledgement:))]
        fn show_update_not_found_with_error(&self, error: &NSError, acknowledgement: &DynBlock<dyn Fn()>) {
            let error = error.retain();
            let acknowledgement = acknowledgement.copy();
            on_main(self, move |this| {
                this.driver().show_update_not_found_with_error(&error, Box::new(move || acknowledgement.call(())));
            });
        }

        #[unsafe(method(showUpdaterError:acknowledgement:))]
        fn show_updater_error(&self, error: &NSError, acknowledgement: &DynBlock<dyn Fn()>) {
            let error = error.retain();
            let acknowledgement = acknowledgement.copy();
            on_main(self, move |this| {
                this.driver().show_updater_error(&error, Box::new(move || acknowledgement.call(())));
            });
        }

        #[unsafe(method(showDownloadInitiatedWithCancellation:))]
        fn show_download_initiated(&self, cancellation: &DynBlock<dyn Fn()>) {
            let cancellation = cancellation.copy();
            on_main(self, move |this| {
                this.driver().show_download_initiated(Box::new(move || cancellation.call(())));
            });
        }

        #[unsafe(method(showDownloadDidReceiveExpectedContentLength:))]
        fn show_download_did_receive_expected_content_length(&self, expected_content_length: u64) {
            on_main(self, move |this| {
                this.driver().show_download_did_receive_expected_content_length(expected_content_length);
            });
        }

        #[unsafe(method(showDownloadDidReceiveDataOfLength:))]
        fn show_download_did_receive_data(&self, length: u64) {
            on_main(self, move |this| this.driver().show_download_did_receive_data(length));
        }

        #[unsafe(method(showDownloadDidStartExtractingUpdate))]
        fn show_download_did_start_extracting_update(&self) {
            on_main(self, |this| this.driver().show_download_did_start_extracting_update());
        }

        #[unsafe(method(showExtractionReceivedProgress:))]
        fn show_extraction_received_progress(&self, progress: f64) {
            on_main(self, move |this| this.driver().show_extraction_received_progress(progress));
        }

        /// `showReady(toInstallAndRelaunch:)`.
        #[unsafe(method(showReadyToInstallAndRelaunch:))]
        fn show_ready_to_install_and_relaunch(&self, reply: &DynBlock<dyn Fn(isize)>) {
            let reply = reply.copy();
            on_main(self, move |this| {
                this.driver().show_ready_to_install_and_relaunch(Box::new(move |choice: SpuUserUpdateChoice| {
                    reply.call((choice as isize,))
                }));
            });
        }

        #[unsafe(method(showInstallingUpdateWithApplicationTerminated:retryTerminatingApplication:))]
        fn show_installing_update(
            &self,
            application_terminated: bool,
            retry_terminating_application: &DynBlock<dyn Fn()>,
        ) {
            let retry = retry_terminating_application.copy();
            on_main(self, move |this| {
                this.driver().show_installing_update(application_terminated, Box::new(move || retry.call(())));
            });
        }

        #[unsafe(method(showUpdateInstalledAndRelaunched:acknowledgement:))]
        fn show_update_installed_and_relaunched(&self, relaunched: bool, acknowledgement: &DynBlock<dyn Fn()>) {
            let acknowledgement = acknowledgement.copy();
            on_main(self, move |this| {
                this.driver()
                    .show_update_installed_and_relaunched(relaunched, Box::new(move || acknowledgement.call(())));
            });
        }

        #[unsafe(method(dismissUpdateInstallation))]
        fn dismiss_update_installation(&self) {
            on_main(self, |this| this.driver().dismiss_update_installation());
        }

        /// Optional in the protocol; Downright implements it.
        #[unsafe(method(showUpdateInFocus))]
        fn show_update_in_focus(&self) {
            on_main(self, |this| this.driver().show_update_in_focus());
        }
    }
);

impl DownrightUpdateDriverObject {
    /// `DownrightUpdateDriver(host:)`'s Objective-C half, over an existing
    /// ported driver.
    pub fn new(mtm: MainThreadMarker, driver: Rc<DownrightUpdateDriver>) -> Retained<DownrightUpdateDriverObject> {
        let this = Self::alloc(mtm).set_ivars(DownrightUpdateDriverIvars { driver: MainOnly::new(driver) });
        // SAFETY: `-[NSObject init]` on a freshly allocated instance.
        unsafe { msg_send![super(this), init] }
    }

    /// The ported driver (main thread only).
    pub fn driver(&self) -> Rc<DownrightUpdateDriver> {
        self.ivars().driver.get().clone()
    }
}

/// `SPUUpdatePermissionRequest` by value. Downright ignores it; each entry of
/// `systemProfile` becomes its `key` and `value`.
fn permission_request(request: &SPUUpdatePermissionRequest) -> SpuUpdatePermissionRequest {
    let key = NSString::from_str("key");
    let value = NSString::from_str("value");
    let text = |entry: &NSDictionary<NSString, NSString>, name: &NSString| {
        entry.objectForKey(name).map(|text| text.to_string()).unwrap_or_default()
    };
    SpuUpdatePermissionRequest {
        system_profile: request
            .systemProfile()
            .iter()
            .map(|entry| (text(&entry, &key), text(&entry, &value)))
            .collect(),
    }
}

/// `SUUpdatePermissionResponse(automaticUpdateChecks:automaticUpdateDownloading:sendSystemProfile:)`.
fn permission_response(response: &SuUpdatePermissionResponse) -> Retained<SUUpdatePermissionResponse> {
    let downloading = response.automatic_update_downloading.map(NSNumber::new_bool);
    SUUpdatePermissionResponse::initWithAutomaticUpdateChecks_automaticUpdateDownloading_sendSystemProfile(
        SUUpdatePermissionResponse::alloc(),
        response.automatic_update_checks,
        downloading.as_deref(),
        response.send_system_profile,
    )
}

// MARK: - SPUUpdaterDelegate

/// The ivars of [`BackgroundDownloadNotifierObject`].
pub struct BackgroundDownloadNotifierIvars {
    /// Opened only on the main thread; leaked rather than dropped elsewhere
    /// (Sparkle loads its weak delegate reference on whatever thread it
    /// calls from).
    notifier: MainOnly<Rc<BackgroundDownloadNotifier>>,
}

define_class!(
    /// `final class BackgroundDownloadNotifier: NSObject,
    /// SPUUpdaterDelegate`: the one delegate callback Downright needs, which
    /// learns that a download finished, including the silent background
    /// ones that never reach the user driver.
    ///
    /// Swift's notifier calls its handler on Sparkle's thread, and the
    /// coordinator's handler hops to the main actor when that is not the main
    /// thread. The port's handler is a main-thread closure, so this class
    /// makes the hop instead: off the main thread it retains the item and
    /// forwards on the main queue.
    // SAFETY: `init` is forwarded in `new` after the ivars are set; the
    // method's signature is the one in Sparkle 2.9.6's `SPUUpdaterDelegate.h`.
    #[unsafe(super(NSObject))]
    #[name = "BackgroundDownloadNotifier"]
    #[ivars = BackgroundDownloadNotifierIvars]
    pub struct BackgroundDownloadNotifierObject;

    unsafe impl NSObjectProtocol for BackgroundDownloadNotifierObject {}

    unsafe impl SPUUpdaterDelegate for BackgroundDownloadNotifierObject {
        /// `updater(_:didDownloadUpdate:)`.
        #[unsafe(method(updater:didDownloadUpdate:))]
        fn updater_did_download_update(&self, _updater: &AnyObject, item: &SUAppcastItem) {
            let item = item.retain();
            on_main(self, move |this| this.notifier().updater_did_download_update(&*item));
        }
    }
);

impl BackgroundDownloadNotifierObject {
    /// The Objective-C half of an existing ported notifier. Main thread only.
    pub fn new(notifier: Rc<BackgroundDownloadNotifier>) -> Retained<BackgroundDownloadNotifierObject> {
        let this = Self::alloc().set_ivars(BackgroundDownloadNotifierIvars { notifier: MainOnly::new(notifier) });
        // SAFETY: `-[NSObject init]` on a freshly allocated instance.
        unsafe { msg_send![super(this), init] }
    }

    /// The ported notifier (main thread only).
    pub fn notifier(&self) -> Rc<BackgroundDownloadNotifier> {
        self.ivars().notifier.get().clone()
    }
}
