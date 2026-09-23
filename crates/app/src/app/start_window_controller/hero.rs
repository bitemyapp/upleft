//! The welcome column of the start window (`// MARK: - Hero`,
//! `// MARK: - Action button` and `// MARK: - Brand` of
//! `App/StartWindowController.swift`, with `KeycapBadgeField`):
//! `StartHeroView`, `StartActionButton`, `KeycapBadgeField` and
//! `BrandMarkView`.

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::{Allocated, Retained, Weak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSAttributedStringNSStringDrawing, NSButton, NSButtonType, NSColor, NSCompositingOperation,
    NSControl, NSCursor, NSEvent, NSFocusRingType, NSFont, NSGraphicsContext, NSImage, NSImageScaling,
    NSImageSymbolConfiguration, NSImageView, NSLayoutAttribute, NSLayoutConstraint, NSLayoutConstraintOrientation,
    NSLayoutPriorityDefaultLow, NSLayoutPriorityRequired, NSLineBreakMode, NSResponder, NSStackView,
    NSTextAlignment, NSTextField, NSTrackingArea, NSTrackingAreaOptions, NSUserInterfaceLayoutOrientation, NSView,
    NSWindowDidBecomeKeyNotification, NSWindowDidResignKeyNotification,
};
use objc2_core_foundation::{CGFloat, CGSize};
use objc2_core_graphics::CGContext;
use objc2_foundation::{NSBundle, NSNotification, NSNotificationCenter, NSNumber, NSOperationQueue, NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::kCACornerCurveContinuous;
use upleft_render::appkit_compat::{RECT_ZERO, RectExt, attributed_string, keys, ns_string};
use upleft_render::motion::{self, Curve};
use upleft_render::swift_compat::smax;
use upleft_render::theme::style_sheet::StyleSheet;

use super::recents::{IDENTITY, scale, stack_view};
use super::{KeycapFormatter, StartGuideOffer, StartLayout, StartWindowController, configure_passive_label};
use crate::panels::appkit_support::{activate, cg, role, set_help, set_label, set_role, weight_medium, weight_semibold};
use crate::panels::panel_chrome::PanelMetrics;
use crate::support::commands::Command;
use crate::support::keybindings::KeybindingStore;

/// A block-based `NotificationCenter` observer token.
type ObserverToken = Retained<ProtocolObject<dyn objc2_foundation::NSObjectProtocol>>;

fn remove_observer(token: &ObserverToken) {
    // SAFETY: the token came from the default centre.
    unsafe { NSNotificationCenter::defaultCenter().removeObserver(&*(Retained::as_ptr(token) as *const AnyObject)) };
}

/// Whether `view` is its window's first responder and the window is key
/// (`window?.firstResponder === self && window?.isKeyWindow == true`).
fn is_focused(view: &NSView) -> bool {
    let Some(window) = view.window() else { return false };
    let is_first_responder = window
        .firstResponder()
        .is_some_and(|responder| std::ptr::eq(Retained::as_ptr(&responder).cast::<AnyObject>(), (view as *const NSView).cast()));
    is_first_responder && window.isKeyWindow()
}

// MARK: - KeycapBadgeField

pub(super) struct KeycapBadgeFieldIvars {
    min_width_constraint: RefCell<Option<Retained<NSLayoutConstraint>>>,
}

define_class!(
    /// An optical, symmetrical keycap badge that draws its shortcut text
    /// perfectly centered horizontally and vertically with continuous
    /// rounded corners and smooth borders.
    // SAFETY: `initWithFrame:` sets the ivars, so every initialiser path
    // (`-init` included) creates a valid instance.
    #[unsafe(super(NSTextField, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "KeycapBadgeField"]
    #[ivars = KeycapBadgeFieldIvars]
    pub(super) struct KeycapBadgeField;

    unsafe impl NSObjectProtocol for KeycapBadgeField {}

    impl KeycapBadgeField {
        #[unsafe(method_id(initWithFrame:))]
        fn __init_with_frame(this: Allocated<Self>, frame: NSRect) -> Retained<Self> {
            let this = this.set_ivars(KeycapBadgeFieldIvars { min_width_constraint: RefCell::new(None) });
            let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
            this.setWantsLayer(true);
            if let Some(layer) = this.layer() {
                layer.setCornerRadius(5.0);
                // SAFETY: Core Animation's corner-curve constant.
                layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
                layer.setMasksToBounds(true);
            }
            this.setAlignment(NSTextAlignment::Center);
            this.setUsesSingleLineMode(true);
            this.setLineBreakMode(NSLineBreakMode::ByClipping);
            configure_passive_label(&this);
            let min_width = this.widthAnchor().constraintGreaterThanOrEqualToConstant(30.0);
            *this.ivars().min_width_constraint.borrow_mut() = Some(min_width.clone());
            min_width.setActive(true);
            this.heightAnchor().constraintEqualToConstant(20.0).setActive(true);
            let required = NSLayoutPriorityRequired;
            this.setContentHuggingPriority_forOrientation(required, NSLayoutConstraintOrientation::Horizontal);
            this.setContentCompressionResistancePriority_forOrientation(required, NSLayoutConstraintOrientation::Horizontal);
            this
        }

        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            let text_size = self.attributedStringValue().size();
            let width = smax(self.min_width(), text_size.width.ceil() + 12.0);
            NSSize::new(width, 20.0)
        }

        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, _dirty_rect: NSRect) {
            let string = self.attributedStringValue();
            if string.length() == 0 {
                return;
            }
            let text_size = string.size();
            let bounds = self.bounds();
            let centered = NSRect::new(
                NSPoint::new(((bounds.width() - text_size.width) / 2.0).floor(), ((bounds.height() - text_size.height) / 2.0).floor()),
                NSSize::new(text_size.width.ceil(), text_size.height.ceil()),
            );
            string.drawInRect(centered);
        }
    }
);

