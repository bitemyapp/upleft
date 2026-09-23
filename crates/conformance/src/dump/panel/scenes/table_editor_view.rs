//! `TableEditorView` scenes (`Scenes/TableEditorViewScene.swift`): the
//! editor over `state.text` or the scenario's document, then `select`,
//! `operations`, `actions`, `alignment`, `update` and `reload` in that order.
//! See the Swift file for the state keys.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{ClassType, Message, msg_send};
use objc2_app_kit::{NSApplication, NSPopUpButton, NSTableView, NSView};
use serde_json::{Map, Value};
use upleft_app::panels::appkit_support::downcast;
use upleft_app::panels::table_editor_view::{TableEditorDelegate, TableEditorView};
use upleft_core::NSRange;
use upleft_core::editing::table_editing::{TableEditOperation, TableEditProposal};
use upleft_core::model::TableAlignment;
use upleft_core::parser::MarkdownParser;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene};

#[derive(Default)]
pub struct TableEditorViewScene {
    editor: Option<Retained<TableEditorView>>,
    recorder: Rc<Recorder>,
}

#[derive(Default)]
struct Recorder {
    text: RefCell<String>,
    host: Cell<bool>,
    proposals: RefCell<Vec<TableEditProposal>>,
    source_requests: RefCell<Vec<NSRange>>,
    finished: Cell<i64>,
    cancelled: Cell<i64>,
}

impl TableEditorDelegate for Recorder {
    fn table_editor_did_apply(&self, editor: &TableEditorView, proposal: &TableEditProposal) {
        self.proposals.borrow_mut().push(proposal.clone());
        if !self.host.get() {
            return;
        }
        let Some(next) = proposal.applying(&self.text.borrow()) else { return };
        *self.text.borrow_mut() = next.clone();
        editor.update(MarkdownParser::parse(&next));
    }

    fn table_editor_did_request_source(&self, _editor: &TableEditorView, range: NSRange) {
        self.source_requests.borrow_mut().push(range);
    }

    fn table_editor_did_finish(&self, _editor: &TableEditorView) {
        self.finished.set(self.finished.get() + 1);
    }

    fn table_editor_did_cancel(&self, _editor: &TableEditorView) {
        self.cancelled.set(self.cancelled.get() + 1);
    }
}

/// `TableEditorViewScene.operation(_:)`.
pub fn operation(object: &Map<String, Value>) -> Option<TableEditOperation> {
    let int = |key: &str| -> isize {
        object.get(key).and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|f| f as i64))).unwrap_or(0) as isize
    };
    let string = |key: &str| -> String { object.get(key).and_then(Value::as_str).unwrap_or("").to_owned() };
    let strings = |key: &str| -> Vec<String> {
        object
            .get(key)
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(|value| value.as_str().map(str::to_owned)).collect())
            .unwrap_or_default()
    };
    Some(match object.get("op").and_then(Value::as_str)? {
        "setCell" => TableEditOperation::SetCell { row: int("row"), column: int("column"), text: string("text") },
        "setAlignment" => TableEditOperation::SetAlignment {
            column: int("column"),
            alignment: match string("alignment").as_str() {
                "left" => TableAlignment::Left,
                "center" => TableAlignment::Center,
                "right" => TableAlignment::Right,
                _ => TableAlignment::None,
            },
        },
        "insertRow" => TableEditOperation::InsertRow { index: int("index"), cells: strings("cells") },
        "deleteRow" => TableEditOperation::DeleteRow { index: int("index") },
        "moveRow" => TableEditOperation::MoveRow { from: int("from"), to: int("to") },
        "insertColumn" => {
            TableEditOperation::InsertColumn { index: int("index"), header: string("header"), cells: strings("cells") }
        }
        "deleteColumn" => TableEditOperation::DeleteColumn { index: int("index") },
        "moveColumn" => TableEditOperation::MoveColumn { from: int("from"), to: int("to") },
        _ => return None,
    })
}

/// `TableEditorViewScene.descendants(of:as:)`: depth first, pre-order.
pub fn descendants<T: ClassType + Message>(view: &NSView) -> Vec<Retained<T>> {
    let mut found = Vec::new();
    for child in view.subviews().iter() {
        if let Some(matched) = downcast::<T>(&child) {
            found.push(matched);
        }
        found.extend(descendants::<T>(&child));
    }
    found
}

pub fn range_json(range: NSRange) -> Value {
    Value::Array(vec![Value::from(range.location as i64), Value::from(range.length as i64)])
}

