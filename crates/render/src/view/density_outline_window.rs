//! Port of `View/DensityOutlineWindow.swift`: the expanded navigation rail.
//! It is a child window so it can grow over the document without changing
//! the document measure or split-view geometry.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send};
use objc2_app_kit::{
    NSBackingStoreType, NSControlTextEditingDelegate, NSBezierPath, NSColor, NSControl, NSEvent, NSFloatingWindowLevel,
    NSFont, NSFontWeightRegular, NSFontWeightSemibold, NSLayoutConstraint, NSLineBreakMode, NSPanel, NSResponder,
    NSScreen, NSScrollView, NSTableCellView, NSTableColumn, NSTableColumnResizingOptions, NSTableView,
    NSTableViewDataSource, NSTableViewDelegate, NSTableViewSelectionHighlightStyle, NSTextField, NSTrackingArea,
    NSTrackingAreaOptions, NSUserInterfaceItemIdentification, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
    NSVisualEffectView, NSWindow, NSWindowAnimationBehavior, NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSArray, NSIndexSet, NSNotification, NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::{CABasicAnimation, CAMediaTiming};

use crate::appkit_compat::{RECT_ZERO, RectExt, rect, rect_fill};
use crate::motion::{self, Curve};
use crate::swift_compat::{smax, smin};
use crate::theme::style_sheet::StyleSheet;
use crate::view::style_sheet_defaults::GutterChrome;
use crate::view::tracking_area::refresh_tracking_area;

/// `DensityOutlineEntry`.
#[derive(Debug, Clone, PartialEq)]
pub struct DensityOutlineEntry {
    pub title: String,
    pub level: isize,
    pub fraction: CGFloat,
    pub is_current: bool,
}

impl DensityOutlineEntry {
    pub fn new(title: impl Into<String>, level: isize, fraction: CGFloat, is_current: bool) -> DensityOutlineEntry {
        DensityOutlineEntry { title: title.into(), level, fraction, is_current }
    }
}

pub struct DensityOutlineWindowIvars {
    style_sheet: RefCell<Rc<StyleSheet>>,
    entries: RefCell<Vec<DensityOutlineEntry>>,
    on_select: RefCell<Option<Rc<dyn Fn(CGFloat)>>>,
    on_pointer_presence: RefCell<Option<Rc<dyn Fn(bool)>>>,
    table: Retained<OutlineTableView>,
    #[allow(dead_code)]
    scroll: Retained<NSScrollView>,
    backdrop: Retained<OutlineBackdrop>,
    presented_frame: Cell<NSRect>,
    presented_opens_inward: Cell<bool>,
    dismiss_generation: Cell<isize>,
}

define_class!(
    // SAFETY: `initWithContentRect:styleMask:backing:defer:` is forwarded in
    // `new` after the ivars are set. Drop is on the ivars only.
    #[unsafe(super(NSPanel, NSWindow, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "DensityOutlineWindow"]
    #[ivars = DensityOutlineWindowIvars]
    pub struct DensityOutlineWindow;

    unsafe impl NSObjectProtocol for DensityOutlineWindow {}

    impl DensityOutlineWindow {
        #[unsafe(method(canBecomeKeyWindow))]
        fn __can_become_key(&self) -> bool {
            true
        }

        #[unsafe(method(cancelOperation:))]
        fn __cancel_operation(&self, _sender: Option<&AnyObject>) {
            self.dismiss();
        }
    }

    unsafe impl NSTableViewDataSource for DensityOutlineWindow {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn __number_of_rows(&self, _table_view: &NSTableView) -> isize {
            self.ivars().entries.borrow().len() as isize
        }
    }

    unsafe impl NSControlTextEditingDelegate for DensityOutlineWindow {}

    unsafe impl NSTableViewDelegate for DensityOutlineWindow {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn __view_for(
            &self,
            table_view: &NSTableView,
            _table_column: Option<&NSTableColumn>,
            row: isize,
        ) -> Option<Retained<NSView>> {
            self.view_for(table_view, row)
        }

        #[unsafe(method(tableViewSelectionDidChange:))]
        fn __selection_did_change(&self, _notification: &NSNotification) {
            NSView::setNeedsDisplay(&self.ivars().table, true);
        }
    }
);

impl DensityOutlineWindow {
    pub const ROW_HEIGHT: CGFloat = 44.0;
    pub const CORNER_RADIUS: CGFloat = 14.0;
    pub const SHOW_DWELL: f64 = 0.25;
    pub const HIDE_DELAY: f64 = 0.09;
    pub const SHOW_DURATION: f64 = 0.12;
    pub const HIDE_DURATION: f64 = 0.09;

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<DensityOutlineWindow> {
        let table = OutlineTableView::new(mtm);
        let scroll = NSScrollView::new(mtm);
        let backdrop = OutlineBackdrop::new(style_sheet.clone(), mtm);
        let this = Self::alloc(mtm).set_ivars(DensityOutlineWindowIvars {
            style_sheet: RefCell::new(style_sheet),
            entries: RefCell::new(Vec::new()),
            on_select: RefCell::new(None),
            on_pointer_presence: RefCell::new(None),
            table: table.clone(),
            scroll: scroll.clone(),
            backdrop: backdrop.clone(),
            presented_frame: Cell::new(RECT_ZERO),
            presented_opens_inward: Cell::new(false),
            dismiss_generation: Cell::new(0),
        });
        let this: Retained<DensityOutlineWindow> = unsafe {
            msg_send![
                super(this),
                initWithContentRect: rect(0.0, 0.0, 360.0, 300.0),
                styleMask: NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
                backing: NSBackingStoreType::Buffered,
                defer: true
            ]
        };
        this.setOpaque(false);
        this.setBackgroundColor(Some(&NSColor::clearColor()));
        this.setHasShadow(true);
        this.setLevel(NSFloatingWindowLevel);
        this.setHidesOnDeactivate(true);
        // SAFETY: the panel is owned by the gutter, never released on close.
        unsafe { this.setReleasedWhenClosed(false) };
        this.setAnimationBehavior(NSWindowAnimationBehavior::None);

        table.setHeaderView(None);
        table.setBackgroundColor(&NSColor::clearColor());
        table.setRowHeight(Self::ROW_HEIGHT);
        table.setIntercellSpacing(NSSize::new(0.0, 0.0));
        table.setSelectionHighlightStyle(NSTableViewSelectionHighlightStyle::None);
        // SAFETY: the panel owns the table and outlives it.
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(&*this)));
            table.setDelegate(Some(ProtocolObject::from_ref(&*this)));
        }
        let weak: ObjcWeak<DensityOutlineWindow> = ObjcWeak::from(&*this);
        table.set_on_single_click(Some(Rc::new(move || {
            if let Some(this) = weak.load() {
                this.activate_selection();
            }
        })));
        let weak: ObjcWeak<DensityOutlineWindow> = ObjcWeak::from(&*this);
        table.set_on_activate(Some(Rc::new(move || {
            if let Some(this) = weak.load() {
                this.activate_selection();
            }
        })));
        let column = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), &NSString::from_str("outline"));
        column.setResizingMask(NSTableColumnResizingOptions::AutoresizingMask);
        table.addTableColumn(&column);

        scroll.setDocumentView(Some(&table));
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setDrawsBackground(false);
        scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
        backdrop.addSubview(&scroll);
        let constraints = NSArray::from_retained_slice(&[
            scroll.leadingAnchor().constraintEqualToAnchor_constant(&backdrop.leadingAnchor(), 4.0),
            scroll.trailingAnchor().constraintEqualToAnchor_constant(&backdrop.trailingAnchor(), -4.0),
            scroll.topAnchor().constraintEqualToAnchor_constant(&backdrop.topAnchor(), 6.0),
            scroll.bottomAnchor().constraintEqualToAnchor_constant(&backdrop.bottomAnchor(), -6.0),
        ]);
        NSLayoutConstraint::activateConstraints(&constraints);
        this.setContentView(Some(&backdrop));
        let weak: ObjcWeak<DensityOutlineWindow> = ObjcWeak::from(&*this);
        backdrop.set_on_pointer_presence(Some(Rc::new(move |is_inside| {
            let Some(this) = weak.load() else { return };
            let callback = this.ivars().on_pointer_presence.borrow().clone();
            if let Some(callback) = callback {
                callback(is_inside);
            }
        })));
        this
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet.clone();
        self.ivars().backdrop.set_style_sheet(style_sheet);
        self.ivars().table.reloadData();
    }

    pub fn entries(&self) -> Vec<DensityOutlineEntry> {
        self.ivars().entries.borrow().clone()
    }

    pub fn set_entries(&self, entries: Vec<DensityOutlineEntry>) {
        let old_value = std::mem::replace(&mut *self.ivars().entries.borrow_mut(), entries);
        let is_empty = self.ivars().entries.borrow().is_empty();
        if !(self.isVisible() || is_empty != old_value.is_empty()) {
            return;
        }
        self.ivars().table.reloadData();
    }

    pub fn set_on_select(&self, callback: Option<Box<dyn Fn(CGFloat)>>) {
        *self.ivars().on_select.borrow_mut() = callback.map(Rc::from);
    }

    pub fn set_on_pointer_presence(&self, callback: Option<Box<dyn Fn(bool)>>) {
        *self.ivars().on_pointer_presence.borrow_mut() = callback.map(Rc::from);
    }

    /// The outline's table, for harnesses that inspect it.
    pub fn table(&self) -> &Retained<OutlineTableView> {
        &self.ivars().table
    }

    fn animator(&self) -> Retained<AnyObject> {
        unsafe { msg_send![self, animator] }
    }

    pub fn show(&self, rail: &NSView, parent: &NSWindow, keyboard: bool, anchor_y: Option<CGFloat>) {
        let ivars = self.ivars();
        let entry_count = ivars.entries.borrow().len();
        if entry_count == 0 {
            return;
        }
        let style_sheet = self.style_sheet();
        let table = ivars.table.clone();
        let rail_frame = rail.convertRect_toView(rail.bounds(), None);
        let rail_screen = parent.convertRectToScreen(rail_frame);
        let maximum_height = smax(160.0, (parent.frame().height() * 0.70).floor());
        let desired_height = smin(maximum_height, entry_count as CGFloat * table.rowHeight() + 12.0);
        let size = NSSize::new(360.0, desired_height);
        let focus_y = match anchor_y {
            None => rail_screen.mid_y(),
            Some(anchor_y) => {
                let local = NSPoint::new(rail.bounds().mid_x(), anchor_y);
                let window_point = rail.convertPoint_toView(local, None);
                parent.convertRectToScreen(NSRect::new(window_point, NSSize::new(0.0, 0.0))).mid_y()
            }
        };
        let opens_inward = rail_screen.mid_x() > parent.frame().mid_x();
        let mut origin = NSPoint::new(
            if opens_inward { rail_screen.min_x() - size.width - 8.0 } else { rail_screen.max_x() + 8.0 },
            focus_y - size.height / 2.0,
        );
        if let Some(visible) =
            parent.screen().or_else(|| NSScreen::mainScreen(self.mtm())).map(|screen| screen.visibleFrame())
        {
            origin.x = smax(visible.min_x() + 4.0, origin.x);
            origin.y = smin(smax(visible.min_y() + 4.0, origin.y), visible.max_y() - size.height - 4.0);
        }
        let final_frame = NSRect::new(origin, size);
        ivars.dismiss_generation.set(ivars.dismiss_generation.get() + 1);
        ivars.presented_frame.set(final_frame);
        ivars.presented_opens_inward.set(opens_inward);
        self.setFrame_display(
            final_frame.offset_by(
                if style_sheet.reduce_motion {
                    0.0
                } else if opens_inward {
                    4.0
                } else {
                    -4.0
                },
                0.0,
            ),
            true,
        );
        if !self.parentWindow().is_some_and(|current| std::ptr::eq(&*current, parent)) {
            // SAFETY: the child is a live panel owned by the gutter.
            unsafe { parent.addChildWindow_ordered(self, NSWindowOrderingMode::Above) };
        }
        self.setAlphaValue(if style_sheet.reduce_motion { 1.0 } else { 0.0 });
        self.orderFront(None);

        let current = ivars.entries.borrow().iter().position(|entry| entry.is_current).unwrap_or(0);
        table.selectRowIndexes_byExtendingSelection(&NSIndexSet::indexSetWithIndex(current), false);
        table.scrollRowToVisible(current as isize);
        if keyboard {
            self.makeKeyWindow();
            self.makeFirstResponder(Some(&table));
        }
        let this = self.retain();
        GutterChrome::animate(
            style_sheet.reduce_motion,
            Self::SHOW_DURATION,
            move |_| {
                let animator = this.animator();
                let _: () = unsafe { msg_send![&*animator, setAlphaValue: 1.0 as CGFloat] };
                let _: () = unsafe { msg_send![&*animator, setFrame: final_frame, display: true] };
            },
            None,
        );
    }

    pub fn dismiss(&self) {
        if !self.isVisible() {
            return;
        }
        let ivars = self.ivars();
        ivars.dismiss_generation.set(ivars.dismiss_generation.get() + 1);
        let generation = ivars.dismiss_generation.get();
        let parent_window = self.parentWindow();
        let weak: ObjcWeak<DensityOutlineWindow> = ObjcWeak::from(self);
        let remove = Rc::new(move || {
            let Some(this) = weak.load() else { return };
            if this.ivars().dismiss_generation.get() != generation {
                return;
            }
            if let Some(parent_window) = &parent_window {
                parent_window.removeChildWindow(&this);
            }
            this.orderOut(None);
            this.setAlphaValue(1.0);
        });
        if self.style_sheet().reduce_motion {
            remove();
            return;
        }
        let frame = self.frame();
        let this = self.retain();
        GutterChrome::animate(
            false,
            Self::HIDE_DURATION,
            move |_| {
                let animator = this.animator();
                let _: () = unsafe { msg_send![&*animator, setAlphaValue: 0.0 as CGFloat] };
                let dx = if this.ivars().presented_opens_inward.get() { 4.0 } else { -4.0 };
                let _: () = unsafe { msg_send![&*animator, setFrame: frame.offset_by(dx, 0.0), display: true] };
            },
            Some(Box::new(move || remove())),
        );
    }

    pub fn cancel_dismiss_animation(&self) {
        if !self.isVisible() {
            return;
        }
        let ivars = self.ivars();
        ivars.dismiss_generation.set(ivars.dismiss_generation.get() + 1);
        self.setAlphaValue(1.0);
        self.setFrame_display(ivars.presented_frame.get(), true);
    }

    fn view_for(&self, table_view: &NSTableView, row: isize) -> Option<Retained<NSView>> {
        let entry = {
            let entries = self.ivars().entries.borrow();
            if !(row >= 0 && (row as usize) < entries.len()) {
                return None;
            }
            entries[row as usize].clone()
        };
        let identifier = NSString::from_str("densityOutlineRow");
        // SAFETY: the owner is this panel, alive for the call.
        let reused = unsafe { table_view.makeViewWithIdentifier_owner(&identifier, Some(self)) }
            .and_then(|view| view.downcast::<DensityOutlineRow>().ok());
        let cell = reused.unwrap_or_else(|| DensityOutlineRow::new(&identifier, self.mtm()));
        cell.configure(&entry, self.style_sheet());
        Some(Retained::into_super(Retained::into_super(cell)))
    }

    fn activate_selection(&self) {
        let table = &self.ivars().table;
        let row = if table.clickedRow() >= 0 { table.clickedRow() } else { table.selectedRow() };
        let fraction = {
            let entries = self.ivars().entries.borrow();
            if !(row >= 0 && (row as usize) < entries.len()) {
                return;
            }
            entries[row as usize].fraction
        };
        self.dismiss();
        let callback = self.ivars().on_select.borrow().clone();
        if let Some(callback) = callback {
            callback(fraction);
        }
    }
}

