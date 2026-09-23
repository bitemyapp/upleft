//! Port of `Panels/BreadcrumbView.swift`: the stable current-section control
//! (§5.1).
//!
//! The document title already lives in the toolbar. Repeating every ancestor
//! here makes the quiet reading lane look like a second title bar, so the
//! control shows only the section the reader is in. The complete path remains
//! one click away in a native menu. Its host reserves a quiet navigation lane:
//! orientation chrome must never cover the prose it describes.
//!
//! One deviation the runtime forces: Swift stores a `ZoomLevel` (a Swift
//! enum, boxed as `__SwiftValue`) as each zoom menu item's
//! `representedObject`; the port stores its raw value as an `NSNumber`.
//! Nothing outside this file reads it.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAnimationContext, NSAnimatablePropertyContainer, NSAttributedStringNSStringDrawing,
    NSBezelStyle, NSButtonCell, NSCellImagePosition, NSControlStateValueOff, NSControlStateValueOn, NSCursor,
    NSFocusRingType, NSImageScaling, NSLineBreakMode, NSMenu, NSMenuItem, NSResponder, NSStringDrawing, NSView,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSAttributedString, NSNumber, NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::{CAMediaTiming, CATransition, kCATransitionFade};
use upleft_core::contracts::ZoomLevel;
use upleft_render::appkit_compat::{attributed_string, attributes_dictionary, keys};
use upleft_render::motion;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_swift_text as swift_text;

use super::appkit_support::{
    RectExt, configured_symbol, downcast, ns_string, object, rect, role, set_help, set_label, set_role, smax, smin,
    symbol_configuration, weight_medium, weight_semibold,
};
use super::panel_chrome::{ButtonAction, PanelFont};
use crate::app::toolbar_controls::{ToolbarChromePolicy, ToolbarInteractiveButton};

/// `BreadcrumbDelegate`.
pub trait BreadcrumbDelegate {
    fn breadcrumb_did_select_heading_at(&self, view: &BreadcrumbView, index: isize);
}

/// One element of `trail`: Swift's `(index: Int, title: String, level: Int)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crumb {
    pub index: isize,
    pub title: String,
    pub level: isize,
}

impl Crumb {
    pub fn new(index: isize, title: &str, level: isize) -> Crumb {
        Crumb { index, title: title.to_owned(), level }
    }
}

/// `BreadcrumbView.Metrics`.
struct Metrics;

impl Metrics {
    /// Fixed chrome height. An empty or temporarily reparsing heading list
    /// must never move the document under the reader's eyes.
    const HEIGHT: CGFloat = 28.0;
    const BUTTON_HEIGHT: CGFloat = 24.0;
    const MAXIMUM_BUTTON_WIDTH: CGFloat = 420.0;
    const HORIZONTAL_CONTENT_ALLOWANCE: CGFloat = 26.0;
    /// A crumb swapping as the reader crosses into a new section: feedback
    /// they should register without watching, so it is `Motion.quick`.
    const SECTION_CHANGE_DURATION: f64 = motion::QUICK;
}

pub struct BreadcrumbViewIvars {
    delegate: RefCell<Option<Weak<dyn BreadcrumbDelegate>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    zoom_level: Cell<ZoomLevel>,
    on_zoom_change: RefCell<Option<Rc<dyn Fn(ZoomLevel)>>>,
    /// Ancestor chain, root first. Indices are into the document's headings.
    trail: RefCell<Vec<Crumb>>,
    section_button: Retained<ToolbarInteractiveButton>,
    zoom_button: Retained<ToolbarInteractiveButton>,
    section_action: RefCell<Option<Retained<ButtonAction>>>,
    zoom_action: RefCell<Option<Retained<ButtonAction>>>,
    is_cue_presented: Cell<bool>,
}

