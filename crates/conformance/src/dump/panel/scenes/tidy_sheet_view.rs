//! `TidySheetView` scenes (`Scenes/TidySheetViewScene.swift`): the sheet as
//! `presentTidySheet` builds it, then `toggles`, `expand` and `buttons`.
//! See the Swift file for the state keys.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::msg_send;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSButton, NSTableView, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use serde_json::{Map, Value};
use upleft_app::panels::panel_chrome::PanelCheckbox;
use upleft_app::panels::tidy_sheet_view::{TidyProposal, TidySheetDelegate, TidySheetView};
use upleft_core::contracts::{TextEdit, TidyRule};
use upleft_core::parser::MarkdownParser;
use upleft_core::swift_text::ns::NSStringExt;
use upleft_core::tidy::TidyDocument;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_swift_text as swift_text;

use super::table_editor_view::{descendants, range_json};
use crate::dump::Failure;
use crate::dump::panel::{PanelScenario, PanelScene};

#[derive(Default)]
pub struct TidySheetViewScene {
    sheet: Option<Retained<TidySheetView>>,
    recorder: Rc<Recorder>,
    display_texts: Vec<String>,
}

#[derive(Default)]
struct Recorder {
    applied: RefCell<Vec<Vec<TextEdit>>>,
    cancelled: Cell<i64>,
}

impl TidySheetDelegate for Recorder {
    fn tidy_sheet_did_apply(&self, _sheet: &TidySheetView, edits: &[TextEdit]) {
        self.applied.borrow_mut().push(edits.to_vec());
    }

    fn tidy_sheet_did_cancel(&self, _sheet: &TidySheetView) {
        self.cancelled.set(self.cancelled.get() + 1);
    }
}

fn ints(scenario: &PanelScenario, key: &str) -> Vec<isize> {
    scenario
        .array(key)
        .iter()
        .filter_map(|value| value.as_i64().or_else(|| value.as_f64().map(|f| f as i64)))
        .map(|value| value as isize)
        .collect()
}

/// `NSApp.sendAction(button.action!, to: button.target, from: button)`.
fn send_action(button: &NSButton, mtm: MainThreadMarker) {
    let Some(action) = button.action() else { return };
    let target = button.target();
    unsafe { NSApplication::sharedApplication(mtm).sendAction_to_from(action, target.as_deref(), Some(button)) };
}

impl PanelScene for TidySheetViewScene {
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
        let parsed = MarkdownParser::parse(&text);
        let names = scenario.strings("rules");
        let mut edits = if names.is_empty() {
            TidyDocument::plan(&parsed)
        } else {
            let rules: Vec<TidyRule> = TidyRule::ALL_CASES
                .into_iter()
                .filter(|rule| names.iter().any(|name| name == rule.raw_value()))
                .collect();
            TidyDocument::plan_with(&parsed, &rules)
        };
        if let Some(limit) = scenario.int("limit") {
            edits.truncate(limit.max(0) as usize);
        }

        let sheet = if scenario.bool("current") {
            let sheet = TidySheetView::new_current(mtm);
            sheet.set_style_sheet(style_sheet);
            sheet
        } else {
            TidySheetView::new(style_sheet, mtm)
        };
        let utf16: Vec<u16> = text.encode_utf16().collect();
        sheet.set_proposals(
            edits
                .into_iter()
                .map(|edit| TidyProposal {
                    before: utf16.as_slice().substring(edit.range),
                    after: edit.replacement.clone(),
                    edit,
                })
                .collect(),
        );
        let recorder: Rc<dyn TidySheetDelegate> = self.recorder.clone();
        sheet.set_delegate(Some(Rc::downgrade(&recorder)));
        sheet.reload();

        let toggles = ints(scenario, "toggles");
        let expand = ints(scenario, "expand");
        let buttons = scenario.strings("buttons");
        if !toggles.is_empty() || !expand.is_empty() || !buttons.is_empty() {
            sheet.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(scenario.width, scenario.height)));
            sheet.layoutSubtreeIfNeeded();
        }
        let table = descendants::<NSTableView>(&sheet).into_iter().next();
        for row in toggles {
            let Some(table) = &table else { continue };
            if !(row >= 0 && row < table.numberOfRows()) {
                continue;
            }
            let Some(view) = table.viewAtColumn_row_makeIfNecessary(0, row, true) else { continue };
            let Some(checkbox) = descendants::<PanelCheckbox>(&view).into_iter().next() else { continue };
            let _: bool = unsafe { msg_send![&*checkbox, accessibilityPerformPress] };
        }
        for row in expand {
            let Some(table) = &table else { continue };
            if !(row >= 0 && row < table.numberOfRows()) {
                continue;
            }
            let Some(view) = table.viewAtColumn_row_makeIfNecessary(0, row, true) else { continue };
            let Some(button) = descendants::<NSButton>(&view).into_iter().next() else { continue };
            send_action(&button, mtm);
        }
        for title in buttons {
            let button = descendants::<NSButton>(&sheet).into_iter().find(|button| {
                let current = button.title().to_string();
                swift_text::str_eq(&current, &title) || swift_text::has_prefix(&current, &title)
            });
            if let Some(button) = button {
                send_action(&button, mtm);
            }
        }
        self.display_texts = scenario.strings("displayTexts");
        self.sheet = Some(sheet.clone());
        Ok(Retained::into_super(sheet))
    }

    fn model(&self) -> Value {
        let Some(sheet) = &self.sheet else { return Value::Null };
        let table = descendants::<NSTableView>(sheet).into_iter().next();
        let mut map = Map::new();
        map.insert("tableRows".into(), Value::from(table.map_or(-1, |table| table.numberOfRows() as i64)));
        map.insert(
            "proposals".into(),
            Value::Array(
                sheet
                    .proposals()
                    .iter()
                    .map(|proposal| {
                        let mut object = Map::new();
                        object.insert("summary".into(), Value::String(proposal.edit.summary.clone()));
                        object.insert(
                            "rule".into(),
                            proposal.edit.rule.map_or(Value::Null, |rule| Value::String(rule.raw_value().into())),
                        );
                        object.insert("range".into(), range_json(proposal.edit.range));
                        object.insert("before".into(), Value::String(TidySheetView::display_text(&proposal.before)));
                        object.insert("after".into(), Value::String(TidySheetView::display_text(&proposal.after)));
                        Value::Object(object)
                    })
                    .collect(),
            ),
        );
        map.insert(
            "applied".into(),
            Value::Array(
                self.recorder
                    .applied
                    .borrow()
                    .iter()
                    .map(|edits| Value::Array(edits.iter().map(|edit| Value::String(edit.summary.clone())).collect()))
                    .collect(),
            ),
        );
        map.insert("cancelled".into(), Value::from(self.recorder.cancelled.get()));
        map.insert(
            "displayTexts".into(),
            Value::Array(self.display_texts.iter().map(|text| Value::String(TidySheetView::display_text(text))).collect()),
        );
        Value::Object(map)
    }
}
