//! Port of `Panels/UpdateWindowController.swift`: the one nonmodal update
//! surface (`UpdateWindowController`), its root view (`UpdatePanelView`) and
//! the pieces it is built from (`UpdatePanelHeader`, `UpdatePanelFooter`,
//! `UpdatePanelContent`, `UpdateNotesView`, `UpdateProgressView`,
//! `UpdateStatusMessageView`, `UpdateFailureView`, `UpdatePanelButton`,
//! `ActionTarget`), plus the `UpdatePanelController` the coordinator's
//! factory seam builds ([`panel_factory`]).
//!
//! Objective-C class names equal the Swift ones, private classes included.
//! `UpdatePanelContent` and `UpdatePanelDateFormatter` are plain Swift types
//! (not `NSObject`s) and are Rust types here.
//!
//! Where the Swift reads a `Dictionary` in iteration order (the HTML entity
//! table in `textFromHTML`), Swift's order is the per-process hash order;
//! the port walks the literal's order (see `docs/KNOWN-DIFFERENCES.md`).

#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::{Rc, Weak};

use block2::RcBlock;
use objc2::rc::{Allocated, Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAppearanceCustomization, NSApplication, NSAttributedStringNSStringDrawing, NSAutoresizingMaskOptions, NSBackingStoreType, NSBorderType,
    NSButton, NSButtonType, NSCellImagePosition, NSColor, NSControl, NSControlSize, NSControlStateValueOff,
    NSControlStateValueOn, NSCursor, NSEvent, NSFocusRingType, NSFont, NSImageScaling, NSImageView,
    NSLayoutAttribute, NSLayoutConstraintOrientation, NSLayoutPriorityDefaultLow, NSLayoutPriorityRequired,
    NSLineBreakMode, NSMutableParagraphStyle, NSPasteboard, NSPasteboardTypeString, NSProgressIndicator,
    NSProgressIndicatorStyle, NSResponder, NSScrollView, NSScrollerStyle, NSStackView, NSTextAlignment,
    NSTextField, NSTextStorage, NSTextView, NSTrackingArea, NSTrackingAreaOptions, NSUserInterfaceLayoutOrientation,
    NSView, NSWindow, NSWindowController, NSWindowDelegate, NSWindowStyleMask, NSWindowTitleVisibility,
};
use objc2_core_foundation::{CGFloat, CGSize};
use objc2_foundation::{
    NSArray, NSBundle, NSByteCountFormatter, NSByteCountFormatterCountStyle, NSDate, NSDateFormatter,
    NSDateFormatterStyle, NSMatchingOptions, NSNotFound, NSNotification, NSNotificationCenter, NSOperationQueue,
    NSRange, NSRect, NSRegularExpression, NSRegularExpressionOptions, NSSize, NSString, NSStringCompareOptions,
};
use upleft_core::contracts::DirtySet;
use upleft_core::parser::MarkdownParser;
use upleft_foundation::date::Date;
use upleft_render::appkit_compat::{attributed_string, keys};
use upleft_render::core_types::CalloutKind;
use upleft_render::render_contracts::RenderMode;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;
use upleft_render::view::markdown_text_view::MarkdownTextView;

use super::appkit_support::{
    RectExt, activate, cg, downcast, label, ns_string, object, rect, role, set_label, set_role, set_value, smax,
    smin, superview, symbol_configuration, system_symbol, weight_medium, weight_regular, weight_semibold,
    wrapping_label,
};
use super::panel_chrome::PanelFont;
use crate::app::themed_window_appearance::ThemedWindowAppearance;
use crate::updater::update_coordinator::{
    UpdateCoordinator, UpdatePanelController, UpdatePanelFactory, UpdateReleaseNotesState,
};
use crate::updater::update_metadata::{UpdateFailure, UpdateMetadata};
use crate::updater::update_state_machine::{UpdatePhase, UpdateStage};

/// `UpdateWindowLayout`.
struct UpdateWindowLayout;

impl UpdateWindowLayout {
    const WIDTH: CGFloat = 540.0;
    const REGULAR_HEIGHT: CGFloat = 480.0;
    const COMPACT_HEIGHT: CGFloat = 340.0;
    const MINIMUM_HEIGHT: CGFloat = 320.0;
}

/// `NSApp` (the app and every harness have one).
fn app(mtm: MainThreadMarker) -> Retained<NSApplication> {
    NSApplication::sharedApplication(mtm)
}

/// `Bundle.main.object(forInfoDictionaryKey: key) as? String`.
fn info_string(key: &str) -> Option<String> {
    NSBundle::mainBundle()
        .objectForInfoDictionaryKey(&NSString::from_str(key))
        .and_then(|value| value.downcast::<NSString>().ok())
        .map(|value| value.to_string())
}

/// `ByteCountFormatter.string(fromByteCount: Int64(bytes), countStyle:
/// .file)`. `Int64(bytes)` traps above `Int64.max`, as in Swift.
pub(crate) fn byte_count(bytes: u64) -> String {
    let bytes = i64::try_from(bytes).expect("Int64(UInt64) overflow (Swift traps here)");
    NSByteCountFormatter::stringFromByteCount_countStyle(bytes, NSByteCountFormatterCountStyle::File).to_string()
}

/// `NSStackView(views:)`.
fn stack_with_views(views: &[&NSView], mtm: MainThreadMarker) -> Retained<NSStackView> {
    let array = NSArray::from_slice(views);
    NSStackView::stackViewWithViews(&array, mtm)
}

/// Any `NSView` subclass instance as `&NSView`.
fn as_view<T: objc2::Message>(view: &T) -> &NSView {
    // SAFETY: every caller passes an `NSView` subclass instance.
    unsafe { &*(view as *const T).cast::<NSView>() }
}

// MARK: - UpdateWindowController

pub struct UpdateWindowControllerIvars {
    coordinator: RefCell<Weak<UpdateCoordinator>>,
    panel_view: RefCell<Option<Retained<UpdatePanelView>>>,
    state_observer: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
}

impl Drop for UpdateWindowControllerIvars {
    /// `deinit`.
    fn drop(&mut self) {
        if let Some(observer) = self.state_observer.get_mut().take() {
            // SAFETY: the token came from `addObserverForName:…`.
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(object(&*observer)) };
        }
    }
}

define_class!(
    /// The one nonmodal update surface. Documents stay usable while it is
    /// open; it never activates the app over another application, and
    /// closing it is always safe (any pending Sparkle choice resolves to
    /// "later").
    ///
    /// The panel is a pure projection of the coordinator's state machine: it
    /// rebuilds its content when the *kind* of state changes and patches
    /// only the progress numbers while a download advances.
    // SAFETY: `initWithWindow:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSWindowController, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpdateWindowController"]
    #[ivars = UpdateWindowControllerIvars]
    pub struct UpdateWindowController;

    unsafe impl NSObjectProtocol for UpdateWindowController {}

    unsafe impl NSWindowDelegate for UpdateWindowController {
        #[unsafe(method(windowWillClose:))]
        fn __window_will_close(&self, _notification: &NSNotification) {
            let coordinator = self.ivars().coordinator.borrow().upgrade();
            if let Some(coordinator) = coordinator {
                coordinator.user_did_dismiss_panel();
            }
        }
    }

    impl UpdateWindowController {
        /// Esc closes the update window, like every other summonable surface.
        #[unsafe(method(cancelOperation:))]
        fn __cancel_operation(&self, sender: Option<&AnyObject>) {
            if let Some(window) = self.window() {
                window.performClose(sender);
            }
        }
    }
);

impl UpdateWindowController {
    /// `convenience init(coordinator:)`. Builds the (titled) window without
    /// showing it; `showWindow` is the coordinator's call.
    pub fn new(coordinator: &Rc<UpdateCoordinator>, mtm: MainThreadMarker) -> Retained<UpdateWindowController> {
        let view = UpdatePanelView::new(coordinator, mtm);
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(0.0, 0.0, UpdateWindowLayout::WIDTH, UpdateWindowLayout::REGULAR_HEIGHT),
                NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::FullSizeContentView,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // Swift's ARC owns the window through the controller; objc2 must not
        // let `close` release it a second time.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&ns_string("Updates"));
        window.setTitlebarAppearsTransparent(true);
        // The panel has its own branded header. Leaving the native title
        // visible puts a second "Updates" label directly above it in the
        // full-content titlebar and makes the icon/title stack look collided.
        window.setTitleVisibility(NSWindowTitleVisibility::Hidden);
        window.setMovableByWindowBackground(true);
        window.setMinSize(NSSize::new(480.0, UpdateWindowLayout::MINIMUM_HEIGHT));
        window.setRestorable(false);
        window.setContentView(Some(&view));
        window.center();
        let this = Self::alloc(mtm).set_ivars(UpdateWindowControllerIvars {
            coordinator: RefCell::new(Weak::new()),
            panel_view: RefCell::new(None),
            state_observer: RefCell::new(None),
        });
        let this: Retained<UpdateWindowController> = unsafe { msg_send![super(this), initWithWindow: Some(&*window)] };
        *this.ivars().panel_view.borrow_mut() = Some(view);
        *this.ivars().coordinator.borrow_mut() = Rc::downgrade(coordinator);
        // Closing the window must resolve whatever Sparkle capability is
        // pending — otherwise the updater waits on a reply nobody can give
        // and `canCheckForUpdates` stays false until the app restarts.
        window.setDelegate(Some(ProtocolObject::from_ref(&*this)));
        let weak: ObjcWeak<UpdateWindowController> = ObjcWeak::from(&*this);
        let block = RcBlock::new(move |_note: NonNull<NSNotification>| {
            if let Some(this) = weak.load() {
                let panel_view = this.ivars().panel_view.borrow().clone();
                if let Some(panel_view) = panel_view {
                    panel_view.refresh();
                }
            }
        });
        // SAFETY: the block runs on the main queue, where the weak reference
        // is loaded.
        let observer = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(&NSString::from_str(UpdateCoordinator::STATE_DID_CHANGE)),
                Some(object(coordinator.notification_object())),
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
        };
        *this.ivars().state_observer.borrow_mut() = Some(observer);
        this
    }

    /// `panelView` (private in Swift; for scenes and tests).
    pub fn panel_view(&self) -> Option<Retained<UpdatePanelView>> {
        self.ivars().panel_view.borrow().clone()
    }
}

