//! `InspectorHostView` scenes (`Scenes/InspectorHostViewScene.swift`).

use std::cell::RefCell;
use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use serde_json::{Map, Value};
use upleft_app::panels::appkit_support::label;
use upleft_app::panels::inspector_host_view::{InspectorHostView, InspectorSection};
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
pub struct InspectorHostViewScene {
    host: Option<Retained<InspectorHostView>>,
    selections: Rc<RefCell<Vec<String>>>,
}

pub fn section(name: &str) -> Option<InspectorSection> {
    match name {
        "tasks" => Some(InspectorSection::Tasks),
        "history" => Some(InspectorSection::History),
        "context" => Some(InspectorSection::Context),
        "search" => Some(InspectorSection::Search),
        _ => None,
    }
}

impl PanelScene for InspectorHostViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let host = InspectorHostView::new(
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(scenario.width, scenario.height)),
            mtm,
        );
        host.set_style_sheet(style_sheet.clone());
        let selections = self.selections.clone();
        host.set_on_selection_change(Some(Rc::new(move |section: Option<InspectorSection>| {
            selections.borrow_mut().push(section.map_or("nil".to_owned(), |section| section.title().to_owned()));
        })));
        for name in scenario.strings("sections") {
            let Some(section) = section(&name) else { continue };
            let content = label(&format!("{} content", section.title()), mtm);
            content.setTextColor(Some(&style_sheet.text));
            host.set_content(&content, section);
        }
        let titles = scenario.object("titles");
        let mut names: Vec<&String> = titles.keys().collect();
        names.sort();
        for name in names {
            let (Some(section), Some(title)) = (section(name), titles.get(name).and_then(Value::as_str)) else { continue };
            host.set_title(title, section);
        }
        if let Some(select) = scenario.string("select")
            && let Some(section) = section(&select)
        {
            host.select(section);
        }
        for name in scenario.strings("remove") {
            let Some(section) = section(&name) else { continue };
            host.remove_content(section);
        }
        self.host = Some(host.clone());
        Ok(Retained::into_super(host))
    }

    fn model(&self) -> Value {
        let Some(host) = &self.host else { return Value::Null };
        let mut map = Map::new();
        map.insert(
            "selectedSection".into(),
            host.selected_section().map_or(Value::Null, |section| Value::String(section.title().into())),
        );
        map.insert("contentCount".into(), Value::from(host.content_count() as i64));
        map.insert("hasContent".into(), Value::Bool(host.has_content()));
        map.insert("floatingFittingHeight".into(), double(host.floating_fitting_height()));
        map.insert("closeButtonFrame".into(), tree::rect(host.close_button_for_testing().frame()));
        map.insert(
            "selections".into(),
            Value::Array(self.selections.borrow().iter().cloned().map(Value::String).collect()),
        );
        Value::Object(map)
    }
}
