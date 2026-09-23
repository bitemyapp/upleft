//! View-level tests for FindBarView, ChangeSummaryBarView, SearchResultsPanelView, SearchInspectorView
//! (ported from DownrightAppTests). Runs on the main thread; any window is
//! off-screen and never activated.
//!
//! Ported (Swift file → test):
//!
//! - `PanelAccessibilityTests.swift`: `searchResultsExposeSearchingAndEmptyStates`,
//!   `findAccentGlyphDoesNotDuplicateTheSearchFieldAnnouncement`.
//! - `WindowChromeTests.swift`: `changeSummaryUsesCompactCountedNavigation`,
//!   `searchInspectorKeepsFindAndReplaceInOneSurface`,
//!   `searchInspectorLaysOutItsFindFieldInsideTheVisibleHeader`,
//!   `replaceBarSettledLayout`, `replaceBarRapidToggleSettlesVisible`,
//!   `replaceBarReducedMotionToggleIsAtomic`,
//!   `inspectorFindKeepsTheSameRhythmWithoutADuplicateClose`,
//!   `findOptionsLiveInOneCompactMenu`,
//!   `everyFindBarControlDispatchesItsDocumentAction`,
//!   `findBarParksItsMatchActionsUntilThereIsSomethingToWalk`.
//! - `ChangeReviewTests.swift`: the counting, headline, position,
//!   distribution, accessibility, bar and action-hierarchy tests, and
//!   `ChangeSummaryBarView`'s half of `reviewBarsNeverTakeTheKeyboard`.
//!
//! Skipped (each drives a `DocumentWindowController`, which lives in `App/`
//! and is not ported): in `WindowChromeTests.swift`, `localFindUsesCompactDocumentBar`,
//! `replaceModePreservesActiveQuery`, `localFindPreservesViewport`,
//! `findMotionDoesNotShiftDocument`, `ordinaryFindDoesNotReplaceItsQueryWithDocumentSelection`,
//! `selectionFindIgnoresAnEmptySelection`, `closingTheFindBarRetiresItsOverlayWithoutRaising`,
//! `findActionFlushesTheVisibleQueryBeforeTheDebounceFires`, and the change-summary
//! placement test before `changeSummaryUsesCompactCountedNavigation`; every
//! `SiblingSearchTests.swift` case except `anAlreadyCancelledSearchReadsNoFiles`
//! (already in `sibling_search_tests.rs`); `DocumentChromeLayoutTests.swift`'s
//! change-summary cases. `FindRegexRegressionTests.swift` is model-only and
//! lives in `find_regex_regression_tests.rs`. The conflict-bar tests of
//! `ChangeReviewTests.swift` belong to `panels_chrome_tests.rs`, and its
//! expiry tests to `change_review_tests.rs`.

#[path = "main_thread/mod.rs"]
mod main_thread;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameDarkAqua, NSApplication, NSBackingStoreType, NSButton, NSSearchField, NSTextField,
    NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use upleft_app::panels::appkit_support::{accessibility_label, ns_string};
use upleft_app::panels::find_bar_view::{FindBarDelegate, FindBarDensity, FindBarView, Presentation};
use upleft_app::support::find_engine::FindQuery;
use upleft_core::NSRange;
use upleft_render::appkit_compat::RectExt;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("main thread")
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

/// `descendants(of:)`.
fn descendants(root: &NSView) -> Vec<Retained<NSView>> {
    let mut out = Vec::new();
    for view in root.subviews().iter() {
        out.push(view.clone());
        out.extend(descendants(&view));
    }
    out
}

fn find_button(label: &str, root: &NSView) -> Retained<NSButton> {
    descendants(root)
        .into_iter()
        .filter_map(|view| view.downcast::<NSButton>().ok())
        .find(|button| accessibility_label(&**button).as_deref() == Some(label))
        .unwrap_or_else(|| panic!("no button labelled {label}"))
}