/// The coordinator's `panel`: the calls `showPanel()` and `closePanel()`
/// make on the window controller.
struct UpdateWindowPanel {
    controller: Retained<UpdateWindowController>,
}

impl UpdatePanelController for UpdateWindowPanel {
    fn show_window(&self) {
        // SAFETY: a plain `showWindow(nil)`.
        unsafe { self.controller.showWindow(None) };
    }

    fn make_key_and_order_front(&self) {
        if let Some(window) = self.controller.window() {
            window.makeKeyAndOrderFront(None);
        }
    }

    fn perform_close(&self) {
        if let Some(window) = self.controller.window() {
            window.performClose(None);
        }
    }
}

/// `UpdateWindowController(coordinator: self)`: the factory every
/// coordinator is born with, as Swift's `showPanel()` builds the window
/// controller directly.
pub fn panel_factory() -> UpdatePanelFactory {
    Rc::new(|coordinator: &Rc<UpdateCoordinator>| -> Rc<dyn UpdatePanelController> {
        let mtm = MainThreadMarker::new().expect("the update panel is built on the main thread");
        Rc::new(UpdateWindowPanel { controller: UpdateWindowController::new(coordinator, mtm) })
    })
}

// MARK: - Root view

pub struct UpdatePanelViewIvars {
    coordinator: RefCell<Weak<UpdateCoordinator>>,
    header: Retained<UpdatePanelHeader>,
    content_container: Retained<NSView>,
    footer: Retained<UpdatePanelFooter>,
    sheet: RefCell<Rc<StyleSheet>>,
    last_kind: Cell<Option<Kind>>,
}

define_class!(
    /// `UpdatePanelView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpdatePanelView"]
    #[ivars = UpdatePanelViewIvars]
    pub struct UpdatePanelView;

    unsafe impl NSObjectProtocol for UpdatePanelView {}

    impl UpdatePanelView {
        #[unsafe(method(viewDidMoveToWindow))]
        fn __view_did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            if let Some(window) = self.window() {
                window.apply_theme_appearance(&ThemeStore::shared().current());
            }
            self.refresh();
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.view_did_change_effective_appearance();
        }
    }
);

impl UpdatePanelView {
    /// `init(coordinator:)`.
    pub fn new(coordinator: &Rc<UpdateCoordinator>, mtm: MainThreadMarker) -> Retained<UpdatePanelView> {
        // Stored-property initialisers first, in declaration order.
        let header = UpdatePanelHeader::new(mtm);
        let content_container = NSView::new(mtm);
        let footer = UpdatePanelFooter::new(mtm);
        let sheet = Self::make_sheet(None, mtm);
        let this = Self::alloc(mtm).set_ivars(UpdatePanelViewIvars {
            coordinator: RefCell::new(Rc::downgrade(coordinator)),
            header: header.clone(),
            content_container: content_container.clone(),
            footer: footer.clone(),
            sheet: RefCell::new(sheet),
            last_kind: Cell::new(None),
        });
        let this: Retained<UpdatePanelView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };

        this.setWantsLayer(true);
        header.setTranslatesAutoresizingMaskIntoConstraints(false);
        content_container.setTranslatesAutoresizingMaskIntoConstraints(false);
        footer.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&header);
        this.addSubview(&content_container);
        this.addSubview(&footer);

        activate(&[
            header.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), 22.0),
            header.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -22.0),
            header.topAnchor().constraintEqualToAnchor_constant(&this.topAnchor(), 18.0),
            content_container.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), 22.0),
            content_container.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -22.0),
            content_container.topAnchor().constraintEqualToAnchor_constant(&header.bottomAnchor(), 14.0),
            footer.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), 22.0),
            footer.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -22.0),
            footer.topAnchor().constraintEqualToAnchor_constant(&content_container.bottomAnchor(), 14.0),
            footer.bottomAnchor().constraintEqualToAnchor_constant(&this.bottomAnchor(), -18.0),
        ]);
        this.refresh();
        let _: () = unsafe { msg_send![&*this, viewDidChangeEffectiveAppearance] };
        this
    }

    /// Pass the view's own appearance once it is in a window: the panel
    /// pins itself to the theme, so `NSApp`'s answer is only right before
    /// that lands.
    fn make_sheet(appearance: Option<Retained<objc2_app_kit::NSAppearance>>, mtm: MainThreadMarker) -> Rc<StyleSheet> {
        let appearance = appearance.unwrap_or_else(|| app(mtm).effectiveAppearance());
        Rc::new(StyleSheet::new(ThemeStore::shared().current(), &appearance, None))
    }

    fn view_did_change_effective_appearance(&self) {
        let ivars = self.ivars();
        if let Some(window) = self.window() {
            window.apply_theme_appearance(&ThemeStore::shared().current());
        }
        let sheet = Self::make_sheet(Some(self.effectiveAppearance()), self.mtm());
        *ivars.sheet.borrow_mut() = sheet.clone();
        if let Some(window) = self.window() {
            window.setBackgroundColor(Some(&sheet.background));
        }
        if let Some(layer) = self.layer() {
            layer.setBackgroundColor(Some(&cg(&sheet.background)));
        }
        ivars.header.apply(&sheet);
        ivars.footer.apply(&sheet);
        // Content views capture their colors when they are created. Rebuild
        // them after the panel enters a window or the system appearance
        // changes, otherwise a dark panel can retain light-theme body text.
        if self.window().is_some() {
            self.refresh();
        }
    }

    pub fn refresh(&self) {
        let ivars = self.ivars();
        let Some(coordinator) = ivars.coordinator.borrow().upgrade() else { return };
        let sheet = ivars.sheet.borrow().clone();
        ivars.header.apply(&sheet);
        ivars.header.update(&coordinator);

        let content = UpdatePanelContent::new(&coordinator);
        // Progress keeps the same subview and only patches numbers.
        if Some(content.kind) == ivars.last_kind.get()
            && content.kind == Kind::Downloading
            && let Some(first) = ivars.content_container.subviews().firstObject()
            && let Some(existing) = downcast::<UpdateProgressView>(object(&*first))
        {
            existing.update(content.received, content.expected);
            return;
        }
        ivars.last_kind.set(Some(content.kind));
        for subview in ivars.content_container.subviews().iter() {
            subview.removeFromSuperview();
        }
        let content_view = content.make_view(&coordinator, &sheet, self.mtm());
        content_view.setTranslatesAutoresizingMaskIntoConstraints(false);
        let container = &ivars.content_container;
        container.addSubview(&content_view);
        activate(&[
            content_view.leadingAnchor().constraintEqualToAnchor(&container.leadingAnchor()),
            content_view.trailingAnchor().constraintEqualToAnchor(&container.trailingAnchor()),
            content_view.topAnchor().constraintEqualToAnchor(&container.topAnchor()),
            content_view.bottomAnchor().constraintEqualToAnchor(&container.bottomAnchor()),
        ]);
        ivars.footer.update(&coordinator);
        self.resize_for_content(content.kind);
    }

    fn resize_for_content(&self, kind: Kind) {
        let Some(window) = self.window() else { return };
        let desired_height = match kind {
            Kind::Checking | Kind::Extracting | Kind::Waiting | Kind::Installing | Kind::UpToDate | Kind::Failed => {
                UpdateWindowLayout::COMPACT_HEIGHT
            }
            Kind::Available | Kind::Downloading | Kind::Ready | Kind::Informational => {
                UpdateWindowLayout::REGULAR_HEIGHT
            }
        };
        if !((window.frame().height() - desired_height).abs() > 0.5) {
            return;
        }
        let mut frame = window.frame();
        frame.origin.y += (frame.size.height - desired_height) / 2.0;
        frame.size.height = desired_height;
        window.setFrame_display_animate(frame, true, false);
    }

    /// The footer (for scenes and tests).
    pub fn footer(&self) -> Retained<UpdatePanelFooter> {
        self.ivars().footer.clone()
    }

    /// The content container (for scenes and tests).
    pub fn content_container(&self) -> Retained<NSView> {
        self.ivars().content_container.clone()
    }

    /// The header's title and versions line (for scenes and tests).
    pub fn header_text(&self) -> (String, String) {
        let header = self.ivars().header.ivars();
        (header.title_label.stringValue().to_string(), header.versions_label.stringValue().to_string())
    }

    /// The last content kind built (for scenes and tests).
    pub fn last_kind(&self) -> Option<Kind> {
        self.ivars().last_kind.get()
    }
}

// MARK: - Header

pub struct UpdatePanelHeaderIvars {
    icon_view: Retained<NSImageView>,
    title_label: Retained<NSTextField>,
    versions_label: Retained<NSTextField>,
}

define_class!(
    /// `UpdatePanelHeader` (private in Swift).
    // SAFETY: `initWithFrame:` sets the ivars before forwarding to `NSView`.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpdatePanelHeader"]
    #[ivars = UpdatePanelHeaderIvars]
    pub struct UpdatePanelHeader;

    unsafe impl NSObjectProtocol for UpdatePanelHeader {}

    impl UpdatePanelHeader {
        /// `override init(frame:)`.
        #[unsafe(method_id(initWithFrame:))]
        fn __init_with_frame(this: Allocated<Self>, frame: NSRect) -> Retained<Self> {
            let mtm = MainThreadMarker::new().expect("UpdatePanelHeader is created on the main thread");
            let this = this.set_ivars(UpdatePanelHeaderIvars {
                icon_view: NSImageView::new(mtm),
                title_label: label("", mtm),
                versions_label: label("", mtm),
            });
            let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
            this.finish_init(mtm);
            this
        }
    }
);

impl UpdatePanelHeader {
    /// `UpdatePanelHeader()`.
    fn new(mtm: MainThreadMarker) -> Retained<UpdatePanelHeader> {
        unsafe { msg_send![Self::alloc(mtm), init] }
    }

