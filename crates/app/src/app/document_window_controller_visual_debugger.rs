//! Port of `App/DocumentWindowController+VisualDebugger.swift`: host hooks
//! for the read-only Visual Debugger panel.
//!
//! The extension owns only transient panel lifetime. It does not add editor
//! commands or mutate source text; selection remains the sole navigation
//! input.
//!
//! `VisualDebuggerViewDelegate` is implemented on the controller's delegate
//! proxy and forwards to the methods below.

use std::rc::{Rc, Weak};

use dispatch2::MainThreadBound;
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, Message};
use objc2_app_kit::{
    NSAppearanceCustomization, NSColor, NSColorSpace, NSFont, NSFontAttributeName, NSForegroundColorAttributeName,
    NSParagraphStyle, NSParagraphStyleAttributeName, NSTextAlignment,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::NSNumber;
use upleft_core::NSRange;
use upleft_core::compatibility::compatibility_diagnostics::MarkdownCompatibility;
use upleft_core::compatibility::render_target::RenderTargetProfile;
use upleft_render::render_contracts::{Theme, attribute_keys};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::{ThemeObservation, ThemeStore};
use upleft_render::view::markdown_text_view::MarkdownTextView;

use crate::app::document_window_controller::{DocumentWindowController, DocumentWindowControllerDelegates};
use crate::assets::asset_doctor::AssetDoctor;
use crate::assets::asset_resolver::AssetResolutionContext;
use crate::debugging::visual_debugger_model::{
    VisualDebuggerInput, VisualDebuggerMapping, VisualDebuggerModel, VisualDebuggerStyleFacts,
};
use crate::panels::visual_debugger_view::{VisualDebuggerView, VisualDebuggerViewDelegate};

/// The extension's associated-object state (`visualDebuggerPanel` and its
/// theme observation), held by the controller as `visual_debugger_state()`.
#[derive(Default)]
pub struct VisualDebuggerState {
    /// `visualDebuggerPanel`.
    pub(crate) visual_debugger_panel: Option<Retained<VisualDebuggerView>>,
    /// The `visualDebuggerThemeKey` association; dropping it cancels the
    /// observation.
    pub(crate) theme_observation: Option<ThemeObservation>,
}

impl DocumentWindowController {
    fn visual_debugger_panel(&self) -> Option<Retained<VisualDebuggerView>> {
        self.visual_debugger_state().borrow().visual_debugger_panel.clone()
    }

    fn set_visual_debugger_panel(&self, panel: Option<Retained<VisualDebuggerView>>) {
        self.visual_debugger_state().borrow_mut().visual_debugger_panel = panel;
    }

    /// Replaces the stored observation; the old one is dropped (cancelled)
    /// outside the borrow.
    fn set_visual_debugger_theme_observation(&self, observation: Option<ThemeObservation>) {
        let previous = std::mem::replace(&mut self.visual_debugger_state().borrow_mut().theme_observation, observation);
        drop(previous);
    }

    /// `toggleVisualDebuggerPanel()`.
    pub fn toggle_visual_debugger_panel(&self) {
        if let Some(panel) = self.visual_debugger_panel() {
            self.dismiss_trailing(&panel);
            self.set_visual_debugger_panel(None);
            self.set_visual_debugger_theme_observation(None);
            return;
        }

        let mtm = MainThreadMarker::from(self);
        let panel = VisualDebuggerView::new(self.active_style_sheet(), mtm);
        let delegates = self.delegates();
        panel.set_delegate(Some(Rc::downgrade(&delegates) as Weak<dyn VisualDebuggerViewDelegate>));
        self.set_visual_debugger_panel(Some(panel.clone()));
        self.configure_visual_debugger(&panel);
        self.install_trailing(&panel, None);

        let captured = MainThreadBound::new((ObjcWeak::from(self), ObjcWeak::from(&*panel)), mtm);
        let observation = ThemeStore::shared().observe(move |theme: &Theme| {
            let Some(mtm) = MainThreadMarker::new() else { return };
            let (weak_self, weak_panel): &(ObjcWeak<DocumentWindowController>, ObjcWeak<VisualDebuggerView>) =
                captured.get(mtm);
            let (Some(this), Some(panel)) = (weak_self.load(), weak_panel.load()) else { return };
            let Some(window) = this.window() else { return };
            panel.set_style_sheet(Rc::new(StyleSheet::new(theme.clone(), &window.effectiveAppearance(), None)));
        });
        self.set_visual_debugger_theme_observation(Some(observation));
    }

    /// `configureVisualDebugger(_:)`.
    pub fn configure_visual_debugger(&self, panel: &VisualDebuggerView) {
        let text_view = self.container_text_view();
        let selection = text_view.source_selected_range();
        let source_offset = selection.location;
        // NSTextView exposes the active TextKit selection. The source range
        // above comes from MarkdownTextView's public source-coordinate API;
        // keeping both values here makes the coordinate boundary explicit.
        let text_kit = text_view.selectedRange();
        let text_kit_range = NSRange::new(text_kit.location as isize, text_kit.length as isize);
        let text_kit_offset = text_kit_range.location;
        let attributes = Self::style_facts(source_offset, &text_view);
        let context = AssetResolutionContext::new(
            self.markdown_document().url(),
            self.markdown_document().url().map(|url| url.deleting_last_path_component()),
        );
        let report =
            MarkdownCompatibility::diagnose(&self.markdown_document().parsed(), &RenderTargetProfile::git_hub());
        let input = VisualDebuggerInput::with(
            self.markdown_document().parsed(),
            selection,
            text_view.mode(),
            attributes,
            Some(VisualDebuggerMapping::with(
                selection,
                text_kit_range,
                Some(source_offset),
                Some(text_kit_offset),
                text_view.source_selected_ranges().first() == Some(&selection),
                Vec::new(),
            )),
            Some(report),
            AssetDoctor::diagnose(&self.markdown_document().parsed(), &context, Some(&self.local_asset_probe())),
        );
        panel.set_model(VisualDebuggerModel::new(&input));
    }

    /// `refreshVisualDebuggerIfVisible()`.
    pub fn refresh_visual_debugger_if_visible(&self) {
        let Some(visual_debugger_panel) = self.visual_debugger_panel() else { return };
        self.configure_visual_debugger(&visual_debugger_panel);
    }

    /// `visualDebugger(_:didCopy:)`: copy is complete in the panel. This
    /// hook is intentionally empty so callers can observe the action without
    /// changing document state.
    pub fn visual_debugger_did_copy(&self, _view: &VisualDebuggerView, _summary: &str) {}

    /// `styleFacts(atSourceOffset:in:)`.
    fn style_facts(source_offset: isize, text_view: &MarkdownTextView) -> VisualDebuggerStyleFacts {
        // SAFETY: the text view's own storage.
        let storage = unsafe { text_view.textStorage() };
        let length = storage.as_ref().map_or(0, |storage| storage.length() as isize);
        let index = 0.max(source_offset.min(0.max(length) - 1));
        let style_sheet = text_view.style_sheet();
        let Some(storage) = storage.filter(|storage| storage.length() > 0) else {
            return VisualDebuggerStyleFacts::new(
                style_sheet.body_font().familyName().map_or_else(|| "System".to_owned(), |name| name.to_string()),
                style_sheet.body_font().pointSize(),
                Self::color_description(&style_sheet.text),
                "left",
                style_sheet.line_height,
                0.0,
                Vec::new(),
            );
        };

        // SAFETY: `index` is inside the storage; no effective range is asked.
        let attributes = unsafe { storage.attributesAtIndex_effectiveRange(index as usize, std::ptr::null_mut()) };
        // SAFETY: AppKit's immutable attribute-name constants.
        let (font_key, paragraph_key, color_key) =
            unsafe { (NSFontAttributeName, NSParagraphStyleAttributeName, NSForegroundColorAttributeName) };
        let font: Retained<NSFont> = attributes
            .objectForKey(font_key)
            .and_then(|value| value.downcast::<NSFont>().ok())
            .unwrap_or_else(|| style_sheet.body_font());
        let paragraph: Option<Retained<NSParagraphStyle>> =
            attributes.objectForKey(paragraph_key).and_then(|value| value.downcast::<NSParagraphStyle>().ok());
        let mut facts: Vec<String> = Vec::new();
        if attributes.objectForKey(attribute_keys::dr_hidden()).and_then(|value| swift_bool(&value)) == Some(true) {
            facts.push("hidden marker".to_owned());
        }
        if attributes.objectForKey(attribute_keys::dr_marker()).and_then(|value| swift_bool(&value)) == Some(true) {
            facts.push("syntax marker".to_owned());
        }
        if attributes.objectForKey(attribute_keys::dr_fragment()).is_some() {
            facts.push("render fragment".to_owned());
        }
        if attributes.objectForKey(attribute_keys::dr_link()).is_some() {
            facts.push("link".to_owned());
        }
        if attributes.objectForKey(attribute_keys::dr_path_token()).is_some() {
            facts.push("path token".to_owned());
        }
        let color: Retained<NSColor> = attributes
            .objectForKey(color_key)
            .and_then(|value| value.downcast::<NSColor>().ok())
            .unwrap_or_else(|| style_sheet.text.clone());
        VisualDebuggerStyleFacts::new(
            font.familyName().map_or_else(|| "System".to_owned(), |name| name.to_string()),
            font.pointSize(),
            Self::color_description(&color),
            Self::alignment_name(paragraph.as_ref().map_or(NSTextAlignment::Left, |paragraph| paragraph.alignment())),
            paragraph.as_ref().map_or(style_sheet.line_height, |paragraph| paragraph.minimumLineHeight()),
            paragraph.as_ref().map_or(0.0, |paragraph| paragraph.lineSpacing()),
            facts,
        )
    }

    /// `colorDescription(_:)`: `#RRGGBBAA` in sRGB.
    fn color_description(color: &NSColor) -> String {
        let converted = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()).unwrap_or_else(|| color.retain());
        let (mut red, mut green, mut blue, mut alpha): (CGFloat, CGFloat, CGFloat, CGFloat) = (0.0, 0.0, 0.0, 0.0);
        // SAFETY: four valid out-pointers.
        unsafe { converted.getRed_green_blue_alpha(&mut red, &mut green, &mut blue, &mut alpha) };
        // `String(format: "%02X", Int(x * 255))`: `Int` truncates, and `%X`
        // reads the low 32 bits of the 64-bit argument as unsigned.
        let hex = |value: CGFloat| format!("{:02X}", ((value * 255.0) as i64) as u32);
        format!("#{}{}{}{}", hex(red), hex(green), hex(blue), hex(alpha))
    }

    /// `alignmentName(_:)`.
    fn alignment_name(alignment: NSTextAlignment) -> &'static str {
        match alignment {
            NSTextAlignment::Right => "right",
            NSTextAlignment::Center => "center",
            NSTextAlignment::Justified => "justified",
            NSTextAlignment::Natural => "natural",
            _ => "left",
        }
    }
}

/// `value as? Bool`: an `NSNumber` that is exactly 0 or 1.
fn swift_bool(value: &AnyObject) -> Option<bool> {
    let number = value.downcast_ref::<NSNumber>()?;
    let double = number.doubleValue();
    if double == 1.0 {
        Some(true)
    } else if double == 0.0 {
        Some(false)
    } else {
        None
    }
}

// MARK: - VisualDebuggerViewDelegate

impl VisualDebuggerViewDelegate for DocumentWindowControllerDelegates {
    fn visual_debugger_did_copy(&self, view: &VisualDebuggerView, summary: &str) {
        if let Some(controller) = self.controller() {
            controller.visual_debugger_did_copy(view, summary);
        }
    }
}