define_class!(
    /// `BreadcrumbView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "BreadcrumbView"]
    #[ivars = BreadcrumbViewIvars]
    pub struct BreadcrumbView;

    unsafe impl NSObjectProtocol for BreadcrumbView {}

    impl BreadcrumbView {
        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            NSSize::new(Metrics::MAXIMUM_BUTTON_WIDTH, Metrics::HEIGHT)
        }

        /// Belt and braces for the fade itself: while alpha is on its way to
        /// zero the button must already be unclickable.
        #[unsafe(method_id(hitTest:))]
        fn __hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            self.hit_test(point)
        }

        #[unsafe(method(layout))]
        fn __layout(&self) {
            self.layout_body();
        }

        #[unsafe(method(resetCursorRects))]
        fn __reset_cursor_rects(&self) {
            let ivars = self.ivars();
            if ivars.is_cue_presented.get() && !ivars.section_button.isHidden() {
                self.addCursorRect_cursor(ivars.section_button.frame(), &NSCursor::pointingHandCursor());
            }
            if !ivars.zoom_button.isHidden() {
                self.addCursorRect_cursor(ivars.zoom_button.frame(), &NSCursor::pointingHandCursor());
            }
        }

        #[unsafe(method(selectZoomItem:))]
        fn __select_zoom_item(&self, sender: &NSMenuItem) {
            self.select_zoom_item(sender);
        }

        #[unsafe(method(selectPathItem:))]
        fn __select_path_item(&self, sender: &NSMenuItem) {
            self.select_path_item(sender);
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.rebuild(false);
        }
    }
);

