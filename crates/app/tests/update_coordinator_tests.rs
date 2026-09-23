//! Port of `Tests/DownrightAppTests/UpdateCoordinatorTests.swift`.
//!
//! Each Swift `@Test` keeps its name (as `Suite/test`) and its assertions.
//! The suites are `@MainActor` and `.serialized` in Swift; here they run in
//! order on the main thread, which runs its run loop wherever Swift awaited
//! a main-queue hop (see `updater_support`).

mod updater_support;

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{
    NSData, NSDictionary, NSError, NSLocalizedDescriptionKey, NSNotification, NSNotificationCenter, NSOperationQueue,
    NSString,
};
use upleft_app::updater::downright_update_driver::{UpdateDriverHost, UpdateUserChoice};
use upleft_app::updater::release_watch::{ReleaseFeedProbe, ReleaseFeedProbeResult, ReleaseWatch, ReleaseWatchPolicy};
use upleft_app::updater::update_coordinator::{
    Capability, UpdateConfiguration, UpdateCoordinator, UpdatePillModel, UpdateReleaseNotesState,
};
use upleft_app::updater::update_engine::{FakeUpdateEngine, UpdateEngine};
use upleft_app::updater::update_metadata::{UpdateFailure, UpdateMetadata, Url};
use upleft_app::updater::update_state_machine::{UpdateEvent, UpdatePhase, UpdateStage, UpdateStateMachine};
use updater_support::{Skipped, Test, drain_main_queue, pump};

// MARK: - Fixtures

fn sample_metadata() -> UpdateMetadata {
    UpdateMetadata {
        version_string: "47".into(),
        display_version_string: "1.1.0".into(),
        title: Some("Upleft 1.1.0".into()),
        item_description: Some("## What's new\n- Faster parsing".into()),
        release_notes_url: None,
        info_url: Url::from_string("https://github.com/bitemyapp/upleft/releases/tag/v1.1.0"),
        content_length: 4_000_000,
        is_information_only: false,
        is_major_upgrade: false,
        is_critical: false,
        minimum_system_version: None,
    }
}

fn informational_metadata() -> UpdateMetadata {
    UpdateMetadata {
        version_string: "50".into(),
        display_version_string: "1.2.0".into(),
        title: None,
        item_description: None,
        release_notes_url: None,
        info_url: Url::from_string("https://github.com/bitemyapp/upleft/releases/tag/v1.2.0"),
        content_length: 0,
        is_information_only: true,
        is_major_upgrade: false,
        is_critical: false,
        minimum_system_version: None,
    }
}

/// `NSError(domain:code:userInfo:)`.
fn ns_error(domain: &str, code: isize, description: Option<&str>) -> Retained<NSError> {
    match description {
        None => NSError::new(code, &NSString::from_str(domain)),
        Some(description) => {
            // SAFETY: `NSLocalizedDescriptionKey` is Foundation's constant.
            let key: &NSString = unsafe { NSLocalizedDescriptionKey };
            let value = NSString::from_str(description);
            let user_info: Retained<NSDictionary<NSString, AnyObject>> =
                NSDictionary::from_slices(&[key], &[value.as_ref() as &AnyObject]);
            // SAFETY: a string-keyed user info dictionary.
            unsafe { NSError::errorWithDomain_code_userInfo(&NSString::from_str(domain), code, Some(&user_info)) }
        }
    }
}

/// A capability body that records its value.
fn recorder<T: 'static>(log: &Rc<RefCell<Vec<T>>>) -> Box<dyn FnOnce(T)> {
    let log = log.clone();
    Box::new(move |value| log.borrow_mut().push(value))
}

/// A `() -> Void` body that counts.
fn counter(count: &Rc<Cell<i32>>) -> Box<dyn FnOnce()> {
    let count = count.clone();
    Box::new(move || count.set(count.get() + 1))
}

fn nothing() -> Box<dyn FnOnce()> {
    Box::new(|| {})
}

fn ignore_choice() -> Box<dyn FnOnce(UpdateUserChoice)> {
    Box::new(|_| {})
}

// MARK: - State machine transitions

fn machine() -> UpdateStateMachine {
    UpdateStateMachine::new()
}

fn check_phases() {
    let mut m = machine();
    m.reduce(&UpdateEvent::UserInitiatedCheckBegan);
    assert_eq!(*m.phase(), UpdatePhase::Checking { user_initiated: true });
    m.reduce(&UpdateEvent::AutomaticCheckBegan);
    assert_eq!(*m.phase(), UpdatePhase::Checking { user_initiated: false });
}

fn update_found_distinguishes_informational() {
    let mut m = machine();
    m.reduce(&UpdateEvent::UpdateFound(sample_metadata(), UpdateStage::NotDownloaded));
    assert_eq!(*m.phase(), UpdatePhase::Available(sample_metadata(), UpdateStage::NotDownloaded));

    let mut i = machine();
    i.reduce(&UpdateEvent::UpdateFound(informational_metadata(), UpdateStage::NotDownloaded));
    assert_eq!(*i.phase(), UpdatePhase::Informational(informational_metadata()));
}

fn not_found_depends_on_user_initiation() {
    let mut manual = machine();
    manual.reduce(&UpdateEvent::UpdateNotFound { user_initiated: true });
    assert_eq!(*manual.phase(), UpdatePhase::UpToDate);

    let mut automatic = machine();
    automatic.reduce(&UpdateEvent::UpdateNotFound { user_initiated: false });
    assert_eq!(*automatic.phase(), UpdatePhase::Idle);
}

fn error_phase() {
    let mut m = machine();
    let failure = UpdateFailure::new("boom", Some("detail".into()), 2001, true);
    m.reduce(&UpdateEvent::UpdaterError(failure.clone()));
    assert_eq!(*m.phase(), UpdatePhase::Failed(failure, true));
}

fn download_progress_accounting() {
    let mut m = machine();
    m.reduce(&UpdateEvent::DownloadInitiated);
    assert_eq!(*m.phase(), UpdatePhase::Downloading { received: 0, expected: None });

    // Unknown content length then a late length callback: latest wins.
    m.reduce(&UpdateEvent::DataReceived(500));
    assert_eq!(*m.phase(), UpdatePhase::Downloading { received: 500, expected: None });
    m.reduce(&UpdateEvent::ExpectedLength(1_000));
    assert_eq!(*m.phase(), UpdatePhase::Downloading { received: 500, expected: Some(1_000) });

    // Repeated length callbacks for the same download are tolerated.
    m.reduce(&UpdateEvent::ExpectedLength(1_200));
    assert_eq!(*m.phase(), UpdatePhase::Downloading { received: 500, expected: Some(1_200) });

    // Overrun is clamped to the expected size: progress never exceeds 100%.
    m.reduce(&UpdateEvent::DataReceived(100_000));
    assert_eq!(*m.phase(), UpdatePhase::Downloading { received: 1_200, expected: Some(1_200) });
}

