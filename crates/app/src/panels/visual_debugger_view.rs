//! Port of `Panels/VisualDebuggerView.swift`: read-only source, AST, render,
//! and layout facts for the current caret.  The view owns no document state
//! and never edits source text.

use std::cell::RefCell;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBezelStyle, NSBorderType, NSButton, NSControlSize, NSEvent, NSEventModifierFlags, NSFont, NSPasteboard,
    NSPasteboardTypeString, NSResponder, NSScrollView, NSTextAlignment, NSTextDelegate, NSTextField, NSTextView,
    NSTextViewDelegate, NSView,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSRect, NSSize};
use upleft_core::{NSRange, ParsedDocument};
use upleft_render::render_contracts::RenderMode;
use upleft_render::theme::style_sheet::StyleSheet;

use super::appkit_support::{activate, label, ns_string, object, role, set_label, set_role, set_value, weight_regular};
use super::panel_chrome::{PanelBackdrop, PanelFont, PanelMetrics, PanelSurface, install_backdrop, panel_title};
use crate::debugging::visual_debugger_model::{VisualDebuggerInput, VisualDebuggerModel};
use crate::support::commands::Command;

/// `VisualDebuggerViewDelegate`.
pub trait VisualDebuggerViewDelegate {
    fn visual_debugger_did_copy(&self, view: &VisualDebuggerView, summary: &str);
}

pub struct VisualDebuggerViewIvars {
    delegate: RefCell<Option<Weak<dyn VisualDebuggerViewDelegate>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    model: RefCell<VisualDebuggerModel>,
    backdrop: Retained<PanelBackdrop>,
    title_label: Retained<NSTextField>,
    location_label: Retained<NSTextField>,
    summary_view: Retained<NSTextView>,
    copy_button: Retained<NSButton>,
    /// Held as Swift holds it; only the layout reads it after `init`.
    #[allow(dead_code)]
    scroll_view: Retained<NSScrollView>,
    /// Where copies land.  Tests inject a named pasteboard so exercising the
    /// copy path never clobbers the user's real clipboard.
    pasteboard_for_testing: RefCell<Retained<NSPasteboard>>,
}

define_class!(
    /// `VisualDebuggerView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "VisualDebuggerView"]
    #[ivars = VisualDebuggerViewIvars]
    pub struct VisualDebuggerView;

    unsafe impl NSObjectProtocol for VisualDebuggerView {}

    unsafe impl NSTextDelegate for VisualDebuggerView {}

    unsafe impl NSTextViewDelegate for VisualDebuggerView {}

    impl VisualDebuggerView {
        /// `PanelSurface.preferredWidth`.
        #[unsafe(method(preferredWidth))]
        fn __preferred_width(&self) -> CGFloat {
            self.preferred_width()
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn __accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(becomeFirstResponder))]
        fn __become_first_responder(&self) -> bool {
            self.become_first_responder()
        }

        #[unsafe(method(keyDown:))]
        fn __key_down(&self, event: &NSEvent) {
            if event.modifierFlags().contains(NSEventModifierFlags::Command) && event.keyCode() == 8 {
                self.copy_summary(Some(event));
                return;
            }
            let _: () = unsafe { msg_send![super(self), keyDown: event] };
        }

        #[unsafe(method(copySummary:))]
        fn __copy_summary(&self, sender: Option<&AnyObject>) {
            self.copy_summary(sender);
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.apply_style();
        }
    }
);

impl PanelSurface for VisualDebuggerView {
    fn preferred_width(&self) -> CGFloat {
        PanelMetrics::DETAIL_WIDTH
    }
}

