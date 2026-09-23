//! Port of `Panels/UpdateNotesPopover.swift`: the glass panel that unfurls
//! under "Update Now" while the pointer rests on it, showing what the
//! waiting build actually changes.
//!
//! It exists because the press installs. A button that restarts the app the
//! moment it is clicked has to answer "into what?" before it is clicked, and
//! a tooltip cannot carry release notes. Hovering is the one gesture that can
//! ask the question without committing to the answer.
//!
//! Three constraints shape everything here:
//!
//! * **The pill must never move.** The panel is a separate child window
//!   hanging below the pill; the button keeps the exact frame the pointer is
//!   already aiming at.
//! * **The travel must be bridged.** The window's top edge is flush with the
//!   pill's bottom edge and the visible glass is inset below it, so a
//!   pointer moving from button to panel never crosses dead space.
//! * **It must not eat the click.** The window never becomes key, never
//!   activates the app, and hit-tests to nothing outside its own body.
//!
//! `UpdateNotesPopover` is a plain Swift class (not an `NSObject`), so it is
//! a Rust type held in an `Rc`; `UpdateNotesRootView`, `UpdateNotesSurface`,
//! `UpdateNotesContentView` and `UpdateNotesLinkButton` are Objective-C
//! classes with the Swift names. `UpdateNotesSummary` (the release-notes
//! reduction) lives here, as in Swift.

#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::{Cell, RefCell};
use std::ptr::NonNull;
use std::rc::{Rc, Weak};
use std::time::SystemTime;

use block2::RcBlock;
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAnimationContext, NSApplication, NSAutoresizingMaskOptions, NSBackingStoreType, NSBorderType, NSBox,
    NSBoxType, NSButton, NSButtonType, NSColor, NSControl, NSCursor, NSEvent, NSLayoutAttribute,
    NSLayoutConstraint, NSLayoutConstraintOrientation, NSLayoutPriorityDefaultLow, NSLayoutPriorityRequired,
    NSLineBreakMode, NSNormalWindowLevel, NSPanel, NSResponder, NSScrollView, NSScrollerStyle, NSStackView,
    NSStackViewDistribution, NSTextSelectionDataSource, NSUserInterfaceLayoutOrientation, NSView,
    NSWindow, NSWindowAnimationBehavior, NSWindowCollectionBehavior, NSWindowOrderingMode, NSWindowStyleMask,
    NSWorkspace,
};
use objc2_core_foundation::{CGFloat, CGRect};
use objc2_core_graphics::CGPath;
use objc2_foundation::{NSPoint, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize, NSTimer};
use objc2_quartz_core::CAShapeLayer;
use upleft_core::contracts::DirtySet;
use upleft_core::parser::MarkdownParser;
use upleft_render::appkit_compat::{attributed_string, keys};
use upleft_render::motion::{self, Curve, SpringScalar, SpringSurfaceView};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_swift_text::CharSet;

use super::appkit_support::{
    RectExt, activate, cg, label, ns_string, null_actions, object, rect, role, set_label, set_mask, set_role, smax,
    smin, without_actions, wrapping_label,
};
use super::chrome_glass::{ChromeGlass, RoundedCorners, Tint};
use super::panel_chrome::{PanelFont, PanelMetrics};
use super::update_window_controller::{UpdateNotesView, byte_count};
use crate::updater::update_metadata::{UpdateMetadata, Url};

/// `UpdateNotesPopover.Layout`.
pub struct Layout;

impl Layout {
    pub const WIDTH: CGFloat = PanelMetrics::DETAIL_WIDTH;
    /// The transparent strip joining the pill to the glass. Part of the
    /// window, so the pointer is still "inside" while crossing it.
    pub const BRIDGE_HEIGHT: CGFloat = 6.0;
    pub const INSET: CGFloat = 14.0;
    pub const NOTES_MINIMUM_HEIGHT: CGFloat = 54.0;
    /// Notes render at the app's own document type scale — this is Upleft
    /// reading its own release notes — so the cap is set in lines rather
    /// than pixels: about ten of them, which is an opening worth reading and
    /// still a panel rather than a window.
    pub const NOTES_MAXIMUM_HEIGHT: CGFloat = 244.0;
    /// Room inside the window for the body's shadow to fall.
    pub const SHADOW_MARGIN: CGFloat = PanelMetrics::FLOATING_SHADOW_MARGIN;
}

thread_local! {
    /// One pointer, one panel. A second presentation replaces the first
    /// rather than leaving two glass bodies on screen.
    ///
    /// Strong on purpose, and cleared in `dismiss()`. The panel has to
    /// outlive the call that built it — its own pointer tracking is what
    /// takes it down — so a weak owner here means a body on screen with
    /// nothing left alive to dismiss it.
    static CURRENT: RefCell<Option<Rc<UpdateNotesPopover>>> = const { RefCell::new(None) };
}