fn extraction_progress_is_clamped() {
    let mut m = machine();
    m.reduce(&UpdateEvent::ExtractionBegan);
    m.reduce(&UpdateEvent::ExtractionProgress(1.4));
    assert_eq!(*m.phase(), UpdatePhase::Extracting { progress: Some(1.0) });
    m.reduce(&UpdateEvent::ExtractionProgress(-0.2));
    assert_eq!(*m.phase(), UpdatePhase::Extracting { progress: Some(0.0) });
}

fn installation_stages() {
    let mut delayed = machine();
    delayed.reduce(&UpdateEvent::InstallingUpdate { application_terminated: false });
    assert_eq!(*delayed.phase(), UpdatePhase::WaitingForTermination);

    let mut terminated = machine();
    terminated.reduce(&UpdateEvent::InstallingUpdate { application_terminated: true });
    assert_eq!(*terminated.phase(), UpdatePhase::Installing);
}

fn release_notes_events_do_not_move_the_phase() {
    let mut m = machine();
    m.reduce(&UpdateEvent::UpdateFound(sample_metadata(), UpdateStage::NotDownloaded));
    m.reduce(&UpdateEvent::ReleaseNotesAvailable);
    assert_eq!(*m.phase(), UpdatePhase::Available(sample_metadata(), UpdateStage::NotDownloaded));
    m.reduce(&UpdateEvent::ReleaseNotesFailed);
    assert_eq!(*m.phase(), UpdatePhase::Available(sample_metadata(), UpdateStage::NotDownloaded));
}

fn cancellations_return_to_idle() {
    let mut checking = machine();
    checking.reduce(&UpdateEvent::UserInitiatedCheckBegan);
    checking.reduce(&UpdateEvent::CheckCancelled);
    assert_eq!(*checking.phase(), UpdatePhase::Idle);

    let mut downloading = machine();
    downloading.reduce(&UpdateEvent::DownloadInitiated);
    downloading.reduce(&UpdateEvent::DownloadCancelled);
    assert_eq!(*downloading.phase(), UpdatePhase::Idle);
}

fn dismissal_returns_to_idle_from_any_state() {
    let setups = [
        machine(),
        {
            let mut m = UpdateStateMachine::new();
            m.reduce(&UpdateEvent::UpdateFound(sample_metadata(), UpdateStage::NotDownloaded));
            m
        },
        {
            let mut m = UpdateStateMachine::new();
            m.reduce(&UpdateEvent::DownloadInitiated);
            m.reduce(&UpdateEvent::ExpectedLength(10));
            m.reduce(&UpdateEvent::DataReceived(5));
            m
        },
        {
            let mut m = UpdateStateMachine::new();
            m.reduce(&UpdateEvent::UpdaterError(UpdateFailure::generic()));
            m
        },
    ];
    for setup in setups {
        let mut m = setup;
        m.reduce(&UpdateEvent::Dismissed);
        assert_eq!(*m.phase(), UpdatePhase::Idle);
    }
}

/// The complete happy-path ordering from a manual check to relaunch.
fn full_happy_path_ordering() {
    let mut m = machine();
    let expected = [
        UpdatePhase::Checking { user_initiated: true },
        UpdatePhase::Available(sample_metadata(), UpdateStage::NotDownloaded),
        UpdatePhase::Downloading { received: 0, expected: None },
        UpdatePhase::Downloading { received: 100, expected: Some(1_000) },
        UpdatePhase::Extracting { progress: None },
        UpdatePhase::Extracting { progress: Some(0.5) },
        UpdatePhase::ReadyToRelaunch,
        UpdatePhase::WaitingForTermination,
        UpdatePhase::Installing,
        UpdatePhase::Idle,
    ];
    for event in [
        UpdateEvent::UserInitiatedCheckBegan,
        UpdateEvent::UpdateFound(sample_metadata(), UpdateStage::NotDownloaded),
        UpdateEvent::DownloadInitiated,
        UpdateEvent::ExpectedLength(1_000),
        UpdateEvent::DataReceived(100),
        UpdateEvent::ExtractionBegan,
        UpdateEvent::ExtractionProgress(0.5),
        UpdateEvent::ReadyToInstallAndRelaunch,
        UpdateEvent::InstallingUpdate { application_terminated: false },
        UpdateEvent::InstallingUpdate { application_terminated: true },
        UpdateEvent::UpdateInstalled { relaunched: true },
    ] {
        m.reduce(&event);
    }
    assert_eq!(Some(m.phase()), expected.last());
}

// MARK: - Exactly-once capabilities

fn call_invokes_exactly_once() {
    let count = Rc::new(Cell::new(0));
    let body = count.clone();
    let capability = Capability::<i32>::new(move |_| body.set(body.get() + 1));
    capability.call(1);
    capability.call(2);
    capability.call(3);
    assert_eq!(count.get(), 1);
    assert!(!capability.is_armed());
}

fn discard_prevents_invocation() {
    let count = Rc::new(Cell::new(0));
    let body = count.clone();
    let capability = Capability::<()>::new(move |()| body.set(body.get() + 1));
    capability.discard();
    capability.call(());
    assert_eq!(count.get(), 0);
    assert!(!capability.is_armed());
}

// MARK: - Production configuration

fn info_dictionary(feed: &str, key: &str) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys = [NSString::from_str("SUFeedURL"), NSString::from_str("SUPublicEDKey")];
    let feed = NSString::from_str(feed);
    let key = NSString::from_str(key);
    NSDictionary::from_slices(&[&*keys[0], &*keys[1]], &[feed.as_ref() as &AnyObject, key.as_ref() as &AnyObject])
}

fn accepts_valid_https_feed_and_ed25519_key() {
    let zeros = NSData::with_bytes(&[0; 32]);
    let key = zeros.base64EncodedStringWithOptions(objc2_foundation::NSDataBase64EncodingOptions(0)).to_string();
    let info = info_dictionary("https://updates.example.test/appcast.xml", &key);
    assert!(UpdateConfiguration::is_valid(Some(&info)));
}

fn rejects_invalid_feed_or_key() {
    for feed in ["https://updates.example.test/appcast.xml", "http://updates.example.test/appcast.xml", "not a URL"] {
        let info = info_dictionary(feed, "PLACEHOLDER_DOWNRIGHT_ED25519_PUBLIC_KEY");
        assert!(!UpdateConfiguration::is_valid(Some(&info)), "{feed}");
    }
}

// MARK: - Coordinator flows

fn make_coordinator() -> (Rc<UpdateCoordinator>, Rc<FakeUpdateEngine>) {
    let engine = FakeUpdateEngine::new();
    let coordinator = UpdateCoordinator::new(Some(engine.clone() as Rc<dyn UpdateEngine>));
    coordinator.set_suppress_ui_for_testing(true);
    let _ = engine.start();
    (coordinator, engine)
}

fn install_reply_is_exactly_once() {
    let (coordinator, _) = make_coordinator();
    let replies = Rc::new(RefCell::new(Vec::new()));
    coordinator.driver_did_find_update(sample_metadata(), UpdateStage::NotDownloaded, true, recorder(&replies));
    coordinator.user_did_choose_install();
    coordinator.user_did_choose_install(); // second click must be a no-op
    assert_eq!(*replies.borrow(), [UpdateUserChoice::Install]);
}

