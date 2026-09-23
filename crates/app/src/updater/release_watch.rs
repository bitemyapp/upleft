//! Port of `Sources/DownrightApp/Updater/ReleaseWatch.swift`.
//!
//! Threading follows the Swift: the watch lives on the main thread, every
//! probe starts on a later main-queue turn (Swift's `Task { @MainActor in … }`),
//! the schedule is `DispatchQueue.main.asyncAfter`, and the production probe's
//! request runs on `URLSession`'s own queue. One difference, stricter than
//! Swift: the probe classifies the response and hashes the body on the
//! session's delegate queue and hops to the main thread with the answer,
//! where Swift hashes after resuming on the main actor (see
//! `docs/KNOWN-DIFFERENCES.md`).

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::{Rc, Weak};
use std::sync::Mutex;

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSApplication, NSApplicationDidBecomeActiveNotification, NSApplicationDidResignActiveNotification,
    NSWorkspace, NSWorkspaceDidWakeNotification,
};
use objc2_foundation::{
    NSData, NSError, NSHTTPCookieAcceptPolicy, NSHTTPURLResponse, NSMutableURLRequest, NSNotification,
    NSNotificationCenter, NSObjectProtocol, NSOperationQueue, NSProcessInfo, NSProcessInfoPowerStateDidChangeNotification,
    NSString, NSURLRequestCachePolicy, NSURLResponse, NSURLSession, NSURLSessionConfiguration,
};
use sha2::{Digest, Sha256};
use upleft_foundation::date::Date;

use super::main_actor::{MainOnly, WorkItem, async_after, async_main};
use super::update_metadata::{Url, optional_str_eq};

/// What one conditional probe of the appcast learned.
#[derive(Clone, Debug)]
pub enum ReleaseFeedProbeResult {
    /// Byte-identical to the last feed this session saw.
    Unchanged,
    /// The feed moved. `validator` is the token to send back next time.
    Changed { validator: Option<String> },
    /// The probe could not complete. Never surfaced: a laptop opened without
    /// wifi is not an update failure, and `UpdateCoordinator` already refuses
    /// to raise that alarm for background cycles.
    Unreachable,
}

impl PartialEq for ReleaseFeedProbeResult {
    fn eq(&self, other: &ReleaseFeedProbeResult) -> bool {
        match (self, other) {
            (ReleaseFeedProbeResult::Unchanged, ReleaseFeedProbeResult::Unchanged) => true,
            (ReleaseFeedProbeResult::Changed { validator: a }, ReleaseFeedProbeResult::Changed { validator: b }) => {
                optional_str_eq(a, b)
            }
            (ReleaseFeedProbeResult::Unreachable, ReleaseFeedProbeResult::Unreachable) => true,
            _ => false,
        }
    }
}

/// One conditional GET. A protocol so the watch's whole schedule can be
/// tested without a network, a server, or a wall clock.
///
/// Swift's requirement is `func probe(feed: URL, validator: String?) async ->
/// ReleaseFeedProbeResult` on the main actor. The port hands the answer to
/// `completion`, on the main thread: synchronously when the Swift conformer
/// would return without suspending (a fake), or on a later main-queue turn
/// when it would suspend (the network probe).
pub trait ReleaseFeedProbe {
    fn probe(&self, feed: &Url, validator: Option<&str>, completion: Box<dyn FnOnce(ReleaseFeedProbeResult)>);
}

// MARK: - Policy

/// When the watch may probe, and how often.
///
/// Pure policy, deliberately kept apart from the timer that obeys it: every
/// rule here is then a value comparison in a test rather than a wait.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReleaseWatchPolicy {
    pub is_app_active: bool,
    pub is_low_power: bool,
    pub has_network: bool,
}

impl Default for ReleaseWatchPolicy {
    fn default() -> ReleaseWatchPolicy {
        ReleaseWatchPolicy { is_app_active: false, is_low_power: false, has_network: true }
    }
}

impl ReleaseWatchPolicy {
    /// Frontmost and in use. A build that lands while the reader is sitting
    /// here should be offered while they are still sitting here.
    pub const ACTIVE_INTERVAL: f64 = 90.0;
    /// Running, but the reader is in another app. Still worth knowing before
    /// they come back; not worth waking the radio on their behalf.
    pub const BACKGROUND_INTERVAL: f64 = 15.0 * 60.0;
    /// The floor between two probes, however many events coincide. Wake fires
    /// activate as well, and a held Cmd-Tab flaps activation several times a
    /// second; without a floor each of those becomes its own request.
    pub const MINIMUM_SPACING: f64 = 20.0;

