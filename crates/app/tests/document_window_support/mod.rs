//! Shared set-up for the test binaries that build a
//! `DocumentWindowController`.
//!
//! Sandbox. The controller reads `Preferences.shared`, `ThemeStore.shared`
//! (which reads its selection from `UserDefaults.standard`), and every
//! document it opens goes through `DocumentStateStore.shared` and
//! `SnapshotStore.shared` in the support folder. So [`enter_sandbox`]
//! re-runs the binary with `HOME` and `CFFIXED_USER_HOME` pointing at a
//! temporary folder and Downright's own `DOWNRIGHT_SUPPORT_DIRECTORY`
//! override inside it (all read once, before anything else), installs a
//! sandboxed `Preferences` as `Preferences::shared()` (the real one
//! publishes the Quick Look appearance to the user's global preferences
//! domain), and removes the test process's own `UserDefaults` domain before
//! and after.
//!
//! Windows. No test orders a window in: the document window is titled, and
//! AppKit would pull a titled window onto a screen. Geometry is read from the
//! laid-out, never-shown window, as the Swift tests read it.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSBundle, NSProcessInfo, NSString, NSUserDefaults};
use upleft_app::support::preferences::Preferences;
use upleft_foundation::url::FileUrl;

/// Set (to the sandbox root) in the re-run child.
const SANDBOX_VARIABLE: &str = "UPLEFT_DOCUMENT_WINDOW_TESTS_SANDBOX";

pub fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("document window tests run on the main thread")
}

/// Call first thing in `main`. In the parent, runs this binary again inside
/// a fresh sandbox and exits with its status; in the child, finishes the
/// sandbox (shared application, `Preferences.shared`, a clean defaults
/// domain) and returns.
pub fn enter_sandbox() {
    if std::env::var_os(SANDBOX_VARIABLE).is_none() {
        std::process::exit(run_in_sandbox());
    }
    let root = std::env::var(SANDBOX_VARIABLE).expect("the sandbox root");
    let home = objc2_foundation::NSHomeDirectory().to_string();
    assert!(home.starts_with(&root), "Foundation's home ({home}) must be the sandbox's, under {root}");
    // Windows want the shared application to exist; it is never activated.
    let _ = NSApplication::sharedApplication(mtm());
    remove_standard_domain();
    let support = std::env::var("DOWNRIGHT_SUPPORT_DIRECTORY").expect("the sandbox sets the support folder");
    let preferences_file = FileUrl::from_path(&format!("{support}/preferences.json"));
    assert!(
        Preferences::install_shared(Preferences::for_testing(preferences_file, None)),
        "nothing may read Preferences.shared before the sandbox installs it"
    );
    // The fragment layer reaches Mermaid through a hook the host installs,
    // as the app does at launch.
    upleft_mermaid::downright::mermaid_renderer_bridge::install_fragment_renderer();
}

/// Call last in `main` (the child).
pub fn leave_sandbox() {
    remove_standard_domain();
}

fn standard_domain_name() -> String {
    NSBundle::mainBundle()
        .bundleIdentifier()
        .map(|identifier| identifier.to_string())
        .unwrap_or_else(|| NSProcessInfo::processInfo().processName().to_string())
}

/// `UserDefaults` ignores `CFFIXED_USER_HOME`: the domain's plist lives in
/// the real home.
fn real_preferences_file(domain: &str) -> Option<PathBuf> {
    // SAFETY: reads the password database entry of the current user.
    let entry = unsafe { libc::getpwuid(libc::getuid()) };
    if entry.is_null() {
        return None;
    }
    // SAFETY: `pw_dir` is a NUL-terminated string owned by the entry.
    let home = unsafe { std::ffi::CStr::from_ptr((*entry).pw_dir) }.to_string_lossy().into_owned();
    Some(Path::new(&home).join("Library/Preferences").join(format!("{domain}.plist")))
}

fn remove_standard_domain() {
    let domain = standard_domain_name();
    let defaults = NSUserDefaults::standardUserDefaults();
    defaults.removePersistentDomainForName(&NSString::from_str(&domain));
    #[allow(deprecated)]
    defaults.synchronize();
}

fn run_in_sandbox() -> i32 {
    let root = std::env::temp_dir()
        .join(format!("upleft-document-window-tests-{}", objc2_foundation::NSUUID::UUID().UUIDString()));
    let home = root.join("home");
    let support = root.join("support");
    std::fs::create_dir_all(&home).expect("create the sandbox home");
    std::fs::create_dir_all(&support).expect("create the sandbox support folder");
    let status = std::process::Command::new(std::env::current_exe().expect("the test binary's path"))
        .args(std::env::args_os().skip(1))
        .env("HOME", &home)
        .env("CFFIXED_USER_HOME", &home)
        .env("DOWNRIGHT_SUPPORT_DIRECTORY", &support)
        .env(SANDBOX_VARIABLE, &root)
        .status();
    remove_standard_domain();
    if let Some(file) = real_preferences_file(&standard_domain_name()) {
        let _ = std::fs::remove_file(file);
    }
    let _ = std::fs::remove_dir_all(&root);
    match status {
        Ok(status) => status.code().unwrap_or(101),
        Err(error) => {
            eprintln!("could not run the sandboxed tests: {error}");
            101
        }
    }
}

/// A folder of files under the sandbox's temporary directory, removed on
/// drop (Swift's `Fixture` / `makeDocument`).
pub struct Fixture {
    pub root: FileUrl,
}

impl Fixture {
    pub fn new(prefix: &str) -> Fixture {
        let root = FileUrl::from_path_is_directory(
            &format!(
                "{}{prefix}-{}",
                objc2_foundation::NSTemporaryDirectory(),
                objc2_foundation::NSUUID::UUID().UUIDString()
            ),
            true,
        );
        std::fs::create_dir_all(root.path()).expect("create the fixture folder");
        Fixture { root }
    }

    pub fn write(&self, name: &str, contents: &str) -> FileUrl {
        let url = self.root.appending_path_component(name);
        std::fs::write(url.path(), contents).expect("write a fixture file");
        url
    }

    pub fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.root.appending_path_component(name).path()).expect("read a fixture file")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.root.path());
    }
}
