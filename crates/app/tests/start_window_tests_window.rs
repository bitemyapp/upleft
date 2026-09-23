//! Port of `Tests/DownrightAppTests/StartWindowTests.swift`: the 27 tests
//! that build the start window or its row copy (`StartWindowTests` and
//! `RecentRowSubtitleTests`). The one `DocumentStateStore` test of that file
//! lives in `start_window_tests.rs`.
//!
//! Both Swift suites are `@MainActor` (`StartWindowTests` also
//! `.serialized`), so this binary owns the main thread (`harness = false`,
//! see `main_thread`).
//!
//! Sandbox. The window reads `Preferences.shared` (the theme appearance),
//! `ThemeStore.shared` and `KeybindingStore.shared` (the shortcut hints), so
//! `main` re-runs this binary with `HOME` and `CFFIXED_USER_HOME` pointing at
//! a temporary folder and Downright's own `DOWNRIGHT_SUPPORT_DIRECTORY`
//! override inside it, installs a sandboxed `Preferences` as
//! `Preferences::shared()` (the real one publishes the Quick Look appearance
//! to the user's global preferences domain), and removes the test process's
//! own `UserDefaults` domain before and after.
//!
//! No test orders a window in: the start window is titled, and AppKit would
//! pull a titled window onto a screen. The Swift tests never show it either;
//! `layoutSubtreeIfNeeded`, `performClick`, `makeFirstResponder` and direct
//! `keyDown:` calls all work on a window that is not ordered in.
//!
//! `malformedJoinedHeading` uses the Swift test's original strings
//! (`ThiDownright …` → `Downright …`): the rebranded copy rewrites only the
//! expectation (`\bDownright\b` does not match inside `ThiDownright`), so it
//! no longer tests anything.

mod main_thread;

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{ClassType, MainThreadMarker, Message, msg_send, sel};
use objc2_foundation::NSObjectProtocol;
use objc2_app_kit::{
    NSApplication, NSButton, NSColor, NSEvent, NSEventModifierFlags, NSEventType, NSImageView, NSMenu, NSParagraphStyle,
    NSScrollView, NSTextAlignment, NSTextField, NSView, NSWindow, NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_foundation::{
    NSBundle, NSCalendar, NSCalendarOptions, NSCalendarUnit, NSDate, NSPoint, NSProcessInfo, NSString, NSUserDefaults,
};
use upleft_app::ai::document_state_store::RecentDocument;
use upleft_app::app::start_window_controller::{
    KeycapFormatter, RecentRowCopy, StartGuideOffer, StartLayout, StartWindowController,
};
use upleft_app::support::preferences::Preferences;
use upleft_foundation::date::Date;
use upleft_foundation::url::FileUrl;

/// Set (to the sandbox root) in the re-run child.
const SANDBOX_VARIABLE: &str = "UPLEFT_START_WINDOW_TESTS_SANDBOX";

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("the start window tests run on the main thread")
}

// MARK: - Helpers

/// `defer { controller.close() }`.
struct Close(Retained<StartWindowController>);

impl Drop for Close {
    fn drop(&mut self) {
        self.0.close();
    }
}

fn controller(recents: Vec<RecentDocument>) -> Retained<StartWindowController> {
    StartWindowController::new(recents, StartGuideOffer::Unavailable, mtm())
}

fn recent(path: &str, display_name: &str, first_heading: &str, last_opened: Date, word_count: isize) -> RecentDocument {
    RecentDocument {
        path: path.to_owned(),
        display_name: display_name.to_owned(),
        first_heading: first_heading.to_owned(),
        last_opened,
        word_count,
    }
}

fn window_of(controller: &StartWindowController) -> Retained<NSWindow> {
    controller.window().expect("the controller owns its window")
}

fn content_of(window: &NSWindow) -> Retained<NSView> {
    window.contentView().expect("the window has a content view")
}

fn accessibility_label(view: &NSView) -> Option<String> {
    let label: Option<Retained<NSString>> = unsafe { msg_send![view, accessibilityLabel] };
    label.map(|label| label.to_string())
}