fn skip_reply_is_exactly_once() {
    let (coordinator, _) = make_coordinator();
    let replies = Rc::new(RefCell::new(Vec::new()));
    coordinator.driver_did_find_update(sample_metadata(), UpdateStage::NotDownloaded, true, recorder(&replies));
    coordinator.user_did_choose_skip();
    coordinator.user_did_choose_skip();
    assert_eq!(*replies.borrow(), [UpdateUserChoice::Skip]);
}

fn later_reply_is_exactly_once() {
    let (coordinator, _) = make_coordinator();
    let replies = Rc::new(RefCell::new(Vec::new()));
    coordinator.driver_did_find_update(sample_metadata(), UpdateStage::NotDownloaded, true, recorder(&replies));
    coordinator.user_did_choose_later();
    coordinator.user_did_choose_later();
    assert_eq!(*replies.borrow(), [UpdateUserChoice::Later]);
}

fn ready_to_relaunch_uses_its_own_reply() {
    let (coordinator, _) = make_coordinator();
    let ready_replies = Rc::new(RefCell::new(Vec::new()));
    let found_replies = Rc::new(RefCell::new(Vec::new()));
    coordinator.driver_did_find_update(sample_metadata(), UpdateStage::NotDownloaded, true, recorder(&found_replies));
    // A stale found-update reply is replaced when the ready reply arrives.
    coordinator.driver_did_become_ready_to_relaunch(recorder(&ready_replies));
    coordinator.user_did_choose_install();
    assert_eq!(*ready_replies.borrow(), [UpdateUserChoice::Install]);
    assert!(found_replies.borrow().is_empty());
}

fn error_acknowledgement_is_exactly_once() {
    let (coordinator, _) = make_coordinator();
    let acks = Rc::new(Cell::new(0));
    coordinator.driver_did_encounter_error(&ns_error("sparkle", 2001, None), counter(&acks));
    coordinator.user_did_dismiss_panel();
    coordinator.user_did_dismiss_panel();
    assert_eq!(acks.get(), 1);
}

fn not_found_acknowledgement_is_exactly_once() {
    let (coordinator, _) = make_coordinator();
    let acks = Rc::new(Cell::new(0));
    coordinator.driver_did_find_no_update(true, counter(&acks));
    // The result stays visible until the user dismisses it; a second
    // dismissal must not acknowledge Sparkle twice.
    assert_eq!(acks.get(), 0);
    coordinator.user_did_choose_later();
    coordinator.user_did_dismiss_panel();
    assert_eq!(acks.get(), 1);
}

fn check_cancellation_is_exactly_once() {
    let (coordinator, _) = make_coordinator();
    let cancels = Rc::new(Cell::new(0));
    coordinator.driver_did_begin_user_check(counter(&cancels));
    coordinator.user_did_cancel_check();
    coordinator.user_did_cancel_check();
    assert_eq!(cancels.get(), 1);
}

fn download_cancellation_is_exactly_once() {
    let (coordinator, _) = make_coordinator();
    let cancels = Rc::new(Cell::new(0));
    coordinator.driver_did_begin_download(counter(&cancels));
    coordinator.user_did_cancel_download();
    coordinator.user_did_cancel_download();
    assert_eq!(cancels.get(), 1);
}

fn retry_termination_is_exactly_once() {
    let (coordinator, _) = make_coordinator();
    let retries = Rc::new(Cell::new(0));
    coordinator.driver_did_begin_installation(false, counter(&retries));
    coordinator.user_did_retry_termination();
    coordinator.user_did_retry_termination();
    assert_eq!(retries.get(), 1);
}

/// Closing the panel mid-check must invoke Sparkle's cancellation exactly
/// once and leave the machine idle — a leaked cancellation would leave the
/// check running with nobody able to stop it.
fn dismissal_during_check_cancels_exactly_once() {
    let (coordinator, _) = make_coordinator();
    let cancels = Rc::new(Cell::new(0));
    coordinator.driver_did_begin_user_check(counter(&cancels));
    assert_eq!(coordinator.phase(), UpdatePhase::Checking { user_initiated: true });
    coordinator.user_did_dismiss_panel();
    coordinator.user_did_dismiss_panel();
    assert_eq!(cancels.get(), 1);
    assert_eq!(coordinator.phase(), UpdatePhase::Idle);
}

/// Closing the panel mid-download must cancel the download exactly once.
fn dismissal_during_download_cancels_exactly_once() {
    let (coordinator, _) = make_coordinator();
    let cancels = Rc::new(Cell::new(0));
    coordinator.driver_did_begin_download(counter(&cancels));
    assert_eq!(coordinator.phase(), UpdatePhase::Downloading { received: 0, expected: None });
    coordinator.user_did_dismiss_panel();
    coordinator.user_did_dismiss_panel();
    assert_eq!(cancels.get(), 1);
    assert_eq!(coordinator.phase(), UpdatePhase::Idle);
}

/// The panel's Cancel buttons resolve the machine out of the active state.
fn cancel_buttons_resolve_the_machine() {
    let (coordinator, _) = make_coordinator();
    let check_cancels = Rc::new(Cell::new(0));
    coordinator.driver_did_begin_user_check(counter(&check_cancels));
    coordinator.user_did_cancel_check();
    assert_eq!(check_cancels.get(), 1);
    assert_eq!(coordinator.phase(), UpdatePhase::Idle);

    let download_cancels = Rc::new(Cell::new(0));
    coordinator.driver_did_begin_download(counter(&download_cancels));
    coordinator.user_did_cancel_download();
    assert_eq!(download_cancels.get(), 1);
    assert_eq!(coordinator.phase(), UpdatePhase::Idle);
}

/// Choosing Later/Skip from an offered update must leave `.available` so
/// the pill stops offering the update.
fn later_and_skip_leave_the_offered_state() {
    let (coordinator, _) = make_coordinator();
    coordinator.driver_did_find_update(sample_metadata(), UpdateStage::NotDownloaded, true, ignore_choice());
    coordinator.user_did_choose_later();
    assert_eq!(coordinator.phase(), UpdatePhase::Idle);

    coordinator.driver_did_find_update(sample_metadata(), UpdateStage::NotDownloaded, true, ignore_choice());
    coordinator.user_did_choose_skip();
    assert_eq!(coordinator.phase(), UpdatePhase::Idle);
}

// MARK: Background cycles stay on the pill

/// An automatically-scheduled presentation must not open the panel; a
/// user-initiated one must.
fn background_update_does_not_open_the_panel() {
    let (coordinator, _) = make_coordinator();
    coordinator.driver_did_find_update(sample_metadata(), UpdateStage::Downloaded, false, ignore_choice());
    assert_eq!(coordinator.panel_show_count(), 0);
    assert_eq!(coordinator.phase(), UpdatePhase::Available(sample_metadata(), UpdateStage::Downloaded));
    assert_eq!(coordinator.pill_model(), Some(UpdatePillModel::UpdateNow { version: "1.1.0".into(), is_ready: true }));

    coordinator.driver_did_find_update(sample_metadata(), UpdateStage::NotDownloaded, true, ignore_choice());
    assert!(coordinator.panel_show_count() > 0);
}