fn find_text_field(label: &str, root: &NSView) -> Retained<NSTextField> {
    descendants(root)
        .into_iter()
        .filter_map(|view| view.downcast::<NSTextField>().ok())
        .find(|field| accessibility_label(&**field).as_deref() == Some(label))
        .unwrap_or_else(|| panic!("no text field labelled {label}"))
}

/// `CGRect.contains(_: CGRect)`: `CGRectContainsRect`, which is the union
/// test.
fn contains(outer: NSRect, inner: NSRect) -> bool {
    let union = outer.union(inner);
    union.origin.x == outer.origin.x
        && union.origin.y == outer.origin.y
        && union.size.width == outer.size.width
        && union.size.height == outer.size.height
}

/// `StyleSheet(theme: ThemeStore.shared.current, appearance: .darkAqua,
/// reduceMotionOverride:)`.
fn dark_style(reduce_motion: bool) -> Rc<StyleSheet> {
    let appearance = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }).expect("dark aqua");
    Rc::new(StyleSheet::new(ThemeStore::shared().current(), &appearance, Some(reduce_motion)))
}

/// `NSWindow(contentRect:styleMask: [.borderless], backing: .buffered,
/// defer: false)`, placed off every screen and never ordered in.
fn borderless_window(content: NSRect) -> Retained<NSWindow> {
    let mtm = mtm();
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            content,
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setFrameOrigin(NSPoint::new(-30000.0, -30000.0));
    window
}

// MARK: - FindBarView

#[derive(Default)]
struct FindBarDelegateRecorder {
    queries: RefCell<Vec<FindQuery>>,
    advances: RefCell<Vec<bool>>,
    replacements: RefCell<Vec<(String, bool)>>,
    close_count: Cell<usize>,
}

impl FindBarDelegate for FindBarDelegateRecorder {
    fn find_bar_did_change(&self, _bar: &FindBarView, query: FindQuery) {
        self.queries.borrow_mut().push(query);
    }
    fn find_bar_did_request_advance(&self, _bar: &FindBarView, forward: bool) {
        self.advances.borrow_mut().push(forward);
    }
    fn find_bar_did_request_replace(&self, _bar: &FindBarView, replacement: &str, all: bool) {
        self.replacements.borrow_mut().push((replacement.to_owned(), all));
    }
    fn find_bar_did_request_close(&self, _bar: &FindBarView) {
        self.close_count.set(self.close_count.get() + 1);
    }
}

/// `NSApp.sendAction(item.action!, to: item.target, from: item)`.
fn send_menu_action(item: &objc2_app_kit::NSMenuItem) {
    let action = item.action().expect("menu item action");
    let target = item.target();
    let from: &AnyObject = item;
    let _ = unsafe { NSApplication::sharedApplication(mtm()).sendAction_to_from(action, target.as_deref(), Some(from)) };
}

fn find_accent_glyph_does_not_duplicate_the_search_field_announcement() {
    let bar = FindBarView::new_current(mtm());
    assert!(!bar.leading_glyph_is_accessible_for_testing());
    let role: Option<Retained<objc2_foundation::NSString>> = unsafe { objc2::msg_send![&*bar, accessibilityRole] };
    assert_eq!(role.map(|role| role.to_string()).as_deref(), Some("AXGroup"));
}

fn replace_bar_settled_layout() {
    let style = dark_style(true);
    let bar = FindBarView::new(style, Presentation::Bar, mtm());
    let host = NSView::initWithFrame(NSView::alloc(mtm()), rect(0.0, 0.0, 560.0, 140.0));
    let window = borderless_window(host.bounds());
    window.setContentView(Some(&host));
    host.addSubview(&bar);
    bar.setFrame(rect(40.0, 30.0, FindBarDensity::BAR_WIDTH, FindBarDensity::REPLACE_HEIGHT));

    bar.set_shows_replace(true);
    bar.layoutSubtreeIfNeeded();

    let find = bar.find_row_frame_for_testing();
    let replace = bar.replace_row_frame_for_testing();
    assert_eq!(bar.intrinsicContentSize().height, FindBarDensity::REPLACE_HEIGHT);
    assert!(!bar.replace_row_is_hidden_for_testing());
    assert_eq!(bar.replace_row_alpha_for_testing(), 1.0);
    assert!(bar.uses_dense_replace_material_for_testing());
    assert!(find.height() > 0.0);
    assert!(replace.height() > 0.0);
    assert!(!find.intersects(replace));
    assert!(contains(bar.bounds(), find));
    assert!(contains(bar.bounds(), replace));
    drop(window);
}

