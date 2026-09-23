//! Rust side of the `updater` suite; mirrors
//! `oracle/app/Sources/downright-app-oracle/UpdaterDump.swift`, whose doc
//! comment describes the case kinds and the output.
//!
//! Clocks: `ReleaseWatch` reads the wall clock for its 20 s spacing floor
//! with no injection point, on both sides. No timestamp reaches the output
//! (every script runs in well under 20 s), so nothing is normalised. The
//! coordinator's `lastUpdateCheckDate` is injected through `FakeUpdateEngine`.
//!
//! Nothing reaches the network (stub `NSURLProtocol`, `.invalid` hosts) and
//! nothing opens a window, alert, URL or beep (`suppress_ui_for_testing`; an
//! informational "Update Now" press is reported as skipped).

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, NSObjectProtocol};
use objc2::{AllocAnyThread, ClassType, Message, define_class};
use objc2_foundation::{
    NSArray, NSData, NSDictionary, NSError, NSHTTPURLResponse, NSJSONReadingOptions, NSJSONSerialization,
    NSNotification, NSNotificationCenter, NSString, NSURLCacheStoragePolicy, NSURLProtocol, NSURLProtocolClient,
    NSURLRequest, NSURLResponse, NSURLSession, NSURLSessionConfiguration,
};
use serde_json::{Value, json};
use upleft_app::updater::downright_update_driver::{
    DownrightUpdateDriver, SpuUpdatePermissionRequest, SpuUserUpdateChoice, UpdateDriverHost, UpdateUserChoice,
};
use upleft_app::updater::release_watch::{
    ReleaseFeedProbe, ReleaseFeedProbeResult, ReleaseFeedURLProbe, ReleaseWatch, ReleaseWatchPolicy,
};
use upleft_app::updater::update_coordinator::{
    UpdateConfiguration, UpdateCoordinator, UpdatePillModel, UpdateReleaseNotesState,
};
use upleft_app::updater::update_engine::{FakeUpdateEngine, UpdateEngine, UpdateStartError};
use upleft_app::updater::update_metadata::{UpdateFailure, UpdateMetadata, Url};
use upleft_app::updater::update_state_machine::{UpdateEvent, UpdatePhase, UpdateStage, UpdateStateMachine};
use upleft_foundation::date::Date;

use super::{Failure, Request, json as dump_json};

pub fn run(request: &Request) -> Result<(), Failure> {
    let text = std::fs::read(&request.input)?;
    let root: Value = serde_json::from_slice(&text).map_err(|error| Failure::Error(error.to_string()))?;
    let Some(kind) = root["kind"].as_str() else {
        return Err(Failure::Error("updater case needs a `kind`".into()));
    };
    let directory = request.input.parent().map(|parent| parent.to_path_buf()).unwrap_or_default();
    let fixtures = Fixtures(root["metadata"].as_object().cloned().unwrap_or_default());
    let output = match kind {
        "state-machine-table" => state_machine_table(&root, &fixtures),
        "coordinator" => coordinator_scripts(&root, &fixtures),
        "coordinator-table" => coordinator_table(&root, &fixtures),
        "release-watch" => release_watch(&root, &fixtures),
        "feed-probe" => feed_probe(&root, &directory)?,
        "configuration" => configuration(&root),
        "failure" => failure(&root),
        "driver" => driver(&root, &fixtures),
        other => return Err(Failure::Error(format!("unknown updater case kind {other}"))),
    };
    Ok(dump_json::write(&output, &request.output)?)
}

// MARK: - Values

fn u64_value(value: &Value) -> u64 {
    match value {
        Value::String(string) => string.parse().expect("a UInt64 string"),
        other => other.as_u64().expect("a UInt64"),
    }
}

fn double_value(value: &Value) -> f64 {
    match value {
        Value::String(string) => match string.as_str() {
            "nan" => f64::NAN,
            "inf" => f64::INFINITY,
            "-inf" => f64::NEG_INFINITY,
            other => other.parse().expect("a Double string"),
        },
        other => other.as_f64().expect("a Double"),
    }
}

/// `(value as? NSNumber)?.boolValue ?? fallback`.
fn bool_value(value: &Value, fallback: bool) -> bool {
    match value {
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        _ => fallback,
    }
}

