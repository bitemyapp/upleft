//! Port of `Tests/DownrightAppTests/SettingsPaneTests.swift`.
//!
//! The Settings window, which is mostly constructed rather than laid out by
//! hand: both bugs these tests pin down were timing accidents of construction
//! — a tab that read a title before the pane had one, and a form that hung off
//! the bottom of an unflipped clip view.
//!
//! The suite is `@MainActor` and `.serialized` in Swift, so this binary owns
//! the main thread (`harness = false`, see `main_thread`).
//!
//! Sandbox. Building the window loads the General pane, which reads
//! `Preferences.shared`, the agent settings under the home folder and the
//! support folder, and the window saves its frame and selected pane in
//! `UserDefaults.standard`. So `main` re-runs this binary with `HOME` and
//! `CFFIXED_USER_HOME` pointing at a temporary folder and Downright's own
//! `DOWNRIGHT_SUPPORT_DIRECTORY` override inside it (both must be set before
//! Foundation first reads them), installs a sandboxed `Preferences` as
//! `Preferences::shared()` (the real one publishes the Quick Look appearance
//! to the user's global preferences domain), and removes the test process's
//! own `UserDefaults` domain before and after. No test orders a window in:
//! the Settings window is titled, and AppKit would pull a titled window back
//! onto a screen.

mod main_thread;

use std::path::{Path, PathBuf};

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSScrollView, NSStackView, NSTabViewItem};
use objc2_foundation::{NSBundle, NSPoint, NSProcessInfo, NSRect, NSString, NSUserDefaults};
use upleft_app::app::preferences_window_controller::{
    PreferenceRow, PreferencesPane, PreferencesWindowController, SettingsPane,
};
use upleft_app::support::preferences::Preferences;
use upleft_foundation::url::FileUrl;

/// Set (to the sandbox root) in the re-run child.
const SANDBOX_VARIABLE: &str = "UPLEFT_SETTINGS_PANE_TESTS_SANDBOX";

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("the settings tests run on the main thread")
}

fn every_pane_names_itself_before_its_view_loads() {
    let mtm = mtm();
    for pane in SettingsPane::ALL_CASES {
        let controller = PreferencesWindowController::controller(pane, mtm);
        // The tab item is built from a controller whose view has not loaded,
        // and it copies the title once. A pane that names itself in
        // `loadView()` is therefore unnamed at exactly the moment that
        // matters, and the label falls back to the class description.
        assert!(!controller.isViewLoaded(), "{pane:?} loaded its view during construction");
        assert_eq!(controller.title().map(|title| title.to_string()).as_deref(), Some(pane.title()));
        assert_eq!(NSTabViewItem::tabViewItemWithViewController(&controller).label().to_string(), pane.title());
    }
}

fn settings_tabs_are_labelled_with_pane_names() {
    let controller = PreferencesWindowController::new(mtm());
    // `defer { controller.close() }`.
    struct Close<'a>(&'a PreferencesWindowController);
    impl Drop for Close<'_> {
        fn drop(&mut self) {
            self.0.close();
        }
    }
    let _close = Close(&controller);

    let expected: Vec<String> = SettingsPane::ALL_CASES.iter().map(|pane| pane.title().to_owned()).collect();
    assert_eq!(controller.tab_labels_for_testing(), expected);
    // The failure mode was a raw class description in the toolbar.
    assert!(!controller.tab_labels_for_testing().iter().any(|label| label.contains("DownrightApp.")));
}

/// A short pane — the General pane is six controls — must start under the
/// title bar, not at the foot of a 620pt window.
fn a_short_pane_sits_at_the_top_of_its_scroll_view() {
    let pane = PreferencesPane::with_rows(
        SettingsPane::General,
        || {
            vec![
                PreferenceRow::section("On open"),
                PreferenceRow::toggle("Restore windows", None, || false, |_| {}),
            ]
        },
        mtm(),
    );
    let scroll = pane.view().downcast::<NSScrollView>().expect("the pane's view is a scroll view");
    scroll.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), objc2_foundation::NSSize::new(760.0, 620.0)));
    scroll.layoutSubtreeIfNeeded();

    let form = scroll
        .documentView()
        .and_then(|view| view.downcast::<NSStackView>().ok())
        .expect("the document view is a stack view");
    let first_row = form.arrangedSubviews().firstObject().expect("the form has a first row");
    assert!(form.isFlipped(), "the document stack must use top-left coordinates");
    assert!(form.frame().size.height >= scroll.contentView().bounds().size.height);
    assert_eq!(form.frame().origin.y, 0.0);
    assert_eq!(scroll.contentView().bounds().origin.y, 0.0);
    assert!(first_row.frame().origin.y <= 24.0, "the first row must stay at the top of a short pane");
}

/// Filtering rebuilds the stack under a live clip view, so the pane has to
/// come back to the top of the *new* list rather than keep the offset of the
/// one the user was reading.
fn filtering_returns_the_pane_to_its_first_row() {
    let rows: Vec<PreferenceRow> = (0..40)
        .map(|index| PreferenceRow::toggle(format!("Setting {index}"), None, || false, |_| {}))
        .collect();
    let pane = PreferencesPane::with_rows(SettingsPane::Editor, move || rows.clone(), mtm());
    let scroll = pane.view().downcast::<NSScrollView>().expect("the pane's view is a scroll view");
    scroll.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), objc2_foundation::NSSize::new(760.0, 200.0)));
    scroll.layoutSubtreeIfNeeded();

    let form = scroll.documentView().expect("the pane has a document view");
    assert!(form.frame().size.height > scroll.contentView().bounds().size.height, "the list must overflow");
    scroll.contentView().scrollToPoint(NSPoint::new(0.0, 120.0));
    scroll.reflectScrolledClipView(&scroll.contentView());
    assert!(scroll.contentView().bounds().origin.y > 0.0);

    pane.set_search_query("Setting 3");
    scroll.layoutSubtreeIfNeeded();
    assert_eq!(scroll.contentView().bounds().origin.y, 0.0);
    assert_eq!(form.frame().origin.y, 0.0);
}