    fn finish_init(&self, mtm: MainThreadMarker) {
        let ivars = self.ivars();
        let icon_view = &ivars.icon_view;
        icon_view.setTranslatesAutoresizingMaskIntoConstraints(false);
        icon_view.setImage(app(mtm).applicationIconImage().as_deref());
        icon_view.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        icon_view.setWantsLayer(true);
        if let Some(layer) = icon_view.layer() {
            layer.setCornerRadius(10.0);
            layer.setCornerCurve(unsafe { objc2_quartz_core::kCACornerCurveContinuous });
            layer.setMasksToBounds(true);
            layer.setShadowColor(Some(&cg(&NSColor::blackColor())));
            layer.setShadowOpacity(0.20);
            layer.setShadowOffset(CGSize::new(0.0, -1.5));
            layer.setShadowRadius(4.0);
        }
        self.addSubview(icon_view);

        let title_label = &ivars.title_label;
        title_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        title_label.setFont(Some(&NSFont::systemFontOfSize_weight(15.0, weight_semibold())));
        self.addSubview(title_label);

        let versions_label = &ivars.versions_label;
        versions_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        versions_label.setFont(Some(&PanelFont::secondary()));
        self.addSubview(versions_label);

        activate(&[
            icon_view.leadingAnchor().constraintEqualToAnchor(&self.leadingAnchor()),
            icon_view.topAnchor().constraintEqualToAnchor(&self.topAnchor()),
            icon_view.widthAnchor().constraintEqualToConstant(44.0),
            icon_view.heightAnchor().constraintEqualToConstant(44.0),
            title_label.leadingAnchor().constraintEqualToAnchor_constant(&icon_view.trailingAnchor(), 14.0),
            title_label.topAnchor().constraintEqualToAnchor_constant(&self.topAnchor(), 2.0),
            versions_label.leadingAnchor().constraintEqualToAnchor(&title_label.leadingAnchor()),
            versions_label.topAnchor().constraintEqualToAnchor_constant(&title_label.bottomAnchor(), 3.0),
            versions_label.bottomAnchor().constraintLessThanOrEqualToAnchor(&self.bottomAnchor()),
        ]);
    }

    fn apply(&self, sheet: &StyleSheet) {
        self.ivars().title_label.setTextColor(Some(&sheet.text));
        self.ivars().versions_label.setTextColor(Some(&sheet.text_secondary));
    }

    fn update(&self, coordinator: &UpdateCoordinator) {
        let installed = info_string("CFBundleShortVersionString").unwrap_or_else(|| "—".into());
        let build = info_string("CFBundleVersion").unwrap_or_else(|| "—".into());

        let phase = coordinator.phase();
        let target: Option<(String, String)> = match &phase {
            UpdatePhase::Available(metadata, _) | UpdatePhase::Informational(metadata) => {
                Some((metadata.display_version_string.clone(), metadata.version_string.clone()))
            }
            UpdatePhase::Idle
            | UpdatePhase::Downloading { .. }
            | UpdatePhase::Extracting { .. }
            | UpdatePhase::ReadyToRelaunch
            | UpdatePhase::WaitingForTermination
            | UpdatePhase::Installing => coordinator
                .downloaded_update()
                .map(|update| (update.display_version_string, update.version_string)),
            _ => None,
        };

        let ivars = self.ivars();
        if let Some(target) = target {
            ivars.title_label.setStringValue(&ns_string(&format!("Update Upleft to {}", target.0)));
            ivars.versions_label.setStringValue(&ns_string(&format!(
                "Installed {installed} ({build})  →  {} ({})",
                target.0, target.1
            )));
        } else if matches!(phase, UpdatePhase::UpToDate) {
            ivars.title_label.setStringValue(&ns_string("Upleft is Up to Date"));
            ivars.versions_label.setStringValue(&ns_string(&format!("Version {installed} ({build})")));
        } else {
            ivars.title_label.setStringValue(&ns_string("Upleft Updates"));
            ivars.versions_label.setStringValue(&ns_string(&format!("Version {installed} ({build})")));
        }
    }
}

// MARK: - Footer

pub struct UpdatePanelFooterIvars {
    leading_stack: Retained<NSStackView>,
    trailing_stack: Retained<NSStackView>,
    buttons: RefCell<Vec<Retained<UpdatePanelButton>>>,
}

define_class!(
    /// `UpdatePanelFooter`.
    // SAFETY: `initWithFrame:` sets the ivars before forwarding to `NSView`.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpdatePanelFooter"]
    #[ivars = UpdatePanelFooterIvars]
    pub struct UpdatePanelFooter;

    unsafe impl NSObjectProtocol for UpdatePanelFooter {}

    impl UpdatePanelFooter {
        /// `override init(frame:)`.
        #[unsafe(method_id(initWithFrame:))]
        fn __init_with_frame(this: Allocated<Self>, frame: NSRect) -> Retained<Self> {
            let mtm = MainThreadMarker::new().expect("UpdatePanelFooter is created on the main thread");
            let this = this.set_ivars(UpdatePanelFooterIvars {
                leading_stack: NSStackView::new(mtm),
                trailing_stack: NSStackView::new(mtm),
                buttons: RefCell::new(Vec::new()),
            });
            let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
            this.finish_init();
            this
        }
    }
);

impl UpdatePanelFooter {
    /// `UpdatePanelFooter()`.
    pub fn new(mtm: MainThreadMarker) -> Retained<UpdatePanelFooter> {
        unsafe { msg_send![Self::alloc(mtm), init] }
    }

    /// `UpdatePanelFooter(frame:)`.
    pub fn with_frame(frame: NSRect, mtm: MainThreadMarker) -> Retained<UpdatePanelFooter> {
        unsafe { msg_send![Self::alloc(mtm), initWithFrame: frame] }
    }

    fn finish_init(&self) {
        let ivars = self.ivars();
        let leading = &ivars.leading_stack;
        leading.setTranslatesAutoresizingMaskIntoConstraints(false);
        leading.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        leading.setAlignment(NSLayoutAttribute::CenterY);
        leading.setSpacing(10.0);

        let trailing = &ivars.trailing_stack;
        trailing.setTranslatesAutoresizingMaskIntoConstraints(false);
        trailing.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        trailing.setAlignment(NSLayoutAttribute::CenterY);
        trailing.setSpacing(10.0);

        self.addSubview(leading);
        self.addSubview(trailing);

        activate(&[
            leading.leadingAnchor().constraintEqualToAnchor(&self.leadingAnchor()),
            leading.centerYAnchor().constraintEqualToAnchor(&self.centerYAnchor()),
            leading.topAnchor().constraintGreaterThanOrEqualToAnchor(&self.topAnchor()),
            leading.bottomAnchor().constraintLessThanOrEqualToAnchor(&self.bottomAnchor()),
            trailing.trailingAnchor().constraintEqualToAnchor(&self.trailingAnchor()),
            trailing.centerYAnchor().constraintEqualToAnchor(&self.centerYAnchor()),
            trailing.topAnchor().constraintGreaterThanOrEqualToAnchor(&self.topAnchor()),
            trailing.bottomAnchor().constraintLessThanOrEqualToAnchor(&self.bottomAnchor()),
            trailing.leadingAnchor().constraintGreaterThanOrEqualToAnchor_constant(&leading.trailingAnchor(), 16.0),
            self.heightAnchor().constraintEqualToConstant(34.0),
        ]);
    }

    pub fn apply(&self, sheet: &Rc<StyleSheet>) {
        let buttons = self.ivars().buttons.borrow().clone();
        for button in buttons {
            button.apply(sheet.clone());
        }
    }

    fn add_leading(&self, title: &str, action: impl Fn() + 'static) {
        let button = UpdatePanelButton::new(title, ButtonKind::Secondary, None, action, self.mtm());
        set_label(&*button, title);
        self.ivars().buttons.borrow_mut().push(button.clone());
        self.ivars().leading_stack.addArrangedSubview(&button);
    }

    fn add_trailing(&self, title: &str, kind: ButtonKind, action: impl Fn() + 'static) {
        let button = UpdatePanelButton::new(title, kind, None, action, self.mtm());
        set_label(&*button, title);
        self.ivars().buttons.borrow_mut().push(button.clone());
        self.ivars().trailing_stack.addArrangedSubview(&button);
    }

    pub fn update(&self, coordinator: &Rc<UpdateCoordinator>) {
        let ivars = self.ivars();
        Self::remove_buttons(&ivars.leading_stack);
        Self::remove_buttons(&ivars.trailing_stack);
        ivars.buttons.borrow_mut().clear();

        let c = coordinator.clone();
        match coordinator.phase() {
            UpdatePhase::Checking { .. } => {
                self.add_trailing("Cancel", ButtonKind::Secondary, move || c.user_did_cancel_check());
            }
            UpdatePhase::Idle if coordinator.downloaded_update().is_some() => {
                let later = c.clone();
                self.add_trailing("Later", ButtonKind::Secondary, move || later.user_did_choose_later());
                self.add_trailing("Update & Relaunch", ButtonKind::Primary, move || c.user_did_choose_install());
            }
            UpdatePhase::Available(metadata, stage) => {
                if !metadata.is_critical {
                    let skip = c.clone();
                    self.add_leading("Skip This Version", move || skip.user_did_choose_skip());
                }
                let later = c.clone();
                self.add_trailing("Later", ButtonKind::Secondary, move || later.user_did_choose_later());
                let install_title = if stage == UpdateStage::NotDownloaded { "Update" } else { "Update & Relaunch" };
                self.add_trailing(install_title, ButtonKind::Primary, move || c.user_did_choose_install());
            }
            UpdatePhase::Downloading { .. } => {
                self.add_trailing("Cancel Download", ButtonKind::Secondary, move || c.user_did_cancel_download());
            }
            UpdatePhase::Extracting { .. } => {}
            UpdatePhase::ReadyToRelaunch => {
                let later = c.clone();
                self.add_trailing("Later", ButtonKind::Secondary, move || later.user_did_choose_later());
                self.add_trailing("Update & Relaunch", ButtonKind::Primary, move || c.user_did_choose_install());
            }
            UpdatePhase::WaitingForTermination => {
                let later = c.clone();
                self.add_trailing("Later", ButtonKind::Secondary, move || later.user_did_choose_later());
                self.add_trailing("Retry Quit", ButtonKind::Primary, move || c.user_did_retry_termination());
            }
            UpdatePhase::Installing => {}
            UpdatePhase::Informational(metadata) => {
                let skip = c.clone();
                self.add_leading("Skip This Version", move || skip.user_did_choose_skip());
                let later = c.clone();
                self.add_trailing("Later", ButtonKind::Secondary, move || later.user_did_choose_later());
                self.add_trailing("Learn More", ButtonKind::Primary, move || c.user_did_request_learn_more(&metadata));
            }
            UpdatePhase::UpToDate => {
                self.add_trailing("OK", ButtonKind::Primary, move || c.user_did_choose_later());
            }
            UpdatePhase::Failed(_, retryable) => {
                let later = c.clone();
                self.add_trailing("Later", ButtonKind::Secondary, move || later.user_did_choose_later());
                if retryable {
                    self.add_trailing("Retry", ButtonKind::Primary, move || c.user_did_retry());
                }
            }
            UpdatePhase::Idle => {}
        }
    }

