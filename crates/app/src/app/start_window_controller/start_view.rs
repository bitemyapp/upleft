//! The start window's root (`// MARK: - Root` of
//! `App/StartWindowController.swift`): `StartView`, with its drop overlay
//! (`StartDropOverlay`) and its focusable canvas (`StartCanvasView`).

use crate::panels::update_status_pill::{Presentation, UpdateStatusPill};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::{Allocated, Retained, Weak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSAppearanceCustomization, NSDragOperation, NSDraggingInfo, NSEvent, NSEventModifierFlags, NSGraphicsContext, NSLayoutAttribute,
    NSLayoutPriorityDefaultHigh, NSPasteboard, NSPasteboardTypeFileURL, NSPasteboardURLReadingFileURLsOnlyKey,
    NSResponder, NSStackView, NSStackViewDistribution, NSUserInterfaceLayoutOrientation, NSView,
    NSWindowDidBecomeKeyNotification,
};
use objc2_core_foundation::{CGAffineTransform, CGFloat};
use objc2_core_graphics::{CGContext, CGPath};
use objc2_foundation::{
    NSArray, NSDictionary, NSNotification, NSNotificationCenter, NSNumber, NSOperationQueue, NSPoint, NSRect, NSString,
    NSURL,
};
use upleft_foundation::url::FileUrl;
use upleft_render::appkit_compat::{RECT_ZERO, RectExt, main_async, main_after};
use upleft_render::motion::{self, Curve};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;

use super::hero::{StartActionButton, StartHeroView};
use super::recents::{IDENTITY, RecentDocumentButton, RecentDocumentsPanel};
use super::{StartGuideOffer, StartLayout, StartTheme, StartWindowController};
use crate::ai::document_state_store::RecentDocument;
use crate::app::document_types;
use crate::app::themed_window_appearance::ThemedWindowAppearance;
use crate::panels::appkit_support::{activate, cg, role, set_label, set_role};

type ObserverToken = Retained<ProtocolObject<dyn objc2_foundation::NSObjectProtocol>>;

fn remove_observer(token: &ObserverToken) {
    // SAFETY: the token came from the default centre.
    unsafe { NSNotificationCenter::defaultCenter().removeObserver(&*(Retained::as_ptr(token) as *const AnyObject)) };
}

/// `window.firstResponder` as the row it is, if it is one.
fn first_responder_row(view: &NSView) -> Option<Retained<RecentDocumentButton>> {
    view.window()?.firstResponder()?.downcast::<RecentDocumentButton>().ok()
}

// MARK: - StartView

pub(super) struct StartViewIvars {
    owner: Weak<StartWindowController>,
    recent_panel: Retained<RecentDocumentsPanel>,
    hero: Retained<StartHeroView>,
    drop_overlay: Retained<StartDropOverlay>,
    content_stack: Retained<NSStackView>,
    /// The start surface has enough context to make an icon-only warning
    /// discoverable through its tooltip and accessibility label. Keep the
    /// titlebar warning from becoming a second hero CTA.
    update_pill: Retained<UpdateStatusPill>,
    sheet: RefCell<Rc<StyleSheet>>,
    did_play_entrance: Cell<bool>,
    key_observer: RefCell<Option<ObserverToken>>,
}

impl Drop for StartViewIvars {
    fn drop(&mut self) {
        if let Some(token) = self.key_observer.get_mut().take() {
            remove_observer(&token);
        }
    }
}

