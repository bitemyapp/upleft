//! `FrontMatterEditorView` scenes (`Scenes/FrontMatterEditorViewScene.swift`):
//! the panel as `showFrontMatterEditor` builds it, then `scrollOriginY`,
//! `edits`, `remove`, `add`, `sourceMode`, `prepare` and `focusSelector`; and
//! `focus` after the window is shown. See the Swift file for the state keys.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSApplication, NSButton, NSControl, NSControlTextDidChangeNotification, NSControlTextDidEndEditingNotification,
    NSPopUpButton, NSTextField, NSView, NSWindow,
};
use objc2_foundation::NSNotificationCenter;
use serde_json::{Map, Value};
use upleft_app::panels::appkit_support::{accessibility_label, ns_string};
use upleft_app::panels::front_matter_editor_view::{FrontMatterEditorDelegate, FrontMatterEditorView};
use upleft_core::editing::front_matter_editing::{FrontMatterEditOperation, FrontMatterEditing, FrontMatterValue};
use upleft_core::parser::MarkdownParser;
use upleft_render::theme::style_sheet::StyleSheet;

use super::table_editor_view::descendants;
use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene};

#[derive(Default)]
pub struct FrontMatterEditorViewScene {
    editor: Option<Retained<FrontMatterEditorView>>,
    recorder: Rc<Recorder>,
}

#[derive(Default)]
struct Recorder {
    text: RefCell<String>,
    host: Cell<bool>,
    requests: RefCell<Vec<FrontMatterEditOperation>>,
    source_mode_requests: Cell<i64>,
}

impl FrontMatterEditorDelegate for Recorder {
    fn front_matter_editor_did_request(&self, editor: &FrontMatterEditorView, operation: FrontMatterEditOperation) {
        self.requests.borrow_mut().push(operation.clone());
        if !self.host.get() {
            return;
        }
        let text = self.text.borrow().clone();
        let parsed = MarkdownParser::parse(&text);
        let result = FrontMatterEditing::propose(&parsed, &operation);
        let Some(next) = result.proposal.and_then(|proposal| proposal.applying(&text)) else {
            editor.set_document(parsed);
            return;
        };
        *self.text.borrow_mut() = next.clone();
        editor.set_document(MarkdownParser::parse(&next));
    }

    fn front_matter_editor_wants_source_mode(&self, _editor: &FrontMatterEditorView) {
        self.source_mode_requests.set(self.source_mode_requests.get() + 1);
    }
}

fn value_json(value: &FrontMatterValue) -> Value {
    let (kind, value) = match value {
        FrontMatterValue::Text(text) => ("text", Value::String(text.clone())),
        FrontMatterValue::Boolean(flag) => ("boolean", Value::Bool(*flag)),
        FrontMatterValue::Number(number) => ("number", double(*number)),
        FrontMatterValue::List(items) => ("list", Value::Array(items.iter().cloned().map(Value::String).collect())),
    };
    let mut map = Map::new();
    map.insert("kind".into(), Value::String(kind.into()));
    map.insert("value".into(), value);
    Value::Object(map)
}

fn operation_json(operation: &FrontMatterEditOperation) -> Value {
    let mut map = Map::new();
    match operation {
        FrontMatterEditOperation::Set { key, value } => {
            map.insert("op".into(), Value::String("set".into()));
            map.insert("key".into(), Value::String(key.clone()));
            map.insert("value".into(), value_json(value));
        }
        FrontMatterEditOperation::Add { key, value } => {
            map.insert("op".into(), Value::String("add".into()));
            map.insert("key".into(), Value::String(key.clone()));
            map.insert("value".into(), value_json(value));
        }
        FrontMatterEditOperation::Remove { key } => {
            map.insert("op".into(), Value::String("remove".into()));
            map.insert("key".into(), Value::String(key.clone()));
        }
    }
    Value::Object(map)
}

/// `NSApp.sendAction(control.action!, to: control.target, from: control)`.
fn send(control: &NSControl, mtm: MainThreadMarker) {
    let Some(action) = control.action() else { return };
    let target = control.target();
    unsafe { NSApplication::sharedApplication(mtm).sendAction_to_from(action, target.as_deref(), Some(control)) };
}

/// The row for `key`, by its accessibility label.
fn row(key: &str, editor: &NSView) -> Option<Retained<NSView>> {
    let wanted = format!("Front matter field {key}");
    descendants::<NSView>(editor).into_iter().find(|view| accessibility_label(&**view).as_deref() == Some(wanted.as_str()))
}

fn int_value(value: Option<&Value>) -> Option<isize> {
    value.and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|f| f as i64))).map(|value| value as isize)
}

