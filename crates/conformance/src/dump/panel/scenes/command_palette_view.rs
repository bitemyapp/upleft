//! `CommandPaletteView` scenes (`Scenes/CommandPaletteViewScene.swift`).

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSApplication, NSButton, NSControlTextDidChangeNotification, NSEvent, NSEventModifierFlags,
    NSEventType, NSScrollView, NSSearchField, NSStackView, NSTableView, NSView, NSWindow,
};
use objc2_foundation::{NSIndexSet, NSNotification, NSPoint, NSString};
use serde_json::{Map, Value};
use upleft_app::panels::appkit_support::downcast;
use upleft_app::panels::command_palette_view::{CommandPaletteView, CommandPaletteViewDelegate};
use upleft_app::panels::panel_chrome::PanelEmptyStateView;
use upleft_app::support::command_palette_model::{CommandPaletteModel, CommandPaletteRecentStore};
use upleft_app::support::commands::Command;
use upleft_app::support::keybindings::KeybindingDefaults;
use upleft_app::support::quick_open_providers::{CurrentDocumentQuickOpenProvider, QuickOpenProvider, QuickOpenResult};
use upleft_core::parser::MarkdownParser;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene};

/// `MemoryRecentStore`.
#[derive(Default)]
struct MemoryRecentStore {
    values: RefCell<Vec<Command>>,
}

impl CommandPaletteRecentStore for MemoryRecentStore {
    fn recent_commands(&self) -> Vec<Command> {
        self.values.borrow().clone()
    }

    fn record(&self, command: Command) {
        let mut values = self.values.borrow_mut();
        values.retain(|value| *value != command);
        values.insert(0, command);
    }
}

/// The scene's `CommandPaletteViewDelegate` half.
#[derive(Default)]
struct Events {
    chosen: RefCell<Vec<String>>,
    cancels: RefCell<i64>,
}

impl CommandPaletteViewDelegate for Events {
    fn command_palette_did_choose(&self, _palette: &CommandPaletteView, result: &QuickOpenResult) {
        self.chosen.borrow_mut().push(result.id.clone());
    }

    fn command_palette_did_cancel(&self, _palette: &CommandPaletteView) {
        *self.cancels.borrow_mut() += 1;
    }
}

#[derive(Default)]
pub struct CommandPaletteViewScene {
    palette: Option<Retained<CommandPaletteView>>,
    store: Rc<MemoryRecentStore>,
    events: Rc<Events>,
    key_codes: Vec<u16>,
}

impl CommandPaletteViewScene {
    fn search_field(&self) -> Option<Retained<NSSearchField>> {
        let palette = self.palette.as_ref()?;
        palette.subviews().iter().find_map(|view| downcast::<NSSearchField>(&view))
    }

    fn table_view(&self) -> Option<Retained<NSTableView>> {
        let palette = self.palette.as_ref()?;
        palette.subviews().iter().find_map(|view| {
            downcast::<NSScrollView>(&view)
                .and_then(|scroll| scroll.documentView())
                .and_then(|document| downcast::<NSTableView>(&document))
        })
    }

    fn empty_state(&self) -> Option<Retained<PanelEmptyStateView>> {
        let palette = self.palette.as_ref()?;
        palette.subviews().iter().find_map(|view| downcast::<PanelEmptyStateView>(&view))
    }
}