define_class!(
    /// One task path: choose an action, then a recent file. The start
    /// window is a single left-aligned column — brand, title, actions,
    /// recents — so the eye travels once, top-down, instead of choosing
    /// between two visual centres. Colours come from the app's
    /// `StyleSheet` so the welcome surface agrees with the editor it hands
    /// off to.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set;
    // the overrides keep AppKit's signatures.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "StartView"]
    #[ivars = StartViewIvars]
    pub(super) struct StartView;

    unsafe impl NSObjectProtocol for StartView {}

    impl StartView {
        // MARK: Keyboard navigation

        /// Arrow keys move focus through the recent rows; Space opens the
        /// focused row natively, Return is handled here so keyboard-only
        /// users get both keys. One focus model, no separate selection
        /// state: the row under the first responder draws the accent ring.
        /// ⌘1…⌘9 open the nth recent.
        ///
        /// Handled here rather than as menu items: these are only meaningful
        /// while this window is up, and a global menu binding would collide
        /// the moment a document window took over. `performKeyEquivalent`
        /// sees the chord before the responder chain turns it into a beep.
        #[unsafe(method(performKeyEquivalent:))]
        fn __perform_key_equivalent(&self, event: &NSEvent) -> bool {
            self.perform_key_equivalent(event)
        }

        #[unsafe(method(keyDown:))]
        fn __key_down(&self, event: &NSEvent) {
            // Key codes rather than `specialKey`: the latter is resolved
            // through the active keyboard layout, so synthetic events and
            // unusual layouts can report nil for arrow keys. Key codes are
            // stable.
            match event.keyCode() {
                125 => {
                    // ↓
                    self.move_recent_focus(1);
                    return;
                }
                126 => {
                    // ↑
                    self.move_recent_focus(-1);
                    return;
                }
                36 | 76 => {
                    // ⏎ and keypad enter
                    if let Some(row) = first_responder_row(self) {
                        // SAFETY: a nil sender.
                        unsafe { row.performClick(None) };
                        return;
                    }
                }
                _ => {}
            }
            let _: () = unsafe { msg_send![super(self), keyDown: event] };
        }

        /// Escape returns focus to the primary action, unwinding the recents
        /// selection without opening anything.
        #[unsafe(method(cancelOperation:))]
        fn __cancel_operation(&self, sender: Option<&AnyObject>) {
            if first_responder_row(self).is_some() {
                if let Some(window) = self.window() {
                    window.makeFirstResponder(Some(&self.ivars().hero.lead_button()));
                }
            } else {
                let _: () = unsafe { msg_send![super(self), cancelOperation: sender] };
            }
        }

        // MARK: Entrance

        /// A short fade-up for first layout (DESIGN §Motion): the column
        /// lands as one unit, then the recent rows settle in beneath it.
        /// Input stays live throughout — the first responder is set before
        /// the animation runs.
        #[unsafe(method(viewDidMoveToWindow))]
        fn __view_did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            self.view_did_move_to_window();
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.view_did_change_effective_appearance();
        }

        // MARK: Drag & drop

        #[unsafe(method(draggingEntered:))]
        fn __dragging_entered(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            self.dragging_entered(sender)
        }

        #[unsafe(method(draggingUpdated:))]
        fn __dragging_updated(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
            self.dragging_entered(sender)
        }

        #[unsafe(method(draggingExited:))]
        fn __dragging_exited(&self, _sender: Option<&ProtocolObject<dyn NSDraggingInfo>>) {
            self.ivars().drop_overlay.set_active(false);
        }

        #[unsafe(method(draggingEnded:))]
        fn __dragging_ended(&self, _sender: &ProtocolObject<dyn NSDraggingInfo>) {
            self.ivars().drop_overlay.set_active(false);
        }

        #[unsafe(method(performDragOperation:))]
        fn __perform_drag_operation(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
            self.perform_drag_operation(sender)
        }
    }
);

impl StartView {
    /// Clear of traffic lights; keep content below chrome
    /// (`titlebarClearance`, declared and unused in Swift).
    #[allow(dead_code)]
    const TITLEBAR_CLEARANCE: CGFloat = 30.0;

    /// `init(recents:guide:owner:)`.
    pub(super) fn new(
        recents: &[RecentDocument],
        guide: StartGuideOffer,
        owner: &StartWindowController,
        mtm: MainThreadMarker,
    ) -> Retained<StartView> {
        let drop_overlay = StartDropOverlay::new(mtm);
        let content_stack = NSStackView::new(mtm);
        let update_pill = UpdateStatusPill::new(Presentation::CompactWarning, mtm);
        let sheet = Rc::new(StartTheme::make_sheet(None, mtm));
        let recent_panel = RecentDocumentsPanel::new(recents, owner, sheet.clone(), mtm);
        let hero = StartHeroView::new(owner, guide, !recents.is_empty(), sheet.clone(), mtm);
        let this = Self::alloc(mtm).set_ivars(StartViewIvars {
            owner: Weak::from(owner),
            recent_panel,
            hero,
            drop_overlay,
            content_stack,
            update_pill,
            sheet: RefCell::new(sheet),
            did_play_entrance: Cell::new(false),
            key_observer: RefCell::new(None),
        });
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        let ivars = this.ivars();

        this.setWantsLayer(true);
        // SAFETY: AppKit's pasteboard type constant.
        this.registerForDraggedTypes(&NSArray::from_slice(&[unsafe { NSPasteboardTypeFileURL }]));
        set_role(&*this, role::group());
        set_label(&*this, "Upleft start window");

        let canvas = StartCanvasView::new(mtm);
        canvas.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&canvas);

