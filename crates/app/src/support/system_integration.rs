//! Port of `Sources/DownrightApp/Support/SystemIntegration.swift`.
//!
//! Everything Downright has to tell the rest of macOS about itself: which files
//! it opens, that its Quick Look extensions exist, and where `down` lives.
//!
//! The app performs its own installation (the DMG-and-drag user never runs
//! `Scripts/install.sh`); the script stays for the source path and calls the
//! same steps in the same order.
//!
//! Threading follows the Swift file: the subprocess-driven registration runs
//! on a global queue and reports back on the main queue; callbacks that Swift
//! runs on the main actor take a [`MainThreadMarker`] here and are delivered
//! on the main queue.

use std::collections::HashSet;
use std::io::Read;
use std::process::{Command as Process, Stdio};

use block2::RcBlock;
use dispatch2::{DispatchQoS, DispatchQueue, GlobalQueueIdentifier, MainThreadBound};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_app_kit::{NSApplication, NSRunningApplication, NSWorkspace, NSWorkspaceOpenConfiguration};
use objc2_foundation::{
    NSBundle, NSCocoaErrorDomain, NSDirectoryEnumerationOptions, NSError, NSFileManager, NSObjectProtocol, NSString,
    NSURL,
};
use objc2_uniform_type_identifiers::UTType;
use upleft_foundation::url::FileUrl;
use upleft_swift_text as swift_text;

/// An `NSError` as Swift code observes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CocoaError {
    pub domain: String,
    pub code: isize,
    pub localized_description: String,
}

impl CocoaError {
    pub fn from_ns_error(error: &NSError) -> CocoaError {
        CocoaError {
            domain: error.domain().to_string(),
            code: error.code(),
            localized_description: error.localizedDescription().to_string(),
        }
    }

    /// `CocoaError(code)`, with Foundation's own description.
    pub fn cocoa(code: isize) -> CocoaError {
        let error = unsafe { NSError::errorWithDomain_code_userInfo(NSCocoaErrorDomain, code, None) };
        CocoaError::from_ns_error(&error)
    }

    /// `CocoaError(.fileWriteNoPermission)`.
    pub fn file_write_no_permission() -> CocoaError {
        CocoaError::cocoa(513)
    }
}

impl std::fmt::Display for CocoaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.localized_description)
    }
}

impl std::error::Error for CocoaError {}

fn manager() -> Retained<NSFileManager> {
    NSFileManager::defaultManager()
}

fn ns(value: &str) -> Retained<NSString> {
    NSString::from_str(value)
}

/// `Bundle.main.bundleURL`.
fn bundle_url() -> FileUrl {
    let url = NSBundle::mainBundle().bundleURL();
    FileUrl::from_nsurl(&url).unwrap_or_else(|| FileUrl::from_path(&url.path().map(|p| p.to_string()).unwrap_or_default()))
}

/// `FileManager.default.homeDirectoryForCurrentUser`.
fn home_directory() -> FileUrl {
    let url = manager().homeDirectoryForCurrentUser();
    FileUrl::from_nsurl(&url).unwrap_or_else(|| FileUrl::from_path(&objc2_foundation::NSHomeDirectory().to_string()))
}

fn file_exists(path: &str) -> bool {
    manager().fileExistsAtPath(&ns(path))
}

fn is_writable_file(path: &str) -> bool {
    manager().isWritableFileAtPath(&ns(path))
}

fn is_executable_file(path: &str) -> bool {
    manager().isExecutableFileAtPath(&ns(path))
}

/// `FileManager.default.destinationOfSymbolicLink(atPath:)`.
fn destination_of_symbolic_link(path: &str) -> Option<String> {
    manager().destinationOfSymbolicLinkAtPath_error(&ns(path)).ok().map(|destination| destination.to_string())
}

pub struct SystemIntegration;

/// `SystemIntegration.CommandLineTarget`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandLineTarget {
    pub directory: FileUrl,
    /// False when the directory works but no login shell will look in it
    /// without the user editing their PATH — which the panel then says out
    /// loud rather than reporting a success the terminal will contradict.
    pub is_on_path: bool,
}

/// `SystemIntegration.CommandLineResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandLineResult {
    pub directory: FileUrl,
    pub linked: Vec<String>,
    /// Names left alone because something that is not ours already owns
    /// them. Clobbering a stranger's `md` is not ours to do.
    pub skipped: Vec<String>,
    pub is_on_path: bool,
}