/// Automatic download + ready-to-relaunch must proceed silently on the
/// pill; the panel opens only when the user clicks it.
fn background_download_and_ready_stay_on_the_pill() {
    let (coordinator, _) = make_coordinator();
    coordinator.driver_did_begin_download(nothing());
    assert_eq!(coordinator.panel_show_count(), 0);
    assert_eq!(coordinator.phase(), UpdatePhase::Downloading { received: 0, expected: None });
    coordinator.driver_did_become_ready_to_relaunch(ignore_choice());
    assert_eq!(coordinator.panel_show_count(), 0);
    assert_eq!(coordinator.phase(), UpdatePhase::ReadyToRelaunch);
    assert_eq!(coordinator.pill_model().map(|pill| pill.offers_install()), Some(true));
}

/// A background-cycle failure is a non-event: no window, and no warning
/// pill either. It used to leave an orange badge on every window because
/// a laptop was opened without wifi, which is the alarm DESIGN.md rules
/// out; the coordinator now acknowledges and finishes the cycle instead.
fn background_error_is_silent_and_finishes_the_cycle() {
    let (coordinator, _) = make_coordinator();
    coordinator.driver_did_encounter_error(&ns_error("Sparkle", 1002, None), nothing());
    assert_eq!(coordinator.panel_show_count(), 0);
    assert_eq!(coordinator.pill_model(), None);
    assert_eq!(coordinator.phase(), UpdatePhase::Idle, "the cycle must end, or the next check can never start");
}

fn teardown_discards_pending_reply() {
    let (coordinator, _) = make_coordinator();
    let replies = Rc::new(RefCell::new(Vec::new()));
    coordinator.driver_did_find_update(sample_metadata(), UpdateStage::NotDownloaded, true, recorder(&replies));
    coordinator.tear_down_for_testing();
    coordinator.user_did_choose_install();
    assert!(replies.borrow().is_empty());
    assert_eq!(coordinator.phase(), UpdatePhase::Idle);
}

fn dismissal_discards_pending_reply() {
    let (coordinator, _) = make_coordinator();
    let replies = Rc::new(RefCell::new(Vec::new()));
    coordinator.driver_did_find_update(sample_metadata(), UpdateStage::NotDownloaded, true, recorder(&replies));
    coordinator.driver_did_dismiss();
    coordinator.user_did_choose_install();
    assert!(replies.borrow().is_empty());
    assert_eq!(coordinator.phase(), UpdatePhase::Idle);
}

fn release_notes_never_leak_into_the_next_update_cycle() {
    let (coordinator, _) = make_coordinator();
    coordinator.driver_did_find_update(sample_metadata(), UpdateStage::NotDownloaded, true, ignore_choice());
    coordinator.driver_did_receive_release_notes(b"v1 notes".to_vec());
    assert_eq!(coordinator.release_notes(), UpdateReleaseNotesState::Loaded(b"v1 notes".to_vec()));

    coordinator.driver_did_dismiss();
    assert_eq!(coordinator.release_notes(), UpdateReleaseNotesState::None);
    coordinator.driver_did_find_update(
        UpdateMetadata {
            version_string: "48".into(),
            display_version_string: "1.1.1".into(),
            title: None,
            item_description: None,
            release_notes_url: None,
            info_url: None,
            content_length: 0,
            is_information_only: false,
            is_major_upgrade: false,
            is_critical: false,
            minimum_system_version: None,
        },
        UpdateStage::NotDownloaded,
        true,
        ignore_choice(),
    );
    assert_eq!(coordinator.release_notes(), UpdateReleaseNotesState::None);
}

// MARK: Background downloads

fn background_download_drives_update_now_pill() {
    let (coordinator, engine) = make_coordinator();
    assert_eq!(coordinator.pill_model(), None);
    engine.complete_background_download("1.1.0");
    assert_eq!(coordinator.pill_model(), Some(UpdatePillModel::UpdateNow { version: "1.1.0".into(), is_ready: true }));
    assert!(coordinator.downloaded_update().is_some());
    coordinator.driver_did_dismiss();
    assert!(coordinator.downloaded_update().is_none());
    assert_eq!(coordinator.pill_model(), None);
}

// MARK: Error handling

/// Only a check the user asked for reports its failure. The begin call is
/// what marks the cycle user-initiated, exactly as Sparkle drives it.
fn user_initiated_error_produces_warning_pill_and_retryable_state() {
    let (coordinator, _) = make_coordinator();
    coordinator.driver_did_begin_user_check(nothing());
    coordinator.driver_did_encounter_error(&ns_error("Sparkle", 1002, Some("The feed couldn't be loaded.")), nothing());
    assert_eq!(coordinator.pill_model(), Some(UpdatePillModel::Warning));
    let UpdatePhase::Failed(_, retryable) = coordinator.phase() else {
        panic!("expected failed phase");
    };
    assert!(retryable);
}

fn retry_acknowledges_and_returns_to_idle() {
    let (coordinator, _) = make_coordinator();
    let acks = Rc::new(Cell::new(0));
    coordinator.driver_did_encounter_error(&ns_error("sparkle", 3001, None), counter(&acks));
    coordinator.user_did_retry();
    // The retried check is scheduled for the next runloop turn; in a test
    // binary there is no feed configuration, so checkForUpdates() gates at
    // isConfigured and stays quiet. Hopping through the main queue lets
    // that turn happen — the queue is FIFO, so our block runs after the
    // retry's. What must hold: the failed cycle is acknowledged exactly
    // once and the machine returns to idle.
    drain_main_queue();
    assert_eq!(acks.get(), 1);
    assert_eq!(coordinator.phase(), UpdatePhase::Idle);
}

// MARK: Informational / critical

fn informational_update_never_offers_install() {
    let (coordinator, _) = make_coordinator();
    let replies = Rc::new(RefCell::new(Vec::new()));
    coordinator.driver_did_find_update(informational_metadata(), UpdateStage::NotDownloaded, true, recorder(&replies));
    assert_eq!(coordinator.phase(), UpdatePhase::Informational(informational_metadata()));
    coordinator.user_did_choose_later();
    assert_eq!(*replies.borrow(), [UpdateUserChoice::Later]);
}

fn critical_update_keeps_skip_available_to_the_machine() {
    let (coordinator, _) = make_coordinator();
    let mut critical = sample_metadata();
    critical.is_critical = true;
    let replies = Rc::new(RefCell::new(Vec::new()));
    coordinator.driver_did_find_update(critical, UpdateStage::NotDownloaded, true, recorder(&replies));
    // The machine still supports skip; the UI hides the button for critical
    // updates (asserted by the panel's own logic in UpdateWindowController).
    coordinator.user_did_choose_skip();
    assert_eq!(*replies.borrow(), [UpdateUserChoice::Skip]);
}

// MARK: Settings

fn settings_proxy_writes_through_to_engine() {
    let (coordinator, engine) = make_coordinator();
    coordinator.set_automatically_checks_for_updates(false);
    assert!(!engine.automatically_checks_for_updates.get());
    coordinator.set_automatically_downloads_updates(false);
    assert!(!engine.automatically_downloads_updates.get());
    coordinator.set_automatically_checks_for_updates(true);
    assert!(engine.automatically_checks_for_updates.get());
}

