//! View-level tests for TaskPanelView, TaskSectionBarView
//! (ported from DownrightAppTests). Runs on the main thread; any window is
//! off-screen and never activated.
//!
//! Ported from `Tests/DownrightAppTests/PanelAccessibilityTests.swift`: every
//! test that exercises the task panel (including the two that exercise its
//! `PanelChrome` building blocks as the panel configures them: the 17-point
//! task checkbox's hit target and the task table's ⌘N key equivalent).
//!
//! Added here (no Swift original): `task_section_bar_summarises_segments`
//! and `task_panel_builds_agent_5000_within_budget`.
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
use objc2_app_kit::{NSAccessibility, NSButton, NSEvent, NSEventModifierFlags, NSEventType, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use upleft_app::panels::panel_chrome::{PanelCheckbox, PanelTableView};
use upleft_app::panels::task_panel_view::{TaskPanelDelegate, TaskPanelView};
use upleft_app::panels::task_section_bar_view::TaskSectionBarView;
use upleft_core::NSRange;
use upleft_core::model::TaskItem;
use upleft_core::parser::MarkdownParser;
use upleft_core::task_worklist::TaskWorklist;
use upleft_render::theme::style_sheet::StyleSheet;

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

fn task_panel_empty_state_points_at_quick_add() {
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
}

/// "The populated Add task row responds to accessibility press"
fn task_panel_add_row_supports_every_press_path() {
    let view = TaskPanelView::new_current(mtm());
    view.set_tasks(vec![make_task("Open", false, 0)]);
    set_frame_size(&view, 300.0, 320.0);
    view.layoutSubtreeIfNeeded();

    assert!(view.perform_add_row_accessibility_press_for_testing());
    assert!(view.quick_add_editing_for_testing());
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
    ]);
}
