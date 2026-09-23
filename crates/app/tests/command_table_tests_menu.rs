//! Port of the menu-bar tests of `Tests/DownrightAppTests/CommandTableTests.swift`
//! (`@MainActor`): the ones that build `MainMenu`. The rest of that suite is
//! `command_table_tests.rs`.
//!
//! `harness = false` (`tests/main_thread/mod.rs`): `MainMenu.build()` assigns
//! `NSApp.servicesMenu` and friends, so it runs on the main thread. The
//! support directory is sandboxed (`tests/document_support/mod.rs`) before
//! `KeybindingStore.shared` first loads, so the menu reads default bindings
//! and never the user's `keybindings.json`.

mod document_support;
mod main_thread;

use std::collections::HashMap;

use objc2::{MainThreadMarker, MainThreadOnly};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::NSString;
use upleft_app::app::main_menu::{HelpLinkTarget, MainMenu};
use upleft_app::support::commands::{Command, CommandContext, Menu};

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("main-thread tests run on the main thread")
}

/// `NSEvent.ModifierFlags` literals.
fn flags(names: &[NSEventModifierFlags]) -> NSEventModifierFlags {
    NSEventModifierFlags(names.iter().fold(0, |bits, flag| bits | flag.0))
}

// MARK: - The menu bar is derived from the table

struct Placement {
    command: Command,
    /// Title of the top-level menu the item was found under.
    menu_title: String,
}

fn placements() -> Vec<Placement> {
    // `MainMenu.build()` assigns NSApp.servicesMenu and friends.
    let _ = NSApplication::sharedApplication(mtm());
    let mut found = Vec::new();
    fn walk(menu: &NSMenu, title: &str, found: &mut Vec<Placement>) {
        for item in menu.itemArray().iter() {
            if let Some(command) = MainMenu::command(&item) {
                found.push(Placement { command, menu_title: title.to_owned() });
            }
            if let Some(submenu) = item.submenu() {
                walk(&submenu, title, found);
            }
        }
    }
    for item in MainMenu::build(mtm()).itemArray().iter() {
        let Some(submenu) = item.submenu() else { continue };
        walk(&submenu, &submenu.title().to_string(), &mut found);
    }
    found
}

/// The one test that stops the menu table from drifting away from the
/// command table again — commands reachable only from the palette, menus
/// whose groups were never iterated, and items duplicated across two menus
/// are all the same failure.
fn every_command_appears_in_exactly_one_menu() {
    let mut counts: HashMap<Command, usize> = HashMap::new();
    for placement in placements() {
        *counts.entry(placement.command).or_insert(0) += 1;
    }
    for command in Command::ALL_CASES {
        let count = counts.get(&command).copied().unwrap_or(0);
        if MainMenu::COMMANDS_HIDDEN_FROM_MENU_BAR.contains(&command) {
            assert_eq!(count, 0, "{} is on the hidden list but appears in a menu", command.raw_value());
        } else {
            assert_eq!(count, 1, "{} appears in {count} menus, expected 1", command.raw_value());
        }
    }
}

/// The keybinding editor prints `Command.menu` in its "Menu" column, so a
/// command placed elsewhere makes Settings lie to the user.
fn every_command_sits_in_the_menu_it_advertises() {
    for placement in placements() {
        assert_eq!(
            placement.menu_title,
            placement.command.menu().title(),
            "{} says {}, found under {}",
            placement.command.raw_value(),
            placement.command.menu().title(),
            placement.menu_title
        );
    }
}

/// Window and Help used to be hand-built, so anything declared for them
/// vanished. Application was not even a case.
fn application_window_and_help_menus_come_from_the_table() {
    let mut by_menu: HashMap<String, Vec<Command>> = HashMap::new();
    for placement in placements() {
        by_menu.entry(placement.menu_title).or_default().push(placement.command);
    }
    let contains = |menu: &str, command: Command| by_menu.get(menu).is_some_and(|commands| commands.contains(&command));
    assert!(contains("Upleft", Command::CheckForUpdates));
    assert!(contains("Upleft", Command::Preferences));
    assert!(contains("Window", Command::SplitView));
    assert!(contains("Window", Command::PinWindow));
    for command in Command::ALL_CASES.into_iter().filter(|command| command.menu() == Menu::Help) {
        assert!(contains("Help", command), "{} is declared for Help", command.raw_value());
    }
}

/// The submenu of the top-level item titled `title`.
fn top_level_menu(root: &NSMenu, title: &str) -> Option<Retained<NSMenu>> {
    root.itemArray().iter().filter_map(|item| item.submenu()).find(|submenu| submenu.title().to_string() == title)
}

