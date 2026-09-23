//! Port of `Sources/DownrightApp/Updater/DownrightUpdateDriver.swift`.
//!
//! The driver is Downright's `SPUUserDriver`. Sparkle is not linked yet, so
//! the Sparkle types it touches are modelled here by value (`SPUUserUpdateChoice`,
//! `SPUUserUpdateStage`, `SPUUserUpdateState`, `SUUpdatePermissionResponse`)
//! or by the accessors the driver reads (`SUAppcastItem`). SEAM(Sparkle): the
//! packaging layer declares the Objective-C `SPUUserDriver` conformer with
//! `define_class!` and forwards each protocol method to the method of the same
//! name here, converting Sparkle's arguments with these types.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use objc2_foundation::{NSError, NSNumber, NSString};

use super::update_metadata::{UpdateMetadata, Url};
use super::update_state_machine::UpdateStage;

/// What the user chose, in Downright's vocabulary. `DownrightUpdateDriver` is
/// the only place that maps to/from Sparkle's `SPUUserUpdateChoice`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateUserChoice {
    Install,
    Later,
    Skip,
}

/// Sparkle's `SPUUserUpdateChoice` (`NS_ENUM(NSInteger)`, Sparkle 2.9.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(isize)]
pub enum SpuUserUpdateChoice {
    Skip = 0,
    Install = 1,
    Dismiss = 2,
}

/// Sparkle's `SPUUserUpdateStage` raw values (`NS_ENUM(NSInteger)`).
pub mod spu_user_update_stage {
    pub const NOT_DOWNLOADED: isize = 0;
    pub const DOWNLOADED: isize = 1;
    pub const INSTALLING: isize = 2;
}

/// Sparkle's `SPUUserUpdateState`: the two properties the driver reads. The
/// stage is the raw `NSInteger`, so a stage from a newer Sparkle reaches the
/// driver's `@unknown default`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpuUserUpdateState {
    pub stage: isize,
    pub user_initiated: bool,
}

/// Sparkle's `SUAppcastItem`, reduced to the properties
/// `UpdateMetadata(appcastItem:)` and `BackgroundDownloadNotifier` read.
/// SEAM(Sparkle): implemented over the real `SUAppcastItem` when Sparkle is
/// linked. Downright never parses an appcast itself; Sparkle does.
pub trait SuAppcastItem {
    /// `versionString` (nonnull).
    fn version_string(&self) -> String;
    /// `displayVersionString` (nonnull).
    fn display_version_string(&self) -> String;
    /// `title`.
    fn title(&self) -> Option<String>;
    /// `itemDescription`.
    fn item_description(&self) -> Option<String>;
    /// `releaseNotesURL`.
    fn release_notes_url(&self) -> Option<Url>;
    /// `infoURL`.
    fn info_url(&self) -> Option<Url>;
    /// `contentLength`.
    fn content_length(&self) -> u64;
    /// `isInformationOnlyUpdate`.
    fn is_information_only_update(&self) -> bool;
    /// `isMajorUpgrade`.
    fn is_major_upgrade(&self) -> bool;
    /// `isCriticalUpdate`.
    fn is_critical_update(&self) -> bool;
    /// `minimumSystemVersion`.
    fn minimum_system_version(&self) -> Option<String>;
}

/// Sparkle's `SPUUpdatePermissionRequest`. Downright ignores its contents.
#[derive(Clone, Debug, Default)]
pub struct SpuUpdatePermissionRequest {
    /// `systemProfile`: key/value pairs Sparkle would send.
    pub system_profile: Vec<(String, String)>,
}

/// Sparkle's `SUUpdatePermissionResponse`
/// (`initWithAutomaticUpdateChecks:automaticUpdateDownloading:sendSystemProfile:`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SuUpdatePermissionResponse {
    pub automatic_update_checks: bool,
    /// `NSNumber *` (nil means "leave the setting alone").
    pub automatic_update_downloading: Option<bool>,
    pub send_system_profile: bool,
}

/// Sparkle's `SPUNoUpdateFoundUserInitiatedKey` (`SUConstants.m`).
pub const SPU_NO_UPDATE_FOUND_USER_INITIATED_KEY: &str = "SPUNoUpdateUserInitiated";

/// A one-shot closure Sparkle hands the driver (reply, cancellation,
/// acknowledgement, retry).
pub type Reply<T> = Box<dyn FnOnce(T)>;
pub type Callback = Box<dyn FnOnce()>;