// MARK: - OutlineTableView

pub struct OutlineTableViewIvars {
    on_single_click: RefCell<Option<Rc<dyn Fn()>>>,
    on_activate: RefCell<Option<Rc<dyn Fn()>>>,
}

define_class!(
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set;
    // overrides keep AppKit's signatures.
    #[unsafe(super(NSTableView, NSControl, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "OutlineTableView"]
    #[ivars = OutlineTableViewIvars]
    pub struct OutlineTableView;

    unsafe impl NSObjectProtocol for OutlineTableView {}

    impl OutlineTableView {
        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, event: &NSEvent) {
            let row = self.rowAtPoint(self.convertPoint_fromView(event.locationInWindow(), None));
            let _: () = unsafe { msg_send![super(self), mouseDown: event] };
            if !(row >= 0) {
                return;
            }
            let callback = self.ivars().on_single_click.borrow().clone();
            if let Some(callback) = callback {
                callback();
            }
        }

        #[unsafe(method(keyDown:))]
        fn __key_down(&self, event: &NSEvent) {
            if event.keyCode() == 36 || event.keyCode() == 76 {
                let callback = self.ivars().on_activate.borrow().clone();
                if let Some(callback) = callback {
                    callback();
                }
                return;
            }
            let _: () = unsafe { msg_send![super(self), keyDown: event] };
        }
    }
);

