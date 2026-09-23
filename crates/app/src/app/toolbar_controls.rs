//! Port of `App/ToolbarControls.swift`: the titlebar's controls.
//!
//! | Swift | Rust | Objective-C name |
//! |---|---|---|
//! | `ToolbarChromePolicy` (`InteractionState`, `ScrubState`) | [`ToolbarChromePolicy`], [`InteractionState`], [`ScrubState`] | — |
//! | `ToolbarScrubPhase` | [`ToolbarScrubPhase`] | — |
//! | `ToolbarDocumentIdentityView` | [`ToolbarDocumentIdentityView`] | `ToolbarDocumentIdentityView` |
//! | `ToolbarPresentationControl` | [`ToolbarPresentationControl`] | `ToolbarPresentationControl` |
//! | `ToolbarInteractiveButton` | [`ToolbarInteractiveButton`] | `ToolbarInteractiveButton` |
//! | `ToolbarModeButton` (private) | `ToolbarModeButton` | `ToolbarModeButton` |
//! | `ToolbarMenuButton` | [`ToolbarMenuButton`] | `ToolbarMenuButton` |
//! | `ToolbarActionButton` | [`ToolbarActionButton`] | `ToolbarActionButton` |
//! | `ToolbarTrailingCluster` | [`ToolbarTrailingCluster`] | `ToolbarTrailingCluster` |
//!
//! The panels (`BreadcrumbView`, `CommandPaletteView`, `TaskProgressRing`,
//! `UpdateStatusPill`, `PanelChrome`) build on `ToolbarChromePolicy` and
//! `ToolbarInteractiveButton`; their names and signatures are stable:
//! `ToolbarChromePolicy::{HOVER_DURATION, PRESS_IN_DURATION,
//! PRESS_OUT_DURATION, SELECTION_DURATION, EMPHASIS_DURATION, PRESSED_SCALE,
//! RING_PRESSED_SCALE, timing_function, feedback_opacity, indicator_opacity,
//! scrub_state_for_position, scrub_state}`, `InteractionState`, `ScrubState`,
//! and `ToolbarInteractiveButton` with its Objective-C overridables
//! `styleSheetDidChange` and `permitsHoverFeedback`.
//!
//! Swift's `NSObject.observe(_:options:changeHandler:)` (the identity view's
//! window observations) is [`KeyValueObservation`]: the same
//! `addObserver:forKeyPath:options:context:` registration, removed on
//! `invalidate` or drop, with the observed object held weakly as Swift's
//! `NSKeyValueObservation` holds it.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::Rc;

use objc2::rc::{Allocated, Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBeep, NSBezelStyle, NSButton, NSCellImagePosition, NSColor, NSControl, NSControlSize, NSDragOperation,
    NSDraggingContext, NSDraggingItem, NSDraggingSession, NSDraggingSource, NSEvent, NSFocusRingType, NSFont,
    NSGestureRecognizerState, NSHapticFeedbackManager, NSHapticFeedbackPattern, NSHapticFeedbackPerformanceTime,
    NSHapticFeedbackPerformer, NSImageScaling, NSImageView, NSLayoutAttribute, NSLayoutConstraintOrientation,
    NSLayoutPriorityDefaultLow, NSLayoutPriorityRequired, NSLineBreakMode, NSMenu, NSMenuItem, NSPanGestureRecognizer,
    NSPasteboardWriting, NSResponder, NSStackView, NSTextAlignment, NSTextField, NSTrackingArea, NSTrackingAreaOptions,
    NSUserInterfaceLayoutOrientation, NSView, NSWindow, NSWindowDidBecomeKeyNotification,
    NSWindowDidResignKeyNotification, NSWorkspace, NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
};
use objc2_core_foundation::{CGFloat, CGSize};
use objc2_foundation::{
    NSArray, NSFileManager, NSKeyValueObservingOptions, NSNotification, NSNotificationCenter, NSNotificationName,
    NSObjectNSKeyValueObserverRegistration, NSOperationQueue, NSPoint, NSRect, NSSize, NSString, NSURL,
};
use objc2_quartz_core::{CABasicAnimation, CALayer, CAMediaTiming, CAMediaTimingFunction, CATransform3D};
use upleft_foundation::url::FileUrl;
use upleft_render::appkit_compat::{RECT_ZERO, keys};
use upleft_render::motion::{self, Curve, SpringScalar, SpringSurfaceView};
use upleft_render::theme::style_sheet::StyleSheet;

use crate::ai::markdown_document::{Phase, PresentationState};
use crate::panels::appkit_support::{
    Presentation, RectExt, activate, cg, configured_symbol, downcast, needs_display, ns_string, object, rect, role,
    set_help, set_label, set_role, set_tool_tip, set_value, superview, symbol_configuration, system_font,
    weight_medium, weight_semibold, without_actions,
};
use crate::panels::chrome_glass::{ChromeGlass, RoundedCorners, Tint};
use crate::panels::panel_chrome::{PanelMetrics, presentation_transform, set_number_values, set_transform_values};

/// `ToolbarChromePolicy`: one policy for toolbar motion and emphasis.
pub struct ToolbarChromePolicy;

/// `ToolbarChromePolicy.InteractionState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionState {
    Idle,
    Hover,
    Pressed,
}

/// `ToolbarChromePolicy.ScrubState`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrubState {
    pub indicator_center_x: CGFloat,
    pub segment: isize,
}

impl ToolbarChromePolicy {
    pub const HOVER_DURATION: f64 = motion::HOVER;
    pub const PRESS_IN_DURATION: f64 = motion::PRESS_IN;
    pub const PRESS_OUT_DURATION: f64 = motion::PRESS_OUT;
    pub const SELECTION_DURATION: f64 = motion::SELECTION;
    pub const EMPHASIS_DURATION: f64 = motion::EMPHASIS;
    pub const PRESSED_SCALE: CGFloat = 0.985;
    /// The task ring's press dips further than a plate button's.
    pub const RING_PRESSED_SCALE: CGFloat = 0.86;

    pub fn timing_function() -> Retained<CAMediaTimingFunction> {
        motion::timing(Curve::Snap)
    }

    pub fn feedback_opacity(state: InteractionState, increase_contrast: bool) -> f32 {
        match (state, increase_contrast) {
            (InteractionState::Idle, _) => 0.0,
            (InteractionState::Hover, false) => 0.075,
            (InteractionState::Hover, true) => 0.11,
            (InteractionState::Pressed, false) => 0.14,
            (InteractionState::Pressed, true) => 0.19,
        }
    }

    pub fn indicator_opacity(is_window_active: bool, increase_contrast: bool) -> f32 {
        match (is_window_active, increase_contrast) {
            (true, false) => 0.82,
            (true, true) => 1.0,
            (false, false) => 0.38,
            (false, true) => 0.56,
        }
    }

    /// `scrubState(position:leftCenterX:rightCenterX:)`: the same state for
    /// a gesture that reports how far across the rail it has travelled
    /// rather than where a pointer sits.
    pub fn scrub_state_for_position(position: CGFloat, left_center_x: CGFloat, right_center_x: CGFloat) -> ScrubState {
        let travelled = swift_min(swift_max(position, 0.0), 1.0);
        Self::scrub_state(left_center_x + (right_center_x - left_center_x) * travelled, left_center_x, right_center_x)
    }

    /// `scrubState(pointerX:leftCenterX:rightCenterX:)`.
    pub fn scrub_state(pointer_x: CGFloat, left_center_x: CGFloat, right_center_x: CGFloat) -> ScrubState {
        let lower_bound = swift_min(left_center_x, right_center_x);
        let upper_bound = swift_max(left_center_x, right_center_x);
        let center_x = swift_min(swift_max(pointer_x, lower_bound), upper_bound);
        ScrubState {
            indicator_center_x: center_x,
            segment: if center_x < ((lower_bound + upper_bound) / 2.0) { 0 } else { 1 },
        }
    }
}

fn swift_min(x: CGFloat, y: CGFloat) -> CGFloat {
    upleft_render::swift_compat::smin(x, y)
}

fn swift_max(x: CGFloat, y: CGFloat) -> CGFloat {
    upleft_render::swift_compat::smax(x, y)
}

/// `ToolbarScrubPhase`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarScrubPhase {
    Began,
    Changed,
    Ended,
    Cancelled,
}

// MARK: - Observation helpers

/// A token from `NotificationCenter.addObserver(forName:object:queue:using:)`.
type ObserverToken = Retained<ProtocolObject<dyn NSObjectProtocol>>;

/// `center.removeObserver(token)`.
fn remove_observer(center: &NSNotificationCenter, token: &ObserverToken) {
    // SAFETY: the token came from this centre's block-based registration.
    unsafe { center.removeObserver(&*(Retained::as_ptr(token) as *const AnyObject)) };
}

/// `NotificationCenter.default.addObserver(forName: name, object: window,
/// queue: .main) { _ in handler() }`.
fn observe_window_notification(
    name: &NSNotificationName,
    window: &NSWindow,
    handler: impl Fn() + 'static,
) -> ObserverToken {
    let block = block2::RcBlock::new(move |_notification: std::ptr::NonNull<NSNotification>| handler());
    // SAFETY: the block only runs on the main queue.
    unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(name),
            Some(object(window)),
            Some(&NSOperationQueue::mainQueue()),
            &block,
        )
    }
}

/// `NSWorkspace.shared.notificationCenter.addObserver(forName:
/// accessibilityDisplayOptionsDidChangeNotification, object: nil, queue: .main)`.
fn observe_accessibility_display_options(handler: impl Fn() + 'static) -> ObserverToken {
    let block = block2::RcBlock::new(move |_notification: std::ptr::NonNull<NSNotification>| handler());
    // SAFETY: the block only runs on the main queue.
    unsafe {
        NSWorkspace::sharedWorkspace().notificationCenter().addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
            None,
            Some(&NSOperationQueue::mainQueue()),
            &block,
        )
    }
}

fn increase_contrast() -> bool {
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldIncreaseContrast()
}

pub struct KeyValueObserverIvars {
    handler: Box<dyn Fn(&AnyObject)>,
}

define_class!(
    /// The observer object behind a [`KeyValueObservation`].
    // SAFETY: NSObject's `init` is forwarded after the ivars are set.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpleftKeyValueObservation"]
    #[ivars = KeyValueObserverIvars]
    struct KeyValueObserver;

    unsafe impl NSObjectProtocol for KeyValueObserver {}

    impl KeyValueObserver {
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn __observe_value(
            &self,
            _key_path: Option<&NSString>,
            object: Option<&AnyObject>,
            _change: Option<&AnyObject>,
            _context: *mut c_void,
        ) {
            if let Some(object) = object {
                (self.ivars().handler)(object);
            }
        }
    }
);

