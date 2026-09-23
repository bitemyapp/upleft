//! Port of `Panels/HistoryInspectorView.swift`: compact history controls for
//! the shared inspector. Full rendered comparison remains a separate window
//! because it needs document-scale room.
//!
//! The view is its timeline's delegate in Swift; here a small proxy
//! (`TimelineDelegate`) holding a weak reference to the view implements
//! `VersionTimelineDelegate`, and the view owns it.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSButton, NSResponder, NSTextField, NSView};
use objc2_foundation::NSRect;
use upleft_foundation::date::Date;
use upleft_render::theme::style_sheet::StyleSheet;

use super::appkit_support::{activate, ns_string, object, role, set_label, set_role, wrapping_label};
use super::panel_chrome::{ButtonAction, PanelBackdrop, PanelButton, PanelFont, PanelMetrics, RelativeTime, install_backdrop};
use super::version_timeline_view::{VersionTimelineDelegate, VersionTimelineView};
use crate::ai::snapshot_store::VersionRecord;

/// `HistoryInspectorViewDelegate`.
pub trait HistoryInspectorViewDelegate {
    fn history_inspector_did_request_full_history(&self, inspector: &HistoryInspectorView);
    fn history_inspector_did_request_restore(&self, inspector: &HistoryInspectorView, record: &VersionRecord);
}

/// `extension HistoryInspectorView: VersionTimelineDelegate`.
struct TimelineDelegate {
    view: ObjcWeak<HistoryInspectorView>,
}

impl VersionTimelineDelegate for TimelineDelegate {
    fn version_timeline_did_scrub_to(&self, _view: &VersionTimelineView, _record: &VersionRecord) {
        if let Some(inspector) = self.view.load() {
            inspector.update_caption();
        }
    }

    fn version_timeline_did_request_restore(&self, _view: &VersionTimelineView, record: &VersionRecord) {
        if let Some(inspector) = self.view.load()
            && let Some(delegate) = inspector.delegate()
        {
            delegate.history_inspector_did_request_restore(&inspector, record);
        }
    }
}

pub struct HistoryInspectorViewIvars {
    delegate: RefCell<Option<Weak<dyn HistoryInspectorViewDelegate>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    versions: RefCell<Vec<VersionRecord>>,
    /// Held as Swift holds it; its sheet is never updated (see
    /// `set_style_sheet`).
    #[allow(dead_code)]
    backdrop: Retained<PanelBackdrop>,
    timeline: Retained<VersionTimelineView>,
    caption: Retained<NSTextField>,
    open_button: Retained<NSButton>,
    open_action: RefCell<Option<Retained<ButtonAction>>>,
    timeline_delegate: RefCell<Option<Rc<TimelineDelegate>>>,
}

define_class!(
    /// `HistoryInspectorView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "HistoryInspectorView"]
    #[ivars = HistoryInspectorViewIvars]
    pub struct HistoryInspectorView;

    unsafe impl NSObjectProtocol for HistoryInspectorView {}
);