/// `String(describing: view.accessibilityValue())`.
fn accessibility_value_description(view: &NSView) -> String {
    let value: Option<Retained<AnyObject>> = unsafe { msg_send![view, accessibilityValue] };
    match value {
        Some(value) => {
            let description: Retained<NSString> = unsafe { msg_send![&*value, description] };
            format!("Optional({description})")
        }
        None => "nil".to_owned(),
    }
}

fn subviews_of(view: &NSView) -> Vec<Retained<NSView>> {
    view.subviews().iter().collect()
}

fn collect<T: objc2::ClassType + objc2::Message + objc2::DowncastTarget>(view: &NSView, out: &mut Vec<Retained<T>>) {
    if let Some(found) = view.retain().downcast::<T>().ok() {
        out.push(found);
    }
    for subview in subviews_of(view) {
        collect(&subview, out);
    }
}

fn buttons(view: &NSView) -> Vec<Retained<NSButton>> {
    let mut out = Vec::new();
    collect(view, &mut out);
    out
}

fn text_fields(view: &NSView) -> Vec<Retained<NSTextField>> {
    let mut out = Vec::new();
    collect(view, &mut out);
    out
}

fn image_views(view: &NSView) -> Vec<Retained<NSImageView>> {
    let mut out = Vec::new();
    collect(view, &mut out);
    out
}

fn menus(view: &NSView) -> Vec<Retained<NSMenu>> {
    let mut out: Vec<Retained<NSMenu>> = view.menu().into_iter().collect();
    for subview in subviews_of(view) {
        out.extend(menus(&subview));
    }
    out
}

fn button_labelled(view: &NSView, label: &str) -> Retained<NSButton> {
    buttons(view)
        .into_iter()
        .find(|button| accessibility_label(button).as_deref() == Some(label))
        .unwrap_or_else(|| panic!("no button labelled {label:?}"))
}

fn labels(view: &NSView) -> Vec<String> {
    buttons(view).iter().filter_map(|button| accessibility_label(button)).collect()
}

fn titles(menu: &NSMenu) -> Vec<String> {
    menu.itemArray().iter().map(|item| item.title().to_string()).collect()
}

fn same_object(a: &AnyObject, b: &AnyObject) -> bool {
    std::ptr::eq(a, b)
}

fn midpoint(view: &NSView) -> NSPoint {
    let bounds = view.bounds();
    NSPoint::new(bounds.origin.x + bounds.size.width / 2.0, bounds.origin.y + bounds.size.height / 2.0)
}

// MARK: - StartWindowTests

fn empty_start_window_has_clear_primary_actions() {
    let controller = controller(Vec::new());
    let _close = Close(controller.clone());

    let window = window_of(&controller);
    content_of(&window).layoutSubtreeIfNeeded();

    // Fixed size, and fixed to the layout's own constant — the assertion is
    // "this window does not resize", not "this window is 576pt tall".
    assert_eq!(window.minSize(), StartLayout::WINDOW_SIZE);
    assert_eq!(window.maxSize(), StartLayout::WINDOW_SIZE);
    assert!(!window.styleMask().contains(NSWindowStyleMask::Resizable));
    assert_eq!(window.titleVisibility(), NSWindowTitleVisibility::Hidden);
    let content = content_of(&window);
    assert_eq!(accessibility_label(&content).as_deref(), Some("Upleft start window"));
    assert!(window.initialFirstResponder().is_some());
    assert!(!subviews_of(&content).iter().any(|view| view.isKindOfClass(NSScrollView::class())));

    let labels = labels(&content);
    assert!(labels.iter().any(|label| label == "Open File"));
    assert!(labels.iter().any(|label| label == "New Document"));
    let text: Vec<String> = text_fields(&content).iter().map(|field| field.stringValue().to_string()).collect();
    assert!(!text.iter().any(|text| text == "You can also drop a file anywhere"));
}

