//! Port of `Panels/FindBarView.swift`: the find bar (§9.4).
//!
//! Find-as-you-type: every keystroke emits a query, because a live match count
//! and live gutter ticks are the whole point of searching a document you are
//! looking at.  A half-typed regex is not an error — the field warns quietly
//! and the document keeps its previous highlighting rather than throwing a
//! dialog per character.
//!
//! Objective-C class names equal the Swift ones: `FindBarView`, and the two
//! private field classes `FindBarSearchField` (an `NSSearchField` with no
//! overrides) and `FindBarReplaceField`.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::{Rc, Weak};

use block2::RcBlock;
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAnimatablePropertyContainer, NSAnimationContext, NSApplication, NSAutoresizingMaskOptions, NSBezierPath, NSButton, NSColor,
    NSControl, NSControlSize, NSControlStateValue, NSControlStateValueOff, NSControlStateValueOn,
    NSControlTextEditingDelegate, NSEventModifierFlags, NSFocusRingType, NSImageScaling, NSImageView,
    NSLayoutAttribute, NSLayoutConstraintOrientation, NSLayoutPriorityDefaultLow, NSLayoutPriorityRequired, NSMenu,
    NSMenuItem, NSResponder, NSSearchField, NSSearchFieldCell, NSSearchFieldDelegate, NSStackView, NSTextAlignment,
    NSTextField, NSTextFieldDelegate, NSTextView, NSUserInterfaceItemIdentification, NSUserInterfaceLayoutOrientation,
    NSView, NSViewNoIntrinsicMetric, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
    NSWindowDidBecomeKeyNotification, NSWindowDidResignKeyNotification, NSWindowOrderingMode,
};
use objc2_core_foundation::{CFRetained, CGAffineTransform, CGFloat, CGSize};
use objc2_core_graphics::CGPath;
use objc2_foundation::{
    NSArray, NSNotification, NSNotificationCenter, NSNumber, NSOperationQueue, NSPoint, NSRect, NSSize, NSString,
};
use objc2_quartz_core::{
    CAAnimation, CAAnimationGroup, CACurrentMediaTime, CAKeyframeAnimation, CAMediaTiming, CAShapeLayer,
    CATransaction, CATransform3D, CATransform3DIdentity, CATransition, kCACornerCurveContinuous, kCAFillModeBackwards,
    kCATransitionFade,
};
use upleft_core::{ChangeKind, NSRange};
use upleft_render::appkit_compat::{attributed_string, keys};
use upleft_render::motion::{self, Curve};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::view::style_sheet_defaults::PanelAlpha;
use upleft_swift_text as swift;

use super::appkit_support::{
    IDENTITY, Presentation as LayerPresentation, RectExt, activate, cg, configured_symbol, label, needs_display,
    ns_string, null_actions, object, rect, rect_fill, role, set_label, set_mask, set_role, smax, smin, symbol_configuration,
    system_symbol, weight_medium, weight_regular,
};
use super::chrome_glass::{ChromeGlass, RoundedCorners, Tint};
use super::panel_chrome::{ButtonAction, PanelBackdrop, PanelButton, PanelFont, PanelMetrics, install_backdrop};
use crate::support::find_engine::FindQuery;

/// `FindBarDelegate`.
pub trait FindBarDelegate {
    fn find_bar_did_change(&self, bar: &FindBarView, query: FindQuery);
    fn find_bar_did_request_advance(&self, bar: &FindBarView, forward: bool);
    fn find_bar_did_request_replace(&self, bar: &FindBarView, replacement: &str, all: bool);
    fn find_bar_did_request_close(&self, bar: &FindBarView);
}

/// `FindBarDensity`: the find bar's typographic span. The document stack
/// compresses this with a high-priority cap so a narrow window shrinks the
/// pill instead of clipping it.
pub struct FindBarDensity;

impl FindBarDensity {
    pub const BAR_WIDTH: CGFloat = 480.0;
    pub const BAR_HEIGHT: CGFloat = 42.0;
    pub const REPLACE_HEIGHT: CGFloat = 80.0;
}

// MARK: - Focus ring

/// `FindFieldRing`: the find bar's fields draw their own focus ring (§9.4).
struct FindFieldRing;

type ObserverToken = Retained<ProtocolObject<dyn NSObjectProtocol>>;

impl FindFieldRing {
    /// Inset from the field's edge, so the stroke lands just inside the bezel
    /// outline instead of reaching over the chrome outside it.
    const INSET: CGFloat = 1.0;
    const LINE_WIDTH: CGFloat = 1.5;

    /// The platform ring's visibility rule, kept: the field owns the key
    /// window's first responder. (Swift's `===` on two optionals: two nils
    /// are identical.)
    fn is_visible(field: &NSTextField) -> bool {
        let Some(window) = field.window() else { return false };
        if !window.isKeyWindow() {
            return false;
        }
        let first = window.firstResponder().map(|responder| Retained::as_ptr(&responder) as *const AnyObject);
        let editor = field.currentEditor().map(|editor| Retained::as_ptr(&editor) as *const AnyObject);
        first == editor || first == Some(object(field) as *const AnyObject)
    }

    fn stroke(field: &NSTextField, corner_radius: CGFloat, color: &NSColor) {
        let bezel = field.bounds().inset_by(Self::INSET, Self::INSET);
        let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bezel, corner_radius, corner_radius);
        path.setLineWidth(Self::LINE_WIDTH);
        color.setStroke();
        path.stroke();
    }

    /// With the system ring retired, AppKit no longer drives ring repaints, so
    /// the field repaints itself when its window's key status flips.
    /// Observers from a previous window are retired first.
    fn reinstall_key_status_redraw(field: &FindBarReplaceField, old: Vec<ObserverToken>) -> Vec<ObserverToken> {
        let center = NSNotificationCenter::defaultCenter();
        for token in &old {
            // SAFETY: each token came from this centre's `addObserverForName:…`.
            unsafe { center.removeObserver(object(&**token)) };
        }
        let Some(window) = field.window() else { return Vec::new() };
        // SAFETY: AppKit exports the notification names as immutable globals.
        let names = unsafe { [NSWindowDidBecomeKeyNotification, NSWindowDidResignKeyNotification] };
        names
            .into_iter()
            .map(|name| {
                let weak: ObjcWeak<FindBarReplaceField> = ObjcWeak::from(field);
                let block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
                    if let Some(field) = weak.load() {
                        needs_display(&field);
                    }
                });
                // SAFETY: the block only touches the field on the main queue.
                unsafe {
                    center.addObserverForName_object_queue_usingBlock(
                        Some(name),
                        Some(object(&*window)),
                        Some(&NSOperationQueue::mainQueue()),
                        &block,
                    )
                }
            })
            .collect()
    }
}

define_class!(
    /// `FindBarSearchField`: the find field itself.  Its bezel is a capsule,
    /// so the ring's radius comes from the ring rect's own height.
    // SAFETY: no ivars; AppKit's own initialisers are safe to inherit.
    #[unsafe(super(NSSearchField, NSTextField, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "FindBarSearchField"]
    pub struct FindBarSearchField;

    unsafe impl NSObjectProtocol for FindBarSearchField {}
);

impl FindBarSearchField {
    /// `FindBarSearchField()`.
    fn new(mtm: MainThreadMarker) -> Retained<FindBarSearchField> {
        unsafe { msg_send![FindBarSearchField::alloc(mtm), init] }
    }
}

pub struct FindBarReplaceFieldIvars {
    ring_color: RefCell<Retained<NSColor>>,
    key_observers: RefCell<Vec<ObserverToken>>,
}

impl Drop for FindBarReplaceFieldIvars {
    /// `deinit { keyObservers.forEach(NotificationCenter.default.removeObserver) }`.
    fn drop(&mut self) {
        let center = NSNotificationCenter::defaultCenter();
        for token in self.key_observers.get_mut().drain(..) {
            // SAFETY: each token came from this centre's `addObserverForName:…`.
            unsafe { center.removeObserver(object(&*token)) };
        }
    }
}