/// Swift's `NSKeyValueObservation`: `object.observe(\.keyPath, options:)
/// { object, _ in … }`. The handler receives the observed object; the
/// observation is removed on [`KeyValueObservation::invalidate`] or drop.
pub struct KeyValueObservation {
    observer: Retained<KeyValueObserver>,
    object: RefCell<ObjcWeak<NSObject>>,
    key_path: Retained<NSString>,
}

impl KeyValueObservation {
    /// `object.observe(\.<key_path>, options: options, changeHandler:)`.
    /// `key_path` is the Objective-C key (`documentEdited` for Swift's
    /// `\.isDocumentEdited`).
    pub fn new(
        object: &NSObject,
        key_path: &str,
        options: NSKeyValueObservingOptions,
        handler: impl Fn(&AnyObject) + 'static,
        mtm: MainThreadMarker,
    ) -> KeyValueObservation {
        let observer = KeyValueObserver::alloc(mtm).set_ivars(KeyValueObserverIvars { handler: Box::new(handler) });
        let observer: Retained<KeyValueObserver> = unsafe { msg_send![super(observer), init] };
        let key_path = NSString::from_str(key_path);
        let observation = KeyValueObservation { observer, object: RefCell::new(ObjcWeak::from(object)), key_path };
        // SAFETY: the observer outlives its registration: `invalidate` (run
        // on drop at the latest) removes it.
        unsafe {
            object.addObserver_forKeyPath_options_context(
                &observation.observer,
                &observation.key_path,
                options,
                std::ptr::null_mut(),
            )
        };
        observation
    }

    /// `observation.invalidate()`.
    pub fn invalidate(&self) {
        let object = self.object.borrow().load();
        if let Some(object) = object {
            // SAFETY: the registration made in `new`.
            unsafe { object.removeObserver_forKeyPath_context(&self.observer, &self.key_path, std::ptr::null_mut()) };
        }
        *self.object.borrow_mut() = ObjcWeak::default();
    }
}

impl Drop for KeyValueObservation {
    fn drop(&mut self) {
        self.invalidate();
    }
}

/// `options: [.initial, .new]`.
fn initial_and_new() -> NSKeyValueObservingOptions {
    NSKeyValueObservingOptions::Initial | NSKeyValueObservingOptions::New
}

/// The observed object of a window observation.
fn as_window(object: &AnyObject) -> &NSWindow {
    // SAFETY: only windows are observed with this helper's handlers.
    unsafe { &*(object as *const AnyObject as *const NSWindow) }
}

/// `url.path` of a file URL (Swift's `URL.path` is never nil).
fn url_path(url: &NSURL) -> Retained<NSString> {
    url.path().unwrap_or_else(|| NSString::from_str(""))
}

fn file_exists(path: &NSString) -> bool {
    NSFileManager::defaultManager().fileExistsAtPath(path)
}

// MARK: - ToolbarDocumentIdentityView

/// `ToolbarDocumentIdentityView.Metrics`.
struct IdentityMetrics;

impl IdentityMetrics {
    const WIDTH: CGFloat = 220.0;
    const HEIGHT: CGFloat = 36.0;
    const PROXY_SIZE: CGFloat = 16.0;
    const TITLE_SIZE: CGFloat = 12.5;
    const STATE_SIZE: CGFloat = 9.5;
    const CONTEXT_SIZE: CGFloat = 10.0;
    const LINE_GAP: CGFloat = 1.0;
}

pub struct ToolbarDocumentIdentityViewIvars {
    host_window: ObjcWeak<NSWindow>,
    proxy_button: Retained<ToolbarInteractiveButton>,
    state_label: Retained<NSTextField>,
    title_label: Retained<NSTextField>,
    context_label: Retained<NSTextField>,
    title_row: Retained<NSStackView>,
    secondary_row: Retained<NSStackView>,
    text_column: Retained<NSStackView>,
    title_observation: RefCell<Option<KeyValueObservation>>,
    subtitle_observation: RefCell<Option<KeyValueObservation>>,
    edited_observation: RefCell<Option<KeyValueObservation>>,
    url_observation: RefCell<Option<KeyValueObservation>>,
    activation_observers: RefCell<Vec<ObserverToken>>,
    is_edited: Cell<bool>,
    has_external_changes: Cell<bool>,
    document_state: RefCell<PresentationState>,
}

impl Drop for ToolbarDocumentIdentityViewIvars {
    /// `deinit`: invalidate the observations, then remove the activation
    /// observers.
    fn drop(&mut self) {
        for observation in
            [&self.title_observation, &self.subtitle_observation, &self.edited_observation, &self.url_observation]
        {
            if let Some(observation) = observation.borrow_mut().take() {
                observation.invalidate();
            }
        }
        let center = NSNotificationCenter::defaultCenter();
        for observer in self.activation_observers.get_mut().drain(..) {
            remove_observer(&center, &observer);
        }
    }
}

define_class!(
    /// Leading titlebar identity. It mirrors the window's document title
    /// while preserving a deliberate two-line hierarchy and the titlebar drag
    /// region. A quiet proxy opens the path menu; a compact native label
    /// names exceptional document state. Neutral documents remain visually
    /// silent.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set;
    // every override keeps AppKit's signature.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "ToolbarDocumentIdentityView"]
    #[ivars = ToolbarDocumentIdentityViewIvars]
    pub struct ToolbarDocumentIdentityView;

    unsafe impl NSObjectProtocol for ToolbarDocumentIdentityView {}

    unsafe impl NSDraggingSource for ToolbarDocumentIdentityView {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn __source_operation_mask(&self, _session: &NSDraggingSession, context: NSDraggingContext) -> NSDragOperation {
            if context == NSDraggingContext::OutsideApplication {
                NSDragOperation::Copy | NSDragOperation::Link
            } else {
                NSDragOperation::Copy
            }
        }
    }

    impl ToolbarDocumentIdentityView {
        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            NSSize::new(IdentityMetrics::WIDTH, IdentityMetrics::HEIGHT)
        }

        #[unsafe(method(mouseDownCanMoveWindow))]
        fn __mouse_down_can_move_window(&self) -> bool {
            true
        }

        #[unsafe(method_id(hitTest:))]
        fn __hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            // `point` is in the superview's coordinate space.
            let point_in_self = self.convertPoint_fromView(point, superview(self).as_deref());
            let proxy_button = &self.ivars().proxy_button;
            let view: &NSView = if proxy_button.frame().inset_by(-2.0, -2.0).contains_point(point_in_self) {
                proxy_button
            } else {
                self
            };
            Some(view.retain())
        }

        #[unsafe(method(mouseDragged:))]
        fn __mouse_dragged(&self, event: &NSEvent) {
            self.mouse_dragged(event);
        }

        #[unsafe(method(showPathMenu:))]
        fn __show_path_menu(&self, _sender: Option<&AnyObject>) {
            self.show_path_menu();
        }

        #[unsafe(method(openPathComponent:))]
        fn __open_path_component(&self, sender: &NSMenuItem) {
            self.open_path_component(sender);
        }

        #[unsafe(method(revealInFinder:))]
        fn __reveal_in_finder(&self, sender: &NSMenuItem) {
            self.reveal_in_finder(sender);
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.refresh_emphasis();
        }
    }
);

impl ToolbarDocumentIdentityView {
    /// `init(window:)`.
    pub fn new(window: &NSWindow, mtm: MainThreadMarker) -> Retained<ToolbarDocumentIdentityView> {
        let this = Self::alloc(mtm).set_ivars(ToolbarDocumentIdentityViewIvars {
            host_window: ObjcWeak::from(window),
            proxy_button: ToolbarInteractiveButton::new(RECT_ZERO, mtm),
            state_label: NSTextField::labelWithString(&NSString::from_str(""), mtm),
            title_label: NSTextField::labelWithString(&NSString::from_str(""), mtm),
            context_label: NSTextField::labelWithString(&NSString::from_str(""), mtm),
            title_row: NSStackView::new(mtm),
            secondary_row: NSStackView::new(mtm),
            text_column: NSStackView::new(mtm),
            title_observation: RefCell::new(None),
            subtitle_observation: RefCell::new(None),
            edited_observation: RefCell::new(None),
            url_observation: RefCell::new(None),
            activation_observers: RefCell::new(Vec::new()),
            is_edited: Cell::new(false),
            has_external_changes: Cell::new(false),
            document_state: RefCell::new(PresentationState::NEUTRAL),
        });
        let this: Retained<ToolbarDocumentIdentityView> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        this.finish_init(window, mtm);
        this
    }

