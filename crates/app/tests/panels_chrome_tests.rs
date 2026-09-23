//! View-level tests for the shared panel chrome: PanelChrome's controls,
//! ChromeGlass, InspectorHostView and ConflictBarView (ported from
//! DownrightAppTests: PanelAccessibilityTests, ChangeReviewTests,
//! WindowChromeTests). Runs on the main thread; no window is needed.
//!
//! Not ported here: the DocumentChromeLayoutTests and WindowChromeTests cases
//! that drive `DocumentWindowController` (the app shell's), and the
//! PanelAccessibilityTests cases for the task panel, search results and find
//! bar (their panels' test binaries).

mod document_support;
#[path = "main_thread/mod.rs"]
mod main_thread;

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::{MainThreadOnly, msg_send};
use objc2_app_kit::{NSAccessibility, NSAccessibilityButtonRole, NSButton, NSEvent, NSEventModifierFlags, NSEventType, NSView};
use objc2_foundation::{NSPoint, NSRect, NSString};
use upleft_app::panels::appkit_support::{RectExt, downcast, is, rect};
use upleft_app::panels::chrome_glass::ChromeGlass;
use upleft_app::panels::conflict_bar_view::ConflictBarView;
use upleft_app::panels::inspector_host_view::{InspectorHostView, InspectorSection};
use upleft_app::panels::panel_chrome::{PanelCheckbox, PanelTableView};
use upleft_app::support::preferences::Preferences;

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("main thread")
}

fn plain_view() -> Retained<NSView> {
    NSView::new(mtm())
}

fn same(a: Option<&NSView>, b: &NSView) -> bool {
    a.is_some_and(|a| std::ptr::eq(a, b))
}

fn accessibility_value(view: &NSView) -> Option<String> {
    let value = view.accessibilityValue()?;
    if !is::<NSString>(&value) {
        return None;
    }
    Some(unsafe { &*(Retained::as_ptr(&value) as *const NSString) }.to_string())
}

/// `buttons(in:)` (ChangeReviewTests): every button under `view`.
fn buttons(view: &NSView) -> Vec<Retained<NSButton>> {
    let mut found = Vec::new();
    for subview in view.subviews().iter() {
        if let Some(button) = downcast::<NSButton>(&subview) {
            found.push(button);
        }
        found.extend(buttons(&subview));
    }
    found
}

// MARK: - PanelAccessibilityTests

/// "The task checkbox hit target stays centred on its drawn circle".
fn task_checkbox_hit_target_uses_local_coordinates() {
    let parent = NSView::initWithFrame(NSView::alloc(mtm()), rect(0.0, 0.0, 180.0, 44.0));
    let checkbox = PanelCheckbox::new(17.0, 0.5, mtm());
    checkbox.setFrameOrigin(NSPoint::new(48.0, 13.0));
    parent.addSubview(&checkbox);

    let centre = NSPoint::new(48.0 + 8.5, 13.0 + 8.5);
    assert!(same(checkbox.hitTest(centre).as_deref(), &checkbox));
    assert!(same(checkbox.hitTest(NSPoint::new(48.0 - 5.0, 13.0 + 8.5)).as_deref(), &checkbox));
    assert!(checkbox.hitTest(NSPoint::new(48.0 - 7.0, 13.0 + 8.5)).is_none());
    assert!(checkbox.hitTest(NSPoint::new(179.0, 8.5)).is_none());
}

/// "The task table claims command-N before the app menu".
fn task_table_claims_quick_add_key_equivalent() {
    let table = PanelTableView::with_frame(rect(0.0, 0.0, 300.0, 200.0), mtm());
    let claimed = Rc::new(Cell::new(false));
    let seen = claimed.clone();
    table.set_on_key_event(Some(Rc::new(move |event: &NSEvent| {
        seen.set(event.keyCode() == 45);
        seen.get()
    })));
    let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        NSEventType::KeyDown,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags::Command,
        0.0,
        0,
        None,
        &NSString::from_str("n"),
        &NSString::from_str("n"),
        false,
        45,
    )
    .expect("key event");
    let handled: bool = unsafe { msg_send![&*table, performKeyEquivalent: &*event] };
    assert!(handled);
    assert!(claimed.get());
}

fn inspector_section_navigation_stays_in_sync() {
    let host = InspectorHostView::new(NSRect::ZERO, mtm());
    host.set_content(&plain_view(), InspectorSection::Search);
    host.set_content(&plain_view(), InspectorSection::Tasks);

    host.select(InspectorSection::Search);
    assert_eq!(host.selected_section(), Some(InspectorSection::Search));
    assert_eq!(accessibility_value(&host).as_deref(), Some("Search section"));
}

/// "The resting close control is visible, accessible, and clickable".
fn inspector_close_is_always_available() {
    let host = InspectorHostView::new(NSRect::ZERO, mtm());
    let close_count = Rc::new(Cell::new(0));
    let counter = close_count.clone();
    host.set_on_close(Some(Rc::new(move || counter.set(counter.get() + 1))));
    host.set_content(&plain_view(), InspectorSection::Tasks);
    host.setFrame(rect(0.0, 0.0, 300.0, 240.0));
    host.layoutSubtreeIfNeeded();

    let close = host.close_button();
    assert_eq!(close.alphaValue(), 1.0);
    assert!(!close.isHidden());
    assert_eq!(
        close.accessibilityRole().map(|role| role.to_string()),
        Some(unsafe { NSAccessibilityButtonRole }.to_string())
    );
    assert_eq!(close.accessibilityLabel().map(|label| label.to_string()).as_deref(), Some("Close inspector"));

    let bounds = close.bounds();
    let close_point = host.convertPoint_fromView(NSPoint::new(bounds.mid_x(), bounds.mid_y()), Some(&close));
    assert!(same(host.hitTest(close_point).as_deref(), &close));
    unsafe { close.performClick(None) };
    assert_eq!(close_count.get(), 1);
    assert!(close.accessibilityPerformPress());
    assert_eq!(close_count.get(), 2);
}