        let content_stack = &ivars.content_stack;
        content_stack.setTranslatesAutoresizingMaskIntoConstraints(false);
        content_stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        content_stack.setAlignment(NSLayoutAttribute::Width);
        content_stack.setDistribution(NSStackViewDistribution::Fill);
        content_stack.setSpacing(StartLayout::SECTION_SPACING);
        canvas.addSubview(content_stack);

        ivars.hero.setTranslatesAutoresizingMaskIntoConstraints(false);
        ivars.recent_panel.setTranslatesAutoresizingMaskIntoConstraints(false);
        content_stack.addArrangedSubview(&ivars.hero);
        content_stack.addArrangedSubview(&ivars.recent_panel);

        let center_x = content_stack.centerXAnchor().constraintEqualToAnchor(&canvas.centerXAnchor());
        center_x.setPriority(NSLayoutPriorityDefaultHigh);

        activate(&[
            canvas.leadingAnchor().constraintEqualToAnchor(&this.leadingAnchor()),
            canvas.trailingAnchor().constraintEqualToAnchor(&this.trailingAnchor()),
            canvas.topAnchor().constraintEqualToAnchor(&this.topAnchor()),
            canvas.bottomAnchor().constraintEqualToAnchor(&this.bottomAnchor()),
            content_stack
                .leadingAnchor()
                .constraintGreaterThanOrEqualToAnchor_constant(&canvas.leadingAnchor(), StartLayout::HORIZONTAL_INSET),
            content_stack
                .trailingAnchor()
                .constraintLessThanOrEqualToAnchor_constant(&canvas.trailingAnchor(), -StartLayout::HORIZONTAL_INSET),
            content_stack.topAnchor().constraintEqualToAnchor_constant(&canvas.topAnchor(), StartLayout::TOP_INSET),
            content_stack
                .bottomAnchor()
                .constraintLessThanOrEqualToAnchor_constant(&canvas.bottomAnchor(), -StartLayout::BOTTOM_INSET),
            center_x,
            content_stack.widthAnchor().constraintEqualToConstant(StartLayout::CONTENT_WIDTH),
            ivars.hero.widthAnchor().constraintEqualToAnchor(&content_stack.widthAnchor()),
            ivars.recent_panel.widthAnchor().constraintEqualToAnchor(&content_stack.widthAnchor()),
        ]);