/// `UpdateNotesPopover`.
pub struct UpdateNotesPopover {
    weak_self: Weak<UpdateNotesPopover>,
    panel: Retained<NSPanel>,
    surface: Retained<UpdateNotesSurface>,
    anchor: ObjcWeak<NSView>,
    pointer_timer: RefCell<Option<Retained<NSTimer>>>,
    /// Body plus bridge in window coordinates — what the pointer has to stay
    /// inside, as distinct from the window, which is larger by the room the
    /// shadow needs to fall into.
    body_window_rect: NSRect,
    outside_since: Cell<Option<SystemTime>>,
    is_dismissing: Cell<bool>,
    /// The panel dismisses itself on pointer-exit, so the pill it came from
    /// has to be told rather than asked.
    on_dismiss: RefCell<Option<Rc<dyn Fn()>>>,
}

impl UpdateNotesPopover {
    /// Dwell before the panel appears. Without it, the panel flashes every
    /// time the pointer sweeps across the titlebar on its way somewhere
    /// else, and no entrance survives being triggered by accident all day.
    pub const HOVER_IN_DELAY: f64 = 0.25;
    /// Grace after the pointer leaves both bodies. Long enough to cross the
    /// bridge on a diagonal, short enough that leaving feels like leaving.
    pub const DISMISS_GRACE: f64 = 0.14;
    /// How often the pointer is tested against the live region. Cheaper and
    /// far more predictable than tracking areas spanning two windows.
    const POINTER_POLL_INTERVAL: f64 = 0.1;

    // MARK: - Presentation

    /// Builds and shows the panel under `anchor`. Returns `None` when there
    /// is nothing worth showing (no window, no screen, or no update to
    /// describe).
    pub fn present(
        anchor: &NSView,
        metadata: Option<UpdateMetadata>,
        is_ready: bool,
        sheet: Rc<StyleSheet>,
    ) -> Option<Rc<UpdateNotesPopover>> {
        let current = CURRENT.with(|current| current.borrow().clone());
        if let Some(current) = current {
            current.dismiss(false);
        }
        let window = anchor.window()?;
        if !window.isVisible() || window.screen().is_none() {
            return None;
        }
        let metadata = metadata?;
        let popover = UpdateNotesPopover::new(anchor, &window, &metadata, is_ready, sheet, anchor.mtm());
        CURRENT.with(|current| *current.borrow_mut() = Some(popover.clone()));
        popover.show();
        Some(popover)
    }

    /// `dismissCurrent(animated:)`; Swift's default is `true`.
    pub fn dismiss_current(animated: bool) {
        let current = CURRENT.with(|current| current.borrow().clone());
        if let Some(current) = current {
            current.dismiss(animated);
        }
    }

