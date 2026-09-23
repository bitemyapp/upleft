//! `PanelChrome.swift`'s controls (`Scenes/PanelChromeScene.swift`).

use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly as _};
use objc2_app_kit::{NSControlStateValueOn, NSTableRowView, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use serde_json::{Map, Value};
use upleft_app::panels::panel_chrome::{
    ButtonAction, CheckState, CheckboxGeometry, MessageBarView, PanelBackdrop, PanelButton, PanelCheckbox,
    PanelEmptyStateView, PanelGroupRowView, PanelMetrics, PanelProgressBar, PanelSegmentedControl, PanelSelectionRowView,
    RelativeTime, SourceLineIndex,
};
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
pub struct PanelChromeScene {
    values: Map<String, Value>,
}

/// `state.frame` as a rect, or `fallback`.
pub fn frame_or(scenario: &PanelScenario, fallback: NSRect) -> NSRect {
    let values: Vec<f64> = scenario.array("frame").iter().filter_map(Value::as_f64).collect();
    if values.len() == 4 {
        NSRect::new(NSPoint::new(values[0], values[1]), NSSize::new(values[2], values[3]))
    } else {
        fallback
    }
}

/// `NSView(frame: NSRect(x: 0, y: 0, width: scenario.width, height: scenario.height))`.
pub fn container(scenario: &PanelScenario, mtm: MainThreadMarker) -> Retained<NSView> {
    NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(scenario.width, scenario.height)),
    )
}

