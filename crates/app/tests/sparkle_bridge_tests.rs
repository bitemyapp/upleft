//! The real Sparkle 2.9.6 behind the updater (`updater::sparkle`): the parts
//! of `Tests/DownrightAppTests/UpdateCoordinatorTests.swift` that depend on
//! the framework, plus the bridge's own checks.
//!
//! - `UpdateBuildContractTests`: Sparkle is linked by the app binary only
//!   (this test binary links upleft-app and never loads Sparkle until it
//!   `dlopen`s it), and every pin says 2.9.6.
//! - The coordinator flows of `UpdateCoordinatorFlowTests` that start at a
//!   Sparkle callback, driven through the Objective-C `SPUUserDriver` and
//!   `SPUUpdaterDelegate` with Sparkle's own objects (`SUAppcastItem`,
//!   `SPUUserUpdateState`, `SPUDownloadData`, `NSError` user info keyed by
//!   Sparkle's constants) and Sparkle's reply blocks. Each keeps the Swift
//!   test's name and assertions.
//! - A real `SPUUpdater`, built by `make_updater` for a throw-away host
//!   bundle: its settings, its start-up failure, and its wiring.
//!
//! Nothing here checks for updates, touches the network or shows UI. The
//! test process's main bundle has no `SUFeedURL` (a dev bundle), so the
//! coordinator stays disabled. The only `SPUUpdater`s are built for a
//! temporary bundle with no feed and no signing key, whose `startUpdater:`
//! fails before Sparkle schedules anything; their settings live in a unique
//! `upleft.conformance.sparkle.<uuid>` defaults suite, which each test
//! removes, plist included.
//!
//! The framework comes from `UPLEFT_SPARKLE_FRAMEWORK`, or else from where
//! `scripts/sparkle-framework.sh` puts it. When it is absent, the tests that
//! need it are reported as skipped.

mod updater_support;

use std::cell::{Cell, RefCell};
use std::ffi::{CStr, CString, c_char, c_uint, c_void};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::rc::{Rc, Weak};
use std::sync::OnceLock;
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, AnyProtocol, Bool, Sel};
use objc2::{AnyThread, ClassType, MainThreadMarker, Message, msg_send};
use objc2_foundation::{
    NSArray, NSBundle, NSData, NSDate, NSDictionary, NSError, NSNumber, NSString, NSURL, NSUUID, NSUserDefaults,
};
use updater_support::{Skipped, Test, pump};
use upleft_app::updater::downright_update_driver::{
    DownrightUpdateDriver, SPU_NO_UPDATE_FOUND_USER_INITIATED_KEY, UpdateDriverHost,
};
use upleft_app::updater::main_actor::is_main_thread;
use upleft_app::updater::sparkle::{
    self, BackgroundDownloadNotifierObject, DownrightUpdateDriverObject, SPARKLE_VERSION, SPUDownloadData,
    SPUUpdatePermissionRequest, SPUUserUpdateState, SUAppcastItem, SUUpdatePermissionResponse, SparkleUpdater,
};
use upleft_app::updater::update_coordinator::{UpdateCoordinator, UpdatePillModel, UpdateReleaseNotesState};
use upleft_app::updater::update_engine::{
    BackgroundDownloadNotifier, FakeUpdateEngine, SparkleUpdateEngine, SpuUpdater, UpdateEngine,
};
use upleft_app::updater::update_metadata::{UpdateMetadata, Url};
use upleft_app::updater::update_state_machine::{UpdatePhase, UpdateStage};

// MARK: - The framework

/// Where `scripts/sparkle-framework.sh` finds the SwiftPM-resolved framework.
const DEFAULT_FRAMEWORK: &str =
    "target/app-oracle/artifacts/sparkle/Sparkle/Sparkle.xcframework/macos-arm64_x86_64/Sparkle.framework";

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("the workspace root")
}

fn framework_path() -> Option<PathBuf> {
    let path = match std::env::var_os("UPLEFT_SPARKLE_FRAMEWORK") {
        Some(path) => PathBuf::from(path),
        None => workspace_root().join(DEFAULT_FRAMEWORK),
    };
    path.join("Sparkle").exists().then_some(path)
}

static FRAMEWORK: OnceLock<PathBuf> = OnceLock::new();

/// `dlsym` of one of Sparkle's `NSString *const` constants.
fn sparkle_string_constant(name: &CStr) -> Retained<NSString> {
    // SAFETY: looks a symbol up in every loaded image.
    let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, name.as_ptr()) };
    assert!(!symbol.is_null(), "Sparkle exports {name:?}");
    // SAFETY: the symbol is an `NSString *const` holding a constant string.
    unsafe { (*(symbol as *const *const NSString)).as_ref() }.expect("a non-nil constant").retain()
}

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("the tests run on the main thread")
}

// MARK: - Fixtures (from UpdateCoordinatorTests.swift)

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