    /// An arranged subview belongs to exactly one stack. Asking another
    /// stack to remove it is an AppKit programming error and raises an
    /// exception; rebuild each column from its own arranged-subview list
    /// instead.
    fn remove_buttons(stack: &NSStackView) {
        for view in stack.arrangedSubviews().iter() {
            stack.removeArrangedSubview(&view);
            view.removeFromSuperview();
        }
    }

    /// The buttons in creation order (for scenes and tests).
    pub fn buttons(&self) -> Vec<Retained<UpdatePanelButton>> {
        self.ivars().buttons.borrow().clone()
    }

    /// The two columns (for scenes and tests).
    pub fn stacks(&self) -> (Retained<NSStackView>, Retained<NSStackView>) {
        (self.ivars().leading_stack.clone(), self.ivars().trailing_stack.clone())
    }
}

// MARK: - Content

/// `UpdatePanelContent.Kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Checking,
    Available,
    Downloading,
    Extracting,
    Ready,
    Waiting,
    Installing,
    Informational,
    UpToDate,
    Failed,
}

/// `UpdatePanelContent` (a private Swift class, not an `NSObject`).
pub struct UpdatePanelContent {
    pub kind: Kind,
    pub received: u64,
    pub expected: Option<u64>,
}

impl UpdatePanelContent {
    /// `init(coordinator:)`.
    pub fn new(coordinator: &UpdateCoordinator) -> UpdatePanelContent {
        let simple = |kind| UpdatePanelContent { kind, received: 0, expected: None };
        match coordinator.phase() {
            UpdatePhase::Checking { .. } => simple(Kind::Checking),
            UpdatePhase::Available(..) => simple(Kind::Available),
            // A background download that finished while the machine stayed
            // idle: the panel presents the ready-to-relaunch surface.
            UpdatePhase::Idle if coordinator.downloaded_update().is_some() => simple(Kind::Ready),
            UpdatePhase::Downloading { received, expected } => {
                UpdatePanelContent { kind: Kind::Downloading, received, expected }
            }
            UpdatePhase::Extracting { .. } => simple(Kind::Extracting),
            UpdatePhase::ReadyToRelaunch => simple(Kind::Ready),
            UpdatePhase::WaitingForTermination => simple(Kind::Waiting),
            UpdatePhase::Installing => simple(Kind::Installing),
            UpdatePhase::Informational(_) => simple(Kind::Informational),
            UpdatePhase::UpToDate => simple(Kind::UpToDate),
            UpdatePhase::Failed(..) => simple(Kind::Failed),
            UpdatePhase::Idle => simple(Kind::Checking), // unreachable; panel not shown when idle
        }
    }

    pub fn make_view(
        &self,
        coordinator: &Rc<UpdateCoordinator>,
        sheet: &Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Retained<NSView> {
        match self.kind {
            Kind::Checking => Retained::into_super(UpdateStatusMessageView::new(
                "Checking for updates…",
                Some("Connecting to the update server…"),
                true,
                None,
                None,
                sheet,
                mtm,
            )),
            Kind::Available | Kind::Informational | Kind::Ready => {
                Retained::into_super(UpdateNotesView::new(coordinator, sheet, mtm))
            }
            Kind::Downloading => {
                Retained::into_super(UpdateProgressView::new(self.received, self.expected, sheet.clone(), mtm))
            }
            Kind::Extracting => Retained::into_super(UpdateStatusMessageView::new(
                "Extracting the update…",
                Some("This usually takes a moment."),
                true,
                None,
                None,
                sheet,
                mtm,
            )),
            Kind::Waiting => Retained::into_super(UpdateStatusMessageView::new(
                "Upleft needs to quit to finish installing.",
                Some(
                    "If a document still has unsaved changes, save it and the update continues on quit. Retry Quit asks again now.",
                ),
                false,
                Some("clock.arrow.circlepath"),
                Some(sheet.accent.clone()),
                sheet,
                mtm,
            )),
            Kind::Installing => Retained::into_super(UpdateStatusMessageView::new(
                "Installing the update…",
                Some("Upleft will relaunch automatically."),
                true,
                None,
                None,
                sheet,
                mtm,
            )),
            Kind::UpToDate => {
                let detail = match coordinator.last_update_check_date() {
                    Some(date) => format!(
                        "You're on the latest version. Last check: {}",
                        UpdatePanelDateFormatter::string(date)
                    ),
                    None => "You're on the latest version.".to_owned(),
                };
                Retained::into_super(UpdateStatusMessageView::new(
                    "Upleft is up to date.",
                    Some(&detail),
                    false,
                    Some("checkmark.circle.fill"),
                    Some(sheet.accent.clone()),
                    sheet,
                    mtm,
                ))
            }
            Kind::Failed => Retained::into_super(UpdateFailureView::new(coordinator, sheet, mtm)),
        }
    }

    /// `static func diagnostics(coordinator:failure:)`.
    pub fn diagnostics(coordinator: &UpdateCoordinator, failure: &UpdateFailure) -> String {
        let version = info_string("CFBundleShortVersionString").unwrap_or_else(|| "?".into());
        let build = info_string("CFBundleVersion").unwrap_or_else(|| "?".into());
        let feed = info_string("SUFeedURL").unwrap_or_else(|| "(updates disabled)".into());
        let mut lines = vec![
            format!("Upleft {version} ({build})"),
            format!("Error: {}", failure.message),
            format!("Code: {}", failure.code),
            format!("Feed: {feed}"),
        ];
        if let Some(detail) = &failure.technical_detail {
            lines.push(format!("Detail: {detail}"));
        }
        if let Some(last_check) = coordinator.last_update_check_date() {
            lines.push(format!("Last check: {}", UpdatePanelDateFormatter::string(last_check)));
        }
        lines.join("\n")
    }
}

// MARK: - Notes view (release notes through the real renderer)

define_class!(
    /// `UpdateNotesView`.
    // SAFETY: `initWithFrame:` is inherited from `NSView`; no ivars.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpdateNotesView"]
    pub struct UpdateNotesView;

    unsafe impl NSObjectProtocol for UpdateNotesView {}
);

impl UpdateNotesView {
    /// `init(coordinator:sheet:)`.
    pub fn new(coordinator: &UpdateCoordinator, sheet: &Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<UpdateNotesView> {
        let this: Retained<UpdateNotesView> = unsafe { msg_send![Self::alloc(mtm), initWithFrame: NSRect::ZERO] };

        let notes_state = coordinator.release_notes();
        let phase = coordinator.phase();
        let metadata: Option<UpdateMetadata> = match &phase {
            UpdatePhase::Available(candidate, _) | UpdatePhase::Informational(candidate) => Some(candidate.clone()),
            _ => coordinator.downloaded_update(),
        };

        let stack = NSStackView::new(mtm);
        stack.setTranslatesAutoresizingMaskIntoConstraints(false);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        stack.setAlignment(NSLayoutAttribute::Leading);
        stack.setSpacing(10.0);
        this.addSubview(&stack);
        activate(&[
            stack.leadingAnchor().constraintEqualToAnchor(&this.leadingAnchor()),
            stack.trailingAnchor().constraintEqualToAnchor(&this.trailingAnchor()),
            stack.topAnchor().constraintEqualToAnchor(&this.topAnchor()),
            stack.bottomAnchor().constraintEqualToAnchor(&this.bottomAnchor()),
        ]);

        if let Some(metadata) = &metadata {
            let size_label = label(&Self::size_text(metadata.content_length), mtm);
            size_label.setFont(Some(&PanelFont::secondary()));
            size_label.setTextColor(Some(&sheet.text_secondary));
            stack.addArrangedSubview(&size_label);
        }

        if matches!(phase, UpdatePhase::Informational(_)) {
            let info = wrapping_label(
                "This update is informational — there is nothing to download. The details are below, or open the full announcement in your browser.",
                mtm,
            );
            info.setFont(Some(&PanelFont::row()));
            info.setTextColor(Some(&sheet.text_secondary));
            stack.addArrangedSubview(&info);
        }

        let notes_title = label("What's New", mtm);
        notes_title.setFont(Some(&PanelFont::header()));
        notes_title.setTextColor(Some(&sheet.text_secondary));
        stack.addArrangedSubview(&notes_title);

        let notes_card = NSView::new(mtm);
        notes_card.setTranslatesAutoresizingMaskIntoConstraints(false);
        notes_card.setWantsLayer(true);
        let contrast = sheet.increase_contrast;
        if let Some(layer) = notes_card.layer() {
            layer.setCornerRadius(10.0);
            layer.setCornerCurve(unsafe { objc2_quartz_core::kCACornerCurveContinuous });
            layer.setMasksToBounds(true);
            layer.setBackgroundColor(Some(&cg(&sheet.text.colorWithAlphaComponent(if contrast { 0.05 } else { 0.025 }))));
            layer.setBorderWidth(1.0);
            layer.setBorderColor(Some(&cg(&sheet.rule.colorWithAlphaComponent(if contrast { 0.70 } else { 0.45 }))));
        }
        notes_card.setContentHuggingPriority_forOrientation(NSLayoutPriorityDefaultLow, NSLayoutConstraintOrientation::Vertical);
        notes_card.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Vertical,
        );
        stack.addArrangedSubview(&notes_card);
        notes_card.widthAnchor().constraintEqualToAnchor(&stack.widthAnchor()).setActive(true);
        notes_card.heightAnchor().constraintGreaterThanOrEqualToConstant(180.0).setActive(true);

        let scroll = NSScrollView::new(mtm);
        scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
        scroll.setHasVerticalScroller(true);
        scroll.setDrawsBackground(false);
        scroll.setBorderType(NSBorderType::NoBorder);
        scroll.setAutohidesScrollers(true);
        scroll.setScrollerStyle(NSScrollerStyle::Overlay);
        notes_card.addSubview(&scroll);

        activate(&[
            scroll.leadingAnchor().constraintEqualToAnchor_constant(&notes_card.leadingAnchor(), 8.0),
            scroll.trailingAnchor().constraintEqualToAnchor_constant(&notes_card.trailingAnchor(), -8.0),
            scroll.topAnchor().constraintEqualToAnchor_constant(&notes_card.topAnchor(), 8.0),
            scroll.bottomAnchor().constraintEqualToAnchor_constant(&notes_card.bottomAnchor(), -8.0),
        ]);

        match notes_state {
            UpdateReleaseNotesState::Loaded(data) => {
                let text_view = Self::release_notes_text_view(&data, sheet, mtm);
                scroll.setDocumentView(Some(&text_view));
            }
            UpdateReleaseNotesState::Failed => {
                let failed = wrapping_label("Release notes couldn't be downloaded.", mtm);
                failed.setFont(Some(&PanelFont::row()));
                failed.setTextColor(Some(&sheet.text_faint));
                stack.addArrangedSubview(&failed);
            }
            UpdateReleaseNotesState::None => {
                let markdown = metadata.as_ref().and_then(|metadata| metadata.item_description.clone());
                if let Some(markdown) = markdown.filter(|markdown| !markdown.is_empty()) {
                    let text_view = Self::markdown_text_view(&markdown, sheet, mtm);
                    scroll.setDocumentView(Some(&text_view));
                } else if let Some(url) = metadata.as_ref().and_then(|metadata| metadata.release_notes_url.clone()) {
                    let host = url.host().unwrap_or_default();
                    let loading = wrapping_label(&format!("Loading release notes from {host}…"), mtm);
                    loading.setFont(Some(&PanelFont::row()));
                    loading.setTextColor(Some(&sheet.text_faint));
                    stack.addArrangedSubview(&loading);
                } else {
                    let empty = wrapping_label("No release notes for this update.", mtm);
                    empty.setFont(Some(&PanelFont::row()));
                    empty.setTextColor(Some(&sheet.text_faint));
                    stack.addArrangedSubview(&empty);
                }
            }
        }
        this
    }

