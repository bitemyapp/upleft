//! View-level tests for TaskPanelView, TaskSectionBarView
//! (ported from DownrightAppTests). Runs on the main thread; any window is
//! off-screen and never activated.
//!
//! Ported from `Tests/DownrightAppTests/PanelAccessibilityTests.swift`: every
//! test that exercises the task panel (including the two that exercise its
//! `PanelChrome` building blocks as the panel configures them: the 17-point
//! task checkbox's hit target and the task table's ⌘N key equivalent).
//!
//! Added here (no Swift original): `task_section_bar_summarises_segments`,
//! `task_panel_builds_agent_5000_within_budget`, and two tests of the
//! animated paths the conformance scenes cannot reach (they force Reduce
//! Motion on): `animated_row_rebuilds_keep_the_table_consistent` and
//! `completion_holds_the_row_until_the_deferred_rebuild`. Their window is
//! borderless at (-30000, -30000) and never ordered in or activated. And
//! `selected_task_past_a_shorter_plan_traps_as_in_swift`, which pins a
//! reproduced Downright trap.
//!
//! Skipped, with reasons:
//! - `PanelAccessibilityTests.searchResultsExposeSearchingAndEmptyStates`,
//!   `findAccentGlyphDoesNotDuplicateTheSearchFieldAnnouncement`,
//!   `inspectorSectionNavigationStaysInSync`,
//!   `inspectorCloseIsAlwaysAvailable`: other panels (SearchResultsPanelView,
//!   FindBarView, InspectorHostView), not this group.
//! - `DocumentChromeLayoutTests.taskPanelOpensUsable`,
//!   `taskPanelRendersBodyAndHeader`, `menuAndRingOpenTasks`, every
//!   `FloatingTaskPanelTests` test, `WindowChromeTests.floatingTaskPanelFitsItsFooterRow`
//!   and the task-panel step of `SiblingSearchTests`: they open the panel
//!   through `DocumentWindowController` (App/, not ported).

#[path = "main_thread/mod.rs"]
mod main_thread;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Instant;

use objc2::{MainThreadMarker, MainThreadOnly};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSAccessibility, NSAppearance, NSAppearanceNameAqua, NSBackingStoreType, NSButton, NSEvent, NSEventModifierFlags,
    NSEventType, NSScrollView, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSIndexSet, NSPoint, NSRect, NSSize, NSString};
use upleft_app::panels::panel_chrome::{PanelCheckbox, PanelTableView};
use upleft_app::panels::task_panel_view::{TaskPanelDelegate, TaskPanelView};
use upleft_app::panels::task_section_bar_view::TaskSectionBarView;
use upleft_core::NSRange;
use upleft_core::model::TaskItem;
use upleft_core::parser::MarkdownParser;
use upleft_core::task_worklist::TaskWorklist;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("main thread")
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

#[derive(Default)]
struct TaskDelegateSpy {
    additions: RefCell<Vec<(String, Option<isize>)>>,
}

impl TaskPanelDelegate for TaskDelegateSpy {
    fn task_panel_did_toggle_task_at(&self, _panel: &TaskPanelView, _mark_offset: isize) {}
    fn task_panel_did_select_task_at(&self, _panel: &TaskPanelView, _content_offset: isize) {}
    fn task_panel_did_request_new_task(&self, _panel: &TaskPanelView, text: &str, heading_index: Option<isize>) {
        self.additions.borrow_mut().push((text.to_owned(), heading_index));
    }
    fn task_panel_did_move_task(&self, _panel: &TaskPanelView, _task_index: isize, _before: Option<isize>) {}
}

/// `makeTask(_:checked:at:headingIndex:indent:)` with the default heading
/// and indent.
fn make_task(text: &str, checked: bool, location: isize) -> TaskItem {
    TaskItem::new(checked, NSRange::new(location + 3, 1), NSRange::new(location, 12), text, None, 0)
}

fn set_frame_size(view: &NSView, width: f64, height: f64) {
    view.setFrame(rect(0.0, 0.0, width, height));
}

fn accessibility_value(view: &NSView) -> Option<String> {
    let value: Option<Retained<AnyObject>> = view.accessibilityValue();
    value.and_then(|value| value.downcast::<NSString>().ok()).map(|value| value.to_string())
}

