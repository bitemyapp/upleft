//! Port of `App/ThemedSplitView.swift`: a split view whose divider belongs to
//! the theme.
//!
//! `NSSplitView` paints a thin divider in a system grey that all but vanishes
//! against a themed page: two columns of prose meet with no seam, and the one
//! piece of chrome saying "this is draggable" is invisible. Drawing it in
//! `rule` puts the seam at the same weight as every other separator in the
//! app, and hovering lifts a short segment of it into a grip.
//!
//! Deliberately no animation: a hairline that crossfades on every pass of the
//! pointer is a distraction in a window meant for reading, and the resize
//! cursor AppKit already shows arrives at the same moment.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSBezierPath, NSColor, NSEvent, NSResponder, NSSplitView, NSSplitViewDividerStyle, NSTrackingArea,
    NSTrackingAreaOptions, NSView,
};
use objc2_core_foundation::CGFloat;
use objc2_core_graphics::CGRectEqualToRect;
use objc2_foundation::{NSPoint, NSRect};
use upleft_render::appkit_compat::{RECT_ZERO, RectExt, rect};
use upleft_render::swift_compat::{smax, smin};
use upleft_render::theme::style_sheet::StyleSheet;

/// `ThemedSplitView.Grip`.
struct Grip;

impl Grip {
    /// Length of the brightened segment along the divider.
    const LENGTH: CGFloat = 26.0;
    /// A hairline you must land on exactly is not an affordance. Hover
    /// resolves over a band wide enough to find without aiming.
    const SLOP: CGFloat = 3.0;
}

pub struct ThemedSplitViewIvars {
    style_sheet: RefCell<Rc<StyleSheet>>,
    hovered_band: Cell<Option<NSRect>>,
    hover_tracking: RefCell<Option<Retained<NSTrackingArea>>>,
}

define_class!(
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set;
    // overrides keep AppKit's signatures.
    #[unsafe(super(NSSplitView, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "ThemedSplitView"]
    #[ivars = ThemedSplitViewIvars]
    pub struct ThemedSplitView;

    unsafe impl NSObjectProtocol for ThemedSplitView {}

    impl ThemedSplitView {
        #[unsafe(method_id(dividerColor))]
        fn __divider_color(&self) -> Retained<NSColor> {
            self.ivars().style_sheet.borrow().rule.clone()
        }

        #[unsafe(method(drawDividerInRect:))]
        fn __draw_divider(&self, rect: NSRect) {
            self.draw_divider(rect);
        }

        #[unsafe(method(updateTrackingAreas))]
        fn __update_tracking_areas(&self) {
            self.update_tracking_areas();
        }

        #[unsafe(method(mouseMoved:))]
        fn __mouse_moved(&self, event: &NSEvent) {
            let _: () = unsafe { msg_send![super(self), mouseMoved: event] };
            self.update_hover(Some(self.convertPoint_fromView(event.locationInWindow(), None)));
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, event: &NSEvent) {
            let _: () = unsafe { msg_send![super(self), mouseExited: event] };
            self.update_hover(None);
        }
    }
);

impl ThemedSplitView {
    /// `init(styleSheet:isVertical:)`; Swift's `isVertical` defaults to true.
    pub fn new(style_sheet: Rc<StyleSheet>, is_vertical: bool, mtm: MainThreadMarker) -> Retained<ThemedSplitView> {
        let this = Self::alloc(mtm).set_ivars(ThemedSplitViewIvars {
            style_sheet: RefCell::new(style_sheet),
            hovered_band: Cell::new(None),
            hover_tracking: RefCell::new(None),
        });
        let this: Retained<ThemedSplitView> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        this.setVertical(is_vertical);
        this.setDividerStyle(NSSplitViewDividerStyle::Thin);
        this
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    /// `styleSheet { didSet { needsDisplay = true } }`.
    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet;
        self.setNeedsDisplay(true);
    }