impl KeycapBadgeField {
    /// `KeycapBadgeField()`.
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<KeycapBadgeField> {
        unsafe { msg_send![Self::alloc(mtm), init] }
    }

    fn min_width(&self) -> CGFloat {
        self.ivars().min_width_constraint.borrow().as_ref().map_or(0.0, |constraint| constraint.constant())
    }

    /// `setMinWidth(_:)`.
    pub(super) fn set_min_width(&self, min_width: CGFloat) {
        let constraint = self.ivars().min_width_constraint.borrow().clone();
        if let Some(constraint) = constraint {
            constraint.setConstant(min_width);
        }
        self.invalidateIntrinsicContentSize();
    }
}

// MARK: - StartHeroView

pub(super) struct StartHeroViewIvars {
    open_button: Retained<StartActionButton>,
    new_button: Retained<StartActionButton>,
    guide_button: Option<Retained<StartActionButton>>,
    /// The action the window opens focused on.
    lead_button: Retained<StartActionButton>,
    brand_label: Retained<NSTextField>,
    title_label: Retained<NSTextField>,
    subtitle_label: Retained<NSTextField>,
    brand: Retained<BrandMarkView>,
    guide: StartGuideOffer,
    /// Whether this window has anything of the user's own to show.
    is_returning: bool,
    sheet: RefCell<Rc<StyleSheet>>,
}

define_class!(
    /// The welcome column: brand, one clear sentence, and two equally
    /// weighted entry paths. Dragging is communicated by the window-level
    /// overlay, not by a permanent instruction that competes with the actual
    /// actions.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "StartHeroView"]
    #[ivars = StartHeroViewIvars]
    pub(super) struct StartHeroView;

    unsafe impl NSObjectProtocol for StartHeroView {}
);