// MARK: - PanelAccessibilityTests

/// "The task checkbox hit target stays centred on its drawn circle"
fn task_checkbox_hit_target_uses_local_coordinates() {
    let mtm = mtm();
    let parent = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, 180.0, 44.0));
    let checkbox = PanelCheckbox::new(17.0, 0.5, mtm);
    checkbox.setFrameOrigin(NSPoint::new(48.0, 13.0));
    parent.addSubview(&checkbox);

    let hit = |point: NSPoint| -> Option<Retained<NSView>> { checkbox.hitTest(point) };
    let is_checkbox = |view: Option<Retained<NSView>>| {
        view.is_some_and(|view| std::ptr::eq(&*view as *const NSView, &**checkbox as *const NSView))
    };
    // `hitTest` receives its point in the receiver's *superview* space.
    let centre = NSPoint::new(48.0 + 8.5, 13.0 + 8.5);
    assert!(is_checkbox(hit(centre)));
    // The 6pt slack around the drawn box still belongs to it.
    assert!(is_checkbox(hit(NSPoint::new(48.0 - 5.0, 13.0 + 8.5))));
    // Beyond the slack the box must not claim hits.
    assert!(hit(NSPoint::new(48.0 - 7.0, 13.0 + 8.5)).is_none());
    // Nothing outside the parent's bounds routes to the box.
    assert!(hit(NSPoint::new(179.0, 8.5)).is_none());
}

/// "Narrow task panels remeasure wrapped labels at their real width"
fn narrow_task_panel_measures_wrapped_rows_at_live_width() {
    let view = TaskPanelView::new_current(mtm());
    view.set_tasks(vec![make_task(
        "A long task label that must wrap when the floating card is narrowed",
        false,
        0,
    )]);

    set_frame_size(&view, 220.0, 320.0);
    view.layoutSubtreeIfNeeded();
    let narrow = view.measured_list_height_for_testing();

    set_frame_size(&view, 300.0, 320.0);
    view.layoutSubtreeIfNeeded();
    let wide = view.measured_list_height_for_testing();

    assert!(narrow > wide, "narrow {narrow} should exceed wide {wide}");
}

fn task_panel_summarises_a_finished_plan() {
    let view = TaskPanelView::new_current(mtm());
    view.set_tasks(vec![make_task("Done", true, 0)]);

    assert_eq!(accessibility_value(&view).as_deref(), Some("1 task done"));
    assert_eq!(view.status_line_for_testing(), "1 task done");
    assert_eq!(view.caption_for_testing(), "1 task done");
    // A section with nothing left lists its finished work instead of piling
    // it.
    assert_eq!(view.visible_task_count_for_testing(), 1);
    assert_eq!(view.pile_row_count_for_testing(), 0, "a finished section should not pile");
}

/// The pile still does its job where it earns it: a section that has both
/// finished and unfinished work leads with what is left.
fn task_panel_piles_completed_work_while_work_remains() {
    let view = TaskPanelView::new_current(mtm());
    view.set_tasks(vec![make_task("Done", true, 0), make_task("Open", false, 13)]);

    assert_eq!(view.pile_row_count_for_testing(), 1, "a mixed section should pile its done work");
    assert_eq!(view.visible_task_count_for_testing(), 1, "only the open task lists");

    view.set_completed_pile_expanded_for_testing(true, 0);
    assert_eq!(view.visible_task_count_for_testing(), 2);
}

fn task_panel_lists_open_work_first_without_losing_progress() {
    let view = TaskPanelView::new_current(mtm());
    view.set_tasks(vec![make_task("Done", true, 0), make_task("Open", false, 13)]);

    assert_eq!(view.progress(), (1, 2));
    assert_eq!(view.preferred_width(), 300.0);
    assert_eq!(view.status_line_for_testing(), "1 of 2 done · next: Open");
    assert_eq!(view.caption_for_testing(), "1 of 2 done");
    // Open work lists; the finished task waits in the collapsed pile.
    assert_eq!(view.visible_task_count_for_testing(), 1);

    view.set_completed_pile_expanded_for_testing(true, 0);
    assert_eq!(view.visible_task_count_for_testing(), 2);
    assert_eq!(view.progress(), (1, 2));
}