impl OutlineTableView {
    fn new(mtm: MainThreadMarker) -> Retained<OutlineTableView> {
        let this = Self::alloc(mtm)
            .set_ivars(OutlineTableViewIvars { on_single_click: RefCell::new(None), on_activate: RefCell::new(None) });
        unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] }
    }

    fn set_on_single_click(&self, callback: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_single_click.borrow_mut() = callback;
    }

    fn set_on_activate(&self, callback: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_activate.borrow_mut() = callback;
    }
}

// MARK: - DensityOutlineRow

pub struct DensityOutlineRowIvars {
    label: Retained<NSTextField>,
    leading: RefCell<Option<Retained<NSLayoutConstraint>>>,
    tracking_area: RefCell<Option<Retained<NSTrackingArea>>>,
    is_hovered: Cell<bool>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    is_current: Cell<bool>,
}

define_class!(
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set;
    // overrides keep AppKit's signatures.
    #[unsafe(super(NSTableCellView, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "DensityOutlineRow"]
    #[ivars = DensityOutlineRowIvars]
    pub struct DensityOutlineRow;

    unsafe impl NSObjectProtocol for DensityOutlineRow {}

    impl DensityOutlineRow {
        #[unsafe(method(updateTrackingAreas))]
        fn __update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            refresh_tracking_area(
                self,
                &self.ivars().tracking_area,
                NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::ActiveAlways
                    | NSTrackingAreaOptions::InVisibleRect,
            );
        }

        #[unsafe(method(mouseEntered:))]
        fn __mouse_entered(&self, _event: &NSEvent) {
            self.ivars().is_hovered.set(true);
            self.apply_background(true);
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, _event: &NSEvent) {
            self.ivars().is_hovered.set(false);
            self.apply_background(true);
        }

        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, dirty_rect: NSRect) {
            let _: () = unsafe { msg_send![super(self), drawRect: dirty_rect] };
        }
    }
);

