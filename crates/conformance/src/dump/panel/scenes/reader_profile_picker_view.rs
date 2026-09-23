//! `ReaderProfilePickerView` scenes (`Scenes/ReaderProfilePickerViewScene.swift`),
//! over an in-memory store: the JSON store and the support folder are never
//! touched.

use std::cell::RefCell;
use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSButton, NSPopUpButton, NSStackView, NSView};
use serde_json::{Map, Value};
use upleft_app::panels::panel_chrome::PanelSurface;
use upleft_app::panels::reader_profile_picker_view::{ReaderProfilePickerDelegate, ReaderProfilePickerView};
use upleft_app::support::reader_profiles::{
    InMemoryReaderProfileStore, ReaderChromeDensity, ReaderMotionPreference, ReaderProfile, ReaderProfileStore,
    ReaderTypographyScale,
};
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
struct Delegate {
    events: RefCell<Vec<String>>,
}

impl ReaderProfilePickerDelegate for Delegate {
    fn reader_profile_picker_did_preview(&self, _picker: &ReaderProfilePickerView, profile: &ReaderProfile) {
        self.events.borrow_mut().push(format!("preview {}", profile.name));
    }

    fn reader_profile_picker_did_select(&self, _picker: &ReaderProfilePickerView, profile: &ReaderProfile) {
        self.events.borrow_mut().push(format!("select {}", profile.name));
    }
}

fn describe(profile: &ReaderProfile) -> Value {
    let mut map = Map::new();
    map.insert("name".into(), Value::String(profile.name.clone()));
    map.insert("isBuiltIn".into(), Value::Bool(profile.is_built_in));
    map.insert("typographyScale".into(), Value::String(profile.typography_scale.raw_value().into()));
    map.insert("measureCharacters".into(), double(profile.measure_characters));
    map.insert("chromeDensity".into(), Value::String(profile.chrome_density.raw_value().into()));
    map.insert("motionPreference".into(), Value::String(profile.motion_preference.raw_value().into()));
    Value::Object(map)
}

fn send(button: &NSButton) {
    unsafe { button.sendAction_to(button.action(), button.target().as_deref()) };
}

#[derive(Default)]
pub struct ReaderProfilePickerViewScene {
    picker: Option<Retained<ReaderProfilePickerView>>,
    store: Option<Rc<InMemoryReaderProfileStore>>,
    delegate: Option<Rc<Delegate>>,
}

impl PanelScene for ReaderProfilePickerViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let custom: Vec<ReaderProfile> = scenario
            .array("custom")
            .iter()
            .filter_map(|value| {
                let object = value.as_object()?;
                let text = |key: &str| object.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
                Some(ReaderProfile::with(
                    &text("id"),
                    &text("name"),
                    false,
                    ReaderTypographyScale::from_raw_value(&text("scale")).unwrap_or(ReaderTypographyScale::Standard),
                    object.get("measure").and_then(Value::as_f64).unwrap_or(70.0),
                    ReaderChromeDensity::from_raw_value(&text("density")).unwrap_or(ReaderChromeDensity::Comfortable),
                    ReaderMotionPreference::from_raw_value(&text("motion"))
                        .unwrap_or(ReaderMotionPreference::FollowSystem),
                ))
            })
            .collect();
        let store = Rc::new(InMemoryReaderProfileStore::new(custom));
        let picker = if scenario.bool("current") {
            ReaderProfilePickerView::new(store.clone(), Rc::new(StyleSheet::current(mtm)), mtm)
        } else {
            ReaderProfilePickerView::new(store.clone(), style_sheet.clone(), mtm)
        };
        if scenario.bool("current") {
            picker.set_style_sheet(style_sheet);
        }
        let delegate = Rc::new(Delegate::default());
        let weak: std::rc::Weak<dyn ReaderProfilePickerDelegate> =
            Rc::downgrade(&(delegate.clone() as Rc<dyn ReaderProfilePickerDelegate>));
        picker.set_delegate(Some(weak));
        self.delegate = Some(delegate);
        if let Some(id) = scenario.string("select") {
            picker.select_profile(&id);
        }
        let subviews = picker.subviews();
        let popups: Vec<Retained<NSPopUpButton>> =
            subviews.iter().filter_map(|view| view.downcast::<NSPopUpButton>().ok()).collect();
        if let Some(index) = scenario.int("profileIndex")
            && let Some(popup) = popups.first()
        {
            popup.selectItemAtIndex(index as isize);
            unsafe { popup.sendAction_to(popup.action(), popup.target().as_deref()) };
        }
        for pair in scenario.array("controls") {
            let values: Vec<i64> = pair
                .as_array()
                .map(|items| items.iter().filter_map(|item| item.as_i64().or_else(|| item.as_f64().map(|f| f as i64))).collect())
                .unwrap_or_default();
            if !(values.len() == 2 && ((values[0] + 1) as usize) < popups.len()) {
                continue;
            }
            let popup = &popups[(values[0] + 1) as usize];
            popup.selectItemAtIndex(values[1] as isize);
            unsafe { popup.sendAction_to(popup.action(), popup.target().as_deref()) };
        }
        if let Some(name) = scenario.string("save") {
            picker.save_custom_profile_for_testing(&name);
        }
        let subviews = picker.subviews();
        let buttons: Vec<Retained<NSButton>> = subviews
            .iter()
            .filter(|view| view.downcast_ref::<NSPopUpButton>().is_none())
            .filter_map(|view| view.downcast::<NSButton>().ok())
            .collect();
        let rows: Vec<Retained<NSButton>> = subviews
            .iter()
            .filter_map(|view| view.downcast::<NSStackView>().ok())
            .flat_map(|stack| {
                stack.arrangedSubviews().iter().filter_map(|view| view.downcast::<NSButton>().ok()).collect::<Vec<_>>()
            })
            .collect();
        if scenario.bool("delete")
            && let Some(button) = buttons.iter().find(|button| button.title().to_string() == "Delete Custom")
        {
            send(button);
        }
        if scenario.bool("revert")
            && let Some(button) = rows.iter().find(|button| button.title().to_string() == "Revert")
        {
            send(button);
        }
        self.picker = Some(picker.clone());
        self.store = Some(store);
        Ok(Retained::into_super(picker))
    }

    fn model(&self) -> Value {
        let Some(picker) = &self.picker else { return Value::Null };
        let mut map = Map::new();
        map.insert("preferredWidth".into(), double(picker.preferred_width()));
        map.insert("selectedProfile".into(), describe(&picker.selected_profile()));
        map.insert(
            "profiles".into(),
            Value::Array(picker.profiles().into_iter().map(|profile| Value::String(profile.name)).collect()),
        );
        let stored = self.store.as_ref().map(|store| store.load_custom_profiles()).unwrap_or_default();
        map.insert("stored".into(), Value::Array(stored.iter().map(describe).collect()));
        let events = self.delegate.as_ref().map(|delegate| delegate.events.borrow().clone()).unwrap_or_default();
        map.insert("events".into(), Value::Array(events.into_iter().map(Value::String).collect()));
        map.insert("fittingSize".into(), tree::size(picker.fittingSize()));
        Value::Object(map)
    }
}