fn can_check_for_updates_reflects_engine() {
    let (coordinator, engine) = make_coordinator();
    // This test binary has no SUFeedURL, so the configuration gate says no.
    assert!(!coordinator.can_check_for_updates());
    engine._can_check_for_updates.set(false);
    assert!(!coordinator.can_check_for_updates());
}

// MARK: Pill synchronization (multiwindow)

fn state_change_broadcasts_to_one_coordinator_for_all_pills() {
    let (coordinator, _) = make_coordinator();
    let received = Rc::new(Cell::new(0));
    let sink = received.clone();
    let block = RcBlock::new(move |_notification: NonNull<NSNotification>| sink.set(sink.get() + 1));
    let center = NSNotificationCenter::defaultCenter();
    // SAFETY: the block runs on the main queue, this thread.
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(&NSString::from_str(UpdateCoordinator::STATE_DID_CHANGE)),
            Some(coordinator.notification_object()),
            Some(&NSOperationQueue::mainQueue()),
            &block,
        )
    };
    coordinator.driver_did_receive_data(500);
    coordinator.driver_did_receive_expected_length(1_000);
    // SAFETY: the token this centre issued.
    unsafe { center.removeObserver(token.as_ref()) };
    assert!(received.get() >= 2);
}

// MARK: - Panel footer and transitions (window-free parts)

/// `UpdatePanelFooterTests.rebuildsAfterCheckIntoAvailableState` without the
/// footer view (UI port): the coordinator side of the transition.
fn rebuilds_after_check_into_available_state() {
    let engine = FakeUpdateEngine::new();
    let coordinator = UpdateCoordinator::new(Some(engine.clone() as Rc<dyn UpdateEngine>));
    coordinator.set_suppress_ui_for_testing(true);
    let _ = engine.start();

    coordinator.driver_did_begin_user_check(nothing());
    coordinator.driver_did_find_update(sample_metadata(), UpdateStage::NotDownloaded, true, ignore_choice());
    assert_eq!(coordinator.phase(), UpdatePhase::Available(sample_metadata(), UpdateStage::NotDownloaded));
}

/// `UpdatePanelTransitionTests.rendersEveryUpdaterPhaseWithoutThrowing`
/// without the panel renders (UI port): every callback in the same order.
fn renders_every_updater_phase_without_throwing() {
    let engine = FakeUpdateEngine::new();
    let coordinator = UpdateCoordinator::new(Some(engine.clone() as Rc<dyn UpdateEngine>));
    coordinator.set_suppress_ui_for_testing(true);
    let _ = engine.start();

    coordinator.driver_did_begin_user_check(nothing());
    coordinator.driver_did_find_update(sample_metadata(), UpdateStage::NotDownloaded, true, ignore_choice());
    coordinator.driver_did_begin_download(nothing());
    coordinator.driver_did_receive_expected_length(1_000);
    coordinator.driver_did_receive_data(250);
    coordinator.driver_did_begin_extraction();
    coordinator.driver_did_receive_extraction_progress(0.5);
    coordinator.driver_did_become_ready_to_relaunch(ignore_choice());
    coordinator.driver_did_begin_installation(false, nothing());
    coordinator.driver_did_begin_installation(true, nothing());
    coordinator.driver_did_begin_user_check(nothing());
    coordinator.driver_did_find_no_update(true, nothing());
    coordinator.driver_did_begin_user_check(nothing());
    coordinator.driver_did_encounter_error(&ns_error("sparkle", 2001, None), nothing());
    coordinator.driver_did_find_update(informational_metadata(), UpdateStage::NotDownloaded, true, ignore_choice());
}

// MARK: - Release watch policy

#[allow(clippy::field_reassign_with_default)] // the Swift test's shape: default, then set
fn frontmost_checks_far_more_often_than_backgrounded() {
    let mut policy = ReleaseWatchPolicy::default();
    policy.is_app_active = true;
    assert_eq!(policy.interval(), Some(ReleaseWatchPolicy::ACTIVE_INTERVAL));
    policy.is_app_active = false;
    assert_eq!(policy.interval(), Some(ReleaseWatchPolicy::BACKGROUND_INTERVAL));
}

/// No network is not a failure state; it is simply nothing to do.
fn no_network_suspends_entirely() {
    let mut policy = ReleaseWatchPolicy { is_app_active: true, is_low_power: false, has_network: false };
    assert_eq!(policy.interval(), None);
    policy.has_network = true;
    assert!(policy.interval().is_some());
}

/// Low Power Mode is an explicit request to stop doing optional work, and
/// polling for a build nobody asked for is exactly that.
fn low_power_backs_off_in_front_and_stops_behind() {
    let mut policy = ReleaseWatchPolicy { is_app_active: true, is_low_power: true, has_network: true };
    assert_eq!(policy.interval(), Some(ReleaseWatchPolicy::BACKGROUND_INTERVAL));
    policy.is_app_active = false;
    assert_eq!(policy.interval(), None);
}

// MARK: - Release watch

struct FakeReleaseFeedProbe {
    results: RefCell<Vec<ReleaseFeedProbeResult>>,
    /// Every validator the watch offered, in order — the evidence that a
    /// conditional request is actually conditional.
    validators: RefCell<Vec<Option<String>>>,
}

impl FakeReleaseFeedProbe {
    fn new(results: Vec<ReleaseFeedProbeResult>) -> Rc<FakeReleaseFeedProbe> {
        Rc::new(FakeReleaseFeedProbe { results: RefCell::new(results), validators: RefCell::new(Vec::new()) })
    }
}

impl ReleaseFeedProbe for FakeReleaseFeedProbe {
    fn probe(&self, _feed: &Url, validator: Option<&str>, completion: Box<dyn FnOnce(ReleaseFeedProbeResult)>) {
        self.validators.borrow_mut().push(validator.map(str::to_owned));
        let mut results = self.results.borrow_mut();
        let result = if results.is_empty() { ReleaseFeedProbeResult::Unchanged } else { results.remove(0) };
        drop(results);
        // The Swift fake returns without suspending.
        completion(result);
    }
}

fn feed() -> Url {
    Url::from_string("https://example.invalid/appcast.xml").unwrap()
}

/// The watch hands its work to the main queue; run it until it has landed
/// rather than sleeping for a duration that would only ever be a guess.
fn settle(watch: &ReleaseWatch, probes: isize) {
    pump(|| watch.completed_probe_count() >= probes, Duration::from_secs(5));
}

/// `await Task.yield()`: one main-queue turn.
fn yield_once() {
    drain_main_queue();
}

fn make_watch(results: Vec<ReleaseFeedProbeResult>) -> (Rc<ReleaseWatch>, Rc<FakeReleaseFeedProbe>) {
    let probe = FakeReleaseFeedProbe::new(results);
    (ReleaseWatch::new(feed(), Some(probe.clone() as Rc<dyn ReleaseFeedProbe>)), probe)
}

fn fire_counter(watch: &ReleaseWatch) -> Rc<Cell<i32>> {
    let fired = Rc::new(Cell::new(0));
    let sink = fired.clone();
    watch.set_on_feed_changed(Some(Rc::new(move || sink.set(sink.get() + 1))));
    fired
}