    /// `None` means "do not probe at all right now".
    pub fn interval(&self) -> Option<f64> {
        if !self.has_network {
            return None;
        }
        // Low Power Mode is an explicit request to stop doing optional work.
        // Polling for a build the reader has not asked for is exactly that, so
        // the background cadence stops entirely and the foreground one drops
        // to it — Sparkle's own hourly schedule still covers the app.
        if self.is_low_power {
            return if self.is_app_active { Some(Self::BACKGROUND_INTERVAL) } else { None };
        }
        Some(if self.is_app_active { Self::ACTIVE_INTERVAL } else { Self::BACKGROUND_INTERVAL })
    }
}

// MARK: - Watch

/// Why a probe is happening. Only `Scheduled` is exempt from the spacing
/// floor, because the schedule already spaces itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProbeReason {
    Scheduled,
    Event,
}

type Observer = (Retained<NSNotificationCenter>, Retained<ProtocolObject<dyn NSObjectProtocol>>);

/// Notices that the appcast moved, so a build published while the app is open
/// is offered in about a minute instead of on Sparkle's next scheduled check.
///
/// The important line, and the reason this can be as cheap as it is: **the
/// watch is a trigger, not a trust path.** It learns exactly one bit — the
/// feed is not the one we last saw — and hands that to Sparkle. It never
/// parses the appcast, never compares versions, never reads an enclosure URL,
/// and cannot cause an install. Every signature check, ordering decision, and
/// download stays inside Sparkle, so shortening the *latency* of an update
/// leaves the *security model* of one untouched.
pub struct ReleaseWatch {
    weak_self: Weak<ReleaseWatch>,

    /// Called when the feed has moved since the baseline. Never called for
    /// the probe that establishes the baseline.
    on_feed_changed: RefCell<Option<Rc<dyn Fn()>>>,

    /// Test seam: how many probes have completed, and what the last one said.
    completed_probe_count: Cell<isize>,
    last_result: RefCell<Option<ReleaseFeedProbeResult>>,
    has_baseline: Cell<bool>,

    feed: Url,
    prober: Rc<dyn ReleaseFeedProbe>,
    policy: Cell<ReleaseWatchPolicy>,

    is_running: Cell<bool>,
    is_probing: Cell<bool>,
    validator: RefCell<Option<String>>,
    last_probe_date: Cell<Option<Date>>,
    pending: RefCell<Option<WorkItem>>,
    /// Tokens are paired with the centre that issued them: the wake
    /// notification comes from the workspace centre, and handing its token to
    /// the default centre to remove is a silent no-op that leaks the observer.
    observers: RefCell<Vec<Observer>>,
    path_monitor: RefCell<Option<PathMonitor>>,
}

impl ReleaseWatch {
    /// `init(feed:prober:)`. With no prober, the production
    /// [`ReleaseFeedURLProbe`] is used.
    pub fn new(feed: Url, prober: Option<Rc<dyn ReleaseFeedProbe>>) -> Rc<ReleaseWatch> {
        let prober = prober.unwrap_or_else(|| Rc::new(ReleaseFeedURLProbe::new()));
        Rc::new_cyclic(|weak_self| ReleaseWatch {
            weak_self: weak_self.clone(),
            on_feed_changed: RefCell::new(None),
            completed_probe_count: Cell::new(0),
            last_result: RefCell::new(None),
            has_baseline: Cell::new(false),
            feed,
            prober,
            policy: Cell::new(ReleaseWatchPolicy::default()),
            is_running: Cell::new(false),
            is_probing: Cell::new(false),
            validator: RefCell::new(None),
            last_probe_date: Cell::new(None),
            pending: RefCell::new(None),
            observers: RefCell::new(Vec::new()),
            path_monitor: RefCell::new(None),
        })
    }

    pub fn on_feed_changed(&self) -> Option<Rc<dyn Fn()>> {
        self.on_feed_changed.borrow().clone()
    }

    pub fn set_on_feed_changed(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.on_feed_changed.borrow_mut() = handler;
    }

    pub fn completed_probe_count(&self) -> isize {
        self.completed_probe_count.get()
    }

    pub fn last_result(&self) -> Option<ReleaseFeedProbeResult> {
        self.last_result.borrow().clone()
    }

    pub fn has_baseline(&self) -> bool {
        self.has_baseline.get()
    }

