//! `FindBarView` scenes (`Scenes/FindBarViewScene.swift`): the same state,
//! applied with the same calls in the same order; see the Swift file for the
//! keys.

use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{NSApplication, NSTextField, NSView, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use serde_json::{Map, Value};
use upleft_app::panels::appkit_support::{accessibility_label, cg, ns_string};
use upleft_app::panels::find_bar_view::{FindBarDelegate, FindBarView, Presentation};
use upleft_app::support::find_engine::{FindEngine, FindQuery, FindSession};
use upleft_core::NSRange;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

/// `FindBarSceneRecorder`.
#[derive(Default)]
pub struct FindBarSceneRecorder {
    pub queries: std::cell::RefCell<Vec<FindQuery>>,
}

impl FindBarDelegate for FindBarSceneRecorder {
    fn find_bar_did_change(&self, _bar: &FindBarView, query: FindQuery) {
        self.queries.borrow_mut().push(query);
    }
    fn find_bar_did_request_advance(&self, _bar: &FindBarView, _forward: bool) {}
    fn find_bar_did_request_replace(&self, _bar: &FindBarView, _replacement: &str, _all: bool) {}
    fn find_bar_did_request_close(&self, _bar: &FindBarView) {}
}

pub struct FindBarViewScene {
    bar: Option<Retained<FindBarView>>,
    session: Option<FindSession>,
    recorder: Rc<FindBarSceneRecorder>,
    inset: f64,
}

impl Default for FindBarViewScene {
    fn default() -> Self {
        FindBarViewScene { bar: None, session: None, recorder: Rc::default(), inset: 20.0 }
    }
}

/// `JSON.range(_:)` for a source range.
pub fn range_json(range: Option<NSRange>) -> Value {
    match range {
        Some(range) => Value::Array(vec![Value::from(range.location as i64), Value::from(range.length as i64)]),
        None => Value::Null,
    }
}

/// `FindBarViewScene.textField(_:in:)`: the first text field under `root`
/// (depth first) with the accessibility label.
pub fn text_field(label: &str, root: &NSView) -> Option<Retained<NSTextField>> {
    for view in root.subviews().iter() {
        if let Some(field) = view.downcast_ref::<NSTextField>()
            && accessibility_label(field).as_deref() == Some(label)
        {
            return Some(field.retain());
        }
        if let Some(found) = text_field(label, &view) {
            return Some(found);
        }
    }
    None
}

/// Sends each option-menu item's action to its target, as a click would.
pub fn toggle_options(bar: &FindBarView, options: &[String], mtm: MainThreadMarker) {
    if options.is_empty() {
        return;
    }
    let menu = bar.make_options_menu_for_testing();
    for title in options {
        let Some(item) = menu.itemWithTitle(&ns_string(title)) else { continue };
        let Some(action) = item.action() else { continue };
        let target = item.target();
        let from: &AnyObject = &item;
        let _ = unsafe { NSApplication::sharedApplication(mtm).sendAction_to_from(action, target.as_deref(), Some(from)) };
    }
}

/// `FindBarViewScene.applyFindState(_:_:)`: `selectionScope`, `options`,
/// `query` (with the document's find session), `status` and `valid`, in
/// that order.
pub fn apply_find_state(
    bar: &FindBarView,
    scenario: &PanelScenario,
    mtm: MainThreadMarker,
) -> Result<Option<FindSession>, Failure> {
    let mut session = None;
    let scope: Vec<i64> = scenario.array("selectionScope").iter().filter_map(|value| value.as_i64()).collect();
    if scope.len() == 2 {
        bar.set_selection_scope(Some(NSRange::new(scope[0] as isize, scope[1] as isize)));
    }
    toggle_options(bar, &scenario.strings("options"), mtm);
    if let Some(query) = scenario.string("query") {
        bar.set_query_text(&query, true);
        if scenario.document_path.is_some() {
            let text = scenario.document_text().map_err(Failure::Error)?;
            let current = bar.current_query();
            let mut found = FindSession::new();
            found.update(current.clone(), &text, scenario.int_or("caret", 0) as isize);
            for _ in 0..scenario.int_or("advance", 0) {
                let _ = found.advance(true);
            }
            bar.set_status_text(&found.status_text());
            bar.set_is_query_valid(FindEngine::is_valid(&current));
            session = Some(found);
        }
    }
    if let Some(status) = scenario.string("status") {
        bar.set_status_text(&status);
    }
    if scenario.state.contains_key("valid") {
        bar.set_is_query_valid(scenario.bool("valid"));
    }
    Ok(session)
}

impl PanelScene for FindBarViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let inspector = scenario.string("presentation").as_deref() == Some("inspector");
        let bar = if scenario.bool("current") {
            let bar = FindBarView::new_current(mtm);
            bar.set_style_sheet(style_sheet.clone());
            bar
        } else {
            FindBarView::new(
                style_sheet.clone(),
                if inspector { Presentation::Inspector } else { Presentation::Bar },
                mtm,
            )
        };
        let recorder: Rc<dyn FindBarDelegate> = self.recorder.clone();
        bar.set_delegate(Some(Rc::downgrade(&recorder)));
        self.session = apply_find_state(&bar, scenario, mtm)?;
        if scenario.bool("showsReplace") {
            bar.set_shows_replace(true);
        }
        if let Some(replacement) = scenario.string("replacement")
            && let Some(field) = text_field("Replace with", &bar)
        {
            field.setStringValue(&ns_string(&replacement));
        }
        self.inset = scenario.double_or("inset", 20.0);
        self.bar = Some(bar.clone());
        Ok(Retained::into_super(bar))
    }

    fn host(&mut self, panel: &NSView, window: &NSWindow, scenario: &PanelScenario) {
        let bar = match &self.bar {
            Some(bar) if scenario.string("presentation").as_deref() != Some("inspector") => bar.clone(),
            _ => {
                panel.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(scenario.width, scenario.height)));
                window.setContentView(Some(panel));
                return;
            }
        };
        let mtm = window.mtm();
        let container = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(scenario.width, scenario.height)),
        );
        container.setWantsLayer(true);
        if let Some(layer) = container.layer() {
            layer.setBackgroundColor(Some(&cg(&bar.style_sheet().background)));
        }
        window.setContentView(Some(&container));
        let height = bar.intrinsicContentSize().height;
        bar.setFrame(NSRect::new(
            NSPoint::new(self.inset, ((scenario.height - height) / 2.0).round()),
            NSSize::new(scenario.width - self.inset * 2.0, height),
        ));
        container.addSubview(&bar);
    }

    fn model(&self) -> Value {
        let Some(bar) = &self.bar else { return Value::Null };
        let query = bar.current_query();
        let menu = bar.make_options_menu_for_testing();
        let mut map = Map::new();
        map.insert("statusText".into(), Value::String(bar.status_text()));
        map.insert("isQueryValid".into(), Value::Bool(bar.is_query_valid()));
        map.insert("showsReplace".into(), Value::Bool(bar.shows_replace()));
        let mut query_map = Map::new();
        query_map.insert("text".into(), Value::String(query.text.clone()));
        query_map.insert("isRegex".into(), Value::Bool(query.is_regex));
        query_map.insert("caseSensitive".into(), Value::Bool(query.case_sensitive));
        query_map.insert("wholeWord".into(), Value::Bool(query.whole_word));
        query_map.insert("scope".into(), range_json(query.scope));
        map.insert("query".into(), Value::Object(query_map));
        map.insert("selectionScope".into(), range_json(bar.selection_scope()));
        map.insert("intrinsicContentSize".into(), tree::size(bar.intrinsicContentSize()));
        map.insert("dividerCount".into(), Value::from(bar.divider_count_for_testing() as i64));
        map.insert("hasCloseButton".into(), Value::Bool(bar.has_close_button_for_testing()));
        map.insert("searchFieldIsBezeled".into(), Value::Bool(bar.search_field_is_bezeled_for_testing()));
        map.insert("leadingGlyphIsAccessible".into(), Value::Bool(bar.leading_glyph_is_accessible_for_testing()));
        map.insert("findRowFrame".into(), tree::rect(bar.find_row_frame_for_testing()));
        map.insert("replaceRowFrame".into(), tree::rect(bar.replace_row_frame_for_testing()));
        map.insert("replaceRowAlpha".into(), double(bar.replace_row_alpha_for_testing()));
        map.insert("replaceRowIsHidden".into(), Value::Bool(bar.replace_row_is_hidden_for_testing()));
        map.insert("usesDenseReplaceMaterial".into(), Value::Bool(bar.uses_dense_replace_material_for_testing()));
        let options: Vec<Value> = menu
            .itemArray()
            .iter()
            .map(|item| {
                Value::Array(vec![
                    Value::String(item.title().to_string()),
                    Value::from(item.state() as i64),
                    Value::Bool(item.isEnabled()),
                    Value::Bool(item.isSeparatorItem()),
                ])
            })
            .collect();
        map.insert("options".into(), Value::Array(options));
        map.insert(
            "emittedQueries".into(),
            Value::Array(self.recorder.queries.borrow().iter().map(|query| Value::String(query.text.clone())).collect()),
        );
        if let Some(session) = &self.session {
            let matches = session.matches();
            let mut session_map = Map::new();
            session_map.insert("count".into(), Value::from(session.count() as i64));
            session_map.insert(
                "currentIndex".into(),
                session.current_index().map_or(Value::Null, |index| Value::from(index as i64)),
            );
            session_map.insert("currentMatch".into(), range_json(session.current_match()));
            session_map.insert(
                "first".into(),
                Value::Array(matches.iter().take(8).map(|range| range_json(Some(*range))).collect()),
            );
            let tail = matches.len().saturating_sub(8);
            session_map.insert(
                "last".into(),
                Value::Array(matches[tail..].iter().map(|range| range_json(Some(*range))).collect()),
            );
            map.insert("session".into(), Value::Object(session_map));
        }
        Value::Object(map)
    }
}