impl DensityOutlineRow {
    fn new(identifier: &NSString, mtm: MainThreadMarker) -> Retained<DensityOutlineRow> {
        let label = NSTextField::labelWithString(&NSString::from_str(""), mtm);
        let this = Self::alloc(mtm).set_ivars(DensityOutlineRowIvars {
            label: label.clone(),
            leading: RefCell::new(None),
            tracking_area: RefCell::new(None),
            is_hovered: Cell::new(false),
            style_sheet: RefCell::new(Rc::new(StyleSheet::current(mtm))),
            is_current: Cell::new(false),
        });
        let this: Retained<DensityOutlineRow> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        this.setIdentifier(Some(identifier));
        label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        this.setWantsLayer(true);
        label.setTranslatesAutoresizingMaskIntoConstraints(false);
        this.addSubview(&label);
        let leading = label.leadingAnchor().constraintEqualToAnchor_constant(&this.leadingAnchor(), 12.0);
        *this.ivars().leading.borrow_mut() = Some(leading.clone());
        let constraints = NSArray::from_retained_slice(&[
            leading,
            label.trailingAnchor().constraintEqualToAnchor_constant(&this.trailingAnchor(), -10.0),
            label.centerYAnchor().constraintEqualToAnchor(&this.centerYAnchor()),
        ]);
        NSLayoutConstraint::activateConstraints(&constraints);
        this
    }

