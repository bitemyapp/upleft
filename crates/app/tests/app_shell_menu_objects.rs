//! Main-thread checks for the Objective-C classes of the `MainMenu`,
//! `ThemedSplitView` and `CompareWindowController` ports: they register under
//! their Swift names and build as the Swift does. No Swift test covers them.
//!
//! No window is ordered in: the compare window is titled, and AppKit would
//! constrain a titled window back onto a screen. Building it, laying it out
//! and calling its methods needs no ordering. `SetupWindowController` is not
//! built here: its initialiser reads `Preferences.shared`, whose load
//! publishes the Quick Look appearance to the real user defaults.
//!
//! `harness = false` (`tests/main_thread/mod.rs`), with the support directory
//! sandboxed (`tests/document_support/mod.rs`) and `CFFIXED_USER_HOME` and
//! `HOME` pointed into the sandbox before anything asks Foundation for the
//! home folder: `ThemeStore.shared` (which every `StyleSheet` reads) creates
//! its user-themes folder under Application Support.

mod document_support;
mod main_thread;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{
    NSApplication, NSButton, NSColor, NSControlStateValueOff, NSMenu, NSMenuDelegate, NSSplitViewDividerStyle,
};
use objc2_foundation::NSPoint;
use upleft_app::app::compare_window_controller::CompareWindowController;
use upleft_app::app::main_menu::{
    HelpLinkTarget, MainMenu, RecentsMenuDelegate, ThemeMenuDelegate, UpdateCheckMenuItem,
    perform_downright_command_selector,
};
use upleft_app::app::setup_window_controller::SetupWindowController;
use upleft_app::app::themed_split_view::ThemedSplitView;
use upleft_app::support::commands::Command;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("main-thread tests run on the main thread")
}

fn class_name(object: &AnyObject) -> String {
    object.class().name().to_str().unwrap().to_owned()
}

fn menu_classes_keep_their_swift_names() {
    let mtm = mtm();
    let _ = NSApplication::sharedApplication(mtm);
    assert_eq!(class_name(&UpdateCheckMenuItem::shared(mtm)), "UpdateCheckMenuItem");
    assert_eq!(class_name(&HelpLinkTarget::shared(mtm)), "HelpLinkTarget");
    assert_eq!(class_name(&RecentsMenuDelegate::shared(mtm)), "RecentsMenuDelegate");
    assert_eq!(class_name(&ThemeMenuDelegate::shared(mtm)), "ThemeMenuDelegate");
    // The shared instances are singletons.
    assert!(std::ptr::eq(&*HelpLinkTarget::shared(mtm), &*HelpLinkTarget::shared(mtm)));
}

fn command_items_carry_the_command_and_its_tag() {
    let mtm = mtm();
    let item = MainMenu::command_item(Command::Save, mtm);
    assert_eq!(item.action(), Some(perform_downright_command_selector()));
    assert_eq!(item.tag(), MainMenu::command_tag(Command::Save));
    let index = Command::ALL_CASES.iter().position(|command| *command == Command::Save).unwrap();
    assert_eq!(item.tag(), index as isize + 1000);
    assert_eq!(MainMenu::command(&item), Some(Command::Save));
    assert_eq!(item.keyEquivalent().to_string(), "s");
    // A remap is picked up by `refresh_key_equivalents`; with the default
    // bindings in place it leaves the chord as it was.
    let menu = NSMenu::new(mtm);
    menu.addItem(&item);
    MainMenu::refresh_key_equivalents(&menu);
    assert_eq!(item.keyEquivalent().to_string(), "s");
}

/// The sandboxed support directory has no `recents.json`.
fn recents_menu_says_so_when_there_are_none() {
    let mtm = mtm();
    let menu = NSMenu::new(mtm);
    RecentsMenuDelegate::shared(mtm).menuNeedsUpdate(&menu);
    let items = menu.itemArray();
    assert_eq!(items.count(), 1);
    let empty = items.firstObject().unwrap();
    assert_eq!(empty.title().to_string(), "No Recent Documents");
    assert!(!empty.isEnabled());
    assert!(empty.action().is_none());
}

