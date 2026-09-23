//! `LightboxWindow` scenes (`Scenes/LightboxWindowScene.swift`). The scene
//! owns its window; see the Swift file for what of `present(over:)` it
//! applies and why.

use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AllocAnyThread, msg_send};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType, NSImage, NSResponder, NSView, NSWindow};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use serde_json::{Map, Value};
use upleft_app::panels::appkit_support::ns_string;
use upleft_app::panels::lightbox_window::LightboxWindow;
use upleft_render::theme::style_sheet::StyleSheet;

use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, repository_root, tree};

#[derive(Default)]
pub struct LightboxWindowScene {
    window: Option<Retained<LightboxWindow>>,
}

/// `LightboxWindowScene.keyEvent(_:)`.
pub fn key_event(value: &Value) -> Option<Retained<NSEvent>> {
    let object = value.as_object()?;
    let characters = object.get("characters").and_then(Value::as_str).unwrap_or("");
    let key_code = object.get("keyCode").and_then(Value::as_i64).unwrap_or(0) as u16;
    let characters = ns_string(characters);
    NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        NSEventType::KeyDown,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags::empty(),
        0.0,
        0,
        None,
        &characters,
        &characters,
        false,
        key_code,
    )
}

impl PanelScene for LightboxWindowScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let path = repository_root().join(scenario.string_or("image", "corpus/render-images/img/photo.jpg"));
        let path = path.to_string_lossy().into_owned();
        let image = NSImage::initWithContentsOfFile(NSImage::alloc(), &ns_string(&path))
            .ok_or_else(|| Failure::Error(format!("no image at {path}")))?;
        let window = LightboxWindow::new(
            &image,
            scenario.string("caption").as_deref(),
            style_sheet.reduce_motion,
            scenario.bool("reduceTransparency"),
            mtm,
        );
        window.setFrame_display(
            NSRect::new(NSPoint::new(-30000.0, -30000.0), NSSize::new(scenario.width, scenario.height)),
            true,
        );
        if let Some(view) = window.lightbox_view() {
            view.reset_zoom(false);
        }
        window.setAlphaValue(if style_sheet.reduce_motion { 1.0 } else { 0.0 });
        for value in scenario.array("keys") {
            let Some(event) = key_event(&value) else { continue };
            if let Some(content) = window.contentView() {
                let _: () = unsafe { msg_send![&*content, keyDown: &*event] };
            }
        }
        self.window = Some(window.clone());
        window.contentView().ok_or_else(|| Failure::Error("the lightbox has no content view".into()))
    }

    fn own_window(&mut self, _panel: &NSView) -> Option<Retained<NSWindow>> {
        self.window.clone().map(Retained::into_super)
    }

    fn after_show(&mut self, window: &NSWindow, _scenario: &PanelScenario) {
        let content = window.contentView();
        window.makeFirstResponder(content.as_deref().map(|view| -> &NSResponder { view }));
    }

    fn model(&self) -> Value {
        let Some(window) = &self.window else { return Value::Null };
        let Some(view) = window.lightbox_view() else { return Value::Null };
        let accepts: bool = unsafe { msg_send![&*view, acceptsFirstResponder] };
        let can_become_key: bool = unsafe { msg_send![&**window, canBecomeKeyWindow] };
        let offset = view.offset();
        let mut map = Map::new();
        map.insert("scale".into(), double(view.scale()));
        map.insert("fitScale".into(), double(view.fit_scale()));
        map.insert("offset".into(), tree::size(offset));
        map.insert("imageRect".into(), tree::rect(view.image_rect()));
        map.insert("alpha".into(), double(window.alphaValue()));
        map.insert("canBecomeKey".into(), Value::Bool(can_become_key));
        map.insert("acceptsFirstResponder".into(), Value::Bool(accepts));
        map.insert("collectionBehavior".into(), Value::from(window.collectionBehavior().0 as i64));
        map.insert("animationBehavior".into(), Value::from(window.animationBehavior().0 as i64));
        map.insert("releasedWhenClosed".into(), Value::Bool(window.isReleasedWhenClosed()));
        Value::Object(map)
    }
}

#[allow(unused)]
fn _unused(_: &AnyObject) {}
