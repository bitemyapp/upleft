//! `SearchResultsPanelView` scenes (`Scenes/SearchResultsPanelViewScene.swift`). Not written yet.

use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::NSView;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::panel::{PanelScenario, PanelScene};

#[derive(Default)]
pub struct SearchResultsPanelViewScene;

impl PanelScene for SearchResultsPanelViewScene {
    fn build(
        &mut self,
        _scenario: &PanelScenario,
        _style_sheet: Rc<StyleSheet>,
        _mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        Err(Failure::NotPorted)
    }
}