impl StartHeroView {
    /// `init(owner:guide:isReturning:sheet:)`.
    pub(super) fn new(
        owner: &StartWindowController,
        guide: StartGuideOffer,
        is_returning: bool,
        sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Retained<StartHeroView> {
        let brand_label = NSTextField::labelWithString(&NSString::from_str(""), mtm);
        let title_label = NSTextField::labelWithString(&NSString::from_str(""), mtm);
        let subtitle_label = NSTextField::wrappingLabelWithString(&NSString::from_str(""), mtm);
        let guide_leads = guide == StartGuideOffer::Primary;
        let open_button = StartActionButton::new(
            "Open File",
            "folder",
            Some(Command::Open),
            Kind::Secondary,
            sheet.clone(),
            owner,
            objc2::sel!(openPanel:),
            mtm,
        );
        let new_button = StartActionButton::new(
            "New Document",
            "doc",
            Some(Command::NewDocument),
            if guide_leads { Kind::Secondary } else { Kind::Primary },
            sheet.clone(),
            owner,
            objc2::sel!(newDocument:),
            mtm,
        );
        let guide_button = (guide != StartGuideOffer::Unavailable).then(|| {
            StartActionButton::new(
                "Take the Tour",
                "sparkles",
                None,
                if guide_leads { Kind::Primary } else { Kind::Secondary },
                sheet.clone(),
                owner,
                objc2::sel!(openGuide:),
                mtm,
            )
        });
        let lead_button = guide_button.clone().filter(|_| guide_leads).unwrap_or_else(|| new_button.clone());
        let brand = BrandMarkView::new(mtm);
        let this = Self::alloc(mtm).set_ivars(StartHeroViewIvars {
            open_button,
            new_button,
            guide_button,
            lead_button,
            brand_label,
            title_label,
            subtitle_label,
            brand,
            guide,
            is_returning,
            sheet: RefCell::new(sheet),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        let ivars = this.ivars();

        ivars.brand.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.brand_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.brand_label.setContentHuggingPriority_forOrientation(NSLayoutPriorityRequired, NSLayoutConstraintOrientation::Horizontal);
        configure_passive_label(&ivars.brand_label);

        let brand_row = stack_view(&[&ivars.brand, &ivars.brand_label], mtm);
        brand_row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        brand_row.setAlignment(NSLayoutAttribute::CenterY);
        brand_row.setSpacing(10.0);

        ivars.title_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.title_label.setMaximumNumberOfLines(1);
        configure_passive_label(&ivars.title_label);

        ivars.subtitle_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.subtitle_label.setFont(Some(&NSFont::systemFontOfSize_weight(13.5, crate::panels::appkit_support::weight_regular())));
        ivars.subtitle_label.setMaximumNumberOfLines(2);
        ivars.subtitle_label.setPreferredMaxLayoutWidth(StartLayout::CONTENT_WIDTH);
        configure_passive_label(&ivars.subtitle_label);

        // Peer entry paths, with exactly one carrying primary emphasis. On a
        // normal launch creation is the strongest next step; a first-launch
        // tour temporarily takes that role because it is the onboarding path.
        let mut action_views: Vec<&NSView> = vec![&ivars.open_button, &ivars.new_button];
        if let Some(guide_button) = &ivars.guide_button {
            action_views.push(guide_button);
        }
        let actions = stack_view(&action_views, mtm);
        actions.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        actions.setAlignment(NSLayoutAttribute::CenterY);
        actions.setSpacing(StartLayout::ACTION_SPACING);
        actions.setDistribution(objc2_app_kit::NSStackViewDistribution::Fill);

        let stack = stack_view(&[&brand_row, &ivars.title_label, &ivars.subtitle_label, &actions], mtm);
        stack.setTranslatesAutoresizingMaskIntoConstraints(false);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        stack.setAlignment(NSLayoutAttribute::Leading);
        stack.setSpacing(0.0);
        this.addSubview(&stack);

        // The two file actions are peer entry points, so they share a control
        // well. When no tour is present, they balance and fill the welcome
        // column.
        let button_width = match &ivars.guide_button {
            Some(guide_button) => smax(
                180.0,
                (StartLayout::CONTENT_WIDTH - 2.0 * StartLayout::ACTION_SPACING - guide_button.intrinsicContentSize().width)
                    / 2.0,
            ),
            None => StartLayout::ACTION_BUTTON_WIDTH,
        };
        let mut constraints = vec![
            ivars.brand.widthAnchor().constraintEqualToConstant(38.0),
            ivars.brand.heightAnchor().constraintEqualToConstant(38.0),
            ivars.open_button.heightAnchor().constraintEqualToConstant(StartLayout::BUTTON_HEIGHT),
            ivars.new_button.heightAnchor().constraintEqualToConstant(StartLayout::BUTTON_HEIGHT),
            ivars.open_button.widthAnchor().constraintEqualToConstant(button_width),
            ivars.new_button.widthAnchor().constraintEqualToConstant(button_width),
            ivars.title_label.widthAnchor().constraintLessThanOrEqualToConstant(StartLayout::CONTENT_WIDTH),
            ivars.subtitle_label.widthAnchor().constraintLessThanOrEqualToConstant(StartLayout::CONTENT_WIDTH),
            stack.leadingAnchor().constraintEqualToAnchor(&this.leadingAnchor()),
            stack.trailingAnchor().constraintEqualToAnchor(&this.trailingAnchor()),
            stack.topAnchor().constraintEqualToAnchor(&this.topAnchor()),
            stack.bottomAnchor().constraintEqualToAnchor(&this.bottomAnchor()),
        ];
        if let Some(guide_button) = &ivars.guide_button {
            // The tour carries no shortcut, so it sizes to its own label
            // rather than padding out to match the two file actions.
            constraints.push(guide_button.heightAnchor().constraintEqualToConstant(StartLayout::BUTTON_HEIGHT));
        }
        activate(&constraints);

        stack.setCustomSpacing_afterView(10.0, &brand_row);
        stack.setCustomSpacing_afterView(5.0, &ivars.title_label);
        stack.setCustomSpacing_afterView(18.0, &ivars.subtitle_label);

        let sheet = ivars.sheet.borrow().clone();
        this.apply(sheet);
        set_role(&*this, role::group());
        set_label(&*this, "Upleft welcome");
        this
    }

    /// `leadButton`.
    pub(super) fn lead_button(&self) -> Retained<StartActionButton> {
        self.ivars().lead_button.clone()
    }

    /// `refreshShortcuts()`: bindings are user-editable, so the hints on the
    /// buttons are re-read rather than baked in at build time.
    pub(super) fn refresh_shortcuts(&self) {
        self.ivars().open_button.refresh_shortcut();
        self.ivars().new_button.refresh_shortcut();
    }

    /// `apply(sheet:)`.
    pub(super) fn apply(&self, sheet: Rc<StyleSheet>) {
        *self.ivars().sheet.borrow_mut() = sheet.clone();
        let ivars = self.ivars();
        ivars.open_button.apply(sheet.clone());
        ivars.new_button.apply(sheet.clone());
        if let Some(guide_button) = &ivars.guide_button {
            guide_button.apply(sheet.clone());
        }
        ivars.brand_label.setAttributedStringValue(&attributed_string(
            "Upleft",
            &[(keys::font(), &NSFont::systemFontOfSize_weight(14.0, weight_semibold())), (keys::foreground_color(), &sheet.text)],
        ));
        // Three states, because a returning user does not need to be told how
        // to open a file. "Open a Markdown file" restated the button directly
        // under it, and a permanent instruction to someone on their fortieth
        // launch reads as an app that never noticed they had learned it.
        let (headline, subheadline) = match (ivars.guide, ivars.is_returning) {
            (StartGuideOffer::Primary, _) => (
                "Welcome to Upleft",
                "The tour is a real document. It explains the reading tools while you read it.",
            ),
            (_, true) => ("Pick up where you left off", "Read, edit, and review — all in one focused place."),
            (_, false) => ("Open a Markdown file", "Read, edit, and review it in one focused place."),
        };
        // SAFETY: AppKit's attribute-name constant.
        let kern = unsafe { objc2_app_kit::NSKernAttributeName };
        ivars.title_label.setAttributedStringValue(&attributed_string(
            headline,
            &[
                (keys::font(), &NSFont::systemFontOfSize_weight(24.0, weight_semibold())),
                (keys::foreground_color(), &sheet.text),
                (kern, &NSNumber::new_f64(-0.3)),
            ],
        ));
        ivars.subtitle_label.setStringValue(&ns_string(subheadline));
        ivars.subtitle_label.setTextColor(Some(&sheet.text_secondary));
        self.setNeedsDisplay(true);
    }
}

// MARK: - StartActionButton

/// `StartActionButton.Kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Primary,
    Secondary,
}

pub(super) struct StartActionButtonIvars {
    kind: Kind,
    /// The command this button runs, when it has one. The shortcut hint is
    /// read from the live binding table rather than written into the label:
    /// bindings are user-editable, and a stale "⌘O" is a lie.
    command: Option<Command>,
    shell: Retained<NSView>,
    icon_view: Retained<NSImageView>,
    title_label: Retained<NSTextField>,
    shortcut_label: Retained<KeycapBadgeField>,
    content_group: Retained<NSStackView>,
    shortcut_hint: RefCell<String>,
    is_hovered: Cell<bool>,
    is_pressed: Cell<bool>,
    sheet: RefCell<Rc<StyleSheet>>,
    key_observers: RefCell<Vec<ObserverToken>>,
}

impl Drop for StartActionButtonIvars {
    fn drop(&mut self) {
        for token in self.key_observers.get_mut().drain(..) {
            remove_observer(&token);
        }
    }
}

define_class!(
    /// A large, calm target with an icon, a label, and the shortcut visible
    /// at the far edge. The shell is the only painted thing — the button
    /// itself stays transparent — and pressed/hover/focus are all expressed
    /// on that shell so the state is one step instead of three. Pointer
    /// events use the simple down/dragged/up cycle rather than an
    /// event-draining loop, so the click resolves as soon as the user lets
    /// go.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set;
    // the overrides keep AppKit's signatures.
    #[unsafe(super(NSButton, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "StartActionButton"]
    #[ivars = StartActionButtonIvars]
    pub(super) struct StartActionButton;

    unsafe impl NSObjectProtocol for StartActionButton {}

    impl StartActionButton {
        #[unsafe(method(mouseDownCanMoveWindow))]
        fn __mouse_down_can_move_window(&self) -> bool {
            false
        }

        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            // fittingSize, not intrinsicContentSize: an NSTextFieldCell
            // reports its cell ~4pt wider than its intrinsic content size,
            // and the layout engine gives the label that wider frame.
            // Computing the button from the under-reported intrinsic made the
            // shell ~4pt too narrow — the label-to-shortcut gap collapsed
            // below its constant and the longer label clipped. fittingSize is
            // what the label actually renders at. The content group is
            // centered in the well, so intrinsic sizing is only used by the
            // optional tour button; Open/New receive equal wells from
            // StartHeroView.
            NSSize::new((self.ivars().content_group.fittingSize().width + 24.0).ceil(), 34.0)
        }

        #[unsafe(method_id(hitTest:))]
        fn __hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            let superview = unsafe { self.superview() };
            let local = self.convertPoint_fromView(point, superview.as_deref());
            self.bounds().contains_point(local).then(|| Retained::into_super(Retained::into_super(Retained::into_super(self.retain()))))
        }

        #[unsafe(method(resetCursorRects))]
        fn __reset_cursor_rects(&self) {
            self.addCursorRect_cursor(self.bounds(), &NSCursor::pointingHandCursor());
        }

        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, _event: &NSEvent) {
            if !self.isEnabled() {
                return;
            }
            if let Some(window) = self.window() {
                window.makeFirstResponder(Some(self));
            }
            self.ivars().is_pressed.set(true);
            self.ivars().is_hovered.set(true);
            self.update_surface(false);
        }