    /// `private init(anchor:window:metadata:isReady:sheet:)`. Public so the
    /// conformance scene can build the panel without `present` (whose
    /// window-on-a-screen guard an off-screen host never passes), as the
    /// Swift scene reaches the private initialiser through
    /// `@_private(sourceFile:)`.
    pub fn new(
        anchor: &NSView,
        window: &NSWindow,
        metadata: &UpdateMetadata,
        is_ready: bool,
        sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Rc<UpdateNotesPopover> {
        let content = UpdateNotesContentView::new(metadata, is_ready, sheet.clone(), mtm);
        let body_height = content.preferred_height(Layout::WIDTH);
        let surface = UpdateNotesSurface::new(&content, &sheet, mtm);

        // The window carries the bridge strip above the body and shadow room
        // around it; the body itself is the only part that paints.
        let window_size = NSSize::new(
            Layout::WIDTH + Layout::SHADOW_MARGIN * 2.0,
            body_height + Layout::BRIDGE_HEIGHT + Layout::SHADOW_MARGIN,
        );
        let anchor_frame = anchor.convertRect_toView(anchor.bounds(), None);
        let anchor_on_screen = window.convertRectToScreen(anchor_frame);
        // Trailing edges align; the window's top edge meets the pill's bottom.
        let mut origin = NSPoint::new(
            anchor_on_screen.max_x() - Layout::WIDTH - Layout::SHADOW_MARGIN,
            anchor_on_screen.min_y() - window_size.height,
        );
        if let Some(screen) = window.screen() {
            let visible = screen.visibleFrame();
            origin.x = smin(
                smax(origin.x, visible.min_x() - Layout::SHADOW_MARGIN),
                visible.max_x() - window_size.width + Layout::SHADOW_MARGIN,
            );
            origin.y = smax(origin.y, visible.min_y());
        }

        let panel: Retained<NSPanel> = unsafe {
            msg_send![
                NSPanel::alloc(mtm),
                initWithContentRect: NSRect::new(origin, window_size),
                styleMask: NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
                backing: NSBackingStoreType::Buffered,
                defer: false
            ]
        };
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        // The body draws its own; a window shadow would need re-invalidating
        // on every frame of the pour.
        panel.setHasShadow(false);
        panel.setBecomesKeyOnlyIfNeeded(true);
        panel.setHidesOnDeactivate(false);
        panel.setFloatingPanel(false);
        panel.setLevel(NSNormalWindowLevel);
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setCollectionBehavior(NSWindowCollectionBehavior::MoveToActiveSpace | NSWindowCollectionBehavior::Transient);
        panel.setAnimationBehavior(NSWindowAnimationBehavior::None);
        set_role(&*panel, role::popover());
        set_label(&*panel, &format!("Release notes for {}", metadata.display_version_string));

        let root = UpdateNotesRootView::new(Layout::SHADOW_MARGIN, mtm);
        root.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), window_size));
        surface.setFrame(rect(Layout::SHADOW_MARGIN, Layout::SHADOW_MARGIN, Layout::WIDTH, body_height));
        let surface_frame = surface.frame();
        let body_window_rect = surface_frame.union(rect(
            surface_frame.min_x(),
            surface_frame.max_y(),
            surface_frame.width(),
            Layout::BRIDGE_HEIGHT,
        ));
        root.ivars().live_region.set(body_window_rect);
        root.addSubview(&surface);
        panel.setContentView(Some(&root));

        Rc::new_cyclic(|weak_self| UpdateNotesPopover {
            weak_self: weak_self.clone(),
            panel,
            surface,
            anchor: ObjcWeak::from(anchor),
            pointer_timer: RefCell::new(None),
            body_window_rect,
            outside_since: Cell::new(None),
            is_dismissing: Cell::new(false),
            on_dismiss: RefCell::new(None),
        })
    }

    fn show(&self) {
        let Some(host) = self.anchor.load().and_then(|anchor| anchor.window()) else { return };
        unsafe { host.addChildWindow_ordered(&self.panel, NSWindowOrderingMode::Above) };
        self.panel.orderFront(None);
        self.surface.refresh_glass_after_window_attach();
        self.surface.present();
        self.start_pointer_tracking();
    }

    pub fn set_on_dismiss(&self, handler: Option<Rc<dyn Fn()>>) {
        *self.on_dismiss.borrow_mut() = handler;
    }

    // MARK: - Dismissal

    /// `dismiss(animated:)`; Swift's default is `true`.
    pub fn dismiss(&self, animated: bool) {
        if self.is_dismissing.get() {
            return;
        }
        self.is_dismissing.set(true);
        CURRENT.with(|current| {
            let is_current = current.borrow().as_ref().is_some_and(|current| std::ptr::eq(Rc::as_ptr(current), self));
            if is_current {
                *current.borrow_mut() = None;
            }
        });
        let handler = self.on_dismiss.borrow().clone();
        if let Some(handler) = handler {
            handler();
        }
        *self.on_dismiss.borrow_mut() = None;
        if let Some(timer) = self.pointer_timer.borrow_mut().take() {
            timer.invalidate();
        }
        if !animated || self.surface.reduces_motion() {
            self.close();
            return;
        }
        // Nobody watches an exit: a plain fade, faster than the arrival.
        let panel = self.panel.clone();
        let changes = RcBlock::new(move |context: NonNull<NSAnimationContext>| {
            let context = unsafe { context.as_ref() };
            context.setDuration(motion::QUICK);
            context.setTimingFunction(Some(&motion::timing(Curve::EaseOut)));
            let animator: Retained<NSPanel> = unsafe { msg_send![&*panel, animator] };
            animator.setAlphaValue(0.0);
        });
        let weak = self.weak_self.clone();
        let completion = RcBlock::new(move || {
            if let Some(this) = weak.upgrade() {
                this.close();
            }
        });
        NSAnimationContext::runAnimationGroup_completionHandler(&changes, Some(&completion));
    }

    fn close(&self) {
        if let Some(parent) = self.panel.parentWindow() {
            parent.removeChildWindow(&self.panel);
        }
        self.panel.orderOut(None);
        self.panel.setContentView(None);
    }

    /// The union the pointer must stay inside: the pill, the bridge, the body.
    fn live_screen_region(&self) -> Option<NSRect> {
        let anchor = self.anchor.load()?;
        let host = anchor.window()?;
        let anchor_on_screen = host.convertRectToScreen(anchor.convertRect_toView(anchor.bounds(), None));
        Some(anchor_on_screen.union(self.panel.convertRectToScreen(self.body_window_rect)))
    }

    fn start_pointer_tracking(&self) {
        if let Some(timer) = self.pointer_timer.borrow_mut().take() {
            timer.invalidate();
        }
        let weak = self.weak_self.clone();
        let block = RcBlock::new(move |_timer: NonNull<NSTimer>| {
            if let Some(this) = weak.upgrade() {
                this.check_pointer();
            }
        });
        // SAFETY: the timer fires on the main run loop; the block holds a
        // weak reference only.
        let timer = unsafe { NSTimer::scheduledTimerWithTimeInterval_repeats_block(Self::POINTER_POLL_INTERVAL, true, &block) };
        // The pour and any document scrolling both run in tracking modes; a
        // default-mode-only timer would stop testing the pointer mid-gesture.
        unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
        *self.pointer_timer.borrow_mut() = Some(timer);
    }

    fn check_pointer(&self) {
        let host = self.anchor.load().and_then(|anchor| anchor.window());
        let active = NSApplication::sharedApplication(self.panel.mtm()).isActive();
        if !host.is_some_and(|host| host.isVisible()) || !active {
            self.dismiss(true);
            return;
        }
        let Some(region) = self.live_screen_region() else {
            self.dismiss(true);
            return;
        };
        if region.contains_point(NSEvent::mouseLocation()) {
            self.outside_since.set(None);
            return;
        }
        let since = self.outside_since.get().unwrap_or_else(SystemTime::now);
        self.outside_since.set(Some(since));
        let elapsed = SystemTime::now().duration_since(since).map(|elapsed| elapsed.as_secs_f64()).unwrap_or(0.0);
        if elapsed >= Self::DISMISS_GRACE {
            self.dismiss(true);
        }
    }

    // MARK: - Test and scene access (private in Swift)

    pub fn panel(&self) -> Retained<NSPanel> {
        self.panel.clone()
    }

    pub fn surface(&self) -> Retained<UpdateNotesSurface> {
        self.surface.clone()
    }

    pub fn body_window_rect(&self) -> NSRect {
        self.body_window_rect
    }

    /// Whether `present` stored this popover as the current one.
    pub fn is_current(&self) -> bool {
        CURRENT.with(|current| current.borrow().as_ref().is_some_and(|current| std::ptr::eq(Rc::as_ptr(current), self)))
    }
}

