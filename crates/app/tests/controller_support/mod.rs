//! Fixtures for the tests that drive a real `DocumentWindowController`.
//!
//! Downright's window tests build `DocumentWindowController()`, which reads
//! `Preferences.shared`, `SnapshotStore.shared` and `DocumentStateStore.shared`
//! from the user's real Application Support folder, and they order the titled
//! window in (`showWindow`, `makeKeyAndOrderFront`). These ports never touch
//! the real home and never put a window on a display:
//!
//! - [`prepare`] points Downright's `DOWNRIGHT_SUPPORT_DIRECTORY` override at a
//!   fresh temporary folder (`document_support::sandbox`) and installs a
//!   sandboxed `Preferences` as `Preferences::shared()` before anything reads
//!   it. Call it first thing in `main`.
//! - [`new_controller`] parks the window at (-30000, -30000) and never orders
//!   it in: AppKit pulls a titled window onto a display once it is ordered in,
//!   even from there. Where Swift orders the window in before making the text
//!   view first responder, the port only makes it first responder (which an
//!   unordered window accepts).
//! - Nothing activates the app.

#![allow(dead_code)]

use std::time::Duration;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, Message};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType, NSTextView, NSView};
use objc2_foundation::{NSPoint, NSProcessInfo, NSString};
use upleft_app::ai::snapshot_store::SnapshotStore;
use upleft_app::app::document_window_controller::DocumentWindowController;
use upleft_app::support::preferences::Preferences;
use upleft_foundation::url::FileUrl;
use upleft_render::render_contracts::RenderMode;

#[path = "../document_support/mod.rs"]
pub mod document_support;
#[path = "../main_thread/mod.rs"]
pub mod main_thread;

pub fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("controller tests run on the main thread")
}

/// Sandboxes the stores and `Preferences.shared`. Call first in `main`.
pub fn prepare() {
    let root = document_support::sandbox();
    let file = root.appending_path_component("shared-preferences.json");
    let _ = Preferences::install_shared(Preferences::for_testing(file, Some(SnapshotStore::shared().clone())));
}

/// What `Preferences.shared.update` posts after a change. The sandboxed
/// instance [`prepare`] installs is built with `Preferences::for_testing`,
/// which never posts, so a test whose Swift original relies on the
/// notification posts it itself, right after the update, as the real
/// instance does.
pub fn post_preferences_did_change() {
    let center = objc2_foundation::NSNotificationCenter::defaultCenter();
    // SAFETY: a plain notification with no object.
    unsafe {
        center.postNotificationName_object(
            &NSString::from_str(upleft_app::support::preferences::DID_CHANGE),
            None,
        )
    };
}

/// Removes the sandbox. Call after the tests ran.
pub fn finish() {
    document_support::remove_sandbox();
}

/// `RunLoop.main.run(mode: .common, before:)` until `condition` holds or
/// `timeout` passes, as the Swift suites' `pumpMainRunLoop(until:timeout:)`.
pub fn pump_main_run_loop(condition: impl Fn() -> bool, timeout: f64) -> bool {
    main_thread::pump_until(condition, Duration::from_secs_f64(timeout))
}

/// Parks the controller's window off every screen without ordering it in.
pub fn park(controller: &DocumentWindowController) {
    if let Some(window) = controller.window() {
        window.setFrameOrigin(NSPoint::new(-30000.0, -30000.0));
    }
}

/// `DocumentWindowController()`, parked.
pub fn new_controller() -> Retained<DocumentWindowController> {
    let controller = DocumentWindowController::new(mtm());
    park(&controller);
    controller
}

/// `try text.write(to:atomically:encoding:)`, then `DocumentWindowController()`
/// and `try controller.open(fileURL, mode:)`.
pub fn make_controller(text: &str, file: &FileUrl, mode: RenderMode) -> Retained<DocumentWindowController> {
    std::fs::write(file.path(), text).expect("write the fixture");
    let controller = new_controller();
    controller.open(file, mode).expect("open the fixture");
    park(&controller);
    controller
}

/// `defer { controller.close() }`.
pub struct Closing(pub Retained<DocumentWindowController>);

impl Drop for Closing {
    fn drop(&mut self) {
        self.0.close();
    }
}