    fn size_text(bytes: u64) -> String {
        if !(bytes > 0) {
            return String::new();
        }
        format!("Download size: {}", byte_count(bytes))
    }

    /// Release notes embedded as Markdown in the appcast description — the
    /// pipeline's normal path — rendered through Upleft's own renderer.
    pub fn markdown_text_view(markdown: &str, sheet: &Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<MarkdownTextView> {
        let storage: Retained<NSTextStorage> =
            unsafe { msg_send![NSTextStorage::alloc(), initWithString: &*ns_string(markdown)] };
        let text_view = MarkdownTextView::new(NSRect::ZERO, &storage, sheet.clone(), mtm);
        text_view.set_mode(RenderMode::Read);
        text_view.setEditable(false);
        text_view.setSelectable(true);
        text_view.setDrawsBackground(false);
        text_view.setTextContainerInset(NSSize::new(4.0, 8.0));
        // The panel's scroll view owns the width; wrap to it rather than the
        // document measure cap (which can exceed a small panel).
        if let Some(container) = unsafe { text_view.textContainer() } {
            container.setWidthTracksTextView(true);
        }
        text_view.setHorizontallyResizable(false);
        text_view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);
        text_view.setMaxSize(NSSize::new(CGFloat::MAX, CGFloat::MAX));
        text_view.update(MarkdownParser::parse(markdown), &DirtySet::wholesale(), true);
        text_view
    }

    /// Linked release notes arrive as arbitrary HTML from the update
    /// channel's host. `NSAttributedString`'s html document type runs the
    /// WebKit content engine over it — remote subresource fetches included —
    /// which is out of proportion for a small panel and outside the network
    /// surface the app documents. Reduce the page to readable text instead;
    /// Upleft's own pipeline embeds Markdown in the appcast, so this path
    /// only ever serves third-party-shaped notes.
    pub fn release_notes_text_view(data: &[u8], sheet: &Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<NSTextView> {
        let prefix = &data[..data.len().min(1_024)];
        let sample = std::str::from_utf8(prefix)
            .map(|text| upleft_swift_text::trim_whitespaces_and_newlines(text).to_owned())
            .unwrap_or_default();
        if upleft_swift_text::has_prefix(&sample, "<") {
            return Retained::into_super(Self::markdown_text_view(&Self::text_from_html(data), sheet, mtm));
        }
        // Not HTML — treat as plain text (or Markdown that Sparkle fetched raw).
        let text = std::str::from_utf8(data).unwrap_or("");
        Retained::into_super(Self::markdown_text_view(text, sheet, mtm))
    }

    /// Reduces an HTML page to plain text without a web engine. Block-level
    /// boundaries become line breaks, script/style content is dropped whole,
    /// and the handful of entities a release page actually uses are decoded.
    pub fn text_from_html(data: &[u8]) -> String {
        let mut text: Retained<NSString> = ns_string(std::str::from_utf8(data).unwrap_or(""));
        for (pattern, replacement) in [
            ("<script[^>]*>.*?</script>", ""),
            ("<style[^>]*>.*?</style>", ""),
            ("<br\\s*/?>", "\n"),
            ("</(p|div|h[1-6]|li|tr|blockquote|pre)>", "\n"),
            ("<(p|div|h[1-6]|li|tr|blockquote|pre)[^>]*>", "\n"),
            ("</?[a-zA-Z][^>]*>", ""),
        ] {
            let range = NSRange::new(0, text.length());
            text = text.stringByReplacingOccurrencesOfString_withString_options_range(
                &NSString::from_str(pattern),
                &NSString::from_str(replacement),
                NSStringCompareOptions::RegularExpressionSearch | NSStringCompareOptions::CaseInsensitiveSearch,
                range,
            );
        }
        // Swift iterates this dictionary literal in its per-process hash
        // order; the port walks the literal's order (KNOWN-DIFFERENCES).
        let entities =
            [("&amp;", "&"), ("&lt;", "<"), ("&gt;", ">"), ("&quot;", "\""), ("&apos;", "'"), ("&#39;", "'")];
        let mut swift_text = text.to_string();
        for (entity, character) in entities {
            swift_text = upleft_swift_text::replacing_occurrences(&swift_text, entity, character);
        }
        let mut text: Retained<NSString> = ns_string(&swift_text);
        if let Ok(numeric) = NSRegularExpression::regularExpressionWithPattern_options_error(
            &NSString::from_str("&#(x?)([0-9a-fA-F]+);"),
            NSRegularExpressionOptions::CaseInsensitive,
        ) {
            let matches =
                numeric.matchesInString_options_range(&text, NSMatchingOptions(0), NSRange::new(0, text.length()));
            // Replace right to left so earlier ranges stay valid.
            for index in (0..matches.count()).rev() {
                let result = matches.objectAtIndex(index);
                let marker = result.rangeAtIndex(1);
                let uses_hex = marker.location != NSNotFound as usize
                    && upleft_swift_text::str_eq(
                        &upleft_swift_text::lowercased(&text.substringWithRange(marker).to_string()),
                        "x",
                    );
                let digits = result.rangeAtIndex(2);
                if digits.location == NSNotFound as usize {
                    continue;
                }
                let digits_text = text.substringWithRange(digits).to_string();
                let Ok(value) = u32::from_str_radix(&digits_text, if uses_hex { 16 } else { 10 }) else { continue };
                let Some(scalar) = char::from_u32(value) else { continue };
                if !(value >= 0x20) {
                    continue;
                }
                text = text.stringByReplacingCharactersInRange_withString(result.range(), &ns_string(&scalar.to_string()));
            }
        }
        let collapsed = text.stringByReplacingOccurrencesOfString_withString_options_range(
            &NSString::from_str("\\n{3,}"),
            &NSString::from_str("\n\n"),
            NSStringCompareOptions::RegularExpressionSearch,
            NSRange::new(0, text.length()),
        );
        upleft_swift_text::trim_whitespaces_and_newlines(&collapsed.to_string()).to_owned()
    }
}

// MARK: - Progress

pub struct UpdateProgressViewIvars {
    bar: Retained<NSProgressIndicator>,
    detail_label: Retained<NSTextField>,
    #[allow(dead_code)]
    sheet: Rc<StyleSheet>,
}

define_class!(
    /// `UpdateProgressView` (private in Swift).
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpdateProgressView"]
    #[ivars = UpdateProgressViewIvars]
    pub struct UpdateProgressView;

    unsafe impl NSObjectProtocol for UpdateProgressView {}
);

impl UpdateProgressView {
    /// `init(received:expected:sheet:)`.
    fn new(received: u64, expected: Option<u64>, sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<UpdateProgressView> {
        let bar = NSProgressIndicator::new(mtm);
        let detail_label = label("", mtm);
        let this = Self::alloc(mtm).set_ivars(UpdateProgressViewIvars {
            bar: bar.clone(),
            detail_label: detail_label.clone(),
            sheet: sheet.clone(),
        });
        let this: Retained<UpdateProgressView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };

        let container = NSView::new(mtm);
        container.setTranslatesAutoresizingMaskIntoConstraints(false);
        container.setWantsLayer(true);
        let contrast = sheet.increase_contrast;
        if let Some(layer) = container.layer() {
            layer.setCornerRadius(12.0);
            layer.setCornerCurve(unsafe { objc2_quartz_core::kCACornerCurveContinuous });
            layer.setBackgroundColor(Some(&cg(&sheet.text.colorWithAlphaComponent(if contrast { 0.10 } else { 0.06 }))));
            layer.setBorderWidth(1.0);
            layer.setBorderColor(Some(&cg(&sheet.rule.colorWithAlphaComponent(if contrast { 0.75 } else { 0.50 }))));
        }
        this.addSubview(&container);

        let title = label("Downloading update…", mtm);
        title.setFont(Some(&NSFont::systemFontOfSize_weight(14.0, weight_semibold())));
        title.setTextColor(Some(&sheet.text));
        title.setTranslatesAutoresizingMaskIntoConstraints(false);

        bar.setTranslatesAutoresizingMaskIntoConstraints(false);
        bar.setStyle(NSProgressIndicatorStyle::Bar);
        bar.setIndeterminate(false);
        bar.setMinValue(0.0);
        bar.setMaxValue(100.0);

        detail_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        detail_label.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(12.0, weight_regular())));
        detail_label.setTextColor(Some(&sheet.text_secondary));