impl Drop for UpdateNotesPopover {
    /// `deinit`: every release path goes through `dismiss()`, which
    /// invalidates this. Belt and braces: a repeating timer the run loop
    /// still owns would otherwise outlive the object it was polling on
    /// behalf of.
    fn drop(&mut self) {
        if let Some(timer) = self.pointer_timer.get_mut().take() {
            timer.invalidate();
        }
    }
}

// MARK: - Root view

pub struct UpdateNotesRootViewIvars {
    live_region: Cell<NSRect>,
    #[allow(dead_code)]
    live_inset: CGFloat,
}

define_class!(
    /// Passes every click outside the body straight through to the
    /// document. The window is much larger than what it paints — it carries
    /// a bridge strip and shadow room — and none of that margin may swallow
    /// a click.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpdateNotesRootView"]
    #[ivars = UpdateNotesRootViewIvars]
    pub struct UpdateNotesRootView;

    unsafe impl NSObjectProtocol for UpdateNotesRootView {}

    impl UpdateNotesRootView {
        #[unsafe(method_id(hitTest:))]
        fn __hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            self.hit_test(point)
        }
    }
);

impl UpdateNotesRootView {
    /// `init(liveInset:)`.
    fn new(live_inset: CGFloat, mtm: MainThreadMarker) -> Retained<UpdateNotesRootView> {
        let this = Self::alloc(mtm).set_ivars(UpdateNotesRootViewIvars { live_region: Cell::new(NSRect::ZERO), live_inset });
        unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] }
    }

    fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
        if !self.ivars().live_region.get().contains_point(point) {
            return None;
        }
        unsafe { msg_send![super(self), hitTest: point] }
    }

    pub fn live_region(&self) -> NSRect {
        self.ivars().live_region.get()
    }
}

// MARK: - Surface (the pour)

pub struct UpdateNotesSurfaceIvars {
    reduces_motion: bool,
    glass: Retained<ChromeGlass>,
    reveal_mask: Retained<CAShapeLayer>,
    reveal: RefCell<SpringScalar>,
    reveal_target: Cell<CGFloat>,
}