fn string_value(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

struct Fixtures(serde_json::Map<String, Value>);

impl Fixtures {
    /// A metadata value: the name of a fixture, or an inline object.
    fn metadata(&self, value: &Value) -> UpdateMetadata {
        let object = match value {
            Value::String(name) => &self.0[name.as_str()],
            other => other,
        };
        UpdateMetadata {
            version_string: string_value(&object["versionString"]).expect("versionString"),
            display_version_string: string_value(&object["displayVersionString"]).expect("displayVersionString"),
            title: string_value(&object["title"]),
            item_description: string_value(&object["itemDescription"]),
            release_notes_url: object["releaseNotesURL"].as_str().and_then(Url::from_string),
            info_url: object["infoURL"].as_str().and_then(Url::from_string),
            content_length: if object.get("contentLength").is_some() { u64_value(&object["contentLength"]) } else { 0 },
            is_information_only: bool_value(&object["isInformationOnly"], false),
            is_major_upgrade: bool_value(&object["isMajorUpgrade"], false),
            is_critical: bool_value(&object["isCritical"], false),
            minimum_system_version: string_value(&object["minimumSystemVersion"]),
        }
    }
}

fn stage(value: &Value) -> UpdateStage {
    match value.as_str().unwrap_or("notDownloaded") {
        "downloaded" => UpdateStage::Downloaded,
        "installing" => UpdateStage::Installing,
        _ => UpdateStage::NotDownloaded,
    }
}

fn failure_value(value: &Value) -> UpdateFailure {
    if value.as_str() == Some("generic") {
        return UpdateFailure::generic();
    }
    UpdateFailure::new(
        string_value(&value["message"]).expect("message"),
        string_value(&value["technicalDetail"]),
        value["code"].as_i64().expect("code") as isize,
        bool_value(&value["retryable"], true),
    )
}

/// A JSON value as `JSONSerialization` would hand it to Swift.
fn foundation_object(value: &Value) -> Option<Retained<AnyObject>> {
    let text = serde_json::to_vec(value).ok()?;
    NSJSONSerialization::JSONObjectWithData_options_error(&NSData::with_bytes(&text), NSJSONReadingOptions(0)).ok()
}

fn foundation_dictionary(value: &Value) -> Option<Retained<NSDictionary<NSString, AnyObject>>> {
    if !value.is_object() {
        return None;
    }
    let object = foundation_object(value)?;
    let dictionary = object.downcast::<NSDictionary>().ok()?;
    // SAFETY: a JSON object's keys are strings.
    Some(unsafe { Retained::cast_unchecked(dictionary) })
}

/// `NSError(domain:code:userInfo:)` from `{"domain", "code", "userInfo"}`.
fn error_value(value: &Value) -> Retained<NSError> {
    let domain = value["domain"].as_str().unwrap_or("sparkle");
    let code = value["code"].as_i64().unwrap_or(0) as isize;
    let user_info = foundation_dictionary(&value["userInfo"]);
    // SAFETY: a string-keyed user info dictionary, as Swift's `[String: Any]`.
    unsafe { NSError::errorWithDomain_code_userInfo(&NSString::from_str(domain), code, user_info.as_deref()) }
}

fn event(object: &Value, fixtures: &Fixtures) -> UpdateEvent {
    match object["event"].as_str().expect("event") {
        "userInitiatedCheckBegan" => UpdateEvent::UserInitiatedCheckBegan,
        "automaticCheckBegan" => UpdateEvent::AutomaticCheckBegan,
        "updateFound" => UpdateEvent::UpdateFound(fixtures.metadata(&object["metadata"]), stage(&object["stage"])),
        "releaseNotesAvailable" => UpdateEvent::ReleaseNotesAvailable,
        "releaseNotesFailed" => UpdateEvent::ReleaseNotesFailed,
        "updateNotFound" => UpdateEvent::UpdateNotFound { user_initiated: bool_value(&object["userInitiated"], false) },
        "updaterError" => UpdateEvent::UpdaterError(failure_value(&object["failure"])),
        "downloadInitiated" => UpdateEvent::DownloadInitiated,
        "expectedLength" => UpdateEvent::ExpectedLength(u64_value(&object["length"])),
        "dataReceived" => UpdateEvent::DataReceived(u64_value(&object["length"])),
        "extractionBegan" => UpdateEvent::ExtractionBegan,
        "extractionProgress" => UpdateEvent::ExtractionProgress(double_value(&object["progress"])),
        "readyToInstallAndRelaunch" => UpdateEvent::ReadyToInstallAndRelaunch,
        "installingUpdate" => {
            UpdateEvent::InstallingUpdate { application_terminated: bool_value(&object["applicationTerminated"], false) }
        }
        "updateInstalled" => UpdateEvent::UpdateInstalled { relaunched: bool_value(&object["relaunched"], false) },
        "dismissed" => UpdateEvent::Dismissed,
        "checkCancelled" => UpdateEvent::CheckCancelled,
        "downloadCancelled" => UpdateEvent::DownloadCancelled,
        other => panic!("unknown event {other}"),
    }
}

// MARK: - Output

fn opt(value: Option<String>) -> Value {
    value.map_or(Value::Null, Value::String)
}

fn double(value: f64) -> Value {
    dump_json::double(value)
}

fn metadata_json(metadata: Option<&UpdateMetadata>) -> Value {
    let Some(metadata) = metadata else {
        return Value::Null;
    };
    json!({
        "versionString": metadata.version_string,
        "displayVersionString": metadata.display_version_string,
        "title": opt(metadata.title.clone()),
        "itemDescription": opt(metadata.item_description.clone()),
        "releaseNotesURL": opt(metadata.release_notes_url.as_ref().map(Url::absolute_string)),
        "infoURL": opt(metadata.info_url.as_ref().map(Url::absolute_string)),
        "contentLength": metadata.content_length.to_string(),
        "isInformationOnly": metadata.is_information_only,
        "isMajorUpgrade": metadata.is_major_upgrade,
        "isCritical": metadata.is_critical,
        "minimumSystemVersion": opt(metadata.minimum_system_version.clone()),
    })
}

fn failure_json(failure: &UpdateFailure) -> Value {
    json!({
        "message": failure.message,
        "technicalDetail": opt(failure.technical_detail.clone()),
        "code": failure.code,
        "retryable": failure.retryable,
    })
}

fn stage_json(stage: UpdateStage) -> Value {
    Value::String(
        match stage {
            UpdateStage::NotDownloaded => "notDownloaded",
            UpdateStage::Downloaded => "downloaded",
            UpdateStage::Installing => "installing",
        }
        .into(),
    )
}

fn phase_json(phase: &UpdatePhase) -> Value {
    match phase {
        UpdatePhase::Idle => json!({"phase": "idle"}),
        UpdatePhase::Checking { user_initiated } => json!({"phase": "checking", "userInitiated": user_initiated}),
        UpdatePhase::Available(metadata, stage) => {
            json!({"phase": "available", "metadata": metadata_json(Some(metadata)), "stage": stage_json(*stage)})
        }
        UpdatePhase::Downloading { received, expected } => json!({
            "phase": "downloading",
            "received": received.to_string(),
            "expected": opt(expected.map(|expected| expected.to_string())),
        }),
        UpdatePhase::Extracting { progress } => {
            json!({"phase": "extracting", "progress": progress.map_or(Value::Null, double)})
        }
        UpdatePhase::ReadyToRelaunch => json!({"phase": "readyToRelaunch"}),
        UpdatePhase::WaitingForTermination => json!({"phase": "waitingForTermination"}),
        UpdatePhase::Installing => json!({"phase": "installing"}),
        UpdatePhase::Informational(metadata) => {
            json!({"phase": "informational", "metadata": metadata_json(Some(metadata))})
        }
        UpdatePhase::UpToDate => json!({"phase": "upToDate"}),
        UpdatePhase::Failed(failure, retryable) => {
            json!({"phase": "failed", "failure": failure_json(failure), "retryable": retryable})
        }
    }
}

fn pill_json(pill: Option<&UpdatePillModel>) -> Value {
    let Some(pill) = pill else {
        return Value::Null;
    };
    let mut value = match pill {
        UpdatePillModel::UpdateNow { version, is_ready } => {
            json!({"pill": "updateNow", "version": version, "isReady": is_ready})
        }
        UpdatePillModel::RestartToUpdate => json!({"pill": "restartToUpdate"}),
        UpdatePillModel::Progress(label, fraction) => {
            json!({"pill": "progress", "label": label, "fraction": fraction.map_or(Value::Null, double)})
        }
        UpdatePillModel::Warning => json!({"pill": "warning"}),
        UpdatePillModel::Informational(version) => json!({"pill": "informational", "version": version}),
    };
    value["offersInstall"] = Value::Bool(pill.offers_install());
    value
}

fn notes_json(notes: &UpdateReleaseNotesState) -> Value {
    match notes {
        UpdateReleaseNotesState::None => json!({"state": "none"}),
        UpdateReleaseNotesState::Loaded(data) => {
            let hex: String = data.iter().map(|byte| format!("{byte:02x}")).collect();
            json!({"state": "loaded", "hex": hex})
        }
        UpdateReleaseNotesState::Failed => json!({"state": "failed"}),
    }
}

fn probe_result_json(result: Option<&ReleaseFeedProbeResult>) -> Value {
    match result {
        None => Value::Null,
        Some(ReleaseFeedProbeResult::Unchanged) => json!({"result": "unchanged"}),
        Some(ReleaseFeedProbeResult::Changed { validator }) => {
            json!({"result": "changed", "validator": opt(validator.clone())})
        }
        Some(ReleaseFeedProbeResult::Unreachable) => json!({"result": "unreachable"}),
    }
}

fn choice_name(choice: UpdateUserChoice) -> &'static str {
    match choice {
        UpdateUserChoice::Install => "install",
        UpdateUserChoice::Later => "later",
        UpdateUserChoice::Skip => "skip",
    }
}