fn primary_actions_use_balanced_control_wells() {
    let controller = controller(Vec::new());
    let _close = Close(controller.clone());

    let window = window_of(&controller);
    let content = content_of(&window);
    content.layoutSubtreeIfNeeded();

    let open = button_labelled(&content, "Open File");
    let create = button_labelled(&content, "New Document");
    let open_center = open.convertPoint_toView(midpoint(&open), Some(&content));
    let create_center = create.convertPoint_toView(midpoint(&create), Some(&content));

    assert!((open_center.y - create_center.y).abs() < 0.5);
    assert!(create_center.x > open_center.x);
    // Open and New are peer entry points. Keep their wells equal so the
    // longer title does not accidentally become the visual primary.
    assert!((open.frame().size.width - StartLayout::ACTION_BUTTON_WIDTH).abs() < 1.0);
    assert!((create.frame().size.width - StartLayout::ACTION_BUTTON_WIDTH).abs() < 1.0);
    assert!((open.frame().size.width - create.frame().size.width).abs() < 1.0);
    assert_eq!(open.frame().size.height, StartLayout::BUTTON_HEIGHT);
    assert_eq!(create.frame().size.height, StartLayout::BUTTON_HEIGHT);
}

fn open_file_shortcut_sits_close_to_its_label() {
    let controller = controller(Vec::new());
    let _close = Close(controller.clone());

    let window = window_of(&controller);
    let content = content_of(&window);
    content.layoutSubtreeIfNeeded();

    let open = button_labelled(&content, "Open File");
    let labels = text_fields(&open);
    let title = labels.iter().find(|field| field.stringValue().to_string() == "Open File").expect("the title label");
    // Structural, not content-based: the only other text field in the
    // button is the shortcut, whatever the user has bound it to.
    let shortcut = labels.iter().find(|field| !std::ptr::eq(Retained::as_ptr(field), Retained::as_ptr(title))).expect("the shortcut label");

    // The label and the shortcut share the button's shell as a superview,
    // so a frame comparison in that space is the real rendered gap.
    // Matching the two buttons' widths used to stretch this from the 12pt
    // design constant to ~48pt of dead air.
    let gap = shortcut.frame().origin.x - (title.frame().origin.x + title.frame().size.width);
    // The compact keycap group keeps a deliberate 8pt optical gap after the
    // title without making the shortcut feel detached from its label.
    assert!(gap >= 8.0, "gap {gap}");
    assert!(gap <= 22.0, "gap {gap}");
    assert!(shortcut.frame().size.width >= 30.0);
    assert!(shortcut.frame().size.height >= 20.0);
    let shortcut_in_open = shortcut.convertRect_toView(shortcut.bounds(), Some(&open));
    assert!(shortcut_in_open.origin.x + shortcut_in_open.size.width <= open.bounds().size.width - 12.0);
}

fn tour_button_sizes_to_its_own_label() {
    // The tour carries no shortcut, so its button must close up around its
    // label — the same content-hugging intrinsic the two action buttons use,
    // exercised through the no-shortcut path.
    let controller = StartWindowController::new(Vec::new(), StartGuideOffer::Secondary, mtm());
    let _close = Close(controller.clone());

    let window = window_of(&controller);
    let content = content_of(&window);
    content.layoutSubtreeIfNeeded();

    let tour = button_labelled(&content, "Take the Tour");
    assert!((tour.frame().size.width - tour.intrinsicContentSize().width).abs() < 1.0);
    assert!(tour.frame().size.width > 0.0);
}

fn recent_rows_prefer_heading_over_machine_generated_names() {
    let recent = recent(
        "/tmp/T/EditingKeyRepro-92C5F190-B66C-4E83-ABBF-5A58BB0EAFC3.md",
        "EditingKeyRepro-92C5F190-B66C-4E83-ABBF-5A58BB0EAFC3",
        "Editing Keys",
        Date::now(),
        12,
    );
    let controller = controller(vec![recent]);
    let _close = Close(controller.clone());

    let window = window_of(&controller);
    content_of(&window).layoutSubtreeIfNeeded();

    let labels = labels(&content_of(&window));
    assert!(labels.iter().any(|label| label == "Open Editing Keys"));
    assert!(!labels.iter().any(|label| label.contains("92C5F190")));
}