define_class!(
    /// The glass body, and the one piece of motion in this file.
    ///
    /// It arrives as a *pour*, not a fade: the first visible sliver is
    /// already real material at `Motion.floatingSurfaceSliverOpacity`, full
    /// presence lands inside the first quarter of the travel, and the rest
    /// of the arrival is the body unfurling downward under a reveal mask.
    /// Glass faded up from zero has nothing to refract and reads as a grey
    /// rectangle resolving, which is the whole reason those two constants
    /// exist.
    // SAFETY: `initWithFrame:` is forwarded to `SpringSurfaceView` in `new`
    // after the ivars are set.
    #[unsafe(super(SpringSurfaceView, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpdateNotesSurface"]
    #[ivars = UpdateNotesSurfaceIvars]
    pub struct UpdateNotesSurface;

    unsafe impl NSObjectProtocol for UpdateNotesSurface {}

    impl UpdateNotesSurface {
        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            self.ivars().glass.setFrame(self.bounds());
            self.apply_reveal();
        }

        #[unsafe(method(springTick:))]
        fn __spring_tick(&self, dt: CGFloat) -> bool {
            self.ivars().reveal.borrow_mut().advance(dt)
        }

        #[unsafe(method(springApply))]
        fn __spring_apply(&self) {
            self.apply_reveal();
        }

        #[unsafe(method(springsSettleImmediately))]
        fn __springs_settle_immediately(&self) {
            let target = self.ivars().reveal_target.get();
            self.ivars().reveal.borrow_mut().snap(target);
            self.apply_reveal();
        }
    }
);

impl UpdateNotesSurface {
    /// The height that is visible the instant the panel appears.
    const SLIVER_HEIGHT: CGFloat = 20.0;

    /// `init(content:sheet:)`.
    fn new(content: &NSView, sheet: &Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<UpdateNotesSurface> {
        let reveal_mask = CAShapeLayer::new();
        let glass = ChromeGlass::new(sheet.clone(), PanelMetrics::SURFACE_RADIUS, RoundedCorners::All, Tint::Panel, mtm);
        // A hover surface is quicker than a summoned one: the reader is
        // holding still and waiting for it, so `deliberate` reads as lag.
        let reveal = SpringScalar::new(0.0, 0.0, motion::SPRING_STANDARD, 0.06);
        let this = Self::alloc(mtm).set_ivars(UpdateNotesSurfaceIvars {
            reduces_motion: sheet.reduce_motion,
            glass: glass.clone(),
            reveal_mask: reveal_mask.clone(),
            reveal: RefCell::new(reveal),
            reveal_target: Cell::new(0.0),
        });
        let this: Retained<UpdateNotesSurface> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };

        this.setWantsLayer(true);
        reveal_mask.setFillColor(Some(&cg(&NSColor::blackColor())));
        null_actions(&reveal_mask, &["path", "frame"]);
        if let Some(layer) = this.layer() {
            set_mask(&layer, Some(&reveal_mask));
        }

        glass.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
        this.addSubview(&glass);
        content.setTranslatesAutoresizingMaskIntoConstraints(false);
        let glass_content = glass.content_view();
        glass_content.addSubview(content);
        activate(&[
            content.leadingAnchor().constraintEqualToAnchor(&glass_content.leadingAnchor()),
            content.trailingAnchor().constraintEqualToAnchor(&glass_content.trailingAnchor()),
            content.topAnchor().constraintEqualToAnchor(&glass_content.topAnchor()),
            content.bottomAnchor().constraintEqualToAnchor(&glass_content.bottomAnchor()),
        ]);

        set_role(&*this, role::group());
        this
    }

    pub fn reduces_motion(&self) -> bool {
        self.ivars().reduces_motion
    }

    pub fn refresh_glass_after_window_attach(&self) {
        let glass = &self.ivars().glass;
        glass.setFrame(self.bounds());
        glass.layoutSubtreeIfNeeded();
        self.apply_reveal();
    }

    pub fn present(&self) {
        let ivars = self.ivars();
        ivars.reveal_target.set(1.0);
        if ivars.reduces_motion {
            ivars.reveal.borrow_mut().snap(1.0);
            self.apply_reveal();
            return;
        }
        ivars.reveal.borrow_mut().snap(0.0);
        self.apply_reveal();
        // `snap` sets the target as well as the value, so the pour has to be
        // retargeted after it or the spring is born already settled.
        ivars.reveal.borrow_mut().target(1.0);
        self.arm_springs();
    }

    fn apply_reveal(&self) {
        let bounds = self.bounds();
        let full = bounds.height();
        if !(full > 1.0 && bounds.width() > 1.0) {
            return;
        }
        let sliver = smin(Self::SLIVER_HEIGHT, full);
        let fraction = smin(smax(self.ivars().reveal.borrow().value(), 0.0), 1.0);
        let height = sliver + (full - sliver) * fraction;
        // The body unfurls downward, so the revealed rect hangs from the top.
        let reveal_rect: CGRect = rect(0.0, full - height, bounds.width(), height);
        let radius = smin(PanelMetrics::SURFACE_RADIUS, height / 2.0);
        without_actions(|| {
            let path = unsafe { CGPath::with_rounded_rect(reveal_rect, radius, radius, std::ptr::null()) };
            self.ivars().reveal_mask.setPath(Some(&path));
        });

        let presence = smin(1.0, fraction / smax(motion::FLOATING_SURFACE_PRESENCE_FRACTION, 0.001));
        self.setAlphaValue(
            motion::FLOATING_SURFACE_SLIVER_OPACITY + (1.0 - motion::FLOATING_SURFACE_SLIVER_OPACITY) * presence,
        );
    }

