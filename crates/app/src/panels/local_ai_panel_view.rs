//! Port of `Panels/LocalAIPanelView.swift`: the on-device AI panel. It asks
//! its delegate to run a task and shows the result; it never runs a model
//! itself.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSButton, NSLineBreakMode, NSPopUpButton, NSResponder, NSTextField, NSView};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSArray, NSRect, NSString};
use upleft_render::theme::style_sheet::StyleSheet;

use super::appkit_support::{activate, label, ns_string, object, role, set_label, set_role, set_value, wrapping_label};
use super::panel_chrome::{
    ButtonAction, PanelBackdrop, PanelButton, PanelFont, PanelMetrics, PanelSurface, install_backdrop, panel_title,
};
use crate::ai::local_ai::{LocalAIAvailability, LocalAIPreview, LocalAIResult, LocalAITask};
use crate::support::commands::Command;

/// `LocalAIPanelViewDelegate`.
pub trait LocalAIPanelViewDelegate {
    fn local_ai_panel_did_request(&self, panel: &LocalAIPanelView, task: LocalAITask);
    fn local_ai_panel_did_apply(&self, panel: &LocalAIPanelView, preview: &LocalAIPreview);
    fn local_ai_panel_did_cancel(&self, panel: &LocalAIPanelView);
}

pub struct LocalAIPanelViewIvars {
    delegate: RefCell<Option<Weak<dyn LocalAIPanelViewDelegate>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    availability: Cell<LocalAIAvailability>,
    result: RefCell<Option<LocalAIResult>>,
    is_running: Cell<bool>,
    backdrop: Retained<PanelBackdrop>,
    title_label: Retained<NSTextField>,
    task_popup: Retained<NSPopUpButton>,
    status_label: Retained<NSTextField>,
    preview_label: Retained<NSTextField>,
    actions: RefCell<Vec<Retained<ButtonAction>>>,
}

define_class!(
    /// `LocalAIPanelView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "LocalAIPanelView"]
    #[ivars = LocalAIPanelViewIvars]
    pub struct LocalAIPanelView;

    unsafe impl NSObjectProtocol for LocalAIPanelView {}

    impl LocalAIPanelView {
        /// `PanelSurface.preferredWidth`.
        #[unsafe(method(preferredWidth))]
        fn __preferred_width(&self) -> CGFloat {
            self.preferred_width()
        }

        #[unsafe(method(runRequested:))]
        fn __run_requested(&self, sender: &NSPopUpButton) {
            self.run_requested(sender);
        }
    }
);

impl PanelSurface for LocalAIPanelView {
    fn preferred_width(&self) -> CGFloat {
        PanelMetrics::DETAIL_WIDTH
    }
}