    pub fn policy(&self) -> ReleaseWatchPolicy {
        self.policy.get()
    }

    // MARK: Lifecycle

    /// Begins watching. Safe to call twice; the second call is a no-op.
    pub fn start(&self, observing_system_events: bool) {
        if self.is_running.get() {
            return;
        }
        self.is_running.set(true);
        let mut policy = self.policy.get();
        policy.is_app_active = ns_app_is_active();
        policy.is_low_power = NSProcessInfo::processInfo().isLowPowerModeEnabled();
        self.policy.set(policy);
        if observing_system_events {
            self.observe_system_events();
        }
        // The first probe establishes the baseline rather than firing, so it
        // can go out immediately without racing Sparkle's post-launch check.
        self.probe(ProbeReason::Event);
    }

    pub fn stop(&self) {
        self.is_running.set(false);
        let pending = self.pending.borrow_mut().take();
        if let Some(pending) = pending {
            pending.cancel();
        }
        let monitor = self.path_monitor.borrow_mut().take();
        if let Some(monitor) = monitor {
            monitor.cancel();
        }
        let observers = std::mem::take(&mut *self.observers.borrow_mut());
        remove_observers(observers);
    }

    // MARK: Inputs (also the test seam — every one of these is callable directly)

    pub fn application_did_change_activation(&self, is_active: bool) {
        let mut policy = self.policy.get();
        if policy.is_app_active == is_active {
            return;
        }
        policy.is_app_active = is_active;
        self.policy.set(policy);
        // Coming back to the app is the moment a pending build matters most;
        // leaving it only changes the cadence.
        if is_active { self.probe(ProbeReason::Event) } else { self.schedule_next() }
    }

    pub fn system_did_wake(&self) {
        // The feed almost certainly moved across a closed lid, and the
        // schedule that would have caught it did not fire while asleep.
        self.probe(ProbeReason::Event);
    }

    pub fn power_state_did_change(&self, is_low_power: bool) {
        let mut policy = self.policy.get();
        if policy.is_low_power == is_low_power {
            return;
        }
        policy.is_low_power = is_low_power;
        self.policy.set(policy);
        self.schedule_next();
    }

    pub fn network_availability_did_change(&self, has_network: bool) {
        let mut policy = self.policy.get();
        if policy.has_network == has_network {
            return;
        }
        policy.has_network = has_network;
        self.policy.set(policy);
        if has_network { self.probe(ProbeReason::Event) } else { self.schedule_next() }
    }

    /// Ignores the spacing floor. Only for tests and the debug feed override.
    pub fn probe_now(&self) {
        self.last_probe_date.set(None);
        self.probe(ProbeReason::Event);
    }

    // MARK: Scheduling

    fn schedule_next(&self) {
        let pending = self.pending.borrow_mut().take();
        if let Some(pending) = pending {
            pending.cancel();
        }
        if !self.is_running.get() {
            return;
        }
        let Some(interval) = self.policy.get().interval() else {
            return;
        };
        let weak = self.weak_self.clone();
        let work = async_after(interval, move || {
            if let Some(watch) = weak.upgrade() {
                watch.probe(ProbeReason::Scheduled);
            }
        });
        *self.pending.borrow_mut() = Some(work);
    }

    fn probe(&self, reason: ProbeReason) {
        if !self.is_running.get() || self.policy.get().interval().is_none() || self.is_probing.get() {
            return;
        }
        if reason == ProbeReason::Event
            && let Some(last) = self.last_probe_date.get()
            && Date::now().time_interval_since(last) < ReleaseWatchPolicy::MINIMUM_SPACING
        {
            // Too soon. Fall back to the schedule rather than dropping the
            // event entirely, or a burst of activations can leave no timer.
            if self.pending.borrow().is_none() {
                self.schedule_next();
            }
            return;
        }
        self.is_probing.set(true);
        self.last_probe_date.set(Some(Date::now()));
        let pending = self.pending.borrow_mut().take();
        if let Some(pending) = pending {
            pending.cancel();
        }
        let feed = self.feed.clone();
        let validator = self.validator.borrow().clone();
        let prober = self.prober.clone();
        let weak = self.weak_self.clone();
        async_main(move || {
            prober.probe(
                &feed,
                validator.as_deref(),
                Box::new(move |result| {
                    if let Some(watch) = weak.upgrade() {
                        watch.probe_did_finish(result);
                    }
                }),
            );
        });
    }