unsafe extern "C" {
    fn CFRunLoopRunInMode(mode: *const std::ffi::c_void, seconds: f64, return_after_source_handled: u8) -> i32;
    static kCFRunLoopDefaultMode: *const std::ffi::c_void;
}

/// Runs the main run loop until `done` holds (bounded).
fn pump(done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() && Instant::now() < deadline {
        // SAFETY: runs the current (main) thread's run loop briefly.
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.01, 1) };
    }
}

/// Lets every main-queue block enqueued so far run: the queue is FIFO, so a
/// sentinel enqueued now runs after all of them.
fn drain_main_queue() {
    let done = Arc::new(AtomicBool::new(false));
    let flag = done.clone();
    DispatchQueue::main().exec_async(move || flag.store(true, Ordering::SeqCst));
    pump(|| done.load(Ordering::SeqCst));
}

// MARK: - State machine table

fn state_machine_table(root: &Value, fixtures: &Fixtures) -> Value {
    let events = root["events"].as_array().expect("events");
    let mut states = Vec::new();
    for state in root["states"].as_array().expect("states") {
        let mut machine = UpdateStateMachine::new();
        for step in state["events"].as_array().expect("state events") {
            machine.reduce(&event(step, fixtures));
        }
        let rows: Vec<Value> = events
            .iter()
            .map(|step| {
                let mut next = machine.clone();
                next.reduce(&event(step, fixtures));
                json!({"phase": phase_json(next.phase()), "equalsBefore": next == machine})
            })
            .collect();
        states.push(json!({"state": state["name"], "phase": phase_json(machine.phase()), "rows": rows}));
    }
    json!({"states": states})
}

// MARK: - Coordinator

struct CoordinatorHarness {
    engine: Option<Rc<FakeUpdateEngine>>,
    coordinator: Rc<UpdateCoordinator>,
    effects: Rc<RefCell<Vec<String>>>,
    notifications: Rc<Cell<isize>>,
    token: RefCell<Option<Retained<objc2::runtime::ProtocolObject<dyn NSObjectProtocol>>>>,
}

