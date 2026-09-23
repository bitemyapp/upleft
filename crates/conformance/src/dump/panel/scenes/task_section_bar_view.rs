//! `TaskSectionBarView` scenes (`Scenes/TaskSectionBarViewScene.swift`): the
//! bar in a plain container on the task panel's rail, given the segments of
//! the scenario document's worklist, then `state.actions` in order. See the
//! Swift file for the action list and what Reduce Motion does here.

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly, msg_send};
use objc2_app_kit::{NSAccessibility, NSEvent, NSEventModifierFlags, NSEventType, NSView};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use serde_json::{Map, Value};
use upleft_app::panels::appkit_support::{RectExt, activate};
use upleft_app::panels::task_panel_view::TaskRowMetrics;
use upleft_app::panels::task_section_bar_view::TaskSectionBarView;
use upleft_core::parser::MarkdownParser;
use upleft_core::task_worklist::TaskWorklist;
use upleft_render::theme::style_sheet::StyleSheet;

use super::task_panel_view::key_event;
use crate::dump::Failure;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
pub struct TaskSectionBarViewScene {
    bar: Option<Retained<TaskSectionBarView>>,
    selections: Rc<RefCell<Vec<isize>>>,
}

/// `TaskSectionBarViewScene.mouseEvent(_:at:)`.
fn mouse_event(event_type: NSEventType, point: NSPoint) -> Retained<NSEvent> {
    NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
        event_type,
        point,
        NSEventModifierFlags::empty(),
        0.0,
        0,
        None,
        0,
        1,
        0.0,
    )
    .expect("mouse event")
}

impl PanelScene for TaskSectionBarViewScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let document = MarkdownParser::parse(&scenario.document_text().map_err(Failure::Error)?);
        let container = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(scenario.width, scenario.height)),
        );
        let bar = TaskSectionBarView::new(style_sheet, mtm);
        let selections = self.selections.clone();
        bar.set_on_select_segment(Some(Rc::new(move |index| selections.borrow_mut().push(index))));
        container.addSubview(&bar);
        activate(&[
            bar.leadingAnchor()
                .constraintEqualToAnchor_constant(&container.leadingAnchor(), TaskRowMetrics::CONTENT_INSET),
            bar.trailingAnchor()
                .constraintEqualToAnchor_constant(&container.trailingAnchor(), -TaskRowMetrics::CONTENT_INSET),
            bar.topAnchor().constraintEqualToAnchor_constant(&container.topAnchor(), scenario.double_or("top", 20.0)),
        ]);
        bar.set_segments(TaskWorklist::new(&document.tasks, &document.headings).segments);
        container.layoutSubtreeIfNeeded();
        for action in scenario.array("actions") {
            let Some(action) = action.as_array() else { continue };
            let Some(name) = action.first().and_then(Value::as_str) else { continue };
            let value = action.get(1).and_then(Value::as_f64).unwrap_or(0.0);
            let bounds = bar.bounds();
            let point = bar.convertPoint_toView(NSPoint::new(bounds.width() * value, bounds.mid_y()), None);
            match name {
                "hover" => {
                    let event = mouse_event(NSEventType::MouseMoved, point);
                    let _: () = unsafe { msg_send![&*bar, mouseMoved: &*event] };
                }
                "press" => {
                    let event = mouse_event(NSEventType::LeftMouseDown, point);
                    let _: () = unsafe { msg_send![&*bar, mouseDown: &*event] };
                }
                "release" => {
                    let event = mouse_event(NSEventType::LeftMouseUp, point);
                    let _: () = unsafe { msg_send![&*bar, mouseUp: &*event] };
                }
                "exit" => {
                    let event = mouse_event(NSEventType::MouseMoved, point);
                    let _: () = unsafe { msg_send![&*bar, mouseExited: &*event] };
                }
                "key" => {
                    let event = key_event(value as u16, false, "");
                    let _: () = unsafe { msg_send![&*bar, keyDown: &*event] };
                }
                "clear" => bar.set_segments(Vec::new()),
                _ => {}
            }
        }
        self.bar = Some(bar);
        Ok(container)
    }

    fn model(&self) -> Value {
        let Some(bar) = &self.bar else { return Value::Null };
        let accessibility_value: Option<Retained<objc2::runtime::AnyObject>> = bar.accessibilityValue();
        let accessibility_value = accessibility_value
            .and_then(|value| value.downcast::<objc2_foundation::NSString>().ok())
            .map(|value| value.to_string());
        let actions = bar.accessibilityCustomActions().map(|actions| actions.to_vec()).unwrap_or_default();
        let mut map = Map::new();
        map.insert("segmentCount".into(), Value::from(bar.segments().len() as i64));
        map.insert("accessibilityValue".into(), accessibility_value.map_or(Value::Null, Value::String));
        map.insert(
            "customActions".into(),
            Value::Array(actions.iter().map(|action| Value::String(action.name().to_string())).collect()),
        );
        map.insert("intrinsicContentSize".into(), tree::size(bar.intrinsicContentSize()));
        map.insert("acceptsFirstResponder".into(), Value::Bool(bar.acceptsFirstResponder()));
        map.insert("frame".into(), tree::rect(bar.frame()));
        map.insert(
            "selections".into(),
            Value::Array(self.selections.borrow().iter().map(|index| Value::from(*index as i64)).collect()),
        );
        Value::Object(map)
    }
}