    fn apply_background(&self, animated: bool) {
        let ivars = self.ivars();
        let style_sheet = ivars.style_sheet.borrow().clone();
        let alpha: CGFloat = if ivars.is_current.get() {
            0.08
        } else if ivars.is_hovered.get() {
            0.05
        } else {
            0.0
        };
        let target = style_sheet.text.colorWithAlphaComponent(alpha).CGColor();
        let Some(layer) = self.layer() else { return };
        if !(animated && !style_sheet.reduce_motion && self.window().is_some()) {
            layer.setBackgroundColor(Some(&target));
            return;
        }
        let fade = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str("backgroundColor")));
        // SAFETY: the presentation layer is only read.
        let from = unsafe { layer.presentationLayer() }
            .and_then(|presentation| presentation.backgroundColor())
            .or_else(|| layer.backgroundColor());
        // SAFETY: CGColor is a CoreFoundation object, valid as an animation value.
        unsafe {
            fade.setFromValue(from.as_deref().map(|color| &*(color as *const objc2_core_graphics::CGColor as *const AnyObject)));
            fade.setToValue(Some(&*(&*target as *const objc2_core_graphics::CGColor as *const AnyObject)));
        }
        fade.setDuration(motion::HOVER);
        fade.setTimingFunction(Some(&motion::timing(Curve::Snap)));
        layer.addAnimation_forKey(&fade, Some(&NSString::from_str("row-hover")));
        layer.setBackgroundColor(Some(&target));
    }

    fn configure(&self, entry: &DensityOutlineEntry, style_sheet: Rc<StyleSheet>) {
        let ivars = self.ivars();
        *ivars.style_sheet.borrow_mut() = style_sheet.clone();
        ivars.is_current.set(entry.is_current);
        if let Some(leading) = ivars.leading.borrow().as_ref() {
            leading.setConstant(12.0 + 0isize.max(5isize.min(entry.level - 1)) as CGFloat * 14.0);
        }
        ivars.label.setStringValue(&NSString::from_str(&entry.title));
        // SAFETY: AppKit exports the weights as immutable globals.
        let weight = unsafe { if entry.is_current { NSFontWeightSemibold } else { NSFontWeightRegular } };
        ivars.label.setFont(Some(&NSFont::systemFontOfSize_weight(12.0, weight)));
        ivars.label.setTextColor(Some(if entry.is_current { &style_sheet.accent } else { &style_sheet.text }));
        if let Some(layer) = self.layer() {
            layer.setCornerRadius(6.0);
        }
        self.apply_background(false);
    }

    /// The row's label, for harnesses that inspect it.
    pub fn label(&self) -> &Retained<NSTextField> {
        &self.ivars().label
    }
}