impl PanelScene for TableEditorViewScene {
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
        let document = MarkdownParser::parse(&text);
        let editor = if scenario.bool("current") {
            let editor = TableEditorView::new_default(document, mtm);
            editor.set_style_sheet(style_sheet);
            editor
        } else {
            TableEditorView::new(document, scenario.int_or("tableIndex", 0) as isize, style_sheet, mtm)
        };
        let recorder: Rc<dyn TableEditorDelegate> = self.recorder.clone();
        editor.set_delegate(Some(Rc::downgrade(&recorder)));
        let select: Vec<isize> = scenario
            .array("select")
            .iter()
            .filter_map(|value| value.as_i64().or_else(|| value.as_f64().map(|f| f as i64)))
            .map(|value| value as isize)
            .collect();
        if select.len() == 2 {
            editor.select(select[0], select[1]);
        }
        for object in scenario.array("operations") {
            let Some(object) = object.as_object() else { continue };
            let Some(operation) = operation(object) else { continue };
            editor.apply(&operation);
        }
        for name in scenario.strings("actions") {
            let selector = Sel::register(&std::ffi::CString::new(name).expect("selector name"));
            let _: Option<&AnyObject> =
                unsafe { msg_send![&*editor, performSelector: selector, withObject: None::<&AnyObject>] };
        }
        if let Some(index) = scenario.int("alignment")
            && let Some(popup) = descendants::<NSPopUpButton>(&editor).first()
            && let Some(action) = popup.action()
        {
            popup.selectItemAtIndex(index as isize);
            let target = popup.target();
            unsafe { NSApplication::sharedApplication(mtm).sendAction_to_from(action, target.as_deref(), Some(popup)) };
        }
        if let Some(update) = scenario.string("update") {
            *self.recorder.text.borrow_mut() = update.clone();
            editor.update(MarkdownParser::parse(&update));
        }
        if scenario.bool("reload") {
            editor.reload();
        }
        self.editor = Some(editor.clone());
        Ok(Retained::into_super(editor))
    }

    fn model(&self) -> Value {
        let Some(editor) = &self.editor else { return Value::Null };
        let table = descendants::<NSTableView>(editor).into_iter().next();
        let columns: Vec<Value> = table
            .as_ref()
            .map(|table| table.tableColumns().iter().collect::<Vec<_>>())
            .unwrap_or_default()
            .iter()
            .map(|column| {
                let mut map = Map::new();
                map.insert("identifier".into(), Value::String(column.identifier().to_string()));
                map.insert("title".into(), Value::String(column.title().to_string()));
                map.insert("width".into(), double(column.width()));
                map.insert("minWidth".into(), double(column.minWidth()));
                Value::Object(map)
            })
            .collect();
        let recorder = &self.recorder;
        let mut map = Map::new();
        map.insert("rowCount".into(), Value::from(editor.row_count_for_testing() as i64));
        map.insert("columnCount".into(), Value::from(editor.column_count_for_testing() as i64));
        map.insert("sourceRange".into(), range_json(editor.source_range_for_testing()));
        map.insert("appliedEditCount".into(), Value::from(editor.applied_edit_count() as i64));
        map.insert("tableIndex".into(), Value::from(editor.table_index() as i64));
        let preferred_width: f64 = unsafe { msg_send![&**editor, preferredWidth] };
        map.insert("preferredWidth".into(), double(preferred_width));
        map.insert("selectedRow".into(), Value::from(table.as_ref().map_or(-2, |table| table.selectedRow() as i64)));
        map.insert("columns".into(), Value::Array(columns));
        map.insert(
            "proposals".into(),
            Value::Array(
                recorder
                    .proposals
                    .borrow()
                    .iter()
                    .map(|proposal| {
                        let mut object = Map::new();
                        object.insert("summary".into(), Value::String(proposal.summary.clone()));
                        object.insert("range".into(), range_json(proposal.range));
                        object.insert("replacement".into(), Value::String(proposal.replacement.clone()));
                        object.insert("expected".into(), Value::String(proposal.expected.clone()));
                        Value::Object(object)
                    })
                    .collect(),
            ),
        );
        map.insert(
            "sourceRequests".into(),
            Value::Array(recorder.source_requests.borrow().iter().map(|range| range_json(*range)).collect()),
        );
        map.insert("finished".into(), Value::from(recorder.finished.get()));
        map.insert("cancelled".into(), Value::from(recorder.cancelled.get()));
        map.insert("text".into(), Value::String(recorder.text.borrow().clone()));
        Value::Object(map)
    }
}