        // The drop affordance is a non-interactive accent frame that appears
        // while a file is dragged anywhere over the window. There is no idle
        // helper copy: the window only spends visual attention on dropping
        // when a drop is actually possible.
        let drop_overlay = &ivars.drop_overlay;
        drop_overlay.setTranslatesAutoresizingMaskIntoConstraints(false);
        drop_overlay.setHidden(true);
        this.addSubview(drop_overlay);
        activate(&[
            drop_overlay.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), 10.0),
            drop_overlay.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -10.0),
            drop_overlay.topAnchor().constraintEqualToAnchor_constant(&this.topAnchor(), 10.0),
            drop_overlay.bottomAnchor().constraintEqualToAnchor_constant(&this.bottomAnchor(), -10.0),
        ]);

        install_update_pill(&this, mtm);
        this
    }

    fn sheet(&self) -> Rc<StyleSheet> {
        self.ivars().sheet.borrow().clone()
    }

    /// `preferredFirstResponder`.
    pub(super) fn preferred_first_responder(&self) -> Retained<StartActionButton> {
        self.ivars().hero.lead_button()
    }

    /// `reloadRecents(_:)`.
    pub(super) fn reload_recents(&self, recents: &[RecentDocument]) {
        let Some(owner) = self.ivars().owner.load() else { return };
        self.ivars().recent_panel.reload(recents, &owner);
    }

    fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
        // Command must be down and nothing else the user meant; `.numericPad`
        // is *not* one of those. macOS sets it on the top-row digits, so an
        // exact `== .command` test never matches ⌘1 and the shortcut
        // silently does nothing. `.capsLock` rides along the same way.
        let modifiers = event.modifierFlags() & NSEventModifierFlags::DeviceIndependentFlagsMask;
        let digit = if modifiers.contains(NSEventModifierFlags::Command)
            && !modifiers.intersects(NSEventModifierFlags::Shift | NSEventModifierFlags::Control | NSEventModifierFlags::Option)
        {
            event
                .charactersIgnoringModifiers()
                .and_then(|characters| upleft_swift_text::parse_int(&upleft_swift_text::ns::foundation::to_string(&characters)))
                .filter(|digit| *digit >= 1)
        } else {
            None
        };
        let Some(digit) = digit else {
            return unsafe { msg_send![super(self), performKeyEquivalent: event] };
        };

        let rows = self.ivars().recent_panel.row_buttons();
        if digit as usize > rows.len() {
            return unsafe { msg_send![super(self), performKeyEquivalent: event] };
        }
        // SAFETY: a nil sender.
        unsafe { rows[digit as usize - 1].performClick(None) };
        true
    }

    /// `moveRecentFocus(_:)`.
    fn move_recent_focus(&self, delta: isize) {
        let rows = self.ivars().recent_panel.row_buttons();
        let Some(window) = self.window() else { return };
        if rows.is_empty() {
            return;
        }
        // No focus yet: Down lands on the first row, Up on the last.
        let first_responder = window.firstResponder();
        let current = rows
            .iter()
            .position(|row| {
                first_responder.as_ref().is_some_and(|responder| {
                    std::ptr::eq(Retained::as_ptr(responder).cast::<AnyObject>(), Retained::as_ptr(row).cast::<AnyObject>())
                })
            })
            .map_or(if delta > 0 { -1 } else { rows.len() as isize }, |index| index as isize);
        let next = (current + delta).max(0).min(rows.len() as isize - 1);
        window.makeFirstResponder(Some(&rows[next as usize]));
    }

    fn view_did_move_to_window(&self) {
        if let Some(window) = self.window() {
            window.apply_theme_appearance(&ThemeStore::shared().current());
        }
        if let Some(window) = self.window() {
            window.setBackgroundColor(Some(&self.sheet().background));
        }
        let previous = self.ivars().key_observer.borrow_mut().take();
        if let Some(token) = previous {
            remove_observer(&token);
        }
        if let Some(window) = self.window() {
            // Bindings are user-editable, so the hero's shortcut hints are
            // only true at the moment they are drawn. Refreshing when the
            // window comes forward covers the "rebind ⌘O, come back here"
            // path.
            let weak: Weak<StartView> = Weak::from(self);
            let block = RcBlock::new(move |_notification: std::ptr::NonNull<NSNotification>| {
                if let Some(this) = weak.load() {
                    this.ivars().hero.refresh_shortcuts();
                }
            });
            // SAFETY: the block only touches the view on the main queue it
            // is delivered on.
            let token = unsafe {
                NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                    Some(NSWindowDidBecomeKeyNotification),
                    Some(&*window as &AnyObject),
                    Some(&NSOperationQueue::mainQueue()),
                    &block,
                )
            };
            *self.ivars().key_observer.borrow_mut() = Some(token);
        }
        if !self.ivars().did_play_entrance.get() && self.window().is_some() {
            self.ivars().did_play_entrance.set(true);
            self.play_entrance();
        }
    }

    fn view_did_change_effective_appearance(&self) {
        if let Some(window) = self.window() {
            window.apply_theme_appearance(&ThemeStore::shared().current());
        }
        let sheet = Rc::new(StartTheme::make_sheet(Some(&self.effectiveAppearance()), self.mtm()));
        *self.ivars().sheet.borrow_mut() = sheet.clone();
        if let Some(window) = self.window() {
            window.setBackgroundColor(Some(&sheet.background));
        }
        self.ivars().hero.apply(sheet.clone());
        self.ivars().recent_panel.apply(sheet.clone());
        self.ivars().drop_overlay.apply(sheet);
    }

    /// `playEntrance()`.
    fn play_entrance(&self) {
        let reduce = self.sheet().reduce_motion;
        if reduce {
            return;
        }
        let content_stack = self.ivars().content_stack.clone();
        content_stack.setWantsLayer(true);
        if let Some(layer) = content_stack.layer() {
            layer.setOpacity(0.0);
            layer.setAffineTransform(CGAffineTransform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 10.0 });
        }
        motion::run(
            false,
            motion::DELIBERATE,
            Curve::Structural,
            move |_| {
                if let Some(layer) = content_stack.layer() {
                    layer.setOpacity(1.0);
                    layer.setAffineTransform(IDENTITY);
                }
            },
            None,
        );
        self.ivars().recent_panel.reveal_rows();
    }

    fn dragging_entered(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> NSDragOperation {
        let urls = dropped_urls(&sender.draggingPasteboard());
        let ok = !urls.is_empty();
        self.ivars().drop_overlay.set_active(ok);
        if ok { NSDragOperation::Copy } else { NSDragOperation::empty() }
    }

    fn perform_drag_operation(&self, sender: &ProtocolObject<dyn NSDraggingInfo>) -> bool {
        let urls = dropped_urls(&sender.draggingPasteboard());
        self.ivars().drop_overlay.set_active(false);
        if urls.is_empty() {
            return false;
        }
        if self.ivars().owner.load().is_none_or(|owner| !owner.begin_handoff()) {
            return false;
        }
        for url in urls {
            if let Some(owner) = self.ivars().owner.load() {
                owner.call_on_open(url);
            }
        }
        let weak_owner = self.ivars().owner.clone();
        main_async(move || {
            let Some(owner) = weak_owner.load() else { return };
            if owner.window().is_some_and(|window| window.isVisible()) {
                owner.set_handing_off(false);
            }
        });
        true
    }
}