        let stack = stack_with_views(&[as_view(&*title), as_view(&*bar), as_view(&*detail_label)], mtm);
        stack.setTranslatesAutoresizingMaskIntoConstraints(false);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        stack.setAlignment(NSLayoutAttribute::Leading);
        stack.setSpacing(10.0);
        container.addSubview(&stack);

        activate(&[
            container.leadingAnchor().constraintEqualToAnchor(&this.leadingAnchor()),
            container.trailingAnchor().constraintEqualToAnchor(&this.trailingAnchor()),
            container.topAnchor().constraintEqualToAnchor(&this.topAnchor()),
            container.bottomAnchor().constraintEqualToAnchor(&this.bottomAnchor()),
            stack.leadingAnchor().constraintEqualToAnchor_constant(&container.leadingAnchor(), 18.0),
            stack.trailingAnchor().constraintEqualToAnchor_constant(&container.trailingAnchor(), -18.0),
            stack.centerYAnchor().constraintEqualToAnchor(&container.centerYAnchor()),
            bar.widthAnchor().constraintEqualToAnchor(&stack.widthAnchor()),
        ]);
        this.update(received, expected);
        set_role(&*this, role::progress_indicator());
        this
    }

    pub fn update(&self, received: u64, expected: Option<u64>) {
        let ivars = self.ivars();
        if let Some(expected) = expected
            && expected > 0
        {
            let fraction = smin(1.0, received as f64 / expected as f64);
            ivars.bar.setDoubleValue(fraction * 100.0);
            let formatted = byte_count(received);
            let total = byte_count(expected);
            let percent = (fraction * 100.0) as isize;
            ivars.detail_label.setStringValue(&ns_string(&format!("{formatted} of {total}  ({percent}%)")));
            set_value(self, &format!("{percent} percent downloaded"));
        } else {
            ivars.bar.setIndeterminate(true);
            unsafe { ivars.bar.startAnimation(None) };
            let formatted = byte_count(received);
            ivars.detail_label.setStringValue(&ns_string(&format!("Downloaded {formatted} so far")));
            set_value(self, &format!("Downloading, {formatted} received"));
        }
    }
}

// MARK: - Status message

define_class!(
    /// `UpdateStatusMessageView` (private in Swift).
    // SAFETY: `initWithFrame:` is inherited from `NSView`; no ivars.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpdateStatusMessageView"]
    pub struct UpdateStatusMessageView;

    unsafe impl NSObjectProtocol for UpdateStatusMessageView {}
);

impl UpdateStatusMessageView {
    /// `init(text:detail:spinner:iconName:iconColor:sheet:)`.
    fn new(
        text: &str,
        detail: Option<&str>,
        spinner: bool,
        icon_name: Option<&str>,
        icon_color: Option<Retained<NSColor>>,
        sheet: &StyleSheet,
        mtm: MainThreadMarker,
    ) -> Retained<UpdateStatusMessageView> {
        let this: Retained<UpdateStatusMessageView> =
            unsafe { msg_send![Self::alloc(mtm), initWithFrame: NSRect::ZERO] };

        let container = NSView::new(mtm);
        container.setTranslatesAutoresizingMaskIntoConstraints(false);
        container.setWantsLayer(true);
        let contrast = sheet.increase_contrast;
        if let Some(layer) = container.layer() {
            layer.setCornerRadius(12.0);
            layer.setCornerCurve(unsafe { objc2_quartz_core::kCACornerCurveContinuous });
            layer.setBackgroundColor(Some(&cg(&sheet.text.colorWithAlphaComponent(if contrast { 0.04 } else { 0.02 }))));
            layer.setBorderWidth(1.0);
            layer.setBorderColor(Some(&cg(&sheet.rule.colorWithAlphaComponent(if contrast { 0.6 } else { 0.35 }))));
        }
        this.addSubview(&container);

        let stack = NSStackView::new(mtm);
        stack.setTranslatesAutoresizingMaskIntoConstraints(false);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        stack.setAlignment(NSLayoutAttribute::CenterX);
        stack.setSpacing(8.0);
        stack.setContentHuggingPriority_forOrientation(NSLayoutPriorityRequired, NSLayoutConstraintOrientation::Vertical);
        stack.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityRequired,
            NSLayoutConstraintOrientation::Vertical,
        );
        container.addSubview(&stack);

        if let Some(icon_name) = icon_name {
            let icon_well = NSView::new(mtm);
            icon_well.setTranslatesAutoresizingMaskIntoConstraints(false);
            icon_well.setWantsLayer(true);
            let tint = icon_color.unwrap_or_else(|| sheet.accent.clone());
            if let Some(layer) = icon_well.layer() {
                layer.setCornerRadius(24.0);
                layer.setCornerCurve(unsafe { objc2_quartz_core::kCACornerCurveContinuous });
                layer.setBackgroundColor(Some(&cg(&tint.colorWithAlphaComponent(if contrast { 0.18 } else { 0.12 }))));
            }

            let icon = NSImageView::new(mtm);
            icon.setTranslatesAutoresizingMaskIntoConstraints(false);
            icon.setImage(system_symbol(icon_name, None).as_deref());
            icon.setSymbolConfiguration(Some(&symbol_configuration(22.0, weight_semibold())));
            icon.setContentTintColor(Some(&tint));
            icon_well.addSubview(&icon);

            activate(&[
                icon_well.widthAnchor().constraintEqualToConstant(48.0),
                icon_well.heightAnchor().constraintEqualToConstant(48.0),
                icon.centerXAnchor().constraintEqualToAnchor(&icon_well.centerXAnchor()),
                icon.centerYAnchor().constraintEqualToAnchor(&icon_well.centerYAnchor()),
            ]);
            stack.addArrangedSubview(&icon_well);
            stack.setCustomSpacing_afterView(12.0, &icon_well);
        } else if spinner {
            let spin_well = NSView::new(mtm);
            spin_well.setTranslatesAutoresizingMaskIntoConstraints(false);
            spin_well.setWantsLayer(true);
            if let Some(layer) = spin_well.layer() {
                layer.setCornerRadius(24.0);
                layer.setCornerCurve(unsafe { objc2_quartz_core::kCACornerCurveContinuous });
                layer.setBackgroundColor(Some(&cg(&sheet.text.colorWithAlphaComponent(if contrast { 0.08 } else { 0.04 }))));
            }

            let indicator = NSProgressIndicator::new(mtm);
            indicator.setStyle(NSProgressIndicatorStyle::Spinning);
            indicator.setControlSize(NSControlSize::Regular);
            indicator.setTranslatesAutoresizingMaskIntoConstraints(false);
            unsafe { indicator.startAnimation(None) };
            spin_well.addSubview(&indicator);

            activate(&[
                spin_well.widthAnchor().constraintEqualToConstant(48.0),
                spin_well.heightAnchor().constraintEqualToConstant(48.0),
                indicator.centerXAnchor().constraintEqualToAnchor(&spin_well.centerXAnchor()),
                indicator.centerYAnchor().constraintEqualToAnchor(&spin_well.centerYAnchor()),
            ]);
            stack.addArrangedSubview(&spin_well);
            stack.setCustomSpacing_afterView(12.0, &spin_well);
        }

        let title = wrapping_label(text, mtm);
        title.setFont(Some(&NSFont::systemFontOfSize_weight(15.0, weight_semibold())));
        title.setTextColor(Some(&sheet.text));
        title.setAlignment(NSTextAlignment::Center);
        title.setTranslatesAutoresizingMaskIntoConstraints(false);
        stack.addArrangedSubview(&title);

        if let Some(detail) = detail {
            let detail_label = wrapping_label(detail, mtm);
            detail_label.setFont(Some(&NSFont::systemFontOfSize_weight(12.5, weight_regular())));
            detail_label.setTextColor(Some(&sheet.text_secondary));
            detail_label.setAlignment(NSTextAlignment::Center);
            detail_label.setTranslatesAutoresizingMaskIntoConstraints(false);
            stack.addArrangedSubview(&detail_label);
            stack.setCustomSpacing_afterView(4.0, &title);
        }

        activate(&[
            container.leadingAnchor().constraintEqualToAnchor(&this.leadingAnchor()),
            container.trailingAnchor().constraintEqualToAnchor(&this.trailingAnchor()),
            container.topAnchor().constraintEqualToAnchor(&this.topAnchor()),
            container.bottomAnchor().constraintEqualToAnchor(&this.bottomAnchor()),
            stack.centerXAnchor().constraintEqualToAnchor(&container.centerXAnchor()),
            stack.centerYAnchor().constraintEqualToAnchor(&container.centerYAnchor()),
            stack.leadingAnchor().constraintGreaterThanOrEqualToAnchor_constant(&container.leadingAnchor(), 24.0),
            stack.trailingAnchor().constraintLessThanOrEqualToAnchor_constant(&container.trailingAnchor(), -24.0),
            container.heightAnchor().constraintGreaterThanOrEqualToConstant(132.0),
        ]);
        this
    }
}