impl LocalAIPanelView {
    /// `LocalAIPanelView()`.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<LocalAIPanelView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<LocalAIPanelView> {
        let title_label = label(&panel_title(Command::LocalAi), mtm);
        let task_popup = NSPopUpButton::new(mtm);
        let status_label = wrapping_label("", mtm);
        let preview_label = wrapping_label("", mtm);
        let backdrop = PanelBackdrop::new_default(style_sheet.clone(), mtm);
        let this = Self::alloc(mtm).set_ivars(LocalAIPanelViewIvars {
            delegate: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            availability: Cell::new(LocalAIAvailability::SystemUnavailable),
            result: RefCell::new(None),
            is_running: Cell::new(false),
            backdrop: backdrop.clone(),
            title_label,
            task_popup,
            status_label,
            preview_label,
            actions: RefCell::new(Vec::new()),
        });
        let this: Retained<LocalAIPanelView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        install_backdrop(&this, &backdrop);
        this.build_interface(mtm);
        this.apply_style();
        this.update_status();
        this
    }

    fn build_interface(&self, mtm: MainThreadMarker) {
        let ivars = self.ivars();
        let title_label = &ivars.title_label;
        let task_popup = &ivars.task_popup;
        let status_label = &ivars.status_label;
        let preview_label = &ivars.preview_label;

        title_label.setFont(Some(&PanelFont::header()));
        let titles: Vec<Retained<NSString>> = LocalAITask::ALL_CASES.iter().map(|task| ns_string(task.title())).collect();
        task_popup.addItemsWithTitles(&NSArray::from_retained_slice(&titles));
        unsafe {
            task_popup.setTarget(Some(object(self)));
            task_popup.setAction(Some(sel!(runRequested:)));
        }
        set_label(&**task_popup, "Local AI task");

        status_label.setFont(Some(&PanelFont::secondary()));
        status_label.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        status_label.setMaximumNumberOfLines(3);
        preview_label.setFont(Some(&PanelFont::row()));
        preview_label.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        preview_label.setMaximumNumberOfLines(12);
        set_label(&**preview_label, "Local AI preview");

        let weak: ObjcWeak<LocalAIPanelView> = ObjcWeak::from(self);
        let apply_action = ButtonAction::new(
            move || {
                if let Some(this) = weak.load() {
                    this.apply_requested();
                }
            },
            mtm,
        );
        let weak: ObjcWeak<LocalAIPanelView> = ObjcWeak::from(self);
        let cancel_action = ButtonAction::new(
            move || {
                let Some(this) = weak.load() else { return };
                if let Some(delegate) = this.delegate() {
                    delegate.local_ai_panel_did_cancel(&this);
                }
            },
            mtm,
        );
        *ivars.actions.borrow_mut() = vec![apply_action.clone(), cancel_action.clone()];
        let apply = PanelButton::text("Apply Preview", &apply_action, false, mtm);
        let cancel = PanelButton::text("Close", &cancel_action, false, mtm);

        let views: [&NSView; 6] = [title_label, task_popup, status_label, preview_label, &apply, &cancel];
        for view in views {
            view.setTranslatesAutoresizingMaskIntoConstraints(false);
            self.addSubview(view);
        }
        activate(&[
            title_label.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), PanelMetrics::INSET),
            title_label
                .topAnchor()
                .constraintEqualToAnchor_constant(&self.topAnchor(), PanelMetrics::HEADER_TOP_PADDING),
            task_popup.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            task_popup.centerYAnchor().constraintEqualToAnchor(&title_label.centerYAnchor()),
            task_popup.widthAnchor().constraintEqualToConstant(150.0),
            status_label.leadingAnchor().constraintEqualToAnchor(&title_label.leadingAnchor()),
            status_label.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            status_label.topAnchor().constraintEqualToAnchor_constant(&title_label.bottomAnchor(), 12.0),
            preview_label.leadingAnchor().constraintEqualToAnchor(&title_label.leadingAnchor()),
            preview_label.trailingAnchor().constraintEqualToAnchor_constant(&self.trailingAnchor(), -PanelMetrics::INSET),
            preview_label.topAnchor().constraintEqualToAnchor_constant(&status_label.bottomAnchor(), 10.0),
            cancel.leadingAnchor().constraintEqualToAnchor(&title_label.leadingAnchor()),
            apply.topAnchor().constraintEqualToAnchor_constant(&preview_label.bottomAnchor(), 12.0),
            apply.leadingAnchor().constraintEqualToAnchor_constant(&cancel.trailingAnchor(), 6.0),
            cancel.centerYAnchor().constraintEqualToAnchor(&apply.centerYAnchor()),
        ]);
        set_role(self, role::group());
        set_label(self, "Local on-device AI");
    }

    pub fn delegate(&self) -> Option<Rc<dyn LocalAIPanelViewDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn LocalAIPanelViewDelegate>>) {
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

    pub fn availability(&self) -> LocalAIAvailability {
        self.ivars().availability.get()
    }

    pub fn set_availability(&self, availability: LocalAIAvailability) {
        self.ivars().availability.set(availability);
        self.update_status();
    }

    pub fn result(&self) -> Option<LocalAIResult> {
        self.ivars().result.borrow().clone()
    }

    pub fn set_result(&self, result: Option<LocalAIResult>) {
        *self.ivars().result.borrow_mut() = result;
        self.update_result();
    }

    pub fn is_running(&self) -> bool {
        self.ivars().is_running.get()
    }

    pub fn set_is_running(&self, is_running: bool) {
        self.ivars().is_running.set(is_running);
        self.update_status();
    }

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        ivars.title_label.setTextColor(Some(&style_sheet.text_secondary));
        ivars.status_label.setTextColor(Some(&style_sheet.text_faint));
        ivars.preview_label.setTextColor(Some(&style_sheet.text));
    }

    fn update_status(&self) {
        let status_label = &self.ivars().status_label;
        let text = if self.is_running() {
            "Working on this Mac…"
        } else {
            match self.availability() {
                LocalAIAvailability::Available => "On-device only. Nothing is sent to a server.",
                LocalAIAvailability::FrameworkUnavailable => "Apple on-device AI is not installed.",
                LocalAIAvailability::SystemUnavailable => "This Mac does not provide Apple on-device AI.",
            }
        };
        status_label.setStringValue(&ns_string(text));
        set_label(&**status_label, &status_label.stringValue().to_string());
    }

    fn update_result(&self) {
        let preview_label = &self.ivars().preview_label;
        let Some(result) = self.result() else {
            preview_label.setStringValue(&ns_string("Select a task to preview a result."));
            return;
        };
        if let Some(preview) = &result.preview {
            preview_label.setStringValue(&ns_string(&format!(
                "Original:\n{}\n\nProposed:\n{}",
                preview.original_source, preview.proposed_source
            )));
        } else {
            preview_label.setStringValue(&ns_string(&result.text));
        }
        set_value(&**preview_label, &preview_label.stringValue().to_string());
    }

    fn run_requested(&self, sender: &NSPopUpButton) {
        let index = sender.indexOfSelectedItem();
        let Some(task) = (if index >= 0 { LocalAITask::ALL_CASES.get(index as usize).copied() } else { None }) else {
            return;
        };
        if let Some(delegate) = self.delegate() {
            delegate.local_ai_panel_did_request(self, task);
        }
    }

    fn apply_requested(&self) {
        let Some(preview) = self.result().and_then(|result| result.preview) else { return };
        if let Some(delegate) = self.delegate() {
            delegate.local_ai_panel_did_apply(self, &preview);
        }
    }

    /// The task menu (tests and the conformance scene pick an item and send
    /// its action, as a click does).
    pub fn task_popup_for_testing(&self) -> Retained<NSPopUpButton> {
        self.ivars().task_popup.clone()
    }

    /// The Apply Preview and Close targets, in that order.
    pub fn actions_for_testing(&self) -> Vec<Retained<ButtonAction>> {
        self.ivars().actions.borrow().clone()
    }
}

#[allow(unused)]
fn _unused(_: &AnyObject, _: &NSButton) {}