const LSREGISTER_PATH: &str =
    "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

impl SystemIntegration {
    // MARK: - Where the app is running from

    /// Gatekeeper runs an app launched straight from a DMG or from Downloads
    /// out of a randomised, read-only mount. Every step is gated on the app
    /// living somewhere permanent.
    pub fn is_translocated() -> bool {
        swift_text::contains_bridged(&bundle_url().path(), "/AppTranslocation/")
    }

    /// False for a bare `swift build` (or `cargo`) binary, whose "bundle" is a
    /// build directory.
    pub fn is_app_bundle() -> bool {
        bundle_url().path_extension() == "app"
    }

    pub fn is_in_applications_folder() -> bool {
        let path = bundle_url().resolving_symlinks_in_path().path();
        let home = home_directory().path();
        swift_text::has_prefix(&path, "/Applications/") || swift_text::has_prefix(&path, &format!("{home}/Applications/"))
    }

    /// True when the bundle sits somewhere the rest of this file can rely on.
    pub fn is_permanently_installed() -> bool {
        !Self::is_translocated() && Self::is_in_applications_folder()
    }

    /// Where a move would put the app, or `None` when neither Applications
    /// folder can be written without a password.
    pub fn applications_destination() -> Option<FileUrl> {
        let home = home_directory().appending_path_component("Applications");
        for base in [FileUrl::from_path("/Applications"), home] {
            if !is_writable_file(&base.path()) {
                continue;
            }
            return Some(base.appending_path_component("Upleft.app"));
        }
        None
    }

    /// Copies the running bundle into Applications, launches the copy, and
    /// terminates this process. `on_relaunch_failure` runs, on the main
    /// thread, when the copy succeeded but the new instance would not start.
    pub fn move_to_applications(
        on_relaunch_failure: impl FnOnce(CocoaError) + 'static,
        mtm: MainThreadMarker,
    ) -> Result<(), CocoaError> {
        let Some(destination) = Self::applications_destination() else {
            return Err(CocoaError::file_write_no_permission());
        };
        let manager = manager();
        let source = bundle_url();

        if file_exists(&destination.path()) {
            // An older build is being replaced on purpose. The Trash keeps it
            // recoverable; `removeItem` would not.
            manager
                .trashItemAtURL_resultingItemURL_error(&destination.to_nsurl(), None)
                .map_err(|error| CocoaError::from_ns_error(&error))?;
        }
        manager
            .copyItemAtURL_toURL_error(&source.to_nsurl(), &destination.to_nsurl())
            .map_err(|error| CocoaError::from_ns_error(&error))?;

        let configuration = NSWorkspaceOpenConfiguration::configuration();
        configuration.setCreatesNewApplicationInstance(true);
        let on_failure = MainThreadBound::new(std::cell::Cell::new(Some(Box::new(on_relaunch_failure) as Box<dyn FnOnce(CocoaError)>)), mtm);
        let on_failure = std::sync::Mutex::new(Some(on_failure));
        let handler = RcBlock::new(move |_application: *mut NSRunningApplication, error: *mut NSError| {
            let failure = unsafe { error.as_ref() }.map(CocoaError::from_ns_error);
            let Some(bound) = on_failure.lock().ok().and_then(|mut slot| slot.take()) else { return };
            DispatchQueue::main().exec_async(move || {
                let mtm = MainThreadMarker::new().expect("the main queue runs on the main thread");
                match failure {
                    Some(failure) => {
                        if let Some(callback) = bound.get(mtm).take() {
                            callback(failure);
                        }
                    }
                    None => NSApplication::sharedApplication(mtm).terminate(None),
                }
            });
        });
        NSWorkspace::sharedWorkspace().openApplicationAtURL_configuration_completionHandler(
            &destination.to_nsurl(),
            &configuration,
            Some(&handler),
        );
        Ok(())
    }

    // MARK: - Default application