    fn probe_did_finish(&self, result: ReleaseFeedProbeResult) {
        self.is_probing.set(false);
        self.completed_probe_count.set(self.completed_probe_count.get() + 1);
        *self.last_result.borrow_mut() = Some(result.clone());
        if let ReleaseFeedProbeResult::Changed { validator: new_validator } = result {
            *self.validator.borrow_mut() = new_validator;
            if self.has_baseline.get() {
                let handler = self.on_feed_changed.borrow().clone();
                if let Some(handler) = handler {
                    handler();
                }
            } else {
                // Sparkle's own post-launch check already answers "was an
                // update waiting when you opened the app". Firing here as well
                // would ask the same feed the same question twice.
                self.has_baseline.set(true);
            }
        }
        self.schedule_next();
    }

    // MARK: System events

    fn observe_system_events(&self) {
        let center = NSNotificationCenter::defaultCenter();
        let main_queue = NSOperationQueue::mainQueue();
        let mut observers = Vec::new();

        let observe = |center: &Retained<NSNotificationCenter>, name: &NSString, handler: Box<dyn Fn(&ReleaseWatch)>| {
            let weak = MainOnly::new(self.weak_self.clone());
            let handler = MainOnly::new(handler);
            let block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
                // `queue: .main`, then `MainActor.assumeIsolated`.
                if let Some(watch) = weak.get().upgrade() {
                    (handler.get())(&watch);
                }
            });
            // SAFETY: the block runs on the main operation queue, the thread
            // that owns everything it captures (checked by `MainOnly`).
            let token = unsafe {
                center.addObserverForName_object_queue_usingBlock(Some(name), None, Some(&main_queue), &block)
            };
            (center.clone(), token)
        };

        // SAFETY: AppKit's and Foundation's notification-name constants.
        let (became_active, resigned_active, power_state, did_wake) = unsafe {
            (
                NSApplicationDidBecomeActiveNotification,
                NSApplicationDidResignActiveNotification,
                NSProcessInfoPowerStateDidChangeNotification,
                NSWorkspaceDidWakeNotification,
            )
        };
        observers.push(observe(&center, became_active, Box::new(|watch| watch.application_did_change_activation(true))));
        observers.push(observe(&center, resigned_active, Box::new(|watch| watch.application_did_change_activation(false))));
        observers.push(observe(
            &center,
            power_state,
            Box::new(|watch| {
                let low = NSProcessInfo::processInfo().isLowPowerModeEnabled();
                watch.power_state_did_change(low);
            }),
        ));
        let workspace = NSWorkspace::sharedWorkspace().notificationCenter();
        observers.push(observe(&workspace, did_wake, Box::new(|watch| watch.system_did_wake())));
        self.observers.borrow_mut().extend(observers);

        let weak = self.weak_self.clone();
        let monitor = PathMonitor::start_on_main_queue(move |satisfied| {
            let weak = weak.clone();
            async_main(move || {
                if let Some(watch) = weak.upgrade() {
                    watch.network_availability_did_change(satisfied);
                }
            });
        });
        *self.path_monitor.borrow_mut() = Some(monitor);
    }
}

impl Drop for ReleaseWatch {
    fn drop(&mut self) {
        if let Some(pending) = self.pending.get_mut().take() {
            pending.cancel();
        }
        if let Some(monitor) = self.path_monitor.get_mut().take() {
            monitor.cancel();
        }
        remove_observers(std::mem::take(self.observers.get_mut()));
    }
}

fn remove_observers(observers: Vec<Observer>) {
    for (center, observer) in observers {
        // SAFETY: the token came from this centre's `addObserverForName:…`.
        unsafe { center.removeObserver(observer.as_ref()) };
    }
}

/// `NSApp?.isActive ?? false`: reads AppKit's `NSApp` global without creating
/// the shared application (objc2's `NSApp()` would create it).
fn ns_app_is_active() -> bool {
    unsafe extern "C" {
        static NSApp: *mut NSApplication;
    }
    // SAFETY: `NSApp` is AppKit's global; it is null until `NSApplication`
    // is created, and only read here on the main thread (the watch is a
    // main-actor object).
    let app = unsafe { NSApp };
    if app.is_null() {
        return false;
    }
    // SAFETY: a non-null `NSApp` is the live shared application.
    unsafe { &*app }.isActive()
}

// MARK: - NWPathMonitor