fn changed(validator: &str) -> ReleaseFeedProbeResult {
    ReleaseFeedProbeResult::Changed { validator: Some(validator.into()) }
}

/// Sparkle's own post-launch check already answers "was something waiting
/// when I opened the app". The first probe only learns where to start.
fn first_probe_establishes_the_baseline_without_firing() {
    let (watch, _) = make_watch(vec![changed("etag:a")]);
    let fired = fire_counter(&watch);
    watch.start(false);
    settle(&watch, 1);
    assert!(watch.has_baseline());
    assert_eq!(fired.get(), 0);
    watch.stop();
}

fn a_moved_feed_fires_once() {
    let (watch, probe) = make_watch(vec![changed("etag:a"), changed("etag:b")]);
    let fired = fire_counter(&watch);
    watch.start(false);
    settle(&watch, 1);
    watch.probe_now();
    settle(&watch, 2);
    assert_eq!(fired.get(), 1);
    // The second request carried the first response's validator back.
    assert_eq!(*probe.validators.borrow(), [None, Some("etag:a".to_owned())]);
    watch.stop();
}

fn an_unchanged_feed_is_silent() {
    let (watch, _) =
        make_watch(vec![changed("etag:a"), ReleaseFeedProbeResult::Unchanged, ReleaseFeedProbeResult::Unchanged]);
    let fired = fire_counter(&watch);
    watch.start(false);
    settle(&watch, 1);
    watch.probe_now();
    settle(&watch, 2);
    watch.probe_now();
    settle(&watch, 3);
    assert_eq!(fired.get(), 0);
    watch.stop();
}

/// An unreachable feed must not be mistaken for a baseline, or the first
/// successful probe after a flight would look like a brand-new release.
fn an_unreachable_feed_does_not_establish_the_baseline() {
    let (watch, _) = make_watch(vec![ReleaseFeedProbeResult::Unreachable, changed("etag:a")]);
    let fired = fire_counter(&watch);
    watch.start(false);
    settle(&watch, 1);
    assert!(!watch.has_baseline());
    watch.probe_now();
    settle(&watch, 2);
    assert!(watch.has_baseline());
    assert_eq!(fired.get(), 0);
    watch.stop();
}

/// Wake fires activation too, and a held Cmd-Tab flaps it several times a
/// second. Without a floor each of those becomes its own request.
fn coinciding_events_collapse_into_one_probe() {
    let (watch, _) = make_watch(vec![changed("etag:a")]);
    watch.start(false);
    settle(&watch, 1);
    watch.system_did_wake();
    watch.application_did_change_activation(true);
    watch.network_availability_did_change(true);
    yield_once();
    assert_eq!(watch.completed_probe_count(), 1);
    watch.stop();
}

fn losing_the_network_stops_probing() {
    let (watch, _) = make_watch(vec![changed("etag:a")]);
    watch.start(false);
    settle(&watch, 1);
    watch.network_availability_did_change(false);
    watch.probe_now();
    yield_once();
    assert_eq!(watch.completed_probe_count(), 1);
    watch.stop();
}

// MARK: - Update Now (one press installs)

fn only_actionable_states_offer_an_install() {
    assert!(UpdatePillModel::UpdateNow { version: "1.1.0".into(), is_ready: false }.offers_install());
    assert!(UpdatePillModel::RestartToUpdate.offers_install());
    assert!(!UpdatePillModel::Warning.offers_install());
    assert!(!UpdatePillModel::Progress("Updating…".into(), None).offers_install());
    assert!(!UpdatePillModel::Informational("1.2.0".into()).offers_install());
}

/// The whole point of the control: the press is the decision, so no window
/// opens to ask it again.
fn press_installs_without_opening_the_panel() {
    let (coordinator, _) = make_coordinator();
    let replies = Rc::new(RefCell::new(Vec::new()));
    coordinator.driver_did_find_update(sample_metadata(), UpdateStage::NotDownloaded, false, recorder(&replies));
    assert_eq!(coordinator.pill_model(), Some(UpdatePillModel::UpdateNow { version: "1.1.0".into(), is_ready: false }));
    coordinator.user_did_press_update_now();
    assert_eq!(*replies.borrow(), [UpdateUserChoice::Install]);
    assert_eq!(coordinator.panel_show_count(), 0);
}

/// A background download leaves no prompt armed, so the press has to
/// re-enter Sparkle — and that cycle must stay as silent as the press.
fn press_on_a_background_download_reenters_sparkle_silently() {
    let (coordinator, engine) = make_coordinator();
    engine.complete_background_download("1.1.0");
    coordinator.user_did_press_update_now();
    assert_eq!(engine.foreground_check_count(), 1);
    assert!(coordinator.is_expedited_install());

    coordinator.driver_did_begin_user_check(nothing());
    assert_eq!(coordinator.panel_show_count(), 0, "a press must not open the panel it replaced");

    let replies = Rc::new(RefCell::new(Vec::new()));
    coordinator.driver_did_find_update(sample_metadata(), UpdateStage::Downloaded, true, recorder(&replies));
    assert_eq!(*replies.borrow(), [UpdateUserChoice::Install]);
    assert_eq!(coordinator.panel_show_count(), 0);
}

fn an_expedited_ready_to_relaunch_installs_immediately() {
    let (coordinator, engine) = make_coordinator();
    engine.complete_background_download("1.1.0");
    coordinator.user_did_press_update_now();
    coordinator.driver_did_begin_user_check(nothing());
    coordinator.driver_did_find_update(sample_metadata(), UpdateStage::NotDownloaded, true, ignore_choice());
    coordinator.driver_did_begin_download(nothing());
    assert_eq!(coordinator.panel_show_count(), 0, "progress belongs on the pill during a press");

    let replies = Rc::new(RefCell::new(Vec::new()));
    coordinator.driver_did_become_ready_to_relaunch(recorder(&replies));
    assert_eq!(*replies.borrow(), [UpdateUserChoice::Install]);
    assert_eq!(coordinator.panel_show_count(), 0);
}

/// A press that could not be honoured owes an explanation, so the failure
/// path deliberately drops back to the ordinary panel.
fn an_expedited_failure_falls_back_to_the_panel() {
    let (coordinator, engine) = make_coordinator();
    engine.complete_background_download("1.1.0");
    coordinator.user_did_press_update_now();
    coordinator.driver_did_begin_user_check(nothing());
    coordinator.driver_did_encounter_error(&ns_error("Sparkle", 2001, Some("no")), nothing());
    assert!(!coordinator.is_expedited_install());
    assert!(coordinator.panel_show_count() > 0);
    assert_eq!(coordinator.pill_model(), Some(UpdatePillModel::Warning));
}

/// Once installation has begun only the quit is outstanding, so the pill
/// says so and the press retries the quit rather than starting again.
fn press_while_waiting_for_termination_retries_the_quit() {
    let (coordinator, _) = make_coordinator();
    let retries = Rc::new(Cell::new(0));
    coordinator.driver_did_begin_installation(false, counter(&retries));
    assert_eq!(coordinator.pill_model(), Some(UpdatePillModel::RestartToUpdate));
    coordinator.user_did_press_update_now();
    assert_eq!(retries.get(), 1);
}