impl std::ops::Deref for Closing {
    type Target = DocumentWindowController;

    fn deref(&self) -> &DocumentWindowController {
        &self.0
    }
}

/// `controller.window?.makeFirstResponder(view)`.
pub fn make_first_responder(controller: &DocumentWindowController, view: &NSView) {
    if let Some(window) = controller.window() {
        window.makeFirstResponder(Some(view));
    }
}

/// `URL(fileURLWithPath: NSTemporaryDirectory()).appendingPathComponent(name)`.
pub fn temporary_file(name: &str) -> FileUrl {
    document_support::temporary_directory().appending_path_component(name)
}

/// A fresh temporary directory (`…/prefix-UUID/`).
pub fn temporary_directory(prefix: &str) -> FileUrl {
    let directory = document_support::temporary_directory()
        .appending_path_component_is_directory(&format!("{prefix}-{}", document_support::unique()), true);
    std::fs::create_dir_all(directory.path()).expect("create the fixture directory");
    directory
}

/// Removes a path when dropped (`defer { try? FileManager.default.removeItem(at:) }`).
pub struct Removing(pub FileUrl);

impl Drop for Removing {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.path());
        let _ = std::fs::remove_file(self.0.path());
    }
}

/// `(text as NSString).range(of: needle)`.
pub fn range_of(text: &str, needle: &str) -> upleft_core::NSRange {
    let haystack: Vec<u16> = text.encode_utf16().collect();
    let needle: Vec<u16> = needle.encode_utf16().collect();
    match haystack.windows(needle.len()).position(|window| window == needle.as_slice()) {
        Some(location) => upleft_core::NSRange::new(location as isize, needle.len() as isize),
        None => upleft_core::NSRange::new(upleft_core::NS_NOT_FOUND, 0),
    }
}

/// `(text as NSString).length`.
pub fn utf16_length(text: &str) -> isize {
    text.encode_utf16().count() as isize
}

/// `NSEvent.keyEvent(with: .keyDown, …)` as the Swift suites build it.
pub fn key_event(
    text_view: &NSTextView,
    characters: &str,
    modifiers: NSEventModifierFlags,
    key_code: u16,
) -> Retained<NSEvent> {
    let characters = NSString::from_str(characters);
    let window_number = text_view.window().map_or(0, |window| window.windowNumber());
    NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        NSEventType::KeyDown,
        NSPoint::new(0.0, 0.0),
        modifiers,
        NSProcessInfo::processInfo().systemUptime(),
        window_number,
        None,
        &characters,
        &characters,
        false,
        key_code,
    )
    .expect("a key event")
}

/// `type(_:into:)`.
pub fn type_character(character: char, text_view: &NSTextView) {
    let event = key_event(text_view, &character.to_string(), NSEventModifierFlags::empty(), 0);
    text_view.keyDown(&event);
}

/// `pressCommandA(into:)`.
pub fn press_command_a(text_view: &NSTextView) {
    let event = key_event(text_view, "a", NSEventModifierFlags::Command, 0);
    text_view.keyDown(&event);
}

/// `String(UnicodeScalar(NSDeleteCharacter)!)`.
pub const DELETE_CHARACTER: char = '\u{7F}';

/// `pressDelete(into:)`.
pub fn press_delete(text_view: &NSTextView) {
    let event = key_event(text_view, &DELETE_CHARACTER.to_string(), NSEventModifierFlags::empty(), 51);
    text_view.keyDown(&event);
}

/// `pressOptionDelete(into:)`.
pub fn press_option_delete(text_view: &NSTextView) {
    let event = key_event(text_view, &DELETE_CHARACTER.to_string(), NSEventModifierFlags::Option, 51);
    text_view.keyDown(&event);
}

/// `pressTab(into:)`.
pub fn press_tab(text_view: &NSTextView) {
    let event = key_event(text_view, "\t", NSEventModifierFlags::empty(), 48);
    text_view.keyDown(&event);
}

/// `view.subviews.flatMap { [$0] + descendants(of: $0) }`.
pub fn descendants(root: &NSView) -> Vec<Retained<NSView>> {
    let mut result = Vec::new();
    for view in root.subviews().iter() {
        result.push(view.retain());
        result.extend(descendants(&view));
    }
    result
}