/// Runs a test body in an autorelease pool and checks the panel it returns
/// is gone afterwards, as Swift's ARC frees the test's `view` at scope end.
/// Quick add schedules a main-queue block (`focusAddField`) that reads the
/// table through `[weak self]`; a panel kept alive past the test would run
/// it against a table the test has since emptied, which NSTableView traps
/// (as Downright would).
fn pooled(body: impl FnOnce() -> Retained<TaskPanelView>) {
    let weak = objc2::rc::autoreleasepool(|_| {
        let panel = body();
        objc2::rc::Weak::from_retained(&panel)
    });
    assert!(weak.load().is_none(), "the test's panel outlives the test");
}

fn task_panel_empty_state_points_at_quick_add() {
    pooled(task_panel_empty_state_points_at_quick_add_body);
}

fn task_panel_empty_state_points_at_quick_add_body() -> Retained<TaskPanelView> {
    let view = TaskPanelView::new_current(mtm());
    let delegate = Rc::new(TaskDelegateSpy::default());
    view.set_delegate(Some(Rc::downgrade(&delegate) as Weak<dyn TaskPanelDelegate>));
    assert_eq!(accessibility_value(&view).as_deref(), Some("No tasks"));
    assert_eq!(view.visible_task_count_for_testing(), 0);
    assert_eq!(view.caption_for_testing(), "");
    let button = view.empty_add_button_for_testing();
    assert_eq!(button.title().to_string(), "Add Markdown task");
    assert_eq!(
        button.toolTip().map(|tip| tip.to_string()).as_deref(),
        Some("Insert a - [ ] checkbox into this document")
    );
    let button_role = unsafe { objc2_app_kit::NSAccessibilityButtonRole }.to_string();
    assert_eq!(button.accessibilityRole().map(|role| role.to_string()), Some(button_role));
    unsafe { button.performClick(None) };
    assert!(view.quick_add_editing_for_testing());
    view.commit_new_task_for_testing("Ship the polished panel");
    let additions = delegate.additions.borrow();
    assert_eq!(additions.len(), 1);
    assert_eq!(additions.first().map(|(text, _)| text.as_str()), Some("Ship the polished panel"));
    assert_eq!(additions.first().map(|(_, heading)| *heading), Some(None));
    drop(additions);
    view
}

/// "The populated Add task row responds to accessibility press"
fn task_panel_add_row_supports_every_press_path() {
    pooled(task_panel_add_row_supports_every_press_path_body);
}

fn task_panel_add_row_supports_every_press_path_body() -> Retained<TaskPanelView> {
    let view = TaskPanelView::new_current(mtm());
    view.set_tasks(vec![make_task("Open", false, 0)]);
    set_frame_size(&view, 300.0, 320.0);
    view.layoutSubtreeIfNeeded();

    assert!(view.perform_add_row_accessibility_press_for_testing());
    assert!(view.quick_add_editing_for_testing());
    view
}

/// "The task table claims command-N before the app menu"
fn task_table_claims_quick_add_key_equivalent() {
    let table = PanelTableView::new(mtm());
    table.setFrame(rect(0.0, 0.0, 300.0, 200.0));
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
    );
    assert!(event.is_some());
    let event = event.unwrap();
    let handled: bool = unsafe { objc2::msg_send![&*table, performKeyEquivalent: &*event] };
    assert!(handled);
    assert!(claimed.get());
}

fn undo_pill_reserves_the_last_rows() {
    let view = TaskPanelView::new_current(mtm());
    let base_inset = view.undo_bottom_inset_for_testing();
    view.present_undo_for_testing("Done");
    assert!(view.undo_bottom_inset_for_testing() > base_inset);
    view.dismiss_undo_for_testing();
    assert_eq!(view.undo_bottom_inset_for_testing(), base_inset);
}

fn descendants(parent: &NSView) -> Vec<Retained<NSView>> {
    let mut result = Vec::new();
    for child in parent.subviews().iter() {
        result.push(child.clone());
        result.extend(descendants(&child));
    }
    result
}