/// `droppedURLs(from:)`.
fn dropped_urls(pasteboard: &NSPasteboard) -> Vec<FileUrl> {
    let classes = NSArray::from_slice(&[NSURL::class()]);
    // SAFETY: AppKit's reading-option key.
    let options: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(
        &[unsafe { NSPasteboardURLReadingFileURLsOnlyKey }],
        &[&*NSNumber::new_bool(true) as &AnyObject],
    );
    // SAFETY: `NSURL` reads from a pasteboard; the options dictionary holds
    // a boolean for a known key.
    let objects = unsafe { pasteboard.readObjectsForClasses_options(&classes, Some(&options)) };
    // `as? [URL] ?? []`: every element must be a URL.
    let urls: Vec<FileUrl> = match objects {
        Some(objects) => {
            let mut urls = Vec::new();
            for object in objects.iter() {
                let Some(url) = object.downcast::<NSURL>().ok().and_then(|url| FileUrl::from_nsurl(&url)) else {
                    return Vec::new();
                };
                urls.push(url);
            }
            urls
        }
        None => Vec::new(),
    };
    urls.into_iter()
        .filter(|url| {
            document_types::is_markdown(&url.path_extension()) && upleft_foundation::file_manager::file_exists(&url.path())
        })
        .collect()
}

/// The lines of `init(recents:guide:owner:)` that place the update pill.
fn install_update_pill(view: &StartView, _mtm: MainThreadMarker) {
    let update_pill = &view.ivars().update_pill;
    // The update pill sits in the transparent titlebar strip, clear of the
    // hero and the traffic lights, and stays collapsed to zero width until
    // the coordinator has something to say.
    update_pill.setTranslatesAutoresizingMaskIntoConstraints(false);
    view.addSubview(update_pill);
    activate(&[
        // Give the warning its own titlebar lane: the extra inset keeps the
        // pill off the window edge and visually separates it from the
        // traffic-light/titlebar chrome at small capture sizes.
        update_pill.trailingAnchor().constraintEqualToAnchor_constant(&view.trailingAnchor(), -18.0),
        update_pill.topAnchor().constraintEqualToAnchor_constant(&view.topAnchor(), 10.0),
    ]);
}

// MARK: - StartDropOverlay

pub(super) struct StartDropOverlayIvars {
    is_active: Cell<bool>,
    sheet: RefCell<Rc<StyleSheet>>,
}