// MARK: - ChangeReviewTests

/// "Every conflict action states its consequence".
fn conflict_actions_explain_themselves() {
    let bar = ConflictBarView::new_current(mtm());
    let mut by_title: HashMap<String, Retained<NSButton>> = HashMap::new();
    for button in buttons(&bar).into_iter().filter(|button| !button.title().to_string().is_empty()) {
        let previous = by_title.insert(button.title().to_string(), button);
        assert!(previous.is_none(), "duplicate button title");
    }
    let tool_tip = |title: &str| by_title[title].toolTip().map(|tip| tip.to_string()).unwrap_or_default();
    assert!(tool_tip("Review").contains("Changes nothing"));
    assert!(tool_tip("Keep Mine").contains("discarding the change on disk"));
    assert!(tool_tip("Take Theirs").contains("discarding your unsaved edits"));
}

/// "Consequences reach VoiceOver as well as the pointer".
fn conflict_consequences_are_accessible() {
    let bar = ConflictBarView::new_current(mtm());
    for button in buttons(&bar).into_iter().filter(|button| !button.title().to_string().is_empty()) {
        let help = button.accessibilityHelp().map(|help| help.to_string());
        assert!(help.as_deref().is_some_and(|help| !help.is_empty()));
        assert_eq!(help, button.toolTip().map(|tip| tip.to_string()));
    }
}

/// "The conflict bar names the situation, not just the file".
fn conflict_bar_names_the_situation() {
    let bar = ConflictBarView::new_current(mtm());
    assert!(bar.message().contains("while you were editing"));
}

/// "Neither review bar becomes first responder" (the conflict bar's half;
/// the change summary bar's is with its own panel's tests).
fn review_bars_never_take_the_keyboard() {
    let bar = ConflictBarView::new_current(mtm());
    let accepts: bool = unsafe { msg_send![&*bar, acceptsFirstResponder] };
    assert!(!accepts);
}

// MARK: - WindowChromeTests

fn inspector_host_shows_exactly_one_owned_section() {
    let host = InspectorHostView::new(NSRect::ZERO, mtm());
    let search = plain_view();
    let tasks = plain_view();

    host.set_content(&search, InspectorSection::Search);
    assert_eq!(host.selected_section(), Some(InspectorSection::Search));
    assert!(!search.isHidden());

    host.set_content(&tasks, InspectorSection::Tasks);
    assert_eq!(host.selected_section(), Some(InspectorSection::Tasks));
    assert!(search.isHidden());
    assert!(!tasks.isHidden());

    host.select(InspectorSection::Search);
    assert_eq!(host.selected_section(), Some(InspectorSection::Search));
    assert!(!search.isHidden());
    assert!(tasks.isHidden());
}

fn chrome_glass_accessibility_fallback_policy_is_explicit() {
    assert!(!ChromeGlass::supports_glass_with(true, true, false));
    assert!(!ChromeGlass::supports_glass_with(true, false, true));
    assert!(!ChromeGlass::supports_glass_with(false, false, false));
}

/// Points the home folder into the sandbox and installs the testing
/// `Preferences.shared` (every panel reads it through `PanelFont`; loading
/// the real one writes the user's global preferences). Call first in `main`.
fn sandbox() {
    let root = document_support::sandbox();
    let home = root.appending_path_component_is_directory("home", true);
    std::fs::create_dir_all(home.path()).unwrap();
    // SAFETY: called from `main` before any other thread exists.
    unsafe {
        std::env::set_var("CFFIXED_USER_HOME", home.path());
        std::env::set_var("HOME", home.path());
    }
    let preferences = Preferences::for_testing(root.appending_path_component("shared-preferences.json"), None);
    assert!(Preferences::install_shared_for_testing(preferences), "Preferences.shared was loaded too early");
}

fn main() {
    sandbox();
    main_thread::run_off_screen(&[
        ("task_checkbox_hit_target_uses_local_coordinates", task_checkbox_hit_target_uses_local_coordinates),
        ("task_table_claims_quick_add_key_equivalent", task_table_claims_quick_add_key_equivalent),
        ("inspector_section_navigation_stays_in_sync", inspector_section_navigation_stays_in_sync),
        ("inspector_close_is_always_available", inspector_close_is_always_available),
        ("conflict_actions_explain_themselves", conflict_actions_explain_themselves),
        ("conflict_consequences_are_accessible", conflict_consequences_are_accessible),
        ("conflict_bar_names_the_situation", conflict_bar_names_the_situation),
        ("review_bars_never_take_the_keyboard", review_bars_never_take_the_keyboard),
        ("inspector_host_shows_exactly_one_owned_section", inspector_host_shows_exactly_one_owned_section),
        ("chrome_glass_accessibility_fallback_policy_is_explicit", chrome_glass_accessibility_fallback_policy_is_explicit),
    ]);
}