/// "Undo pill exposes a named action"
fn undo_pill_names_its_action() {
    let view = TaskPanelView::new_current(mtm());
    view.present_undo_for_testing("Done");

    let undo = descendants(&view)
        .into_iter()
        .filter_map(|view| view.downcast::<NSButton>().ok())
        .find(|button| button.title().to_string() == "Undo");
    let label = undo.and_then(|button| button.accessibilityLabel()).map(|label| label.to_string());
    assert_eq!(label.as_deref(), Some("Undo"));
}

// MARK: - Added: the section bar and the large plan

fn task_section_bar_summarises_segments() {
    let mtm = mtm();
    let bar = TaskSectionBarView::new(Rc::new(StyleSheet::current(mtm)), mtm);
    assert!(!bar.acceptsFirstResponder());
    let parsed = MarkdownParser::parse("# A\n\n- [x] one\n- [ ] two\n\n# B\n\n- [ ] three\n");
    let worklist = TaskWorklist::new(&parsed.tasks, &parsed.headings);
    bar.set_segments(worklist.segments.clone());
    assert_eq!(accessibility_value(&bar).as_deref(), Some("1 of 3 tasks done"));
    assert!(bar.acceptsFirstResponder());
    let actions = bar.accessibilityCustomActions().map(|actions| actions.to_vec()).unwrap_or_default();
    let names: Vec<String> = actions.iter().map(|action| action.name().to_string()).collect();
    assert_eq!(names, vec!["Open A".to_owned(), "Open B".to_owned()]);
    bar.set_segments(Vec::new());
    assert_eq!(accessibility_value(&bar).as_deref(), Some("No tasks"));
}

/// Builds and lays out the panel on `agent-5000.md` (1040 tasks, 520
/// sections) as the app does, and prints the time. The Swift comparison is
/// made through the oracles; this only guards against a pathological build.
fn task_panel_builds_agent_5000_within_budget() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/generated/agent/agent-5000.md");
    let Ok(text) = std::fs::read_to_string(&path) else {
        println!("    (corpus/generated missing; skipped)");
        return;
    };
    let parsed = MarkdownParser::parse(&text);
    let mtm = mtm();
    let started = Instant::now();
    let view = TaskPanelView::new_current(mtm);
    view.set_tasks(parsed.tasks.clone());
    view.set_headings(parsed.headings.clone());
    view.reload();
    set_frame_size(&view, 336.0, 480.0);
    view.layoutSubtreeIfNeeded();
    let fitted = view.fitted_content_height();
    let elapsed = started.elapsed();
    println!(
        "    agent-5000: {} rows, fitted height {fitted}, {:.1} ms",
        view.row_count_for_testing(),
        elapsed.as_secs_f64() * 1000.0
    );
    assert_eq!(view.visible_task_count_for_testing(), 520);
    assert_eq!(view.pile_row_count_for_testing(), 520);
    assert!(elapsed.as_secs_f64() < 5.0);
}

// MARK: - Added: the animated paths (Reduce Motion off)

#[derive(Default)]
struct ToggleSpy {
    toggles: RefCell<Vec<isize>>,
}

impl TaskPanelDelegate for ToggleSpy {
    fn task_panel_did_toggle_task_at(&self, _panel: &TaskPanelView, mark_offset: isize) {
        self.toggles.borrow_mut().push(mark_offset);
    }
    fn task_panel_did_select_task_at(&self, _panel: &TaskPanelView, _content_offset: isize) {}
    fn task_panel_did_request_new_task(&self, _panel: &TaskPanelView, _text: &str, _heading_index: Option<isize>) {}
    fn task_panel_did_move_task(&self, _panel: &TaskPanelView, _task_index: isize, _before: Option<isize>) {}
}

/// Paper Light against Aqua with Reduce Motion forced off.
fn motion_style_sheet() -> Rc<StyleSheet> {
    let appearance = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).expect("aqua");
    let theme = ThemeStore::shared().themes().into_iter().find(|theme| theme.name == "Paper Light").expect("theme");
    Rc::new(StyleSheet::new(theme, &appearance, Some(false)))
}

