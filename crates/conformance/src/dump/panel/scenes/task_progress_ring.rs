//! `TaskProgressRing` scenes (`Scenes/TaskProgressRingScene.swift`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, msg_send};
use objc2_app_kit::{NSAccessibility, NSEvent, NSEventModifierFlags, NSEventType, NSView};
use objc2_foundation::NSPoint;
use serde_json::{Map, Value};
use upleft_app::panels::task_progress_ring::TaskProgressRing;
use upleft_core::parser::MarkdownParser;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
pub struct TaskProgressRingScene {
    ring: Option<Retained<TaskProgressRing>>,
    activations: Rc<Cell<i64>>,
    visibility_changes: Rc<RefCell<Vec<bool>>>,
    press_result: Option<bool>,
}

fn enter_exit(kind: NSEventType) -> Option<Retained<NSEvent>> {
    unsafe {
        NSEvent::enterExitEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_trackingNumber_userData(
            kind,
            NSPoint::new(0.0, 0.0),
            NSEventModifierFlags::empty(),
            0.0,
            0,
            None,
            0,
            0,
            std::ptr::null_mut(),
        )
    }
}

fn int(value: &Value) -> i64 {
    value.as_i64().or_else(|| value.as_f64().map(|f| f as i64)).unwrap_or(0)
}

impl PanelScene for TaskProgressRingScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let ring = if scenario.bool("current") {
            TaskProgressRing::new_current(mtm)
        } else {
            TaskProgressRing::new(style_sheet.clone(), mtm)
        };
        if scenario.bool("current") {
            ring.set_style_sheet(style_sheet.clone());
        }
        let activations = self.activations.clone();
        ring.set_on_activate(Some(Rc::new(move || activations.set(activations.get() + 1))));
        let changes = self.visibility_changes.clone();
        ring.set_on_visibility_change(Some(Rc::new(move |hidden| changes.borrow_mut().push(hidden))));
        if scenario.bool("fromDocument") {
            let text = scenario.document_text().map_err(Failure::Error)?;
            let parsed = MarkdownParser::parse(&text);
            let completed_tasks = parsed.tasks.iter().filter(|task| task.is_checked).count() as isize;
            ring.set_progress(completed_tasks, parsed.tasks.len() as isize);
        }
        for step in scenario.array("steps") {
            let Some(pair) = step.as_array() else { continue };
            if pair.len() != 2 {
                continue;
            }
            ring.set_progress(int(&pair[0]) as isize, int(&pair[1]) as isize);
        }
        if scenario.bool("active") {
            ring.set_is_active(true);
        }
        if scenario.bool("inactive") {
            ring.set_is_active(false);
        }
        if scenario.bool("hover")
            && let Some(event) = enter_exit(NSEventType::MouseEntered)
        {
            let _: () = unsafe { msg_send![&*ring, mouseEntered: &*event] };
        }
        if scenario.bool("exit")
            && let Some(event) = enter_exit(NSEventType::MouseExited)
        {
            let _: () = unsafe { msg_send![&*ring, mouseExited: &*event] };
        }
        if scenario.bool("press") {
            self.press_result = Some(ring.accessibilityPerformPress());
        }
        if scenario.bool("restyle") {
            ring.set_style_sheet(style_sheet);
        }
        self.ring = Some(ring.clone());
        Ok(Retained::into_super(ring))
    }

    fn model(&self) -> Value {
        let Some(ring) = &self.ring else { return Value::Null };
        let (done, total) = ring.progress();
        let focus_ring_mask_bounds: objc2_foundation::NSRect = unsafe { msg_send![&**ring, focusRingMaskBounds] };
        let mouse_down_can_move_window: bool = unsafe { msg_send![&**ring, mouseDownCanMoveWindow] };
        let mut map = Map::new();
        map.insert("progress".into(), Value::Array(vec![done.into(), total.into()]));
        map.insert("countText".into(), Value::String(ring.count_text_for_testing()));
        map.insert("isActive".into(), Value::Bool(ring.is_active()));
        map.insert(
            "accessibilityValueDescription".into(),
            ring.accessibilityValueDescription().map_or(Value::Null, |text| Value::String(text.to_string())),
        );
        map.insert("mouseDownCanMoveWindow".into(), Value::Bool(mouse_down_can_move_window));
        map.insert("focusRingMaskBounds".into(), tree::rect(focus_ring_mask_bounds));
        map.insert("activations".into(), self.activations.get().into());
        map.insert(
            "visibilityChanges".into(),
            Value::Array(self.visibility_changes.borrow().iter().map(|hidden| Value::Bool(*hidden)).collect()),
        );
        map.insert("pressResult".into(), self.press_result.map_or(Value::Null, Value::Bool));
        map.insert("morphSide".into(), double(TaskProgressRing::MORPH_SIDE));
        Value::Object(map)
    }
}
