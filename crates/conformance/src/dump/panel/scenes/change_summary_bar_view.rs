//! `ChangeSummaryBarView` scenes (`Scenes/ChangeSummaryBarViewScene.swift`):
//! the same state, applied with the same calls in the same order; see the
//! Swift file for the keys.

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSButton, NSView, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use serde_json::{Map, Value};
use upleft_app::ai::change_tracker::{Mark, change_kind_from_raw_value};
use upleft_app::panels::appkit_support::{accessibility_label, cg};
use upleft_app::panels::change_summary_bar_view::{ChangeSummaryBarDelegate, ChangeSummaryBarView, Summary};
use upleft_core::NSRange;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

/// `ChangeSummarySceneRecorder`.
#[derive(Default)]
pub struct ChangeSummarySceneRecorder {
    events: RefCell<Vec<String>>,
}

impl ChangeSummaryBarDelegate for ChangeSummarySceneRecorder {
    fn change_summary_bar_did_request_jump(&self, _bar: &ChangeSummaryBarView, forward: bool) {
        self.events.borrow_mut().push(if forward { "next" } else { "previous" }.to_owned());
    }
    fn change_summary_bar_did_request_mark_reviewed(&self, _bar: &ChangeSummaryBarView) {
        self.events.borrow_mut().push("reviewed".to_owned());
    }
    fn change_summary_bar_did_request_dismiss(&self, _bar: &ChangeSummaryBarView) {
        self.events.borrow_mut().push("dismiss".to_owned());
    }
}

#[derive(Default)]
pub struct ChangeSummaryBarViewScene {
    bar: Option<Retained<ChangeSummaryBarView>>,
    summary: Option<Summary>,
    recorder: Rc<ChangeSummarySceneRecorder>,
}

/// `ChangeSummaryBarViewScene.buttons(in:)`: buttons under `view`, not
/// descending into a button.
pub fn buttons(view: &NSView) -> Vec<Retained<NSButton>> {
    let mut out = Vec::new();
    for subview in view.subviews().iter() {
        match subview.clone().downcast::<NSButton>() {
            Ok(button) => out.push(button),
            Err(subview) => out.extend(buttons(&subview)),
        }
    }
    out
}

impl PanelScene for ChangeSummaryBarViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let bar = if scenario.bool("current") {
            ChangeSummaryBarView::new_current(mtm)
        } else {
            ChangeSummaryBarView::new(style_sheet.clone(), mtm)
        };
        if scenario.bool("current") {
            bar.set_style_sheet(style_sheet);
        }
        let recorder: Rc<dyn ChangeSummaryBarDelegate> = self.recorder.clone();
        bar.set_delegate(Some(Rc::downgrade(&recorder)));
        if let Some(count) = scenario.int("changeCount") {
            bar.configure_count(&scenario.string_or("message", "Updated on disk"), count as isize);
        } else if scenario.state.contains_key("marks") {
            let length = if scenario.document_path.is_some() {
                scenario.document_text().map_err(Failure::Error)?.encode_utf16().count() as isize
            } else {
                scenario.int_or("documentLength", 0) as isize
            };
            let marks: Vec<Mark> = scenario
                .array("marks")
                .iter()
                .filter_map(|entry| {
                    let parts = entry.as_array()?;
                    if parts.len() != 3 {
                        return None;
                    }
                    let kind = change_kind_from_raw_value(parts[0].as_str()?)?;
                    let location = parts[1].as_i64().or_else(|| parts[1].as_f64().map(|f| f as i64))?;
                    let size = parts[2].as_i64().or_else(|| parts[2].as_f64().map(|f| f as i64))?;
                    Some(Mark::new(kind, NSRange::new(location as isize, size as isize)))
                })
                .collect();
            let summary = Summary::from_marks(&marks, length);
            bar.configure(scenario.string("message").as_deref(), summary.clone());
            self.summary = Some(summary);
        }
        for label in scenario.strings("press") {
            if let Some(button) =
                buttons(&bar).into_iter().find(|button| accessibility_label(&**button).as_deref() == Some(&*label))
            {
                unsafe { button.performClick(None) };
            }
        }
        self.bar = Some(bar.clone());
        Ok(Retained::into_super(Retained::into_super(bar)))
    }

    fn host(&mut self, panel: &NSView, window: &NSWindow, scenario: &PanelScenario) {
        let mtm = window.mtm();
        let container = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(scenario.width, scenario.height)),
        );
        container.setWantsLayer(true);
        if let Some(bar) = &self.bar
            && let Some(layer) = container.layer()
        {
            layer.setBackgroundColor(Some(&cg(&bar.style_sheet().background)));
        }
        window.setContentView(Some(&container));
        let size = panel.intrinsicContentSize();
        panel.setFrame(NSRect::new(
            NSPoint::new(((scenario.width - size.width) / 2.0).round(), ((scenario.height - size.height) / 2.0).round()),
            size,
        ));
        container.addSubview(panel);
    }

    fn model(&self) -> Value {
        let Some(bar) = &self.bar else { return Value::Null };
        let mut map = Map::new();
        map.insert("message".into(), Value::String(bar.message()));
        map.insert("positionStatus".into(), Value::String(bar.position_status_for_testing()));
        map.insert("intrinsicContentSize".into(), tree::size(bar.intrinsicContentSize()));
        map.insert("fittedWidth".into(), double(bar.fitted_width()));
        map.insert("acceptsFirstResponder".into(), Value::Bool(bar.acceptsFirstResponder()));
        let rows: Vec<Value> = buttons(bar)
            .iter()
            .map(|button| {
                Value::Array(vec![
                    accessibility_label(&**button).map_or(Value::Null, Value::String),
                    Value::Bool(button.isHidden()),
                    Value::Bool(button.isEnabled()),
                    button.toolTip().map_or(Value::Null, |tip| Value::String(tip.to_string())),
                ])
            })
            .collect();
        map.insert("buttons".into(), Value::Array(rows));
        map.insert(
            "events".into(),
            Value::Array(self.recorder.events.borrow().iter().cloned().map(Value::String).collect()),
        );
        if let Some(summary) = &self.summary {
            let mut object = Map::new();
            object.insert("added".into(), Value::from(summary.added as i64));
            object.insert("rewritten".into(), Value::from(summary.rewritten as i64));
            object.insert("removed".into(), Value::from(summary.removed as i64));
            object.insert("total".into(), Value::from(summary.total() as i64));
            object.insert("headline".into(), Value::String(summary.headline()));
            object.insert("accessibilityDescription".into(), Value::String(summary.accessibility_description()));
            object.insert(
                "distributionDescription".into(),
                summary.distribution_description().map_or(Value::Null, Value::String),
            );
            object.insert(
                "positions".into(),
                Value::Array(
                    summary
                        .positions
                        .iter()
                        .map(|position| {
                            Value::Array(vec![
                                double(position.fraction),
                                Value::String(position.kind.raw_value().to_owned()),
                            ])
                        })
                        .collect(),
                ),
            );
            map.insert("summary".into(), Value::Object(object));
        }
        Value::Object(map)
    }
}