/// A borderless window off every screen, never ordered in: the panel only
/// needs `window != nil` to take its animated paths.
fn off_screen_window(content: &NSView, mtm: MainThreadMarker) -> Retained<NSWindow> {
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            rect(-30000.0, -30000.0, 336.0, 480.0),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            true,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setContentView(Some(content));
    window
}

fn task_table(panel: &NSView) -> Retained<PanelTableView> {
    for view in panel.subviews().iter() {
        if let Ok(scroll) = view.downcast::<NSScrollView>()
            && let Some(document) = scroll.documentView()
            && let Ok(table) = document.downcast::<PanelTableView>()
        {
            return table;
        }
    }
    panic!("no task table")
}

fn key(table: &PanelTableView, code: u16) {
    let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        NSEventType::KeyDown,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags::empty(),
        0.0,
        0,
        None,
        &NSString::from_str(""),
        &NSString::from_str(""),
        false,
        code,
    )
    .expect("key event");
    let _: () = unsafe { objc2::msg_send![table, keyDown: &*event] };
}

const NESTED: &str = "# A\n\n- [ ] one\n- [x] two\n- [x] three\n\n# B\n\n- [ ] four\n  - [ ] five\n- [x] six\n\n# C\n\n- [x] seven\n";

/// Folds and unfolds through the diffed `beginUpdates` path: after each
/// change the table's row count is the panel's, which NSTableView enforces
/// (an inconsistent diff raises inside `endUpdates`).
fn animated_row_rebuilds_keep_the_table_consistent() {
    let mtm = mtm();
    let parsed = MarkdownParser::parse(NESTED);
    let panel = TaskPanelView::new_current(mtm);
    panel.set_style_sheet(motion_style_sheet());
    panel.setFrame(rect(0.0, 0.0, 336.0, 480.0));
    let window = off_screen_window(&panel, mtm);
    panel.set_tasks(parsed.tasks.clone());
    panel.set_headings(parsed.headings.clone());
    window.layoutIfNeeded();
    let table = task_table(&panel);
    let check = |step: &str| {
        assert_eq!(table.numberOfRows(), panel.row_count_for_testing(), "{step}");
        main_thread::sleep_pumping(std::time::Duration::from_millis(30));
    };
    check("initial");
    // Rows: A, one, pile, B, four, five, pile, C, seven, add.
    assert_eq!(panel.row_count_for_testing(), 10);
    panel.set_completed_pile_expanded_for_testing(true, 0);
    check("expand A's pile");
    assert_eq!(panel.row_count_for_testing(), 12);
    table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(0), false);
    key(&table, 123);
    check("fold A");
    // Rows: A, B, four, five, pile, C, seven, add.
    assert_eq!(panel.row_count_for_testing(), 8);
    key(&table, 124);
    check("unfold A");
    table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(4), false);
    key(&table, 123);
    check("fold B");
    panel.set_completed_pile_expanded_for_testing(false, 0);
    check("collapse A's pile");
    panel.set_tasks(Vec::new());
    check("empty plan");
    assert_eq!(panel.row_count_for_testing(), 0);
    panel.set_tasks(parsed.tasks.clone());
    check("plan returns");
    // Let the row animations finish before the window and panel go.
    main_thread::sleep_pumping(std::time::Duration::from_millis(600));
    window.close();
}