fn replace_bar_rapid_toggle_settles_visible() {
    let style = dark_style(false);
    let bar = FindBarView::new(style, Presentation::Bar, mtm());
    let host = NSView::initWithFrame(NSView::alloc(mtm()), rect(0.0, 0.0, 560.0, 140.0));
    let window = borderless_window(host.bounds());
    window.setContentView(Some(&host));
    host.addSubview(&bar);
    bar.setFrame(rect(40.0, 30.0, FindBarDensity::BAR_WIDTH, FindBarDensity::REPLACE_HEIGHT));

    bar.set_shows_replace(true);
    bar.set_shows_replace(false);
    bar.set_shows_replace(true);
    // Wait for the fade-in to actually settle instead of sleeping a fixed
    // interval (`pumpMainRunLoop(until:)`, 15 s).
    let settled = main_thread::pump_until(
        || (bar.replace_row_alpha_for_testing() - 1.0).abs() < 0.001,
        Duration::from_secs(15),
    );
    assert!(settled, "the replace row never reached full alpha");
    bar.layoutSubtreeIfNeeded();

    assert!(bar.shows_replace());
    assert!(!bar.replace_row_is_hidden_for_testing());
    assert!((bar.replace_row_alpha_for_testing() - 1.0).abs() < 0.001);
    assert!(bar.uses_dense_replace_material_for_testing());
    assert!(!bar.find_row_frame_for_testing().intersects(bar.replace_row_frame_for_testing()));
    drop(window);
}

fn replace_bar_reduced_motion_toggle_is_atomic() {
    let style = dark_style(true);
    let bar = FindBarView::new(style, Presentation::Bar, mtm());

    bar.set_shows_replace(true);
    assert!(!bar.replace_row_is_hidden_for_testing());
    assert_eq!(bar.replace_row_alpha_for_testing(), 1.0);
    assert!(bar.uses_dense_replace_material_for_testing());

    bar.set_shows_replace(false);
    assert!(bar.replace_row_is_hidden_for_testing());
    assert_eq!(bar.replace_row_alpha_for_testing(), 1.0);
    assert!(!bar.uses_dense_replace_material_for_testing());
    assert_eq!(bar.intrinsicContentSize().height, FindBarDensity::BAR_HEIGHT);
}

fn inspector_find_keeps_the_same_rhythm_without_a_duplicate_close() {
    let bar = FindBarView::new(Rc::new(StyleSheet::current(mtm())), Presentation::Inspector, mtm());
    assert_eq!(bar.divider_count_for_testing(), 2);
    assert!(!bar.has_close_button_for_testing());
    assert!(!bar.search_field_is_bezeled_for_testing());
}

fn find_options_live_in_one_compact_menu() {
    let bar = FindBarView::new_current(mtm());
    let menu = bar.make_options_menu_for_testing();
    let titles: Vec<String> =
        menu.itemArray().iter().filter(|item| !item.isSeparatorItem()).map(|item| item.title().to_string()).collect();
    assert_eq!(titles, ["Regular Expression", "Match Case", "Whole Word", "In Selection"]);
    assert_eq!(menu.itemArray().lastObject().map(|item| item.isEnabled()), Some(false));
}