/// The coordinator (the only other object in the update stack) implements
/// this. Every method corresponds to exactly one `SPUUserDriver` callback and
/// hands the coordinator the one-shot capability the callback carried.
pub trait UpdateDriverHost {
    /// `showUserInitiatedUpdateCheck(cancellation:)` — a check the user asked for.
    fn driver_did_begin_user_check(&self, cancellation: Callback);
    /// `showUpdateFound(_:state:reply:)` — an update is being offered.
    /// `user_initiated` comes straight from `SPUUserUpdateState.userInitiated`
    /// so the coordinator can keep automatic presentations off the panel.
    fn driver_did_find_update(
        &self,
        metadata: UpdateMetadata,
        stage: UpdateStage,
        user_initiated: bool,
        reply: Reply<UpdateUserChoice>,
    );
    /// `showUpdateReleaseNotes(with:)` — inline/linked notes arrived.
    fn driver_did_receive_release_notes(&self, data: Vec<u8>);
    /// `showUpdateReleaseNotesFailedToDownloadWithError(_:)`.
    fn driver_did_fail_to_download_release_notes(&self, error: &NSError);
    /// `showUpdateNotFoundWithError(_:acknowledgement:)`.
    fn driver_did_find_no_update(&self, user_initiated: bool, acknowledgement: Callback);
    /// `showUpdaterError(_:acknowledgement:)`.
    fn driver_did_encounter_error(&self, error: &NSError, acknowledgement: Callback);
    /// `showDownloadInitiated(cancellation:)`.
    fn driver_did_begin_download(&self, cancellation: Callback);
    /// `showDownloadDidReceiveExpectedContentLength(_:)`.
    fn driver_did_receive_expected_length(&self, length: u64);
    /// `showDownloadDidReceiveData(ofLength:)`.
    fn driver_did_receive_data(&self, length: u64);
    /// `showDownloadDidStartExtractingUpdate()`.
    fn driver_did_begin_extraction(&self);
    /// `showExtractionReceivedProgress(_:)`.
    fn driver_did_receive_extraction_progress(&self, progress: f64);
    /// `showReady(toInstallAndRelaunch:)`.
    fn driver_did_become_ready_to_relaunch(&self, reply: Reply<UpdateUserChoice>);
    /// `showInstallingUpdate(withApplicationTerminated:retryTerminatingApplication:)`.
    fn driver_did_begin_installation(&self, application_terminated: bool, retry_termination: Callback);
    /// `showUpdateInstalledAndRelaunched(_:acknowledgement:)`.
    fn driver_did_finish_installation(&self, relaunched: bool, acknowledgement: Callback);
    /// `dismissUpdateInstallation()`.
    fn driver_did_dismiss(&self);
    /// `showUpdateInFocus()`.
    fn driver_did_request_focus(&self);
}

/// Downright's complete `SPUUserDriver`. Sparkle owns download, validation,
/// extraction, authorization, replacement, and relaunch; every visible
/// interaction is routed through here to the coordinator, and no standard
/// Sparkle window ever appears.
///
/// Every callback carries a one-shot reply / cancellation / acknowledgement.
/// Each one is handed to the coordinator *exactly once*; if the coordinator is
/// torn down the driver simply stops forwarding, and Sparkle's
/// `dismissUpdateInstallation()` discards whatever is still pending.
#[derive(Default)]
pub struct DownrightUpdateDriver {
    /// `weak var host: UpdateDriverHost?`
    host: RefCell<Option<Weak<dyn UpdateDriverHost>>>,
}

impl DownrightUpdateDriver {
    /// `init(host:)`.
    pub fn new(host: Option<Weak<dyn UpdateDriverHost>>) -> Rc<DownrightUpdateDriver> {
        Rc::new(DownrightUpdateDriver { host: RefCell::new(host) })
    }