impl PanelScene for CommandPaletteViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        *self.store.values.borrow_mut() =
            scenario.strings("recents").iter().filter_map(|raw| Command::from_raw_value(raw)).collect();
        let mut providers: Vec<Rc<dyn QuickOpenProvider>> = Vec::new();
        if scenario.document_path.is_some() {
            let text = scenario.document_text().map_err(Failure::Error)?;
            providers.push(Rc::new(CurrentDocumentQuickOpenProvider::new(MarkdownParser::parse(&text))));
        }
        let palette = if scenario.bool("current") {
            let palette = CommandPaletteView::new_current(mtm);
            palette.set_style_sheet(style_sheet.clone());
            palette
        } else {
            let commands: Vec<Command> =
                scenario.strings("commands").iter().filter_map(|raw| Command::from_raw_value(raw)).collect();
            let model = (!commands.is_empty()).then(|| {
                CommandPaletteModel::with_commands(
                    &commands,
                    |command| KeybindingDefaults::table().get(&command).cloned().unwrap_or_default(),
                    self.store.recent_commands(),
                    providers.clone(),
                )
            });
            let store: Rc<dyn CommandPaletteRecentStore> = self.store.clone();
            CommandPaletteView::new(style_sheet.clone(), store, model, providers, mtm)
        };
        let events: Rc<dyn CommandPaletteViewDelegate> = self.events.clone();
        palette.set_delegate(Some(Rc::downgrade(&events)));
        self.palette = Some(palette.clone());
        self.key_codes = scenario
            .array("keys")
            .iter()
            .filter_map(|value| value.as_i64().or_else(|| value.as_f64().map(|f| f as i64)))
            .map(|value| value as u16)
            .collect();

        if let Some(query) = scenario.string("query")
            && let Some(field) = self.search_field()
        {
            field.setStringValue(&NSString::from_str(&query));
            let notification = unsafe {
                NSNotification::notificationWithName_object(NSControlTextDidChangeNotification, Some(&*field))
            };
            let _: () = unsafe { msg_send![&*palette, controlTextDidChange: &*notification] };
        }
        if let Some(chip) = scenario.int("chip")
            && let Some(stack) = palette.subviews().iter().find_map(|view| downcast::<NSStackView>(&view))
        {
            let arranged = stack.arrangedSubviews();
            if chip >= 0
                && (chip as usize) < arranged.len()
                && let Some(button) = downcast::<NSButton>(&arranged.objectAtIndex(chip as usize))
            {
                unsafe { button.performClick(None) };
            }
        }
        if let Some(row) = scenario.int("select")
            && let Some(table) = self.table_view()
        {
            table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(row as usize), false);
        }
        if scenario.bool("doubleClick")
            && let Some(table) = self.table_view()
        {
            let _: () = unsafe { msg_send![&*palette, doubleClick: &*table] };
        }
        if scenario.bool("cancel") {
            let _: () = unsafe { msg_send![&*palette, cancelOperation: None::<&AnyObject>] };
        }
        if scenario.bool("restyle") {
            palette.set_style_sheet(style_sheet);
        }
        Ok(Retained::into_super(palette))
    }

    fn after_show(&mut self, window: &NSWindow, _scenario: &PanelScenario) {
        let mtm = window.mtm();
        for &key_code in &self.key_codes {
            let event = unsafe {
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
            };
            let Some(event) = event else { continue };
            NSApplication::sharedApplication(mtm).sendEvent(&event);
        }
    }

    fn model(&self) -> Value {
        let Some(palette) = &self.palette else { return Value::Null };
        let table = self.table_view();
        let empty_state = self.empty_state();
        let string = |value: Option<String>| value.map_or(Value::Null, Value::String);
        let mut map = Map::new();
        map.insert("preferredWidth".into(), double(palette.preferred_width()));
        map.insert("query".into(), string(self.search_field().map(|field| field.stringValue().to_string())));
        map.insert("rows".into(), table.as_ref().map_or(-1, |table| table.numberOfRows()).into());
        map.insert("selectedRow".into(), table.as_ref().map_or(-1, |table| table.selectedRow()).into());
        map.insert(
            "tableValue".into(),
            string(table.as_ref().and_then(|table| {
                table
                    .accessibilityValue()
                    .and_then(|value| value.downcast::<NSString>().ok())
                    .map(|value| value.to_string())
            })),
        );
        map.insert("emptyTitle".into(), string(empty_state.as_ref().map(|state| state.title())));
        map.insert("emptySubtitle".into(), string(empty_state.as_ref().map(|state| state.subtitle())));
        map.insert("emptyHidden".into(), Value::Bool(empty_state.as_ref().is_none_or(|state| state.isHidden())));
        map.insert(
            "chosen".into(),
            Value::Array(self.events.chosen.borrow().iter().map(|id| Value::String(id.clone())).collect()),
        );
        map.insert("cancels".into(), (*self.events.cancels.borrow()).into());
        map.insert(
            "recents".into(),
            Value::Array(
                self.store.values.borrow().iter().map(|command| Value::String(command.raw_value().to_owned())).collect(),
            ),
        );
        Value::Object(map)
    }
}