impl CoordinatorHarness {
    /// `engine`: `"started"` (the tests' `makeCoordinator`), `"stopped"`, or `"none"`.
    fn new(mode: &str) -> CoordinatorHarness {
        let engine = (mode != "none").then(FakeUpdateEngine::new);
        let coordinator = UpdateCoordinator::new(engine.clone().map(|engine| engine as Rc<dyn UpdateEngine>));
        coordinator.set_suppress_ui_for_testing(true);
        if mode == "started"
            && let Some(engine) = &engine
        {
            let _ = engine.start();
        }
        let notifications = Rc::new(Cell::new(0));
        let sink = notifications.clone();
        let block = RcBlock::new(move |_notification: NonNull<NSNotification>| sink.set(sink.get() + 1));
        // SAFETY: `queue: nil`, so the block runs synchronously on the
        // posting (main) thread.
        let token = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(&NSString::from_str(UpdateCoordinator::STATE_DID_CHANGE)),
                Some(coordinator.notification_object()),
                None,
                &block,
            )
        };
        CoordinatorHarness {
            engine,
            coordinator,
            effects: Rc::new(RefCell::new(Vec::new())),
            notifications,
            token: RefCell::new(Some(token)),
        }
    }

    fn close(&self) {
        if let Some(token) = self.token.borrow_mut().take() {
            // SAFETY: the token this centre issued.
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(token.as_ref()) };
        }
    }

    fn callback(&self, label: String) -> Box<dyn FnOnce()> {
        let effects = Rc::downgrade(&self.effects);
        Box::new(move || {
            if let Some(effects) = effects.upgrade() {
                effects.borrow_mut().push(label);
            }
        })
    }

    fn reply(&self, label: String) -> Box<dyn FnOnce(UpdateUserChoice)> {
        let effects = Rc::downgrade(&self.effects);
        Box::new(move |choice| {
            if let Some(effects) = effects.upgrade() {
                effects.borrow_mut().push(format!("{label}:{}", choice_name(choice)));
            }
        })
    }

    /// Applies one step; answers a note when the step was skipped.
    fn apply(&self, step: &Value, index: usize, fixtures: &Fixtures) -> Option<String> {
        let c = &self.coordinator;
        match step["do"].as_str().expect("do") {
            "driverDidBeginUserCheck" => c.driver_did_begin_user_check(self.callback(format!("checkCancel#{index}"))),
            "driverDidFindUpdate" => c.driver_did_find_update(
                fixtures.metadata(&step["metadata"]),
                stage(&step["stage"]),
                bool_value(&step["userInitiated"], true),
                self.reply(format!("choiceReply#{index}")),
            ),
            "driverDidReceiveReleaseNotes" => {
                c.driver_did_receive_release_notes(step["text"].as_str().expect("text").as_bytes().to_vec())
            }
            "driverDidFailToDownloadReleaseNotes" => {
                c.driver_did_fail_to_download_release_notes(&error_value(&step["error"]))
            }
            "driverDidFindNoUpdate" => c.driver_did_find_no_update(
                bool_value(&step["userInitiated"], false),
                self.callback(format!("ack#{index}")),
            ),
            "driverDidEncounterError" => {
                c.driver_did_encounter_error(&error_value(&step["error"]), self.callback(format!("ack#{index}")))
            }
            "driverDidBeginDownload" => c.driver_did_begin_download(self.callback(format!("downloadCancel#{index}"))),
            "driverDidReceiveExpectedLength" => c.driver_did_receive_expected_length(u64_value(&step["length"])),
            "driverDidReceiveData" => c.driver_did_receive_data(u64_value(&step["length"])),
            "driverDidBeginExtraction" => c.driver_did_begin_extraction(),
            "driverDidReceiveExtractionProgress" => {
                c.driver_did_receive_extraction_progress(double_value(&step["progress"]))
            }
            "driverDidBecomeReadyToRelaunch" => {
                c.driver_did_become_ready_to_relaunch(self.reply(format!("readyReply#{index}")))
            }
            "driverDidBeginInstallation" => c.driver_did_begin_installation(
                bool_value(&step["applicationTerminated"], false),
                self.callback(format!("retryTermination#{index}")),
            ),
            "driverDidFinishInstallation" => c.driver_did_finish_installation(
                bool_value(&step["relaunched"], true),
                self.callback(format!("ack#{index}")),
            ),
            "driverDidDismiss" => c.driver_did_dismiss(),
            "driverDidRequestFocus" => c.driver_did_request_focus(),
            "userDidPressUpdateNow" => {
                if matches!(c.phase(), UpdatePhase::Informational(_)) {
                    return Some("skipped: opens the info URL or beeps".into());
                }
                c.user_did_press_update_now();
            }
            "userDidChooseInstall" => c.user_did_choose_install(),
            "userDidRetry" => c.user_did_retry(),
            "userDidChooseLater" => c.user_did_choose_later(),
            "userDidChooseSkip" => c.user_did_choose_skip(),
            "userDidCancelCheck" => c.user_did_cancel_check(),
            "userDidCancelDownload" => c.user_did_cancel_download(),
            "userDidRetryTermination" => c.user_did_retry_termination(),
            "userDidAcknowledge" => c.user_did_acknowledge(),
            "userDidDismissPanel" => c.user_did_dismiss_panel(),
            "checkForUpdates" => c.check_for_updates(),
            "showPanel" => c.show_panel(),
            "closePanel" => c.close_panel(),
            "start" => c.start(),
            "tearDownForTesting" => c.tear_down_for_testing(),
            "releaseFeedDidChange" => c.release_feed_did_change(),
            "setAutomaticallyChecksForUpdates" => c.set_automatically_checks_for_updates(bool_value(&step["value"], true)),
            "setAutomaticallyDownloadsUpdates" => c.set_automatically_downloads_updates(bool_value(&step["value"], true)),
            "completeBackgroundDownload" => {
                if let Some(engine) = &self.engine {
                    engine.complete_background_download(step["version"].as_str().expect("version"));
                }
            }
            "engineStart" => {
                if let Some(engine) = &self.engine
                    && let Err(error) = engine.start()
                {
                    self.effects.borrow_mut().push(format!("engineStartThrew:{}:{}", error.domain(), error.code()));
                }
            }
            "engineSet" => {
                let Some(engine) = &self.engine else {
                    return Some("skipped: no engine".into());
                };
                let values = &step["values"];
                let has = |key: &str| values.get(key).is_some();
                if has("startThrows") {
                    engine.start_throws.set(bool_value(&values["startThrows"], false));
                }
                if has("canCheckForUpdates") {
                    engine._can_check_for_updates.set(bool_value(&values["canCheckForUpdates"], true));
                }
                if has("automaticallyChecksForUpdates") {
                    engine.automatically_checks_for_updates.set(bool_value(&values["automaticallyChecksForUpdates"], true));
                }
                if has("automaticallyDownloadsUpdates") {
                    engine.automatically_downloads_updates.set(bool_value(&values["automaticallyDownloadsUpdates"], true));
                }
                if has("allowsAutomaticUpdates") {
                    engine.allows_automatic_updates.set(bool_value(&values["allowsAutomaticUpdates"], true));
                }
                if has("updateCheckInterval") {
                    engine.update_check_interval.set(double_value(&values["updateCheckInterval"]));
                }
                if has("lastUpdateCheckDate") {
                    let value = &values["lastUpdateCheckDate"];
                    engine
                        .last_update_check_date
                        .set((!value.is_null()).then(|| Date::from_reference(double_value(value))));
                }
            }
            "drain" => drain_main_queue(),
            other => panic!("unknown coordinator step {other}"),
        }
        None
    }

    /// Everything observable about the coordinator, plus the effects and
    /// notifications since the last snapshot.
    fn snapshot(&self) -> Value {
        let c = &self.coordinator;
        let mut value = json!({
            "phase": phase_json(&c.phase()),
            "pill": pill_json(c.pill_model().as_ref()),
            "pendingUpdate": metadata_json(c.pending_update().as_ref()),
            "downloadedUpdate": metadata_json(c.downloaded_update().as_ref()),
            "currentCycleUpdate": metadata_json(c.current_cycle_update().as_ref()),
            "currentCycleIsUserInitiated": c.current_cycle_is_user_initiated(),
            "isExpeditedInstall": c.is_expedited_install(),
            "panelShowCount": c.panel_show_count(),
            "releaseWatchTriggerCount": c.release_watch_trigger_count(),
            "releaseNotes": notes_json(&c.release_notes()),
            "isRunning": c.is_running(),
            "canCheckForUpdates": c.can_check_for_updates(),
            "isUpdateConfigurationPresent": c.is_update_configuration_present(),
            "automaticallyChecksForUpdates": c.automatically_checks_for_updates(),
            "automaticallyDownloadsUpdates": c.automatically_downloads_updates(),
            "allowsAutomaticUpdates": c.allows_automatic_updates(),
            "lastUpdateCheckDate": c.last_update_check_date().map_or(Value::Null, |date| double(date.time_interval_since_reference_date)),
            "statusLine": c.status_line(),
        });
        value["engine"] = match &self.engine {
            Some(engine) => json!({
                "isRunning": engine.is_running(),
                "foregroundCheckCount": engine.foreground_check_count(),
                "backgroundCheckCount": engine.background_check_count(),
                "automaticallyChecksForUpdates": engine.automatically_checks_for_updates.get(),
                "automaticallyDownloadsUpdates": engine.automatically_downloads_updates.get(),
                "hasBackgroundHandler": engine.on_background_download_completed.borrow().is_some(),
            }),
            None => Value::Null,
        };
        value["notifications"] = json!(self.notifications.replace(0));
        value["effects"] = json!(std::mem::take(&mut *self.effects.borrow_mut()));
        value
    }

    /// Runs `steps`, answering one entry (with a snapshot) per step.
    fn run(&self, steps: &[Value], start: usize, fixtures: &Fixtures) -> Vec<Value> {
        steps
            .iter()
            .enumerate()
            .map(|(offset, step)| {
                let note = self.apply(step, start + offset, fixtures);
                let mut entry = json!({"do": step["do"]});
                if let Some(note) = note {
                    entry["note"] = Value::String(note);
                }
                entry["after"] = self.snapshot();
                entry
            })
            .collect()
    }
}

