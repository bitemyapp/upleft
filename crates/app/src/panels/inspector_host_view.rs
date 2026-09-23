//! Port of `Panels/InspectorHostView.swift`: owns the one trailing inspector
//! surface. The toolbar chooses the section; this view owns only its header,
//! close affordance, and content lifecycle.
//!
//! Swift casts the installed views to `TaskPanelView` and
//! `FrontMatterEditorView`; here those are Objective-C class checks and
//! selectors (`focusForPresentation`, `fittedContentHeight`, `focusField`),
//! which those panels implement.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSButton, NSLayoutConstraint, NSLayoutConstraintOrientation, NSLayoutPriorityDefaultLow, NSLineBreakMode,
    NSResponder, NSTextField, NSView,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::NSRect;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::view::style_sheet_defaults::PanelAlpha;

use super::appkit_support::{superview, activate, cg, is_kind_of, label, ns_string, role, set_help, set_label, set_role, set_value, smax};
use super::panel_chrome::{ButtonAction, PanelButton, PanelFont, PanelMetrics, PanelSegmentedControl};

/// `InspectorSection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InspectorSection {
    Tasks = 0,
    History = 1,
    Context = 2,
    Search = 3,
}

impl InspectorSection {
    pub const ALL: [InspectorSection; 4] =
        [InspectorSection::Tasks, InspectorSection::History, InspectorSection::Context, InspectorSection::Search];

    pub fn raw_value(self) -> isize {
        self as isize
    }

    pub fn title(self) -> &'static str {
        match self {
            InspectorSection::Search => "Search",
            InspectorSection::Tasks => "Tasks",
            InspectorSection::History => "History",
            InspectorSection::Context => "Document",
        }
    }
}

const SWITCHER_SECTIONS: [InspectorSection; 3] =
    [InspectorSection::Tasks, InspectorSection::History, InspectorSection::Context];

fn switcher_index(section: InspectorSection) -> Option<usize> {
    SWITCHER_SECTIONS.iter().position(|candidate| *candidate == section)
}

/// `InspectorHostView.Metrics`.
struct Metrics;

impl Metrics {
    const HORIZONTAL_INSET: CGFloat = 18.0;
    const CLOSE_TRAILING_INSET: CGFloat = 14.0;
    /// Air above the switcher and between it and the rule.
    const TOP_PADDING: CGFloat = 10.0;
    /// Air above the slim title row.
    const TITLE_TOP_PADDING: CGFloat = 14.0;
    /// Air between the slim title row and the rule under it.
    const TITLE_BOTTOM_GAP: CGFloat = 10.0;
    /// Title row: one line of `PanelFont.header` plus the gap under it.
    const TITLE_ROW_HEIGHT: CGFloat = 22.0;
}

pub struct InspectorHostViewIvars {
    on_close: RefCell<Option<Rc<dyn Fn()>>>,
    on_selection_change: RefCell<Option<Rc<dyn Fn(Option<InspectorSection>)>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    title_label: Retained<NSTextField>,
    close_button: Retained<NSButton>,
    section_control: Retained<PanelSegmentedControl>,
    content: Retained<NSView>,
    rule: Retained<NSView>,
    title_height: RefCell<Option<Retained<NSLayoutConstraint>>>,
    switcher_height: RefCell<Option<Retained<NSLayoutConstraint>>>,
    close_on_title_row: RefCell<Option<Retained<NSLayoutConstraint>>>,
    close_on_switcher_row: RefCell<Option<Retained<NSLayoutConstraint>>>,
    rule_below_title: RefCell<Option<Retained<NSLayoutConstraint>>>,
    rule_below_switcher: RefCell<Option<Retained<NSLayoutConstraint>>>,
    close_action: RefCell<Option<Retained<ButtonAction>>>,
    views: RefCell<BTreeMap<InspectorSection, Retained<NSView>>>,
    section_titles: RefCell<BTreeMap<InspectorSection, String>>,
    selected_section: Cell<Option<InspectorSection>>,
}

define_class!(
    /// `InspectorHostView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "InspectorHostView"]
    #[ivars = InspectorHostViewIvars]
    pub struct InspectorHostView;

    unsafe impl NSObjectProtocol for InspectorHostView {}

    impl InspectorHostView {
        /// Esc closes the inspector, whatever is inside it (§11.4).
        #[unsafe(method(cancelOperation:))]
        fn __cancel_operation(&self, _sender: Option<&AnyObject>) {
            self.request_close();
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn __accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeKeyView))]
        fn __can_become_key_view(&self) -> bool {
            true
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.set_style_sheet(Rc::new(StyleSheet::current(self.mtm())));
        }
    }
);

