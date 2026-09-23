//! `TaskPanelView` scenes (`Scenes/TaskPanelViewScene.swift`): the panel as
//! the document window builds it, with the scenario document's parsed tasks,
//! a recording delegate, then `state.actions` in order. See the Swift file for
//! the action list, the style-sheet reassignment in `after_show`, and what
//! Reduce Motion does here.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{NSAccessibility, NSEvent, NSEventModifierFlags, NSEventType, NSScrollView, NSView, NSWindow};
use objc2_foundation::{NSIndexSet, NSPoint, NSString};
use serde_json::{Map, Value};
use upleft_app::panels::panel_chrome::PanelTableView;
use upleft_app::panels::task_panel_view::{TaskPanelDelegate, TaskPanelView};
use upleft_core::parser::MarkdownParser;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
struct Recorder {
    calls: RefCell<Vec<String>>,
}

impl TaskPanelDelegate for Recorder {
    fn task_panel_did_toggle_task_at(&self, _panel: &TaskPanelView, mark_offset: isize) {
        self.calls.borrow_mut().push(format!("toggle {mark_offset}"));
    }

    fn task_panel_did_select_task_at(&self, _panel: &TaskPanelView, content_offset: isize) {
        self.calls.borrow_mut().push(format!("select {content_offset}"));
    }

    fn task_panel_did_request_new_task(&self, _panel: &TaskPanelView, text: &str, heading_index: Option<isize>) {
        let heading = heading_index.map_or_else(|| "nil".to_owned(), |index| index.to_string());
        self.calls.borrow_mut().push(format!("new {text} {heading}"));
    }

    fn task_panel_did_move_task(&self, _panel: &TaskPanelView, task_index: isize, before: Option<isize>) {
        let before = before.map_or_else(|| "nil".to_owned(), |index| index.to_string());
        self.calls.borrow_mut().push(format!("move {task_index} {before}"));
    }
}

#[derive(Default)]
pub struct TaskPanelViewScene {
    panel: Option<Retained<TaskPanelView>>,
    style_sheet: Option<Rc<StyleSheet>>,
    recorder: Rc<Recorder>,
    content_size_changes: Rc<Cell<i64>>,
}

/// `TaskPanelViewScene.keyEvent(_:command:characters:)`.
pub fn key_event(code: u16, command: bool, characters: &str) -> Retained<NSEvent> {
    let characters = NSString::from_str(characters);
    NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        NSEventType::KeyDown,
        NSPoint::new(0.0, 0.0),
        if command { NSEventModifierFlags::Command } else { NSEventModifierFlags::empty() },
        0.0,
        0,
        None,
        &characters,
        &characters,
        false,
        code,
    )
    .expect("key event")
}

/// `TaskPanelViewScene.table(in:)`: the document view of the panel's scroll
/// view.
fn table_in(panel: &NSView) -> Option<Retained<PanelTableView>> {
    for view in panel.subviews().iter() {
        if let Ok(scroll) = view.downcast::<NSScrollView>()
            && let Some(document) = scroll.documentView()
            && let Ok(table) = document.downcast::<PanelTableView>()
        {
            return Some(table);
        }
    }
    None
}