// MARK: - OutlineBackdrop

pub struct OutlineBackdropIvars {
    style_sheet: RefCell<Rc<StyleSheet>>,
    on_pointer_presence: RefCell<Option<Rc<dyn Fn(bool)>>>,
    tracking_area: RefCell<Option<Retained<NSTrackingArea>>>,
}

define_class!(
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set;
    // overrides keep AppKit's signatures.
    #[unsafe(super(NSVisualEffectView, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "OutlineBackdrop"]
    #[ivars = OutlineBackdropIvars]
    pub struct OutlineBackdrop;

    unsafe impl NSObjectProtocol for OutlineBackdrop {}

    impl OutlineBackdrop {
        #[unsafe(method(updateTrackingAreas))]
        fn __update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            refresh_tracking_area(
                self,
                &self.ivars().tracking_area,
                NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::ActiveAlways
                    | NSTrackingAreaOptions::InVisibleRect,
            );
        }

        #[unsafe(method(mouseEntered:))]
        fn __mouse_entered(&self, _event: &NSEvent) {
            self.pointer_presence(true);
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, _event: &NSEvent) {
            self.pointer_presence(false);
        }

        #[unsafe(method(drawRect:))]
        fn __draw_rect(&self, dirty_rect: NSRect) {
            let style_sheet = self.ivars().style_sheet.borrow().clone();
            self.setMaterial(if style_sheet.reduce_transparency {
                NSVisualEffectMaterial::WindowBackground
            } else {
                NSVisualEffectMaterial::Popover
            });
            self.setBlendingMode(if style_sheet.reduce_transparency {
                NSVisualEffectBlendingMode::WithinWindow
            } else {
                NSVisualEffectBlendingMode::BehindWindow
            });
            let _: () = unsafe { msg_send![super(self), drawRect: dirty_rect] };
            if style_sheet.reduce_transparency {
                style_sheet.background.setFill();
                rect_fill(self.bounds());
            }
            style_sheet.rule.setStroke();
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                self.bounds().inset_by(0.5, 0.5),
                DensityOutlineWindow::CORNER_RADIUS,
                DensityOutlineWindow::CORNER_RADIUS,
            );
            path.setLineWidth(1.0);
            path.stroke();
        }
    }
);

impl OutlineBackdrop {
    fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<OutlineBackdrop> {
        let this = Self::alloc(mtm).set_ivars(OutlineBackdropIvars {
            style_sheet: RefCell::new(style_sheet),
            on_pointer_presence: RefCell::new(None),
            tracking_area: RefCell::new(None),
        });
        let this: Retained<OutlineBackdrop> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        this.setMaterial(NSVisualEffectMaterial::Popover);
        this.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        this.setState(NSVisualEffectState::Active);
        this.setWantsLayer(true);
        if let Some(layer) = this.layer() {
            layer.setCornerRadius(DensityOutlineWindow::CORNER_RADIUS);
            layer.setMasksToBounds(true);
        }
        this
    }

    fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet;
        self.setNeedsDisplay(true);
    }

    fn set_on_pointer_presence(&self, callback: Option<Rc<dyn Fn(bool)>>) {
        *self.ivars().on_pointer_presence.borrow_mut() = callback;
    }

    fn pointer_presence(&self, is_inside: bool) {
        let callback = self.ivars().on_pointer_presence.borrow().clone();
        if let Some(callback) = callback {
            callback(is_inside);
        }
    }
}