/// The phases after `.available` carry no metadata of their own; without
/// the cycle keeping hold of it, the tooltip and hover notes go blank
/// halfway through the install they are describing.
fn the_version_survives_to_ready_to_relaunch() {
    let (coordinator, _) = make_coordinator();
    coordinator.driver_did_find_update(sample_metadata(), UpdateStage::NotDownloaded, false, ignore_choice());
    coordinator.driver_did_begin_download(nothing());
    coordinator.driver_did_become_ready_to_relaunch(ignore_choice());
    assert_eq!(coordinator.pending_update().map(|update| update.display_version_string).as_deref(), Some("1.1.0"));
    assert_eq!(coordinator.pill_model(), Some(UpdatePillModel::UpdateNow { version: "1.1.0".into(), is_ready: true }));
    coordinator.driver_did_dismiss();
    assert!(coordinator.pending_update().is_none());
}

// MARK: - Release watch → coordinator

/// The full extent of the watch's authority: it asks Sparkle to look.
fn a_moved_feed_asks_sparkle_to_look() {
    let (coordinator, engine) = make_coordinator();
    assert_eq!(engine.background_check_count(), 0);
    coordinator.release_feed_did_change();
    assert_eq!(engine.background_check_count(), 1);
    assert_eq!(coordinator.release_watch_trigger_count(), 1);
}

/// The reader is watching a download or reading a prompt; restarting
/// underneath them replaces what they are looking at with the same answer.
fn a_live_cycle_is_left_alone() {
    let (coordinator, engine) = make_coordinator();
    coordinator.driver_did_begin_user_check(nothing());
    coordinator.release_feed_did_change();
    assert_eq!(engine.background_check_count(), 0);
}

fn an_already_waiting_update_is_not_rechecked() {
    let (coordinator, engine) = make_coordinator();
    engine.complete_background_download("1.1.0");
    coordinator.release_feed_did_change();
    assert_eq!(engine.background_check_count(), 0);
}

/// A bundle with no updater has nothing to trigger.
fn a_stopped_engine_is_never_triggered() {
    let engine = FakeUpdateEngine::new();
    let coordinator = UpdateCoordinator::new(Some(engine.clone() as Rc<dyn UpdateEngine>));
    coordinator.set_suppress_ui_for_testing(true);
    coordinator.release_feed_did_change();
    assert_eq!(engine.background_check_count(), 0);
    assert_eq!(coordinator.release_watch_trigger_count(), 0);
}

// MARK: - The automatic-check setting governs the watch too

/// "Check for updates automatically" has to mean every automatic check.
/// A setting that stopped Sparkle's schedule and left the watch polling
/// would be a narrower promise than the one the settings pane makes.
fn turning_automatic_checks_off_silences_the_watch() {
    let engine = FakeUpdateEngine::new();
    let coordinator = UpdateCoordinator::new(Some(engine.clone() as Rc<dyn UpdateEngine>));
    coordinator.set_suppress_ui_for_testing(true);
    let _ = engine.start();

    coordinator.release_feed_did_change();
    assert_eq!(engine.background_check_count(), 1);

    coordinator.set_automatically_checks_for_updates(false);
    coordinator.release_feed_did_change();
    assert_eq!(engine.background_check_count(), 1, "the watch must not outlive the setting");

    coordinator.set_automatically_checks_for_updates(true);
    coordinator.release_feed_did_change();
    assert_eq!(engine.background_check_count(), 2);
}