define_class!(
    /// `FindBarReplaceField`: the replace row's field draws the same ring so
    /// the two rows never disagree; its bezel is the standard rounded rect.
    // SAFETY: `init` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSTextField, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "FindBarReplaceField"]
    #[ivars = FindBarReplaceFieldIvars]
    pub struct FindBarReplaceField;

    unsafe impl NSObjectProtocol for FindBarReplaceField {}

    impl FindBarReplaceField {
        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, dirty_rect: NSRect) {
            let _: () = unsafe { msg_send![super(self), drawRect: dirty_rect] };
            if !FindFieldRing::is_visible(self) {
                return;
            }
            let color = self.ivars().ring_color.borrow().clone();
            FindFieldRing::stroke(self, 5.0, &color);
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn __view_did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            let old = std::mem::take(&mut *self.ivars().key_observers.borrow_mut());
            let observers = FindFieldRing::reinstall_key_status_redraw(self, old);
            *self.ivars().key_observers.borrow_mut() = observers;
        }
    }
);

impl FindBarReplaceField {
    /// `FindBarReplaceField()`.
    fn new(mtm: MainThreadMarker) -> Retained<FindBarReplaceField> {
        let this = Self::alloc(mtm).set_ivars(FindBarReplaceFieldIvars {
            ring_color: RefCell::new(NSColor::controlAccentColor()),
            key_observers: RefCell::new(Vec::new()),
        });
        unsafe { msg_send![super(this), init] }
    }

    fn set_ring_color(&self, color: Retained<NSColor>) {
        *self.ivars().ring_color.borrow_mut() = color;
        needs_display(self);
    }
}

// MARK: - FindBarView

/// `FindBarView.Presentation`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presentation {
    Bar,
    Inspector,
}

pub struct FindBarViewIvars {
    delegate: RefCell<Option<Weak<dyn FindBarDelegate>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    shows_replace: Cell<bool>,
    status_text: RefCell<String>,
    is_query_valid: Cell<bool>,
    selection_scope: Cell<Option<NSRange>>,

    backdrop: Retained<PanelBackdrop>,
    glass: RefCell<Option<Retained<ChromeGlass>>>,
    inspector_well: Retained<NSView>,
    search_field: Retained<FindBarSearchField>,
    replace_field: Retained<FindBarReplaceField>,
    leading_glyph: Retained<NSImageView>,
    status_label: Retained<NSTextField>,
    warning_image: Retained<NSImageView>,
    trailer_divider: Retained<NSView>,
    options_divider: Retained<NSView>,
    regex_toggle: Retained<NSButton>,
    case_toggle: Retained<NSButton>,
    word_toggle: Retained<NSButton>,
    scope_toggle: Retained<NSButton>,
    options_button: Retained<NSButton>,
    previous_button: RefCell<Option<Retained<NSButton>>>,
    next_button: RefCell<Option<Retained<NSButton>>>,
    replace_button: RefCell<Option<Retained<NSButton>>>,
    replace_all_button: RefCell<Option<Retained<NSButton>>>,
    close_button: RefCell<Option<Retained<NSButton>>>,
    find_row: Retained<NSStackView>,
    replace_row: Retained<NSStackView>,
    rows: Retained<NSStackView>,
    entrance_mask: Retained<CAShapeLayer>,
    entrance_generation: Cell<isize>,
    replace_transition_generation: Cell<isize>,
    actions: RefCell<Vec<Retained<ButtonAction>>>,
    options_action: RefCell<Option<Retained<ButtonAction>>>,
    presentation: Presentation,
}

define_class!(
    /// `FindBarView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "FindBarView"]
    #[ivars = FindBarViewIvars]
    pub struct FindBarView;

    unsafe impl NSObjectProtocol for FindBarView {}

    impl FindBarView {
        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            let ivars = self.ivars();
            let glass = ivars.glass.borrow().clone();
            if let Some(glass) = glass {
                glass.setFrame(self.bounds());
            }
            if ivars.presentation == Presentation::Inspector {
                let row_frame = self.convertRect_fromView(ivars.find_row.bounds(), Some(&ivars.find_row));
                ivars.inspector_well.setFrame(row_frame.inset_by(-2.0, -3.0));
            }
        }

        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            self.intrinsic_content_size()
        }

        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, _dirty_rect: NSRect) {
            if self.ivars().presentation != Presentation::Inspector {
                return;
            }
            self.style_sheet().rule.setFill();
            rect_fill(rect(0.0, 0.0, self.bounds().width(), PanelMetrics::HAIRLINE));
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.apply_style();
        }

        #[unsafe(method(toggleRegexOption:))]
        fn __toggle_regex_option(&self, _sender: Option<&NSMenuItem>) {
            let toggle = self.ivars().regex_toggle.clone();
            self.toggle(&toggle);
        }

        #[unsafe(method(toggleCaseOption:))]
        fn __toggle_case_option(&self, _sender: Option<&NSMenuItem>) {
            let toggle = self.ivars().case_toggle.clone();
            self.toggle(&toggle);
        }

        #[unsafe(method(toggleWordOption:))]
        fn __toggle_word_option(&self, _sender: Option<&NSMenuItem>) {
            let toggle = self.ivars().word_toggle.clone();
            self.toggle(&toggle);
        }

        #[unsafe(method(toggleScopeOption:))]
        fn __toggle_scope_option(&self, _sender: Option<&NSMenuItem>) {
            if self.ivars().selection_scope.get().is_none() {
                return;
            }
            let toggle = self.ivars().scope_toggle.clone();
            self.toggle(&toggle);
        }
    }

    // MARK: - Field editing

    unsafe impl NSControlTextEditingDelegate for FindBarView {
        #[unsafe(method(controlTextDidChange:))]
        fn __control_text_did_change(&self, notification: &NSNotification) {
            if !self.is_search_field(notification.object().as_deref()) {
                return;
            }
            self.update_control_enablement();
            self.emit_query();
        }

        /// With the system ring retired, the fields repaint their own ring
        /// as the field editor comes and goes.
        #[unsafe(method(controlTextDidBeginEditing:))]
        fn __control_text_did_begin_editing(&self, notification: &NSNotification) {
            self.editing_changed(notification, true);
        }

        #[unsafe(method(controlTextDidEndEditing:))]
        fn __control_text_did_end_editing(&self, notification: &NSNotification) {
            self.editing_changed(notification, false);
        }

        #[unsafe(method(control:textView:doCommandBySelector:))]
        fn __control_do_command(&self, control: &NSControl, _text_view: &NSTextView, selector: Sel) -> bool {
            self.do_command(control, selector)
        }
    }

    unsafe impl NSTextFieldDelegate for FindBarView {}

    unsafe impl NSSearchFieldDelegate for FindBarView {}
);