fn recent_rows_use_document_icons_and_context_menu_for_clearing() {
    let recent = recent("/tmp/downright-start-context.md", "note", "Planning", Date::now(), 1);
    let controller = controller(vec![recent]);
    let _close = Close(controller.clone());

    let window = window_of(&controller);
    let content = content_of(&window);
    content.layoutSubtreeIfNeeded();

    let labels = labels(&content);
    assert!(!labels.iter().any(|label| label == "Clear recent files"));

    let document_icons: Vec<_> = image_views(&content)
        .into_iter()
        .filter(|view| accessibility_label(view).as_deref() == Some("Markdown document"))
        .collect();
    assert_eq!(document_icons.len(), 1);

    let menus = menus(&content);
    assert!(menus.iter().any(|menu| titles(menu) == ["Clear Recent Files…"]));
}

fn duplicate_titles_are_disambiguated_by_folder() {
    let first = recent("/tmp/a/DownrightFresh.md", "DownrightFresh", "", Date::now(), 1);
    let second = recent("/tmp/b/DownrightFresh.md", "DownrightFresh", "", Date::now().adding(-10.0), 1);
    let controller = controller(vec![first, second]);
    let _close = Close(controller.clone());

    let window = window_of(&controller);
    content_of(&window).layoutSubtreeIfNeeded();

    let labels = labels(&content_of(&window));
    assert!(labels.iter().any(|label| label == "Open DownrightFresh (a)"));
    assert!(labels.iter().any(|label| label == "Open DownrightFresh (b)"));
}

fn same_folder_duplicates_get_unique_fragments() {
    let first = recent(
        "/tmp/T/EditingKeyRepro-11111111-B66C-4E83-ABBF-5A58BB0EAFC3.md",
        "EditingKeyRepro-11111111-B66C-4E83-ABBF-5A58BB0EAFC3",
        "Title",
        Date::now(),
        1,
    );
    let second = recent(
        "/tmp/T/EditingKeyRepro-22222222-B66C-4E83-ABBF-5A58BB0EAFC3.md",
        "EditingKeyRepro-22222222-B66C-4E83-ABBF-5A58BB0EAFC3",
        "Title",
        Date::now().adding(-10.0),
        1,
    );
    let controller = controller(vec![first, second]);
    let _close = Close(controller.clone());

    let window = window_of(&controller);
    content_of(&window).layoutSubtreeIfNeeded();

    let labels = labels(&content_of(&window));
    assert!(labels.iter().any(|label| label == "Open EditingKeyRepro · 11111111"));
    assert!(labels.iter().any(|label| label == "Open EditingKeyRepro · 22222222"));
}

fn generic_headings_fall_back_to_file_name() {
    let recent = recent(
        "/tmp/T/EditingKeyRepro-92C5F190-B66C-4E83-ABBF-5A58BB0EAFC3.md",
        "EditingKeyRepro-92C5F190-B66C-4E83-ABBF-5A58BB0EAFC3",
        "Title",
        Date::now(),
        12,
    );
    let controller = controller(vec![recent]);
    let _close = Close(controller.clone());

    let window = window_of(&controller);
    content_of(&window).layoutSubtreeIfNeeded();

    let labels = labels(&content_of(&window));
    assert!(labels.iter().any(|label| label == "Open EditingKeyRepro"));
    assert!(!labels.iter().any(|label| label == "Open Title"));
}

fn clicking_a_recent_row_routes_its_url_to_the_owner() {
    let recent = recent("/tmp/downright-start-window-click.md", "note", "Planning", Date::now(), 1);
    let path = recent.path.clone();
    let controller = controller(vec![recent]);
    let _close = Close(controller.clone());

    let opened_url: Rc<RefCell<Option<FileUrl>>> = Rc::new(RefCell::new(None));
    let sink = opened_url.clone();
    controller.set_on_open(Some(Rc::new(move |url| *sink.borrow_mut() = Some(url))));

    let window = window_of(&controller);
    content_of(&window).layoutSubtreeIfNeeded();
    let recent_button = button_labelled(&content_of(&window), "Open note");
    // SAFETY: a nil sender.
    unsafe { recent_button.performClick(None) };

    assert_eq!(opened_url.borrow().as_ref().map(FileUrl::path), Some(path));
}

