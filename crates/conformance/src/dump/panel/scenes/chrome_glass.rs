//! `ChromeGlass` scenes (`Scenes/ChromeGlassScene.swift`).

use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use objc2_foundation::NSRect;
use serde_json::{Map, Value};
use upleft_app::panels::appkit_support::{RectExt, cg, label, rect};
use upleft_app::panels::chrome_glass::{ChromeGlass, RoundedCorners, Tint};
use upleft_app::panels::panel_chrome::PanelMetrics;
use upleft_render::theme::style_sheet::StyleSheet;

use super::panel_chrome::{container, frame_or};
use crate::dump::Failure;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
pub struct ChromeGlassScene {
    glass: Option<Retained<ChromeGlass>>,
}

impl PanelScene for ChromeGlassScene {
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
        let tint = match scenario.string_or("tint", "panel").as_str() {
            "band" => Tint::Band,
            "control" => Tint::Control,
            _ => Tint::Panel,
        };
        let glass = ChromeGlass::new(
            style_sheet.clone(),
            scenario.double_or("cornerRadius", PanelMetrics::SURFACE_RADIUS),
            if scenario.string_or("corners", "all") == "bottomOnly" { RoundedCorners::BottomOnly } else { RoundedCorners::All },
            tint,
            mtm,
        );
        if scenario.bool("showsFocus") {
            glass.set_shows_focus(true);
        }
        if let Some(opacity) = scenario.double("shadowOpacity") {
            glass.set_shadow_opacity(Some(opacity as f32));
        }
        let label = label(&scenario.string_or("label", "Glass"), mtm);
        label.setFrame(rect(12.0, 8.0, 160.0, 18.0));
        glass.content_view().addSubview(&label);
        glass.setFrame(frame_or(scenario, container.bounds().inset_by(20.0, 20.0)));
        container.addSubview(&glass);
        self.glass = Some(glass);
        Ok(container)
    }

    fn model(&self) -> Value {
        let Some(glass) = &self.glass else { return Value::Null };
        let style_sheet = glass.style_sheet();
        let mut map = Map::new();
        map.insert("usesGlass".into(), Value::Bool(glass.uses_glass()));
        map.insert("rendersOpaqueFallback".into(), Value::Bool(glass.renders_opaque_fallback_for_testing()));
        map.insert("isDarkBackground".into(), Value::Bool(ChromeGlass::is_dark_background(&style_sheet.background)));
        map.insert("glassTint".into(), tree::color(Some(ChromeGlass::glass_tint(&style_sheet, glass.tint())), glass));
        map.insert(
            "opaqueFallback".into(),
            tree::color(Some(ChromeGlass::opaque_fallback_color(&style_sheet, glass.tint())), glass),
        );
        Value::Object(map)
    }
}

#[allow(unused)]
fn _unused(_: NSRect) {}