impl PanelScene for PanelChromeScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let container = container(scenario, mtm);
        let frame = frame_or(scenario, container.bounds());
        let kind = scenario.string_or("kind", "");
        let view: Retained<NSView> = match kind.as_str() {
            "messageBar" => {
                let bar = MessageBarView::new(style_sheet.clone(), style_sheet.accent.clone(), mtm);
                bar.set_message(&scenario.string_or("message", ""));
                for title in scenario.strings("actions") {
                    bar.add_action(&title, || {});
                }
                for symbol in scenario.strings("symbols") {
                    bar.add_symbol_action(&symbol, &symbol, || {});
                }
                if let Some(status) = scenario.string("status") {
                    bar.set_status(&status);
                }
                if scenario.bool("reviewLayout") {
                    bar.use_review_bar_layout();
                }
                self.values.insert("fittedWidth".into(), double(bar.fitted_width()));
                Retained::into_super(bar)
            }
            "segmented" => {
                let items = scenario.strings("items");
                let items: Vec<&str> = items.iter().map(String::as_str).collect();
                let control =
                    PanelSegmentedControl::new(&items, scenario.int_or("selectedIndex", 0) as isize, style_sheet.clone(), mtm);
                for index in scenario.array("disabled").iter().filter_map(Value::as_i64) {
                    control.set_enabled(false, index as isize);
                }
                if let Some(select) = scenario.int("select") {
                    control.set_selected_index(select as isize, false);
                }
                self.values.insert("selectedIndex".into(), Value::from(control.selected_index() as i64));
                self.values.insert("intrinsicContentSize".into(), tree::size(control.intrinsicContentSize()));
                Retained::into_super(control)
            }
            "progress" => {
                let bar = PanelProgressBar::new(style_sheet.clone(), mtm);
                bar.set_fraction(scenario.double_or("fraction", 0.0));
                self.values.insert("fraction".into(), double(bar.fraction()));
                Retained::into_super(bar)
            }
            "checkbox" => {
                let side = scenario.double_or("side", CheckboxGeometry::PANEL_SIDE);
                let checkbox = PanelCheckbox::new(side, CheckboxGeometry::CORNER_RATIO, mtm);
                checkbox.set_style_sheet(style_sheet.clone());
                match scenario.string_or("checkState", "off").as_str() {
                    "on" => checkbox.set_state(CheckState::On, false),
                    "mixed" => checkbox.set_state(CheckState::Mixed, false),
                    _ => {}
                }
                if scenario.bool("toggle") {
                    checkbox.perform_toggle();
                }
                let state = match checkbox.state() {
                    CheckState::Off => "off",
                    CheckState::On => "on",
                    CheckState::Mixed => "mixed",
                };
                self.values.insert("state".into(), Value::String(state.into()));
                self.values.insert("hitBounds".into(), tree::rect(checkbox.hit_bounds()));
                Retained::into_super(checkbox)
            }
            "emptyState" => {
                let list = NSView::initWithFrame(NSView::alloc(mtm), container.bounds());
                container.addSubview(&list);
                let empty = PanelEmptyStateView::new(mtm);
                empty.configure(
                    &scenario.string_or("symbol", "checkmark.seal"),
                    &scenario.string_or("title", ""),
                    &scenario.string_or("subtitle", ""),
                    &style_sheet,
                );
                empty.install(&container, &list, scenario.double_or("bias", 1.0));
                empty.setHidden(false);
                self.values.insert("title".into(), Value::String(empty.title()));
                self.values.insert("subtitle".into(), Value::String(empty.subtitle()));
                return Ok(container);
            }
            "groupRow" => {
                let row = PanelGroupRowView::new(&NSString::from_str("group"), mtm);
                row.configure(&scenario.string_or("text", ""), &style_sheet.text_secondary);
                Retained::into_super(row)
            }
            "backdrop" => {
                let backdrop = PanelBackdrop::new_default(style_sheet.clone(), mtm);
                if scenario.bool("usesSurfaceFill") {
                    backdrop.set_uses_surface_fill(true);
                }
                if let Some(veil) = scenario.double("veilAlpha") {
                    backdrop.set_veil_alpha(veil);
                }
                if scenario.bool("opaqueAccent") {
                    backdrop.set_opaque_surface_color(Some(style_sheet.accent.clone()));
                }
                if scenario.bool("blendsWithinWindow") {
                    backdrop.set_blends_within_window(true);
                }
                Retained::into_super(backdrop)
            }
            "symbolButton" => {
                let action = ButtonAction::noop(mtm);
                let button = PanelButton::symbol(
                    &scenario.string_or("symbol", "xmark"),
                    &scenario.string_or("label", "Close"),
                    &action,
                    scenario.double_or("pointSize", 13.0),
                    upleft_app::panels::appkit_support::weight_medium(),
                    false,
                    mtm,
                );
                if scenario.bool("tinted") {
                    button.setContentTintColor(Some(&style_sheet.text_secondary));
                }
                if scenario.bool("disabled") {
                    button.setEnabled(false);
                }
                button.setTranslatesAutoresizingMaskIntoConstraints(true);
                self.values.insert("intrinsicContentSize".into(), tree::size(button.intrinsicContentSize()));
                Retained::into_super(Retained::into_super(button))
            }
            "textButton" => {
                let action = ButtonAction::noop(mtm);
                let button = PanelButton::text(&scenario.string_or("title", ""), &action, scenario.bool("isDefault"), mtm);
                button.setTranslatesAutoresizingMaskIntoConstraints(true);
                self.values.insert("intrinsicContentSize".into(), tree::size(button.intrinsicContentSize()));
                Retained::into_super(Retained::into_super(button))
            }
            "toggle" => {
                let action = ButtonAction::noop(mtm);
                let button =
                    PanelButton::toggle(&scenario.string_or("title", ""), &scenario.string_or("label", ""), &action, mtm);
                if scenario.bool("on") {
                    button.setState(NSControlStateValueOn);
                }
                button.setTranslatesAutoresizingMaskIntoConstraints(true);
                Retained::into_super(Retained::into_super(button))
            }
            "selectionRow" => {
                let row = PanelSelectionRowView::new(mtm);
                row.set_style_sheet(style_sheet.clone());
                NSTableRowView::setSelected(&row, scenario.bool("selected"));
                Retained::into_super(Retained::into_super(row))
            }
            "lineIndex" => {
                let index = SourceLineIndex::new(&scenario.document_text().map_err(Failure::Error)?);
                let mut captions = Vec::new();
                for pair in scenario.array("ranges") {
                    let Some(numbers) = pair.as_array() else { continue };
                    if numbers.len() != 2 {
                        continue;
                    }
                    let (Some(location), Some(length)) = (numbers[0].as_i64(), numbers[1].as_i64()) else { continue };
                    let range = upleft_core::NSRange::new(location as isize, length as isize);
                    captions.push(Value::Array(vec![
                        Value::from(index.line(range.location) as i64),
                        Value::String(index.caption(range)),
                    ]));
                }
                self.values.insert("captions".into(), Value::Array(captions));
                NSView::new(mtm)
            }
            "relativeTime" => {
                let now = upleft_foundation::date::Date::from_reference(scenario.double_or("now", 800_000_000.0));
                let mut strings = Vec::new();
                for offset in scenario.array("offsets").iter().filter_map(Value::as_f64) {
                    let date = now.adding(-offset);
                    strings.push(Value::Array(vec![
                        Value::String(RelativeTime::short(date, now)),
                        Value::String(RelativeTime::long(date, now)),
                        Value::String(RelativeTime::stamp(date)),
                    ]));
                }
                self.values.insert("strings".into(), Value::Array(strings));
                NSView::new(mtm)
            }
            "metrics" => {
                let mut rows = Vec::new();
                for height in scenario.array("heights").iter().filter_map(Value::as_f64) {
                    rows.push(Value::Array(vec![
                        double(PanelMetrics::capsule_radius(height)),
                        double(PanelMetrics::control_radius(height)),
                        tree::rect(PanelMetrics::row_surface(NSRect::new(
                            NSPoint::new(0.0, 0.0),
                            NSSize::new(100.0, height),
                        ))),
                    ]));
                }
                self.values.insert("radii".into(), Value::Array(rows));
                let path = PanelMetrics::continuous_rounded_path(
                    NSRect::new(NSPoint::new(2.0, 3.0), NSSize::new(120.0, 40.0)),
                    scenario.double_or("radius", 12.0),
                );
                self.values.insert(
                    "pathBounds".into(),
                    tree::rect(objc2_core_graphics::CGPath::path_bounding_box(Some(&path))),
                );
                NSView::new(mtm)
            }
            other => return Err(Failure::Error(format!("unknown PanelChrome kind {other}"))),
        };
        view.setFrame(frame);
        container.addSubview(&view);
        Ok(container)
    }

    fn model(&self) -> Value {
        Value::Object(self.values.clone())
    }
}