fn coordinator_scripts(root: &Value, fixtures: &Fixtures) -> Value {
    let mut scripts = Vec::new();
    for script in root["scripts"].as_array().expect("scripts") {
        let harness = CoordinatorHarness::new(script["engine"].as_str().unwrap_or("started"));
        let initial = harness.snapshot();
        let steps = harness.run(script["steps"].as_array().expect("steps"), 0, fixtures);
        harness.close();
        scripts.push(json!({"name": script["name"], "initial": initial, "steps": steps}));
    }
    json!({"scripts": scripts})
}

/// Each setup once with a snapshot per step, then every action from a fresh
/// coordinator brought to that setup.
fn coordinator_table(root: &Value, fixtures: &Fixtures) -> Value {
    let actions = root["actions"].as_array().expect("actions");
    let mut setups = Vec::new();
    for setup in root["setups"].as_array().expect("setups") {
        let engine = setup["engine"].as_str().unwrap_or("started");
        let steps = setup["steps"].as_array().expect("steps");
        let reference = CoordinatorHarness::new(engine);
        let initial = reference.snapshot();
        let setup_steps = reference.run(steps, 0, fixtures);
        reference.close();
        let mut rows = Vec::new();
        for action in actions {
            let harness = CoordinatorHarness::new(engine);
            let _ = harness.run(steps, 0, fixtures);
            rows.push(harness.run(std::slice::from_ref(action), steps.len(), fixtures).remove(0));
            harness.close();
        }
        setups.push(json!({"setup": setup["name"], "initial": initial, "steps": setup_steps, "rows": rows}));
    }
    json!({"setups": setups})
}

// MARK: - Release watch

struct ScriptedFeedProbe {
    results: RefCell<Vec<ReleaseFeedProbeResult>>,
    validators: RefCell<Vec<Option<String>>>,
}

impl ReleaseFeedProbe for ScriptedFeedProbe {
    fn probe(&self, _feed: &Url, validator: Option<&str>, completion: Box<dyn FnOnce(ReleaseFeedProbeResult)>) {
        self.validators.borrow_mut().push(validator.map(str::to_owned));
        let next = {
            let mut results = self.results.borrow_mut();
            if results.is_empty() { ReleaseFeedProbeResult::Unchanged } else { results.remove(0) }
        };
        // Swift's scripted probe returns without suspending.
        completion(next);
    }
}