impl VisualDebuggerView {
    /// `VisualDebuggerView()`.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<VisualDebuggerView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<VisualDebuggerView> {
        let model = VisualDebuggerModel::new(&VisualDebuggerInput::new(
            empty_document(),
            NSRange::new(0, 0),
            RenderMode::Read,
        ));
        let title_label = label(&panel_title(Command::VisualDebugger), mtm);
        let location_label = label("", mtm);
        let summary_view = NSTextView::new(mtm);
        let copy_button =
            unsafe { NSButton::buttonWithTitle_target_action(&ns_string("Copy Summary"), None, None, mtm) };
        let scroll_view = NSScrollView::new(mtm);
        let pasteboard = NSPasteboard::generalPasteboard();
        let backdrop = PanelBackdrop::new_default(style_sheet.clone(), mtm);
        let this = Self::alloc(mtm).set_ivars(VisualDebuggerViewIvars {
            delegate: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            model: RefCell::new(model),
            backdrop: backdrop.clone(),
            title_label: title_label.clone(),
            location_label: location_label.clone(),
            summary_view: summary_view.clone(),
            copy_button: copy_button.clone(),
            scroll_view: scroll_view.clone(),
            pasteboard_for_testing: RefCell::new(pasteboard),
        });
        let this: Retained<VisualDebuggerView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };

        install_backdrop(&this, &backdrop);

        title_label.setFont(Some(&PanelFont::header()));
        title_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&title_label);

        location_label.setFont(Some(&PanelFont::secondary()));
        location_label.setAlignment(NSTextAlignment::Right);
        location_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&location_label);

        summary_view.setEditable(false);
        summary_view.setSelectable(true);
        summary_view.setRichText(false);
        summary_view.setDrawsBackground(false);
        summary_view.setTextContainerInset(NSSize::new(PanelMetrics::INSET, 8.0));
        summary_view.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(11.0, weight_regular())));
        summary_view.setDelegate(Some(ProtocolObject::from_ref(&*this)));
        set_role(&*summary_view, role::text_area());
        set_label(&*summary_view, "Visual debugger details");
        summary_view.setTranslatesAutoresizingMaskIntoConstraints(false);

        scroll_view.setDrawsBackground(false);
        scroll_view.setHasVerticalScroller(true);
        scroll_view.setAutohidesScrollers(true);
        scroll_view.setBorderType(NSBorderType::NoBorder);
        scroll_view.setDocumentView(Some(&summary_view));
        scroll_view.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&scroll_view);

        unsafe {
            copy_button.setTarget(Some(object(&*this)));
            copy_button.setAction(Some(sel!(copySummary:)));
        }
        copy_button.setBezelStyle(NSBezelStyle::Push);
        copy_button.setControlSize(NSControlSize::Small);
        set_label(&*copy_button, "Copy visual debugger summary");
        copy_button.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&copy_button);

        let content_view = scroll_view.contentView();
        activate(&[
            title_label.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), PanelMetrics::INSET),
            title_label.topAnchor().constraintEqualToAnchor_constant(&this.topAnchor(), 8.0),
            location_label
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&this.trailingAnchor(), -PanelMetrics::INSET),
            location_label.centerYAnchor().constraintEqualToAnchor(&title_label.centerYAnchor()),
            location_label
                .leadingAnchor()
                .constraintGreaterThanOrEqualToAnchor_constant(&title_label.trailingAnchor(), 8.0),
            copy_button.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -PanelMetrics::INSET),
            copy_button.topAnchor().constraintEqualToAnchor_constant(&title_label.bottomAnchor(), 6.0),
            scroll_view.leadingAnchor().constraintEqualToAnchor(&this.leadingAnchor()),
            scroll_view.trailingAnchor().constraintEqualToAnchor(&this.trailingAnchor()),
            scroll_view.topAnchor().constraintEqualToAnchor_constant(&copy_button.bottomAnchor(), 4.0),
            scroll_view.bottomAnchor().constraintEqualToAnchor(&this.bottomAnchor()),
            summary_view.widthAnchor().constraintEqualToAnchor(&content_view.widthAnchor()),
            summary_view.heightAnchor().constraintGreaterThanOrEqualToAnchor(&content_view.heightAnchor()),
        ]);

        set_role(&*this, role::group());
        set_label(&*this, "Visual Debugger");
        this.apply_style();
        this.reload();
        this
    }

    pub fn delegate(&self) -> Option<Rc<dyn VisualDebuggerViewDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn VisualDebuggerViewDelegate>>) {
        *self.ivars().delegate.borrow_mut() = delegate;
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet.clone();
        self.ivars().backdrop.set_style_sheet(style_sheet);
        self.apply_style();
    }

    pub fn model(&self) -> VisualDebuggerModel {
        self.ivars().model.borrow().clone()
    }

    pub fn set_model(&self, model: VisualDebuggerModel) {
        *self.ivars().model.borrow_mut() = model;
        self.reload();
    }

    pub fn reload(&self) {
        let ivars = self.ivars();
        let model = self.model();
        let location = format!("Line {}, column {}", model.line, model.column);
        ivars.location_label.setStringValue(&ns_string(&location));
        set_label(&*ivars.location_label, &ivars.location_label.stringValue().to_string());
        let summary = model.summary();
        ivars.summary_view.setString(&ns_string(&summary));
        ivars.summary_view.sizeToFit();
        set_value(&*ivars.summary_view, &summary);
    }

    pub fn copy_summary_for_testing(&self) {
        let button = self.ivars().copy_button.clone();
        self.copy_summary(Some(object(&*button)));
    }

    pub fn pasteboard_for_testing(&self) -> Retained<NSPasteboard> {
        self.ivars().pasteboard_for_testing.borrow().clone()
    }

    pub fn set_pasteboard_for_testing(&self, pasteboard: Retained<NSPasteboard>) {
        *self.ivars().pasteboard_for_testing.borrow_mut() = pasteboard;
    }

    pub fn summary_text_for_testing(&self) -> String {
        self.ivars().summary_view.string().to_string()
    }

    fn become_first_responder(&self) -> bool {
        let summary_view = self.ivars().summary_view.clone();
        match summary_view.window() {
            Some(window) => window.makeFirstResponder(Some(&summary_view)),
            None => unsafe { msg_send![super(self), becomeFirstResponder] },
        }
    }

    fn copy_summary(&self, _sender: Option<&AnyObject>) {
        let pasteboard = self.pasteboard_for_testing();
        let summary = self.model().summary();
        pasteboard.clearContents();
        pasteboard.setString_forType(&ns_string(&summary), unsafe { NSPasteboardTypeString });
        if let Some(delegate) = self.delegate() {
            delegate.visual_debugger_did_copy(self, &summary);
        }
    }

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        ivars.title_label.setTextColor(Some(&style_sheet.text_secondary));
        ivars.location_label.setTextColor(Some(&style_sheet.text_faint));
        ivars.summary_view.setTextColor(Some(&style_sheet.text));
        ivars.summary_view.setInsertionPointColor(Some(&style_sheet.accent));
        ivars.copy_button.setContentTintColor(Some(&style_sheet.text_secondary));
    }
}

/// `ParsedDocument.empty`.
fn empty_document() -> Arc<ParsedDocument> {
    ParsedDocument::empty()
}
