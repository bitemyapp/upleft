//! Port of `quickLookIsAFileMenuCommandOnTheStandardChord` from
//! `Tests/DownrightAppTests/DocumentQuickLookTests.swift`, the file's one
//! menu-bar case (it builds `MainMenu`). Its other windowless case is
//! `document_quick_look_tests_commands.rs`; the rest needs a
//! `DocumentWindowController`.
//!
//! `harness = false` (`tests/main_thread/mod.rs`), with the support directory
//! sandboxed (`tests/document_support/mod.rs`) before `KeybindingStore.shared`
//! first loads.

mod document_support;
mod main_thread;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use upleft_app::app::main_menu::MainMenu;
use upleft_app::support::commands::{Command, KeyBinding, Menu, ModifierFlags};
use upleft_app::support::keybindings::KeybindingDefaults;

fn quick_look_is_a_file_menu_command_on_the_standard_chord() {
    let mtm = MainThreadMarker::new().expect("main-thread tests run on the main thread");
    let _ = NSApplication::sharedApplication(mtm);
    assert_eq!(Command::QuickLook.title(), "Quick Look");
    assert_eq!(Command::QuickLook.menu(), Menu::File);
    assert_eq!(
        KeybindingDefaults::table().get(&Command::QuickLook),
        Some(&vec![KeyBinding::new("y", ModifierFlags::COMMAND)])
    );

    let mut found: Option<Retained<NSMenuItem>> = None;
    fn walk(menu: &NSMenu, found: &mut Option<Retained<NSMenuItem>>) {
        for item in menu.itemArray().iter() {
            if MainMenu::command(&item) == Some(Command::QuickLook) {
                *found = Some(item.clone());
            }
            if let Some(submenu) = item.submenu() {
                walk(&submenu, found);
            }
        }
    }
    walk(&MainMenu::build(mtm), &mut found);
    assert_eq!(found.as_ref().map(|item| item.keyEquivalent().to_string()).as_deref(), Some("y"));
    assert_eq!(found.as_ref().map(|item| item.keyEquivalentModifierMask()), Some(NSEventModifierFlags::Command));
}

fn main() {
    document_support::sandbox();
    main_thread::run(&[(
        "quick_look_is_a_file_menu_command_on_the_standard_chord",
        quick_look_is_a_file_menu_command_on_the_standard_chord,
    )]);
    document_support::remove_sandbox();
}
