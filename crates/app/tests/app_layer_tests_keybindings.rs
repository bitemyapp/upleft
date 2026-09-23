//! Port of the key-binding tests in `Tests/DownrightAppTests/AppLayerTests.swift`
//! (§7.2). The rest of that file belongs to other ports; this file is named
//! apart so the ports can merge.
//!
//! Swift uses `KeybindingStore.shared`, which reads the user's support
//! directory; these tests give the store a fresh temporary file instead.

use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};

use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
use objc2_foundation::{NSPoint, NSString};
use upleft_app::support::commands::{Command, CommandScope, KeyBinding, ModifierFlags};
use upleft_app::support::keybindings::KeybindingStore;
use upleft_foundation::url::FileUrl;

const CMD: ModifierFlags = ModifierFlags::COMMAND;
const SHIFT: ModifierFlags = ModifierFlags::SHIFT;
const OPT: ModifierFlags = ModifierFlags::OPTION;

fn fresh_store() -> KeybindingStore {
    let directory =
        std::env::temp_dir().join(format!("AppLayerTests-{}", objc2_foundation::NSUUID::new().UUIDString()));
    KeybindingStore::with_file(FileUrl::from_path(&directory.join("keybindings.json").to_string_lossy()))
}

#[test]
fn key_binding_round_trips_through_its_string_form() {
    for source in ["cmd+e", "cmd+shift+o", "opt+down", "space", "shift+space", "ctrl+opt+shift+cmd+k", "["] {
        let binding = KeyBinding::parsing(source).unwrap_or_else(|| panic!("failed to parse {source}"));
        assert_eq!(KeyBinding::parsing(&binding.serialized()), Some(binding));
    }
}

/// Caps Lock and Fn are sticky state, not part of a chord. A binding must
/// compare equal to an event carrying them, or every shortcut silently dies
/// when the user's caps lock is on (§7.2).
#[test]
fn key_bindings_ignore_caps_lock_and_function_flags() {
    let binding = KeyBinding::new("s", CMD);
    let with_caps_lock = KeyBinding::new("s", CMD.union(ModifierFlags::CAPS_LOCK).union(ModifierFlags::FUNCTION));
    assert_eq!(binding, with_caps_lock);
    let hasher = BuildHasherDefault::<DefaultHasher>::default();
    assert_eq!(hasher.hash_one(&binding), hasher.hash_one(&with_caps_lock));

    // The event-driven lookup must resolve ⌘S while Caps Lock is held.
    let store = fresh_store();
    let characters = NSString::from_str("s");
    let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        NSEventType::KeyDown,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags::Command | NSEventModifierFlags::CapsLock,
        0.0,
        0,
        None,
        &characters,
        &characters,
        false,
        0,
    );
    let resolved = event.and_then(|event| store.command_for_event(&event, CommandScope::Live));
    assert_eq!(resolved, Some(Command::Save));
}

#[test]
fn default_bindings_match_the_spec_table() {
    let store = fresh_store();
    assert_eq!(store.primary_binding(Command::SourceMode), Some(KeyBinding::new("e", CMD.union(SHIFT))));
    assert_eq!(store.primary_binding(Command::Find), Some(KeyBinding::new("f", CMD)));
    assert_eq!(store.primary_binding(Command::UseSelectionForFind), Some(KeyBinding::new("e", CMD)));
    assert_eq!(store.primary_binding(Command::CopyAsMarkdown), Some(KeyBinding::new("c", CMD.union(OPT).union(SHIFT))));
    // ⌘0 is Actual Size on macOS, ⌘⇧V is Paste and Match Style, and ⌥↑/⌥↓
    // are moveParagraphBackward:/Forward: while a caret is in the document.
    assert_eq!(store.primary_binding(Command::VersionTimeline), Some(KeyBinding::new("v", CMD.union(OPT))));
    assert_eq!(store.primary_binding(Command::NextChange), Some(KeyBinding::new("down", OPT.union(SHIFT))));
    assert_eq!(store.primary_binding(Command::SplitView), Some(KeyBinding::new("backslash", CMD)));
}

#[test]
fn every_command_has_a_title_and_somewhere_to_run() {
    for command in Command::ALL_CASES {
        assert!(!command.title().is_empty(), "{} has no title", command.raw_value());
        assert!(!command.scopes().is_empty(), "{} is dispatchable nowhere", command.raw_value());
    }
}

#[test]
fn binding_conflicts_are_detected() {
    let store = fresh_store();
    let binding = store.primary_binding(Command::Find).expect("find has a binding");
    assert!(store.conflicts(&binding, Command::FindNext).contains(&Command::Find));
    assert!(!store.conflicts(&binding, Command::Find).contains(&Command::Find));
}