    pub fn host(&self) -> Option<Rc<dyn UpdateDriverHost>> {
        self.host.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_host(&self, host: Option<Weak<dyn UpdateDriverHost>>) {
        *self.host.borrow_mut() = host;
    }

    // MARK: SPUUserDriver

    /// `show(_:reply:)`. Never called in production: `SUEnableAutomaticChecks`
    /// is set in the Info.plist, which opts out of Sparkle's permission prompt
    /// entirely. If a dev bundle without that key ever reaches here, adopt the
    /// spec defaults: check automatically, download automatically, no profiling.
    pub fn show_update_permission_request(
        &self,
        _request: &SpuUpdatePermissionRequest,
        reply: Reply<SuUpdatePermissionResponse>,
    ) {
        reply(SuUpdatePermissionResponse {
            automatic_update_checks: true,
            automatic_update_downloading: None,
            send_system_profile: false,
        });
    }

    pub fn show_user_initiated_update_check(&self, cancellation: Callback) {
        if let Some(host) = self.host() {
            host.driver_did_begin_user_check(cancellation);
        }
    }

    pub fn show_update_found(
        &self,
        appcast_item: &dyn SuAppcastItem,
        state: &SpuUserUpdateState,
        reply: Reply<SpuUserUpdateChoice>,
    ) {
        let stage = match state.stage {
            spu_user_update_stage::NOT_DOWNLOADED => UpdateStage::NotDownloaded,
            spu_user_update_stage::DOWNLOADED => UpdateStage::Downloaded,
            spu_user_update_stage::INSTALLING => UpdateStage::Installing,
            _ => UpdateStage::NotDownloaded, // @unknown default
        };
        if let Some(host) = self.host() {
            host.driver_did_find_update(
                UpdateMetadata::from_appcast_item(appcast_item),
                stage,
                state.user_initiated,
                Box::new(move |choice: UpdateUserChoice| reply(choice.sparkle_value())),
            );
        }
    }

    /// `showUpdateReleaseNotes(with:)`: `downloadData.data`.
    pub fn show_update_release_notes(&self, download_data: &[u8]) {
        if let Some(host) = self.host() {
            host.driver_did_receive_release_notes(download_data.to_vec());
        }
    }

    pub fn show_update_release_notes_failed_to_download_with_error(&self, error: &NSError) {
        if let Some(host) = self.host() {
            host.driver_did_fail_to_download_release_notes(error);
        }
    }

    pub fn show_update_not_found_with_error(&self, error: &NSError, acknowledgement: Callback) {
        let user_initiated = user_initiated_flag(error);
        if let Some(host) = self.host() {
            host.driver_did_find_no_update(user_initiated, acknowledgement);
        }
    }

    pub fn show_updater_error(&self, error: &NSError, acknowledgement: Callback) {
        if let Some(host) = self.host() {
            host.driver_did_encounter_error(error, acknowledgement);
        }
    }

    pub fn show_download_initiated(&self, cancellation: Callback) {
        if let Some(host) = self.host() {
            host.driver_did_begin_download(cancellation);
        }
    }

    pub fn show_download_did_receive_expected_content_length(&self, expected_content_length: u64) {
        if let Some(host) = self.host() {
            host.driver_did_receive_expected_length(expected_content_length);
        }
    }

    pub fn show_download_did_receive_data(&self, length: u64) {
        if let Some(host) = self.host() {
            host.driver_did_receive_data(length);
        }
    }

    pub fn show_download_did_start_extracting_update(&self) {
        if let Some(host) = self.host() {
            host.driver_did_begin_extraction();
        }
    }

    pub fn show_extraction_received_progress(&self, progress: f64) {
        if let Some(host) = self.host() {
            host.driver_did_receive_extraction_progress(progress);
        }
    }

    pub fn show_ready_to_install_and_relaunch(&self, reply: Reply<SpuUserUpdateChoice>) {
        if let Some(host) = self.host() {
            host.driver_did_become_ready_to_relaunch(Box::new(move |choice: UpdateUserChoice| {
                reply(choice.sparkle_value())
            }));
        }
    }

    pub fn show_installing_update(&self, application_terminated: bool, retry_terminating_application: Callback) {
        if let Some(host) = self.host() {
            host.driver_did_begin_installation(application_terminated, retry_terminating_application);
        }
    }

    pub fn show_update_installed_and_relaunched(&self, relaunched: bool, acknowledgement: Callback) {
        if let Some(host) = self.host() {
            host.driver_did_finish_installation(relaunched, acknowledgement);
        }
    }

    pub fn dismiss_update_installation(&self) {
        if let Some(host) = self.host() {
            host.driver_did_dismiss();
        }
    }

    pub fn show_update_in_focus(&self) {
        if let Some(host) = self.host() {
            host.driver_did_request_focus();
        }
    }
}

/// `(nsError.userInfo[SPUNoUpdateFoundUserInitiatedKey] as? NSNumber)?.boolValue ?? false`.
fn user_initiated_flag(error: &NSError) -> bool {
    let key = NSString::from_str(SPU_NO_UPDATE_FOUND_USER_INITIATED_KEY);
    let user_info = error.userInfo();
    let Some(value) = user_info.objectForKey(&key) else {
        return false;
    };
    match value.downcast::<NSNumber>() {
        Ok(number) => number.boolValue(),
        Err(_) => false,
    }
}

// MARK: - Mapping

impl UpdateMetadata {
    /// `init(appcastItem:)`: translate Sparkle's appcast item into the plain
    /// value type. Release notes flow two ways: embedded Markdown in
    /// `itemDescription` (our release pipeline) or a linked HTML page
    /// delivered separately via `showUpdateReleaseNotes(with:)`.
    pub fn from_appcast_item(appcast_item: &dyn SuAppcastItem) -> UpdateMetadata {
        UpdateMetadata {
            version_string: appcast_item.version_string(),
            display_version_string: appcast_item.display_version_string(),
            title: appcast_item.title(),
            item_description: appcast_item.item_description(),
            release_notes_url: appcast_item.release_notes_url(),
            info_url: appcast_item.info_url(),
            content_length: appcast_item.content_length(),
            is_information_only: appcast_item.is_information_only_update(),
            is_major_upgrade: appcast_item.is_major_upgrade(),
            is_critical: appcast_item.is_critical_update(),
            minimum_system_version: appcast_item.minimum_system_version(),
        }
    }
}

impl UpdateUserChoice {
    /// `fileprivate var sparkleValue: SPUUserUpdateChoice`.
    pub fn sparkle_value(self) -> SpuUserUpdateChoice {
        match self {
            UpdateUserChoice::Install => SpuUserUpdateChoice::Install,
            UpdateUserChoice::Later => SpuUserUpdateChoice::Dismiss,
            UpdateUserChoice::Skip => SpuUserUpdateChoice::Skip,
        }
    }
}