fn probe_result(object: &Value) -> ReleaseFeedProbeResult {
    match object["result"].as_str().expect("result") {
        "unchanged" => ReleaseFeedProbeResult::Unchanged,
        "changed" => ReleaseFeedProbeResult::Changed { validator: string_value(&object["validator"]) },
        _ => ReleaseFeedProbeResult::Unreachable,
    }
}

fn policy_json(policy: &ReleaseWatchPolicy) -> Value {
    json!({
        "isAppActive": policy.is_app_active,
        "isLowPower": policy.is_low_power,
        "hasNetwork": policy.has_network,
        "interval": policy.interval().map_or(Value::Null, double),
    })
}

fn release_watch(root: &Value, fixtures: &Fixtures) -> Value {
    let mut policies = Vec::new();
    for is_app_active in [false, true] {
        for is_low_power in [false, true] {
            for has_network in [false, true] {
                policies.push(policy_json(&ReleaseWatchPolicy { is_app_active, is_low_power, has_network }));
            }
        }
    }
    let constants = json!({
        "activeInterval": double(ReleaseWatchPolicy::ACTIVE_INTERVAL),
        "backgroundInterval": double(ReleaseWatchPolicy::BACKGROUND_INTERVAL),
        "minimumSpacing": double(ReleaseWatchPolicy::MINIMUM_SPACING),
        "default": policy_json(&ReleaseWatchPolicy::default()),
    });

    let mut scripts = Vec::new();
    let empty = Vec::new();
    for script in root["scripts"].as_array().unwrap_or(&empty) {
        let results = script["results"].as_array().unwrap_or(&empty).iter().map(probe_result).collect();
        let probe = Rc::new(ScriptedFeedProbe { results: RefCell::new(results), validators: RefCell::new(Vec::new()) });
        let feed = Url::from_string("https://feeds.example.invalid/appcast.xml").expect("feed URL");
        let watch = ReleaseWatch::new(feed, Some(probe.clone() as Rc<dyn ReleaseFeedProbe>));
        let fired = Rc::new(Cell::new(0isize));
        let sink = fired.clone();
        watch.set_on_feed_changed(Some(Rc::new(move || sink.set(sink.get() + 1))));
        let mut harness: Option<Rc<CoordinatorHarness>> = None;
        let mut entries = Vec::new();
        for (index, step) in script["steps"].as_array().expect("steps").iter().enumerate() {
            let mut note = None;
            match step["do"].as_str().expect("do") {
                "start" => watch.start(false),
                "stop" => watch.stop(),
                "probeNow" => watch.probe_now(),
                "systemDidWake" => watch.system_did_wake(),
                "activation" => watch.application_did_change_activation(bool_value(&step["isActive"], true)),
                "powerState" => watch.power_state_did_change(bool_value(&step["isLowPower"], false)),
                "network" => watch.network_availability_did_change(bool_value(&step["hasNetwork"], true)),
                "drain" => drain_main_queue(),
                "wireCoordinator" => {
                    // The coordinator's own wiring: the watch asks Sparkle to look.
                    let wired = Rc::new(CoordinatorHarness::new(step["engine"].as_str().unwrap_or("started")));
                    let weak = Rc::downgrade(&wired);
                    harness = Some(wired);
                    let sink = fired.clone();
                    watch.set_on_feed_changed(Some(Rc::new(move || {
                        sink.set(sink.get() + 1);
                        if let Some(wired) = weak.upgrade() {
                            wired.coordinator.release_feed_did_change();
                        }
                    })));
                }
                "coordinator" => {
                    note = match &harness {
                        Some(harness) => harness.apply(&step["step"], index, fixtures),
                        None => Some("skipped: no coordinator".into()),
                    };
                }
                other => panic!("unknown release-watch step {other}"),
            }
            let mut entry = json!({"do": step["do"]});
            if let Some(note) = note {
                entry["note"] = Value::String(note);
            }
            entry["completedProbeCount"] = json!(watch.completed_probe_count());
            entry["lastResult"] = probe_result_json(watch.last_result().as_ref());
            entry["hasBaseline"] = json!(watch.has_baseline());
            entry["policy"] = policy_json(&watch.policy());
            entry["fired"] = json!(fired.get());
            entry["validators"] = json!(probe.validators.borrow().iter().cloned().map(opt).collect::<Vec<_>>());
            if let Some(harness) = &harness {
                entry["coordinator"] = harness.snapshot();
            }
            entries.push(entry);
        }
        watch.stop();
        if let Some(harness) = &harness {
            harness.close();
        }
        scripts.push(json!({"name": script["name"], "steps": entries}));
    }
    json!({"constants": constants, "policies": policies, "scripts": scripts})
}

// MARK: - Feed probe