/// An `NSDictionary` of strings and nested dictionaries, as Sparkle's RSS
/// parser hands `SUAppcastItem`.
enum Value {
    Text(&'static str),
    Dictionary(Vec<(&'static str, Value)>),
}

fn dictionary(entries: &[(&'static str, Value)]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<Retained<NSString>> = entries.iter().map(|(key, _)| NSString::from_str(key)).collect();
    let values: Vec<Retained<AnyObject>> = entries
        .iter()
        .map(|(_, value)| match value {
            Value::Text(text) => Retained::into_super(Retained::into_super(NSString::from_str(text))),
            Value::Dictionary(inner) => Retained::into_super(Retained::into_super(dictionary(inner))),
        })
        .collect();
    let key_refs: Vec<&NSString> = keys.iter().map(|key| &**key).collect();
    let value_refs: Vec<&AnyObject> = values.iter().map(|value| &**value).collect();
    NSDictionary::from_slices(&key_refs, &value_refs)
}

/// `-[SUAppcastItem initWithDictionary:]`, the parser's entry point (no
/// state resolver, so no host-bundle comparisons).
fn appcast_item(entries: Vec<(&'static str, Value)>) -> Retained<SUAppcastItem> {
    let dictionary = dictionary(&entries);
    // SAFETY: `initWithDictionary:` takes an `NSDictionary *` and answers a
    // nullable instance.
    let item: Option<Retained<SUAppcastItem>> =
        unsafe { msg_send![SUAppcastItem::alloc(), initWithDictionary: &*dictionary] };
    item.expect("Sparkle accepts the item")
}

/// The appcast item behind `sampleMetadata`.
fn sample_item() -> Retained<SUAppcastItem> {
    appcast_item(vec![
        ("title", Value::Text("Upleft 1.1.0")),
        ("description", Value::Text("## What's new\n- Faster parsing")),
        ("link", Value::Text("https://github.com/bitemyapp/upleft/releases/tag/v1.1.0")),
        ("sparkle:version", Value::Text("47")),
        ("sparkle:shortVersionString", Value::Text("1.1.0")),
        (
            "enclosure",
            Value::Dictionary(vec![
                ("url", Value::Text("https://updates.example.test/Upleft-1.1.0.zip")),
                ("length", Value::Text("4000000")),
            ]),
        ),
    ])
}

/// The appcast item behind `informationalMetadata`: a link and no enclosure.
fn informational_item() -> Retained<SUAppcastItem> {
    appcast_item(vec![
        ("link", Value::Text("https://github.com/bitemyapp/upleft/releases/tag/v1.2.0")),
        ("sparkle:version", Value::Text("50")),
        ("sparkle:shortVersionString", Value::Text("1.2.0")),
    ])
}

/// `-[SPUUserUpdateState initWithStage:userInitiated:]` (Sparkle's private
/// initializer, which its drivers use).
fn user_update_state(stage: isize, user_initiated: bool) -> Retained<SPUUserUpdateState> {
    // SAFETY: the private initializer's signature, from
    // `SPUUserUpdateState+Private.h` in Sparkle 2.9.6.
    unsafe { msg_send![SPUUserUpdateState::alloc(), initWithStage: stage, userInitiated: user_initiated] }
}

/// `-[SPUDownloadData initWithData:URL:textEncodingName:MIMEType:]`.
fn download_data(bytes: &[u8]) -> Retained<SPUDownloadData> {
    let data = NSData::with_bytes(bytes);
    let url = NSURL::URLWithString(&NSString::from_str("https://updates.example.test/notes.html")).unwrap();
    let mime = NSString::from_str("text/html");
    // SAFETY: the initializer's signature, from `SPUDownloadData.m` in
    // Sparkle 2.9.6.
    unsafe {
        msg_send![SPUDownloadData::alloc(), initWithData: &*data, URL: &*url, textEncodingName: None::<&NSString>, MIMEType: &*mime]
    }
}

fn ns_error(domain: &str, code: isize, user_info: Option<(&NSString, &AnyObject)>) -> Retained<NSError> {
    let user_info = user_info.map(|(key, value)| NSDictionary::<NSString, AnyObject>::from_slices(&[key], &[value]));
    // SAFETY: a string-keyed user info dictionary.
    unsafe { NSError::errorWithDomain_code_userInfo(&NSString::from_str(domain), code, user_info.as_deref()) }
}

// MARK: - Coordinator and bridge

fn make_coordinator() -> (Rc<UpdateCoordinator>, Rc<FakeUpdateEngine>) {
    let engine = FakeUpdateEngine::new();
    let coordinator = UpdateCoordinator::new(Some(engine.clone() as Rc<dyn UpdateEngine>));
    coordinator.set_suppress_ui_for_testing(true);
    let _ = engine.start();
    (coordinator, engine)
}

fn host_of(coordinator: &Rc<UpdateCoordinator>) -> Weak<dyn UpdateDriverHost> {
    let host: Rc<dyn UpdateDriverHost> = coordinator.clone();
    Rc::downgrade(&host)
}

/// A coordinator on the fake engine, and the Objective-C user driver Sparkle
/// would call, forwarding to it.
fn make_bridge() -> (Rc<UpdateCoordinator>, Retained<DownrightUpdateDriverObject>) {
    let (coordinator, _) = make_coordinator();
    let driver = DownrightUpdateDriver::new(Some(host_of(&coordinator)));
    (coordinator, DownrightUpdateDriverObject::new(mtm(), driver))
}

/// A `void (^)(SPUUserUpdateChoice)` block that records Sparkle's choices.
fn choice_block(log: &Rc<RefCell<Vec<isize>>>) -> RcBlock<dyn Fn(isize)> {
    let log = log.clone();
    RcBlock::new(move |choice: isize| log.borrow_mut().push(choice))
}

/// A `void (^)(void)` block that counts.
fn counting_block(count: &Rc<Cell<i32>>) -> RcBlock<dyn Fn()> {
    let count = count.clone();
    RcBlock::new(move || count.set(count.get() + 1))
}

fn nothing_block() -> RcBlock<dyn Fn()> {
    RcBlock::new(|| {})
}

// `SPUUserUpdateChoice` and `SPUUserUpdateStage` raw values (Sparkle 2.9.6).
const CHOICE_SKIP: isize = 0;
const CHOICE_INSTALL: isize = 1;
const CHOICE_DISMISS: isize = 2;
const STAGE_NOT_DOWNLOADED: isize = 0;
const STAGE_DOWNLOADED: isize = 1;

fn show_update_found(
    driver: &DownrightUpdateDriverObject,
    item: &SUAppcastItem,
    stage: isize,
    user_initiated: bool,
    reply: &RcBlock<dyn Fn(isize)>,
) {
    let state = user_update_state(stage, user_initiated);
    // SAFETY: `SPUUserDriver`'s selector and argument types.
    let () = unsafe { msg_send![driver, showUpdateFoundWithAppcastItem: item, state: &*state, reply: &**reply] };
}

// MARK: - A throw-away host bundle

/// A bundle directory Sparkle can target: an identifier, a version, and the
/// production `SU*` keys of Downright's `bundle-app.sh` except the feed URL
/// and the public key. Sparkle keeps its settings in a defaults suite named
/// after the bundle identifier; dropping this removes the suite, its plist
/// and the directory.
struct HostBundle {
    directory: PathBuf,
    suite: String,
    bundle: Retained<NSBundle>,
}

impl HostBundle {
    fn new() -> HostBundle {
        let unique = NSUUID::new().UUIDString().to_string();
        let suite = format!("upleft.conformance.sparkle.{unique}");
        let directory = std::env::temp_dir().join(format!("UpleftSparkleBridgeTest-{unique}.bundle"));
        std::fs::create_dir_all(directory.join("Contents")).unwrap();
        let info = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key><string>{suite}</string>
	<key>CFBundleName</key><string>UpleftSparkleBridgeTest</string>
	<key>CFBundlePackageType</key><string>BNDL</string>
	<key>CFBundleShortVersionString</key><string>1.0</string>
	<key>CFBundleVersion</key><string>1</string>
	<key>SUEnableAutomaticChecks</key><true/>
	<key>SUAutomaticallyUpdate</key><true/>
	<key>SUScheduledCheckInterval</key><integer>86400</integer>
	<key>SUVerifyUpdateBeforeExtraction</key><true/>
	<key>SURequireSignedFeed</key><true/>
	<key>SUEnableSystemProfiling</key><false/>
</dict>
</plist>
"#
        );
        std::fs::write(directory.join("Contents/Info.plist"), info).unwrap();
        let bundle = NSBundle::bundleWithPath(&NSString::from_str(directory.to_str().unwrap())).expect("a bundle");
        assert_eq!(bundle.bundleIdentifier().map(|id| id.to_string()), Some(suite.clone()));
        HostBundle { directory, suite, bundle }
    }

    fn defaults(&self) -> Retained<NSUserDefaults> {
        NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(&NSString::from_str(&self.suite))).unwrap()
    }
}

/// `UserDefaults` ignores `CFFIXED_USER_HOME`: a suite's plist lands in the
/// real home, so the test removes the domain and the file.
fn real_preferences_file(suite: &str) -> Option<PathBuf> {
    // SAFETY: reads this user's password entry.
    let entry = unsafe { libc::getpwuid(libc::getuid()) };
    if entry.is_null() {
        return None;
    }
    // SAFETY: `pw_dir` is a C string owned by the entry.
    let home = unsafe { CStr::from_ptr((*entry).pw_dir) }.to_string_lossy().into_owned();
    Some(PathBuf::from(format!("{home}/Library/Preferences/{suite}.plist")))
}

impl Drop for HostBundle {
    fn drop(&mut self) {
        let defaults = self.defaults();
        defaults.removePersistentDomainForName(&NSString::from_str(&self.suite));
        // cfprefsd writes the emptied domain back asynchronously; wait for it
        // before deleting the file, or an empty plist can land afterwards.
        defaults.synchronize();
        if let Some(path) = real_preferences_file(&self.suite) {
            let _ = std::fs::remove_file(path);
        }
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

/// `SPUUpdater(hostBundle:applicationBundle:userDriver:delegate:)` through
/// the factory, for a throw-away bundle, with drivers forwarding to
/// `coordinator`.
fn make_sparkle_updater(host: &HostBundle, coordinator: &Rc<UpdateCoordinator>) -> Rc<SparkleUpdater> {
    let driver = DownrightUpdateDriver::new(Some(host_of(coordinator)));
    let notifier = Rc::new(BackgroundDownloadNotifier::default());
    sparkle::make_updater(&host.bundle, &host.bundle, &driver, &notifier).expect("Sparkle is loaded")
}

// MARK: - UpdateBuildContractTests

unsafe extern "C" {
    fn _dyld_image_count() -> u32;
    fn _dyld_get_image_name(image_index: u32) -> *const c_char;
}

/// Sparkle must be linked by the host app only — never by MarkdownCore,
/// MarkdownRender, the CLI, or the Quick Look targets. In Upleft: this test
/// binary links upleft-app and its whole dependency graph, and Sparkle is not
/// loaded until the test `dlopen`s it; only `crates/app` names Sparkle, and
/// no build script links it outside the app: a `rustc-link-arg-bin`, or the
/// app binary's own crate, `crates/upleft`, whose only target is the app.
fn sparkle_is_imported_only_by_the_host_app() {
    assert!(!sparkle::is_available(), "Sparkle is loaded before the test loads it");
    // SAFETY: dyld's image list, read on one thread.
    let loaded: Vec<String> = unsafe {
        (0.._dyld_image_count())
            .filter_map(|index| {
                let name = _dyld_get_image_name(index);
                (!name.is_null()).then(|| CStr::from_ptr(name).to_string_lossy().into_owned())
            })
            .collect()
    };
    let linked: Vec<&String> = loaded.iter().filter(|name| name.contains("Sparkle.framework")).collect();
    assert!(linked.is_empty(), "upleft-app links Sparkle: {linked:?}");

    let root = workspace_root();
    let mut offenders = Vec::new();
    for entry in std::fs::read_dir(root.join("crates")).unwrap() {
        let crate_dir = entry.unwrap().path();
        let is_host_app = crate_dir.file_name().is_some_and(|name| name == "app" || name == "upleft");
        let is_app_binary = crate_dir.file_name().is_some_and(|name| name == "upleft");
        for manifest in ["build.rs", "Cargo.toml"] {
            if is_app_binary {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(crate_dir.join(manifest)) else { continue };
            for line in text.lines() {
                let links_sparkle =
                    line.contains("Sparkle") && (line.contains("framework") || line.contains("rustc-link"));
                if links_sparkle && !line.contains("rustc-link-arg-bin") {
                    offenders.push(format!("{}: {line}", crate_dir.join(manifest).display()));
                }
            }
        }
        let mut stack = vec![crate_dir.join("src")];
        while let Some(directory) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&directory) else { continue };
            for entry in entries {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|extension| extension != "rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                let names_sparkle = ["name = \"SPU", "name = \"SUAppcastItem\"", "c\"SPU", "c\"SUAppcastItem\""]
                    .iter()
                    .any(|needle| text.contains(needle));
                let uses_bridge = text.contains("updater::sparkle");
                let is_bridge = path.ends_with("crates/app/src/updater/sparkle.rs") || is_app_binary;
                if (names_sparkle && !is_bridge) || (uses_bridge && !is_host_app) {
                    offenders.push(path.strip_prefix(&root).unwrap().display().to_string());
                }
            }
        }
    }
    assert!(offenders.is_empty(), "Sparkle imported outside the host app: {offenders:?}");
}

/// Every Sparkle pin says 2.9.6: Downright's `Package.swift` (the
/// specification), the oracle package that resolves the framework, the
/// script that hands it to the app's link, and the bridge's declarations.
fn package_and_xcode_project_declare_sparkle_exactly() {
    let root = workspace_root();
    let downright = std::fs::read_to_string(root.join("vendor/downright/Package.swift")).unwrap();
    assert!(downright.contains("exact: \"2.9.6\""), "Downright pins Sparkle exactly to 2.9.6");
    let oracle = std::fs::read_to_string(root.join("oracle/app/Package.swift")).unwrap();
    assert!(
        oracle.contains(".package(url: \"https://github.com/sparkle-project/Sparkle.git\", exact: \"2.9.6\")"),
        "oracle/app must pin Sparkle exactly to 2.9.6"
    );
    let script = std::fs::read_to_string(root.join("scripts/sparkle-framework.sh")).unwrap();
    assert!(script.contains("expected=\"2.9.6\""), "scripts/sparkle-framework.sh must insist on 2.9.6");
    assert_eq!(SPARKLE_VERSION, "2.9.6");
}

// MARK: - Before and after loading

/// Without the framework, the factory answers `None`, and the start-up
/// sequence leaves the updater in its ordinary disabled state.
fn factory_answers_none_without_the_framework() {
    assert!(!sparkle::is_available());
    let (coordinator, _) = make_coordinator();
    let driver = DownrightUpdateDriver::new(Some(host_of(&coordinator)));
    let notifier = Rc::new(BackgroundDownloadNotifier::default());
    let main = NSBundle::mainBundle();
    assert!(sparkle::make_updater(&main, &main, &driver, &notifier).is_none());
    sparkle::install(mtm());
    assert!(SparkleUpdateEngine::new(driver).is_none());
}

/// Loads Sparkle.framework into this process, as the app binary's link
/// does. Every test after the build contract runs this first (it loads
/// once), so a filter can select any one of them.
fn load_sparkle() {
    static LOADED: OnceLock<()> = OnceLock::new();
    LOADED.get_or_init(|| {
        let framework = FRAMEWORK.get().expect("main found the framework");
        let binary = CString::new(framework.join("Sparkle").to_str().unwrap()).unwrap();
        // SAFETY: loads a framework image; nothing is unloaded afterwards.
        let handle = unsafe { libc::dlopen(binary.as_ptr(), libc::RTLD_NOW) };
        if handle.is_null() {
            // SAFETY: `dlerror` after a failed `dlopen`.
            let error = unsafe { CStr::from_ptr(libc::dlerror()) }.to_string_lossy().into_owned();
            panic!("dlopen {}: {error}", framework.display());
        }
    });
}

/// The framework loads, and it is the pinned version.
fn loads_sparkle_2_9_6() {
    assert!(sparkle::is_available());
    let class = AnyClass::get(c"SPUUpdater").unwrap();
    // SAFETY: `+[NSBundle bundleForClass:]`.
    let bundle: Retained<NSBundle> = unsafe { msg_send![NSBundle::class(), bundleForClass: class] };
    let version = bundle
        .objectForInfoDictionaryKey(&NSString::from_str("CFBundleShortVersionString"))
        .and_then(|value| value.downcast::<NSString>().ok())
        .map(|value| value.to_string());
    assert_eq!(version.as_deref(), Some(SPARKLE_VERSION));
}

/// The app's start-up sequence in a dev bundle (no `SUFeedURL`, as this test
/// binary's): the factory is installed and Sparkle is loaded, but the
/// configuration gate keeps the engine unbuilt.
fn a_dev_bundle_stays_disabled_with_sparkle_loaded() {
    let mtm = mtm();
    sparkle::install(mtm);
    let coordinator = UpdateCoordinator::shared(mtm);
    coordinator.start();
    assert!(!coordinator.is_running());
    assert!(!coordinator.is_update_configuration_present());
    assert!(!coordinator.can_check_for_updates());
    let driver = DownrightUpdateDriver::new(None);
    assert!(SparkleUpdateEngine::new(driver).is_none(), "no engine without the Info.plist feed");
}

// MARK: - Protocol conformance

/// `protocol_copyMethodDescriptionList`: (selector, type encoding) pairs.
fn protocol_methods(protocol: &AnyProtocol, required: bool) -> Vec<(Sel, String)> {
    let mut count: c_uint = 0;
    // SAFETY: the runtime returns a malloc'd array of `count` descriptions.
    let list = unsafe {
        objc2::ffi::protocol_copyMethodDescriptionList(protocol, Bool::new(required), Bool::new(true), &mut count)
    };
    if list.is_null() {
        return Vec::new();
    }
    // SAFETY: `list` holds `count` descriptions, freed below.
    let methods = unsafe { std::slice::from_raw_parts(list, count as usize) }
        .iter()
        .map(|description| {
            // SAFETY: each description's types are a C string.
            let types = unsafe { CStr::from_ptr(description.types) }.to_string_lossy().into_owned();
            (description.name.unwrap(), types)
        })
        .collect();
    // SAFETY: frees the runtime's array.
    unsafe { libc::free(list as *mut c_void) };
    methods
}

/// An encoding without its frame offsets (`v32@0:8@16@?24` → `v@:@@?`).
fn without_offsets(types: &str) -> String {
    types.chars().filter(|character| !character.is_ascii_digit()).collect()
}

fn method_types(class: &AnyClass, selector: Sel) -> Option<String> {
    let method = class.instance_method(selector)?;
    // SAFETY: a method's type encoding is a C string.
    let types: *const c_char = unsafe { objc2::ffi::method_getTypeEncoding(method) };
    Some(unsafe { CStr::from_ptr(types) }.to_string_lossy().into_owned())
}

/// The bridge classes carry the Swift classes' names, conform to Sparkle's
/// protocols, implement exactly what the Swift classes implement, and take
/// the argument types Sparkle's headers declare.
fn bridge_classes_conform_to_sparkles_protocols() {
    let user_driver = AnyProtocol::get(c"SPUUserDriver").unwrap();
    let delegate = AnyProtocol::get(c"SPUUpdaterDelegate").unwrap();
    let driver_class = DownrightUpdateDriverObject::class();
    let notifier_class = BackgroundDownloadNotifierObject::class();
    assert_eq!(driver_class.name(), c"DownrightUpdateDriver");
    assert_eq!(notifier_class.name(), c"BackgroundDownloadNotifier");
    assert!(driver_class.conforms_to(user_driver));
    assert!(notifier_class.conforms_to(delegate));

    // Swift's driver implements every required SPUUserDriver method and the
    // optional `showUpdateInFocus`. The other optional methods in Sparkle's
    // compiled protocol are the deprecated ones Sparkle falls back to when a
    // driver answers them; Swift's class does not, so neither may the port.
    let required = protocol_methods(user_driver, true);
    assert_eq!(required.len(), 16);
    let optional = protocol_methods(user_driver, false);
    let (implemented_optional, deprecated): (Vec<_>, Vec<_>) =
        optional.into_iter().partition(|(selector, _)| selector.name() == c"showUpdateInFocus");
    assert_eq!(implemented_optional.len(), 1);
    for (selector, types) in required.iter().chain(&implemented_optional) {
        let implemented = method_types(driver_class, *selector)
            .unwrap_or_else(|| panic!("DownrightUpdateDriver implements {selector:?}"));
        assert_eq!(without_offsets(&implemented), without_offsets(types), "-[DownrightUpdateDriver {selector:?}]");
    }
    for (selector, _) in &deprecated {
        assert!(!driver_class.responds_to(*selector), "DownrightUpdateDriver answers the deprecated {selector:?}");
    }

    // Swift's notifier implements one delegate method; answering any other
    // would change what Sparkle does (a feed URL, a version comparator …).
    assert!(protocol_methods(delegate, true).is_empty());
    let delegate_methods = protocol_methods(delegate, false);
    assert!(delegate_methods.len() > 20);
    for (selector, types) in &delegate_methods {
        let implemented = method_types(notifier_class, *selector);
        if selector.name() == c"updater:didDownloadUpdate:" {
            assert_eq!(without_offsets(&implemented.expect("updater:didDownloadUpdate:")), without_offsets(types));
        } else {
            assert!(implemented.is_none(), "BackgroundDownloadNotifier must not implement {selector:?}");
            assert!(!notifier_class.responds_to(*selector), "BackgroundDownloadNotifier answers {selector:?}");
        }
    }
}

// MARK: - A real SPUUpdater

/// `SPUUpdater` keeps the bridge's user driver strongly and its notifier
/// weakly; the updater wrapper keeps both alive.
fn the_updater_is_wired_to_the_driver_and_notifier() {
    let host = HostBundle::new();
    let (coordinator, _) = make_coordinator();
    let updater = make_sparkle_updater(&host, &coordinator);
    // SAFETY: key-value reads of SPUUpdater's `_userDriver` and `_delegate`.
    let user_driver: Option<Retained<AnyObject>> =
        unsafe { msg_send![updater.updater(), valueForKey: &*NSString::from_str("userDriver")] };
    let delegate: Option<Retained<AnyObject>> =
        unsafe { msg_send![updater.updater(), valueForKey: &*NSString::from_str("delegate")] };
    let as_object = |object: &AnyObject| object as *const AnyObject;
    assert_eq!(user_driver.as_deref().map(as_object), Some(as_object(updater.user_driver().as_ref())));
    assert_eq!(delegate.as_deref().map(as_object), Some(as_object(updater.delegate().as_ref())));
    // SAFETY: `-[SPUUpdater hostBundle]`.
    let host_bundle: Retained<NSBundle> = unsafe { msg_send![updater.updater(), hostBundle] };
    assert_eq!(host_bundle.bundleIdentifier().map(|id| id.to_string()), Some(host.suite.clone()));
}

/// The engine's settings are Sparkle's own persisted properties: they read
/// the host bundle's Info.plist and defaults, and write the defaults.
fn settings_read_and_write_sparkles_own_defaults() {
    let host = HostBundle::new();
    let last_check = NSDate::dateWithTimeIntervalSinceReferenceDate(780_000_000.0);
    let last_check: &AnyObject = last_check.as_ref();
    // SAFETY: a property-list value.
    unsafe { host.defaults().setObject_forKey(Some(last_check), &NSString::from_str("SULastCheckTime")) };
    let (coordinator, _) = make_coordinator();
    let updater = make_sparkle_updater(&host, &coordinator);

    assert!(updater.automatically_checks_for_updates());
    assert!(updater.automatically_downloads_updates());
    assert!(updater.allows_automatic_updates());
    assert_eq!(updater.update_check_interval(), 86_400.0);
    assert_eq!(
        updater.last_update_check_date().map(|date| date.time_interval_since_reference_date),
        Some(780_000_000.0)
    );
    assert!(!updater.can_check_for_updates(), "only a started updater accepts checks");

    updater.set_automatically_downloads_updates(false);
    assert!(!updater.automatically_downloads_updates());
    updater.set_automatically_checks_for_updates(false);
    assert!(!updater.automatically_checks_for_updates());
    updater.set_update_check_interval(3_600.0);
    assert_eq!(updater.update_check_interval(), 3_600.0);

    let defaults = host.defaults();
    let stored =
        |key: &str| defaults.objectForKey(&NSString::from_str(key)).and_then(|value| value.downcast::<NSNumber>().ok());
    assert_eq!(stored("SUEnableAutomaticChecks").map(|value| value.boolValue()), Some(false));
    assert_eq!(stored("SUAutomaticallyUpdate").map(|value| value.boolValue()), Some(false));
    assert_eq!(stored("SUScheduledCheckInterval").map(|value| value.doubleValue()), Some(3_600.0));

    updater.clear_feed_url_from_user_defaults();
    assert!(defaults.objectForKey(&NSString::from_str("SUFeedURL")).is_none());
}

/// A bundle without a signing key must not start: `startUpdater:`'s error
/// comes back through the engine's `start()` as Sparkle's `NSError`, and
/// nothing is scheduled.
fn start_fails_closed_without_a_signing_key() {
    let host = HostBundle::new();
    let (coordinator, _) = make_coordinator();
    let updater = make_sparkle_updater(&host, &coordinator);
    let error = updater.start().expect_err("no SUPublicEDKey, not code signed");
    assert_eq!(error.domain().to_string(), sparkle_string_constant(c"SUSparkleErrorDomain").to_string());
    assert_eq!(error.code(), 1, "SUNoPublicDSAFoundError");
    assert!(error.localizedDescription().to_string().contains("EdDSA"), "{}", error.localizedDescription());
    assert!(!updater.can_check_for_updates());
}

// MARK: - SUAppcastItem

/// `UpdateMetadata(appcastItem:)` reads Sparkle's item property for
/// property.
fn appcast_items_map_to_update_metadata() {
    assert_eq!(UpdateMetadata::from_appcast_item(&*sample_item()), sample_metadata());
    assert_eq!(UpdateMetadata::from_appcast_item(&*informational_item()), informational_metadata());

    let item = appcast_item(vec![
        ("sparkle:version", Value::Text("51")),
        // The RSS parser keeps an element with attributes as a dictionary.
        (
            "sparkle:releaseNotesLink",
            Value::Dictionary(vec![("content", Value::Text("https://updates.example.test/notes/51.html"))]),
        ),
        ("sparkle:minimumSystemVersion", Value::Text("14.0")),
        ("sparkle:criticalUpdate", Value::Dictionary(Vec::new())),
        (
            "enclosure",
            Value::Dictionary(vec![
                ("url", Value::Text("https://updates.example.test/Upleft-51.zip")),
                ("length", Value::Text("123")),
                ("sparkle:shortVersionString", Value::Text("1.3.0")),
            ]),
        ),
    ]);
    let metadata = UpdateMetadata::from_appcast_item(&*item);
    assert_eq!(metadata.version_string, "51");
    assert_eq!(metadata.display_version_string, "1.3.0");
    assert_eq!(metadata.title, None);
    assert_eq!(metadata.item_description, None);
    assert_eq!(metadata.release_notes_url, Url::from_string("https://updates.example.test/notes/51.html"));
    assert_eq!(metadata.info_url, None);
    assert_eq!(metadata.content_length, 123);
    assert!(!metadata.is_information_only);
    assert!(!metadata.is_major_upgrade);
    assert!(metadata.is_critical);
    assert_eq!(metadata.minimum_system_version.as_deref(), Some("14.0"));
}

// MARK: - SPUUserDriver (UpdateCoordinatorFlowTests through Sparkle)

/// `show(_:reply:)` adopts the spec defaults: check automatically, leave
/// downloading alone, send no profile.
fn permission_request_adopts_the_spec_defaults() {
    let (_coordinator, driver) = make_bridge();
    let request =
        SPUUpdatePermissionRequest::initWithSystemProfile(SPUUpdatePermissionRequest::alloc(), &NSArray::new());
    let responses = Rc::new(RefCell::new(Vec::new()));
    let log = responses.clone();
    let reply = RcBlock::new(move |response: NonNull<SUUpdatePermissionResponse>| {
        // SAFETY: Sparkle's reply receives a live response.
        let response = unsafe { response.as_ref() };
        log.borrow_mut().push((
            response.automaticUpdateChecks(),
            response.automaticUpdateDownloading().map(|value| value.boolValue()),
            response.sendSystemProfile(),
        ));
    });
    // SAFETY: `SPUUserDriver`'s selector and argument types.
    let () = unsafe { msg_send![&*driver, showUpdatePermissionRequest: &*request, reply: &*reply] };
    assert_eq!(*responses.borrow(), [(true, None, false)]);
}

fn install_reply_is_exactly_once() {
    let (coordinator, driver) = make_bridge();
    let replies = Rc::new(RefCell::new(Vec::new()));
    show_update_found(&driver, &sample_item(), STAGE_NOT_DOWNLOADED, true, &choice_block(&replies));
    assert_eq!(coordinator.phase(), UpdatePhase::Available(sample_metadata(), UpdateStage::NotDownloaded));
    coordinator.user_did_choose_install();
    coordinator.user_did_choose_install(); // second click must be a no-op
    assert_eq!(*replies.borrow(), [CHOICE_INSTALL]);
}

fn skip_reply_is_exactly_once() {
    let (coordinator, driver) = make_bridge();
    let replies = Rc::new(RefCell::new(Vec::new()));
    show_update_found(&driver, &sample_item(), STAGE_NOT_DOWNLOADED, true, &choice_block(&replies));
    coordinator.user_did_choose_skip();
    coordinator.user_did_choose_skip();
    assert_eq!(*replies.borrow(), [CHOICE_SKIP]);
}

fn later_reply_is_exactly_once() {
    let (coordinator, driver) = make_bridge();
    let replies = Rc::new(RefCell::new(Vec::new()));
    show_update_found(&driver, &sample_item(), STAGE_NOT_DOWNLOADED, true, &choice_block(&replies));
    coordinator.user_did_choose_later();
    coordinator.user_did_choose_later();
    assert_eq!(*replies.borrow(), [CHOICE_DISMISS]);
}

fn ready_to_relaunch_uses_its_own_reply() {
    let (coordinator, driver) = make_bridge();
    let ready_replies = Rc::new(RefCell::new(Vec::new()));
    let found_replies = Rc::new(RefCell::new(Vec::new()));
    show_update_found(&driver, &sample_item(), STAGE_NOT_DOWNLOADED, true, &choice_block(&found_replies));
    // A stale found-update reply is replaced when the ready reply arrives.
    let ready = choice_block(&ready_replies);
    // SAFETY: `SPUUserDriver`'s selector and argument types.
    let () = unsafe { msg_send![&*driver, showReadyToInstallAndRelaunch: &*ready] };
    coordinator.user_did_choose_install();
    assert_eq!(*ready_replies.borrow(), [CHOICE_INSTALL]);
    assert!(found_replies.borrow().is_empty());
}

fn error_acknowledgement_is_exactly_once() {
    let (coordinator, driver) = make_bridge();
    let acks = Rc::new(Cell::new(0));
    let error = ns_error("sparkle", 2001, None);
    let acknowledgement = counting_block(&acks);
    // SAFETY: `SPUUserDriver`'s selector and argument types.
    let () = unsafe { msg_send![&*driver, showUpdaterError: &*error, acknowledgement: &*acknowledgement] };
    coordinator.user_did_dismiss_panel();
    coordinator.user_did_dismiss_panel();
    assert_eq!(acks.get(), 1);
}

/// Also checks the port's `SPUNoUpdateFoundUserInitiatedKey` against
/// Sparkle's exported constant.
fn not_found_acknowledgement_is_exactly_once() {
    let key = sparkle_string_constant(c"SPUNoUpdateFoundUserInitiatedKey");
    assert_eq!(key.to_string(), SPU_NO_UPDATE_FOUND_USER_INITIATED_KEY);
    let (coordinator, driver) = make_bridge();
    let acks = Rc::new(Cell::new(0));
    let user_initiated = NSNumber::new_bool(true);
    let error = ns_error("SUSparkleErrorDomain", 1001, Some((&key, user_initiated.as_ref())));
    let acknowledgement = counting_block(&acks);
    // SAFETY: `SPUUserDriver`'s selector and argument types.
    let () = unsafe { msg_send![&*driver, showUpdateNotFoundWithError: &*error, acknowledgement: &*acknowledgement] };
    assert_eq!(coordinator.phase(), UpdatePhase::UpToDate, "the user info says the user asked");
    // The result stays visible until the user dismisses it; a second
    // dismissal must not acknowledge Sparkle twice.
    assert_eq!(acks.get(), 0);
    coordinator.user_did_choose_later();
    coordinator.user_did_dismiss_panel();
    assert_eq!(acks.get(), 1);

    // Without the key, the check was automatic and the result is silent.
    let (coordinator, driver) = make_bridge();
    let error = ns_error("SUSparkleErrorDomain", 1001, None);
    let acknowledgement = nothing_block();
    // SAFETY: as above.
    let () = unsafe { msg_send![&*driver, showUpdateNotFoundWithError: &*error, acknowledgement: &*acknowledgement] };
    assert_eq!(coordinator.phase(), UpdatePhase::Idle);
}

fn check_cancellation_is_exactly_once() {
    let (coordinator, driver) = make_bridge();
    let cancels = Rc::new(Cell::new(0));
    let cancellation = counting_block(&cancels);
    // SAFETY: `SPUUserDriver`'s selector and argument types.
    let () = unsafe { msg_send![&*driver, showUserInitiatedUpdateCheckWithCancellation: &*cancellation] };
    assert_eq!(coordinator.phase(), UpdatePhase::Checking { user_initiated: true });
    coordinator.user_did_cancel_check();
    coordinator.user_did_cancel_check();
    assert_eq!(cancels.get(), 1);
}

fn download_cancellation_is_exactly_once() {
    let (coordinator, driver) = make_bridge();
    let cancels = Rc::new(Cell::new(0));
    let cancellation = counting_block(&cancels);
    // SAFETY: `SPUUserDriver`'s selectors and argument types.
    unsafe {
        let () = msg_send![&*driver, showDownloadInitiatedWithCancellation: &*cancellation];
        let () = msg_send![&*driver, showDownloadDidReceiveExpectedContentLength: 1_000u64];
        let () = msg_send![&*driver, showDownloadDidReceiveDataOfLength: 250u64];
    }
    assert_eq!(coordinator.phase(), UpdatePhase::Downloading { received: 250, expected: Some(1_000) });
    coordinator.user_did_cancel_download();
    coordinator.user_did_cancel_download();
    assert_eq!(cancels.get(), 1);
}

fn retry_termination_is_exactly_once() {
    let (coordinator, driver) = make_bridge();
    let retries = Rc::new(Cell::new(0));
    let retry = counting_block(&retries);
    // SAFETY: `SPUUserDriver`'s selector and argument types.
    let () = unsafe {
        msg_send![&*driver, showInstallingUpdateWithApplicationTerminated: false, retryTerminatingApplication: &*retry]
    };
    coordinator.user_did_retry_termination();
    coordinator.user_did_retry_termination();
    assert_eq!(retries.get(), 1);
}

/// An automatically-scheduled presentation must not open the panel; a
/// user-initiated one must. The stage and the flag come from Sparkle's
/// `SPUUserUpdateState`.
fn background_update_does_not_open_the_panel() {
    let (coordinator, driver) = make_bridge();
    show_update_found(&driver, &sample_item(), STAGE_DOWNLOADED, false, &choice_block(&Rc::default()));
    assert_eq!(coordinator.panel_show_count(), 0);
    assert_eq!(coordinator.phase(), UpdatePhase::Available(sample_metadata(), UpdateStage::Downloaded));
    assert!(matches!(
        coordinator.pill_model(),
        Some(UpdatePillModel::UpdateNow { ref version, is_ready: true }) if version == "1.1.0"
    ));

    show_update_found(&driver, &sample_item(), STAGE_NOT_DOWNLOADED, true, &choice_block(&Rc::default()));
    assert!(coordinator.panel_show_count() > 0);
}

/// A stage from a newer Sparkle is treated as not downloaded
/// (`@unknown default`).
fn an_unknown_stage_is_not_downloaded() {
    let (coordinator, driver) = make_bridge();
    show_update_found(&driver, &sample_item(), 7, false, &choice_block(&Rc::default()));
    assert_eq!(coordinator.phase(), UpdatePhase::Available(sample_metadata(), UpdateStage::NotDownloaded));
}

fn informational_update_never_offers_install() {
    let (coordinator, driver) = make_bridge();
    let replies = Rc::new(RefCell::new(Vec::new()));
    show_update_found(&driver, &informational_item(), STAGE_NOT_DOWNLOADED, true, &choice_block(&replies));
    assert_eq!(coordinator.phase(), UpdatePhase::Informational(informational_metadata()));
    coordinator.user_did_choose_later();
    assert_eq!(*replies.borrow(), [CHOICE_DISMISS]);
}

fn release_notes_never_leak_into_the_next_update_cycle() {
    let (coordinator, driver) = make_bridge();
    show_update_found(&driver, &sample_item(), STAGE_NOT_DOWNLOADED, true, &choice_block(&Rc::default()));
    let notes = download_data(b"v1 notes");
    // SAFETY: `SPUUserDriver`'s selectors and argument types.
    let () = unsafe { msg_send![&*driver, showUpdateReleaseNotesWithDownloadData: &*notes] };
    assert_eq!(coordinator.release_notes(), UpdateReleaseNotesState::Loaded(b"v1 notes".to_vec()));

    let () = unsafe { msg_send![&*driver, dismissUpdateInstallation] };
    assert_eq!(coordinator.release_notes(), UpdateReleaseNotesState::None);
    let next = appcast_item(vec![
        ("sparkle:version", Value::Text("48")),
        ("sparkle:shortVersionString", Value::Text("1.1.1")),
        ("enclosure", Value::Dictionary(vec![("url", Value::Text("https://updates.example.test/Upleft-1.1.1.zip"))])),
    ]);
    show_update_found(&driver, &next, STAGE_NOT_DOWNLOADED, true, &choice_block(&Rc::default()));
    assert_eq!(coordinator.release_notes(), UpdateReleaseNotesState::None);

    let error = ns_error("NSURLErrorDomain", -1009, None);
    // SAFETY: as above.
    let () = unsafe { msg_send![&*driver, showUpdateReleaseNotesFailedToDownloadWithError: &*error] };
    assert_eq!(coordinator.release_notes(), UpdateReleaseNotesState::Failed);
}

/// Every remaining callback, in the order Sparkle drives a user-initiated
/// install, lands on the coordinator's machine.
fn a_full_cycle_through_the_user_driver() {
    let (coordinator, driver) = make_bridge();
    let replies = Rc::new(RefCell::new(Vec::new()));
    let cancellation = nothing_block();
    let retry = nothing_block();
    let acks = Rc::new(Cell::new(0));
    let acknowledgement = counting_block(&acks);
    // SAFETY: `SPUUserDriver`'s selectors and argument types.
    unsafe {
        let () = msg_send![&*driver, showUserInitiatedUpdateCheckWithCancellation: &*cancellation];
        show_update_found(&driver, &sample_item(), STAGE_NOT_DOWNLOADED, true, &choice_block(&replies));
        coordinator.user_did_choose_install();
        assert_eq!(*replies.borrow(), [CHOICE_INSTALL]);
        let () = msg_send![&*driver, showDownloadInitiatedWithCancellation: &*cancellation];
        let () = msg_send![&*driver, showDownloadDidReceiveExpectedContentLength: 4_000_000u64];
        let () = msg_send![&*driver, showDownloadDidReceiveDataOfLength: 4_000_000u64];
        let () = msg_send![&*driver, showDownloadDidStartExtractingUpdate];
        let () = msg_send![&*driver, showExtractionReceivedProgress: 0.5f64];
        assert_eq!(coordinator.phase(), UpdatePhase::Extracting { progress: Some(0.5) });
        let ready = choice_block(&replies);
        let () = msg_send![&*driver, showReadyToInstallAndRelaunch: &*ready];
        assert_eq!(coordinator.phase(), UpdatePhase::ReadyToRelaunch);
        let () = msg_send![&*driver, showInstallingUpdateWithApplicationTerminated: true, retryTerminatingApplication: &*retry];
        assert_eq!(coordinator.phase(), UpdatePhase::Installing);
        let () = msg_send![&*driver, showUpdateInstalledAndRelaunched: true, acknowledgement: &*acknowledgement];
        assert_eq!(coordinator.phase(), UpdatePhase::Idle);
        assert_eq!(acks.get(), 1, "an installed update is acknowledged at once");
        let shown = coordinator.panel_show_count();
        let () = msg_send![&*driver, showUpdateInFocus];
        assert_eq!(coordinator.panel_show_count(), shown + 1);
    }
}

/// With its host gone the driver stops forwarding, and Sparkle's blocks are
/// released uncalled.
fn a_detached_driver_stops_forwarding() {
    let driver = DownrightUpdateDriverObject::new(mtm(), DownrightUpdateDriver::new(None));
    let replies = Rc::new(RefCell::new(Vec::new()));
    let reply = choice_block(&replies);
    show_update_found(&driver, &sample_item(), STAGE_NOT_DOWNLOADED, true, &reply);
    let cancels = Rc::new(Cell::new(0));
    let cancellation = counting_block(&cancels);
    // SAFETY: `SPUUserDriver`'s selector and argument types.
    let () = unsafe { msg_send![&*driver, showDownloadInitiatedWithCancellation: &*cancellation] };
    assert!(replies.borrow().is_empty());
    assert_eq!(cancels.get(), 0);
    // SAFETY: reads the blocks' reference counts.
    assert_eq!(unsafe { block_retain_count(&reply) }, 1, "the driver keeps no copy of the reply");
}

/// `Block_layout`'s flags: the reference count is in bits 1…15.
unsafe fn block_retain_count<F: ?Sized>(block: &RcBlock<F>) -> i32 {
    let layout = &**block as *const block2::Block<F> as *const i32;
    // SAFETY: a block starts with its isa, then its 32-bit flags.
    let flags = unsafe { *layout.byte_add(std::mem::size_of::<*const c_void>()) };
    (flags & 0xfffe) >> 1
}

// MARK: - SPUUpdaterDelegate

/// The coordinator's background-download handler, behind the notifier the
/// engine would hand Sparkle, recording which thread it ran on.
fn make_notifier(
    engine: &FakeUpdateEngine,
    threads: &Rc<RefCell<Vec<bool>>>,
) -> Retained<BackgroundDownloadNotifierObject> {
    let handler = engine.on_background_download_completed().expect("the coordinator's handler");
    let threads = threads.clone();
    let notifier = Rc::new(BackgroundDownloadNotifier::default());
    *notifier.handler.borrow_mut() = Some(Rc::new(move |version: &str| {
        threads.borrow_mut().push(is_main_thread());
        handler(version);
    }));
    BackgroundDownloadNotifierObject::new(notifier)
}

/// `updater:didDownloadUpdate:` on the main thread reaches the pill at once.
fn background_download_drives_update_now_pill() {
    let host = HostBundle::new();
    let (coordinator, engine) = make_coordinator();
    let updater = make_sparkle_updater(&host, &coordinator);
    let threads = Rc::new(RefCell::new(Vec::new()));
    let notifier = make_notifier(&engine, &threads);
    assert!(coordinator.pill_model().is_none());
    // SAFETY: `SPUUpdaterDelegate`'s selector and argument types.
    let () = unsafe { msg_send![&*notifier, updater: updater.updater(), didDownloadUpdate: &*sample_item()] };
    assert!(matches!(
        coordinator.pill_model(),
        Some(UpdatePillModel::UpdateNow { ref version, is_ready: true }) if version == "1.1.0"
    ));
    assert!(coordinator.downloaded_update().is_some());
    assert_eq!(*threads.borrow(), [true]);
    coordinator.driver_did_dismiss();
    assert!(coordinator.downloaded_update().is_none());
    assert!(coordinator.pill_model().is_none());
}

/// Raw Objective-C pointers handed to another thread; the main thread keeps
/// the objects alive until it has joined.
struct Pointers(Vec<NonNull<AnyObject>>);

// SAFETY: the objects are Objective-C objects the main thread keeps alive
// for the other thread's whole life; only messages the receivers accept on
// any thread are sent.
unsafe impl Send for Pointers {}

fn object<T: objc2::Message>(value: &T) -> NonNull<AnyObject> {
    NonNull::from(value).cast()
}

/// Sparkle may call its delegate off the main thread. Swift's handler hops
/// to the main actor; the bridge carries the call to the main queue, so the
/// coordinator learns of the download on a later main-thread turn.
fn a_background_download_from_another_thread_lands_on_the_main_thread() {
    let host = HostBundle::new();
    let (coordinator, engine) = make_coordinator();
    let updater = make_sparkle_updater(&host, &coordinator);
    let threads = Rc::new(RefCell::new(Vec::new()));
    let notifier = make_notifier(&engine, &threads);
    let item = sample_item();
    let pointers = Pointers(vec![object(&*notifier), object(updater.updater()), object(&*item)]);
    std::thread::spawn(move || {
        let pointers = pointers;
        let [notifier, updater, item] = pointers.0[..] else { unreachable!() };
        // SAFETY: the objects are alive (see `Pointers`); the delegate
        // method accepts calls on any thread.
        unsafe {
            let () = msg_send![notifier.as_ref(), updater: updater.as_ref(), didDownloadUpdate: item.as_ref()];
        }
    })
    .join()
    .unwrap();
    assert!(coordinator.pill_model().is_none(), "nothing reached the coordinator off the main thread");
    assert!(threads.borrow().is_empty());
    assert!(pump(|| coordinator.pill_model().is_some(), Duration::from_secs(5)), "the hop never landed");
    assert!(matches!(
        coordinator.pill_model(),
        Some(UpdatePillModel::UpdateNow { ref version, is_ready: true }) if version == "1.1.0"
    ));
    assert_eq!(*threads.borrow(), [true]);
}

/// Sparkle promises to call its user driver on the main thread; should a
/// call arrive elsewhere, the bridge delivers it on the main queue rather
/// than touch the driver off the main thread.
fn a_user_driver_call_from_another_thread_lands_on_the_main_thread() {
    let (coordinator, driver) = make_bridge();
    let cancellation = nothing_block();
    // SAFETY: `SPUUserDriver`'s selector and argument types.
    let () = unsafe { msg_send![&*driver, showDownloadInitiatedWithCancellation: &*cancellation] };
    let pointers = Pointers(vec![object(&*driver)]);
    std::thread::spawn(move || {
        let pointers = pointers;
        // SAFETY: the driver is alive (see `Pointers`).
        unsafe {
            let () = msg_send![pointers.0[0].as_ref(), showDownloadDidReceiveDataOfLength: 64u64];
        }
    })
    .join()
    .unwrap();
    assert_eq!(coordinator.phase(), UpdatePhase::Downloading { received: 0, expected: None });
    assert!(pump(
        || coordinator.phase() == UpdatePhase::Downloading { received: 64, expected: None },
        Duration::from_secs(5)
    ));
}

// MARK: - Runner

fn main() {
    macro_rules! tests {
        ($($suite:literal / $name:literal => $function:ident),* $(,)?) => {
            vec![$(Test { name: concat!($suite, "/", $name), run: $function }),*]
        };
    }
    let mut tests = tests![
        "UpdateBuildContractTests" / "sparkleIsImportedOnlyByTheHostApp" => sparkle_is_imported_only_by_the_host_app,
        "UpdateBuildContractTests" / "packageAndXcodeProjectDeclareSparkleExactly" => package_and_xcode_project_declare_sparkle_exactly,
        "SparkleBridge" / "factoryAnswersNoneWithoutTheFramework" => factory_answers_none_without_the_framework,
    ];
    // Each of these loads Sparkle first.
    macro_rules! with_sparkle {
        ($($suite:literal / $name:literal => $function:ident),* $(,)?) => {
            vec![$(Test {
                name: concat!($suite, "/", $name),
                run: {
                    fn run() {
                        load_sparkle();
                        $function()
                    }
                    run
                },
            }),*]
        };
    }
    let with_sparkle = with_sparkle![
        "SparkleBridge" / "loadsSparkle296" => loads_sparkle_2_9_6,
        "SparkleBridge" / "aDevBundleStaysDisabledWithSparkleLoaded" => a_dev_bundle_stays_disabled_with_sparkle_loaded,
        "SparkleBridge" / "bridgeClassesConformToSparklesProtocols" => bridge_classes_conform_to_sparkles_protocols,
        "SparkleBridge" / "theUpdaterIsWiredToTheDriverAndNotifier" => the_updater_is_wired_to_the_driver_and_notifier,
        "SparkleBridge" / "settingsReadAndWriteSparklesOwnDefaults" => settings_read_and_write_sparkles_own_defaults,
        "SparkleBridge" / "startFailsClosedWithoutASigningKey" => start_fails_closed_without_a_signing_key,
        "SparkleBridge" / "appcastItemsMapToUpdateMetadata" => appcast_items_map_to_update_metadata,
        "SparkleBridge" / "permissionRequestAdoptsTheSpecDefaults" => permission_request_adopts_the_spec_defaults,
        "UpdateCoordinatorFlowTests" / "installReplyIsExactlyOnce" => install_reply_is_exactly_once,
        "UpdateCoordinatorFlowTests" / "skipReplyIsExactlyOnce" => skip_reply_is_exactly_once,
        "UpdateCoordinatorFlowTests" / "laterReplyIsExactlyOnce" => later_reply_is_exactly_once,
        "UpdateCoordinatorFlowTests" / "readyToRelaunchUsesItsOwnReply" => ready_to_relaunch_uses_its_own_reply,
        "UpdateCoordinatorFlowTests" / "errorAcknowledgementIsExactlyOnce" => error_acknowledgement_is_exactly_once,
        "UpdateCoordinatorFlowTests" / "notFoundAcknowledgementIsExactlyOnce" => not_found_acknowledgement_is_exactly_once,
        "UpdateCoordinatorFlowTests" / "checkCancellationIsExactlyOnce" => check_cancellation_is_exactly_once,
        "UpdateCoordinatorFlowTests" / "downloadCancellationIsExactlyOnce" => download_cancellation_is_exactly_once,
        "UpdateCoordinatorFlowTests" / "retryTerminationIsExactlyOnce" => retry_termination_is_exactly_once,
        "UpdateCoordinatorFlowTests" / "backgroundUpdateDoesNotOpenThePanel" => background_update_does_not_open_the_panel,
        "UpdateCoordinatorFlowTests" / "informationalUpdateNeverOffersInstall" => informational_update_never_offers_install,
        "UpdateCoordinatorFlowTests" / "releaseNotesNeverLeakIntoTheNextUpdateCycle" => release_notes_never_leak_into_the_next_update_cycle,
        "UpdateCoordinatorFlowTests" / "backgroundDownloadDrivesUpdateNowPill" => background_download_drives_update_now_pill,
        "SparkleBridge" / "anUnknownStageIsNotDownloaded" => an_unknown_stage_is_not_downloaded,
        "SparkleBridge" / "aFullCycleThroughTheUserDriver" => a_full_cycle_through_the_user_driver,
        "SparkleBridge" / "aDetachedDriverStopsForwarding" => a_detached_driver_stops_forwarding,
        "SparkleBridge" / "aBackgroundDownloadFromAnotherThreadLandsOnTheMainThread" => a_background_download_from_another_thread_lands_on_the_main_thread,
        "SparkleBridge" / "aUserDriverCallFromAnotherThreadLandsOnTheMainThread" => a_user_driver_call_from_another_thread_lands_on_the_main_thread,
    ];
    let mut skipped = Vec::new();
    match framework_path() {
        Some(path) => {
            FRAMEWORK.set(path).unwrap();
            tests.extend(with_sparkle);
        }
        None => {
            println!(
                "sparkle_bridge_tests: Sparkle.framework not found (UPLEFT_SPARKLE_FRAMEWORK, or {DEFAULT_FRAMEWORK}); \
                 run scripts/sparkle-framework.sh to fetch it. Skipping the {} tests that need it.",
                with_sparkle.len()
            );
            skipped.extend(with_sparkle.iter().map(|test| Skipped {
                name: test.name,
                reason: "Sparkle.framework not found; run scripts/sparkle-framework.sh",
            }));
        }
    }
    updater_support::run("sparkle_bridge_tests", &tests, &skipped);
}