#[link(name = "Network", kind = "framework")]
unsafe extern "C" {
    fn nw_path_monitor_create() -> *mut c_void;
    fn nw_path_monitor_set_update_handler(monitor: *mut c_void, handler: &block2::DynBlock<dyn Fn(*mut c_void)>);
    fn nw_path_monitor_set_queue(monitor: *mut c_void, queue: *const c_void);
    fn nw_path_monitor_start(monitor: *mut c_void);
    fn nw_path_monitor_cancel(monitor: *mut c_void);
    fn nw_path_get_status(path: *mut c_void) -> i32;
    fn nw_release(object: *mut c_void);
}

/// `nw_path_status_satisfied`.
const NW_PATH_STATUS_SATISFIED: i32 = 1;

/// Swift's `NWPathMonitor`, over Network.framework's C API.
struct PathMonitor(*mut c_void);

impl PathMonitor {
    /// `monitor.pathUpdateHandler = { path in … path.status == .satisfied … }`
    /// then `monitor.start(queue: .main)`.
    fn start_on_main_queue(handler: impl Fn(bool) + 'static) -> PathMonitor {
        let handler = MainOnly::new(handler);
        let block = RcBlock::new(move |path: *mut c_void| {
            // SAFETY: Network passes the current path object.
            let satisfied = unsafe { nw_path_get_status(path) } == NW_PATH_STATUS_SATISFIED;
            (handler.get())(satisfied);
        });
        // SAFETY: plain Network.framework calls on a freshly created monitor;
        // the handler block is copied by `set_update_handler` and runs on the
        // main queue.
        unsafe {
            let monitor = nw_path_monitor_create();
            nw_path_monitor_set_update_handler(monitor, &block);
            let main: *const DispatchQueue = DispatchQueue::main();
            nw_path_monitor_set_queue(monitor, main.cast());
            nw_path_monitor_start(monitor);
            PathMonitor(monitor)
        }
    }

    fn cancel(&self) {
        // SAFETY: a live monitor.
        unsafe { nw_path_monitor_cancel(self.0) };
    }
}

impl Drop for PathMonitor {
    fn drop(&mut self) {
        // SAFETY: balances `nw_path_monitor_create`.
        unsafe { nw_release(self.0) };
    }
}

// MARK: - Production probe

/// One conditional GET against the appcast, and nothing else.
///
/// Deliberately the smallest request that can answer the question: no
/// cookies, no credentials, no cache of its own, no identifiers, and an
/// unchanged feed answers `304` with an empty body. It reads the feed only
/// far enough to know *that* it moved — never what it says.
pub struct ReleaseFeedURLProbe {
    session: Retained<NSURLSession>,
}

impl Default for ReleaseFeedURLProbe {
    fn default() -> ReleaseFeedURLProbe {
        ReleaseFeedURLProbe::new()
    }
}

impl ReleaseFeedURLProbe {
    /// A feed larger than this is not one this app publishes; refuse to hash
    /// an unbounded body just to learn one bit.
    pub const MAXIMUM_BODY_BYTES: usize = 4 * 1024 * 1024;

    const ETAG_PREFIX: &'static str = "etag:";
    const LAST_MODIFIED_PREFIX: &'static str = "lastModified:";

    /// `init()`: the locked-down ephemeral configuration.
    pub fn new() -> ReleaseFeedURLProbe {
        ReleaseFeedURLProbe::with_session(NSURLSession::sessionWithConfiguration(&Self::make_configuration()))
    }

    /// `init(session:)`. `session` is a test seam: production uses the
    /// locked-down ephemeral configuration; tests inject one backed by a stub
    /// `URLProtocol`.
    pub fn with_session(session: Retained<NSURLSession>) -> ReleaseFeedURLProbe {
        ReleaseFeedURLProbe { session }
    }

    fn make_configuration() -> Retained<NSURLSessionConfiguration> {
        let configuration = NSURLSessionConfiguration::ephemeralSessionConfiguration();
        configuration.setHTTPCookieAcceptPolicy(NSHTTPCookieAcceptPolicy::Never);
        configuration.setHTTPShouldSetCookies(false);
        configuration.setURLCache(None);
        // The validators below are the cache; a URL cache on top of them only
        // adds a second staleness policy that can hold a fresh feed back.
        configuration.setRequestCachePolicy(NSURLRequestCachePolicy::ReloadIgnoringLocalCacheData);
        configuration.setTimeoutIntervalForRequest(15.0);
        configuration.setWaitsForConnectivity(false);
        configuration
    }