fn primary_action_buttons_route_open_and_new_callbacks() {
    let controller = controller(Vec::new());
    let _close = Close(controller.clone());

    let opened_panel = Rc::new(Cell::new(false));
    let created_document = Rc::new(Cell::new(false));
    let (panel, created) = (opened_panel.clone(), created_document.clone());
    controller.set_on_open_panel(Some(Rc::new(move || panel.set(true))));
    controller.set_on_new(Some(Rc::new(move || created.set(true))));

    let window = window_of(&controller);
    content_of(&window).layoutSubtreeIfNeeded();
    let open = button_labelled(&content_of(&window), "Open File");
    let create = button_labelled(&content_of(&window), "New Document");

    // SAFETY: a nil sender.
    unsafe {
        open.performClick(None);
        create.performClick(None);
    }

    assert!(opened_panel.get());
    assert!(created_document.get());
}

fn reload_recents_swaps_empty_and_populated_states() {
    let controller = controller(Vec::new());
    let _close = Close(controller.clone());

    let window = window_of(&controller);
    content_of(&window).layoutSubtreeIfNeeded();
    assert!(!labels(&content_of(&window)).iter().any(|label| label == "Open note"));

    let recent = recent("/tmp/downright-start-window-reload.md", "note", "Planning", Date::now(), 3);
    controller.reload_recents(&[recent]);
    content_of(&window).layoutSubtreeIfNeeded();

    assert!(labels(&content_of(&window)).iter().any(|label| label == "Open note"));

    controller.reload_recents(&[]);
    content_of(&window).layoutSubtreeIfNeeded();

    assert!(!labels(&content_of(&window)).iter().any(|label| label == "Open note"));
}

fn reload_updates_recent_metadata_when_path_stays_the_same() {
    let path = "/tmp/downright-start-window-metadata.md";
    // `Calendar.current.date(byAdding: .day, value: -1, to: Date()) ??
    // Date().addingTimeInterval(-86_400)`.
    let yesterday = NSCalendar::currentCalendar()
        .dateByAddingUnit_value_toDate_options(NSCalendarUnit::Day, -1, &NSDate::new(), NSCalendarOptions::empty())
        .map(|date| Date::from_reference(date.timeIntervalSinceReferenceDate()))
        .unwrap_or_else(|| Date::now().adding(-86_400.0));
    let older = recent(path, "note", "Planning", yesterday, 3);
    let controller = controller(vec![older]);
    let _close = Close(controller.clone());

    let window = window_of(&controller);
    content_of(&window).layoutSubtreeIfNeeded();
    let button = button_labelled(&content_of(&window), "Open note");
    let before = accessibility_value_description(&button);

    let newer = recent(path, "note", "Planning", Date::now(), 3);
    controller.reload_recents(&[newer]);
    content_of(&window).layoutSubtreeIfNeeded();

    let refreshed = button_labelled(&content_of(&window), "Open note");
    assert_ne!(accessibility_value_description(&refreshed), before);
}

fn dismiss_closes_the_window() {
    let controller = controller(Vec::new());
    let _ = window_of(&controller);

    let finished = Rc::new(Cell::new(false));
    let flag = finished.clone();
    controller.dismiss(false, Some(Box::new(move || flag.set(true))));
    assert!(finished.get());
    assert!(!controller.window().is_some_and(|window| window.isVisible()));
}

fn primary_and_recent_buttons_are_enabled_and_fire() {
    let recent = recent("/tmp/downright-start-enabled.md", "note", "", Date::now(), 1);
    let path = recent.path.clone();
    let controller = controller(vec![recent]);
    let _close = Close(controller.clone());

    let opened_panel = Rc::new(Cell::new(false));
    let created = Rc::new(Cell::new(false));
    let opened_url: Rc<RefCell<Option<FileUrl>>> = Rc::new(RefCell::new(None));
    let (panel, create_flag, sink) = (opened_panel.clone(), created.clone(), opened_url.clone());
    controller.set_on_open_panel(Some(Rc::new(move || panel.set(true))));
    controller.set_on_new(Some(Rc::new(move || create_flag.set(true))));
    controller.set_on_open(Some(Rc::new(move |url| *sink.borrow_mut() = Some(url))));

    let window = window_of(&controller);
    content_of(&window).layoutSubtreeIfNeeded();

    let all = buttons(&content_of(&window));
    for button in &all {
        assert!(button.isEnabled());
    }

    let find = |label: &str| {
        all.iter().find(|button| accessibility_label(button).as_deref() == Some(label)).cloned().expect(label)
    };
    let open = find("Open File");
    let create = find("New Document");
    let recent_button = find("Open note");

    // SAFETY: a nil sender.
    unsafe {
        open.performClick(None);
        create.performClick(None);
        recent_button.performClick(None);
    }

    assert!(opened_panel.get());
    assert!(created.get());
    assert_eq!(opened_url.borrow().as_ref().map(FileUrl::path), Some(path));
}