// MARK: - Failure

pub struct UpdateFailureViewIvars {
    detail_disclosure: Retained<NSButton>,
    detail_panel: Retained<NSView>,
    #[allow(dead_code)]
    detail_label: Retained<NSTextField>,
}

define_class!(
    /// `UpdateFailureView` (private in Swift).
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpdateFailureView"]
    #[ivars = UpdateFailureViewIvars]
    pub struct UpdateFailureView;

    unsafe impl NSObjectProtocol for UpdateFailureView {}

    impl UpdateFailureView {
        #[unsafe(method(toggleDetail))]
        fn __toggle_detail(&self) {
            self.toggle_detail();
        }
    }
);

impl UpdateFailureView {
    /// `init(coordinator:sheet:)`.
    fn new(coordinator: &Rc<UpdateCoordinator>, sheet: &StyleSheet, mtm: MainThreadMarker) -> Retained<UpdateFailureView> {
        let detail_disclosure =
            unsafe { NSButton::buttonWithTitle_target_action(&ns_string("Technical Details"), None, None, mtm) };
        let detail_panel = NSView::new(mtm);
        let detail_label = label("", mtm);
        let this = Self::alloc(mtm).set_ivars(UpdateFailureViewIvars {
            detail_disclosure: detail_disclosure.clone(),
            detail_panel: detail_panel.clone(),
            detail_label: detail_label.clone(),
        });
        let this: Retained<UpdateFailureView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        let UpdatePhase::Failed(failure, _) = coordinator.phase() else { return this };

        let danger = sheet.callout_color(CalloutKind::Danger);

        let icon_well = NSView::new(mtm);
        icon_well.setTranslatesAutoresizingMaskIntoConstraints(false);
        icon_well.setWantsLayer(true);
        if let Some(layer) = icon_well.layer() {
            layer.setCornerRadius(12.0);
            layer.setCornerCurve(unsafe { objc2_quartz_core::kCACornerCurveContinuous });
            layer.setMasksToBounds(true);
            layer.setBackgroundColor(Some(&cg(
                &danger.colorWithAlphaComponent(if sheet.increase_contrast { 0.20 } else { 0.12 }),
            )));
        }

        let icon = NSImageView::new(mtm);
        icon.setTranslatesAutoresizingMaskIntoConstraints(false);
        icon.setImage(system_symbol("exclamationmark.triangle.fill", Some("Update unavailable")).as_deref());
        icon.setContentTintColor(Some(&danger));
        icon.setSymbolConfiguration(Some(&symbol_configuration(18.0, weight_semibold())));
        set_role(&*icon, role::image());
        set_label(&*icon, "Update unavailable");
        icon_well.addSubview(&icon);
        activate(&[
            icon_well.widthAnchor().constraintEqualToConstant(40.0),
            icon_well.heightAnchor().constraintEqualToConstant(40.0),
            icon.centerXAnchor().constraintEqualToAnchor(&icon_well.centerXAnchor()),
            icon.centerYAnchor().constraintEqualToAnchor(&icon_well.centerYAnchor()),
        ]);

        let status = label("Update unavailable", mtm);
        status.setFont(Some(&PanelFont::header()));
        status.setTextColor(Some(&danger));

        let summary = wrapping_label(&failure.message, mtm);
        summary.setFont(Some(&PanelFont::system(15.0, weight_semibold())));
        summary.setTextColor(Some(&sheet.text));
        summary.setMaximumNumberOfLines(2);
        summary.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );

        let reassurance = wrapping_label("Your files are safe. Nothing was changed on disk.", mtm);
        reassurance.setFont(Some(&PanelFont::row()));
        reassurance.setTextColor(Some(&sheet.text_secondary));
        reassurance.setMaximumNumberOfLines(2);
        reassurance.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );

        let message_stack = stack_with_views(&[as_view(&*status), as_view(&*summary), as_view(&*reassurance)], mtm);
        message_stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        message_stack.setAlignment(NSLayoutAttribute::Leading);
        message_stack.setSpacing(4.0);
        message_stack.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );

        let hero = stack_with_views(&[&*icon_well, as_view(&*message_stack)], mtm);
        hero.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        hero.setAlignment(NSLayoutAttribute::CenterY);
        hero.setSpacing(12.0);

        let detail_text = failure.technical_detail.clone().unwrap_or_else(|| format!("Error code {}", failure.code));
        detail_label.setStringValue(&ns_string(&detail_text));
        detail_label.setFont(Some(&PanelFont::monospaced_regular(10.5)));
        detail_label.setTextColor(Some(&sheet.text_faint));
        detail_label.setLineBreakMode(NSLineBreakMode::ByCharWrapping);
        detail_label.setMaximumNumberOfLines(3);
        detail_label.setTranslatesAutoresizingMaskIntoConstraints(false);
        detail_panel.setTranslatesAutoresizingMaskIntoConstraints(false);
        detail_panel.setWantsLayer(true);
        if let Some(layer) = detail_panel.layer() {
            layer.setCornerRadius(8.0);
            layer.setCornerCurve(unsafe { objc2_quartz_core::kCACornerCurveContinuous });
            layer.setMasksToBounds(true);
            layer.setBackgroundColor(Some(&cg(&sheet.code_background.colorWithAlphaComponent(0.72))));
            layer.setBorderWidth(1.0);
            layer.setBorderColor(Some(&cg(&sheet.rule.colorWithAlphaComponent(0.65))));
        }
        detail_panel.setHidden(true);
        detail_panel.addSubview(&detail_label);
        activate(&[
            detail_label.leadingAnchor().constraintEqualToAnchor_constant(&detail_panel.leadingAnchor(), 10.0),
            detail_label.trailingAnchor().constraintEqualToAnchor_constant(&detail_panel.trailingAnchor(), -10.0),
            detail_label.topAnchor().constraintEqualToAnchor_constant(&detail_panel.topAnchor(), 8.0),
            detail_label.bottomAnchor().constraintEqualToAnchor_constant(&detail_panel.bottomAnchor(), -8.0),
        ]);

        let stack = NSStackView::new(mtm);
        stack.setTranslatesAutoresizingMaskIntoConstraints(false);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        stack.setAlignment(NSLayoutAttribute::Leading);
        stack.setSpacing(14.0);
        this.addSubview(&stack);

        detail_disclosure.setTranslatesAutoresizingMaskIntoConstraints(false);
        detail_disclosure.setButtonType(NSButtonType::PushOnPushOff);
        detail_disclosure.setBordered(false);
        detail_disclosure.setFont(Some(&PanelFont::secondary()));
        detail_disclosure.setContentTintColor(Some(&sheet.text_secondary));
        detail_disclosure.setImage(system_symbol("chevron.right", Some("Show technical details")).as_deref());
        detail_disclosure.setImagePosition(NSCellImagePosition::ImageLeading);
        detail_disclosure.setImageHugsTitle(true);
        detail_disclosure.setAlignment(NSTextAlignment::Left);
        detail_disclosure.setToolTip(Some(&ns_string("Show technical details")));
        detail_disclosure.setWantsLayer(true);
        if let Some(layer) = detail_disclosure.layer() {
            layer.setCornerRadius(7.0);
            layer.setCornerCurve(unsafe { objc2_quartz_core::kCACornerCurveContinuous });
            layer.setMasksToBounds(true);
            layer.setBackgroundColor(Some(&cg(
                &sheet.text.colorWithAlphaComponent(if sheet.increase_contrast { 0.12 } else { 0.07 }),
            )));
            layer.setBorderWidth(1.0);
            layer.setBorderColor(Some(&cg(&sheet.text.colorWithAlphaComponent(0.14))));
        }
        // `detailDisclosure.font as Any`: an NSButton always has a font.
        let disclosure_font = detail_disclosure.font().unwrap_or_else(PanelFont::secondary);
        let attributed = attributed_string(
            &detail_disclosure.title().to_string(),
            &[(keys::font(), object(&*disclosure_font)), (keys::foreground_color(), object(&*sheet.text_secondary))],
        );
        detail_disclosure.setAttributedTitle(&attributed);
        detail_disclosure.heightAnchor().constraintEqualToConstant(28.0).setActive(true);
        detail_disclosure
            .setContentHuggingPriority_forOrientation(NSLayoutPriorityRequired, NSLayoutConstraintOrientation::Horizontal);
        unsafe {
            detail_disclosure.setTarget(Some(object(&*this)));
            detail_disclosure.setAction(Some(sel!(toggleDetail)));
        }

        let copy_coordinator = coordinator.clone();
        let copy_failure = failure.clone();
        let copy = UpdatePanelButton::new(
            "Copy Diagnostics",
            ButtonKind::Secondary,
            None,
            move || {
                let pasteboard = NSPasteboard::generalPasteboard();
                pasteboard.clearContents();
                pasteboard.setString_forType(
                    &ns_string(&UpdatePanelContent::diagnostics(&copy_coordinator, &copy_failure)),
                    unsafe { NSPasteboardTypeString },
                );
            },
            mtm,
        );
        set_label(&*copy, "Copy diagnostics");

        let diagnostics_actions = stack_with_views(&[as_view(&*detail_disclosure), as_view(&*copy)], mtm);
        diagnostics_actions.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        diagnostics_actions.setAlignment(NSLayoutAttribute::CenterY);
        diagnostics_actions.setSpacing(8.0);

        stack.addArrangedSubview(&hero);
        stack.addArrangedSubview(&diagnostics_actions);
        stack.addArrangedSubview(&detail_panel);

        // Keep the body on one horizontal grid when the disclosure opens. A
        // hidden NSView has no fitting width, so relying on stack alignment
        // alone lets AppKit shrink the whole stack to the diagnostic label.
        for view in [as_view(&*hero), as_view(&*diagnostics_actions), &*detail_panel] {
            view.widthAnchor().constraintEqualToAnchor(&stack.widthAnchor()).setActive(true);
        }

        activate(&[
            stack.leadingAnchor().constraintEqualToAnchor(&this.leadingAnchor()),
            stack.trailingAnchor().constraintEqualToAnchor(&this.trailingAnchor()),
            stack.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()),
            stack.topAnchor().constraintGreaterThanOrEqualToAnchor(&this.topAnchor()),
            stack.bottomAnchor().constraintLessThanOrEqualToAnchor(&this.bottomAnchor()),
        ]);
        this
    }

    fn toggle_detail(&self) {
        let ivars = self.ivars();
        ivars.detail_panel.setHidden(!ivars.detail_panel.isHidden());
        let hidden = ivars.detail_panel.isHidden();
        ivars.detail_disclosure.setState(if hidden { NSControlStateValueOff } else { NSControlStateValueOn });
        ivars.detail_disclosure.setImage(
            system_symbol(
                if hidden { "chevron.right" } else { "chevron.down" },
                Some(if hidden { "Show technical details" } else { "Hide technical details" }),
            )
            .as_deref(),
        );
        ivars.detail_disclosure.setToolTip(Some(&ns_string(if hidden {
            "Show technical details"
        } else {
            "Hide technical details"
        })));
        self.setNeedsLayout(true);
        if let Some(superview) = superview(self) {
            superview.setNeedsLayout(true);
        }
    }

    /// `toggleDetail()`, as the disclosure's click sends it (for scenes and
    /// tests).
    pub fn toggle_detail_for_testing(&self) {
        let _: () = unsafe { msg_send![self, toggleDetail] };
    }
}