impl PanelScene for FrontMatterEditorViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let text = match scenario.string("text") {
            Some(text) => text,
            None => scenario.document_text().map_err(Failure::Error)?,
        };
        *self.recorder.text.borrow_mut() = text.clone();
        self.recorder.host.set(scenario.bool("host"));
        let editor = if scenario.bool("current") {
            let editor = FrontMatterEditorView::new_current(mtm);
            editor.set_style_sheet(style_sheet);
            editor
        } else {
            FrontMatterEditorView::new(style_sheet, mtm)
        };
        let recorder: Rc<dyn FrontMatterEditorDelegate> = self.recorder.clone();
        editor.set_delegate(Some(Rc::downgrade(&recorder)));
        editor.set_document(MarkdownParser::parse(&text));
        if scenario.bool("redocument") {
            editor.set_document(MarkdownParser::parse(&text));
        }

        if let Some(y) = scenario.double("scrollOriginY") {
            editor.set_field_scroll_origin_y_for_testing(y);
        }
        for edit in scenario.array("edits") {
            let Some(edit) = edit.as_object() else { continue };
            let Some(key) = edit.get("key").and_then(Value::as_str) else { continue };
            let Some(row) = row(key, &editor) else { continue };
            let wanted = format!("Value for {key}");
            let field = descendants::<NSTextField>(&row)
                .into_iter()
                .find(|field| accessibility_label(&**field).as_deref() == Some(wanted.as_str()));
            let popup = descendants::<NSPopUpButton>(&row).into_iter().next();
            if let Some(value) = edit.get("value").and_then(Value::as_str)
                && let Some(field) = &field
            {
                field.setStringValue(&ns_string(value));
            }
            if let Some(kind) = int_value(edit.get("kind"))
                && let Some(popup) = &popup
            {
                popup.selectItemAtIndex(kind);
            }
            if edit.get("send").and_then(Value::as_str) == Some("kind") {
                if let Some(popup) = &popup {
                    send(popup, mtm);
                }
            } else if let Some(field) = &field {
                send(field, mtm);
            }
        }
        let center = NSNotificationCenter::defaultCenter();
        for change in scenario.array("changes") {
            let Some(change) = change.as_object() else { continue };
            let Some(key) = change.get("key").and_then(Value::as_str) else { continue };
            let Some(row) = row(key, &editor) else { continue };
            let wanted = format!("Value for {key}");
            let Some(field) = descendants::<NSTextField>(&row)
                .into_iter()
                .find(|field| accessibility_label(&**field).as_deref() == Some(wanted.as_str()))
            else {
                continue;
            };
            if let Some(value) = change.get("value").and_then(Value::as_str) {
                field.setStringValue(&ns_string(value));
            }
            unsafe { center.postNotificationName_object(NSControlTextDidChangeNotification, Some(&field)) };
            if change.get("end").and_then(Value::as_bool).unwrap_or(false) {
                unsafe { center.postNotificationName_object(NSControlTextDidEndEditingNotification, Some(&field)) };
            }
        }
        for key in scenario.strings("remove") {
            let Some(row) = row(&key, &editor) else { continue };
            let button = descendants::<NSButton>(&row)
                .into_iter()
                .find(|button| accessibility_label(&**button).as_deref() == Some("Remove field"));
            if let Some(button) = button {
                send(&button, mtm);
            }
        }
        let add = scenario.object("add");
        if !add.is_empty() {
            let fields = descendants::<NSTextField>(&editor);
            let placeholder = |field: &NSTextField| field.placeholderString().map(|text| text.to_string());
            if let Some(field) = fields.iter().find(|field| placeholder(field).as_deref() == Some("Field name")) {
                field.setStringValue(&ns_string(add.get("key").and_then(Value::as_str).unwrap_or("")));
            }
            if let Some(field) = fields.iter().find(|field| placeholder(field).as_deref() == Some("Value")) {
                field.setStringValue(&ns_string(add.get("value").and_then(Value::as_str).unwrap_or("")));
            }
            if let Some(kind) = int_value(add.get("kind"))
                && let Some(popup) = descendants::<NSPopUpButton>(&editor)
                    .into_iter()
                    .find(|popup| accessibility_label(&**popup).as_deref() == Some("New field type"))
            {
                popup.selectItemAtIndex(kind);
            }
            if let Some(button) =
                descendants::<NSButton>(&editor).into_iter().find(|button| button.title().to_string() == "Add field")
            {
                send(&button, mtm);
            }
        }
        if scenario.bool("sourceMode")
            && let Some(button) =
                descendants::<NSButton>(&editor).into_iter().find(|button| button.title().to_string() == "Open Source Focus")
        {
            send(&button, mtm);
        }
        if let Some(prepare) = scenario.string("prepare") {
            editor.prepare_for_presentation(if prepare.is_empty() { None } else { Some(prepare.as_str()) });
        }
        if scenario.bool("focusSelector") {
            let _: () = unsafe { msg_send![&*editor, focusField] };
        }
        self.editor = Some(editor.clone());
        Ok(Retained::into_super(editor))
    }

    fn after_show(&mut self, _window: &NSWindow, scenario: &PanelScenario) {
        let Some(editor) = &self.editor else { return };
        if let Some(y) = scenario.double("scrollAfterShow") {
            editor.set_field_scroll_origin_y_for_testing(y);
        }
        if let Some(focus) = scenario.string("focus") {
            editor.prepare_for_presentation(if focus.is_empty() { None } else { Some(focus.as_str()) });
        }
    }

    fn model(&self) -> Value {
        let Some(editor) = &self.editor else { return Value::Null };
        let recorder = &self.recorder;
        let mut map = Map::new();
        map.insert("renderedFieldCount".into(), Value::from(editor.rendered_field_count() as i64));
        map.insert("showsSourceModePrompt".into(), Value::Bool(editor.shows_source_mode_prompt()));
        map.insert("fieldScrollOriginY".into(), double(editor.field_scroll_origin_y_for_testing()));
        map.insert(
            "focusedFieldKey".into(),
            editor.focused_field_key_for_testing().map_or(Value::Null, Value::String),
        );
        let preferred_width: f64 = unsafe { msg_send![&**editor, preferredWidth] };
        map.insert("preferredWidth".into(), double(preferred_width));
        map.insert("requests".into(), Value::Array(recorder.requests.borrow().iter().map(operation_json).collect()));
        map.insert("sourceModeRequests".into(), Value::from(recorder.source_mode_requests.get()));
        map.insert("text".into(), Value::String(recorder.text.borrow().clone()));
        Value::Object(map)
    }
}
