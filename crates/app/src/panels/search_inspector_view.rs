//! Port of `Panels/SearchInspectorView.swift`: one search surface for
//! document find and optional cross-file results. The controller owns the
//! search session; this view owns only composition.

use std::cell::RefCell;
use std::rc::Rc;

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{NSLayoutConstraint, NSResponder, NSTextField, NSView};
use objc2_core_foundation::CGFloat;
use objc2_foundation::NSRect;
use upleft_render::motion::{self, Curve};
use upleft_render::theme::style_sheet::StyleSheet;

use super::appkit_support::{activate, is_same_view, role, set_label, set_role, superview, wrapping_label};
use super::find_bar_view::{FindBarView, Presentation};
use super::panel_chrome::{PanelBackdrop, PanelFont, PanelMetrics, install_backdrop};

/// `SearchInspectorView.Metrics`: the find bar is exactly its rows plus air.
struct Metrics;

impl Metrics {
    /// 8 top + one 28pt field row + 8 bottom above the hairline.
    const FIND_BAR_HEIGHT: CGFloat = 52.0;
    /// Adds 8 spacing + a 24pt replace row.
    const REPLACE_BAR_HEIGHT: CGFloat = 88.0;
}

pub struct SearchInspectorViewIvars {
    find_bar: Retained<FindBarView>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    backdrop: Retained<PanelBackdrop>,
    guidance: Retained<NSTextField>,
    result_host: Retained<NSView>,
    find_height: RefCell<Option<Retained<NSLayoutConstraint>>>,
    results: RefCell<ObjcWeak<NSView>>,
}

define_class!(
    /// `SearchInspectorView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "SearchInspectorView"]
    #[ivars = SearchInspectorViewIvars]
    pub struct SearchInspectorView;

    unsafe impl NSObjectProtocol for SearchInspectorView {}
);

impl SearchInspectorView {
    /// `init(styleSheet:)`; Swift's default is `.current`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<SearchInspectorView> {
        // Stored-property initial values, in declaration order.
        let guidance = wrapping_label(
            "Matches are highlighted in the document. Press Return for the next match and Shift-Return for the previous match.",
            mtm,
        );
        let result_host = NSView::new(mtm);
        let backdrop = PanelBackdrop::new_default(style_sheet.clone(), mtm);
        let find_bar = FindBarView::new(style_sheet.clone(), Presentation::Inspector, mtm);

        let this = Self::alloc(mtm).set_ivars(SearchInspectorViewIvars {
            find_bar: find_bar.clone(),
            style_sheet: RefCell::new(style_sheet.clone()),
            backdrop: backdrop.clone(),
            guidance: guidance.clone(),
            result_host: result_host.clone(),
            find_height: RefCell::new(None),
            results: RefCell::new(ObjcWeak::default()),
        });
        let this: Retained<SearchInspectorView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };

        install_backdrop(&this, &backdrop);

        find_bar.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&find_bar);

        guidance.setFont(Some(&PanelFont::secondary()));
        guidance.setTextColor(Some(&style_sheet.text_faint));
        guidance.setMaximumNumberOfLines(0);
        guidance.setTranslatesAutoresizingMaskIntoConstraints(false);
        result_host.addSubview(&guidance);

        result_host.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&result_host);

        let find_height = find_bar.heightAnchor().constraintEqualToConstant(Metrics::FIND_BAR_HEIGHT);
        *this.ivars().find_height.borrow_mut() = Some(find_height.clone());
        activate(&[
            find_bar.leadingAnchor().constraintEqualToAnchor(&this.leadingAnchor()),
            find_bar.trailingAnchor().constraintEqualToAnchor(&this.trailingAnchor()),
            find_bar.topAnchor().constraintEqualToAnchor(&this.topAnchor()),
            find_height,
            result_host.leadingAnchor().constraintEqualToAnchor(&this.leadingAnchor()),
            result_host.trailingAnchor().constraintEqualToAnchor(&this.trailingAnchor()),
            result_host.topAnchor().constraintEqualToAnchor(&find_bar.bottomAnchor()),
            result_host.bottomAnchor().constraintEqualToAnchor(&this.bottomAnchor()),
            guidance
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&result_host.leadingAnchor(), PanelMetrics::INSET),
            guidance
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&result_host.trailingAnchor(), -PanelMetrics::INSET),
            guidance.topAnchor().constraintEqualToAnchor_constant(&result_host.topAnchor(), 12.0),
        ]);

        set_role(&*this, role::group());
        set_label(&*this, "Search");
        this
    }

    /// `SearchInspectorView()` with the `.current` style sheet.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<SearchInspectorView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `findBar`.
    pub fn find_bar(&self) -> Retained<FindBarView> {
        self.ivars().find_bar.clone()
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet.clone();
        self.ivars().backdrop.set_style_sheet(style_sheet.clone());
        self.ivars().find_bar.set_style_sheet(style_sheet.clone());
        self.ivars().guidance.setTextColor(Some(&style_sheet.text_faint));
    }

    pub fn shows_replace(&self) -> bool {
        self.ivars().find_bar.shows_replace()
    }

    pub fn set_shows_replace(&self, new_value: bool) {
        self.ivars().find_bar.set_shows_replace(new_value);
        let target = if new_value { Metrics::REPLACE_BAR_HEIGHT } else { Metrics::FIND_BAR_HEIGHT };
        let find_height = self.ivars().find_height.borrow().clone().expect("findHeight is set in init");
        if find_height.constant() == target {
            return;
        }
        // The bar's row content fades in through the find bar itself; the
        // container's height follows with the same glide.
        if !(self.window().is_some() && !self.style_sheet().reduce_motion) {
            find_height.setConstant(target);
            return;
        }
        let this = self.retain();
        motion::run(
            false,
            motion::STANDARD,
            Curve::Structural,
            move |_| {
                let animator = objc2_app_kit::NSAnimatablePropertyContainer::animator(&*find_height);
                let _: () = unsafe { msg_send![&*animator, setConstant: target] };
                this.layoutSubtreeIfNeeded();
            },
            None,
        );
    }

    /// `setResults(_:)`.
    pub fn set_results(&self, view: Option<&NSView>) {
        let ivars = self.ivars();
        let current = ivars.results.borrow().load();
        if let Some(view) = view
            && current.as_deref().is_some_and(|current| std::ptr::eq(current, view))
            && is_same_view(superview(view), &ivars.result_host)
        {
            ivars.guidance.setHidden(true);
            return;
        }
        if let Some(current) = current {
            current.removeFromSuperview();
        }
        *ivars.results.borrow_mut() = view.map(ObjcWeak::from).unwrap_or_default();
        ivars.guidance.setHidden(view.is_some());
        let Some(view) = view else { return };
        view.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.result_host.addSubview(view);
        let result_host = &ivars.result_host;
        activate(&[
            view.leadingAnchor().constraintEqualToAnchor(&result_host.leadingAnchor()),
            view.trailingAnchor().constraintEqualToAnchor(&result_host.trailingAnchor()),
            view.topAnchor().constraintEqualToAnchor(&result_host.topAnchor()),
            view.bottomAnchor().constraintEqualToAnchor(&result_host.bottomAnchor()),
        ]);
    }

    /// The guidance label, for tests and scenes.
    pub fn guidance_for_testing(&self) -> Retained<NSTextField> {
        self.ivars().guidance.clone()
    }

    /// The find-bar height constraint's constant, for tests and scenes.
    pub fn find_height_for_testing(&self) -> CGFloat {
        self.ivars().find_height.borrow().as_ref().map_or(0.0, |constraint| constraint.constant())
    }
}