    fn finish_init(&self, window: &NSWindow, mtm: MainThreadMarker) {
        let ivars = self.ivars();
        let proxy_button = &ivars.proxy_button;
        proxy_button.set_feedback_inset_x(1.0);
        proxy_button.set_feedback_inset_y(1.0);
        proxy_button.set_feedback_corner_radius(3.0);
        proxy_button.setImage(
            configured_symbol("doc.text", Some("Document path"), &symbol_configuration(12.0, weight_medium()))
                .as_deref(),
        );
        proxy_button.setImagePosition(NSCellImagePosition::ImageOnly);
        proxy_button.setImageScaling(NSImageScaling::ScaleProportionallyDown);
        proxy_button.setBordered(false);
        // Swift's `.inline` (`NSBezelStyleInline`, renamed `Badge`).
        proxy_button.setBezelStyle(NSBezelStyle::Badge);
        proxy_button.setFocusRingType(NSFocusRingType::Default);
        set_role(&**proxy_button, role::button());
        set_label(&**proxy_button, "Document path");
        set_tool_tip(proxy_button, Some("Show document path"));
        // SAFETY: the target is this view, which owns the button.
        unsafe {
            proxy_button.setTarget(Some(object(self)));
            proxy_button.setAction(Some(sel!(showPathMenu:)));
        }

        let state_label = &ivars.state_label;
        state_label.setFont(Some(&system_font(IdentityMetrics::STATE_SIZE, weight_medium())));
        state_label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        state_label.setMaximumNumberOfLines(1);
        state_label.setHidden(true);
        let (required, default_low) = (NSLayoutPriorityRequired, NSLayoutPriorityDefaultLow);
        state_label.setContentHuggingPriority_forOrientation(required, NSLayoutConstraintOrientation::Horizontal);
        state_label.setContentCompressionResistancePriority_forOrientation(
            required,
            NSLayoutConstraintOrientation::Horizontal,
        );

        let title_label = &ivars.title_label;
        title_label.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
        title_label.setMaximumNumberOfLines(1);
        title_label.setFont(Some(&system_font(IdentityMetrics::TITLE_SIZE, weight_semibold())));
        title_label.setContentCompressionResistancePriority_forOrientation(
            default_low,
            NSLayoutConstraintOrientation::Horizontal,
        );
        title_label.setContentHuggingPriority_forOrientation(default_low, NSLayoutConstraintOrientation::Horizontal);

        let context_label = &ivars.context_label;
        context_label.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
        context_label.setMaximumNumberOfLines(1);
        context_label.setFont(Some(&system_font(IdentityMetrics::CONTEXT_SIZE, weight_medium())));
        context_label.setContentCompressionResistancePriority_forOrientation(
            default_low,
            NSLayoutConstraintOrientation::Horizontal,
        );
        context_label.setContentHuggingPriority_forOrientation(default_low, NSLayoutConstraintOrientation::Horizontal);

        let title_row = &ivars.title_row;
        title_row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        title_row.setAlignment(NSLayoutAttribute::CenterY);
        title_row.setDetachesHiddenViews(true);
        title_row.addArrangedSubview(title_label);

        let secondary_row = &ivars.secondary_row;
        secondary_row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        secondary_row.setAlignment(NSLayoutAttribute::CenterY);
        secondary_row.setSpacing(5.0);
        secondary_row.setDetachesHiddenViews(true);
        secondary_row.addArrangedSubview(state_label);
        secondary_row.addArrangedSubview(context_label);

        let text_column = &ivars.text_column;
        text_column.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        text_column.setAlignment(NSLayoutAttribute::Leading);
        text_column.setSpacing(IdentityMetrics::LINE_GAP);
        text_column.addArrangedSubview(title_row);
        text_column.addArrangedSubview(secondary_row);

        proxy_button.setTranslatesAutoresizingMaskIntoConstraints(false);
        text_column.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(proxy_button);
        self.addSubview(text_column);

        self.setContentCompressionResistancePriority_forOrientation(
            default_low,
            NSLayoutConstraintOrientation::Horizontal,
        );
        self.setContentHuggingPriority_forOrientation(default_low, NSLayoutConstraintOrientation::Horizontal);

        activate(&[
            proxy_button.leadingAnchor().constraintEqualToAnchor(&self.leadingAnchor()),
            proxy_button.centerYAnchor().constraintEqualToAnchor(&self.centerYAnchor()),
            proxy_button.widthAnchor().constraintEqualToConstant(IdentityMetrics::PROXY_SIZE + 2.0),
            proxy_button.heightAnchor().constraintEqualToConstant(IdentityMetrics::PROXY_SIZE + 2.0),
            text_column.leadingAnchor().constraintEqualToAnchor_constant(&proxy_button.trailingAnchor(), 5.0),
            text_column.trailingAnchor().constraintLessThanOrEqualToAnchor(&self.trailingAnchor()),
            text_column.centerYAnchor().constraintEqualToAnchor(&self.centerYAnchor()),
        ]);

        let weak = ObjcWeak::from(self);
        let observation = KeyValueObservation::new(
            window,
            "title",
            initial_and_new(),
            move |window| {
                if let Some(this) = weak.load() {
                    let window = as_window(window);
                    this.update(&window.title().to_string(), &window.subtitle().to_string());
                }
            },
            mtm,
        );
        *ivars.title_observation.borrow_mut() = Some(observation);
        let weak = ObjcWeak::from(self);
        let observation = KeyValueObservation::new(
            window,
            "subtitle",
            initial_and_new(),
            move |window| {
                if let Some(this) = weak.load() {
                    let window = as_window(window);
                    this.update(&window.title().to_string(), &window.subtitle().to_string());
                }
            },
            mtm,
        );
        *ivars.subtitle_observation.borrow_mut() = Some(observation);
        let weak = ObjcWeak::from(self);
        let observation = KeyValueObservation::new(
            window,
            "documentEdited",
            initial_and_new(),
            move |window| {
                if let Some(this) = weak.load() {
                    this.set_is_edited(as_window(window).isDocumentEdited());
                }
            },
            mtm,
        );
        *ivars.edited_observation.borrow_mut() = Some(observation);
        let weak = ObjcWeak::from(self);
        let observation = KeyValueObservation::new(
            window,
            "representedURL",
            initial_and_new(),
            move |window| {
                if let Some(this) = weak.load() {
                    this.update_proxy_icon(as_window(window).representedURL().as_deref());
                }
            },
            mtm,
        );
        *ivars.url_observation.borrow_mut() = Some(observation);
        // SAFETY: AppKit exports the notification names as immutable globals.
        let names = unsafe { [NSWindowDidBecomeKeyNotification, NSWindowDidResignKeyNotification] };
        let observers: Vec<ObserverToken> = names
            .into_iter()
            .map(|name| {
                let weak = ObjcWeak::from(self);
                observe_window_notification(name, window, move || {
                    if let Some(this) = weak.load() {
                        this.refresh_emphasis();
                    }
                })
            })
            .collect();
        *ivars.activation_observers.borrow_mut() = observers;
        self.refresh_emphasis();
        self.refresh_document_state();
    }

    fn host_window(&self) -> Option<Retained<NSWindow>> {
        self.ivars().host_window.load()
    }

    /// `isEdited`.
    pub fn is_edited(&self) -> bool {
        self.ivars().is_edited.get()
    }

    /// `isEdited { didSet }`.
    pub fn set_is_edited(&self, value: bool) {
        let old_value = self.ivars().is_edited.replace(value);
        if value == old_value {
            return;
        }
        let phase = self.ivars().document_state.borrow().phase;
        if value && phase == Phase::Neutral {
            self.set_document_state(PresentationState::new(Phase::Edited, Some("Edit".to_owned()), None));
        } else if !value && phase == Phase::Edited {
            self.set_document_state(PresentationState::NEUTRAL);
        }
    }

    /// `hasExternalChanges`.
    pub fn has_external_changes(&self) -> bool {
        self.ivars().has_external_changes.get()
    }

    /// `hasExternalChanges { didSet }`.
    pub fn set_has_external_changes(&self, value: bool) {
        let old_value = self.ivars().has_external_changes.replace(value);
        if value == old_value {
            return;
        }
        let phase = self.ivars().document_state.borrow().phase;
        if value && phase == Phase::Neutral {
            self.set_document_state(PresentationState::new(Phase::ChangedOnDisk, None, None));
        }
    }

    /// `documentState`.
    pub fn document_state(&self) -> PresentationState {
        self.ivars().document_state.borrow().clone()
    }

    /// `documentState { didSet }`.
    pub fn set_document_state(&self, state: PresentationState) {
        let old_value = self.ivars().document_state.replace(state);
        if *self.ivars().document_state.borrow() == old_value {
            return;
        }
        self.refresh_document_state();
    }

    /// The proxy wears the document's own icon, the way the titlebar proxy
    /// in every other macOS document window does.
    fn update_proxy_icon(&self, url: Option<&NSURL>) {
        let proxy_button = &self.ivars().proxy_button;
        let path = url.map(url_path);
        let Some(path) = path.filter(|path| file_exists(path)) else {
            proxy_button.setImage(
                configured_symbol("doc.text", Some("Document path"), &symbol_configuration(12.0, weight_medium()))
                    .as_deref(),
            );
            let key = self.host_window().is_some_and(|window| window.isKeyWindow());
            let tint = if key { NSColor::secondaryLabelColor() } else { NSColor::tertiaryLabelColor() };
            proxy_button.setContentTintColor(Some(&tint));
            return;
        };
        let icon = NSWorkspace::sharedWorkspace().iconForFile(&path);
        icon.setSize(NSSize::new(IdentityMetrics::PROXY_SIZE, IdentityMetrics::PROXY_SIZE));
        proxy_button.setImage(Some(&icon));
        // A file icon is already coloured; tinting it would flatten it to a
        // silhouette.
        proxy_button.setContentTintColor(None);
    }