        #[unsafe(method(mouseDragged:))]
        fn __mouse_dragged(&self, event: &NSEvent) {
            let inside = self.bounds().contains_point(self.convertPoint_fromView(event.locationInWindow(), None));
            self.ivars().is_pressed.set(inside);
            self.ivars().is_hovered.set(inside);
            self.update_surface(false);
        }

        #[unsafe(method(mouseUp:))]
        fn __mouse_up(&self, event: &NSEvent) {
            let inside = self.bounds().contains_point(self.convertPoint_fromView(event.locationInWindow(), None));
            self.ivars().is_pressed.set(false);
            self.ivars().is_hovered.set(inside);
            self.update_surface(true);
            if inside {
                // SAFETY: the button's own action and target.
                unsafe { self.sendAction_to(self.action(), self.target().as_deref()) };
            }
        }

        #[unsafe(method(mouseEntered:))]
        fn __mouse_entered(&self, _event: &NSEvent) {
            if self.ivars().is_pressed.get() {
                return;
            }
            self.ivars().is_hovered.set(true);
            self.update_surface(true);
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, _event: &NSEvent) {
            if self.ivars().is_pressed.get() {
                return;
            }
            self.ivars().is_hovered.set(false);
            self.update_surface(true);
        }

        /// Custom mouseDown/Dragged/Up owns pressed state so AppKit
        /// highlight can't desync clicks.
        #[unsafe(method(highlight:))]
        fn __highlight(&self, _flag: bool) {}