impl BreadcrumbView {
    /// `BreadcrumbView()`: hosts build panels before they have a theme in
    /// hand and assign `styleSheet` immediately afterwards.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<BreadcrumbView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<BreadcrumbView> {
        // Stored-property initial values, in declaration order, before
        // `super.init`.
        let section_button = ToolbarInteractiveButton::new(NSRect::ZERO, mtm);
        let zoom_button = ToolbarInteractiveButton::new(NSRect::ZERO, mtm);
        let this = Self::alloc(mtm).set_ivars(BreadcrumbViewIvars {
            delegate: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            zoom_level: Cell::new(ZoomLevel::Everything),
            on_zoom_change: RefCell::new(None),
            trail: RefCell::new(Vec::new()),
            section_button: section_button.clone(),
            zoom_button: zoom_button.clone(),
            section_action: RefCell::new(None),
            zoom_action: RefCell::new(None),
            is_cue_presented: Cell::new(false),
        });
        let this: Retained<BreadcrumbView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };

        let weak: ObjcWeak<BreadcrumbView> = ObjcWeak::from(&*this);
        let action = ButtonAction::new(
            move || {
                if let Some(this) = weak.load() {
                    this.show_path_menu();
                }
            },
            mtm,
        );
        *this.ivars().section_action.borrow_mut() = Some(action.clone());
        section_button.set_feedback_inset_x(0.0);
        section_button.set_feedback_inset_y(1.0);
        section_button.set_feedback_corner_radius(4.0);
        section_button.setBordered(false);
        section_button.setBezelStyle(NSBezelStyle::AccessoryBarAction);
        section_button.setFocusRingType(NSFocusRingType::Default);
        section_button.setImagePosition(NSCellImagePosition::ImageTrailing);
        section_button.setImageScaling(NSImageScaling::ScaleProportionallyDown);
        unsafe { section_button.setTarget(Some(object(&*action))) };
        unsafe { section_button.setAction(Some(ButtonAction::selector())) };
        if let Some(cell) = section_button.cell().and_then(|cell| downcast::<NSButtonCell>(&cell)) {
            cell.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        }
        this.addSubview(&section_button);

        let weak: ObjcWeak<BreadcrumbView> = ObjcWeak::from(&*this);
        let z_action = ButtonAction::new(
            move || {
                if let Some(this) = weak.load() {
                    this.show_zoom_menu();
                }
            },
            mtm,
        );
        *this.ivars().zoom_action.borrow_mut() = Some(z_action.clone());
        zoom_button.set_feedback_inset_x(2.0);
        zoom_button.set_feedback_inset_y(1.0);
        zoom_button.set_feedback_corner_radius(4.0);
        zoom_button.setBordered(false);
        zoom_button.setBezelStyle(NSBezelStyle::AccessoryBarAction);
        zoom_button.setFocusRingType(NSFocusRingType::Default);
        zoom_button.setImagePosition(NSCellImagePosition::ImageLeading);
        zoom_button.setImageScaling(NSImageScaling::ScaleProportionallyDown);
        unsafe { zoom_button.setTarget(Some(object(&*z_action))) };
        unsafe { zoom_button.setAction(Some(ButtonAction::selector())) };
        if let Some(cell) = zoom_button.cell().and_then(|cell| downcast::<NSButtonCell>(&cell)) {
            cell.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        }
        zoom_button.setHidden(true);
        this.addSubview(&zoom_button);

        set_role(&*this, role::group());
        set_label(&*this, "Current section");
        // Keep the lane in layout even when the cue itself is absent. Toggling
        // `isHidden` here would move the whole document at the first scroll.
        this.setHidden(false);
        this.setAlphaValue(1.0);
        section_button.setAlphaValue(0.0);
        this.update_zoom_button();
        this.rebuild(false);
        this
    }

    // MARK: - Properties

    pub fn delegate(&self) -> Option<Rc<dyn BreadcrumbDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn BreadcrumbDelegate>>) {
        *self.ivars().delegate.borrow_mut() = delegate;
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet;
        self.rebuild(false);
    }

    pub fn zoom_level(&self) -> ZoomLevel {
        self.ivars().zoom_level.get()
    }

    pub fn set_zoom_level(&self, zoom_level: ZoomLevel) {
        let old_value = self.ivars().zoom_level.replace(zoom_level);
        if zoom_level == old_value {
            return;
        }
        self.update_zoom_button();
        self.setNeedsLayout(true);
    }

    pub fn set_on_zoom_change(&self, handler: Option<Rc<dyn Fn(ZoomLevel)>>) {
        *self.ivars().on_zoom_change.borrow_mut() = handler;
    }

    pub fn trail(&self) -> Vec<Crumb> {
        self.ivars().trail.borrow().clone()
    }

    pub fn set_trail(&self, trail: Vec<Crumb>) {
        let old_value = self.ivars().trail.replace(trail);
        let crossed_section_boundary = {
            let trail = self.ivars().trail.borrow();
            if Self::same_trail(&old_value, &trail) {
                return;
            }
            let old_last = old_value.last().map(|crumb| crumb.index);
            let new_last = trail.last().map(|crumb| crumb.index);
            old_last.is_some() && new_last.is_some() && old_last != new_last
        };
        self.rebuild(crossed_section_boundary);
    }

    /// `isPresentedForTesting`.
    pub fn is_presented_for_testing(&self) -> bool {
        !self.isHidden() && self.ivars().is_cue_presented.get()
    }

    /// `currentTitleOrigin`.
    pub fn current_title_origin(&self) -> CGFloat {
        let button = &self.ivars().section_button;
        let title_rect = button
            .cell()
            .and_then(|cell| downcast::<NSButtonCell>(&cell))
            .map(|cell| cell.titleRectForBounds(button.bounds()))
            .unwrap_or_else(|| button.bounds());
        button.frame().min_x() + title_rect.min_x()
    }

    /// Scroll callbacks arrive continuously, but section identity changes
    /// only at heading boundaries. Avoid rebuilding AppKit controls for
    /// identical paths so scrolling stays direct and allocation-free.
    pub fn same_trail(lhs: &[Crumb], rhs: &[Crumb]) -> bool {
        if lhs.len() != rhs.len() {
            return false;
        }
        lhs.iter().zip(rhs).all(|(left, right)| {
            left.index == right.index && swift_text::str_eq(&left.title, &right.title) && left.level == right.level
        })
    }

    // MARK: - Building

    fn rebuild(&self, animated: bool) {
        let ivars = self.ivars();
        let section_button = &ivars.section_button;
        let current = ivars.trail.borrow().last().cloned();
        let Some(current) = current else {
            section_button.setHidden(true);
            section_button.setAttributedTitle(&NSAttributedString::new());
            section_button.setImage(None);
            set_label(self, "Current section");
            self.hide_current_section();
            self.setNeedsLayout(true);
            return;
        };

        section_button.setHidden(false);
        self.prepare_section_change_transition_if_needed(animated);
        section_button.setAttributedTitle(&self.styled_title(&current.title));
        section_button.setImage(
            configured_symbol("chevron.down", None, &symbol_configuration(8.0, weight_semibold())).as_deref(),
        );
        let style_sheet = self.style_sheet();
        section_button.setContentTintColor(Some(&style_sheet.text_faint));
        let titles: Vec<String> = ivars.trail.borrow().iter().map(|crumb| crumb.title.clone()).collect();
        section_button.setToolTip(Some(&ns_string(&titles.join(" › "))));
        set_label(&**section_button, &format!("Current section: {}", current.title));
        set_help(&**section_button, &("Jump to this section or an ancestor. Path: ".to_owned() + &titles.join(", ")));
        set_label(self, &format!("Current section: {}", current.title));
        self.setNeedsLayout(true);
    }

    pub fn show_current_section(&self) {
        let ivars = self.ivars();
        if ivars.trail.borrow().is_empty() {
            return;
        }
        if ivars.is_cue_presented.get() {
            return;
        }
        ivars.is_cue_presented.set(true);
        ivars.section_button.setHidden(false);
        self.update_presentation_state();
        self.fade(1.0, None);
    }

    /// Fades out over the same 0.12s the fade in takes — an instant
    /// disappearance beside a gentle arrival reads as a glitch — and then
    /// takes the button out of hit-testing, the cursor rects, and the
    /// accessibility tree. The lane's height is the view's own intrinsic
    /// size, so the document underneath does not move (§5.1).
    pub fn hide_current_section(&self) {
        self.ivars().is_cue_presented.set(false);
        self.update_presentation_state();
        let weak: ObjcWeak<BreadcrumbView> = ObjcWeak::from(self);
        self.fade(
            0.0,
            Some(Box::new(move || {
                let Some(this) = weak.load() else { return };
                if this.ivars().is_cue_presented.get() {
                    return;
                }
                this.ivars().section_button.setHidden(true);
                this.discardCursorRects();
                if let Some(window) = this.window() {
                    window.invalidateCursorRectsForView(&this);
                }
            })),
        );
    }

    fn fade(&self, alpha: CGFloat, completion: Option<Box<dyn Fn()>>) {
        let section_button = self.ivars().section_button.clone();
        if !(!self.style_sheet().reduce_motion && self.window().is_some()) {
            section_button.setAlphaValue(alpha);
            if let Some(completion) = completion {
                completion();
            }
            return;
        }
        let changes = block2::RcBlock::new(move |context: std::ptr::NonNull<NSAnimationContext>| {
            let context = unsafe { context.as_ref() };
            context.setDuration(Metrics::SECTION_CHANGE_DURATION);
            context.setTimingFunction(Some(&ToolbarChromePolicy::timing_function()));
            let animator = section_button.animator();
            let _: () = unsafe { msg_send![&*animator, setAlphaValue: alpha] };
        });
        let completion = completion.map(|completion| block2::RcBlock::new(move || completion()));
        NSAnimationContext::runAnimationGroup_completionHandler(&changes, completion.as_deref());
    }

    fn update_presentation_state(&self) {
        // An invisible control is not a control: no clicks, no pointing hand,
        // and nothing for VoiceOver to land on.
        let ivars = self.ivars();
        ivars.section_button.setAccessibilityElement(ivars.is_cue_presented.get());
        self.discardCursorRects();
        if let Some(window) = self.window() {
            window.invalidateCursorRectsForView(self);
        }
    }

    fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
        if self.ivars().is_cue_presented.get() { unsafe { msg_send![super(self), hitTest: point] } } else { None }
    }

    fn styled_title(&self, text: &str) -> Retained<NSAttributedString> {
        // Chrome should remain visibly distinct from the document heading
        // it names. Medium system text reads as navigation, not content.
        let font = PanelFont::system(12.0, weight_medium());
        let color = self.style_sheet().text_secondary.clone();
        attributed_string(text, &[(keys::font(), object(&*font)), (keys::foreground_color(), object(&*color))])
    }

    /// A section boundary should register in peripheral vision without
    /// making the title feel delayed or absent. Install the compositor
    /// transition before the immediate title swap so old and new pixels
    /// crossfade.
    fn prepare_section_change_transition_if_needed(&self, animated: bool) {
        if !animated || self.style_sheet().reduce_motion {
            return;
        }
        let Some(layer) = self.ivars().section_button.layer() else { return };
        let key = NSString::from_str("section-change");
        layer.removeAnimationForKey(&key);
        let transition = CATransition::new();
        transition.setType(unsafe { kCATransitionFade });
        transition.setDuration(Metrics::SECTION_CHANGE_DURATION);
        transition.setTimingFunction(Some(&ToolbarChromePolicy::timing_function()));
        layer.addAnimation_forKey(&transition, Some(&key));
    }

    fn update_zoom_button(&self) {
        let ivars = self.ivars();
        let zoom_button = &ivars.zoom_button;
        let zoom_level = ivars.zoom_level.get();
        if zoom_level == ZoomLevel::Everything {
            zoom_button.setHidden(true);
            return;
        }
        zoom_button.setHidden(false);
        let label = match zoom_level {
            ZoomLevel::H1 => "Top level",
            ZoomLevel::H2 => "Two levels",
            ZoomLevel::Headings => "Headings",
            ZoomLevel::Skeleton => "Skeleton",
            ZoomLevel::Everything => "Everything",
        };
        let style_sheet = self.style_sheet();
        let font = PanelFont::system(11.5, weight_medium());
        zoom_button.setAttributedTitle(&attributed_string(
            label,
            &[(keys::font(), object(&*font)), (keys::foreground_color(), object(&*style_sheet.accent))],
        ));
        zoom_button.setImage(
            configured_symbol(
                "line.3.horizontal.decrease.circle.fill",
                Some("Structural Zoom"),
                &symbol_configuration(10.0, weight_semibold()),
            )
            .as_deref(),
        );
        zoom_button.setContentTintColor(Some(&style_sheet.accent));
        zoom_button.setToolTip(Some(&ns_string(&format!(
            "Structural Zoom active ({label}). Click to switch or restore all content."
        ))));
        set_label(&**zoom_button, &format!("Structural zoom: {label}"));
    }

    // MARK: - Layout

    fn layout_body(&self) {
        let _: () = unsafe { msg_send![super(self), layout] };
        let ivars = self.ivars();
        let bounds = self.bounds();
        let current = ivars.trail.borrow().last().cloned();
        if let Some(current) = current {
            let title_width = self.styled_title(&current.title).size().width;
            let available_width = smin(smax(44.0, bounds.width()), Metrics::MAXIMUM_BUTTON_WIDTH);
            let width = smin(available_width, title_width + Metrics::HORIZONTAL_CONTENT_ALLOWANCE);
            ivars.section_button.setFrame(rect(
                0.0,
                (bounds.height() - Metrics::BUTTON_HEIGHT) / 2.0,
                smax(44.0, width),
                Metrics::BUTTON_HEIGHT,
            ));
        }
        if !ivars.zoom_button.isHidden() {
            let font = PanelFont::system(11.5, weight_medium());
            let attributes = attributes_dictionary(&[(keys::font(), object(&*font))]);
            let title = ivars.zoom_button.attributedTitle().string();
            let zoom_text_width = unsafe { title.sizeWithAttributes(Some(&attributes)) }.width + 26.0;
            let zoom_x = if ivars.is_cue_presented.get() && !ivars.section_button.isHidden() {
                ivars.section_button.frame().max_x() + 8.0
            } else {
                0.0
            };
            ivars.zoom_button.setFrame(rect(
                zoom_x,
                (bounds.height() - Metrics::BUTTON_HEIGHT) / 2.0,
                smax(40.0, zoom_text_width),
                Metrics::BUTTON_HEIGHT,
            ));
        }
        self.discardCursorRects();
        if let Some(window) = self.window() {
            window.invalidateCursorRectsForView(self);
        }
    }

    fn show_zoom_menu(&self) {
        let mtm = self.mtm();
        let menu = NSMenu::new(mtm);
        let zoom_level = self.ivars().zoom_level.get();
        for level in ZoomLevel::ALL_CASES {
            let name = match level {
                ZoomLevel::H1 => "Top Level (H1)",
                ZoomLevel::H2 => "Two Levels (H1–H2)",
                ZoomLevel::Headings => "All Headings",
                ZoomLevel::Skeleton => "Skeleton (Headings, First Sentences, Artifacts)",
                ZoomLevel::Everything => "Everything (Full Document)",
            };
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &ns_string(&format!("Level {} — {name}", level.raw_value())),
                    Some(sel!(selectZoomItem:)),
                    &NSString::from_str(""),
                )
            };
            unsafe { item.setTarget(Some(object(self))) };
            // Swift stores the `ZoomLevel` itself (a `__SwiftValue` box).
            unsafe { item.setRepresentedObject(Some(object(&*NSNumber::new_isize(level.raw_value())))) };
            item.setState(if level == zoom_level { NSControlStateValueOn } else { NSControlStateValueOff });
            menu.addItem(&item);
        }
        let frame = self.ivars().zoom_button.frame();
        menu.popUpMenuPositioningItem_atLocation_inView(None, NSPoint::new(frame.min_x(), frame.min_y()), Some(self));
    }

    fn select_zoom_item(&self, sender: &NSMenuItem) {
        let Some(represented) = sender.representedObject() else { return };
        let Some(number) = represented.downcast_ref::<NSNumber>() else { return };
        let Some(level) = ZoomLevel::from_raw_value(number.as_isize()) else { return };
        let handler = self.ivars().on_zoom_change.borrow().clone();
        if let Some(handler) = handler {
            handler(level);
        }
    }

    fn show_path_menu(&self) {
        if self.ivars().trail.borrow().is_empty() {
            return;
        }
        let menu = self.make_path_menu();
        // Anchor to the control that owns the path. Positioning the last item
        // at the host's origin made the menu appear detached from the crumb
        // (and selected the deepest item by default), especially when the
        // navigation lane was inset by a split view.
        let frame = self.ivars().section_button.frame();
        menu.popUpMenuPositioningItem_atLocation_inView(None, NSPoint::new(frame.min_x(), frame.min_y()), Some(self));
    }

    pub fn make_path_menu(&self) -> Retained<NSMenu> {
        let mtm = self.mtm();
        let menu = NSMenu::new(mtm);
        let trail = self.trail();
        let base_level = trail.first().map_or(1, |crumb| crumb.level);
        for (position, crumb) in trail.iter().enumerate() {
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &ns_string(&crumb.title),
                    Some(sel!(selectPathItem:)),
                    &NSString::from_str(""),
                )
            };
            unsafe { item.setTarget(Some(object(self))) };
            unsafe { item.setRepresentedObject(Some(object(&*NSNumber::new_isize(crumb.index)))) };
            item.setIndentationLevel(std::cmp::max(0, crumb.level - base_level));
            item.setState(if position == trail.len() - 1 { NSControlStateValueOn } else { NSControlStateValueOff });
            menu.addItem(&item);
        }
        menu
    }

    fn select_path_item(&self, sender: &NSMenuItem) {
        let Some(represented) = sender.representedObject() else { return };
        let Some(number) = represented.downcast_ref::<NSNumber>() else { return };
        let index = number.as_isize();
        if let Some(delegate) = self.delegate() {
            delegate.breadcrumb_did_select_heading_at(self, index);
        }
    }

    // MARK: - Test hooks

    pub fn section_button_for_testing(&self) -> Retained<ToolbarInteractiveButton> {
        self.ivars().section_button.clone()
    }

    pub fn zoom_button_for_testing(&self) -> Retained<ToolbarInteractiveButton> {
        self.ivars().zoom_button.clone()
    }
}

