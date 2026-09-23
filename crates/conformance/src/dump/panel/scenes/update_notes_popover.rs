//! `UpdateNotesPopover` scenes (`Scenes/UpdateNotesPopoverScene.swift`):
//! the hover panel for one update.
//!
//! `present(from:)` refuses an anchor whose window is on no screen, which
//! the off-screen harness host always is, so the scene calls the private
//! initialiser (`UpdateNotesPopover::new`) with an anchor in a borderless
//! host window created at (-30000, -30000) and never ordered in. The
//! captured window is the popover's own borderless `NSPanel`; `after_show`
//! runs what `show()` does to the surface but neither attaches it as a child
//! window nor starts the pointer poll, whose first tick would dismiss the
//! panel in an app that is never active.
//!
//! The surface takes the harness's style sheet (Reduce Motion forced on), so
//! `present()` snaps the pour to fully revealed with no spring.

use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAccessibility, NSBackingStoreType, NSTextField, NSTextView, NSView, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use serde_json::{Map, Value};
use upleft_app::panels::appkit_support::{downcast, object};
use upleft_app::panels::update_notes_popover::{UpdateNotesContentView, UpdateNotesPopover, UpdateNotesSummary};
use upleft_app::updater::update_metadata::UpdateMetadata;
use upleft_render::theme::style_sheet::StyleSheet;

use super::update_status_pill::metadata;
use crate::dump::Failure;
use crate::dump::json::double;
use crate::dump::panel::{PanelScenario, PanelScene, tree};

#[derive(Default)]
pub struct UpdateNotesPopoverScene {
    popover: Option<Rc<UpdateNotesPopover>>,
    host: Option<Retained<NSWindow>>,
    metadata: Option<UpdateMetadata>,
}

impl PanelScene for UpdateNotesPopoverScene {
    fn build(
        &mut self,
        scenario: &PanelScenario,
        style_sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Result<Retained<NSView>, Failure> {
        let metadata = metadata(&scenario.object("metadata"));
        self.metadata = Some(metadata.clone());
        let host = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                NSRect::new(NSPoint::new(-30000.0, -30000.0), NSSize::new(480.0, 44.0)),
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { host.setReleasedWhenClosed(false) };
        self.host = Some(host.clone());
        let anchor = NSView::initWithFrame(NSView::alloc(mtm), NSRect::new(NSPoint::new(340.0, 9.0), NSSize::new(128.0, 26.0)));
        if let Some(content) = host.contentView() {
            content.addSubview(&anchor);
        }
        let popover = UpdateNotesPopover::new(&anchor, &host, &metadata, scenario.bool("isReady"), style_sheet, mtm);
        self.popover = Some(popover.clone());
        Ok(popover.panel().contentView().expect("the panel's root view"))
    }

    fn own_window(&mut self, _panel: &NSView) -> Option<Retained<NSWindow>> {
        self.popover.as_ref().map(|popover| Retained::into_super(popover.panel()))
    }

    fn after_show(&mut self, _window: &NSWindow, _scenario: &PanelScenario) {
        if let Some(popover) = &self.popover {
            popover.surface().refresh_glass_after_window_attach();
            popover.surface().present();
        }
    }

    fn model(&self) -> Value {
        let (Some(popover), Some(metadata)) = (&self.popover, &self.metadata) else { return Value::Null };
        let summary = UpdateNotesSummary::summary(metadata.item_description.as_deref());
        let mut drops = Vec::new();
        let mut text = summary.clone();
        while drops.len() < 20 {
            let Some(shorter) = UpdateNotesSummary::dropping_last_line(&text) else { break };
            drops.push(Value::String(shorter.clone()));
            text = shorter;
        }
        let surface = popover.surface();
        let content = surface
            .glass()
            .content_view()
            .subviews()
            .firstObject()
            .and_then(|first| downcast::<UpdateNotesContentView>(object(&*first)));
        let document = content.as_ref().and_then(|content| content.notes_scroll().documentView());
        let notes_text = match &document {
            Some(document) => {
                if let Some(text_view) = downcast::<NSTextView>(object(&**document)) {
                    Value::String(text_view.string().to_string())
                } else if let Some(field) = downcast::<NSTextField>(object(&**document)) {
                    Value::String(field.stringValue().to_string())
                } else {
                    Value::Null
                }
            }
            None => Value::Null,
        };
        let panel = popover.panel();
        let mut map = Map::new();
        map.insert("summary".into(), Value::String(summary));
        map.insert("drops".into(), Value::Array(drops));
        map.insert("windowFrame".into(), tree::rect(panel.frame()));
        map.insert("surfaceFrame".into(), tree::rect(surface.frame()));
        map.insert("bodyWindowRect".into(), tree::rect(popover.body_window_rect()));
        map.insert(
            "notesHeight".into(),
            content.as_ref().map_or(Value::Null, |content| double(content.notes_height_constant())),
        );
        map.insert("notesDocumentFrame".into(), document.as_ref().map_or(Value::Null, |document| tree::rect(document.frame())));
        map.insert("notesText".into(), notes_text);
        map.insert("revealValue".into(), double(surface.reveal_value()));
        map.insert("surfaceAlpha".into(), double(surface.alphaValue()));
        map.insert("reducesMotion".into(), Value::Bool(surface.reduces_motion()));
        map.insert(
            "panelLabel".into(),
            panel.accessibilityLabel().map_or(Value::Null, |label| Value::String(label.to_string())),
        );
        map.insert(
            "panelRole".into(),
            panel.accessibilityRole().map_or(Value::Null, |role| Value::String(role.to_string())),
        );
        Value::Object(map)
    }
}
