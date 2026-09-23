//! Port of the menu-bar cases of `Tests/DownrightAppTests/ShareAndCaptureTests.swift`
//! (`shareIsAFileMenuCommandWithItsOwnChord`,
//! `editMenuCarriesTheImportFromDevicePlaceholder`): the ones that build
//! `MainMenu`. The file's other cases are `share_and_capture_tests_commands.rs`
//! and `share_and_capture_tests_share.rs`.
//!
//! `harness = false` (`tests/main_thread/mod.rs`), with the support directory
//! sandboxed (`tests/document_support/mod.rs`) before `KeybindingStore.shared`
//! first loads.

mod document_support;
mod main_thread;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem, NSMenuItemImportFromDeviceIdentifier,
    NSUserInterfaceItemIdentification,
};
use upleft_app::app::main_menu::MainMenu;
use upleft_app::support::commands::{Command, KeyBinding, Menu, ModifierFlags};
use upleft_app::support::keybindings::KeybindingDefaults;

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("main-thread tests run on the main thread")
}

// MARK: - Share in the command table

fn share_is_a_file_menu_command_with_its_own_chord() {
    let _ = NSApplication::sharedApplication(mtm());
    assert_eq!(Command::Share.menu(), Menu::File);
    assert_eq!(Command::ShareAsPdf.menu(), Menu::File);
    let table = KeybindingDefaults::table();
    assert_eq!(
        table.get(&Command::Share),
        Some(&vec![KeyBinding::new("s", ModifierFlags::COMMAND | ModifierFlags::CONTROL)])
    );
    // Save and Save As keep theirs.
    assert_eq!(table.get(&Command::Save), Some(&vec![KeyBinding::new("s", ModifierFlags::COMMAND)]));
    assert_eq!(
        table.get(&Command::SaveAs),
        Some(&vec![KeyBinding::new("s", ModifierFlags::COMMAND | ModifierFlags::SHIFT)])
    );

    let mut found: Option<Retained<NSMenuItem>> = None;
    fn walk(menu: &NSMenu, found: &mut Option<Retained<NSMenuItem>>) {
        for item in menu.itemArray().iter() {
            if MainMenu::command(&item) == Some(Command::Share) {
                *found = Some(item.clone());
            }
            if let Some(submenu) = item.submenu() {
                walk(&submenu, found);
            }
        }
    }
    walk(&MainMenu::build(mtm()), &mut found);
    let item = found;
    assert_eq!(item.as_ref().map(|item| item.title().to_string()).as_deref(), Some("Share…"));
    // The chord shown is read back out of the binding store, so a remap
    // cannot leave the menu advertising a shortcut that does nothing.
    assert_eq!(item.as_ref().map(|item| item.keyEquivalent().to_string()).as_deref(), Some("s"));
    assert_eq!(
        item.as_ref().map(|item| item.keyEquivalentModifierMask()),
        Some(NSEventModifierFlags(NSEventModifierFlags::Command.0 | NSEventModifierFlags::Control.0))
    );
}

// MARK: - The Continuity Camera menu host

/// AppKit substitutes Take Photo / Scan Documents for the placeholder, and
/// the identifier is the only thing that marks it. Lose that and the feature
/// disappears with no other symptom.
fn edit_menu_carries_the_import_from_device_placeholder() {
    let _ = NSApplication::sharedApplication(mtm());
    let edit = MainMenu::build(mtm())
        .itemArray()
        .iter()
        .filter_map(|item| item.submenu())
        .find(|submenu| submenu.title().to_string() == "Edit");
    let insert = edit.and_then(|edit| edit.itemArray().iter().find(|item| item.title().to_string() == "Insert"));
    assert!(insert.as_ref().and_then(|insert| insert.submenu()).is_some());
    let placeholder = insert.and_then(|insert| insert.submenu()).and_then(|submenu| submenu.itemArray().firstObject());
    // SAFETY: AppKit's identifier constant.
    let expected = unsafe { NSMenuItemImportFromDeviceIdentifier }.to_string();
    assert_eq!(
        placeholder.as_ref().and_then(|placeholder| placeholder.identifier()).map(|identifier| identifier.to_string()),
        Some(expected)
    );
    // No action of our own: the substituted items carry theirs, and a
    // hand-written selector would be a second, wrong answer.
    assert!(placeholder.as_ref().and_then(|placeholder| placeholder.action()).is_none());
}

fn main() {
    document_support::sandbox();
    main_thread::run(&[
        ("share_is_a_file_menu_command_with_its_own_chord", share_is_a_file_menu_command_with_its_own_chord),
        ("edit_menu_carries_the_import_from_device_placeholder", edit_menu_carries_the_import_from_device_placeholder),
    ]);
    document_support::remove_sandbox();
}