// MARK: - Shared bits

/// `UpdatePanelButton.Kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
}

pub struct UpdatePanelButtonIvars {
    kind: ButtonKind,
    button_title: String,
    sheet: RefCell<Rc<StyleSheet>>,
    action_target: Retained<ActionTarget>,
    is_hovered: Cell<bool>,
    is_pressed: Cell<bool>,
}

define_class!(
    /// `UpdatePanelButton`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSButton, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpdatePanelButton"]
    #[ivars = UpdatePanelButtonIvars]
    pub struct UpdatePanelButton;

    unsafe impl NSObjectProtocol for UpdatePanelButton {}

    impl UpdatePanelButton {
        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            let text_width = self.attributedTitle().size().width.ceil();
            let width = smax(76.0, text_width + 30.0);
            NSSize::new(width, 32.0)
        }

        #[unsafe(method(resetCursorRects))]
        fn __reset_cursor_rects(&self) {
            self.addCursorRect_cursor(self.bounds(), &NSCursor::pointingHandCursor());
        }

        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, event: &NSEvent) {
            if self.isEnabled() {
                self.ivars().is_pressed.set(true);
                self.update_visuals();
                let _: () = unsafe { msg_send![super(self), mouseDown: event] };
                self.ivars().is_pressed.set(false);
                self.update_visuals();
            }
        }

        #[unsafe(method(mouseEntered:))]
        fn __mouse_entered(&self, _event: &NSEvent) {
            self.ivars().is_hovered.set(true);
            self.update_visuals();
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, _event: &NSEvent) {
            self.ivars().is_hovered.set(false);
            self.update_visuals();
        }
    }
);

impl UpdatePanelButton {
    /// `init(title:kind:sheet:action:)`; Swift's `sheet` defaults to nil.
    pub fn new(
        title: &str,
        kind: ButtonKind,
        sheet: Option<Rc<StyleSheet>>,
        action: impl Fn() + 'static,
        mtm: MainThreadMarker,
    ) -> Retained<UpdatePanelButton> {
        let sheet = sheet.unwrap_or_else(|| {
            Rc::new(StyleSheet::new(ThemeStore::shared().current(), &app(mtm).effectiveAppearance(), None))
        });
        let action_target = ActionTarget::new(action, mtm);
        let this = Self::alloc(mtm).set_ivars(UpdatePanelButtonIvars {
            kind,
            button_title: title.to_owned(),
            sheet: RefCell::new(sheet.clone()),
            action_target: action_target.clone(),
            is_hovered: Cell::new(false),
            is_pressed: Cell::new(false),
        });
        let this: Retained<UpdatePanelButton> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setTitle(&ns_string(""));
        this.setButtonType(NSButtonType::MomentaryPushIn);
        this.setBordered(false);
        this.setFocusRingType(NSFocusRingType::None);
        this.setWantsLayer(true);
        if let Some(layer) = this.layer() {
            layer.setCornerRadius(7.0);
            layer.setCornerCurve(unsafe { objc2_quartz_core::kCACornerCurveContinuous });
            layer.setMasksToBounds(true);
        }
        unsafe {
            this.setTarget(Some(object(&*action_target)));
            this.setAction(Some(sel!(run:)));
        }
        set_role(&*this, role::button());
        set_label(&*this, title);

        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.heightAnchor().constraintEqualToConstant(32.0).setActive(true);
        this.widthAnchor().constraintGreaterThanOrEqualToConstant(76.0).setActive(true);

        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                NSRect::ZERO,
                NSTrackingAreaOptions::ActiveInKeyWindow
                    | NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::InVisibleRect,
                Some(object(&*this)),
                None,
            )
        };
        this.addTrackingArea(&area);

        this.apply(sheet);
        this
    }

    /// `isPrimary`.
    pub fn is_primary(&self) -> bool {
        self.ivars().kind == ButtonKind::Primary
    }

    /// `buttonTitle` (private in Swift; for scenes and tests).
    pub fn button_title(&self) -> String {
        self.ivars().button_title.clone()
    }

    pub fn apply(&self, sheet: Rc<StyleSheet>) {
        *self.ivars().sheet.borrow_mut() = sheet;
        self.update_visuals();
    }

    fn update_visuals(&self) {
        let ivars = self.ivars();
        let sheet = ivars.sheet.borrow().clone();
        let contrast = sheet.increase_contrast;
        let is_focused = self.window().is_some_and(|window| {
            window
                .firstResponder()
                .is_some_and(|responder| std::ptr::eq(Retained::as_ptr(&responder).cast::<u8>(), (self as *const Self).cast::<u8>()))
                && window.isKeyWindow()
        });

        let paragraph = NSMutableParagraphStyle::new();
        paragraph.setAlignment(NSTextAlignment::Center);

        let title_color: Retained<NSColor>;
        let layer = self.layer();
        match ivars.kind {
            ButtonKind::Primary => {
                title_color = NSColor::whiteColor();
                self.setContentTintColor(Some(&NSColor::whiteColor()));
                let base = sheet.start_window_primary_action();
                let fill = if ivars.is_pressed.get() {
                    base.blendedColorWithFraction_ofColor(0.18, &NSColor::blackColor()).unwrap_or_else(|| base.clone())
                } else if ivars.is_hovered.get() {
                    base.blendedColorWithFraction_ofColor(0.08, &NSColor::whiteColor()).unwrap_or_else(|| base.clone())
                } else {
                    base.clone()
                };
                if let Some(layer) = &layer {
                    layer.setBackgroundColor(Some(&cg(&fill)));
                    layer.setBorderWidth(if is_focused { 2.0 } else { 1.0 });
                    let border = if is_focused {
                        Some(NSColor::whiteColor())
                    } else {
                        base.blendedColorWithFraction_ofColor(0.20, &NSColor::whiteColor())
                    };
                    let border = border.map(|color| cg(&color.colorWithAlphaComponent(if is_focused { 0.9 } else { 0.4 })));
                    layer.setBorderColor(border.as_deref());
                }
            }
            ButtonKind::Secondary => {
                title_color = sheet.text.clone();
                self.setContentTintColor(Some(&sheet.text));
                let alpha: CGFloat = if ivars.is_pressed.get() {
                    0.14
                } else if ivars.is_hovered.get() {
                    0.09
                } else {
                    0.05
                };
                if let Some(layer) = &layer {
                    layer.setBackgroundColor(Some(&cg(
                        &sheet.text.colorWithAlphaComponent(if contrast { smax(alpha, 0.12) } else { alpha }),
                    )));
                    layer.setBorderWidth(if is_focused { 2.0 } else { 1.0 });
                    let border = if is_focused { &sheet.accent } else { &sheet.rule };
                    layer.setBorderColor(Some(&cg(&border.colorWithAlphaComponent(if is_focused {
                        0.9
                    } else if contrast {
                        0.60
                    } else {
                        0.35
                    }))));
                }
            }
        }

        let font = PanelFont::system(13.0, if ivars.kind == ButtonKind::Primary { weight_semibold() } else { weight_medium() });
        let attributed = attributed_string(
            &ivars.button_title,
            &[
                (keys::font(), object(&*font)),
                (keys::foreground_color(), object(&*title_color)),
                (keys::paragraph_style(), object(&*paragraph)),
            ],
        );
        self.setAttributedTitle(&attributed);
    }

    /// Runs the button's action as a click would (for tests).
    pub fn perform_action_for_testing(&self) {
        (self.ivars().action_target.ivars().block)();
    }
}

/// `UpdatePanelDateFormatter`.
struct UpdatePanelDateFormatter;

impl UpdatePanelDateFormatter {
    fn string(date: Date) -> String {
        let date = NSDate::dateWithTimeIntervalSinceReferenceDate(date.time_interval_since_reference_date);
        NSDateFormatter::localizedStringFromDate_dateStyle_timeStyle(
            &date,
            NSDateFormatterStyle::MediumStyle,
            NSDateFormatterStyle::ShortStyle,
        )
        .to_string()
    }
}

pub struct ActionTargetIvars {
    block: Box<dyn Fn()>,
}

define_class!(
    /// Tiny retained target so buttons and controls don't need a
    /// view-controller back-pointer.
    // SAFETY: `init` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "ActionTarget"]
    #[ivars = ActionTargetIvars]
    pub struct ActionTarget;

    unsafe impl NSObjectProtocol for ActionTarget {}

    impl ActionTarget {
        #[unsafe(method(run:))]
        fn __run(&self, _sender: Option<&AnyObject>) {
            (self.ivars().block)();
        }
    }
);

impl ActionTarget {
    fn new(block: impl Fn() + 'static, mtm: MainThreadMarker) -> Retained<ActionTarget> {
        let this = Self::alloc(mtm).set_ivars(ActionTargetIvars { block: Box::new(block) });
        unsafe { msg_send![super(this), init] }
    }
}