    /// Dragging the proxy hands the file to whatever is under the pointer.
    fn mouse_dragged(&self, event: &NSEvent) {
        let start = self.convertPoint_fromView(event.locationInWindow(), None);
        let url = self.host_window().and_then(|window| window.representedURL());
        let Some(url) = url.filter(|url| {
            self.ivars().proxy_button.frame().inset_by(-2.0, -2.0).contains_point(start) && file_exists(&url_path(url))
        }) else {
            let _: () = unsafe { msg_send![super(self), mouseDragged: event] };
            return;
        };
        // SAFETY: `NSURL` conforms to `NSPasteboardWriting` (AppKit's
        // `NSURL (NSPasteboardSupport)` category).
        let writer: &ProtocolObject<dyn NSPasteboardWriting> =
            unsafe { &*(Retained::as_ptr(&url) as *const ProtocolObject<dyn NSPasteboardWriting>) };
        let item = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), writer);
        let icon = NSWorkspace::sharedWorkspace().iconForFile(&url_path(&url));
        icon.setSize(NSSize::new(32.0, 32.0));
        // SAFETY: an `NSImage` is valid dragging contents.
        unsafe {
            item.setDraggingFrame_contents(rect(start.x - 16.0, start.y - 16.0, 32.0, 32.0), Some(object(&*icon)))
        };
        let items = NSArray::from_retained_slice(&[item]);
        self.beginDraggingSessionWithItems_event_source(&items, event, ProtocolObject::from_ref(self));
    }

    fn show_path_menu(&self) {
        let mtm = self.mtm();
        let window = self.host_window();
        let Some(url) = window.as_ref().and_then(|window| window.representedURL()) else {
            NSBeep();
            return;
        };
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Document Path"));
        // `url.absoluteURL`, then its parents. The first entry is the
        // represented URL itself, so `openPathComponent` can compare it.
        let absolute = url.absoluteURL().unwrap_or_else(|| url.clone());
        let mut urls: Vec<(FileUrl, Retained<NSURL>)> = Vec::new();
        if let Some(mut current) = FileUrl::from_nsurl(&absolute) {
            let mut current_object = absolute.clone();
            while current.path() != "/" {
                urls.push((current.clone(), current_object));
                let parent = current.deleting_last_path_component();
                if parent.path() == current.path() {
                    break;
                }
                current_object = parent.to_nsurl();
                current = parent;
            }
        }
        for (path_url, path_object) in &urls {
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &ns_string(&path_url.last_path_component()),
                    Some(sel!(openPathComponent:)),
                    &NSString::from_str(""),
                )
            };
            // SAFETY: the target is this view, alive while the menu tracks.
            unsafe {
                item.setTarget(Some(object(self)));
                item.setRepresentedObject(Some(object(&**path_object)));
            }
            item.setImage(Some(&NSWorkspace::sharedWorkspace().iconForFile(&ns_string(&path_url.path()))));
            if let Some(image) = item.image() {
                image.setSize(NSSize::new(16.0, 16.0));
            }
            menu.addItem(&item);
        }
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        let reveal = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str("Reveal in Finder"),
                Some(sel!(revealInFinder:)),
                &NSString::from_str(""),
            )
        };
        // SAFETY: as above.
        unsafe {
            reveal.setTarget(Some(object(self)));
            reveal.setRepresentedObject(Some(object(&*url)));
        }
        menu.addItem(&reveal);
        menu.popUpMenuPositioningItem_atLocation_inView(None, NSPoint::new(0.0, self.bounds().min_y()), Some(self));
    }

    fn open_path_component(&self, sender: &NSMenuItem) {
        let Some(url) = sender.representedObject().and_then(|value| downcast::<NSURL>(&value)) else { return };
        let represented = self.host_window().and_then(|window| window.representedURL());
        let workspace = NSWorkspace::sharedWorkspace();
        if represented.is_some_and(|represented| url.isEqual(Some(object(&*represented)))) {
            workspace.activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
        } else {
            workspace.selectFile_inFileViewerRootedAtPath(None, &url_path(&url));
        }
    }

    fn reveal_in_finder(&self, sender: &NSMenuItem) {
        let url = sender
            .representedObject()
            .and_then(|value| downcast::<NSURL>(&value))
            .or_else(|| self.host_window().and_then(|window| window.representedURL()));
        let Some(url) = url else { return };
        NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
    }

    fn update(&self, title: &str, context: &str) {
        let ivars = self.ivars();
        ivars.title_label.setStringValue(&ns_string(title));
        ivars.context_label.setStringValue(&ns_string(context));
        ivars.context_label.setHidden(context.is_empty());
        self.refresh_tool_tip();
        self.refresh_accessibility();
    }

    fn identity_description(&self) -> String {
        let ivars = self.ivars();
        let context = ivars.context_label.stringValue().to_string();
        let title = ivars.title_label.stringValue().to_string();
        if context.is_empty() { title } else { format!("{title}, {context}") }
    }

    fn refresh_accessibility(&self) {
        let base = self.identity_description();
        let label = match self.state_description() {
            None => base,
            Some(state) => format!("{base}, {state}"),
        };
        set_role(self, role::group());
        set_label(self, &label);
    }

    fn state_description(&self) -> Option<String> {
        let title = self.state_title()?;
        let state = self.ivars().document_state.borrow();
        let provenance = state.provenance.as_ref().map(|value| format!(" — {value}")).unwrap_or_default();
        // Swift's `==` is canonical equivalence; every state title is ASCII
        // without a letter any other scalar decomposes to, so byte equality
        // agrees with it.
        let detail = state
            .detail
            .as_ref()
            .and_then(|value| if value == title { None } else { Some(format!(": {value}")) })
            .unwrap_or_default();
        Some(format!("{title}{provenance}{detail}"))
    }

    fn state_title(&self) -> Option<&'static str> {
        let state = self.ivars().document_state.borrow();
        match state.phase {
            Phase::Neutral | Phase::Edited | Phase::Saving | Phase::Saved => None,
            Phase::ChangedOnDisk => Some(if state.detail.as_deref() == Some("File missing") {
                "File missing"
            } else {
                "Changed externally"
            }),
            Phase::Conflict => Some("Conflict"),
            Phase::SaveFailed => Some("Save failed"),
        }
    }

    fn refresh_document_state(&self) {
        let state_label = &self.ivars().state_label;
        if let Some(state_title) = self.state_title() {
            state_label.setStringValue(&NSString::from_str(state_title));
            state_label.setHidden(false);
        } else {
            state_label.setStringValue(&NSString::from_str(""));
            state_label.setHidden(true);
        }
        self.refresh_tool_tip();
        self.refresh_emphasis();
        self.refresh_accessibility();
    }

    fn refresh_tool_tip(&self) {
        let base = self.identity_description();
        let Some(state) = self.state_description() else {
            set_tool_tip(self, if base.is_empty() { None } else { Some(&base) });
            return;
        };
        let tip = if base.is_empty() { state } else { format!("{base} — {state}") };
        set_tool_tip(self, Some(&tip));
    }

    fn refresh_emphasis(&self) {
        let ivars = self.ivars();
        let key = self.host_window().is_some_and(|window| window.isKeyWindow());
        let title_color = if key { NSColor::labelColor() } else { NSColor::secondaryLabelColor() };
        ivars.title_label.setTextColor(Some(&title_color));
        ivars.context_label.setTextColor(Some(&NSColor::tertiaryLabelColor()));
        let phase = ivars.document_state.borrow().phase;
        match phase {
            Phase::Conflict | Phase::SaveFailed => ivars.state_label.setTextColor(Some(&NSColor::systemRedColor())),
            Phase::ChangedOnDisk => ivars.state_label.setTextColor(Some(&StyleSheet::current(self.mtm()).accent)),
            Phase::Neutral | Phase::Edited | Phase::Saving | Phase::Saved => {
                ivars.state_label.setTextColor(Some(&NSColor::tertiaryLabelColor()))
            }
        }
        let url = self.host_window().and_then(|window| window.representedURL());
        self.update_proxy_icon(url.as_deref());
    }

    pub fn proxy_button_for_testing(&self) -> Retained<ToolbarInteractiveButton> {
        self.ivars().proxy_button.clone()
    }

    pub fn state_label_for_testing(&self) -> Retained<NSTextField> {
        self.ivars().state_label.clone()
    }
}

// MARK: - ToolbarPresentationControl

/// `ToolbarPresentationControl.Metrics`.
struct PresentationMetrics;

impl PresentationMetrics {
    const WIDTH: CGFloat = 184.0;
    const HEIGHT: CGFloat = 34.0;
    const SEGMENT_WIDTH: CGFloat = 91.0;
    const INDICATOR_WIDTH: CGFloat = 34.0;
    const INDICATOR_HEIGHT: CGFloat = 2.0;
}

pub struct ToolbarPresentationControlIvars {
    style_sheet: RefCell<Rc<StyleSheet>>,
    glass: Retained<ChromeGlass>,
    document_button: Retained<ToolbarModeButton>,
    source_button: Retained<ToolbarModeButton>,
    selection_indicator: Retained<CALayer>,
    indicator_center: RefCell<SpringScalar>,
    activation_observers: RefCell<Vec<ObserverToken>>,
    accessibility_observer: RefCell<Option<ObserverToken>>,
    scrubbed_segment: Cell<Option<isize>>,
    indicator_layout_width: Cell<Option<CGFloat>>,
    selected_segment: Cell<isize>,
    on_change: Box<dyn Fn(isize)>,
    perform_haptic_feedback: Box<dyn Fn()>,
}

impl Drop for ToolbarPresentationControlIvars {
    /// `deinit`.
    fn drop(&mut self) {
        let center = NSNotificationCenter::defaultCenter();
        for observer in self.activation_observers.get_mut().drain(..) {
            remove_observer(&center, &observer);
        }
        if let Some(observer) = self.accessibility_observer.get_mut().take() {
            remove_observer(&NSWorkspace::sharedWorkspace().notificationCenter(), &observer);
        }
    }
}

define_class!(
    /// A two-state titlebar rail for the document surface. Selection is
    /// expressed by typography and one baseline, not a capsule competing with
    /// the document.
    // SAFETY: `initWithFrame:` is forwarded to `SpringSurfaceView` in `new`
    // after the ivars are set; overrides keep their signatures.
    #[unsafe(super(SpringSurfaceView, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "ToolbarPresentationControl"]
    #[ivars = ToolbarPresentationControlIvars]
    pub struct ToolbarPresentationControl;

    unsafe impl NSObjectProtocol for ToolbarPresentationControl {}

    impl ToolbarPresentationControl {
        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            NSSize::new(PresentationMetrics::WIDTH, PresentationMetrics::HEIGHT)
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn __view_did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            self.stop_observing_window_activation();
            let Some(window) = self.window() else { return };
            // SAFETY: AppKit exports the notification names as immutable globals.
            let names = unsafe { [NSWindowDidBecomeKeyNotification, NSWindowDidResignKeyNotification] };
            let observers: Vec<ObserverToken> = names
                .into_iter()
                .map(|name| {
                    let weak = ObjcWeak::from(self);
                    observe_window_notification(name, &window, move || {
                        if let Some(this) = weak.load() {
                            this.refresh_window_emphasis(true);
                        }
                    })
                })
                .collect();
            *self.ivars().activation_observers.borrow_mut() = observers;
            self.refresh_window_emphasis(false);
        }

        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            let ivars = self.ivars();
            ivars.glass.setFrame(self.bounds());
            let width = self.bounds().width();
            if ivars.scrubbed_segment.get().is_none()
                && ivars.indicator_layout_width.get().map(|value| (value - width).abs() > 0.01).unwrap_or(true)
            {
                // Button layout happens below this view in AppKit's layout
                // pass. Derive the indicator from our stable capsule geometry
                // instead of sampling child frames before Auto Layout has
                // placed them.
                ivars.indicator_layout_width.set(Some(width));
                self.settle_indicator_on_layout();
            }
        }

        #[unsafe(method(springTick:))]
        fn __spring_tick(&self, dt: CGFloat) -> bool {
            self.ivars().indicator_center.borrow_mut().advance(dt)
        }

        #[unsafe(method(springApply))]
        fn __spring_apply(&self) {
            self.spring_apply();
        }

        #[unsafe(method(springsSettleImmediately))]
        fn __springs_settle_immediately(&self) {
            let target = self.indicator_center_x(self.ivars().selected_segment.get());
            self.ivars().indicator_center.borrow_mut().snap(target);
            self.spring_apply();
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            let accent = self.ivars().style_sheet.borrow().accent.clone();
            self.ivars().selection_indicator.setBackgroundColor(Some(&cg(&accent)));
            self.refresh_window_emphasis(false);
        }

        #[unsafe(method(segmentPressed:))]
        fn __segment_pressed(&self, sender: &NSButton) {
            let tag = sender.tag();
            if tag == self.ivars().selected_segment.get() {
                return;
            }
            self.set_selected_segment(tag);
            (self.ivars().on_change)(tag);
        }

        #[unsafe(method(scrubSelection:))]
        fn __scrub_selection(&self, recognizer: &NSPanGestureRecognizer) {
            self.scrub_selection(recognizer);
        }
    }
);