fn setup_panel_is_skipped_when_nothing_applies() {
    assert!(SetupWindowController::make_for(Vec::new(), mtm()).is_none());
}

fn themed_split_view_draws_its_divider_in_the_rule_colour() {
    let mtm = mtm();
    let appearance = NSApplication::sharedApplication(mtm).effectiveAppearance();
    let style_sheet = std::rc::Rc::new(StyleSheet::new(ThemeStore::shared().current(), &appearance, None));
    let split = ThemedSplitView::new(style_sheet.clone(), true, mtm);
    assert_eq!(class_name(&split), "ThemedSplitView");
    assert!(split.isVertical());
    assert_eq!(split.dividerStyle(), NSSplitViewDividerStyle::Thin);
    let color: Retained<NSColor> = unsafe { msg_send![&*split, dividerColor] };
    assert!(std::ptr::eq(&*color, &*style_sheet.rule));
    let horizontal = ThemedSplitView::new(style_sheet, false, mtm);
    assert!(!horizontal.isVertical());
}

fn compare_window_builds_both_panes_without_being_shown() {
    let mtm = mtm();
    let controller = CompareWindowController::new(
        "# Title\n\nThe old paragraph.\n\nGone soon.\n",
        "old.md",
        "# Title\n\nThe new paragraph.\n",
        "new.md",
        None,
        mtm,
    );
    assert_eq!(class_name(&controller), "CompareWindowController");
    let window = controller.window().unwrap();
    window.setFrameOrigin(NSPoint::new(-30000.0, -30000.0));
    assert!(!window.isVisible());
    assert_eq!(window.title().to_string(), "old.md ⟷ new.md");

    let left = controller.left_container().unwrap();
    let right = controller.right_container().unwrap();
    assert!(!left.text_view().change_marks().is_empty());
    assert!(!right.text_view().change_marks().is_empty());

    // The toolbar asks its delegate for the lock item.
    let toolbar = window.toolbar().unwrap();
    let items = toolbar.items();
    let lock = items.iter().find(|item| item.itemIdentifier().to_string() == "scrollLock").unwrap();
    assert_eq!(lock.label().to_string(), "Scroll Lock");
    assert!(controller.scroll_locked());
    let button = lock.view().unwrap().downcast::<NSButton>().unwrap();
    button.setState(NSControlStateValueOff);
    let _: () = unsafe { msg_send![&*controller, toggleScrollLock: &*button] };
    assert!(!controller.scroll_locked());

    window.contentView().unwrap().layoutSubtreeIfNeeded();
}

/// Points the home folder into the sandbox. Call first thing in `main`.
fn sandbox_home() {
    let home = document_support::sandbox().appending_path_component_is_directory("home", true);
    std::fs::create_dir_all(home.path()).unwrap();
    // SAFETY: called from `main` before any other thread exists.
    unsafe {
        std::env::set_var("CFFIXED_USER_HOME", home.path());
        std::env::set_var("HOME", home.path());
    }
    let themes = ThemeStore::user_themes_directory().and_then(|url| url.path()).unwrap().to_string();
    assert!(themes.starts_with(&home.path()), "the themes folder {themes} escaped the sandbox");
}

fn main() {
    sandbox_home();
    main_thread::run(&[
        ("menu_classes_keep_their_swift_names", menu_classes_keep_their_swift_names),
        ("command_items_carry_the_command_and_its_tag", command_items_carry_the_command_and_its_tag),
        ("recents_menu_says_so_when_there_are_none", recents_menu_says_so_when_there_are_none),
        ("setup_panel_is_skipped_when_nothing_applies", setup_panel_is_skipped_when_nothing_applies),
        ("themed_split_view_draws_its_divider_in_the_rule_colour", themed_split_view_draws_its_divider_in_the_rule_colour),
        ("compare_window_builds_both_panes_without_being_shown", compare_window_builds_both_panes_without_being_shown),
    ]);
    document_support::remove_sandbox();
}