impl HistoryInspectorView {
    /// `HistoryInspectorView()`: `init(styleSheet: .current)`.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<HistoryInspectorView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<HistoryInspectorView> {
        let caption = wrapping_label("", mtm);
        let backdrop = PanelBackdrop::new_default(style_sheet.clone(), mtm);
        let timeline = VersionTimelineView::new(style_sheet.clone(), mtm);
        let placeholder = ButtonAction::noop(mtm);
        let open_button = PanelButton::text("Open comparison…", &placeholder, false, mtm);
        drop(placeholder);
        let this = Self::alloc(mtm).set_ivars(HistoryInspectorViewIvars {
            delegate: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            versions: RefCell::new(Vec::new()),
            backdrop: backdrop.clone(),
            timeline: timeline.clone(),
            caption: caption.clone(),
            open_button: open_button.clone(),
            open_action: RefCell::new(None),
            timeline_delegate: RefCell::new(None),
        });
        let this: Retained<HistoryInspectorView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };

        install_backdrop(&this, &backdrop);

        let proxy = Rc::new(TimelineDelegate { view: ObjcWeak::from(&*this) });
        let weak_proxy: Weak<dyn VersionTimelineDelegate> = Rc::downgrade(&(proxy.clone() as Rc<dyn VersionTimelineDelegate>));
        *this.ivars().timeline_delegate.borrow_mut() = Some(proxy);
        timeline.set_delegate(Some(weak_proxy));
        timeline.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&timeline);

        caption.setFont(Some(&PanelFont::row()));
        caption.setMaximumNumberOfLines(0);
        caption.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&caption);

        let weak: ObjcWeak<HistoryInspectorView> = ObjcWeak::from(&*this);
        let action = ButtonAction::new(
            move || {
                let Some(this) = weak.load() else { return };
                if let Some(delegate) = this.delegate() {
                    delegate.history_inspector_did_request_full_history(&this);
                }
            },
            mtm,
        );
        *this.ivars().open_action.borrow_mut() = Some(action.clone());
        unsafe {
            open_button.setTarget(Some(object(&*action)));
            open_button.setAction(Some(ButtonAction::selector()));
        }
        open_button.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&open_button);

        activate(&[
            timeline.leadingAnchor().constraintEqualToAnchor(&this.leadingAnchor()),
            timeline.trailingAnchor().constraintEqualToAnchor(&this.trailingAnchor()),
            timeline.topAnchor().constraintEqualToAnchor_constant(&this.topAnchor(), 4.0),
            caption.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), PanelMetrics::INSET),
            caption.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -PanelMetrics::INSET),
            caption.topAnchor().constraintEqualToAnchor_constant(&timeline.bottomAnchor(), 8.0),
            open_button.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), PanelMetrics::INSET),
            open_button.topAnchor().constraintEqualToAnchor_constant(&caption.bottomAnchor(), 12.0),
        ]);

        set_role(&*this, role::group());
        set_label(&*this, "Version history");
        this.apply_style();
        this.update_caption();
        this
    }

    pub fn delegate(&self) -> Option<Rc<dyn HistoryInspectorViewDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn HistoryInspectorViewDelegate>>) {
        *self.ivars().delegate.borrow_mut() = delegate;
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    /// Swift's `didSet` hands the sheet to the timeline and restyles the
    /// caption; it never reaches the backdrop, which keeps the sheet it was
    /// built with (reproduced).
    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet.clone();
        self.ivars().timeline.set_style_sheet(style_sheet);
        self.apply_style();
    }

    pub fn versions(&self) -> Vec<VersionRecord> {
        self.ivars().versions.borrow().clone()
    }

    pub fn set_versions(&self, versions: Vec<VersionRecord>) {
        let count = versions.len() as isize;
        *self.ivars().versions.borrow_mut() = versions.clone();
        let timeline = self.ivars().timeline.clone();
        timeline.set_versions(versions);
        timeline.set_selected_index(0.max(count - 1));
        self.update_caption();
    }

    fn update_caption(&self) {
        let ivars = self.ivars();
        let Some(selected) = ivars.timeline.selected_record() else {
            ivars
                .caption
                .setStringValue(&ns_string("No saved versions yet. Upleft records local snapshots when this file changes."));
            ivars.open_button.setEnabled(false);
            return;
        };
        ivars.caption.setStringValue(&ns_string(&format!(
            "Selected {}. Open comparison to review it beside the current document.",
            RelativeTime::long(selected.date, Date::now())
        )));
        ivars.open_button.setEnabled(true);
    }

    fn apply_style(&self) {
        let color = self.style_sheet().text_secondary.clone();
        self.ivars().caption.setTextColor(Some(&color));
    }

    pub fn timeline_for_testing(&self) -> Retained<VersionTimelineView> {
        self.ivars().timeline.clone()
    }

    pub fn caption_for_testing(&self) -> Retained<NSTextField> {
        self.ivars().caption.clone()
    }

    pub fn open_button_for_testing(&self) -> Retained<NSButton> {
        self.ivars().open_button.clone()
    }
}