fn main() {
    macro_rules! tests {
        ($($suite:literal / $name:literal => $function:ident),* $(,)?) => {
            &[$(Test { name: concat!($suite, "/", $name), run: $function }),*]
        };
    }
    let tests: &[Test] = tests![
        "UpdateStateMachineTests" / "checkPhases" => check_phases,
        "UpdateStateMachineTests" / "updateFoundDistinguishesInformational" => update_found_distinguishes_informational,
        "UpdateStateMachineTests" / "notFoundDependsOnUserInitiation" => not_found_depends_on_user_initiation,
        "UpdateStateMachineTests" / "errorPhase" => error_phase,
        "UpdateStateMachineTests" / "downloadProgressAccounting" => download_progress_accounting,
        "UpdateStateMachineTests" / "extractionProgressIsClamped" => extraction_progress_is_clamped,
        "UpdateStateMachineTests" / "installationStages" => installation_stages,
        "UpdateStateMachineTests" / "releaseNotesEventsDoNotMoveThePhase" => release_notes_events_do_not_move_the_phase,
        "UpdateStateMachineTests" / "cancellationsReturnToIdle" => cancellations_return_to_idle,
        "UpdateStateMachineTests" / "dismissalReturnsToIdleFromAnyState" => dismissal_returns_to_idle_from_any_state,
        "UpdateStateMachineTests" / "fullHappyPathOrdering" => full_happy_path_ordering,
        "UpdateCapabilityTests" / "callInvokesExactlyOnce" => call_invokes_exactly_once,
        "UpdateCapabilityTests" / "discardPreventsInvocation" => discard_prevents_invocation,
        "UpdateConfigurationTests" / "acceptsValidHTTPSFeedAndEd25519Key" => accepts_valid_https_feed_and_ed25519_key,
        "UpdateConfigurationTests" / "rejectsInvalidFeedOrKey" => rejects_invalid_feed_or_key,
        "UpdateCoordinatorFlowTests" / "installReplyIsExactlyOnce" => install_reply_is_exactly_once,
        "UpdateCoordinatorFlowTests" / "skipReplyIsExactlyOnce" => skip_reply_is_exactly_once,
        "UpdateCoordinatorFlowTests" / "laterReplyIsExactlyOnce" => later_reply_is_exactly_once,
        "UpdateCoordinatorFlowTests" / "readyToRelaunchUsesItsOwnReply" => ready_to_relaunch_uses_its_own_reply,
        "UpdateCoordinatorFlowTests" / "errorAcknowledgementIsExactlyOnce" => error_acknowledgement_is_exactly_once,
        "UpdateCoordinatorFlowTests" / "notFoundAcknowledgementIsExactlyOnce" => not_found_acknowledgement_is_exactly_once,
        "UpdateCoordinatorFlowTests" / "checkCancellationIsExactlyOnce" => check_cancellation_is_exactly_once,
        "UpdateCoordinatorFlowTests" / "downloadCancellationIsExactlyOnce" => download_cancellation_is_exactly_once,
        "UpdateCoordinatorFlowTests" / "retryTerminationIsExactlyOnce" => retry_termination_is_exactly_once,
        "UpdateCoordinatorFlowTests" / "dismissalDuringCheckCancelsExactlyOnce" => dismissal_during_check_cancels_exactly_once,
        "UpdateCoordinatorFlowTests" / "dismissalDuringDownloadCancelsExactlyOnce" => dismissal_during_download_cancels_exactly_once,
        "UpdateCoordinatorFlowTests" / "cancelButtonsResolveTheMachine" => cancel_buttons_resolve_the_machine,
        "UpdateCoordinatorFlowTests" / "laterAndSkipLeaveTheOfferedState" => later_and_skip_leave_the_offered_state,
        "UpdateCoordinatorFlowTests" / "backgroundUpdateDoesNotOpenThePanel" => background_update_does_not_open_the_panel,
        "UpdateCoordinatorFlowTests" / "backgroundDownloadAndReadyStayOnThePill" => background_download_and_ready_stay_on_the_pill,
        "UpdateCoordinatorFlowTests" / "backgroundErrorIsSilentAndFinishesTheCycle" => background_error_is_silent_and_finishes_the_cycle,
        "UpdateCoordinatorFlowTests" / "teardownDiscardsPendingReply" => teardown_discards_pending_reply,
        "UpdateCoordinatorFlowTests" / "dismissalDiscardsPendingReply" => dismissal_discards_pending_reply,
        "UpdateCoordinatorFlowTests" / "releaseNotesNeverLeakIntoTheNextUpdateCycle" => release_notes_never_leak_into_the_next_update_cycle,
        "UpdateCoordinatorFlowTests" / "backgroundDownloadDrivesUpdateNowPill" => background_download_drives_update_now_pill,
        "UpdateCoordinatorFlowTests" / "userInitiatedErrorProducesWarningPillAndRetryableState" => user_initiated_error_produces_warning_pill_and_retryable_state,
        "UpdateCoordinatorFlowTests" / "retryAcknowledgesAndReturnsToIdle" => retry_acknowledges_and_returns_to_idle,
        "UpdateCoordinatorFlowTests" / "informationalUpdateNeverOffersInstall" => informational_update_never_offers_install,
        "UpdateCoordinatorFlowTests" / "criticalUpdateKeepsSkipAvailableToTheMachine" => critical_update_keeps_skip_available_to_the_machine,
        "UpdateCoordinatorFlowTests" / "settingsProxyWritesThroughToEngine" => settings_proxy_writes_through_to_engine,
        "UpdateCoordinatorFlowTests" / "canCheckForUpdatesReflectsEngine" => can_check_for_updates_reflects_engine,
        "UpdateCoordinatorFlowTests" / "stateChangeBroadcastsToOneCoordinatorForAllPills" => state_change_broadcasts_to_one_coordinator_for_all_pills,
        "UpdatePanelFooterTests" / "rebuildsAfterCheckIntoAvailableState (coordinator part)" => rebuilds_after_check_into_available_state,
        "UpdatePanelTransitionTests" / "rendersEveryUpdaterPhaseWithoutThrowing (coordinator part)" => renders_every_updater_phase_without_throwing,
        "ReleaseWatchPolicyTests" / "frontmostChecksFarMoreOftenThanBackgrounded" => frontmost_checks_far_more_often_than_backgrounded,
        "ReleaseWatchPolicyTests" / "noNetworkSuspendsEntirely" => no_network_suspends_entirely,
        "ReleaseWatchPolicyTests" / "lowPowerBacksOffInFrontAndStopsBehind" => low_power_backs_off_in_front_and_stops_behind,
        "ReleaseWatchTests" / "firstProbeEstablishesTheBaselineWithoutFiring" => first_probe_establishes_the_baseline_without_firing,
        "ReleaseWatchTests" / "aMovedFeedFiresOnce" => a_moved_feed_fires_once,
        "ReleaseWatchTests" / "anUnchangedFeedIsSilent" => an_unchanged_feed_is_silent,
        "ReleaseWatchTests" / "anUnreachableFeedDoesNotEstablishTheBaseline" => an_unreachable_feed_does_not_establish_the_baseline,
        "ReleaseWatchTests" / "coincidingEventsCollapseIntoOneProbe" => coinciding_events_collapse_into_one_probe,
        "ReleaseWatchTests" / "losingTheNetworkStopsProbing" => losing_the_network_stops_probing,
        "UpdateNowPressTests" / "onlyActionableStatesOfferAnInstall" => only_actionable_states_offer_an_install,
        "UpdateNowPressTests" / "pressInstallsWithoutOpeningThePanel" => press_installs_without_opening_the_panel,
        "UpdateNowPressTests" / "pressOnABackgroundDownloadReentersSparkleSilently" => press_on_a_background_download_reenters_sparkle_silently,
        "UpdateNowPressTests" / "anExpeditedReadyToRelaunchInstallsImmediately" => an_expedited_ready_to_relaunch_installs_immediately,
        "UpdateNowPressTests" / "anExpeditedFailureFallsBackToThePanel" => an_expedited_failure_falls_back_to_the_panel,
        "UpdateNowPressTests" / "pressWhileWaitingForTerminationRetriesTheQuit" => press_while_waiting_for_termination_retries_the_quit,
        "UpdateNowPressTests" / "theVersionSurvivesToReadyToRelaunch" => the_version_survives_to_ready_to_relaunch,
        "ReleaseWatchTriggerTests" / "aMovedFeedAsksSparkleToLook" => a_moved_feed_asks_sparkle_to_look,
        "ReleaseWatchTriggerTests" / "aLiveCycleIsLeftAlone" => a_live_cycle_is_left_alone,
        "ReleaseWatchTriggerTests" / "anAlreadyWaitingUpdateIsNotRechecked" => an_already_waiting_update_is_not_rechecked,
        "ReleaseWatchTriggerTests" / "aStoppedEngineIsNeverTriggered" => a_stopped_engine_is_never_triggered,
        "ReleaseWatchSettingTests" / "turningAutomaticChecksOffSilencesTheWatch" => turning_automatic_checks_off_silences_the_watch,
    ];
    let panels = "ported with its subject, in Sources/DownrightApp/Panels: tests/panels_update_tests.rs";
    let skipped = [
        Skipped { name: "UpdatePanelFooterTests/rebuildsAfterCheckIntoAvailableState (footer view)", reason: "UpdatePanelFooter is a view (UI port); the coordinator part runs above" },
        Skipped { name: "UpdatePanelTransitionTests/rendersEveryUpdaterPhaseWithoutThrowing (panel renders)", reason: "UpdatePanelView is a view (UI port); the coordinator part runs above" },
        Skipped { name: "UpdateBuildContractTests/sparkleIsImportedOnlyByTheHostApp", reason: "ported in sparkle_bridge_tests" },
        Skipped { name: "UpdateBuildContractTests/packageAndXcodeProjectDeclareSparkleExactly", reason: "ported in sparkle_bridge_tests" },
        Skipped { name: "UpdateNotesSummaryTests/dropsTheLeadingTitle", reason: panels },
        Skipped { name: "UpdateNotesSummaryTests/stopsWellShortOfAWallOfText", reason: panels },
        Skipped { name: "UpdateNotesSummaryTests/handlesNoNotesAtAll", reason: panels },
        Skipped { name: "UpdateNotesSummaryTests/keepsShortNotesWhole", reason: panels },
        Skipped { name: "UpdateReleaseNotesReductionTests/stripsTagsAndKeepsReadableText", reason: panels },
        Skipped { name: "UpdateReleaseNotesReductionTests/nonHTMLDataPassesThroughAsText", reason: panels },
    ];
    updater_support::run("update_coordinator_tests", tests, &skipped);
}