/// A tick holds its row while the check draws: the rebuild that moves it
/// into the pile waits for the 0.10 s work item after the reparse lands.
fn completion_holds_the_row_until_the_deferred_rebuild() {
    let mtm = mtm();
    let text = "# A\n\n- [ ] one\n- [ ] two\n\n# B\n\n- [ ] three\n";
    let parsed = MarkdownParser::parse(text);
    let spy = Rc::new(ToggleSpy::default());
    let panel = TaskPanelView::new_current(mtm);
    panel.set_delegate(Some(Rc::downgrade(&spy) as Weak<dyn TaskPanelDelegate>));
    panel.set_style_sheet(motion_style_sheet());
    panel.setFrame(rect(0.0, 0.0, 336.0, 480.0));
    let window = off_screen_window(&panel, mtm);
    panel.set_tasks(parsed.tasks.clone());
    panel.set_headings(parsed.headings.clone());
    window.layoutIfNeeded();
    let table = task_table(&panel);
    // Rows: A, one, two, B, three, add. Space ticks Up Next ("one").
    assert_eq!(panel.row_count_for_testing(), 6);
    key(&table, 49);
    let mark = parsed.tasks[0].mark_range.location;
    assert_eq!(*spy.toggles.borrow(), vec![mark]);
    assert!(panel.undo_bottom_inset_for_testing() > 18.0, "the undo pill reserves its rows");
    // The host's reparse: "one" is now checked.
    let mut ticked = parsed.tasks.clone();
    ticked[0].is_checked = true;
    panel.set_tasks(ticked);
    // The row holds its place during the moment: no pile yet.
    assert_eq!(panel.pile_row_count_for_testing(), 0);
    assert_eq!(panel.visible_task_count_for_testing(), 3);
    // Then it slides into A's pile. Rows: A, two, pile, B, three, add.
    assert!(main_thread::pump_until(|| panel.pile_row_count_for_testing() == 1, std::time::Duration::from_secs(2)));
    assert_eq!(panel.visible_task_count_for_testing(), 2);
    assert_eq!(table.numberOfRows(), 6);
    panel.dismiss_undo_for_testing();
    // Let the row animations finish before the window and panel go.
    main_thread::sleep_pumping(std::time::Duration::from_millis(600));
    window.close();
}

/// Downright traps here (verified: the Swift oracle exits with SIGTRAP on
/// the same sequence): `preferredAddSection` reads the new `tasks` at a task
/// index taken from the old rows, so a selected row past the end of a
/// shorter plan is out of range. The port reproduces the trap as a panic.
fn selected_task_past_a_shorter_plan_traps_as_in_swift() {
    let parsed = MarkdownParser::parse("# A\n\n- [ ] one\n- [ ] two\n\n# B\n\n- [ ] three\n- [ ] four\n");
    let view = TaskPanelView::new_current(mtm());
    view.set_tasks(parsed.tasks.clone());
    view.set_headings(parsed.headings.clone());
    // Rows: A, one, two, B, three, four, add; select "four" (task 3).
    let table = task_table(&view);
    table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(5), false);
    let shorter = parsed.tasks[..2].to_vec();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| view.set_tasks(shorter)));
    assert!(outcome.is_err(), "Swift traps on this edit; the port must too");
}

fn main() {
    main_thread::run(&[
        ("task_checkbox_hit_target_uses_local_coordinates", task_checkbox_hit_target_uses_local_coordinates),
        ("narrow_task_panel_measures_wrapped_rows_at_live_width", narrow_task_panel_measures_wrapped_rows_at_live_width),
        ("task_panel_summarises_a_finished_plan", task_panel_summarises_a_finished_plan),
        ("task_panel_piles_completed_work_while_work_remains", task_panel_piles_completed_work_while_work_remains),
        (
            "task_panel_lists_open_work_first_without_losing_progress",
            task_panel_lists_open_work_first_without_losing_progress,
        ),
        ("task_panel_empty_state_points_at_quick_add", task_panel_empty_state_points_at_quick_add),
        ("task_panel_add_row_supports_every_press_path", task_panel_add_row_supports_every_press_path),
        ("task_table_claims_quick_add_key_equivalent", task_table_claims_quick_add_key_equivalent),
        ("undo_pill_reserves_the_last_rows", undo_pill_reserves_the_last_rows),
        ("undo_pill_names_its_action", undo_pill_names_its_action),
        ("task_section_bar_summarises_segments", task_section_bar_summarises_segments),
        ("task_panel_builds_agent_5000_within_budget", task_panel_builds_agent_5000_within_budget),
        ("animated_row_rebuilds_keep_the_table_consistent", animated_row_rebuilds_keep_the_table_consistent),
        ("completion_holds_the_row_until_the_deferred_rebuild", completion_holds_the_row_until_the_deferred_rebuild),
        ("selected_task_past_a_shorter_plan_traps_as_in_swift", selected_task_past_a_shorter_plan_traps_as_in_swift),
    ]);
}