fn arrow_keys_move_focus_across_recent_rows_and_return_opens() {
    let first = recent("/tmp/downright-kb-first.md", "note", "", Date::now(), 1);
    let second = recent("/tmp/downright-kb-second.md", "other", "", Date::now().adding(-10.0), 1);
    let first_path = first.path.clone();
    let controller = controller(vec![first, second]);
    let _close = Close(controller.clone());

    let opened_url: Rc<RefCell<Option<FileUrl>>> = Rc::new(RefCell::new(None));
    let sink = opened_url.clone();
    controller.set_on_open(Some(Rc::new(move |url| *sink.borrow_mut() = Some(url))));

    let window = window_of(&controller);
    let content = content_of(&window);
    content.layoutSubtreeIfNeeded();

    let open = button_labelled(&content, "Open File");
    window.makeFirstResponder(Some(&open));

    let key = |key_code: u16| -> Retained<NSEvent> {
        NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            NSEventType::KeyDown,
            NSPoint::new(0.0, 0.0),
            NSEventModifierFlags::empty(),
            0.0,
            window.windowNumber(),
            None,
            &NSString::from_str(""),
            &NSString::from_str(""),
            false,
            key_code,
        )
        .expect("a key event")
    };

    let focused_label = || -> Option<String> {
        let responder = window.firstResponder()?;
        let view = responder.downcast::<NSView>().ok()?;
        accessibility_label(&view)
    };

    // Down lands on the first recent row.
    content.keyDown(&key(125));
    assert_eq!(focused_label().as_deref(), Some("Open note"));

    // Down again moves to the second row.
    content.keyDown(&key(125));
    assert_eq!(focused_label().as_deref(), Some("Open other"));

    // Up returns to the first.
    content.keyDown(&key(126));
    assert_eq!(focused_label().as_deref(), Some("Open note"));

    // Return opens the focused row.
    content.keyDown(&key(36));
    assert_eq!(opened_url.borrow().as_ref().map(FileUrl::path), Some(first_path));
}

fn hero_stays_sticky_and_anchored_regardless_of_recent_count() {
    let empty_controller = controller(Vec::new());
    let _close_empty = Close(empty_controller.clone());
    let empty_window = window_of(&empty_controller);
    content_of(&empty_window).layoutSubtreeIfNeeded();
    let empty_open = button_labelled(&content_of(&empty_window), "Open File");
    let empty_open_frame = empty_open.convertRect_toView(empty_open.bounds(), None);

    let one_recent = recent("/tmp/single.md", "single", "Single Doc", Date::now(), 10);
    let single_controller = controller(vec![one_recent]);
    let _close_single = Close(single_controller.clone());
    let single_window = window_of(&single_controller);
    content_of(&single_window).layoutSubtreeIfNeeded();
    let single_open = button_labelled(&content_of(&single_window), "Open File");
    let single_open_frame = single_open.convertRect_toView(single_open.bounds(), None);

    // The top position of the hero actions must remain fixed and sticky
    // regardless of recent file count.
    assert!((empty_open_frame.origin.y - single_open_frame.origin.y).abs() < 1.0);
}

fn keycap_badges_are_centered_and_symmetrical() {
    let formatted = KeycapFormatter::format("⌘N", &NSColor::whiteColor());
    assert_eq!(formatted.string().to_string(), "⌘N");
    // SAFETY: AppKit's attribute-name constant; index 0 is inside the string.
    let paragraph = unsafe {
        formatted.attribute_atIndex_effectiveRange(objc2_app_kit::NSParagraphStyleAttributeName, 0, std::ptr::null_mut())
    }
    .and_then(|value| value.downcast::<NSParagraphStyle>().ok());
    assert_eq!(paragraph.map(|paragraph| paragraph.alignment()), Some(NSTextAlignment::Center));
}