        #[unsafe(method(acceptsFirstResponder))]
        fn __accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(becomeFirstResponder))]
        fn __become_first_responder(&self) -> bool {
            let result: bool = unsafe { msg_send![super(self), becomeFirstResponder] };
            self.update_surface(false);
            result
        }

        #[unsafe(method(resignFirstResponder))]
        fn __resign_first_responder(&self) -> bool {
            let result: bool = unsafe { msg_send![super(self), resignFirstResponder] };
            self.update_surface(false);
            result
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn __view_did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            let previous: Vec<ObserverToken> = self.ivars().key_observers.borrow_mut().drain(..).collect();
            for token in &previous {
                remove_observer(token);
            }
            let Some(window) = self.window() else { return };
            // Token-based so cleanup touches only the two observers we own.
            let observers = [unsafe { NSWindowDidBecomeKeyNotification }, unsafe { NSWindowDidResignKeyNotification }]
                .into_iter()
                .map(|name| {
                    let weak: Weak<StartActionButton> = Weak::from(self);
                    let block = RcBlock::new(move |_notification: std::ptr::NonNull<NSNotification>| {
                        if let Some(this) = weak.load() {
                            this.window_key_state_changed();
                        }
                    });
                    // SAFETY: the block only touches the button on the main
                    // queue it is delivered on.
                    unsafe {
                        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                            Some(name),
                            Some(&*window as &AnyObject),
                            Some(&NSOperationQueue::mainQueue()),
                            &block,
                        )
                    }
                })
                .collect();
            *self.ivars().key_observers.borrow_mut() = observers;
            self.update_surface(false);
        }
    }
);