impl InspectorHostView {
    /// `init(frame:)`.
    pub fn new(frame: NSRect, mtm: MainThreadMarker) -> Retained<InspectorHostView> {
        let noop = ButtonAction::noop(mtm);
        let close_button = PanelButton::symbol(
            "xmark",
            "Close inspector",
            &noop,
            13.0,
            super::appkit_support::weight_medium(),
            true,
            mtm,
        );
        let titles: Vec<&str> = SWITCHER_SECTIONS.iter().map(|section| section.title()).collect();
        let section_control = PanelSegmentedControl::new(&titles, 0, Rc::new(StyleSheet::current(mtm)), mtm);
        let this = Self::alloc(mtm).set_ivars(InspectorHostViewIvars {
            on_close: RefCell::new(None),
            on_selection_change: RefCell::new(None),
            style_sheet: RefCell::new(Rc::new(StyleSheet::current(mtm))),
            title_label: label("", mtm),
            close_button,
            section_control,
            content: NSView::new(mtm),
            rule: NSView::new(mtm),
            title_height: RefCell::new(None),
            switcher_height: RefCell::new(None),
            close_on_title_row: RefCell::new(None),
            close_on_switcher_row: RefCell::new(None),
            rule_below_title: RefCell::new(None),
            rule_below_switcher: RefCell::new(None),
            close_action: RefCell::new(None),
            views: RefCell::new(BTreeMap::new()),
            section_titles: RefCell::new(BTreeMap::new()),
            selected_section: Cell::new(None),
        });
        let this: Retained<InspectorHostView> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.finish_init(mtm);
        this
    }

    fn finish_init(&self, mtm: MainThreadMarker) {
        let ivars = self.ivars();
        let title_label = &ivars.title_label;
        title_label.setFont(Some(&PanelFont::floating_title()));
        title_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        title_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        title_label.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );

        let weak: ObjcWeak<InspectorHostView> = ObjcWeak::from(self);
        let action = ButtonAction::new(
            move || {
                if let Some(this) = weak.load() {
                    this.request_close();
                }
            },
            mtm,
        );
        *ivars.close_action.borrow_mut() = Some(action.clone());
        let close_button = &ivars.close_button;
        unsafe {
            close_button.setTarget(Some(&action));
            close_button.setAction(Some(ButtonAction::selector()));
        }
        let weak: ObjcWeak<InspectorHostView> = ObjcWeak::from(self);
        PanelButton::set_immediate_press_handler(close_button, move || {
            if let Some(this) = weak.load() {
                this.request_close();
            }
        });
        close_button.setTranslatesAutoresizingMaskIntoConstraints(false);
        // Closing is a primary panel action, not hover-only decoration.
        close_button.setAlphaValue(1.0);

        ivars.rule.setWantsLayer(true);
        ivars.rule.setTranslatesAutoresizingMaskIntoConstraints(false);

        let weak: ObjcWeak<InspectorHostView> = ObjcWeak::from(self);
        ivars.section_control.set_on_change(Some(Rc::new(move |index| {
            let Some(section) = super::panel_chrome::element_at(&SWITCHER_SECTIONS, index).copied() else { return };
            if let Some(this) = weak.load() {
                this.select(section);
            }
        })));
        set_label(&*ivars.section_control, "Inspector sections");
        set_help(&*ivars.section_control, "Choose which inspector panel is shown");
        for index in 0..SWITCHER_SECTIONS.len() {
            ivars.section_control.set_enabled(false, index as isize);
        }
        ivars.section_control.setTranslatesAutoresizingMaskIntoConstraints(false);

        ivars.content.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(&ivars.content);
        self.addSubview(title_label);
        self.addSubview(&ivars.section_control);
        self.addSubview(close_button);
        self.addSubview(&ivars.rule);