fn recent_row_menu_acts_on_the_row_rather_than_the_whole_list() {
    let recent = recent("/tmp/downright-row-menu-fixture.md", "fixture", "Fixture", Date::now(), 12);
    let path = recent.path.clone();
    let controller = controller(vec![recent]);
    let _close = Close(controller.clone());
    let window = window_of(&controller);
    content_of(&window).layoutSubtreeIfNeeded();

    // The assertion is that a row's menu is about *that file* and keeps the
    // list-wide clear last, behind a separator — not that it holds three
    // particular verbs.
    let row_menu = menus(&content_of(&window))
        .into_iter()
        .find(|menu| titles(menu).iter().any(|title| title == "Remove from Recents"))
        .expect("a row menu");
    assert_eq!(titles(&row_menu).last().map(String::as_str), Some("Clear Recent Files…"));
    let items: Vec<_> = row_menu.itemArray().iter().collect();
    let file_actions: Vec<_> = items.iter().take_while(|item| !item.isSeparatorItem()).collect();
    assert!(file_actions.iter().all(|item| {
        item.representedObject()
            .and_then(|object| object.downcast::<NSString>().ok())
            .is_some_and(|string| string.to_string() == path)
    }));

    let remove = items.iter().find(|item| item.title().to_string() == "Remove from Recents").expect("the remove item");
    let target = remove.target().expect("the remove item has a target");
    assert!(same_object(&target, &controller));
    assert_eq!(remove.action(), Some(sel!(removeRecent:)));

    let removed: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let sink = removed.clone();
    controller.set_on_remove_recent(Some(Rc::new(move |path| *sink.borrow_mut() = Some(path))));
    controller.remove_recent(remove);
    assert_eq!(removed.borrow().as_deref(), Some(path.as_str()));
}

// MARK: - RecentRowSubtitleTests ("Recent row subtitles")

fn row(path: &str, name: &str, heading: &str) -> RecentDocument {
    recent(path, name, heading, Date::from_1970(1_000_000.0), 100)
}

/// "The heading leads, with the folder behind it".
fn heading_and_folder() {
    let row = row("/w/watchtest/notes.md", "notes", "Agent output");
    assert_eq!(RecentRowCopy::subtitle(&row, "notes"), "Agent output  ·  watchtest");
}

/// "A stale joined heading is repaired for the welcome row".
fn malformed_joined_heading() {
    let row = row("/tmp/downright-live-selection-2.md", "downright-live-selection-2", "ThiDownright renderer showcase");
    assert_eq!(
        RecentRowCopy::subtitle(&row, "downright-live-selection-2"),
        "Downright renderer showcase  ·  tmp"
    );
}

/// "Recent timestamps use calendar labels".
fn calendar_timestamps() {
    let calendar = NSCalendar::currentCalendar();
    let today = calendar.startOfDayForDate(&NSDate::new()).dateByAddingTimeInterval(12.0 * 60.0 * 60.0);
    let yesterday = calendar
        .dateByAddingUnit_value_toDate_options(NSCalendarUnit::Day, -1, &today, NSCalendarOptions::empty())
        .expect("yesterday");
    let older = calendar
        .dateByAddingUnit_value_toDate_options(NSCalendarUnit::Day, -3, &today, NSCalendarOptions::empty())
        .expect("three days earlier");
    let date = |date: &NSDate| Date::from_reference(date.timeIntervalSinceReferenceDate());

    assert_eq!(RecentRowCopy::timestamp(&recent("/today.md", "today", "", date(&today), 0)), "Today");
    assert_eq!(RecentRowCopy::timestamp(&recent("/yesterday.md", "yesterday", "", date(&yesterday), 0)), "Yesterday");
    let older_label = RecentRowCopy::timestamp(&recent("/older.md", "older", "", date(&older), 0));
    assert!(!older_label.contains("ago"));
}

/// "A heading that matches the folder is said once": `README` in
/// `Downright/` whose first heading is "Upleft" would read "Upleft · Upleft".
fn heading_matching_folder_is_not_repeated() {
    let row = row("/w/Upleft/README.md", "README", "Upleft");
    assert_eq!(RecentRowCopy::subtitle(&row, "README"), "Upleft");
}