impl StartActionButton {
    /// `init(title:icon:command:kind:sheet:target:action:)`.
    #[allow(clippy::too_many_arguments)]
    fn new(
        title: &str,
        icon: &str,
        command: Option<Command>,
        kind: Kind,
        sheet: Rc<StyleSheet>,
        target: &StartWindowController,
        action: Sel,
        mtm: MainThreadMarker,
    ) -> Retained<StartActionButton> {
        let shell = NSView::new(mtm);
        let icon_view = NSImageView::new(mtm);
        let shortcut_label = KeycapBadgeField::new(mtm);
        let content_group = NSStackView::new(mtm);
        let title_label = NSTextField::labelWithString(&ns_string(title), mtm);
        let this = Self::alloc(mtm).set_ivars(StartActionButtonIvars {
            kind,
            command,
            shell,
            icon_view,
            title_label,
            shortcut_label,
            content_group,
            shortcut_hint: RefCell::new(String::new()),
            is_hovered: Cell::new(false),
            is_pressed: Cell::new(false),
            sheet: RefCell::new(sheet),
            key_observers: RefCell::new(Vec::new()),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        let ivars = this.ivars();

        // SAFETY: the controller implements `action`; a control holds its
        // target weakly, as in Swift.
        unsafe {
            this.setTarget(Some(target));
            this.setAction(Some(action));
        }
        this.setButtonType(NSButtonType::MomentaryPushIn);
        this.setBordered(false);
        this.setTitle(&NSString::from_str(""));
        this.setFocusRingType(NSFocusRingType::None);
        set_role(&*this, role::button());
        set_label(&*this, title);
        this.setWantsLayer(true);

        ivars.shell.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.shell.setWantsLayer(true);
        if let Some(layer) = ivars.shell.layer() {
            layer.setCornerRadius(StartLayout::CORNER_RADIUS);
            // SAFETY: Core Animation's corner-curve constant.
            layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
            layer.setMasksToBounds(true);
        }
        this.addSubview(&ivars.shell);

        ivars.icon_view.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.icon_view.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        ivars.icon_view.setSymbolConfiguration(Some(&NSImageSymbolConfiguration::configurationWithPointSize_weight(14.0, weight_medium())));
        ivars.icon_view.setImage(
            NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns_string(icon), Some(&ns_string(title))).as_deref(),
        );
        ivars.title_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.title_label.setFont(Some(&NSFont::systemFontOfSize_weight(13.0, weight_semibold())));
        ivars.title_label.setUsesSingleLineMode(true);
        ivars.title_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        configure_passive_label(&ivars.title_label);

        ivars.shortcut_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.shortcut_label.set_min_width(30.0);

        ivars.content_group.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        ivars.content_group.setAlignment(NSLayoutAttribute::CenterY);
        ivars.content_group.setSpacing(8.0);
        ivars.content_group.setDetachesHiddenViews(true);
        ivars.content_group.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.content_group.addArrangedSubview(&ivars.icon_view);
        ivars.content_group.addArrangedSubview(&ivars.title_label);
        ivars.content_group.addArrangedSubview(&ivars.shortcut_label);
        ivars.content_group.setCustomSpacing_afterView(12.0, &ivars.title_label);
        ivars.shell.addSubview(&ivars.content_group);

        activate(&[
            ivars.shell.leadingAnchor().constraintEqualToAnchor(&this.leadingAnchor()),
            ivars.shell.trailingAnchor().constraintEqualToAnchor(&this.trailingAnchor()),
            ivars.shell.topAnchor().constraintEqualToAnchor(&this.topAnchor()),
            ivars.shell.bottomAnchor().constraintEqualToAnchor(&this.bottomAnchor()),
            ivars.content_group.leadingAnchor().constraintGreaterThanOrEqualToAnchor_constant(&ivars.shell.leadingAnchor(), 12.0),
            ivars.content_group.trailingAnchor().constraintLessThanOrEqualToAnchor_constant(&ivars.shell.trailingAnchor(), -12.0),
            ivars.content_group.centerXAnchor().constraintEqualToAnchor(&ivars.shell.centerXAnchor()),
            ivars.content_group.centerYAnchor().constraintEqualToAnchor(&ivars.shell.centerYAnchor()),
            ivars.icon_view.widthAnchor().constraintEqualToConstant(16.0),
            ivars.icon_view.heightAnchor().constraintEqualToConstant(16.0),
        ]);

        let horizontal = NSLayoutConstraintOrientation::Horizontal;
        let (low, required) = (NSLayoutPriorityDefaultLow, NSLayoutPriorityRequired);
        // The label must never be the thing that gives: a clipped "New
        // Documen" is worse than a wider button.
        ivars.title_label.setContentCompressionResistancePriority_forOrientation(required, horizontal);
        ivars.shortcut_label.setContentHuggingPriority_forOrientation(required, horizontal);
        ivars.shortcut_label.setContentCompressionResistancePriority_forOrientation(required, horizontal);

        this.setEnabled(true);
        this.setContentHuggingPriority_forOrientation(low, horizontal);
        this.setContentCompressionResistancePriority_forOrientation(required, horizontal);
        this.refresh_shortcut();
        this.update_surface(false);

        // SAFETY: the owner is the view itself.
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                RECT_ZERO,
                NSTrackingAreaOptions::ActiveInKeyWindow
                    | NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::InVisibleRect,
                Some(&*this as &AnyObject),
                None,
            )
        };
        this.addTrackingArea(&area);
        this
    }

    /// `apply(sheet:)`.
    pub(super) fn apply(&self, sheet: Rc<StyleSheet>) {
        *self.ivars().sheet.borrow_mut() = sheet;
        self.update_surface(false);
    }

    /// `refreshShortcut()`: re-reads the binding. A command with no binding
    /// shows no hint and the button closes up around its label.
    pub(super) fn refresh_shortcut(&self) {
        let hint = self
            .ivars()
            .command
            .and_then(|command| KeybindingStore::shared().primary_binding(command))
            .map(|binding| binding.display_string())
            .unwrap_or_default();
        *self.ivars().shortcut_hint.borrow_mut() = hint.clone();
        self.ivars().shortcut_label.setStringValue(&ns_string(&hint));
        self.ivars().shortcut_label.setHidden(hint.is_empty());
        if hint.is_empty() {
            self.setAccessibilityHelp(None);
        } else {
            set_help(self, &hint);
        }
        self.invalidateIntrinsicContentSize();
        self.setNeedsLayout(true);
        self.update_surface(false);
    }

    fn window_key_state_changed(&self) {
        self.update_surface(false);
    }

    /// `updateSurface(animated:)`.
    fn update_surface(&self, animated: bool) {
        let sheet = self.ivars().sheet.borrow().clone();
        let contrast = sheet.increase_contrast;
        let focused = is_focused(self);
        let this = self.retain();
        let reduce_motion = sheet.reduce_motion;
        let changes = move || {
            let ivars = this.ivars();
            let is_pressed = ivars.is_pressed.get();
            let is_hovered = ivars.is_hovered.get();
            let engaged = is_pressed || is_hovered || focused;
            let shortcut_hint = ivars.shortcut_hint.borrow().clone();
            if ivars.kind == Kind::Primary {
                let accent = sheet.start_window_primary_action();
                let fill = if is_pressed {
                    accent.blendedColorWithFraction_ofColor(0.18, &NSColor::blackColor()).unwrap_or_else(|| accent.clone())
                } else if is_hovered {
                    accent.blendedColorWithFraction_ofColor(0.08, &NSColor::whiteColor()).unwrap_or_else(|| accent.clone())
                } else {
                    accent.clone()
                };
                if let Some(layer) = ivars.shell.layer() {
                    layer.setBackgroundColor(Some(&cg(&fill)));
                    layer.setBorderWidth(if focused { 2.0 } else { 0.0 });
                    layer.setBorderColor(Some(&cg(&NSColor::whiteColor().colorWithAlphaComponent(0.72))));
                }
                ivars.title_label.setTextColor(Some(&NSColor::whiteColor()));
                ivars.icon_view.setContentTintColor(Some(&NSColor::whiteColor()));
                ivars.shortcut_label.setAttributedStringValue(&KeycapFormatter::format(&shortcut_hint, &NSColor::whiteColor()));
                if let Some(layer) = ivars.shortcut_label.layer() {
                    let alpha = if is_pressed {
                        0.22
                    } else if engaged {
                        0.16
                    } else {
                        0.12
                    };
                    layer.setBackgroundColor(Some(&cg(&NSColor::whiteColor().colorWithAlphaComponent(alpha))));
                    layer.setBorderWidth(1.0);
                    layer.setBorderColor(Some(&cg(&NSColor::whiteColor().colorWithAlphaComponent(if engaged { 0.35 } else { 0.22 }))));
                }
            } else {
                let fill_alpha: CGFloat = if is_pressed {
                    0.11
                } else if is_hovered {
                    0.075
                } else {
                    0.045
                };
                if let Some(layer) = ivars.shell.layer() {
                    layer.setBackgroundColor(Some(&cg(
                        &sheet.text.colorWithAlphaComponent(if contrast { smax(fill_alpha, 0.12) } else { fill_alpha }),
                    )));
                    layer.setBorderWidth(if focused { 2.0 } else { 1.0 });
                    let border = if focused { &sheet.accent } else { &sheet.rule };
                    let border_alpha = if focused {
                        0.9
                    } else if contrast {
                        0.55
                    } else {
                        0.35
                    };
                    layer.setBorderColor(Some(&cg(&border.colorWithAlphaComponent(border_alpha))));
                }
                ivars.title_label.setTextColor(Some(&sheet.text));
                let text_color = if engaged { &sheet.text } else { &sheet.text_secondary };
                ivars.shortcut_label.setAttributedStringValue(&KeycapFormatter::format(&shortcut_hint, text_color));
                ivars.icon_view.setContentTintColor(Some(if engaged { &sheet.text } else { &sheet.text_secondary }));
                if let Some(layer) = ivars.shortcut_label.layer() {
                    let alpha = if contrast {
                        if engaged { 0.14 } else { 0.08 }
                    } else if engaged {
                        0.10
                    } else {
                        0.06
                    };
                    layer.setBackgroundColor(Some(&cg(&sheet.text.colorWithAlphaComponent(alpha))));
                    layer.setBorderWidth(1.0);
                    let border = if engaged { &sheet.text } else { &sheet.rule };
                    layer.setBorderColor(Some(&cg(&border.colorWithAlphaComponent(if engaged { 0.28 } else { 0.38 }))));
                }
            }
            if let Some(layer) = ivars.shell.layer() {
                layer.setAffineTransform(if is_pressed {
                    scale(0.985, 0.985)
                } else if is_hovered {
                    scale(1.010, 1.010)
                } else {
                    IDENTITY
                });
            }
        };
        if animated {
            motion::run(reduce_motion, motion::HOVER, Curve::EaseOut, move |_| changes(), None);
        } else {
            changes();
        }
    }
}