impl PanelScene for TaskPanelViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let document = MarkdownParser::parse(&scenario.document_text().map_err(Failure::Error)?);
        let panel = TaskPanelView::new_current(mtm);
        let recorder: Rc<dyn TaskPanelDelegate> = self.recorder.clone();
        panel.set_delegate(Some(Rc::downgrade(&recorder) as Weak<dyn TaskPanelDelegate>));
        panel.set_style_sheet(style_sheet.clone());
        let changes = self.content_size_changes.clone();
        panel.set_on_content_size_change(Some(Rc::new(move || changes.set(changes.get() + 1))));
        panel.set_tasks(document.tasks.clone());
        panel.set_headings(document.headings.clone());
        panel.reload();
        let table = table_in(&panel);
        for action in scenario.array("actions") {
            let Some(action) = action.as_array() else { continue };
            let Some(name) = action.first().and_then(Value::as_str) else { continue };
            let argument = action.get(1);
            let number = argument
                .and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|f| f as i64)))
                .unwrap_or(0) as isize;
            let text = argument.and_then(Value::as_str).unwrap_or("");
            match name {
                "expandPile" => panel.set_completed_pile_expanded_for_testing(true, number),
                "collapsePile" => panel.set_completed_pile_expanded_for_testing(false, number),
                "select" => {
                    if let Some(table) = &table {
                        table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(number as usize), false);
                    }
                }
                "deselect" => {
                    if let Some(table) = &table {
                        unsafe { table.deselectAll(None) };
                    }
                }
                "key" => {
                    if let Some(table) = &table {
                        let event = key_event(number as u16, false, "");
                        let _: () = unsafe { msg_send![&**table, keyDown: &*event] };
                    }
                }
                "commandN" => {
                    let event = key_event(45, true, "n");
                    let _: bool = unsafe { msg_send![&*panel, performKeyEquivalent: &*event] };
                }
                "undo" => panel.present_undo_for_testing(text),
                "dismissUndo" => panel.dismiss_undo_for_testing(),
                "beginNewTask" => panel.begin_new_task_for_command(),
                "commit" => panel.commit_new_task_for_testing(text),
                "cancel" => {
                    let _: () = unsafe { msg_send![&*panel, cancelOperation: None::<&AnyObject>] };
                }
                "scrollRow" => {
                    if let Some(table) = &table {
                        table.scrollRowToVisible(number);
                    }
                }
                "reload" => panel.reload(),
                _ => {}
            }
        }
        self.panel = Some(panel.clone());
        self.style_sheet = Some(style_sheet);
        Ok(Retained::into_super(panel))
    }

    fn after_show(&mut self, _window: &NSWindow, _scenario: &PanelScenario) {
        if let (Some(panel), Some(style_sheet)) = (&self.panel, &self.style_sheet) {
            panel.set_style_sheet(style_sheet.clone());
        }
    }

    fn model(&self) -> Value {
        let Some(panel) = &self.panel else { return Value::Null };
        let (done, total) = panel.progress();
        let accessibility_value: Option<Retained<AnyObject>> = panel.accessibilityValue();
        let accessibility_value =
            accessibility_value.and_then(|value| value.downcast::<NSString>().ok()).map(|value| value.to_string());
        let mut map = Map::new();
        map.insert("theme".into(), Value::String(panel.style_sheet().theme.name.clone()));
        map.insert("statusLine".into(), Value::String(panel.status_line_for_testing()));
        map.insert("caption".into(), Value::String(panel.caption_for_testing()));
        map.insert("progress".into(), Value::Array(vec![Value::from(done as i64), Value::from(total as i64)]));
        map.insert("preferredWidth".into(), double(panel.preferred_width()));
        map.insert("rowCount".into(), Value::from(panel.row_count_for_testing() as i64));
        map.insert("visibleTaskCount".into(), Value::from(panel.visible_task_count_for_testing() as i64));
        map.insert("pileRowCount".into(), Value::from(panel.pile_row_count_for_testing() as i64));
        map.insert("quickAddEditing".into(), Value::Bool(panel.quick_add_editing_for_testing()));
        map.insert("measuredListHeight".into(), double(panel.measured_list_height_for_testing()));
        map.insert("contentDocumentHeight".into(), double(panel.content_document_height_for_testing()));
        map.insert("contentViewportHeight".into(), double(panel.content_viewport_height_for_testing()));
        map.insert("undoBottomInset".into(), double(panel.undo_bottom_inset_for_testing()));
        map.insert("undoRequiredBottomInset".into(), double(panel.undo_required_bottom_inset_for_testing()));
        map.insert("undoPillFrame".into(), tree::rect(panel.undo_pill_frame_for_testing()));
        map.insert("lastRowFrame".into(), panel.last_row_frame_for_testing().map_or(Value::Null, tree::rect));
        map.insert("emptyAddButtonTitle".into(), Value::String(panel.empty_add_button_for_testing().title().to_string()));
        map.insert("accessibilityValue".into(), accessibility_value.map_or(Value::Null, Value::String));
        map.insert(
            "delegateCalls".into(),
            Value::Array(self.recorder.calls.borrow().iter().cloned().map(Value::String).collect()),
        );
        map.insert("contentSizeChanges".into(), Value::from(self.content_size_changes.get()));
        map.insert("fittedContentHeight".into(), double(panel.fitted_content_height()));
        Value::Object(map)
    }
}