define_class!(
    /// A quiet accent frame that acknowledges a drag before the drop. It is
    /// hidden at rest and becomes a dashed border only while a supported
    /// file is over the window, so the welcome surface never carries a
    /// permanent instruction line.
    // SAFETY: `initWithFrame:` sets the ivars, so every initialiser path
    // (`-init` included) creates a valid instance.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "StartDropOverlay"]
    #[ivars = StartDropOverlayIvars]
    pub(super) struct StartDropOverlay;

    unsafe impl NSObjectProtocol for StartDropOverlay {}

    impl StartDropOverlay {
        #[unsafe(method_id(initWithFrame:))]
        fn __init_with_frame(this: Allocated<Self>, frame: NSRect) -> Retained<Self> {
            let mtm = MainThreadMarker::new().expect("StartDropOverlay is created on the main thread");
            let this = this.set_ivars(StartDropOverlayIvars {
                is_active: Cell::new(false),
                sheet: RefCell::new(Rc::new(StartTheme::make_sheet(None, mtm))),
            });
            let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
            this.setWantsLayer(true);
            if let Some(layer) = this.layer() {
                layer.setCornerRadius(14.0);
                layer.setOpacity(0.0);
            }
            let sheet = this.ivars().sheet.borrow().clone();
            this.apply(sheet);
            this
        }

        #[unsafe(method_id(hitTest:))]
        fn __hit_test(&self, _point: NSPoint) -> Option<Retained<NSView>> {
            None
        }

        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, _dirty_rect: NSRect) {
            if !self.ivars().is_active.get() {
                return;
            }
            let Some(graphics) = NSGraphicsContext::currentContext() else { return };
            let cg_context = graphics.CGContext();
            let context = Some(&*cg_context);
            let sheet = self.ivars().sheet.borrow().clone();
            let rect = self.bounds().inset_by(1.5, 1.5);
            // SAFETY: a null transform is allowed.
            let path = unsafe { CGPath::with_rounded_rect(rect, 13.0, 13.0, std::ptr::null()) };
            CGContext::save_g_state(context);
            CGContext::add_path(context, Some(&path));
            CGContext::set_line_width(context, 1.5);
            let lengths: [CGFloat; 2] = [7.0, 5.0];
            // SAFETY: `lengths` holds `count` values and outlives the call.
            unsafe { CGContext::set_line_dash(context, 0.0, lengths.as_ptr(), lengths.len()) };
            CGContext::set_stroke_color_with_color(
                context,
                Some(&cg(&sheet.accent.colorWithAlphaComponent(if sheet.increase_contrast { 0.8 } else { 0.52 }))),
            );
            CGContext::stroke_path(context);
            CGContext::restore_g_state(context);
        }
    }
);

impl StartDropOverlay {
    /// `StartDropOverlay()`.
    fn new(mtm: MainThreadMarker) -> Retained<StartDropOverlay> {
        unsafe { msg_send![Self::alloc(mtm), init] }
    }

    /// `apply(sheet:)`.
    fn apply(&self, sheet: Rc<StyleSheet>) {
        *self.ivars().sheet.borrow_mut() = sheet.clone();
        let contrast = sheet.increase_contrast;
        if let Some(layer) = self.layer() {
            layer.setBackgroundColor(Some(&cg(&sheet.accent.colorWithAlphaComponent(if contrast { 0.055 } else { 0.025 }))));
        }
        self.setNeedsDisplay(true);
    }

    /// `setActive(_:)`.
    fn set_active(&self, active: bool) {
        if active == self.ivars().is_active.get() {
            return;
        }
        self.ivars().is_active.set(active);
        self.setHidden(false);
        self.setNeedsDisplay(true);
        let reduce = self.ivars().sheet.borrow().reduce_motion;
        let this = self.retain();
        let apply = move || {
            if let Some(layer) = this.layer() {
                layer.setOpacity(if active { 1.0 } else { 0.0 });
            }
            this.setNeedsDisplay(true);
        };
        if reduce {
            apply();
            if !active {
                self.setHidden(true);
            }
        } else {
            motion::run(false, motion::QUICK, Curve::EaseOut, move |_| apply(), None);
            if !active {
                let weak: Weak<StartDropOverlay> = Weak::from(self);
                main_after(motion::QUICK, move || {
                    let Some(this) = weak.load() else { return };
                    if this.ivars().is_active.get() {
                        return;
                    }
                    this.setHidden(true);
                });
            }
        }
    }
}

// MARK: - StartCanvasView

define_class!(
    /// The scroll canvas takes keyboard focus when clicked, so a blank-area
    /// click does not strand arrow-key navigation (the content view is
    /// covered by the scroll view and cannot grab focus itself).
    // SAFETY: no ivars; the overrides keep AppKit's signatures.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "StartCanvasView"]
    pub(super) struct StartCanvasView;

    unsafe impl NSObjectProtocol for StartCanvasView {}

    impl StartCanvasView {
        #[unsafe(method(acceptsFirstResponder))]
        fn __accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, event: &NSEvent) {
            if let Some(window) = self.window() {
                window.makeFirstResponder(Some(self));
            }
            let _: () = unsafe { msg_send![super(self), mouseDown: event] };
        }
    }
);

impl StartCanvasView {
    /// `StartCanvasView()`.
    fn new(mtm: MainThreadMarker) -> Retained<StartCanvasView> {
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}