        let title_height = title_label.heightAnchor().constraintEqualToConstant(0.0);
        let switcher_height =
            ivars.section_control.heightAnchor().constraintEqualToConstant(PanelSegmentedControl::CONTROL_HEIGHT);
        let close_on_title_row = close_button.centerYAnchor().constraintEqualToAnchor(&title_label.centerYAnchor());
        let close_on_switcher_row =
            close_button.centerYAnchor().constraintEqualToAnchor(&ivars.section_control.centerYAnchor());
        let rule_below_title =
            ivars.rule.topAnchor().constraintEqualToAnchor_constant(&title_label.bottomAnchor(), Metrics::TITLE_BOTTOM_GAP);
        let rule_below_switcher = ivars
            .rule
            .topAnchor()
            .constraintEqualToAnchor_constant(&ivars.section_control.bottomAnchor(), Metrics::TOP_PADDING);
        activate(&[
            title_label.leadingAnchor().constraintEqualToAnchor_constant(&self.leadingAnchor(), Metrics::HORIZONTAL_INSET),
            title_label.trailingAnchor().constraintLessThanOrEqualToAnchor_constant(&close_button.leadingAnchor(), -6.0),
            title_label.topAnchor().constraintEqualToAnchor_constant(&self.topAnchor(), Metrics::TITLE_TOP_PADDING),
            title_height.clone(),
            ivars
                .section_control
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&self.leadingAnchor(), Metrics::HORIZONTAL_INSET),
            ivars.section_control.topAnchor().constraintEqualToAnchor(&title_label.bottomAnchor()),
            switcher_height.clone(),
            close_button
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&self.trailingAnchor(), -Metrics::CLOSE_TRAILING_INSET),
            close_button.widthAnchor().constraintEqualToConstant(28.0),
            close_button.heightAnchor().constraintEqualToConstant(28.0),
            ivars.rule.leadingAnchor().constraintEqualToAnchor(&self.leadingAnchor()),
            ivars.rule.trailingAnchor().constraintEqualToAnchor(&self.trailingAnchor()),
            ivars.rule.heightAnchor().constraintEqualToConstant(PanelMetrics::HAIRLINE),
            ivars.content.topAnchor().constraintEqualToAnchor(&ivars.rule.bottomAnchor()),
            ivars.content.leadingAnchor().constraintEqualToAnchor(&self.leadingAnchor()),
            ivars.content.trailingAnchor().constraintEqualToAnchor(&self.trailingAnchor()),
            ivars.content.bottomAnchor().constraintEqualToAnchor(&self.bottomAnchor()),
        ]);
        *ivars.title_height.borrow_mut() = Some(title_height);
        *ivars.switcher_height.borrow_mut() = Some(switcher_height);
        *ivars.close_on_title_row.borrow_mut() = Some(close_on_title_row);
        *ivars.close_on_switcher_row.borrow_mut() = Some(close_on_switcher_row);
        *ivars.rule_below_title.borrow_mut() = Some(rule_below_title);
        *ivars.rule_below_switcher.borrow_mut() = Some(rule_below_switcher);

        set_role(self, role::group());
        set_label(self, "Inspector");
        self.apply_style();
    }

    pub fn set_on_close(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_close.borrow_mut() = handler;
    }

    pub fn set_on_selection_change(&self, handler: Option<Rc<dyn Fn(Option<InspectorSection>)>>) {
        *self.ivars().on_selection_change.borrow_mut() = handler;
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet.clone();
        self.ivars().section_control.set_style_sheet(style_sheet);
        self.apply_style();
    }

    pub fn selected_section(&self) -> Option<InspectorSection> {
        self.ivars().selected_section.get()
    }

    pub fn close_button_for_testing(&self) -> Retained<NSView> {
        Retained::into_super(Retained::into_super(self.ivars().close_button.clone()))
    }

    pub fn close_button(&self) -> Retained<NSButton> {
        self.ivars().close_button.clone()
    }

    pub fn sync_close_visibility_with_pointer(&self) {
        self.ivars().close_button.setAlphaValue(1.0);
    }

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        ivars.title_label.setTextColor(Some(&style_sheet.text_secondary));
        ivars.close_button.setContentTintColor(Some(&style_sheet.text_secondary));
        if let Some(layer) = ivars.rule.layer() {
            let alpha = if style_sheet.increase_contrast { 0.9 } else { 0.30 };
            layer.setBackgroundColor(Some(&cg(&style_sheet.rule.panel_alpha(alpha, false))));
        }
    }

    /// The same close a panel's own Done button should perform.
    pub fn request_close(&self) {
        let handler = self.ivars().on_close.borrow().clone();
        if let Some(handler) = handler {
            handler();
        }
    }

    /// The selected panel is the first key-loop stop when the shared
    /// floating body opens.
    pub fn focus_for_presentation(&self) {
        let selected = self.ivars().selected_section.get();
        let views = self.ivars().views.borrow().clone();
        match selected {
            Some(InspectorSection::Tasks) => {
                if let Some(view) = views.get(&InspectorSection::Tasks)
                    && is_kind_of(view, c"TaskPanelView")
                {
                    let _: () = unsafe { msg_send![&**view, focusForPresentation] };
                }
            }
            Some(InspectorSection::Context) => {
                if let Some(view) = views.get(&InspectorSection::Context)
                    && is_kind_of(view, c"FrontMatterEditorView")
                {
                    let _: () = unsafe { msg_send![&**view, focusField] };
                } else if let Some(window) = self.window() {
                    window.makeFirstResponder(Some(self));
                }
            }
            Some(InspectorSection::History) | Some(InspectorSection::Search) | None => {
                if let Some(window) = self.window() {
                    window.makeFirstResponder(Some(self));
                }
            }
        }
    }

    pub fn has_content(&self) -> bool {
        !self.ivars().views.borrow().is_empty()
    }

    pub fn set_content(&self, view: &NSView, section: InspectorSection) {
        let old = self.ivars().views.borrow().get(&section).cloned();
        if let Some(old) = old
            && !std::ptr::eq(Retained::as_ptr(&old), view as *const NSView)
        {
            old.removeFromSuperview();
        }
        self.ivars().views.borrow_mut().insert(section, view.retain());
        if let Some(index) = switcher_index(section) {
            self.ivars().section_control.set_enabled(true, index as isize);
        }
        self.install_if_needed(view);
        self.select(section);
    }

    pub fn select(&self, section: InspectorSection) {
        if !self.ivars().views.borrow().contains_key(&section) {
            return;
        }
        let selection_changed = self.ivars().selected_section.get() != Some(section);
        self.ivars().selected_section.set(Some(section));
        self.update_header_chrome();
        if let Some(index) = switcher_index(section) {
            self.ivars().section_control.set_selected_index(index as isize, true);
        }
        let views = self.ivars().views.borrow().clone();
        for (candidate, view) in views.iter() {
            view.setHidden(*candidate != section);
        }
        set_value(self, &format!("{} section", section.title()));
        if selection_changed {
            let handler = self.ivars().on_selection_change.borrow().clone();
            if let Some(handler) = handler {
                handler(Some(section));
            }
        }
    }

    /// How many surfaces the host is holding.
    pub fn content_count(&self) -> usize {
        self.ivars().views.borrow().len()
    }

    /// One measurement contract for every floating inspector.
    pub fn floating_fitting_height(&self) -> CGFloat {
        self.layoutSubtreeIfNeeded();
        self.ivars().content.layoutSubtreeIfNeeded();
        let selected = self.ivars().selected_section.get();
        let views = self.ivars().views.borrow().clone();
        let task_view = views.get(&InspectorSection::Tasks).filter(|view| is_kind_of(view, c"TaskPanelView"));
        let content_height: CGFloat = if let Some(task) = task_view
            && selected == Some(InspectorSection::Tasks)
        {
            unsafe { msg_send![&**task, fittedContentHeight] }
        } else if let Some(section) = selected
            && let Some(view) = views.get(&section)
        {
            let fitted = view.fittingSize().height;
            if fitted.is_finite() && fitted > 0.0 { fitted } else { view.frame().size.height }
        } else {
            0.0
        };
        smax(0.0, self.header_fitting_height() + content_height)
    }

    fn header_fitting_height(&self) -> CGFloat {
        let Some(section) = self.ivars().selected_section.get() else { return 0.0 };
        let views = self.ivars().views.borrow();
        let switcher_visible = SWITCHER_SECTIONS.iter().filter(|candidate| views.contains_key(candidate)).count() > 1;
        let title = self.ivars().section_titles.borrow().get(&section).cloned().unwrap_or_else(|| section.title().to_owned());
        let title_visible = !switcher_visible || title != section.title() || !SWITCHER_SECTIONS.contains(&section);
        let title_height = if title_visible { Metrics::TITLE_ROW_HEIGHT } else { 0.0 };
        let row_height = if switcher_visible {
            PanelSegmentedControl::CONTROL_HEIGHT + Metrics::TOP_PADDING
        } else {
            Metrics::TITLE_BOTTOM_GAP
        };
        Metrics::TITLE_TOP_PADDING + title_height + row_height + PanelMetrics::HAIRLINE
    }

    /// The surface installed for a section, if any.
    pub fn content_for(&self, section: InspectorSection) -> Option<Retained<NSView>> {
        self.ivars().views.borrow().get(&section).cloned()
    }

    /// A panel may name itself (§7.2).
    pub fn set_title(&self, title: &str, section: InspectorSection) {
        self.ivars().section_titles.borrow_mut().insert(section, title.to_owned());
        if self.ivars().selected_section.get() != Some(section) {
            return;
        }
        self.update_header_chrome();
    }

    fn update_header_chrome(&self) {
        let ivars = self.ivars();
        let constraint = |cell: &RefCell<Option<Retained<NSLayoutConstraint>>>| cell.borrow().clone().expect("constraint");
        let Some(section) = ivars.selected_section.get() else {
            ivars.title_label.setStringValue(&ns_string(""));
            ivars.title_label.setHidden(true);
            constraint(&ivars.title_height).setConstant(0.0);
            ivars.section_control.setHidden(true);
            constraint(&ivars.switcher_height).setConstant(0.0);
            ivars.close_button.setHidden(true);
            constraint(&ivars.close_on_title_row).setActive(false);
            constraint(&ivars.close_on_switcher_row).setActive(false);
            constraint(&ivars.rule_below_title).setActive(false);
            constraint(&ivars.rule_below_switcher).setActive(false);
            return;
        };
        let switcher_visible = {
            let views = ivars.views.borrow();
            SWITCHER_SECTIONS.iter().filter(|candidate| views.contains_key(candidate)).count() > 1
        };
        let title = ivars.section_titles.borrow().get(&section).cloned().unwrap_or_else(|| section.title().to_owned());
        let title_visible = !switcher_visible || title != section.title() || !SWITCHER_SECTIONS.contains(&section);

        ivars.title_label.setStringValue(&ns_string(&title));
        set_label(&*ivars.title_label, &format!("{title} inspector"));
        ivars.title_label.setHidden(!title_visible);
        constraint(&ivars.title_height).setConstant(if title_visible { Metrics::TITLE_ROW_HEIGHT } else { 0.0 });
        ivars.section_control.setHidden(!switcher_visible);
        constraint(&ivars.switcher_height).setConstant(if switcher_visible {
            PanelSegmentedControl::CONTROL_HEIGHT
        } else {
            0.0
        });
        ivars.close_button.setHidden(false);

        constraint(&ivars.close_on_switcher_row).setActive(switcher_visible);
        constraint(&ivars.close_on_title_row).setActive(title_visible && !switcher_visible);
        constraint(&ivars.rule_below_switcher).setActive(switcher_visible);
        constraint(&ivars.rule_below_title).setActive(title_visible && !switcher_visible);
    }

    pub fn remove_content(&self, section: InspectorSection) {
        let removed = self.ivars().views.borrow_mut().remove(&section);
        if let Some(removed) = removed {
            removed.removeFromSuperview();
        }
        self.ivars().section_titles.borrow_mut().remove(&section);
        if let Some(index) = switcher_index(section) {
            self.ivars().section_control.set_enabled(false, index as isize);
        }
        if self.ivars().selected_section.get() != Some(section) {
            self.update_header_chrome();
            return;
        }
        let fallback = self.ivars().views.borrow().keys().min_by_key(|section| section.raw_value()).copied();
        if let Some(fallback) = fallback {
            self.select(fallback);
        } else {
            self.ivars().selected_section.set(None);
            self.update_header_chrome();
            set_value(self, "No inspector section");
            let handler = self.ivars().on_selection_change.borrow().clone();
            if let Some(handler) = handler {
                handler(None);
            }
        }
    }

    pub fn remove_content_view(&self, view: &NSView, section: InspectorSection) {
        let matches = self
            .ivars()
            .views
            .borrow()
            .get(&section)
            .is_some_and(|installed| std::ptr::eq(Retained::as_ptr(installed), view as *const NSView));
        if !matches {
            return;
        }
        self.remove_content(section);
    }

    fn install_if_needed(&self, view: &NSView) {
        if superview(&view).is_some() {
            return;
        }
        let content = &self.ivars().content;
        view.setTranslatesAutoresizingMaskIntoConstraints(false);
        content.addSubview(view);
        activate(&[
            view.leadingAnchor().constraintEqualToAnchor(&content.leadingAnchor()),
            view.trailingAnchor().constraintEqualToAnchor(&content.trailingAnchor()),
            view.topAnchor().constraintEqualToAnchor(&content.topAnchor()),
            view.bottomAnchor().constraintEqualToAnchor(&content.bottomAnchor()),
        ]);
    }
}