impl ToolbarPresentationControl {
    /// `init(onChange:)` with Swift's default haptic performer
    /// (`NSHapticFeedbackManager.defaultPerformer.perform(.alignment,
    /// performanceTime: .now)`).
    pub fn new(on_change: impl Fn(isize) + 'static, mtm: MainThreadMarker) -> Retained<ToolbarPresentationControl> {
        Self::new_with_haptics(
            on_change,
            || {
                NSHapticFeedbackManager::defaultPerformer().performFeedbackPattern_performanceTime(
                    NSHapticFeedbackPattern::Alignment,
                    NSHapticFeedbackPerformanceTime::Now,
                );
            },
            mtm,
        )
    }

    /// `init(onChange:performHapticFeedback:)`.
    pub fn new_with_haptics(
        on_change: impl Fn(isize) + 'static,
        perform_haptic_feedback: impl Fn() + 'static,
        mtm: MainThreadMarker,
    ) -> Retained<ToolbarPresentationControl> {
        // The stored properties' initial values, then the init body's
        // assignments before `super.init`, in Swift's order.
        let style_sheet = Rc::new(StyleSheet::current(mtm));
        let selection_indicator = CALayer::new();
        let indicator_center = SpringScalar::new(0.0, 0.0, motion::SPRING_QUICK, 0.08);
        let glass = ChromeGlass::new(Rc::new(StyleSheet::current(mtm)), 12.0, RoundedCorners::All, Tint::Control, mtm);
        let document_button = ToolbarModeButton::new("Document", Typography::Document, "Document", mtm);
        let source_button = ToolbarModeButton::new("Source", Typography::Source, "Source", mtm);
        let this = Self::alloc(mtm).set_ivars(ToolbarPresentationControlIvars {
            style_sheet: RefCell::new(style_sheet),
            glass,
            document_button,
            source_button,
            selection_indicator,
            indicator_center: RefCell::new(indicator_center),
            activation_observers: RefCell::new(Vec::new()),
            accessibility_observer: RefCell::new(None),
            scrubbed_segment: Cell::new(None),
            indicator_layout_width: Cell::new(None),
            selected_segment: Cell::new(-1),
            on_change: Box::new(on_change),
            perform_haptic_feedback: Box::new(perform_haptic_feedback),
        });
        let this: Retained<ToolbarPresentationControl> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        this.finish_init();
        this
    }

    fn finish_init(&self) {
        let ivars = self.ivars();
        self.setWantsLayer(true);
        let glass = &ivars.glass;
        glass.set_shadow_radius(8.0);
        glass.set_shadow_offset(CGSize::new(0.0, -2.0));
        glass.set_shadow_opacity(Some(0.08));
        glass.setAutoresizingMask(
            objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable
                | objc2_app_kit::NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        glass.setFrame(self.bounds());
        self.addSubview(glass);
        let content_view = glass.content_view();
        content_view.setWantsLayer(true);
        ivars.selection_indicator.setCornerRadius(PresentationMetrics::INDICATOR_HEIGHT / 2.0);
        if let Some(layer) = content_view.layer() {
            layer.addSublayer(&ivars.selection_indicator);
        }

        let document_button = &ivars.document_button;
        let source_button = &ivars.source_button;
        document_button.setTag(0);
        source_button.setTag(1);
        // SAFETY: the target is this control, which owns both buttons.
        unsafe {
            document_button.setTarget(Some(object(self)));
            source_button.setTarget(Some(object(self)));
            document_button.setAction(Some(sel!(segmentPressed:)));
            source_button.setAction(Some(sel!(segmentPressed:)));
        }
        let weak = ObjcWeak::from(self);
        document_button.set_on_navigate(Some(Rc::new(move |target| {
            if let Some(this) = weak.load() {
                this.select_from_keyboard(target);
            }
        })));
        let weak = ObjcWeak::from(self);
        source_button.set_on_navigate(Some(Rc::new(move |target| {
            if let Some(this) = weak.load() {
                this.select_from_keyboard(target);
            }
        })));
        // SAFETY: the recognizer's target is this view, which owns it.
        let pan = unsafe {
            NSPanGestureRecognizer::initWithTarget_action(
                NSPanGestureRecognizer::alloc(self.mtm()),
                Some(object(self)),
                Some(sel!(scrubSelection:)),
            )
        };
        self.addGestureRecognizer(&pan);

        for button in [document_button, source_button] {
            button.setTranslatesAutoresizingMaskIntoConstraints(false);
            content_view.addSubview(button);
        }

        activate(&[
            document_button.leadingAnchor().constraintEqualToAnchor_constant(&content_view.leadingAnchor(), 1.0),
            document_button.topAnchor().constraintEqualToAnchor_constant(&content_view.topAnchor(), 1.0),
            document_button.bottomAnchor().constraintEqualToAnchor_constant(&content_view.bottomAnchor(), -1.0),
            source_button.topAnchor().constraintEqualToAnchor(&document_button.topAnchor()),
            source_button.bottomAnchor().constraintEqualToAnchor(&document_button.bottomAnchor()),
            source_button.trailingAnchor().constraintEqualToAnchor_constant(&content_view.trailingAnchor(), -1.0),
            source_button.leadingAnchor().constraintEqualToAnchor(&document_button.trailingAnchor()),
            document_button.widthAnchor().constraintEqualToConstant(PresentationMetrics::SEGMENT_WIDTH),
            source_button.widthAnchor().constraintEqualToAnchor(&document_button.widthAnchor()),
        ]);

        self.set_selected_segment(0);
        set_role(self, role::group());
        set_label(self, "Source Focus");
        set_help(self, "Switch between rendered Document and Source Focus");
        set_tool_tip(document_button, Some("Rendered document"));
        set_tool_tip(source_button, Some("Source Focus — show raw Markdown"));
        let weak = ObjcWeak::from(self);
        let observer = observe_accessibility_display_options(move || {
            if let Some(this) = weak.load() {
                this.refresh_window_emphasis(false);
            }
        });
        *ivars.accessibility_observer.borrow_mut() = Some(observer);
    }

    /// `styleSheet`.
    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    /// `styleSheet { didSet }`.
    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet.clone();
        let ivars = self.ivars();
        ivars.glass.set_style_sheet(style_sheet.clone());
        ivars.document_button.set_style_sheet(style_sheet.clone());
        ivars.source_button.set_style_sheet(style_sheet.clone());
        ivars.selection_indicator.setBackgroundColor(Some(&cg(&style_sheet.accent)));
    }

    /// `selectedSegment` (`-1` until the first selection).
    pub fn selected_segment(&self) -> isize {
        self.ivars().selected_segment.get()
    }

    /// `segmentTitles`.
    pub fn segment_titles(&self) -> Vec<String> {
        vec![
            self.ivars().document_button.display_title().to_owned(),
            self.ivars().source_button.display_title().to_owned(),
        ]
    }

    /// `setSelectedSegment(_:)`.
    pub fn set_selected_segment(&self, segment: isize) {
        let normalized = segment.clamp(0, 1);
        if normalized == self.ivars().selected_segment.get() {
            return;
        }
        self.ivars().selected_segment.set(normalized);
        self.apply_visual_selection(normalized);
        set_value(self, &self.segment_titles()[normalized as usize]);
        self.update_selection_indicator(self.window().is_some());
    }

    fn select_from_keyboard(&self, target: isize) {
        if target == self.ivars().selected_segment.get() {
            return;
        }
        self.set_selected_segment(target);
        (self.ivars().on_change)(target);
        if let Some(window) = self.window() {
            let button: &NSResponder =
                if target == 0 { &self.ivars().document_button } else { &self.ivars().source_button };
            window.makeFirstResponder(Some(button));
        }
    }

    fn scrub_selection(&self, recognizer: &NSPanGestureRecognizer) {
        let state: NSGestureRecognizerState = unsafe { msg_send![recognizer, state] };
        let x = recognizer.locationInView(Some(self)).x;
        match state {
            NSGestureRecognizerState::Began => self.update_scrub(x, ToolbarScrubPhase::Began),
            NSGestureRecognizerState::Changed => self.update_scrub(x, ToolbarScrubPhase::Changed),
            NSGestureRecognizerState::Ended => self.update_scrub(x, ToolbarScrubPhase::Ended),
            NSGestureRecognizerState::Cancelled | NSGestureRecognizerState::Failed => {
                self.update_scrub(x, ToolbarScrubPhase::Cancelled)
            }
            NSGestureRecognizerState::Possible => {}
            _ => self.update_scrub(x, ToolbarScrubPhase::Cancelled),
        }
    }

    /// `updateScrub(at:phase:)`.
    pub fn update_scrub(&self, pointer_x: CGFloat, phase: ToolbarScrubPhase) {
        let (left, right) = self.segment_centers();
        let state = ToolbarChromePolicy::scrub_state(pointer_x, left, right);
        let ivars = self.ivars();
        match phase {
            ToolbarScrubPhase::Began | ToolbarScrubPhase::Changed => {
                let previous_segment = ivars.scrubbed_segment.replace(Some(state.segment));
                self.apply_visual_selection(state.segment);
                self.update_selection_indicator_to(state.indicator_center_x);
                if let Some(previous_segment) = previous_segment
                    && previous_segment != state.segment
                {
                    (ivars.perform_haptic_feedback)();
                }
            }
            ToolbarScrubPhase::Ended => {
                ivars.scrubbed_segment.set(None);
                self.commit_scrubbed_segment(state.segment);
            }
            ToolbarScrubPhase::Cancelled => {
                ivars.scrubbed_segment.set(None);
                self.apply_visual_selection(ivars.selected_segment.get());
                self.update_selection_indicator(true);
            }
        }
    }