/// "Check for Updates…" was a hardcoded item with its own target, so it
/// ignored rebinding and duplicated the title literal.
fn check_for_updates_is_an_ordinary_command_item() {
    let _ = NSApplication::sharedApplication(mtm());
    let app_menu = top_level_menu(&MainMenu::build(mtm()), "Upleft");
    let item = app_menu.and_then(|menu| {
        menu.itemArray().iter().find(|item| item.title().to_string() == Command::CheckForUpdates.title())
    });
    assert!(item.is_some());
    let item = item.unwrap_or_else(|| NSMenuItem::new(mtm()));
    assert_eq!(MainMenu::command(&item), Some(Command::CheckForUpdates));
}

/// The standard items the Edit and Window menus were missing.
fn standard_menu_items_exist_with_their_mac_os_chords() {
    let _ = NSApplication::sharedApplication(mtm());
    let mut titles: HashMap<String, (String, NSEventModifierFlags)> = HashMap::new();
    fn walk(menu: &NSMenu, titles: &mut HashMap<String, (String, NSEventModifierFlags)>) {
        for item in menu.itemArray().iter() {
            titles.insert(item.title().to_string(), (item.keyEquivalent().to_string(), item.keyEquivalentModifierMask()));
            if let Some(submenu) = item.submenu() {
                walk(&submenu, titles);
            }
        }
    }
    walk(&MainMenu::build(mtm()), &mut titles);

    let command = NSEventModifierFlags::Command;
    let shift = NSEventModifierFlags::Shift;
    let control = NSEventModifierFlags::Control;
    let key = |title: &str| titles.get(title).map(|(key, _)| key.clone());
    let mask = |title: &str| titles.get(title).map(|(_, mask)| *mask);
    assert_eq!(key("Paste and Match Style").as_deref(), Some("v"));
    assert_eq!(mask("Paste and Match Style"), Some(flags(&[command, shift])));
    assert_eq!(key("Paste as Markdown").as_deref(), Some(""));
    assert_eq!(key("Page Setup…").as_deref(), Some("p"));
    assert_eq!(mask("Page Setup…"), Some(flags(&[command, shift])));
    assert_eq!(key("Enter Full Screen").as_deref(), Some("f"));
    assert_eq!(mask("Enter Full Screen"), Some(flags(&[control, command])));
    for expected in [
        "Delete",
        "Find",
        "Spelling and Grammar",
        "Substitutions",
        "Transformations",
        "Speech",
        "Upleft Help",
        "Markdown Reference",
        "Report an Issue",
        "Star Upleft on GitHub",
        "Support Upleft",
    ] {
        assert!(titles.contains_key(expected), "the menu bar is missing {expected}");
    }
}

fn support_menu_item_opens_git_hub_sponsors() {
    let _ = NSApplication::sharedApplication(mtm());
    let help_menu = top_level_menu(&MainMenu::build(mtm()), "Help");
    let item = help_menu
        .and_then(|menu| menu.itemArray().iter().find(|item| item.title().to_string() == "Support Upleft"));
    let represented = item
        .as_ref()
        .and_then(|item| item.representedObject())
        .and_then(|object| object.downcast::<NSString>().ok())
        .map(|string| string.to_string());
    assert_eq!(represented.as_deref(), Some("https://github.com/sponsors/ezzy1630"));
    let target = item.as_ref().and_then(|item| item.target());
    let shared = HelpLinkTarget::shared(mtm());
    assert!(target.is_some_and(|target| std::ptr::eq(&*target, &**shared as &AnyObject)));
}

fn validation_only_claims_items_that_carry_a_command() {
    let plain = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm()),
            &NSString::from_str("Undo"),
            Some(sel!(undo:)),
            &NSString::from_str("z"),
        )
    };
    assert!(MainMenu::validate(&plain, &CommandContext::application_only(false)));
    assert!(!MainMenu::validate(&MainMenu::command_item(Command::Save, mtm()), &CommandContext::application_only(false)));
}

fn main() {
    document_support::sandbox();
    main_thread::run(&[
        ("every_command_appears_in_exactly_one_menu", every_command_appears_in_exactly_one_menu),
        ("every_command_sits_in_the_menu_it_advertises", every_command_sits_in_the_menu_it_advertises),
        ("application_window_and_help_menus_come_from_the_table", application_window_and_help_menus_come_from_the_table),
        ("check_for_updates_is_an_ordinary_command_item", check_for_updates_is_an_ordinary_command_item),
        ("standard_menu_items_exist_with_their_mac_os_chords", standard_menu_items_exist_with_their_mac_os_chords),
        ("support_menu_item_opens_git_hub_sponsors", support_menu_item_opens_git_hub_sponsors),
        ("validation_only_claims_items_that_carry_a_command", validation_only_claims_items_that_carry_a_command),
    ]);
    document_support::remove_sandbox();
}