/// Not in SettingsPaneTests: every pane's rows build (selecting a tab loads
/// its pane), and a search lands on the first pane with a match.
fn every_pane_loads_and_a_search_selects_the_first_matching_pane() {
    let controller = PreferencesWindowController::new(mtm());
    struct Close<'a>(&'a PreferencesWindowController);
    impl Drop for Close<'_> {
        fn drop(&mut self) {
            self.0.close();
        }
    }
    let _close = Close(&controller);
    let window = controller.window().expect("the controller owns its window");
    let tabs = window
        .contentViewController()
        .and_then(|content| content.downcast::<objc2_app_kit::NSTabViewController>().ok())
        .expect("the content is a tab view controller");

    for pane in SettingsPane::ALL_CASES {
        controller.select(pane);
        let index = tabs.selectedTabViewItemIndex();
        assert_eq!(SettingsPane::ALL_CASES[index as usize], pane);
        let item = tabs.tabViewItems().objectAtIndex(index as usize);
        let pane_controller = item.viewController(mtm()).expect("every tab has a pane");
        assert!(pane_controller.isViewLoaded(), "{pane:?} did not load when selected");
    }
    let saved = NSUserDefaults::standardUserDefaults().integerForKey(&NSString::from_str("settings.selectedPane"));
    assert_eq!(saved, 6, "the last selected pane is remembered");
    assert_eq!(window.contentRectForFrameRect(window.frame()).size.height, 680.0, "the keys pane is 680pt tall");

    let accessory = window.titlebarAccessoryViewControllers().objectAtIndex(0);
    let search_field = accessory
        .view()
        .subviews()
        .firstObject()
        .and_then(|view| view.downcast::<objc2_app_kit::NSSearchField>().ok())
        .expect("the title bar accessory holds the search field");
    search_field.setStringValue(&NSString::from_str("  light theme "));
    // SAFETY: sends the field's own action to its own target.
    unsafe { search_field.sendAction_to(search_field.action(), search_field.target().as_deref()) };
    assert_eq!(tabs.selectedTabViewItemIndex(), 1, "\"light theme\" lives on the Appearance pane");
    assert_eq!(window.contentRectForFrameRect(window.frame()).size.height, 460.0);
}

// MARK: - Sandbox

/// The persistent domain `UserDefaults.standard` writes: the bundle
/// identifier, or for a bare executable its process name.
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

/// Removes this test binary's own `UserDefaults` domain, and waits for
/// `cfprefsd` to write the (now empty) domain out, so a file deleted after
/// this stays deleted.
fn remove_standard_domain() {
    let domain = standard_domain_name();
    let defaults = NSUserDefaults::standardUserDefaults();
    defaults.removePersistentDomainForName(&NSString::from_str(&domain));
    #[allow(deprecated)]
    defaults.synchronize();
}

/// The parent: runs this binary again inside a fresh sandbox and cleans up
/// after it.
fn run_in_sandbox() -> i32 {
    let root = std::env::temp_dir().join(format!(
        "upleft-settings-pane-tests-{}",
        objc2_foundation::NSUUID::UUID().UUIDString()
    ));
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

fn main() {
    if std::env::var_os(SANDBOX_VARIABLE).is_none() {
        std::process::exit(run_in_sandbox());
    }
    let root = std::env::var(SANDBOX_VARIABLE).expect("the sandbox root");
    let home = objc2_foundation::NSHomeDirectory().to_string();
    assert!(home.starts_with(&root), "Foundation's home ({home}) must be the sandbox's, under {root}");
    let mtm = mtm();
    // Windows want the shared application to exist; it is never activated.
    let _ = NSApplication::sharedApplication(mtm);
    remove_standard_domain();
    let support = std::env::var("DOWNRIGHT_SUPPORT_DIRECTORY").expect("the sandbox sets the support folder");
    let preferences_file = FileUrl::from_path(&format!("{support}/preferences.json"));
    assert!(
        Preferences::install_shared(Preferences::for_testing(preferences_file, None)),
        "nothing may read Preferences.shared before the sandbox installs it"
    );
    main_thread::run(&[
        ("every_pane_names_itself_before_its_view_loads", every_pane_names_itself_before_its_view_loads),
        ("settings_tabs_are_labelled_with_pane_names", settings_tabs_are_labelled_with_pane_names),
        ("a_short_pane_sits_at_the_top_of_its_scroll_view", a_short_pane_sits_at_the_top_of_its_scroll_view),
        ("filtering_returns_the_pane_to_its_first_row", filtering_returns_the_pane_to_its_first_row),
        (
            "every_pane_loads_and_a_search_selects_the_first_matching_pane",
            every_pane_loads_and_a_search_selects_the_first_matching_pane,
        ),
    ]);
    remove_standard_domain();
}