    /// `trackSwipe(position:)`: track a gesture that owns the mode change
    /// itself — the two-finger swipe over the document. Unlike
    /// `update_scrub`, this never calls `onChange`. `position` runs `0` at
    /// Document to `1` at Source.
    pub fn track_swipe(&self, position: CGFloat) {
        let (left, right) = self.segment_centers();
        let state = ToolbarChromePolicy::scrub_state_for_position(position, left, right);
        let ivars = self.ivars();
        let previous_segment = ivars.scrubbed_segment.replace(Some(state.segment));
        self.apply_visual_selection(state.segment);
        self.update_selection_indicator_to(state.indicator_center_x);
        if let Some(previous_segment) = previous_segment
            && previous_segment != state.segment
        {
            (ivars.perform_haptic_feedback)();
        }
    }

    /// `settleSwipe(at:)`: land the indicator on a segment after such a
    /// gesture. Unconditional rather than a `set_selected_segment` no-op,
    /// because a cancelled swipe ends on the segment it started from with the
    /// rail scrubbed away from it.
    pub fn settle_swipe(&self, segment: isize) {
        let normalized = segment.clamp(0, 1);
        let ivars = self.ivars();
        ivars.scrubbed_segment.set(None);
        ivars.selected_segment.set(normalized);
        set_value(self, &self.segment_titles()[normalized as usize]);
        self.apply_visual_selection(normalized);
        self.update_selection_indicator(self.window().is_some());
    }

    fn apply_visual_selection(&self, segment: isize) {
        self.ivars().document_button.set_is_selected(segment == 0);
        self.ivars().source_button.set_is_selected(segment == 1);
    }

    fn commit_scrubbed_segment(&self, segment: isize) {
        if segment == self.ivars().selected_segment.get() {
            self.apply_visual_selection(self.ivars().selected_segment.get());
            self.update_selection_indicator(true);
            return;
        }
        self.set_selected_segment(segment);
        (self.ivars().on_change)(segment);
    }

    /// `updateSelectionIndicator(centerX:)`.
    fn update_selection_indicator_to(&self, center_x: CGFloat) {
        self.park_springs();
        self.ivars().indicator_center.borrow_mut().snap(center_x);
        self.spring_apply();
    }

    /// `updateSelectionIndicator(animated:)`.
    fn update_selection_indicator(&self, animated: bool) {
        let ivars = self.ivars();
        let selected_segment = ivars.selected_segment.get();
        if selected_segment < 0 {
            return;
        }
        let center_x = self.indicator_center_x(selected_segment);
        let style_sheet = ivars.style_sheet.borrow().clone();
        ivars.selection_indicator.setBackgroundColor(Some(&cg(&style_sheet.accent)));
        if !(animated && self.window().is_some() && !style_sheet.reduce_motion) {
            self.park_springs();
            ivars.indicator_center.borrow_mut().snap(center_x);
            self.spring_apply();
            return;
        }
        ivars.indicator_center.borrow_mut().target(center_x);
        if !self.arm_springs() {
            ivars.indicator_center.borrow_mut().snap(center_x);
            self.spring_apply();
        }
    }

    /// `segmentCenters`: `(left, right)`.
    fn segment_centers(&self) -> (CGFloat, CGFloat) {
        let inset: CGFloat = 1.0;
        let available_width = swift_max(0.0, self.bounds().width() - 2.0 * inset);
        let segment_width = available_width / 2.0;
        (inset + segment_width / 2.0, inset + segment_width * 1.5)
    }

    fn indicator_center_x(&self, segment: isize) -> CGFloat {
        if segment == 0 { self.segment_centers().0 } else { self.segment_centers().1 }
    }

    fn settle_indicator_on_layout(&self) {
        let selected_segment = self.ivars().selected_segment.get();
        if selected_segment < 0 {
            return;
        }
        self.park_springs();
        let center_x = self.indicator_center_x(selected_segment);
        self.ivars().indicator_center.borrow_mut().snap(center_x);
        self.spring_apply();
    }

    /// `springApply()`.
    fn spring_apply(&self) {
        let value = self.ivars().indicator_center.borrow().value();
        self.place_selection_indicator(value);
    }

    fn place_selection_indicator(&self, center_x: CGFloat) {
        let indicator = &self.ivars().selection_indicator;
        without_actions(|| {
            indicator.setFrame(rect(
                center_x - PresentationMetrics::INDICATOR_WIDTH / 2.0,
                4.0,
                PresentationMetrics::INDICATOR_WIDTH,
                PresentationMetrics::INDICATOR_HEIGHT,
            ));
        });
    }

    fn refresh_window_emphasis(&self, animated: bool) {
        let ivars = self.ivars();
        let active = self.window().is_some_and(|window| window.isKeyWindow());
        ivars.document_button.set_window_active(active);
        ivars.source_button.set_window_active(active);
        let opacity = ToolbarChromePolicy::indicator_opacity(active, increase_contrast());
        let indicator = &ivars.selection_indicator;
        let reduce_motion = ivars.style_sheet.borrow().reduce_motion;
        if !animated || reduce_motion {
            indicator.removeAnimationForKey(&NSString::from_str("window-emphasis"));
            indicator.setOpacity(opacity);
            return;
        }
        let animation = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("opacity")));
        let from = indicator
            .__presentation()
            .map(|presentation| presentation.opacity())
            .unwrap_or_else(|| indicator.opacity());
        set_number_values(&animation, Some(from as f64), opacity as f64);
        animation.setDuration(ToolbarChromePolicy::EMPHASIS_DURATION);
        animation.setTimingFunction(Some(&ToolbarChromePolicy::timing_function()));
        indicator.addAnimation_forKey(&animation, Some(&NSString::from_str("window-emphasis")));
        indicator.setOpacity(opacity);
    }

    fn stop_observing_window_activation(&self) {
        let center = NSNotificationCenter::defaultCenter();
        let observers: Vec<ObserverToken> = self.ivars().activation_observers.borrow_mut().drain(..).collect();
        for observer in &observers {
            remove_observer(&center, observer);
        }
    }

    /// `selectionIndicatorFrameForTesting`.
    pub fn selection_indicator_frame_for_testing(&self) -> NSRect {
        self.ivars().selection_indicator.frame()
    }

    /// `selectedSegmentCenterForTesting`.
    pub fn selected_segment_center_for_testing(&self) -> CGFloat {
        self.indicator_center_x(self.ivars().selected_segment.get())
    }

    /// The two segment buttons (Document, Source), for tests.
    pub fn segment_buttons_for_testing(&self) -> [Retained<NSButton>; 2] {
        let document: &NSButton = &self.ivars().document_button;
        let source: &NSButton = &self.ivars().source_button;
        [document.retain(), source.retain()]
    }
}

// MARK: - ToolbarInteractiveButton

pub struct ToolbarInteractiveButtonIvars {
    style_sheet: RefCell<Rc<StyleSheet>>,
    feedback_inset_x: Cell<CGFloat>,
    feedback_inset_y: Cell<CGFloat>,
    feedback_corner_radius: Cell<CGFloat>,
    feedback_layer: Retained<CALayer>,
    is_pointer_inside: Cell<bool>,
    is_pressed_for_feedback: Cell<bool>,
    accessibility_observer: RefCell<Option<ObserverToken>>,
}

impl Drop for ToolbarInteractiveButtonIvars {
    fn drop(&mut self) {
        if let Some(observer) = self.accessibility_observer.get_mut().take() {
            remove_observer(&NSWorkspace::sharedWorkspace().notificationCenter(), &observer);
        }
    }
}

define_class!(
    /// Shared compositor-only pointer feedback for titlebar buttons. Not
    /// final in Swift: subclasses override `styleSheetDidChange` and
    /// `permitsHoverFeedback`, which are Objective-C methods here so the base
    /// dispatches to an override.
    // SAFETY: `initWithFrame:` sets the ivars, so every initialiser path
    // (including AppKit's class factories) creates a valid instance.
    #[unsafe(super(NSButton, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "ToolbarInteractiveButton"]
    #[ivars = ToolbarInteractiveButtonIvars]
    pub struct ToolbarInteractiveButton;

    unsafe impl NSObjectProtocol for ToolbarInteractiveButton {}

    impl ToolbarInteractiveButton {
        #[unsafe(method_id(initWithFrame:))]
        fn __init_with_frame(this: Allocated<Self>, frame: NSRect) -> Retained<Self> {
            let mtm = MainThreadMarker::new().expect("ToolbarInteractiveButton is created on the main thread");
            let this = this.set_ivars(ToolbarInteractiveButtonIvars {
                style_sheet: RefCell::new(Rc::new(StyleSheet::current(mtm))),
                feedback_inset_x: Cell::new(5.0),
                feedback_inset_y: Cell::new(3.0),
                feedback_corner_radius: Cell::new(5.0),
                feedback_layer: CALayer::new(),
                is_pointer_inside: Cell::new(false),
                is_pressed_for_feedback: Cell::new(false),
                accessibility_observer: RefCell::new(None),
            });
            let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
            this.setWantsLayer(true);
            this.ivars().feedback_layer.setOpacity(0.0);
            if let Some(layer) = this.layer() {
                layer.insertSublayer_atIndex(&this.ivars().feedback_layer, 0);
            }
            this.refresh_feedback_color();
            let weak: ObjcWeak<ToolbarInteractiveButton> = ObjcWeak::from(&*this);
            let observer = observe_accessibility_display_options(move || {
                if let Some(this) = weak.load() {
                    this.refresh_interaction_feedback(false);
                }
            });
            *this.ivars().accessibility_observer.borrow_mut() = Some(observer);
            this
        }

        /// Overridable hook, called after `styleSheet` changes.
        #[unsafe(method(styleSheetDidChange))]
        fn __style_sheet_did_change(&self) {}

        /// Overridable: whether hover shows the feedback plate.
        #[unsafe(method(permitsHoverFeedback))]
        fn __permits_hover_feedback(&self) -> bool {
            true
        }

        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            let ivars = self.ivars();
            without_actions(|| {
                ivars
                    .feedback_layer
                    .setFrame(self.bounds().inset_by(ivars.feedback_inset_x.get(), ivars.feedback_inset_y.get()));
                ivars.feedback_layer.setCornerRadius(ivars.feedback_corner_radius.get());
            });
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.refresh_feedback_color();
        }

        #[unsafe(method(updateTrackingAreas))]
        fn __update_tracking_areas(&self) {
            for area in self.trackingAreas().iter() {
                self.removeTrackingArea(&area);
            }
            // SAFETY: the owner is the view itself.
            let area = unsafe {
                NSTrackingArea::initWithRect_options_owner_userInfo(
                    NSTrackingArea::alloc(),
                    self.bounds(),
                    NSTrackingAreaOptions::ActiveInKeyWindow
                        | NSTrackingAreaOptions::InVisibleRect
                        | NSTrackingAreaOptions::MouseEnteredAndExited,
                    Some(self),
                    None,
                )
            };
            self.addTrackingArea(&area);
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
        }

        #[unsafe(method(mouseEntered:))]
        fn __mouse_entered(&self, _event: &NSEvent) {
            self.ivars().is_pointer_inside.set(true);
            self.refresh_interaction_feedback(true);
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, _event: &NSEvent) {
            self.ivars().is_pointer_inside.set(false);
            self.refresh_interaction_feedback(true);
        }

        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, event: &NSEvent) {
            self.set_pressed_feedback(true);
            let _: () = unsafe { msg_send![super(self), mouseDown: event] };
            self.set_pressed_feedback(false);
        }
    }
);