    /// Everything after `session.data(for:)` returns: classify the response
    /// and, for a fresh body, derive the validator.
    fn result(data: Option<&NSData>, response: Option<&NSURLResponse>, failed: bool, validator: Option<&str>) -> ReleaseFeedProbeResult {
        if failed {
            return ReleaseFeedProbeResult::Unreachable;
        }
        let Some(http) = response.and_then(|response| response.downcast_ref::<NSHTTPURLResponse>()) else {
            return ReleaseFeedProbeResult::Unreachable;
        };
        let status = http.statusCode();
        if status == 304 {
            return ReleaseFeedProbeResult::Unchanged;
        }
        if !(200..300).contains(&status) {
            return ReleaseFeedProbeResult::Unreachable;
        }
        let body = data.map(|data| data.to_vec()).unwrap_or_default();
        if body.len() > Self::MAXIMUM_BODY_BYTES {
            return ReleaseFeedProbeResult::Unreachable;
        }

        let fresh = Self::validator(http, &body);
        // `fresh == validator`: a `String` against a `String?`.
        if validator.is_some_and(|validator| upleft_swift_text::str_eq(&fresh, validator)) {
            ReleaseFeedProbeResult::Unchanged
        } else {
            ReleaseFeedProbeResult::Changed { validator: Some(fresh) }
        }
    }

    /// Prefer the server's own validator; fall back to the document date,
    /// which keeps conditional requests working on a host that serves no
    /// `ETag`; fall back further to hashing the body, which keeps the watch
    /// working on a host that serves neither.
    fn validator(response: &NSHTTPURLResponse, body: &[u8]) -> String {
        if let Some(etag) = response.valueForHTTPHeaderField(&NSString::from_str("ETag"))
            && etag.length() != 0
        {
            return format!("{}{}", Self::ETAG_PREFIX, etag);
        }
        if let Some(last_modified) = response.valueForHTTPHeaderField(&NSString::from_str("Last-Modified"))
            && last_modified.length() != 0
        {
            return format!("{}{}", Self::LAST_MODIFIED_PREFIX, last_modified);
        }
        let digest = Sha256::digest(body);
        let mut text = String::from("sha256:");
        for byte in digest {
            text.push_str(&format!("{byte:02x}"));
        }
        text
    }
}

impl ReleaseFeedProbe for ReleaseFeedURLProbe {
    fn probe(&self, feed: &Url, validator: Option<&str>, completion: Box<dyn FnOnce(ReleaseFeedProbeResult)>) {
        let request = NSMutableURLRequest::requestWithURL(feed.as_nsurl());
        request.setHTTPMethod(&NSString::from_str("GET"));
        request.setCachePolicy(NSURLRequestCachePolicy::ReloadIgnoringLocalCacheData);
        // An ETag round-trips verbatim, and so does a stored document date.
        // A body hash is ours, not the server's, so it must never be offered
        // as either.
        if let Some(validator) = validator {
            if upleft_swift_text::has_prefix(validator, Self::ETAG_PREFIX) {
                let value = upleft_swift_text::drop_first(validator, upleft_swift_text::count(Self::ETAG_PREFIX));
                request.setValue_forHTTPHeaderField(Some(&NSString::from_str(value)), &NSString::from_str("If-None-Match"));
            } else if upleft_swift_text::has_prefix(validator, Self::LAST_MODIFIED_PREFIX) {
                let value =
                    upleft_swift_text::drop_first(validator, upleft_swift_text::count(Self::LAST_MODIFIED_PREFIX));
                request.setValue_forHTTPHeaderField(
                    Some(&NSString::from_str(value)),
                    &NSString::from_str("If-Modified-Since"),
                );
            }
        }
        let previous = validator.map(str::to_owned);
        let completion = Mutex::new(Some(MainOnly::new(completion)));
        let handler = RcBlock::new(move |data: *mut NSData, response: *mut NSURLResponse, error: *mut NSError| {
            // On the session's delegate queue.
            // SAFETY: URLSession passes valid (or nil) objects for the call.
            let (data, response) = unsafe { (data.as_ref(), response.as_ref()) };
            let result = ReleaseFeedURLProbe::result(data, response, !error.is_null(), previous.as_deref());
            let completion = completion.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
            if let Some(completion) = completion {
                DispatchQueue::main().exec_async(move || (completion.into_inner())(result));
            }
        });
        // SAFETY: the handler only touches `Send` state (the completion is
        // carried to the main queue in a `MainOnly`).
        let task = unsafe { self.session.dataTaskWithRequest_completionHandler(&request, &handler) };
        task.resume();
    }
}