impl FindBarView {
    /// `FindBarView()`: hosts build panels before they have a theme in hand
    /// and assign `styleSheet` immediately afterwards.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<FindBarView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), Presentation::Bar, mtm)
    }

    /// `init(styleSheet:presentation:)`; Swift's default presentation is
    /// `.bar`.
    pub fn new(style_sheet: Rc<StyleSheet>, presentation: Presentation, mtm: MainThreadMarker) -> Retained<FindBarView> {
        // Stored-property initial values, in declaration order.
        let inspector_well = NSView::new(mtm);
        let search_field = FindBarSearchField::new(mtm);
        let replace_field = FindBarReplaceField::new(mtm);
        let leading_glyph = NSImageView::new(mtm);
        let status_label = label("", mtm);
        let warning_image = NSImageView::new(mtm);
        let trailer_divider = NSView::new(mtm);
        let options_divider = NSView::new(mtm);
        let find_row = NSStackView::new(mtm);
        let replace_row = NSStackView::new(mtm);
        let rows = NSStackView::new(mtm);
        let entrance_mask = CAShapeLayer::new();

        let backdrop = PanelBackdrop::new(
            style_sheet.clone(),
            NSVisualEffectMaterial::HeaderView,
            NSVisualEffectBlendingMode::WithinWindow,
            mtm,
        );

        // Targets are rebound below; the toggles have to exist before
        // `super.init`, and their real handler needs `self`.
        let placeholder = ButtonAction::noop(mtm);
        let regex_toggle = PanelButton::toggle(".*", "Regular expression", &placeholder, mtm);
        let case_toggle = PanelButton::toggle("Aa", "Match case", &placeholder, mtm);
        let word_toggle = PanelButton::toggle("W", "Whole word", &placeholder, mtm);
        let scope_toggle = PanelButton::toggle("In Selection", "Search in selection", &placeholder, mtm);
        let options_button = PanelButton::symbol_default("slider.horizontal.3", "Find options", &placeholder, mtm);

        let this = Self::alloc(mtm).set_ivars(FindBarViewIvars {
            delegate: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet.clone()),
            shows_replace: Cell::new(false),
            status_text: RefCell::new(String::new()),
            is_query_valid: Cell::new(true),
            selection_scope: Cell::new(None),
            backdrop: backdrop.clone(),
            glass: RefCell::new(None),
            inspector_well: inspector_well.clone(),
            search_field,
            replace_field,
            leading_glyph,
            status_label,
            warning_image,
            trailer_divider,
            options_divider,
            regex_toggle,
            case_toggle,
            word_toggle,
            scope_toggle,
            options_button,
            previous_button: RefCell::new(None),
            next_button: RefCell::new(None),
            replace_button: RefCell::new(None),
            replace_all_button: RefCell::new(None),
            close_button: RefCell::new(None),
            find_row,
            replace_row,
            rows,
            entrance_mask,
            entrance_generation: Cell::new(0),
            replace_transition_generation: Cell::new(0),
            actions: RefCell::new(Vec::new()),
            options_action: RefCell::new(None),
            presentation,
        });
        let this: Retained<FindBarView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };

        let content_host: Retained<NSView> = if presentation == Presentation::Bar {
            let glass = ChromeGlass::new(
                style_sheet.clone(),
                PanelMetrics::CHROME_PILL_RADIUS,
                RoundedCorners::All,
                Tint::Panel,
                mtm,
            );
            glass.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            glass.setFrame(this.bounds());
            glass.set_shadow_radius(18.0);
            glass.set_shadow_offset(CGSize::new(0.0, -4.0));
            this.addSubview(&glass);
            *this.ivars().glass.borrow_mut() = Some(glass.clone());
            glass.content_view()
        } else {
            install_backdrop(&this, &backdrop);
            inspector_well.setWantsLayer(true);
            if let Some(layer) = inspector_well.layer() {
                // SAFETY: Core Animation exports the corner curves as
                // immutable globals.
                layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
            }
            if let Some(layer) = inspector_well.layer() {
                layer.setCornerRadius(PanelMetrics::NESTED_SURFACE_RADIUS);
            }
            this.addSubview_positioned_relativeTo(&inspector_well, NSWindowOrderingMode::Above, Some(&backdrop));
            Retained::into_super(this.clone())
        };

        this.build_find_row();
        this.build_replace_row();

        let ivars = this.ivars();
        let rows = &ivars.rows;
        rows.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        rows.setAlignment(NSLayoutAttribute::Leading);
        rows.setSpacing(if presentation == Presentation::Inspector { 8.0 } else { 6.0 });
        rows.setTranslatesAutoresizingMaskIntoConstraints(false);
        rows.addArrangedSubview(&ivars.find_row);
        rows.addArrangedSubview(&ivars.replace_row);
        ivars.replace_row.setHidden(true);
        content_host.addSubview(rows);

        activate(&[
            rows.leadingAnchor().constraintEqualToAnchor_constant(&content_host.leadingAnchor(), PanelMetrics::INSET),
            rows.trailingAnchor()
                .constraintEqualToAnchor_constant(&content_host.trailingAnchor(), -PanelMetrics::INSET),
            rows.topAnchor().constraintEqualToAnchor_constant(
                &content_host.topAnchor(),
                if presentation == Presentation::Inspector { 10.0 } else { 7.0 },
            ),
            ivars.find_row.widthAnchor().constraintEqualToAnchor(&rows.widthAnchor()),
            ivars.replace_row.widthAnchor().constraintEqualToAnchor(&rows.widthAnchor()),
        ]);

        // The toggles all mean "recompute the query", so they share one
        // handler rather than four near-identical ones.
        for toggle in [&ivars.regex_toggle, &ivars.case_toggle, &ivars.word_toggle, &ivars.scope_toggle] {
            let weak: ObjcWeak<FindBarView> = ObjcWeak::from(&*this);
            let action = ButtonAction::new(
                move || {
                    if let Some(this) = weak.load() {
                        this.emit_query();
                    }
                },
                mtm,
            );
            ivars.actions.borrow_mut().push(action.clone());
            // SAFETY: the panel keeps the action alive in `actions`.
            unsafe { toggle.setTarget(Some(&action)) };
            // SAFETY: `fire:` is the action's selector.
            unsafe { toggle.setAction(Some(ButtonAction::selector())) };
        }
        let weak: ObjcWeak<FindBarView> = ObjcWeak::from(&*this);
        let options_action = ButtonAction::new(
            move || {
                if let Some(this) = weak.load() {
                    this.show_options_menu();
                }
            },
            mtm,
        );
        *ivars.options_action.borrow_mut() = Some(options_action.clone());
        // SAFETY: the panel keeps the action alive in `options_action`.
        unsafe { ivars.options_button.setTarget(Some(&options_action)) };
        // SAFETY: `fire:` is the action's selector.
        unsafe { ivars.options_button.setAction(Some(ButtonAction::selector())) };
        ivars.scope_toggle.setEnabled(false);

        this.apply_style();
        this.apply_validity();
        this.update_control_enablement();
        set_role(&*this, role::group());
        set_label(&*this, "Find");

        this.setWantsLayer(true);
        drop(placeholder);
        this
    }

    fn build_find_row(&self) {
        let mtm = self.mtm();
        let ivars = self.ivars();
        let search_field = &ivars.search_field;
        search_field.setPlaceholderString(Some(&ns_string("Find…")));
        search_field.setSendsWholeSearchString(false);
        search_field.setSendsSearchStringImmediately(true);
        // SAFETY: the bar owns the field and outlives it.
        unsafe { search_field.setDelegate(Some(ProtocolObject::from_ref(self))) };
        search_field.setFont(Some(&PanelFont::system_regular(13.5)));
        search_field.setControlSize(NSControlSize::Small);
        search_field.setBezeled(false);
        search_field.setDrawsBackground(false);
        search_field.setBackgroundColor(Some(&NSColor::clearColor()));
        if let Some(cell) = search_field.cell() {
            cell.setBezeled(false);
        }
        if let Some(cell) = search_field.cell().and_then(|cell| cell.downcast::<NSSearchFieldCell>().ok()) {
            cell.setSearchButtonCell(None);
        }
        search_field.setFocusRingType(NSFocusRingType::None);
        set_label(&**search_field, "Find");
        search_field.setContentHuggingPriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );

        let leading_glyph = &ivars.leading_glyph;
        leading_glyph.setImage(
            configured_symbol("magnifyingglass", None, &symbol_configuration(15.0, weight_medium())).as_deref(),
        );
        leading_glyph.setImageScaling(NSImageScaling::ScaleProportionallyDown);
        leading_glyph.setAccessibilityElement(false);
        leading_glyph.widthAnchor().constraintEqualToConstant(22.0).setActive(true);

        let warning_image = &ivars.warning_image;
        warning_image
            .setImage(system_symbol("exclamationmark.triangle.fill", Some("Invalid regular expression")).as_deref());
        warning_image.setToolTip(Some(&ns_string("Invalid regular expression")));
        warning_image.setHidden(true);
        // Layer-backed so the invalid-pattern glyph can pop in rather than
        // appear (`setWarningVisible`).
        warning_image.setWantsLayer(true);

        let status_label = &ivars.status_label;
        status_label.setFont(Some(&PanelFont::secondary()));
        status_label.setAlignment(NSTextAlignment::Right);
        // Layer-backed so a changing count can crossfade (`setStatusLabelText`).
        status_label.setWantsLayer(true);
        // Starts hidden: no count is pinned until a session reports one, and
        // a hidden stack member leaves no gap in the tray.
        status_label.setHidden(true);
        status_label
            .setContentHuggingPriority_forOrientation(NSLayoutPriorityRequired, NSLayoutConstraintOrientation::Horizontal);
        status_label.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityRequired,
            NSLayoutConstraintOrientation::Horizontal,
        );
        // A settled slot for the count so "1 of 1" and "12 of 12" never nudge
        // the surrounding buttons (§9.4).
        status_label.widthAnchor().constraintGreaterThanOrEqualToConstant(52.0).setActive(true);

        let weak: ObjcWeak<FindBarView> = ObjcWeak::from(self);
        let previous = ButtonAction::new(
            move || {
                if let Some(this) = weak.load()
                    && let Some(delegate) = this.delegate()
                {
                    delegate.find_bar_did_request_advance(&this, false);
                }
            },
            mtm,
        );
        let weak: ObjcWeak<FindBarView> = ObjcWeak::from(self);
        let next = ButtonAction::new(
            move || {
                if let Some(this) = weak.load()
                    && let Some(delegate) = this.delegate()
                {
                    delegate.find_bar_did_request_advance(&this, true);
                }
            },
            mtm,
        );
        ivars.actions.borrow_mut().extend([previous.clone(), next.clone()]);

        let previous_button =
            PanelButton::symbol("chevron.up", "Previous match", &previous, 14.0, weight_regular(), false, mtm);
        let next_button = PanelButton::symbol("chevron.down", "Next match", &next, 14.0, weight_regular(), false, mtm);
        *ivars.previous_button.borrow_mut() = Some(previous_button.clone());
        *ivars.next_button.borrow_mut() = Some(next_button.clone());
        let find_row = &ivars.find_row;
        find_row.setTranslatesAutoresizingMaskIntoConstraints(false);
        find_row.setWantsLayer(true);
        find_row.setHuggingPriority_forOrientation(NSLayoutPriorityDefaultLow, NSLayoutConstraintOrientation::Horizontal);

        // One row for both presentations: the field stretches; the count and
        // the walk chevrons sit as one tray split off by a hairline, so the
        // row reads as a single control; options (and, for the floating bar,
        // the close key) trail after (§9.4).
        find_row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        find_row.setSpacing(6.0);
        find_row.setAlignment(NSLayoutAttribute::CenterY);
        let trailer_divider = &ivars.trailer_divider;
        trailer_divider.setWantsLayer(true);
        if let Some(layer) = trailer_divider.layer() {
            layer.setCornerRadius(1.0);
        }
        trailer_divider.widthAnchor().constraintEqualToConstant(1.0).setActive(true);
        trailer_divider.setIdentifier(Some(&NSString::from_str("find-divider")));
        trailer_divider.heightAnchor().constraintEqualToConstant(18.0).setActive(true);

        let options_divider = &ivars.options_divider;
        options_divider.setWantsLayer(true);
        if let Some(layer) = options_divider.layer() {
            layer.setCornerRadius(1.0);
        }
        options_divider.setIdentifier(Some(&NSString::from_str("find-divider")));
        options_divider.widthAnchor().constraintEqualToConstant(1.0).setActive(true);
        options_divider.heightAnchor().constraintEqualToConstant(18.0).setActive(true);

        let mut row_views: Vec<Retained<NSView>> = vec![
            Retained::into_super(Retained::into_super(ivars.leading_glyph.clone())),
            Retained::into_super(Retained::into_super(Retained::into_super(Retained::into_super(
                ivars.search_field.clone(),
            )))),
            Retained::into_super(Retained::into_super(ivars.warning_image.clone())),
            Retained::into_super(Retained::into_super(ivars.status_label.clone())),
            ivars.trailer_divider.clone(),
            Retained::into_super(Retained::into_super(previous_button)),
            Retained::into_super(Retained::into_super(next_button)),
            ivars.options_divider.clone(),
            Retained::into_super(Retained::into_super(ivars.options_button.clone())),
        ];
        if ivars.presentation == Presentation::Bar {
            let weak: ObjcWeak<FindBarView> = ObjcWeak::from(self);
            let close = ButtonAction::new(
                move || {
                    if let Some(this) = weak.load()
                        && let Some(delegate) = this.delegate()
                    {
                        delegate.find_bar_did_request_close(&this);
                    }
                },
                mtm,
            );
            ivars.actions.borrow_mut().push(close.clone());
            let close_button =
                PanelButton::symbol("xmark", "Close find bar", &close, 14.0, weight_regular(), false, mtm);
            *ivars.close_button.borrow_mut() = Some(close_button.clone());
            row_views.push(Retained::into_super(Retained::into_super(close_button)));
        }
        for view in &row_views {
            find_row.addArrangedSubview(view);
        }
    }

    /// `makeOptionsMenuForTesting()`.
    pub fn make_options_menu_for_testing(&self) -> Retained<NSMenu> {
        let mtm = self.mtm();
        let ivars = self.ivars();
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &ns_string("Find Options"));
        menu.addItem(&self.option_item(
            "Regular Expression",
            sel!(toggleRegexOption:),
            ivars.regex_toggle.state(),
            true,
        ));
        menu.addItem(&self.option_item("Match Case", sel!(toggleCaseOption:), ivars.case_toggle.state(), true));
        menu.addItem(&self.option_item("Whole Word", sel!(toggleWordOption:), ivars.word_toggle.state(), true));
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&self.option_item(
            "In Selection",
            sel!(toggleScopeOption:),
            ivars.scope_toggle.state(),
            ivars.selection_scope.get().is_some(),
        ));
        menu
    }

    fn option_item(&self, title: &str, action: Sel, state: NSControlStateValue, enabled: bool) -> Retained<NSMenuItem> {
        // SAFETY: the action is one of this class's own methods.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(self.mtm()),
                &ns_string(title),
                Some(action),
                &ns_string(""),
            )
        };
        // SAFETY: the bar outlives the menu it builds for its own options.
        unsafe { item.setTarget(Some(object(self))) };
        item.setState(state);
        item.setEnabled(enabled);
        item
    }

    fn show_options_menu(&self) {
        let menu = self.make_options_menu_for_testing();
        let options_button = self.ivars().options_button.clone();
        let bounds = options_button.bounds();
        menu.popUpMenuPositioningItem_atLocation_inView(
            None,
            NSPoint::new(bounds.min_x(), bounds.max_y() + 2.0),
            Some(&options_button),
        );
    }

    fn toggle(&self, control: &NSButton) {
        control.setState(if control.state() == NSControlStateValueOn {
            NSControlStateValueOff
        } else {
            NSControlStateValueOn
        });
        self.emit_query();
    }

    fn build_replace_row(&self) {
        let mtm = self.mtm();
        let ivars = self.ivars();
        let replace_field = &ivars.replace_field;
        replace_field.setPlaceholderString(Some(&ns_string("Replace")));
        replace_field.setFont(Some(&PanelFont::row()));
        replace_field.setControlSize(NSControlSize::Small);
        replace_field.setFocusRingType(NSFocusRingType::None);
        // SAFETY: the bar owns the field and outlives it.
        unsafe { replace_field.setDelegate(Some(ProtocolObject::from_ref(self))) };
        set_label(&**replace_field, "Replace with");
        replace_field.setContentHuggingPriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );

        let weak: ObjcWeak<FindBarView> = ObjcWeak::from(self);
        let replace = ButtonAction::new(
            move || {
                if let Some(this) = weak.load()
                    && let Some(delegate) = this.delegate()
                {
                    let replacement = this.ivars().replace_field.stringValue().to_string();
                    delegate.find_bar_did_request_replace(&this, &replacement, false);
                }
            },
            mtm,
        );
        let weak: ObjcWeak<FindBarView> = ObjcWeak::from(self);
        let replace_all = ButtonAction::new(
            move || {
                if let Some(this) = weak.load()
                    && let Some(delegate) = this.delegate()
                {
                    let replacement = this.ivars().replace_field.stringValue().to_string();
                    delegate.find_bar_did_request_replace(&this, &replacement, true);
                }
            },
            mtm,
        );
        ivars.actions.borrow_mut().extend([replace.clone(), replace_all.clone()]);

        let replace_button = PanelButton::text("Replace", &replace, false, mtm);
        let replace_all_button = PanelButton::text("All", &replace_all, false, mtm);
        *ivars.replace_button.borrow_mut() = Some(replace_button.clone());
        *ivars.replace_all_button.borrow_mut() = Some(replace_all_button.clone());

        let replace_row = &ivars.replace_row;
        replace_row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        replace_row.setSpacing(6.0);
        replace_row.setAlignment(NSLayoutAttribute::CenterY);
        replace_row.setTranslatesAutoresizingMaskIntoConstraints(false);
        replace_row.setWantsLayer(true);
        replace_row.addArrangedSubview(replace_field);
        replace_row.addArrangedSubview(&replace_button);
        replace_row.addArrangedSubview(&replace_all_button);
    }

    // MARK: - Properties

    pub fn delegate(&self) -> Option<Rc<dyn FindBarDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn FindBarDelegate>>) {
        *self.ivars().delegate.borrow_mut() = delegate;
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet.clone();
        self.ivars().backdrop.set_style_sheet(style_sheet.clone());
        let glass = self.ivars().glass.borrow().clone();
        if let Some(glass) = glass {
            glass.set_style_sheet(style_sheet);
        }
        self.apply_style();
    }

    pub fn shows_replace(&self) -> bool {
        self.ivars().shows_replace.get()
    }

    pub fn set_shows_replace(&self, shows_replace: bool) {
        let old_value = self.ivars().shows_replace.replace(shows_replace);
        if shows_replace == old_value {
            return;
        }
        self.set_replace_row_visible(shows_replace);
    }

    /// Animates the replace row into and out of the bar. Outside a window
    /// (measurement during tests) and under Reduce Motion the row simply
    /// appears or disappears.
    fn set_replace_row_visible(&self, visible: bool) {
        let ivars = self.ivars();
        // A two-row bar needs the denser native control material.
        let glass = ivars.glass.borrow().clone();
        if let Some(glass) = glass {
            glass.set_tint(if visible { Tint::Control } else { Tint::Panel });
        }

        ivars.replace_transition_generation.set(ivars.replace_transition_generation.get().wrapping_add(1));
        let generation = ivars.replace_transition_generation.get();
        if let Some(layer) = ivars.replace_row.layer() {
            layer.removeAllAnimations();
        }

        if !(self.window().is_some() && !self.style_sheet().reduce_motion) {
            ivars.replace_row.setHidden(!visible);
            ivars.replace_row.setAlphaValue(1.0);
            if let Some(layer) = ivars.replace_row.layer() {
                layer.setAffineTransform(IDENTITY);
            }
            self.invalidateIntrinsicContentSize();
            return;
        }
        let this = self.retain();
        if visible {
            ivars.replace_row.setHidden(false);
            ivars.replace_row.setAlphaValue(0.0);
            if let Some(layer) = ivars.replace_row.layer() {
                layer.setAffineTransform(translation(0.0, -5.0));
            }
            self.glide_bar_height();
            motion::run(
                false,
                motion::STANDARD,
                Curve::EaseOut,
                move |_| {
                    let replace_row = &this.ivars().replace_row;
                    set_alpha_animated(replace_row, 1.0);
                    if let Some(layer) = replace_row.layer() {
                        layer.setAffineTransform(IDENTITY);
                    }
                },
                None,
            );
        } else {
            let weak: ObjcWeak<FindBarView> = ObjcWeak::from(self);
            motion::run(
                false,
                motion::QUICK,
                Curve::EaseOut,
                move |_| {
                    let replace_row = &this.ivars().replace_row;
                    set_alpha_animated(replace_row, 0.0);
                    if let Some(layer) = replace_row.layer() {
                        layer.setAffineTransform(translation(0.0, -5.0));
                    }
                },
                Some(Box::new(move || {
                    let Some(this) = weak.load() else { return };
                    let ivars = this.ivars();
                    if !(ivars.replace_transition_generation.get() == generation && !ivars.shows_replace.get()) {
                        return;
                    }
                    ivars.replace_row.setHidden(true);
                    ivars.replace_row.setAlphaValue(1.0);
                    if let Some(layer) = ivars.replace_row.layer() {
                        layer.setAffineTransform(IDENTITY);
                    }
                    this.glide_bar_height();
                })),
            );
        }
    }

    /// The pill's height follows its rows with a glide, not a jump.
    fn glide_bar_height(&self) {
        self.invalidateIntrinsicContentSize();
        if self.ivars().presentation != Presentation::Bar {
            return;
        }
        let Some(content) = self.window().and_then(|window| window.contentView()) else { return };
        let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
            // SAFETY: AppKit hands the block a live context.
            let context = unsafe { context.as_ref() };
            context.setDuration(motion::STANDARD);
            context.setTimingFunction(Some(&motion::timing(Curve::Structural)));
            context.setAllowsImplicitAnimation(true);
            content.layoutSubtreeIfNeeded();
        });
        NSAnimationContext::runAnimationGroup(&changes);
    }

    pub fn status_text(&self) -> String {
        self.ivars().status_text.borrow().clone()
    }

    pub fn set_status_text(&self, status_text: &str) {
        let old_value = self.ivars().status_text.replace(status_text.to_owned());
        if swift::str_eq(status_text, &old_value) {
            return;
        }
        self.set_status_label_text(status_text);
        set_label(&*self.ivars().status_label, if status_text.is_empty() { "No search" } else { status_text });
        self.apply_status_color();
        self.update_control_enablement();
    }

    /// The count crossfades rather than swapping mid-read, and the tray it
    /// lives in glides open and closed through the stack's own member
    /// animation.
    fn set_status_label_text(&self, text: &str) {
        let status_label = self.ivars().status_label.clone();
        let becoming_hidden = text.is_empty();
        let live = self.window().is_some() && !self.style_sheet().reduce_motion;
        if !(live && becoming_hidden != status_label.isHidden()) {
            // A count arriving while its own fade-out is still running wins:
            // the fade is retired before it can park the label at alpha 0.
            if live {
                if let Some(layer) = status_label.layer() {
                    layer.removeAllAnimations();
                }
                if !becoming_hidden && !swift::str_eq(&status_label.stringValue().to_string(), text) {
                    let fade = CATransition::new();
                    // SAFETY: Core Animation exports the transition types as
                    // immutable globals.
                    fade.setType(unsafe { kCATransitionFade });
                    fade.setDuration(motion::QUICK);
                    fade.setTimingFunction(Some(&motion::timing(Curve::EaseOut)));
                    if let Some(layer) = status_label.layer() {
                        layer.addAnimation_forKey(&fade, Some(&NSString::from_str("find-status")));
                    }
                }
            }
            status_label.setStringValue(&ns_string(text));
            status_label.setHidden(becoming_hidden);
            status_label.setAlphaValue(1.0);
            return;
        }
        if becoming_hidden {
            let weak: ObjcWeak<FindBarView> = ObjcWeak::from(self);
            let fading = status_label.clone();
            motion::run(
                false,
                motion::QUICK,
                Curve::Decelerate,
                move |_| set_alpha_animated(&fading, 0.0),
                Some(Box::new(move || {
                    let Some(this) = weak.load() else { return };
                    let status_label = this.ivars().status_label.clone();
                    if !this.ivars().status_text.borrow().is_empty() {
                        if let Some(layer) = status_label.layer() {
                            layer.removeAllAnimations();
                        }
                        status_label.setAlphaValue(1.0);
                        return;
                    }
                    status_label.setStringValue(&ns_string(""));
                    let inner = this.clone();
                    motion::run(
                        false,
                        motion::QUICK,
                        Curve::Decelerate,
                        move |_| {
                            let animator = inner.ivars().status_label.animator();
                            let _: () = unsafe { msg_send![&*animator, setHidden: true] };
                            inner.layoutSubtreeIfNeeded();
                        },
                        None,
                    );
                    status_label.setAlphaValue(1.0);
                })),
            );
        } else {
            if let Some(layer) = status_label.layer() {
                layer.removeAllAnimations();
            }
            status_label.setStringValue(&ns_string(text));
            status_label.setAlphaValue(0.0);
            status_label.setHidden(false);
            let this = self.retain();
            motion::run(
                false,
                motion::QUICK,
                Curve::Decelerate,
                move |_| {
                    set_alpha_animated(&this.ivars().status_label, 1.0);
                    this.layoutSubtreeIfNeeded();
                },
                None,
            );
        }
    }

    pub fn is_query_valid(&self) -> bool {
        self.ivars().is_query_valid.get()
    }

    pub fn set_is_query_valid(&self, is_query_valid: bool) {
        let old_value = self.ivars().is_query_valid.replace(is_query_valid);
        if is_query_valid == old_value {
            return;
        }
        self.apply_validity();
    }

    /// Range the "in selection" toggle scopes to.  The host pushes it in; the
    /// toggle is disabled while it is nil.
    pub fn selection_scope(&self) -> Option<NSRange> {
        self.ivars().selection_scope.get()
    }

    pub fn set_selection_scope(&self, scope: Option<NSRange>) {
        let ivars = self.ivars();
        ivars.selection_scope.set(scope);
        ivars.scope_toggle.setEnabled(scope.is_some());
        if scope.is_none() && ivars.scope_toggle.state() == NSControlStateValueOn {
            ivars.scope_toggle.setState(NSControlStateValueOff);
            self.emit_query();
        }
    }

    // MARK: - API

    fn intrinsic_content_size(&self) -> NSSize {
        // SAFETY: AppKit exports the metric as an immutable global.
        let none = unsafe { NSViewNoIntrinsicMetric };
        match self.ivars().presentation {
            Presentation::Bar => NSSize::new(
                none,
                if self.shows_replace() { FindBarDensity::REPLACE_HEIGHT } else { FindBarDensity::BAR_HEIGHT },
            ),
            Presentation::Inspector => NSSize::new(none, none),
        }
    }

    /// `focusSearchField(selectAll:)`; Swift's default is `true`.
    pub fn focus_search_field(&self, select_all: bool) {
        let search_field = self.ivars().search_field.clone();
        if let Some(window) = self.window() {
            window.makeFirstResponder(Some(&search_field));
        }
        if select_all && let Some(editor) = search_field.currentEditor() {
            // SAFETY: the field editor is live while the field is editing.
            unsafe { editor.selectAll(None) };
        }
    }

    /// Keep labels and glyphs out of the material's topology change.
    pub fn prepare_for_liquid_entrance(&self) {
        let ivars = self.ivars();
        ivars.entrance_generation.set(ivars.entrance_generation.get().wrapping_add(1));
        if let Some(layer) = self.layer() {
            layer.removeAnimationForKey(&NSString::from_str("find-liquid-presence"));
        }
        if let Some(layer) = self.layer() {
            set_mask(&layer, None);
        }
        ivars.rows.setWantsLayer(true);
        if let Some(layer) = ivars.rows.layer() {
            layer.removeAnimationForKey(&NSString::from_str("find-content-arrival"));
        }
        ivars.rows.setAlphaValue(0.0);
        if let Some(layer) = ivars.rows.layer() {
            layer.setAffineTransform(translation(0.0, -2.0));
        }
    }

    /// Reveals the real glass body from a compact lens near the toolbar's
    /// Find control. `source` is in window coordinates.
    pub fn play_liquid_entrance(&self, source: Option<NSPoint>) {
        self.layoutSubtreeIfNeeded();
        let bounds = self.bounds();
        let eligible = !self.style_sheet().reduce_motion
            && self.window().is_some()
            && bounds.width() > 1.0
            && bounds.height() > 1.0;
        let Some(layer) = self.layer().filter(|_| eligible) else {
            if let Some(layer) = self.layer() {
                set_mask(&layer, None);
            }
            self.play_liquid_entrance_content();
            return;
        };

        let seed_x = source.map(|point| self.convertPoint_fromView(point, None).x).unwrap_or(bounds.max_x());
        let clamped_x = smin(bounds.max_x() - 17.0, smax(bounds.min_x() + 17.0, seed_x));
        let start = self.liquid_entrance_rect(34.0, smin(34.0, bounds.height()), clamped_x);
        let first_pour = self.liquid_entrance_rect(
            bounds.width() * 0.42,
            bounds.height() * 0.82,
            bounds.max_x() - bounds.width() * 0.21,
        );
        let overshoot = bounds.inset_by(-2.5, 1.0);
        let paths = [
            PanelMetrics::continuous_rounded_path(start, start.height() / 2.0),
            PanelMetrics::continuous_rounded_path(first_pour, first_pour.height() / 2.0),
            PanelMetrics::continuous_rounded_path(overshoot, PanelMetrics::CHROME_PILL_RADIUS + 1.0),
            PanelMetrics::continuous_rounded_path(bounds, PanelMetrics::CHROME_PILL_RADIUS),
        ];
        let entrance_mask = self.ivars().entrance_mask.clone();
        entrance_mask.setFrame(bounds);
        entrance_mask.setFillColor(Some(&cg(&NSColor::blackColor())));
        entrance_mask.setPath(paths.last().map(|path| &**path));
        null_actions(&entrance_mask, &["path", "frame"]);
        set_mask(&layer, Some(&entrance_mask));

        let reveal = CAKeyframeAnimation::animationWithKeyPath(Some(&NSString::from_str("path")));
        let values = path_array(&paths);
        // SAFETY: an animation's values are Objective-C objects.
        unsafe { reveal.setValues(Some(&values)) };
        reveal.setKeyTimes(Some(&numbers(&[0.0, 0.46, 0.82, 1.0])));
        reveal.setTimingFunctions(Some(&NSArray::from_retained_slice(&[
            motion::timing(Curve::Structural),
            motion::timing(Curve::Structural),
            motion::timing(Curve::Decelerate),
        ])));
        let presence = CAKeyframeAnimation::animationWithKeyPath(Some(&NSString::from_str("opacity")));
        let presence_values = numbers(&[0.62, 0.94, 1.0]);
        // SAFETY: as above.
        unsafe { presence.setValues(Some(Retained::cast_unchecked::<NSArray>(presence_values).as_ref())) };
        presence.setKeyTimes(Some(&numbers(&[0.0, 0.58, 1.0])));
        presence.setTimingFunctions(Some(&NSArray::from_retained_slice(&[
            motion::timing(Curve::EaseOut),
            motion::timing(Curve::Decelerate),
        ])));
        let group = CAAnimationGroup::animation();
        let animations: Retained<NSArray<CAAnimation>> = NSArray::from_retained_slice(&[
            Retained::into_super(Retained::into_super(reveal)),
            Retained::into_super(Retained::into_super(presence)),
        ]);
        group.setAnimations(Some(&animations));
        group.setDuration(motion::LIQUID_SETTLE);
        CATransaction::begin();
        let weak: ObjcWeak<FindBarView> = ObjcWeak::from(self);
        let completion = RcBlock::new(move || {
            if let Some(this) = weak.load()
                && let Some(layer) = this.layer()
            {
                set_mask(&layer, None);
            }
        });
        // SAFETY: the block only touches the bar on the main thread.
        unsafe { CATransaction::setCompletionBlock(Some(&completion)) };
        layer.addAnimation_forKey(&group, Some(&NSString::from_str("find-liquid-presence")));
        CATransaction::commit();
        self.play_liquid_entrance_content();
    }

    fn liquid_entrance_rect(&self, width: CGFloat, height: CGFloat, center_x: CGFloat) -> NSRect {
        let bounds = self.bounds();
        let width = smin(bounds.width(), smax(1.0, width));
        let height = smin(bounds.height(), smax(1.0, height));
        let origin_x = smin(bounds.max_x() - width, smax(bounds.min_x(), center_x - width / 2.0));
        rect(origin_x, bounds.mid_y() - height / 2.0, width, height)
    }

    pub fn play_liquid_entrance_content(&self) {
        let ivars = self.ivars();
        let generation = ivars.entrance_generation.get();
        let eligible = !self.style_sheet().reduce_motion && self.window().is_some();
        let Some(layer) = ivars.rows.layer().filter(|_| eligible) else {
            ivars.rows.setAlphaValue(1.0);
            if let Some(layer) = ivars.rows.layer() {
                layer.setAffineTransform(IDENTITY);
            }
            return;
        };

        let opacity = CAKeyframeAnimation::animationWithKeyPath(Some(&NSString::from_str("opacity")));
        let opacity_values = numbers(&[0.0, 0.72, 1.0]);
        // SAFETY: an animation's values are Objective-C objects.
        unsafe { opacity.setValues(Some(Retained::cast_unchecked::<NSArray>(opacity_values).as_ref())) };
        opacity.setKeyTimes(Some(&numbers(&[0.0, 0.58, 1.0])));
        let transform = CAKeyframeAnimation::animationWithKeyPath(Some(&NSString::from_str("transform")));
        let transform_values = NSArray::from_retained_slice(&[
            transform_value(CATransform3D::new_translation(0.0, -2.0, 0.0)),
            transform_value(CATransform3D::new_translation(0.0, 0.5, 0.0)),
            // SAFETY: Core Animation exports the identity as an immutable global.
            transform_value(unsafe { CATransform3DIdentity }),
        ]);
        // SAFETY: as above.
        unsafe { transform.setValues(Some(Retained::cast_unchecked::<NSArray>(transform_values).as_ref())) };
        transform.setKeyTimes(Some(&numbers(&[0.0, 0.72, 1.0])));
        let group = CAAnimationGroup::animation();
        let animations: Retained<NSArray<CAAnimation>> = NSArray::from_retained_slice(&[
            Retained::into_super(Retained::into_super(opacity)),
            Retained::into_super(Retained::into_super(transform)),
        ]);
        group.setAnimations(Some(&animations));
        group.setDuration(motion::STANDARD);
        group.setBeginTime(CACurrentMediaTime() + motion::FLOATING_CONTENT_REVEAL_LEAD);
        // SAFETY: Core Animation exports the fill modes as immutable globals.
        group.setFillMode(unsafe { kCAFillModeBackwards });
        group.setTimingFunction(Some(&motion::timing(Curve::Decelerate)));
        CATransaction::begin();
        let weak: ObjcWeak<FindBarView> = ObjcWeak::from(self);
        let completion = RcBlock::new(move || {
            let Some(this) = weak.load() else { return };
            if this.ivars().entrance_generation.get() != generation {
                return;
            }
            if let Some(layer) = this.ivars().rows.layer() {
                layer.removeAnimationForKey(&NSString::from_str("find-content-arrival"));
            }
        });
        // SAFETY: the block only touches the bar on the main thread.
        unsafe { CATransaction::setCompletionBlock(Some(&completion)) };
        ivars.rows.setAlphaValue(1.0);
        layer.setAffineTransform(IDENTITY);
        layer.addAnimation_forKey(&group, Some(&NSString::from_str("find-content-arrival")));
        CATransaction::commit();
    }

    pub fn cancel_liquid_entrance(&self) {
        let ivars = self.ivars();
        ivars.entrance_generation.set(ivars.entrance_generation.get().wrapping_add(1));
        if let Some(layer) = self.layer() {
            layer.removeAnimationForKey(&NSString::from_str("find-liquid-presence"));
        }
        if let Some(layer) = self.layer() {
            set_mask(&layer, None);
        }
        if let Some(layer) = ivars.rows.layer() {
            layer.removeAnimationForKey(&NSString::from_str("find-content-arrival"));
        }
        ivars.rows.setAlphaValue(1.0);
        if let Some(layer) = ivars.rows.layer() {
            layer.setAffineTransform(IDENTITY);
        }
    }

    /// Stop an in-flight entrance without snapping the pill back to its
    /// identity transform.
    pub fn prepare_for_liquid_exit(&self) {
        let ivars = self.ivars();
        ivars.entrance_generation.set(ivars.entrance_generation.get().wrapping_add(1));
        if let Some(layer) = self.layer() {
            if let Some(presentation) = layer.__presentation() {
                CATransaction::begin();
                CATransaction::setDisableActions(true);
                // Use the same 3-D key path the exit animation drives.
                layer.setTransform(presentation.transform());
                layer.setOpacity(presentation.opacity());
                CATransaction::commit();
            }
            layer.removeAnimationForKey(&NSString::from_str("find-liquid-presence"));
            set_mask(&layer, None);
        }
        if let Some(rows_layer) = ivars.rows.layer() {
            if let Some(presentation) = rows_layer.__presentation() {
                CATransaction::begin();
                CATransaction::setDisableActions(true);
                rows_layer.setTransform(presentation.transform());
                rows_layer.setOpacity(presentation.opacity());
                CATransaction::commit();
            }
            rows_layer.removeAnimationForKey(&NSString::from_str("find-content-arrival"));
        }
    }

    /// Used by "Use Selection for Find" (§7.2): the host pushes text in and
    /// the bar behaves exactly as if it had been typed. Swift's default for
    /// `notify` is `true`.
    pub fn set_query_text(&self, text: &str, notify: bool) {
        self.ivars().search_field.setStringValue(&ns_string(text));
        self.update_control_enablement();
        if notify {
            self.emit_query();
        }
    }

    /// Buttons that walk or rewrite matches only mean something while there
    /// is a query to walk.
    fn update_control_enablement(&self) {
        let ivars = self.ivars();
        let can_navigate = ivars.search_field.stringValue().length() != 0
            && !swift::str_eq(&ivars.status_text.borrow(), "No matches");
        for button in [&ivars.previous_button, &ivars.next_button, &ivars.replace_button, &ivars.replace_all_button] {
            let button = button.borrow().clone();
            if let Some(button) = button {
                button.setEnabled(can_navigate);
            }
        }
    }

    /// `currentQuery`: `var query = FindQuery()`, then field by field.
    #[allow(clippy::field_reassign_with_default)]
    pub fn current_query(&self) -> FindQuery {
        let ivars = self.ivars();
        let mut query = FindQuery::default();
        query.text = ivars.search_field.stringValue().to_string();
        query.is_regex = ivars.regex_toggle.state() == NSControlStateValueOn;
        query.case_sensitive = ivars.case_toggle.state() == NSControlStateValueOn;
        query.whole_word = ivars.word_toggle.state() == NSControlStateValueOn;
        query.scope =
            if ivars.scope_toggle.state() == NSControlStateValueOn { ivars.selection_scope.get() } else { None };
        query
    }

    fn emit_query(&self) {
        if let Some(delegate) = self.delegate() {
            delegate.find_bar_did_change(self, self.current_query());
        }
    }

    // MARK: - Style

    fn apply_style(&self) {
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        if let Some(layer) = ivars.trailer_divider.layer() {
            layer.setBackgroundColor(Some(&cg(&style_sheet.rule)));
        }
        if let Some(layer) = ivars.options_divider.layer() {
            layer.setBackgroundColor(Some(&cg(&style_sheet.rule)));
        }
        ivars.leading_glyph.setContentTintColor(Some(&style_sheet.accent));
        let font = PanelFont::system_regular(13.5);
        let placeholder = attributed_string(
            "Find…",
            &[(keys::font(), object(&*font)), (keys::foreground_color(), object(&*style_sheet.text_secondary))],
        );
        ivars.search_field.setPlaceholderAttributedString(Some(&placeholder));
        let contrast = style_sheet.increase_contrast;
        if let Some(layer) = ivars.inspector_well.layer() {
            layer.setBackgroundColor(Some(&cg(&style_sheet.surface.panel_alpha(0.85, contrast))));
        }
        if let Some(layer) = ivars.inspector_well.layer() {
            layer.setBorderColor(Some(&cg(&style_sheet.rule.panel_alpha(0.9, contrast))));
        }
        if let Some(layer) = ivars.inspector_well.layer() {
            layer.setBorderWidth(PanelMetrics::HAIRLINE);
        }
        // Chrome glyphs share the quiet text tint.
        let buttons = [
            Some(ivars.options_button.clone()),
            ivars.previous_button.borrow().clone(),
            ivars.next_button.borrow().clone(),
            ivars.close_button.borrow().clone(),
        ];
        for button in buttons.into_iter().flatten() {
            button.setContentTintColor(Some(&style_sheet.text_secondary));
        }
        // The focus ring echoes the theme's accent.
        ivars.replace_field.set_ring_color(style_sheet.accent.clone());
        self.apply_status_color();
        ivars.warning_image.setContentTintColor(Some(&style_sheet.change_color(ChangeKind::Deleted)));
        self.apply_validity();
        self.setNeedsDisplay(true);
    }

    /// The count echoes the theme's accent once a match is pinned; a failure
    /// stays in the delete tone; a bare total stays secondary. Replace
    /// confirmations read as success (§9.4).
    fn apply_status_color(&self) {
        let style_sheet = self.style_sheet();
        let status_text = self.ivars().status_text.borrow().clone();
        let color = if swift::str_eq(&status_text, "No matches") {
            style_sheet.change_color(ChangeKind::Deleted)
        } else if swift::contains(&status_text, " of ") || swift::has_prefix(&status_text, "Replaced") {
            style_sheet.accent.clone()
        } else {
            style_sheet.text_secondary.clone()
        };
        self.ivars().status_label.setTextColor(Some(&color));
    }

    /// Subtle by design: an invalid pattern while you are still typing one is
    /// the normal case, not a failure state.
    fn apply_validity(&self) {
        let ivars = self.ivars();
        let show = !ivars.is_query_valid.get();
        if show == ivars.warning_image.isHidden() {
            self.set_warning_visible(show);
        }
        let style_sheet = self.style_sheet();
        let color = if ivars.is_query_valid.get() {
            style_sheet.text.clone()
        } else {
            style_sheet.change_color(ChangeKind::Deleted)
        };
        ivars.search_field.setTextColor(Some(&color));
    }

    fn set_warning_visible(&self, visible: bool) {
        let warning_image = self.ivars().warning_image.clone();
        let live = self.window().is_some() && !self.style_sheet().reduce_motion;
        if visible {
            if let Some(layer) = warning_image.layer() {
                layer.removeAllAnimations();
            }
            warning_image.setHidden(false);
            if !live {
                return;
            }
            warning_image.setAlphaValue(0.0);
            if let Some(layer) = warning_image.layer() {
                layer.setAffineTransform(scale(0.6, 0.6));
            }
            let this = self.retain();
            motion::run(
                false,
                motion::STANDARD,
                Curve::Snap,
                move |_| {
                    let warning_image = &this.ivars().warning_image;
                    set_alpha_animated(warning_image, 1.0);
                    if let Some(layer) = warning_image.layer() {
                        layer.setAffineTransform(IDENTITY);
                    }
                    this.layoutSubtreeIfNeeded();
                },
                None,
            );
        } else {
            if !live {
                warning_image.setHidden(true);
                return;
            }
            let fading = warning_image.clone();
            let weak: ObjcWeak<FindBarView> = ObjcWeak::from(self);
            motion::run(
                false,
                motion::QUICK,
                Curve::Decelerate,
                move |_| set_alpha_animated(&fading, 0.0),
                Some(Box::new(move || {
                    let Some(this) = weak.load() else { return };
                    if !this.ivars().is_query_valid.get() {
                        return;
                    }
                    let warning_image = &this.ivars().warning_image;
                    if let Some(layer) = warning_image.layer() {
                        layer.removeAllAnimations();
                    }
                    warning_image.setHidden(true);
                    warning_image.setAlphaValue(1.0);
                    if let Some(layer) = warning_image.layer() {
                        layer.setAffineTransform(IDENTITY);
                    }
                })),
            );
        }
    }

    // MARK: - Field editing helpers

    /// `(notification.object as? NSSearchField) === searchField`.
    fn is_search_field(&self, object: Option<&AnyObject>) -> bool {
        let Some(object) = object else { return false };
        let Some(field) = object.downcast_ref::<NSSearchField>() else { return false };
        let search_field: &NSSearchField = &self.ivars().search_field;
        std::ptr::eq(field, search_field)
    }

    fn editing_changed(&self, notification: &NSNotification, began: bool) {
        let object = notification.object();
        if let Some(control) = object.as_deref().and_then(|object| object.downcast_ref::<NSControl>()) {
            needs_display(control);
        }
        if self.is_search_field(object.as_deref()) {
            let glass = self.ivars().glass.borrow().clone();
            if let Some(glass) = glass {
                glass.set_shows_focus(began);
            }
        }
    }

    fn do_command(&self, control: &NSControl, selector: Sel) -> bool {
        if selector == sel!(insertNewline:) {
            let replace_field: &NSControl = &self.ivars().replace_field;
            if std::ptr::eq(control, replace_field) {
                if let Some(delegate) = self.delegate() {
                    let replacement = self.ivars().replace_field.stringValue().to_string();
                    delegate.find_bar_did_request_replace(self, &replacement, false);
                }
            } else {
                // ⏎ advances, ⇧⏎ goes back — the platform's find-bar idiom.
                let backwards = NSApplication::sharedApplication(self.mtm())
                    .currentEvent()
                    .is_some_and(|event| event.modifierFlags().contains(NSEventModifierFlags::Shift));
                if let Some(delegate) = self.delegate() {
                    delegate.find_bar_did_request_advance(self, !backwards);
                }
            }
            true
        } else if selector == sel!(cancelOperation:) {
            if let Some(delegate) = self.delegate() {
                delegate.find_bar_did_request_close(self);
            }
            true
        } else {
            false
        }
    }

    // MARK: - Testing

    /// `dividerCountForTesting`.
    pub fn divider_count_for_testing(&self) -> usize {
        self.ivars()
            .find_row
            .arrangedSubviews()
            .iter()
            .filter(|view| view.identifier().is_some_and(|identifier| identifier.to_string() == "find-divider"))
            .count()
    }

    pub fn has_close_button_for_testing(&self) -> bool {
        self.ivars().close_button.borrow().is_some()
    }

    pub fn leading_glyph_is_accessible_for_testing(&self) -> bool {
        self.ivars().leading_glyph.isAccessibilityElement()
    }

    pub fn search_field_is_bezeled_for_testing(&self) -> bool {
        self.ivars().search_field.isBezeled()
    }

    pub fn find_row_frame_for_testing(&self) -> NSRect {
        let find_row = &self.ivars().find_row;
        self.convertRect_fromView(find_row.bounds(), Some(find_row))
    }

    pub fn replace_row_frame_for_testing(&self) -> NSRect {
        let replace_row = &self.ivars().replace_row;
        self.convertRect_fromView(replace_row.bounds(), Some(replace_row))
    }

    pub fn replace_row_alpha_for_testing(&self) -> CGFloat {
        self.ivars().replace_row.alphaValue()
    }

    pub fn replace_row_is_hidden_for_testing(&self) -> bool {
        self.ivars().replace_row.isHidden()
    }

    pub fn uses_dense_replace_material_for_testing(&self) -> bool {
        match self.ivars().glass.borrow().as_ref() {
            Some(glass) => glass.tint() == Tint::Control,
            None => false,
        }
    }
}