impl ToolbarInteractiveButton {
    /// `ToolbarInteractiveButton(frame:)`.
    pub fn new(frame: NSRect, mtm: MainThreadMarker) -> Retained<ToolbarInteractiveButton> {
        unsafe { msg_send![ToolbarInteractiveButton::alloc(mtm), initWithFrame: frame] }
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    /// `styleSheet { didSet { styleSheetDidChange() } }`.
    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet;
        let _: () = unsafe { msg_send![self, styleSheetDidChange] };
    }

    pub fn feedback_inset_x(&self) -> CGFloat {
        self.ivars().feedback_inset_x.get()
    }

    pub fn set_feedback_inset_x(&self, value: CGFloat) {
        self.ivars().feedback_inset_x.set(value);
    }

    pub fn feedback_inset_y(&self) -> CGFloat {
        self.ivars().feedback_inset_y.get()
    }

    pub fn set_feedback_inset_y(&self, value: CGFloat) {
        self.ivars().feedback_inset_y.set(value);
    }

    pub fn feedback_corner_radius(&self) -> CGFloat {
        self.ivars().feedback_corner_radius.get()
    }

    pub fn set_feedback_corner_radius(&self, value: CGFloat) {
        self.ivars().feedback_corner_radius.set(value);
    }

    fn permits_hover_feedback(&self) -> bool {
        unsafe { msg_send![self, permitsHoverFeedback] }
    }

    pub fn set_pressed_feedback(&self, pressed: bool) {
        if pressed == self.ivars().is_pressed_for_feedback.get() {
            return;
        }
        self.ivars().is_pressed_for_feedback.set(pressed);
        self.refresh_interaction_feedback(true);
        self.update_press_transform(true);
    }

    pub fn refresh_interaction_feedback(&self, animated: bool) {
        let ivars = self.ivars();
        let state = if ivars.is_pressed_for_feedback.get() {
            InteractionState::Pressed
        } else if ivars.is_pointer_inside.get() && self.permits_hover_feedback() {
            InteractionState::Hover
        } else {
            InteractionState::Idle
        };
        let target_opacity = ToolbarChromePolicy::feedback_opacity(state, increase_contrast());
        self.animate_feedback_opacity(target_opacity, animated);
    }

    fn refresh_feedback_color(&self) {
        self.ivars().feedback_layer.setBackgroundColor(Some(&cg(&NSColor::labelColor())));
    }

    fn animate_feedback_opacity(&self, opacity: f32, animated: bool) {
        let layer = &self.ivars().feedback_layer;
        let reduce_motion = self.ivars().style_sheet.borrow().reduce_motion;
        if !animated || reduce_motion {
            layer.removeAnimationForKey(&NSString::from_str("feedback-opacity"));
            layer.setOpacity(opacity);
            return;
        }
        let animation = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("opacity")));
        let from = layer.__presentation().map(|presentation| presentation.opacity()).unwrap_or_else(|| layer.opacity());
        set_number_values(&animation, Some(from as f64), opacity as f64);
        animation.setDuration(ToolbarChromePolicy::HOVER_DURATION);
        animation.setTimingFunction(Some(&ToolbarChromePolicy::timing_function()));
        layer.addAnimation_forKey(&animation, Some(&NSString::from_str("feedback-opacity")));
        layer.setOpacity(opacity);
    }

    fn update_press_transform(&self, animated: bool) {
        let pressed = self.ivars().is_pressed_for_feedback.get();
        let scale = if pressed { ToolbarChromePolicy::PRESSED_SCALE } else { 1.0 };
        let transform = CATransform3D::new_scale(scale, scale, 1.0);
        let reduce_motion = self.ivars().style_sheet.borrow().reduce_motion;
        let Some(layer) = self.layer() else { return };
        if !animated || reduce_motion {
            layer.setTransform(transform);
            return;
        }
        let animation = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("transform")));
        set_transform_values(&animation, presentation_transform(&layer), transform);
        animation.setDuration(if pressed {
            ToolbarChromePolicy::PRESS_IN_DURATION
        } else {
            ToolbarChromePolicy::PRESS_OUT_DURATION
        });
        animation.setTimingFunction(Some(&ToolbarChromePolicy::timing_function()));
        layer.addAnimation_forKey(&animation, Some(&NSString::from_str("press-transform")));
        layer.setTransform(transform);
    }

    pub fn feedback_layer_for_testing(&self) -> Retained<CALayer> {
        self.ivars().feedback_layer.clone()
    }
}

// MARK: - ToolbarModeButton

/// `ToolbarModeButton.Typography`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Typography {
    Document,
    Source,
}

struct ToolbarModeButtonIvars {
    display_title: String,
    typography: Typography,
    is_window_active: Cell<bool>,
    on_navigate: RefCell<Option<Rc<dyn Fn(isize)>>>,
    is_selected: Cell<bool>,
}

define_class!(
    /// One segment in `ToolbarPresentationControl` (a private Swift class).
    // SAFETY: `new` sets the ivars before forwarding to
    // `ToolbarInteractiveButton`'s `initWithFrame:`.
    #[unsafe(super(ToolbarInteractiveButton, NSButton, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "ToolbarModeButton"]
    #[ivars = ToolbarModeButtonIvars]
    struct ToolbarModeButton;

    unsafe impl NSObjectProtocol for ToolbarModeButton {}

    impl ToolbarModeButton {
        #[unsafe(method(permitsHoverFeedback))]
        fn __permits_hover_feedback(&self) -> bool {
            !self.ivars().is_selected.get()
        }

        #[unsafe(method(keyDown:))]
        fn __key_down(&self, event: &NSEvent) {
            match event.keyCode() {
                123 => self.navigate(0),
                124 => self.navigate(1),
                _ => {
                    let _: () = unsafe { msg_send![super(self), keyDown: event] };
                }
            }
        }
    }
);

impl ToolbarModeButton {
    /// `init(title:typography:accessibilityLabel:)`.
    fn new(title: &str, typography: Typography, accessibility_label: &str, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ToolbarModeButtonIvars {
            display_title: title.to_owned(),
            typography,
            is_window_active: Cell::new(true),
            on_navigate: RefCell::new(None),
            is_selected: Cell::new(false),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        this.setTitle(&ns_string(title));
        // Swift's `.regularSquare` (`NSBezelStyleRegularSquare`, renamed
        // `FlexiblePush`).
        this.setBezelStyle(NSBezelStyle::FlexiblePush);
        this.setControlSize(NSControlSize::Small);
        this.setBordered(false);
        this.setAlignment(NSTextAlignment::Center);
        this.setFocusRingType(NSFocusRingType::Default);
        set_role(&*this, role::radio_button());
        set_label(&*this, accessibility_label);
        set_tool_tip(&this, Some(accessibility_label));
        this.update_title();
        this
    }

    fn display_title(&self) -> &str {
        &self.ivars().display_title
    }

    fn set_on_navigate(&self, handler: Option<Rc<dyn Fn(isize)>>) {
        *self.ivars().on_navigate.borrow_mut() = handler;
    }

    fn navigate(&self, target: isize) {
        let handler = self.ivars().on_navigate.borrow().clone();
        if let Some(handler) = handler {
            handler(target);
        }
    }

    /// `isSelected { didSet }`.
    fn set_is_selected(&self, value: bool) {
        let old_value = self.ivars().is_selected.replace(value);
        if value == old_value {
            return;
        }
        self.update_title();
        set_value(self, if value { "Selected" } else { "Not selected" });
        self.refresh_interaction_feedback(true);
    }

    fn update_title(&self) {
        let selected = self.ivars().is_selected.get();
        let weight = if selected { weight_semibold() } else { weight_medium() };
        let font = match self.ivars().typography {
            Typography::Document => system_font(12.0, weight),
            Typography::Source => NSFont::monospacedSystemFontOfSize_weight(11.5, weight),
        };
        let color = self.title_color();
        let title = upleft_render::appkit_compat::attributed_string(
            &self.ivars().display_title,
            &[(keys::font(), object(&*font)), (keys::foreground_color(), object(&*color))],
        );
        self.setAttributedTitle(&title);
    }

    fn title_color(&self) -> Retained<NSColor> {
        let increase_contrast = increase_contrast();
        match (self.ivars().is_window_active.get(), self.ivars().is_selected.get()) {
            (true, true) => NSColor::labelColor(),
            (true, false) if increase_contrast => NSColor::labelColor(),
            (true, false) | (false, true) => NSColor::secondaryLabelColor(),
            (false, false) => NSColor::tertiaryLabelColor(),
        }
    }

    fn set_window_active(&self, active: bool) {
        if active == self.ivars().is_window_active.get() {
            return;
        }
        self.ivars().is_window_active.set(active);
        self.update_title();
    }
}

// MARK: - ToolbarMenuButton

/// `ToolbarMenuButton.Metrics` and `ToolbarActionButton.Metrics`.
struct ButtonMetrics;

impl ButtonMetrics {
    const SIDE: CGFloat = PanelMetrics::TOOLBAR_CONTROL_SIDE;
    const CORNER_RADIUS: CGFloat = 7.0;
}

pub struct ToolbarMenuButtonIvars {
    popup_menu: Retained<NSMenu>,
}

define_class!(
    /// Compact trailing menu button. `NSMenuToolbarItem` adds a second pill
    /// and chevron around the symbol; this keeps one deliberate icon and lets
    /// the menu itself provide the disclosure affordance when opened.
    // SAFETY: `new` sets the ivars before forwarding to
    // `ToolbarInteractiveButton`'s `initWithFrame:`.
    #[unsafe(super(ToolbarInteractiveButton, NSButton, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "ToolbarMenuButton"]
    #[ivars = ToolbarMenuButtonIvars]
    pub struct ToolbarMenuButton;

    unsafe impl NSObjectProtocol for ToolbarMenuButton {}

    impl ToolbarMenuButton {
        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            NSSize::new(ButtonMetrics::SIDE, ButtonMetrics::SIDE)
        }

        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, _event: &NSEvent) {
            self.present_menu();
        }