    /// The reveal spring's value (for scenes and tests).
    pub fn reveal_value(&self) -> CGFloat {
        self.ivars().reveal.borrow().value()
    }

    pub fn glass(&self) -> Retained<ChromeGlass> {
        self.ivars().glass.clone()
    }
}

// MARK: - Content

pub struct UpdateNotesContentViewIvars {
    #[allow(dead_code)]
    stack: Retained<NSStackView>,
    notes_scroll: Retained<NSScrollView>,
    metadata: UpdateMetadata,
    sheet: Rc<StyleSheet>,
    notes_height: RefCell<Option<Retained<NSLayoutConstraint>>>,
}

define_class!(
    /// Version, what it changes, and a way out to the full notes.
    ///
    /// The notes are already in hand: the release pipeline runs
    /// `generate_appcast` with `--embed-release-notes`, so the appcast
    /// `<description>` carries the Markdown and `UpdateMetadata.itemDescription`
    /// has it the moment the update is found. This panel costs no network
    /// request of its own.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpdateNotesContentView"]
    #[ivars = UpdateNotesContentViewIvars]
    pub struct UpdateNotesContentView;

    unsafe impl NSObjectProtocol for UpdateNotesContentView {}
);

impl UpdateNotesContentView {
    /// `init(metadata:isReady:sheet:)`.
    pub fn new(
        metadata: &UpdateMetadata,
        is_ready: bool,
        sheet: Rc<StyleSheet>,
        mtm: MainThreadMarker,
    ) -> Retained<UpdateNotesContentView> {
        let stack = NSStackView::new(mtm);
        let notes_scroll = NSScrollView::new(mtm);
        let this = Self::alloc(mtm).set_ivars(UpdateNotesContentViewIvars {
            stack: stack.clone(),
            notes_scroll: notes_scroll.clone(),
            metadata: metadata.clone(),
            sheet: sheet.clone(),
            notes_height: RefCell::new(None),
        });
        let this: Retained<UpdateNotesContentView> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setTranslatesAutoresizingMaskIntoConstraints(false);

        stack.setTranslatesAutoresizingMaskIntoConstraints(false);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        stack.setAlignment(NSLayoutAttribute::Leading);
        stack.setSpacing(8.0);
        this.addSubview(&stack);
        activate(&[
            stack.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), 14.0),
            stack.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -14.0),
            stack.topAnchor().constraintEqualToAnchor_constant(&this.topAnchor(), 12.0),
            stack.bottomAnchor().constraintEqualToAnchor_constant(&this.bottomAnchor(), -12.0),
        ]);

        // Header: the version this installs into, and how far away it is.
        let header = NSStackView::new(mtm);
        header.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        header.setAlignment(NSLayoutAttribute::FirstBaseline);
        header.setSpacing(8.0);
        let title = label(&format!("Upleft {}", metadata.display_version_string), mtm);
        title.setFont(Some(&PanelFont::title()));
        title.setTextColor(Some(&sheet.text));
        let status = label(&Self::status_text(metadata, is_ready), mtm);
        status.setFont(Some(&PanelFont::secondary()));
        status.setTextColor(Some(&sheet.text_faint));
        status.setContentHuggingPriority_forOrientation(NSLayoutPriorityRequired, NSLayoutConstraintOrientation::Horizontal);
        header.setDistribution(NSStackViewDistribution::Fill);
        title.setContentHuggingPriority_forOrientation(NSLayoutPriorityDefaultLow, NSLayoutConstraintOrientation::Horizontal);
        title.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );
        title.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        header.addArrangedSubview(&title);
        header.addArrangedSubview(&status);
        stack.addArrangedSubview(&header);
        header.widthAnchor().constraintEqualToAnchor(&stack.widthAnchor()).setActive(true);

        let rule = NSBox::new(mtm);
        rule.setBoxType(NSBoxType::Separator);
        stack.addArrangedSubview(&rule);
        rule.widthAnchor().constraintEqualToAnchor(&stack.widthAnchor()).setActive(true);

        notes_scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
        notes_scroll.setHasVerticalScroller(true);
        notes_scroll.setDrawsBackground(false);
        notes_scroll.setBorderType(NSBorderType::NoBorder);
        notes_scroll.setAutohidesScrollers(true);
        notes_scroll.setScrollerStyle(NSScrollerStyle::Overlay);
        stack.addArrangedSubview(&notes_scroll);
        notes_scroll.widthAnchor().constraintEqualToAnchor(&stack.widthAnchor()).setActive(true);
        let notes_height = notes_scroll.heightAnchor().constraintEqualToConstant(Layout::NOTES_MINIMUM_HEIGHT);
        *this.ivars().notes_height.borrow_mut() = Some(notes_height.clone());
        notes_height.setActive(true);

        // The footer is the honest half of "capped": the panel shows an
        // opening, and says plainly where the rest of it lives.
        if let Some(info_url) = &metadata.info_url
            && info_url.scheme().is_some_and(|scheme| upleft_swift_text::str_eq(&scheme, "https"))
        {
            let link = UpdateNotesLinkButton::new("Full notes", info_url.clone(), &sheet, mtm);
            stack.addArrangedSubview(&link);
        }
        this
    }

    fn notes_height(&self) -> Retained<NSLayoutConstraint> {
        self.ivars().notes_height.borrow().clone().expect("set in init")
    }

    /// Lays the notes out at the real width, then reports the height the
    /// panel should take: as tall as the notes need, inside the cap.
    pub fn preferred_height(&self, width: CGFloat) -> CGFloat {
        let mtm = self.mtm();
        let ivars = self.ivars();
        let sheet = ivars.sheet.clone();
        let notes_width = width - 28.0;
        let summary = UpdateNotesSummary::summary(ivars.metadata.item_description.as_deref());
        if summary.is_empty() {
            let empty = wrapping_label("No release notes for this build.", mtm);
            empty.setFont(Some(&PanelFont::row()));
            empty.setTextColor(Some(&sheet.text_faint));
            empty.setPreferredMaxLayoutWidth(notes_width);
            ivars.notes_scroll.setDocumentView(Some(&empty));
            empty.setFrame(rect(0.0, 0.0, notes_width, empty.fittingSize().height));
            self.notes_height().setConstant(Layout::NOTES_MINIMUM_HEIGHT);
        } else {
            let cap = Layout::NOTES_MAXIMUM_HEIGHT;
            // The factory lays out at a zero width; it is re-run below at the
            // real one so every measurement reflects the shown width.
            // Drop whole lines until the notes fit, rather than letting the
            // scroll view clip the last one. A hover panel that has to be
            // scrolled to finish a sentence is worse than one that stops
            // cleanly and says where the rest is — which the footer does.
            let mut text = summary;
            let mut measured = Self::measure(&text, notes_width, &sheet, mtm);
            let mut guard_count = 0;
            while measured > cap && guard_count < 12 {
                let Some(shorter) = UpdateNotesSummary::dropping_last_line(&text) else { break };
                text = shorter;
                measured = Self::measure(&text, notes_width, &sheet, mtm);
                guard_count += 1;
            }
            // A fresh view for the one that is shown: the measuring views
            // were re-laid out repeatedly, and reusing one leaves the
            // discarded candidates' fragments drawn beneath the final text.
            let text_view = UpdateNotesView::markdown_text_view(&text, &sheet, mtm);
            text_view.setFrame(rect(0.0, 0.0, notes_width, smax(measured, 1.0)));
            text_view.update(MarkdownParser::parse(&text), &DirtySet::wholesale(), true);
            ivars.notes_scroll.setDocumentView(Some(&text_view));
            self.notes_height().setConstant(smin(smax(measured, Layout::NOTES_MINIMUM_HEIGHT), cap));
        }
        self.widthAnchor().constraintEqualToConstant(width).setActive(true);
        self.layoutSubtreeIfNeeded();
        self.fittingSize().height.ceil()
    }

    /// Lays `text` out at `width` and reports the height it needs.
    fn measure(text: &str, width: CGFloat, sheet: &Rc<StyleSheet>, mtm: MainThreadMarker) -> CGFloat {
        let text_view = UpdateNotesView::markdown_text_view(text, sheet, mtm);
        text_view.setFrame(rect(0.0, 0.0, width, 1.0));
        text_view.update(MarkdownParser::parse(text), &DirtySet::wholesale(), true);
        let Some(layout_manager) = text_view.textLayoutManager() else {
            return Layout::NOTES_MAXIMUM_HEIGHT;
        };
        layout_manager.ensureLayoutForRange(&layout_manager.documentRange());
        layout_manager.usageBoundsForTextContainer().height().ceil() + 12.0
    }

    fn status_text(metadata: &UpdateMetadata, is_ready: bool) -> String {
        if is_ready {
            return "Ready to install".to_owned();
        }
        if !(metadata.content_length > 0) {
            return String::new();
        }
        byte_count(metadata.content_length)
    }

    /// The notes scroll view and its height constraint's constant (for
    /// scenes and tests).
    pub fn notes_scroll(&self) -> Retained<NSScrollView> {
        self.ivars().notes_scroll.clone()
    }

    pub fn notes_height_constant(&self) -> CGFloat {
        self.notes_height().constant()
    }
}