// MARK: - Small helpers

/// `view.animator().alphaValue = alpha`.
fn set_alpha_animated(view: &NSView, alpha: CGFloat) {
    let animator = view.animator();
    let _: () = unsafe { msg_send![&*animator, setAlphaValue: alpha] };
}

/// `CGAffineTransform(translationX:y:)`.
fn translation(x: CGFloat, y: CGFloat) -> CGAffineTransform {
    CGAffineTransform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: x, ty: y }
}

/// `CGAffineTransform(scaleX:y:)`.
fn scale(x: CGFloat, y: CGFloat) -> CGAffineTransform {
    CGAffineTransform { a: x, b: 0.0, c: 0.0, d: y, tx: 0.0, ty: 0.0 }
}

fn numbers(values: &[f64]) -> Retained<NSArray<NSNumber>> {
    let numbers: Vec<Retained<NSNumber>> = values.iter().map(|value| NSNumber::new_f64(*value)).collect();
    NSArray::from_retained_slice(&numbers)
}

/// `[CGPath]` bridged to an `NSArray` (a path keyframe's `values`).
fn path_array(paths: &[CFRetained<CGPath>]) -> Retained<NSArray> {
    let objects: Vec<Retained<AnyObject>> = paths
        .iter()
        // SAFETY: `CGPath` is a CF type, and CF types are Objective-C objects
        // when bridged.
        .map(|path| unsafe {
            Retained::retain(CFRetained::as_ptr(path).as_ptr().cast::<AnyObject>()).expect("non-null path")
        })
        .collect();
    NSArray::from_retained_slice(&objects)
}

fn transform_value(transform: CATransform3D) -> Retained<objc2_foundation::NSValue> {
    use objc2_quartz_core::NSValueCATransform3DAdditions;
    // SAFETY: a plain value conversion.
    unsafe { objc2_foundation::NSValue::valueWithCATransform3D(transform) }
}

#[allow(unused)]
fn _unused(_: &dyn CAMediaTiming, _: &dyn NSAccessibility) {}