    fn draw_divider(&self, rect: NSRect) {
        let _: () = unsafe { msg_send![super(self), drawDividerInRect: rect] };
        let Some(hovered_band) = self.ivars().hovered_band.get() else { return };
        if !hovered_band.intersects(rect) {
            return;
        }
        self.ivars().style_sheet.borrow().marker.setFill();
        let grip = self.grip_rect(rect);
        let radius = smin(grip.width(), grip.height()) / 2.0;
        NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(grip, radius, radius).fill();
    }

    /// The grip lives inside the divider rect, so it costs no layout, cannot
    /// be clipped, and never shifts the panes on either side of it.
    fn grip_rect(&self, divider: NSRect) -> NSRect {
        if self.isVertical() {
            let height = smin(Grip::LENGTH, divider.height());
            return rect(divider.min_x(), divider.mid_y() - height / 2.0, divider.width(), height);
        }
        let width = smin(Grip::LENGTH, divider.width());
        rect(divider.mid_x() - width / 2.0, divider.min_y(), width, divider.height())
    }

    /// The gaps between live panes. Derived rather than cached because a drag
    /// moves them continuously, and `min`/`max` keeps it correct whichever
    /// way the coordinate system runs.
    fn divider_bands(&self) -> Vec<NSRect> {
        let panes: Vec<Retained<NSView>> =
            self.arrangedSubviews().iter().filter(|pane| !self.isSubviewCollapsed(pane)).collect();
        if !(panes.len() > 1) {
            return Vec::new();
        }
        let is_vertical = self.isVertical();
        let bounds = self.bounds();
        let divider_thickness = self.dividerThickness();
        panes
            .iter()
            .zip(panes.iter().skip(1))
            .map(|(lead, follow)| {
                if is_vertical {
                    let start = smin(lead.frame().max_x(), follow.frame().min_x());
                    let end = smax(lead.frame().max_x(), follow.frame().min_x());
                    return rect(start, bounds.min_y(), smax(divider_thickness, end - start), bounds.height());
                }
                let start = smin(lead.frame().max_y(), follow.frame().min_y());
                let end = smax(lead.frame().max_y(), follow.frame().min_y());
                rect(bounds.min_x(), start, bounds.width(), smax(divider_thickness, end - start))
            })
            .collect()
    }

    // MARK: - Hover

    fn update_tracking_areas(&self) {
        let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
        // Remove only our own: `NSSplitView` installs areas of its own for the
        // divider cursor, and clearing the lot takes the resize cursor with it.
        let existing = self.ivars().hover_tracking.borrow().clone();
        if let Some(hover_tracking) = existing {
            self.removeTrackingArea(&hover_tracking);
        }
        // SAFETY: the owner is the view itself, which outlives its own area.
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                RECT_ZERO,
                NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::MouseMoved
                    | NSTrackingAreaOptions::ActiveInKeyWindow
                    | NSTrackingAreaOptions::InVisibleRect,
                Some(self),
                None,
            )
        };
        self.addTrackingArea(&area);
        *self.ivars().hover_tracking.borrow_mut() = Some(area);
    }

    fn update_hover(&self, point: Option<NSPoint>) {
        let is_vertical = self.isVertical();
        let band = point.and_then(|location| {
            self.divider_bands().into_iter().find(|band| {
                band.inset_by(if is_vertical { -Grip::SLOP } else { 0.0 }, if is_vertical { 0.0 } else { -Grip::SLOP })
                    .contains_point(location)
            })
        });
        // Redrawing on every pointer sample would repaint the seam
        // continuously while someone is only reading; only a change of state
        // is worth a pass.
        let hovered_band = self.ivars().hovered_band.get();
        let unchanged = match (band, hovered_band) {
            (None, None) => true,
            (Some(band), Some(hovered)) => CGRectEqualToRect(band, hovered),
            _ => false,
        };
        if unchanged {
            return;
        }
        self.ivars().hovered_band.set(band);
        self.setNeedsDisplay(true);
    }
}