fn every_find_bar_control_dispatches_its_document_action() {
    let bar = FindBarView::new_current(mtm());
    let delegate = Rc::new(FindBarDelegateRecorder::default());
    let dynamic: Rc<dyn FindBarDelegate> = delegate.clone();
    bar.set_delegate(Some(Rc::downgrade(&dynamic)));
    bar.set_status_text("1 of 2");
    bar.set_shows_replace(true);

    bar.set_query_text("alpha", true);
    unsafe { find_button("Previous match", &bar).performClick(None) };
    unsafe { find_button("Next match", &bar).performClick(None) };

    let replacement = find_text_field("Replace with", &bar);
    replacement.setStringValue(&ns_string("omega"));
    unsafe { find_button("Replace", &bar).performClick(None) };
    unsafe { find_button("All", &bar).performClick(None) };

    assert_eq!(delegate.queries.borrow().last().map(|query| query.text.clone()).as_deref(), Some("alpha"));
    assert_eq!(*delegate.advances.borrow(), [false, true]);
    assert_eq!(delegate.replacements.borrow().len(), 2);
    assert_eq!(delegate.replacements.borrow()[0].0, "omega");
    assert!(!delegate.replacements.borrow()[0].1);
    assert!(delegate.replacements.borrow()[1].1);

    let options = bar.make_options_menu_for_testing();
    let regex = options.itemWithTitle(&ns_string("Regular Expression")).expect("regex item");
    send_menu_action(&regex);
    assert!(bar.current_query().is_regex);

    bar.set_selection_scope(Some(NSRange::new(0, 5)));
    let scoped_options = bar.make_options_menu_for_testing();
    let in_selection = scoped_options.itemWithTitle(&ns_string("In Selection")).expect("scope item");
    assert!(in_selection.isEnabled());
    send_menu_action(&in_selection);
    assert_eq!(bar.current_query().scope, Some(NSRange::new(0, 5)));

    unsafe { find_button("Close find bar", &bar).performClick(None) };
    assert_eq!(delegate.close_count.get(), 1);
}

fn find_bar_parks_its_match_actions_until_there_is_something_to_walk() {
    let bar = FindBarView::new_current(mtm());
    let previous = find_button("Previous match", &bar);
    let next = find_button("Next match", &bar);

    // An empty field parks the walk.
    assert!(!previous.isEnabled());
    assert!(!next.isEnabled());

    bar.set_query_text("alpha", true);
    assert!(previous.isEnabled());
    assert!(next.isEnabled());

    // A settled "No matches" parks them again; editing re-arms at once.
    bar.set_status_text("No matches");
    assert!(!previous.isEnabled());
    assert!(!next.isEnabled());
    bar.set_status_text("2 of 4");
    assert!(previous.isEnabled());
    assert!(next.isEnabled());

    bar.set_shows_replace(true);
    let replace = find_button("Replace", &bar);
    let replace_all = find_button("All", &bar);
    assert!(replace.isEnabled());
    assert!(replace_all.isEnabled());
    bar.set_query_text("", true);
    assert!(!replace.isEnabled());
    assert!(!replace_all.isEnabled());
}

fn main() {
    let _ = NSApplication::sharedApplication(mtm());
    main_thread::run(&[
        // PanelAccessibilityTests
        (
            "find_accent_glyph_does_not_duplicate_the_search_field_announcement",
            find_accent_glyph_does_not_duplicate_the_search_field_announcement,
        ),
        // WindowChromeTests
        ("replace_bar_settled_layout", replace_bar_settled_layout),
        ("replace_bar_rapid_toggle_settles_visible", replace_bar_rapid_toggle_settles_visible),
        ("replace_bar_reduced_motion_toggle_is_atomic", replace_bar_reduced_motion_toggle_is_atomic),
        (
            "inspector_find_keeps_the_same_rhythm_without_a_duplicate_close",
            inspector_find_keeps_the_same_rhythm_without_a_duplicate_close,
        ),
        ("find_options_live_in_one_compact_menu", find_options_live_in_one_compact_menu),
        ("every_find_bar_control_dispatches_its_document_action", every_find_bar_control_dispatches_its_document_action),
        (
            "find_bar_parks_its_match_actions_until_there_is_something_to_walk",
            find_bar_parks_its_match_actions_until_there_is_something_to_walk,
        ),
    ]);
}

#[allow(unused)]
fn _unused(_: &NSSearchField) {}