    /// The types Downright claims when the user says yes: the plain-markdown
    /// family only. `.mdx`, `.qmd`, and `.rmd` belong to toolchains people
    /// have already chosen; the bundle still declares them, so Downright
    /// appears in "Open With". `.txt` is not claimed here or anywhere.
    pub const CLAIMED_EXTENSIONS: [&'static str; 4] = ["md", "markdown", "mdown", "mkd"];

    pub fn claimed_types() -> Vec<Retained<UTType>> {
        let mut types: Vec<Retained<UTType>> = Vec::new();
        if let Some(markdown) = UTType::typeWithIdentifier(&ns("net.daringfireball.markdown")) {
            types.push(markdown);
        }
        for ext in Self::CLAIMED_EXTENSIONS {
            let Some(kind) = UTType::typeWithFilenameExtension(&ns(ext)) else { continue };
            if types.iter().any(|existing| existing.isEqual(Some(&kind))) {
                continue;
            }
            types.push(kind);
        }
        types
    }

    pub fn is_default_markdown_handler() -> bool {
        let Some(markdown) = Self::claimed_types().into_iter().next() else { return false };
        let Some(handler) = NSWorkspace::sharedWorkspace().URLForApplicationToOpenContentType(&markdown) else {
            return false;
        };
        let Some(handler) = FileUrl::from_nsurl(&handler) else { return false };
        handler.resolving_symlinks_in_path().standardized_file_url()
            == bundle_url().resolving_symlinks_in_path().standardized_file_url()
    }

    /// Routes every claimed type here, one after another, and hands the first
    /// failure (if any) to `completion` on the main thread. macOS puts its own
    /// confirmation in front of this on some releases.
    pub fn make_default_markdown_handler(completion: impl FnOnce(Option<CocoaError>) + 'static, mtm: MainThreadMarker) {
        let bundle = bundle_url().to_nsurl();
        let types: Vec<Retained<UTType>> = Self::claimed_types();
        let completion = MainThreadBound::new(Box::new(completion) as Box<dyn FnOnce(Option<CocoaError>)>, mtm);
        let step = DefaultHandlerStep { bundle: bundle.into(), types: types.into_iter().map(Into::into).collect(), index: 0, first_failure: None, completion };
        step.run();
    }

    // MARK: - Command line tool

    pub const COMMAND_LINE_NAMES: [&'static str; 2] = ["down", "md"];

    pub fn command_line_executable() -> FileUrl {
        bundle_url().appending_path_component("Contents/MacOS/down")
    }

    pub fn command_line_tool_is_bundled() -> bool {
        is_executable_file(&Self::command_line_executable().path())
    }

    /// Directories `down` might already be installed into, most conventional
    /// first. Also the search order for a new install.
    fn command_line_search_directories() -> [FileUrl; 3] {
        [
            FileUrl::from_path("/usr/local/bin"),
            FileUrl::from_path("/opt/homebrew/bin"),
            home_directory().appending_path_component(".local/bin"),
        ]
    }

    /// What `path_helper` builds a login shell's PATH from: `/etc/paths` and
    /// `/etc/paths.d`, not this process's environment (a GUI app inherits
    /// launchd's minimal PATH). Keys are NFC, as a Swift `Set<String>`
    /// compares them.
    fn login_path_directories() -> HashSet<String> {
        let mut result = HashSet::new();
        let mut absorb = |path: &str| {
            let Ok(bytes) = std::fs::read(path) else { return };
            let Ok(text) = String::from_utf8(bytes) else { return };
            for line in swift_text::split_default(&text, '\n') {
                let entry = swift_text::trim_whitespaces(line);
                if !entry.is_empty() {
                    result.insert(swift_text::string_key(entry));
                }
            }
        };
        absorb("/etc/paths");
        let fragments = manager().contentsOfDirectoryAtURL_includingPropertiesForKeys_options_error(
            &FileUrl::from_path("/etc/paths.d").to_nsurl(),
            None,
            NSDirectoryEnumerationOptions::empty(),
        );
        if let Ok(fragments) = fragments {
            for fragment in fragments.iter() {
                if let Some(path) = fragment.path() {
                    absorb(&path.to_string());
                }
            }
        }
        result
    }

    /// Somewhere `down` can go without an administrator prompt.
    pub fn command_line_destination() -> Option<CommandLineTarget> {
        let on_path = Self::login_path_directories();
        for directory in Self::command_line_search_directories() {
            let path = directory.path();
            let mut is_directory = Bool::NO;
            let exists = unsafe { manager().fileExistsAtPath_isDirectory(&ns(&path), &mut is_directory) };
            if !(exists && is_directory.as_bool() && is_writable_file(&path)) {
                continue;
            }
            let is_on_path = on_path.contains(&swift_text::string_key(&path));
            return Some(CommandLineTarget { directory, is_on_path });
        }

        // Nothing that exists is writable. `~/.local/bin` is the one place we
        // may create on the user's behalf.
        let fallback = home_directory().appending_path_component(".local/bin");
        super::app_paths::create(&fallback).ok()?;
        let is_on_path = on_path.contains(&swift_text::string_key(&fallback.path()));
        Some(CommandLineTarget { directory: fallback, is_on_path })
    }

    /// True when a `down` symlink somewhere on the search path already points
    /// into *this* bundle.
    pub fn is_command_line_tool_installed() -> bool {
        let expected = Self::command_line_executable().resolving_symlinks_in_path().path();
        for directory in Self::command_line_search_directories() {
            let link = directory.appending_path_component("down");
            let Some(destination) = destination_of_symbolic_link(&link.path()) else { continue };
            let resolved = FileUrl::from_path(&destination).resolving_symlinks_in_path().path();
            if swift_text::str_eq(&resolved, &expected) {
                return true;
            }
        }
        false
    }

    pub fn install_command_line_tool() -> Result<CommandLineResult, CocoaError> {
        let Some(target) = Self::command_line_destination() else {
            return Err(CocoaError::file_write_no_permission());
        };
        let manager = manager();
        let executable = Self::command_line_executable();
        let mut linked = Vec::new();
        let mut skipped = Vec::new();

        for name in Self::COMMAND_LINE_NAMES {
            let link = target.directory.appending_path_component(name);
            // `fileExists` follows symlinks, so it answers "no" for a link left
            // dangling by a deleted build — exactly the case that has to be
            // replaced. Ask about the link itself.
            let existing_link = destination_of_symbolic_link(&link.path());
            let occupied = existing_link.is_some() || file_exists(&link.path());

            if occupied {
                match &existing_link {
                    Some(existing) if swift_text::contains_bridged(existing, "Upleft.app/") => {}
                    _ => {
                        skipped.push(name.to_owned());
                        continue;
                    }
                }
                manager.removeItemAtURL_error(&link.to_nsurl()).map_err(|error| CocoaError::from_ns_error(&error))?;
            }
            manager
                .createSymbolicLinkAtURL_withDestinationURL_error(&link.to_nsurl(), &executable.to_nsurl())
                .map_err(|error| CocoaError::from_ns_error(&error))?;
            linked.push(name.to_owned());
        }
        Ok(CommandLineResult { directory: target.directory, linked, skipped, is_on_path: target.is_on_path })
    }

    // MARK: - Quick Look

    pub const PREVIEW_EXTENSION_IDENTIFIER: &'static str = "com.bitemyapp.upleft.quicklook";
    pub const THUMBNAIL_EXTENSION_IDENTIFIER: &'static str = "com.bitemyapp.upleft.thumbnail";

    fn plug_ins_directory() -> FileUrl {
        bundle_url().appending_path_component("Contents/PlugIns")
    }

    /// False for a dev build, which cannot produce an `.appex` at all.
    pub fn quick_look_extensions_are_bundled() -> bool {
        file_exists(&Self::plug_ins_directory().appending_path_component("DownrightQL.appex").path())
    }

    /// Whether `pluginkit` will let the preview extension run. Blocks on a
    /// subprocess: never call it on the main thread.
    pub fn is_quick_look_preview_enabled() -> bool {
        Self::is_enabled(
            &run("/usr/bin/pluginkit", &["-m", "-v", "-i", Self::PREVIEW_EXTENSION_IDENTIFIER]),
            Self::PREVIEW_EXTENSION_IDENTIFIER,
        )
    }

    /// The flag column reads backwards from the obvious: `+` means explicitly
    /// on, `-` explicitly off, and **blank — what a healthy extension shows —
    /// means available**. An unregistered identifier prints "(no matches)"
    /// and exits 0, so the answer is in the output, never the status code.
    pub fn is_enabled(listing: &str, identifier: &str) -> bool {
        let Some(line) = swift_text::split_default(listing, '\n').into_iter().find(|line| swift_text::contains(line, identifier))
        else {
            return false;
        };
        !swift_text::has_prefix(line, "-")
    }

    /// Registers the bundle with LaunchServices, offers both extensions to
    /// `pluginkit`, and reloads Quick Look, on a global queue; `completion`
    /// gets the resulting preview state on the main thread.
    ///
    /// `reset_thumbnail_cache` discards every cached thumbnail on the system,
    /// so it runs once at setup and never on an ordinary launch.
    pub fn register_with_system(
        reset_thumbnail_cache: bool,
        completion: impl FnOnce(bool) + 'static,
        mtm: MainThreadMarker,
    ) {
        let bundle_path = bundle_url().path();
        let plug_ins = Self::plug_ins_directory();
        let extensions = [
            (plug_ins.appending_path_component("DownrightQL.appex").path(), Self::PREVIEW_EXTENSION_IDENTIFIER),
            (plug_ins.appending_path_component("DownrightThumb.appex").path(), Self::THUMBNAIL_EXTENSION_IDENTIFIER),
        ];
        let completion = MainThreadBound::new(Box::new(completion) as Box<dyn FnOnce(bool)>, mtm);

        // All four tools block, and `qlmanage -r cache` takes seconds on a busy
        // machine. None of it may run on the main thread during launch.
        DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(DispatchQoS::UserInitiated)).exec_async(
            move || {
                run(LSREGISTER_PATH, &["-f", &bundle_path]);
                for (path, identifier) in &extensions {
                    if !file_exists(path) {
                        continue;
                    }
                    run("/usr/bin/pluginkit", &["-a", path]);
                    run("/usr/bin/pluginkit", &["-e", "use", "-i", identifier]);
                }
                run("/usr/bin/qlmanage", &["-r"]);
                if reset_thumbnail_cache {
                    run("/usr/bin/qlmanage", &["-r", "cache"]);
                }

                let enabled = Self::is_quick_look_preview_enabled();
                DispatchQueue::main().exec_async(move || {
                    let mtm = MainThreadMarker::new().expect("the main queue runs on the main thread");
                    (completion.into_inner(mtm))(enabled);
                });
            },
        );
    }

    /// The pane the user lands on when the automatic path did not take.
    pub fn open_quick_look_settings() {
        let url = NSURL::URLWithString(&ns(
            "x-apple.systempreferences:com.apple.ExtensionsPreferences?extensionPointIdentifier=com.apple.quicklook.preview",
        ));
        let Some(url) = url else { return };
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

/// One `setDefaultApplication` call of `makeDefaultMarkdownHandler`'s loop,
/// chained from the previous call's completion as Swift's `await` chains it.
struct DefaultHandlerStep {
    bundle: SendRetained<NSURL>,
    types: Vec<SendRetained<UTType>>,
    index: usize,
    first_failure: Option<CocoaError>,
    completion: MainThreadBound<Box<dyn FnOnce(Option<CocoaError>)>>,
}

/// `NSURL` and `UTType` are immutable and thread-safe; Swift passes them
/// across its continuation freely.
struct SendRetained<T: objc2::Message>(Retained<T>);

unsafe impl<T: objc2::Message> Send for SendRetained<T> {}

impl<T: objc2::Message> From<Retained<T>> for SendRetained<T> {
    fn from(value: Retained<T>) -> Self {
        SendRetained(value)
    }
}

impl DefaultHandlerStep {
    fn run(self) {
        if self.index >= self.types.len() {
            let DefaultHandlerStep { first_failure, completion, .. } = self;
            DispatchQueue::main().exec_async(move || {
                let mtm = MainThreadMarker::new().expect("the main queue runs on the main thread");
                (completion.into_inner(mtm))(first_failure);
            });
            return;
        }
        let bundle = self.bundle.0.clone();
        let kind = self.types[self.index].0.clone();
        let slot = std::sync::Mutex::new(Some(self));
        let handler = RcBlock::new(move |error: *mut NSError| {
            let failure = unsafe { error.as_ref() }.map(CocoaError::from_ns_error);
            let Some(mut step) = slot.lock().ok().and_then(|mut slot| slot.take()) else { return };
            if step.first_failure.is_none() {
                step.first_failure = failure;
            }
            step.index += 1;
            step.run();
        });
        NSWorkspace::sharedWorkspace().setDefaultApplicationAtURL_toOpenContentType_completionHandler(
            &bundle,
            &kind,
            Some(&handler),
        );
    }
}

/// Best-effort: every caller treats a missing tool or a non-zero exit as
/// "that step did not happen", and reports the outcome by re-checking the
/// world rather than by trusting an exit code.
fn run(launch_path: &str, arguments: &[&str]) -> String {
    if !is_executable_file(launch_path) {
        return String::new();
    }
    let Ok(mut child) = Process::new(launch_path).args(arguments).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else {
        return String::new();
    };
    // Read before waiting: a tool that fills the pipe buffer deadlocks
    // against a parent sitting in `wait`.
    let mut data = Vec::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_end(&mut data);
    }
    let _ = child.wait();
    String::from_utf8(data).unwrap_or_default()
}