/// "A heading that just restates the title gives way to the folder".
fn heading_echoing_title_falls_back_to_folder() {
    let row = row("/w/plans/Release Plan.md", "Release Plan", "Release plan");
    assert_eq!(RecentRowCopy::subtitle(&row, "Release Plan"), "plans");
}

/// "No heading leaves the folder".
fn no_heading_falls_back_to_folder() {
    let row = row("/w/scratchpad/unicode.md", "unicode", "");
    assert_eq!(RecentRowCopy::subtitle(&row, "unicode"), "scratchpad");
}

/// "Neither a heading nor a folder leaves the line empty rather than wrong".
fn nothing_to_say() {
    let row = row("/notes.md", "notes", "");
    assert_eq!(RecentRowCopy::subtitle(&row, "notes"), "");
}

/// "Echo detection folds case and punctuation": case and punctuation must
/// not make two spellings of one phrase look like two different facts.
fn echo_folding() {
    assert!(RecentRowCopy::echoes("Release Plan", "release-plan"));
    assert!(RecentRowCopy::echoes("Setup", "Setup Guide"));
    assert!(!RecentRowCopy::echoes("Agent output", "notes"));
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
        "upleft-start-window-tests-{}",
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
        ("empty_start_window_has_clear_primary_actions", empty_start_window_has_clear_primary_actions),
        ("primary_actions_use_balanced_control_wells", primary_actions_use_balanced_control_wells),
        ("open_file_shortcut_sits_close_to_its_label", open_file_shortcut_sits_close_to_its_label),
        ("tour_button_sizes_to_its_own_label", tour_button_sizes_to_its_own_label),
        ("recent_rows_prefer_heading_over_machine_generated_names", recent_rows_prefer_heading_over_machine_generated_names),
        (
            "recent_rows_use_document_icons_and_context_menu_for_clearing",
            recent_rows_use_document_icons_and_context_menu_for_clearing,
        ),
        ("duplicate_titles_are_disambiguated_by_folder", duplicate_titles_are_disambiguated_by_folder),
        ("same_folder_duplicates_get_unique_fragments", same_folder_duplicates_get_unique_fragments),
        ("generic_headings_fall_back_to_file_name", generic_headings_fall_back_to_file_name),
        ("clicking_a_recent_row_routes_its_url_to_the_owner", clicking_a_recent_row_routes_its_url_to_the_owner),
        ("primary_action_buttons_route_open_and_new_callbacks", primary_action_buttons_route_open_and_new_callbacks),
        ("reload_recents_swaps_empty_and_populated_states", reload_recents_swaps_empty_and_populated_states),
        (
            "reload_updates_recent_metadata_when_path_stays_the_same",
            reload_updates_recent_metadata_when_path_stays_the_same,
        ),
        ("dismiss_closes_the_window", dismiss_closes_the_window),
        ("primary_and_recent_buttons_are_enabled_and_fire", primary_and_recent_buttons_are_enabled_and_fire),
        (
            "arrow_keys_move_focus_across_recent_rows_and_return_opens",
            arrow_keys_move_focus_across_recent_rows_and_return_opens,
        ),
        (
            "hero_stays_sticky_and_anchored_regardless_of_recent_count",
            hero_stays_sticky_and_anchored_regardless_of_recent_count,
        ),
        ("keycap_badges_are_centered_and_symmetrical", keycap_badges_are_centered_and_symmetrical),
        (
            "recent_row_menu_acts_on_the_row_rather_than_the_whole_list",
            recent_row_menu_acts_on_the_row_rather_than_the_whole_list,
        ),
        ("heading_and_folder", heading_and_folder),
        ("malformed_joined_heading", malformed_joined_heading),
        ("calendar_timestamps", calendar_timestamps),
        ("heading_matching_folder_is_not_repeated", heading_matching_folder_is_not_repeated),
        ("heading_echoing_title_falls_back_to_folder", heading_echoing_title_falls_back_to_folder),
        ("no_heading_falls_back_to_folder", no_heading_falls_back_to_folder),
        ("nothing_to_say", nothing_to_say),
        ("echo_folding", echo_folding),
    ]);
    remove_standard_domain();
}