/// Trims the embedded release notes to what a hover can honestly show.
///
/// The panel disappears when the pointer leaves it, so it is the wrong
/// surface for a wall of text: it stops early and lets the "Full notes" link
/// carry the rest. The leading `#` title is dropped because the panel header
/// above it already names the version it would repeat.
pub struct UpdateNotesSummary;

impl UpdateNotesSummary {
    /// `summary(from:)`.
    pub fn summary(markdown: Option<&str>) -> String {
        let Some(markdown) = markdown else { return String::new() };
        let maximum_lines = 10;
        let maximum_characters = 700;
        let mut kept: Vec<String> = Vec::new();
        let mut characters = 0;
        let mut truncated = false;
        let mut seen_content = false;
        for line in upleft_swift_text::components_separated_by_set(markdown, CharSet::Newlines) {
            let trimmed = upleft_swift_text::trim_whitespaces(&line);
            if !seen_content {
                if trimmed.is_empty() {
                    continue;
                }
                if upleft_swift_text::has_prefix(trimmed, "# ") {
                    seen_content = true;
                    continue;
                }
                seen_content = true;
            }
            let count = upleft_swift_text::count(trimmed);
            if kept.len() >= maximum_lines || characters + count > maximum_characters {
                truncated = !trimmed.is_empty();
                break;
            }
            kept.push(line);
            characters += count;
        }
        while kept.last().is_some_and(|last| upleft_swift_text::trim_whitespaces(last).is_empty()) {
            kept.pop();
        }
        if kept.is_empty() {
            return String::new();
        }
        if truncated {
            kept.push(String::new());
            kept.push("…".to_owned());
        }
        kept.join("\n")
    }