        #[unsafe(method(showMenu:))]
        fn __show_menu(&self, _sender: Option<&AnyObject>) {
            self.present_menu();
        }

        #[unsafe(method(accessibilityPerformPress))]
        fn __accessibility_perform_press(&self) -> bool {
            self.present_menu();
            true
        }
    }
);

impl ToolbarMenuButton {
    /// `init(menu:)`.
    pub fn new(menu: &NSMenu, mtm: MainThreadMarker) -> Retained<ToolbarMenuButton> {
        let this = Self::alloc(mtm).set_ivars(ToolbarMenuButtonIvars { popup_menu: menu.retain() });
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        this.set_feedback_inset_x(1.0);
        this.set_feedback_inset_y(1.0);
        this.set_feedback_corner_radius(ButtonMetrics::CORNER_RADIUS);
        this.setImage(
            configured_symbol("ellipsis", Some("More actions"), &symbol_configuration(13.0, weight_medium()))
                .as_deref(),
        );
        this.setImagePosition(NSCellImagePosition::ImageOnly);
        this.setImageScaling(NSImageScaling::ScaleProportionallyDown);
        this.setBezelStyle(NSBezelStyle::AccessoryBarAction);
        this.setControlSize(NSControlSize::Regular);
        this.setBordered(false);
        this.setFocusRingType(NSFocusRingType::Default);
        // A real target/action is what makes Space and Return reach the menu:
        // `mouseDown` never calls super, so without this the button had no
        // keyboard path at all and only VoiceOver's press worked.
        // SAFETY: the button is its own target.
        unsafe {
            this.setTarget(Some(object(&*this)));
            this.setAction(Some(sel!(showMenu:)));
        }
        set_role(&*this, role::pop_up_button());
        set_label(&*this, "More actions");
        set_help(&*this, "Open document actions");
        set_tool_tip(&this, Some("More document actions"));
        this
    }

    /// `popupMenuItems`.
    pub fn popup_menu_items(&self) -> Vec<Retained<NSMenuItem>> {
        self.ivars().popup_menu.itemArray().to_vec()
    }

    fn present_menu(&self) {
        self.set_pressed_feedback(true);
        let bounds = self.bounds();
        self.ivars().popup_menu.popUpMenuPositioningItem_atLocation_inView(
            None,
            NSPoint::new(bounds.max_x(), bounds.min_y()),
            Some(self),
        );
        self.set_pressed_feedback(false);
    }
}

// MARK: - ToolbarActionButton

pub struct ToolbarActionButtonIvars {
    glass: RefCell<Option<Retained<ChromeGlass>>>,
    symbol_view: Retained<NSImageView>,
    is_on: Cell<bool>,
}

define_class!(
    /// A plain symbol button for the toolbar's trailing cluster. It shares
    /// `ToolbarInteractiveButton`'s hover plate and press feedback with
    /// `ToolbarMenuButton`, and its square is the same geometry.
    // SAFETY: `new` sets the ivars before forwarding to
    // `ToolbarInteractiveButton`'s `initWithFrame:`.
    #[unsafe(super(ToolbarInteractiveButton, NSButton, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "ToolbarActionButton"]
    #[ivars = ToolbarActionButtonIvars]
    pub struct ToolbarActionButton;

    unsafe impl NSObjectProtocol for ToolbarActionButton {}

    impl ToolbarActionButton {
        #[unsafe(method(styleSheetDidChange))]
        fn __style_sheet_did_change(&self) {
            self.apply_tint();
        }

        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            NSSize::new(ButtonMetrics::SIDE, ButtonMetrics::SIDE)
        }

        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            let glass = self.ivars().glass.borrow().clone();
            if let Some(glass) = glass {
                glass.setFrame(self.bounds());
            }
            self.ivars().symbol_view.setFrame(self.bounds().inset_by(8.0, 8.0));
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.apply_tint();
        }
    }
);

impl ToolbarActionButton {
    /// `init(symbol:label:help:target:action:usesGlassSurface:)`; Swift's
    /// `usesGlassSurface` defaults to `false`. `target` is held weakly, as an
    /// `NSControl` target is.
    pub fn new(
        symbol: &str,
        label: &str,
        help: &str,
        target: Option<&AnyObject>,
        action: Sel,
        uses_glass_surface: bool,
        mtm: MainThreadMarker,
    ) -> Retained<ToolbarActionButton> {
        let this = Self::alloc(mtm).set_ivars(ToolbarActionButtonIvars {
            glass: RefCell::new(None),
            symbol_view: NSImageView::new(mtm),
            is_on: Cell::new(false),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        this.set_feedback_inset_x(1.0);
        this.set_feedback_inset_y(1.0);
        this.set_feedback_corner_radius(ButtonMetrics::CORNER_RADIUS);
        let configured_image = configured_symbol(
            symbol,
            Some(label),
            &symbol_configuration(if uses_glass_surface { 15.0 } else { 13.0 }, weight_medium()),
        );
        this.setImage(if uses_glass_surface { None } else { configured_image.as_deref() });
        this.setImagePosition(NSCellImagePosition::ImageOnly);
        this.setImageScaling(NSImageScaling::ScaleProportionallyDown);
        this.setBezelStyle(NSBezelStyle::AccessoryBarAction);
        this.setControlSize(NSControlSize::Regular);
        this.setBordered(false);
        // SAFETY: the caller keeps the target alive for the button's life,
        // as AppKit requires of any control target.
        unsafe {
            this.setTarget(target);
            this.setAction(Some(action));
        }
        set_role(&*this, role::button());
        set_label(&*this, label);
        set_help(&*this, help);
        set_tool_tip(&this, Some(help));
        if uses_glass_surface {
            // The glass is circular. A rounded-square hover layer beneath it
            // leaked around the rim and looked like a second button. Keep the
            // hit target square, but make its visual feedback the same circle.
            this.set_feedback_inset_x(0.0);
            this.set_feedback_inset_y(0.0);
            this.set_feedback_corner_radius(ButtonMetrics::SIDE / 2.0);
            let glass = ChromeGlass::new(
                this.style_sheet(),
                PanelMetrics::TOOLBAR_CONTROL_SIDE / 2.0,
                RoundedCorners::All,
                Tint::Control,
                mtm,
            );
            glass.set_passes_through_hits(true);
            glass.set_shadow_radius(8.0);
            glass.set_shadow_offset(CGSize::new(0.0, -2.0));
            glass.set_shadow_opacity(Some(0.10));
            glass.setAutoresizingMask(
                objc2_app_kit::NSAutoresizingMaskOptions::ViewWidthSizable
                    | objc2_app_kit::NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            this.addSubview(&glass);
            let symbol_view = &this.ivars().symbol_view;
            symbol_view.setImage(configured_image.as_deref());
            symbol_view.setImageScaling(NSImageScaling::ScaleProportionallyDown);
            symbol_view.setAutoresizingMask(
                objc2_app_kit::NSAutoresizingMaskOptions::ViewMinXMargin
                    | objc2_app_kit::NSAutoresizingMaskOptions::ViewMaxXMargin
                    | objc2_app_kit::NSAutoresizingMaskOptions::ViewMinYMargin
                    | objc2_app_kit::NSAutoresizingMaskOptions::ViewMaxYMargin,
            );
            glass.content_view().addSubview(symbol_view);
            *this.ivars().glass.borrow_mut() = Some(glass);
        }
        this.apply_tint();
        this
    }

    /// `isOn`: lit the way the task ring lights when its panel is open.
    pub fn is_on(&self) -> bool {
        self.ivars().is_on.get()
    }

    /// `isOn { didSet }`.
    pub fn set_is_on(&self, value: bool) {
        let old_value = self.ivars().is_on.replace(value);
        if value == old_value {
            return;
        }
        self.refresh_interaction_feedback(self.window().is_some());
        self.apply_tint();
        needs_display(self);
    }

    fn apply_tint(&self) {
        let style_sheet = self.style_sheet();
        let tint = if self.ivars().is_on.get() { style_sheet.accent.clone() } else { style_sheet.text.clone() };
        self.setContentTintColor(Some(&tint));
        self.ivars().symbol_view.setContentTintColor(Some(&tint));
        let glass = self.ivars().glass.borrow().clone();
        if let Some(glass) = glass {
            glass.set_style_sheet(style_sheet);
        }
    }

    /// `usesGlassSurfaceForTesting`.
    pub fn uses_glass_surface_for_testing(&self) -> bool {
        self.ivars().glass.borrow().is_some()
    }
}

// MARK: - ToolbarTrailingCluster

define_class!(
    /// The trailing cluster as one toolbar item: activity, Find, the task
    /// ring, the update pill, and the `···` overflow, laid out by hand on a
    /// fixed pitch.
    // SAFETY: no ivars; `new` forwards to `initWithFrame:`.
    #[unsafe(super(NSStackView, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "ToolbarTrailingCluster"]
    pub struct ToolbarTrailingCluster;

    unsafe impl NSObjectProtocol for ToolbarTrailingCluster {}
);

impl ToolbarTrailingCluster {
    /// Edge-to-edge gap between neighbours (`Metrics.spacing`).
    const SPACING: CGFloat = 8.0;

    /// `init(views:)`.
    pub fn new(views: &[&NSView], mtm: MainThreadMarker) -> Retained<ToolbarTrailingCluster> {
        let this: Retained<ToolbarTrailingCluster> =
            unsafe { msg_send![ToolbarTrailingCluster::alloc(mtm), initWithFrame: RECT_ZERO] };
        this.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        this.setAlignment(NSLayoutAttribute::CenterY);
        this.setSpacing(Self::SPACING);
        // The layout depends on this: the activity cue and the update pill
        // hide rather than resize, and a detached view takes no spacing with
        // it, so a hidden neighbour never leaves a hole in the row.
        this.setDetachesHiddenViews(true);
        for view in views {
            this.addArrangedSubview(view);
        }
        this
    }
}