/// What the stub answers for one request.
struct StubSpec {
    kind: String,
    status: isize,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

static STUB_SPEC: Mutex<Option<StubSpec>> = Mutex::new(None);
static STUB_SEEN: Mutex<Vec<Value>> = Mutex::new(Vec::new());

fn header(request: &NSURLRequest, field: &str) -> Value {
    opt(request.valueForHTTPHeaderField(&NSString::from_str(field)).map(|value| value.to_string()))
}

define_class!(
    // SAFETY: an NSURLProtocol subclass overriding the four methods below;
    // it keeps no state of its own.
    #[unsafe(super(NSURLProtocol))]
    #[thread_kind = AllocAnyThread]
    #[name = "UpleftUpdaterDumpStubURLProtocol"]
    struct UpdaterStubURLProtocol;

    unsafe impl NSObjectProtocol for UpdaterStubURLProtocol {}

    impl UpdaterStubURLProtocol {
        #[unsafe(method(canInitWithRequest:))]
        fn can_init_with_request(_request: &NSURLRequest) -> bool {
            true
        }

        #[unsafe(method(canonicalRequestForRequest:))]
        fn canonical_request_for_request(request: &NSURLRequest) -> *mut NSURLRequest {
            Retained::autorelease_return(request.retain())
        }

        #[unsafe(method(startLoading))]
        fn start_loading(&self) {
            let request = self.request();
            STUB_SEEN.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(json!({
                "method": opt(request.HTTPMethod().map(|method| method.to_string())),
                "ifNoneMatch": header(&request, "If-None-Match"),
                "ifModifiedSince": header(&request, "If-Modified-Since"),
                "cachePolicy": request.cachePolicy().0,
            }));
            let Some(client) = self.client() else { return };
            let spec = STUB_SPEC.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let (Some(spec), Some(url)) = (spec.as_ref(), request.URL()) else {
                let error = NSError::new(-1011, &NSString::from_str("NSURLErrorDomain")); // .badServerResponse
                client.URLProtocol_didFailWithError(self, &error);
                return;
            };
            match spec.kind.as_str() {
                "error" => {
                    let error = NSError::new(-1009, &NSString::from_str("NSURLErrorDomain")); // .notConnectedToInternet
                    client.URLProtocol_didFailWithError(self, &error);
                    return;
                }
                "nonHTTP" => {
                    let response = NSURLResponse::initWithURL_MIMEType_expectedContentLength_textEncodingName(
                        NSURLResponse::alloc(),
                        &url,
                        Some(&NSString::from_str("application/xml")),
                        spec.body.len() as isize,
                        None,
                    );
                    client.URLProtocol_didReceiveResponse_cacheStoragePolicy(self, &response, NSURLCacheStoragePolicy::NotAllowed);
                }
                _ => {
                    let keys: Vec<Retained<NSString>> = spec.headers.iter().map(|(key, _)| NSString::from_str(key)).collect();
                    let values: Vec<Retained<NSString>> =
                        spec.headers.iter().map(|(_, value)| NSString::from_str(value)).collect();
                    let key_refs: Vec<&NSString> = keys.iter().map(|key| &**key).collect();
                    let fields = NSDictionary::from_retained_objects(&key_refs, &values);
                    let response = NSHTTPURLResponse::initWithURL_statusCode_HTTPVersion_headerFields(
                        NSHTTPURLResponse::alloc(),
                        &url,
                        spec.status,
                        Some(&NSString::from_str("HTTP/1.1")),
                        Some(&fields),
                    )
                    .expect("an HTTP response");
                    client.URLProtocol_didReceiveResponse_cacheStoragePolicy(self, &response, NSURLCacheStoragePolicy::NotAllowed);
                }
            }
            client.URLProtocol_didLoadData(self, &NSData::with_bytes(&spec.body));
            client.URLProtocolDidFinishLoading(self);
        }

        #[unsafe(method(stopLoading))]
        fn stop_loading(&self) {}
    }
);

fn body(value: &Value, feed: &[u8]) -> Vec<u8> {
    match value {
        Value::String(name) => match name.as_str() {
            "feed" => feed.to_vec(),
            "empty" => Vec::new(),
            other => panic!("unknown body {other}"),
        },
        Value::Object(object) => {
            if let Some(text) = object.get("text").and_then(Value::as_str) {
                return text.as_bytes().to_vec();
            }
            if let Some(repeated) = object.get("repeat") {
                let byte = repeated["byte"].as_u64().expect("byte") as u8;
                return vec![byte; repeated["count"].as_u64().expect("count") as usize];
            }
            feed.to_vec()
        }
        _ => feed.to_vec(),
    }
}

fn feed_probe(root: &Value, directory: &std::path::Path) -> Result<Value, Failure> {
    let configuration = NSURLSessionConfiguration::ephemeralSessionConfiguration();
    let classes: Retained<NSArray<AnyClass>> = NSArray::from_slice(&[UpdaterStubURLProtocol::class()]);
    // SAFETY: an array of `NSURLProtocol` subclasses.
    unsafe { configuration.setProtocolClasses(Some(&classes)) };
    let session = NSURLSession::sessionWithConfiguration(&configuration);
    let feed_url = Url::from_string("https://feeds.example.invalid/appcast.xml").expect("feed URL");
    let scenarios = root["scenarios"].as_array().expect("scenarios");

    let mut feeds = Vec::new();
    for path in root["feeds"].as_array().expect("feeds") {
        let path = path.as_str().expect("feed path");
        let feed = std::fs::read(directory.join(path))?;
        let mut results = Vec::new();
        for scenario in scenarios {
            let probe = ReleaseFeedURLProbe::with_session(session.clone());
            let mut held: Option<String> = None;
            let mut probes = Vec::new();
            for step in scenario["probes"].as_array().expect("probes") {
                let response = &step["response"];
                let headers = response["headers"]
                    .as_object()
                    .map(|headers| {
                        headers
                            .iter()
                            .map(|(key, value)| (key.clone(), value.as_str().expect("header value").to_owned()))
                            .collect()
                    })
                    .unwrap_or_default();
                *STUB_SPEC.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(StubSpec {
                    kind: response["kind"].as_str().unwrap_or("http").to_owned(),
                    status: response["status"].as_i64().unwrap_or(200) as isize,
                    headers,
                    body: body(&response["body"], &feed),
                });
                STUB_SEEN.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
                let offered = match &step["offer"] {
                    Value::String(string) => Some(string.clone()),
                    Value::Object(object) if object.contains_key("held") => held.clone(),
                    _ => None,
                };
                let answer: Rc<RefCell<Option<ReleaseFeedProbeResult>>> = Rc::new(RefCell::new(None));
                let sink = answer.clone();
                probe.probe(&feed_url, offered.as_deref(), Box::new(move |result| *sink.borrow_mut() = Some(result)));
                pump(|| answer.borrow().is_some());
                let answer = answer.borrow_mut().take();
                if let Some(ReleaseFeedProbeResult::Changed { validator }) = &answer {
                    held = validator.clone();
                }
                let seen = std::mem::take(&mut *STUB_SEEN.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
                probes.push(json!({
                    "offered": opt(offered),
                    "result": probe_result_json(answer.as_ref()),
                    "requests": seen,
                }));
            }
            results.push(json!({"scenario": scenario["name"], "probes": probes}));
        }
        feeds.push(json!({"feed": path, "bytes": feed.len(), "scenarios": results}));
    }
    Ok(json!({"feeds": feeds}))
}

// MARK: - Configuration and failures

fn configuration(root: &Value) -> Value {
    let cases: Vec<Value> = root["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .map(|entry| {
            let info = foundation_dictionary(&entry["info"]);
            // `entry["info"] as? [String: Any] ?? [:]`: an empty dictionary is invalid too.
            json!({"name": entry["name"], "isValid": UpdateConfiguration::is_valid(info.as_deref())})
        })
        .collect();
    json!({"cases": cases})
}

fn failure(root: &Value) -> Value {
    let mut entries = vec![
        json!({"name": "generic", "failure": failure_json(&UpdateFailure::generic())}),
        json!({
            "name": "UpdateStartError.updaterRefusedToStart",
            "failure": failure_json(&UpdateFailure::from_error(&UpdateStartError::UpdaterRefusedToStart.to_ns_error())),
        }),
    ];
    for entry in root["errors"].as_array().expect("errors") {
        let failure = UpdateFailure::from_error(&error_value(&entry["error"]));
        let mut machine = UpdateStateMachine::new();
        machine.reduce(&UpdateEvent::UpdaterError(failure.clone()));
        entries.push(json!({"name": entry["name"], "failure": failure_json(&failure), "phase": phase_json(machine.phase())}));
    }
    json!({"failures": entries})
}

// MARK: - Driver

fn driver(root: &Value, fixtures: &Fixtures) -> Value {
    let mut scripts = Vec::new();
    for script in root["scripts"].as_array().expect("scripts") {
        let harness = CoordinatorHarness::new(script["engine"].as_str().unwrap_or("started"));
        let host: Rc<dyn UpdateDriverHost> = harness.coordinator.clone();
        let driver = DownrightUpdateDriver::new(Some(Rc::downgrade(&host)));
        let mut entries = Vec::new();
        for (index, step) in script["steps"].as_array().expect("steps").iter().enumerate() {
            let mut note = None;
            let effects = Rc::downgrade(&harness.effects);
            let sparkle_reply = Box::new(move |choice: SpuUserUpdateChoice| {
                if let Some(effects) = effects.upgrade() {
                    effects.borrow_mut().push(format!("sparkleReply#{index}:{}", choice as isize));
                }
            });
            match step["do"].as_str().expect("do") {
                "permissionRequest" => {
                    let effects = Rc::downgrade(&harness.effects);
                    driver.show_update_permission_request(
                        &SpuUpdatePermissionRequest::default(),
                        Box::new(move |response| {
                            let downloading = match response.automatic_update_downloading {
                                Some(true) => "true",
                                Some(false) => "false",
                                None => "nil",
                            };
                            if let Some(effects) = effects.upgrade() {
                                effects.borrow_mut().push(format!(
                                    "permission#{index}:checks={},downloading={downloading},profile={}",
                                    response.automatic_update_checks, response.send_system_profile
                                ));
                            }
                        }),
                    );
                }
                "showUserInitiatedUpdateCheck" => {
                    driver.show_user_initiated_update_check(harness.callback(format!("sparkleCancel#{index}")))
                }
                "showUpdateReleaseNotesFailedToDownloadWithError" => {
                    driver.show_update_release_notes_failed_to_download_with_error(&error_value(&step["error"]))
                }
                "showUpdateNotFoundWithError" => driver.show_update_not_found_with_error(
                    &error_value(&step["error"]),
                    harness.callback(format!("sparkleAck#{index}")),
                ),
                "showUpdaterError" => {
                    driver.show_updater_error(&error_value(&step["error"]), harness.callback(format!("sparkleAck#{index}")))
                }
                "showDownloadInitiated" => {
                    driver.show_download_initiated(harness.callback(format!("sparkleCancel#{index}")))
                }
                "showDownloadDidReceiveExpectedContentLength" => {
                    driver.show_download_did_receive_expected_content_length(u64_value(&step["length"]))
                }
                "showDownloadDidReceiveData" => driver.show_download_did_receive_data(u64_value(&step["length"])),
                "showDownloadDidStartExtractingUpdate" => driver.show_download_did_start_extracting_update(),
                "showExtractionReceivedProgress" => {
                    driver.show_extraction_received_progress(double_value(&step["progress"]))
                }
                "showReadyToInstallAndRelaunch" => driver.show_ready_to_install_and_relaunch(sparkle_reply),
                "showInstallingUpdate" => driver.show_installing_update(
                    bool_value(&step["applicationTerminated"], false),
                    harness.callback(format!("sparkleRetry#{index}")),
                ),
                "showUpdateInstalledAndRelaunched" => driver.show_update_installed_and_relaunched(
                    bool_value(&step["relaunched"], true),
                    harness.callback(format!("sparkleAck#{index}")),
                ),
                "dismissUpdateInstallation" => driver.dismiss_update_installation(),
                "showUpdateInFocus" => driver.show_update_in_focus(),
                "detachHost" => driver.set_host(None),
                "coordinator" => note = harness.apply(&step["step"], index, fixtures),
                other => panic!("unknown driver step {other}"),
            }
            let mut entry = json!({"do": step["do"]});
            if let Some(note) = note {
                entry["note"] = Value::String(note);
            }
            entry["after"] = harness.snapshot();
            entries.push(entry);
        }
        harness.close();
        scripts.push(json!({"name": script["name"], "steps": entries}));
    }
    json!({"scripts": scripts})
}