    /// Drops one content line, keeping (or adding) the trailing marker that
    /// says the notes were cut. `None` once there is nothing left to drop.
    pub fn dropping_last_line(text: &str) -> Option<String> {
        let mut lines = upleft_swift_text::components_separated_by(text, "\n");
        fn drop_trailing_noise(lines: &mut Vec<String>) {
            while let Some(last) = lines.last() {
                let trimmed = upleft_swift_text::trim_whitespaces(last);
                if !(trimmed.is_empty() || upleft_swift_text::str_eq(trimmed, "…")) {
                    return;
                }
                lines.pop();
            }
        }
        drop_trailing_noise(&mut lines);
        if !(lines.len() > 1) {
            return None;
        }
        lines.pop();
        drop_trailing_noise(&mut lines);
        if lines.is_empty() {
            return None;
        }
        Some(lines.join("\n") + "\n\n…")
    }
}

// MARK: - Footer link

pub struct UpdateNotesLinkButtonIvars {
    url: Url,
}

define_class!(
    /// A plain text link. Uses `acceptsFirstMouse` because the panel never
    /// becomes key: the first click on it must be the click that works.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set.
    #[unsafe(super(NSButton, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpdateNotesLinkButton"]
    #[ivars = UpdateNotesLinkButtonIvars]
    pub struct UpdateNotesLinkButton;

    unsafe impl NSObjectProtocol for UpdateNotesLinkButton {}

    impl UpdateNotesLinkButton {
        #[unsafe(method(acceptsFirstMouse:))]
        fn __accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(resetCursorRects))]
        fn __reset_cursor_rects(&self) {
            self.addCursorRect_cursor(self.bounds(), &NSCursor::pointingHandCursor());
        }

        #[unsafe(method(open))]
        fn __open(&self) {
            NSWorkspace::sharedWorkspace().openURL(self.ivars().url.as_nsurl());
            UpdateNotesPopover::dismiss_current(false);
        }
    }
);

impl UpdateNotesLinkButton {
    /// `init(title:url:sheet:)`.
    fn new(title: &str, url: Url, sheet: &StyleSheet, mtm: MainThreadMarker) -> Retained<UpdateNotesLinkButton> {
        let tool_tip = url.absolute_string();
        let this = Self::alloc(mtm).set_ivars(UpdateNotesLinkButtonIvars { url });
        let this: Retained<UpdateNotesLinkButton> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        this.setBordered(false);
        this.setButtonType(NSButtonType::MomentaryChange);
        let font = PanelFont::secondary();
        this.setAttributedTitle(&attributed_string(
            &format!("{title} ↗"),
            &[(keys::font(), object(&*font)), (keys::foreground_color(), object(&*sheet.link))],
        ));
        unsafe {
            this.setTarget(Some(object(&*this)));
            this.setAction(Some(sel!(open)));
        }
        set_role(&*this, role::link());
        this.setToolTip(Some(&ns_string(&tool_tip)));
        this
    }
}

#[allow(unused)]
fn _unused(_: &AnyObject) {}
