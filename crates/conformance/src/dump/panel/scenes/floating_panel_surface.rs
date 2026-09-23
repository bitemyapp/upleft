//! `FloatingPanelSurface` scenes (`Scenes/FloatingPanelSurfaceScene.swift`).

use std::cell::Cell;
use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use objc2_foundation::NSRect;
use serde_json::{Map, Value};
use upleft_app::panels::appkit_support::{RECT_ZERO, RectExt, cg, label, rect};
use upleft_app::panels::floating_panel_surface::{FloatingPanelSurface, Top};
use upleft_app::panels::inspector_host_view::{InspectorHostView, InspectorSection};
use upleft_render::theme::style_sheet::StyleSheet;

use super::panel_chrome::container;
use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
pub struct FloatingPanelSurfaceScene {
    surface: Option<Retained<FloatingPanelSurface>>,
    settled_count: Rc<Cell<i64>>,
}

impl PanelScene for FloatingPanelSurfaceScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let container = container(scenario, mtm);
        container.setWantsLayer(true);
        if let Some(layer) = container.layer() {
            layer.setBackgroundColor(Some(&cg(&style_sheet.background)));
        }
        let host = InspectorHostView::new(RECT_ZERO, mtm);
        host.set_style_sheet(style_sheet.clone());
        let content = label(&scenario.string_or("label", "Floating content"), mtm);
        content.setTextColor(Some(&style_sheet.text));
        host.set_content(&content, InspectorSection::Tasks);
        let surface = FloatingPanelSurface::new(style_sheet.clone(), &host, mtm);
        let settled = self.settled_count.clone();
        surface.set_on_frame_spring_settled(Some(Rc::new(move || settled.set(settled.get() + 1))));
        let width = scenario.double_or("panelWidth", 300.0);
        let height = scenario.double_or("panelHeight", 260.0);
        let resting = rect(scenario.width - width - 20.0, scenario.height - height - 20.0, width, height);
        let sliver = rect(resting.min_x(), resting.max_y() - Top::POUR_SLIVER_HEIGHT, width, Top::POUR_SLIVER_HEIGHT);
        container.addSubview(&surface);
        surface.set_resting_frame(resting);
        surface.configure_window_frames(resting, sliver, height);
        match scenario.string_or("presentation", "present").as_str() {
            "dismiss" => {
                surface.present_from_sliver(false);
                surface.dismiss_to_sliver(false);
            }
            "sliver" => {}
            _ => surface.present_from_sliver(false),
        }
        self.surface = Some(surface);
        Ok(container)
    }

    fn model(&self) -> Value {
        let Some(surface) = &self.surface else { return Value::Null };
        let mut map = Map::new();
        map.insert("frame".into(), tree::rect(surface.frame()));
        map.insert("usesGlass".into(), Value::Bool(surface.uses_glass()));
        map.insert("isDismissing".into(), Value::Bool(surface.is_dismissing()));
        map.insert("visibleBody".into(), tree::rect(surface.visible_body_bounds_for_hit_testing()));
        map.insert("preferredWidth".into(), double(surface.preferred_width()));
        map.insert("fittedContentHeight".into(), double(surface.fitted_content_height()));
        map.insert("contentLayoutHeight".into(), double(surface.content_layout_height_for_testing()));
        map.insert("rendersBody".into(), Value::Bool(surface.renders_body_for_testing()));
        map.insert("opaqueFallbackMounted".into(), Value::Bool(surface.opaque_fallback_is_mounted_for_testing()));
        map.insert("settledCount".into(), Value::from(self.settled_count.get()));
        Value::Object(map)
    }
}

#[allow(unused)]
fn _unused(_: NSRect) {}