// MARK: - BrandMarkView

define_class!(
    // SAFETY: `initWithFrame:` is overridden without ivars, so every
    // initialiser path creates a valid instance.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "BrandMarkView"]
    pub(super) struct BrandMarkView;

    unsafe impl NSObjectProtocol for BrandMarkView {}

    impl BrandMarkView {
        #[unsafe(method_id(initWithFrame:))]
        fn __init_with_frame(this: Allocated<Self>, frame: NSRect) -> Retained<Self> {
            let this = this.set_ivars(());
            let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
            set_role(&*this, role::image());
            set_label(&*this, "Upleft app icon");
            this
        }

        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, _dirty_rect: NSRect) {
            let Some(icon) = Self::icon() else { return };
            let icon_rect = self.bounds().inset_by(1.0, 1.0);
            let radius = icon_rect.width() * 0.24;

            let Some(graphics) = NSGraphicsContext::currentContext() else { return };
            let cg_context = graphics.CGContext();
            let context = Some(&*cg_context);
            CGContext::save_g_state(context);
            CGContext::set_shadow_with_color(
                context,
                CGSize::new(0.0, -1.5),
                4.0,
                Some(&cg(&NSColor::blackColor().colorWithAlphaComponent(0.24))),
            );
            NSColor::whiteColor().colorWithAlphaComponent(0.96).setFill();
            let path = PanelMetrics::continuous_rounded_path(icon_rect, radius);
            CGContext::add_path(context, Some(&path));
            CGContext::fill_path(context);
            CGContext::restore_g_state(context);

            NSGraphicsContext::saveGraphicsState_class();
            CGContext::add_path(context, Some(&path));
            CGContext::clip(context);
            // Swift passes `[.interpolation: NSImageInterpolation.high]`,
            // which bridges to a `__SwiftValue` box, not an `NSNumber`
            // (probed): AppKit cannot read it, so the draw uses the
            // context's interpolation. No hint reproduces that.
            // SAFETY: a nil hints dictionary is allowed.
            unsafe {
                icon.drawInRect_fromRect_operation_fraction_respectFlipped_hints(
                    icon_rect,
                    RECT_ZERO,
                    NSCompositingOperation::SourceOver,
                    1.0,
                    true,
                    None,
                );
            }
            NSGraphicsContext::restoreGraphicsState_class();
        }
    }
);

thread_local! {
    /// `BrandMarkView.icon`: loaded once, on first draw.
    static BRAND_ICON: OnceCell<Option<Retained<NSImage>>> = const { OnceCell::new() };
}

impl BrandMarkView {
    /// `BrandMarkView()`.
    fn new(mtm: MainThreadMarker) -> Retained<BrandMarkView> {
        unsafe { msg_send![Self::alloc(mtm), init] }
    }

    /// `private static let icon`: `Contents/Resources/AppIcon.png` in the
    /// main bundle.
    fn icon() -> Option<Retained<NSImage>> {
        BRAND_ICON.with(|cell| {
            cell.get_or_init(|| {
                let url = NSBundle::mainBundle()
                    .bundleURL()
                    .URLByAppendingPathComponent(&NSString::from_str("Contents/Resources/AppIcon.png"))?;
                NSImage::initWithContentsOfURL(NSImage::alloc(), &url)
            })
            .clone()
        })
    }
}
