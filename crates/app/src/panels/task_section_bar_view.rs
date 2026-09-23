//! Port of `Panels/TaskSectionBarView.swift`: the section-map progress bar
//! (§8.5).
//!
//! A whole-plan bar answers "how much is left"; this one answers "where is it
//! left".  One segment per document section, width proportional to its task
//! count, fill for its completion — the shape of the remaining work is visible
//! as geography before a single row is read. Hovering lights a segment;
//! clicking scrolls the list to its section, so the bar is a map rather than a
//! second whole-plan meter.
//!
//! The bar replaces the old header's percent figure, count caption, and plain
//! progress bar — three chrome elements that between them said one thing.

// `!(a > b)` spells Swift's `guard a > b`; the negated comparisons are
// deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{Bool, NSObjectProtocol};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityCustomAction, NSCursor, NSEvent, NSFocusRingType, NSResponder, NSTrackingArea,
    NSTrackingAreaOptions, NSView, NSViewNoIntrinsicMetric,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSArray, NSNumber, NSObjectNSKeyValueCoding, NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::{CACurrentMediaTime, CALayer, CATransaction};
use upleft_core::task_worklist::Segment;
use upleft_render::motion::{self, Curve};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::view::style_sheet_defaults::PanelAlpha;

use super::appkit_support::{RECT_ZERO, RectExt, cg, rect, role, set_label, set_role, set_value, smax};
use super::panel_chrome::{PanelMetrics, refresh_tracking_area};

/// `TaskSectionBarView.SegmentLayers`.
struct SegmentLayers {
    track: Retained<CALayer>,
    fill: Retained<CALayer>,
}

impl SegmentLayers {
    fn new() -> SegmentLayers {
        SegmentLayers { track: CALayer::new(), fill: CALayer::new() }
    }
}

pub struct TaskSectionBarViewIvars {
    segments: RefCell<Rc<Vec<Segment>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    on_select_segment: RefCell<Option<Rc<dyn Fn(isize)>>>,
    segment_layers: RefCell<Rc<Vec<SegmentLayers>>>,
    tracking_area_ref: RefCell<Option<Retained<NSTrackingArea>>>,
    hovered_index: Cell<Option<isize>>,
    pressed_index: Cell<Option<isize>>,
    keyboard_index: Cell<isize>,
}

define_class!(
    /// `TaskSectionBarView`.
    // SAFETY: `initWithFrame:` is forwarded in `new` after the ivars are set;
    // Swift's `init(frame:)` is unavailable, as here.
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "TaskSectionBarView"]
    #[ivars = TaskSectionBarViewIvars]
    pub struct TaskSectionBarView;

    unsafe impl NSObjectProtocol for TaskSectionBarView {}

    impl TaskSectionBarView {
        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            NSSize::new(unsafe { NSViewNoIntrinsicMetric }, Self::HIT_HEIGHT)
        }

        // MARK: - Layout

        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            self.place_segments(false, motion::DELIBERATE);
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.apply_style();
        }

        // MARK: - Pointer

        #[unsafe(method(updateTrackingAreas))]
        fn __update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            refresh_tracking_area(
                self,
                &self.ivars().tracking_area_ref,
                NSTrackingAreaOptions::MouseMoved
                    | NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::ActiveInKeyWindow,
            );
        }

        #[unsafe(method(mouseMoved:))]
        fn __mouse_moved(&self, event: &NSEvent) {
            let index = self.hit_segment(self.convertPoint_fromView(event.locationInWindow(), None));
            if index == self.ivars().hovered_index.get() {
                return;
            }
            self.set_hovered_index(index);
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, _event: &NSEvent) {
            self.set_hovered_index(None);
        }

        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, event: &NSEvent) {
            self.set_pressed_index(self.hit_segment(self.convertPoint_fromView(event.locationInWindow(), None)));
        }

        #[unsafe(method(mouseUp:))]
        fn __mouse_up(&self, event: &NSEvent) {
            let index = self.hit_segment(self.convertPoint_fromView(event.locationInWindow(), None));
            if let Some(index) = index
                && Some(index) == self.ivars().pressed_index.get()
            {
                self.select_segment(index);
            }
            self.set_pressed_index(None);
        }

        #[unsafe(method(resetCursorRects))]
        fn __reset_cursor_rects(&self) {
            let segments = self.segments();
            let cursor = NSCursor::pointingHandCursor();
            for index in 0..segments.len() as isize {
                self.addCursorRect_cursor(self.segment_frame(&segments, index).inset_by(0.0, -4.0), &cursor);
            }
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn __accepts_first_responder(&self) -> bool {
            !self.ivars().segments.borrow().is_empty()
        }

        #[unsafe(method(keyDown:))]
        fn __key_down(&self, event: &NSEvent) {
            self.key_down(event);
        }
    }
);

impl TaskSectionBarView {
    /// Tall enough to round its own ends and to read as a bar rather than as a
    /// rule that happens to use the accent colour.
    pub const BAR_HEIGHT: CGFloat = 3.0;
    /// The band a pointer can actually hit; the bar draws centred inside it.
    pub const HIT_HEIGHT: CGFloat = 12.0;
    /// A sliver of backdrop between two segments, so sections read as separate
    /// blocks of work rather than as one gradient.
    pub const GAP: CGFloat = 2.0;

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<TaskSectionBarView> {
        let this = Self::alloc(mtm).set_ivars(TaskSectionBarViewIvars {
            segments: RefCell::new(Rc::new(Vec::new())),
            style_sheet: RefCell::new(style_sheet),
            on_select_segment: RefCell::new(None),
            segment_layers: RefCell::new(Rc::new(Vec::new())),
            tracking_area_ref: RefCell::new(None),
            hovered_index: Cell::new(None),
            pressed_index: Cell::new(None),
            keyboard_index: Cell::new(0),
        });
        let this: Retained<TaskSectionBarView> = unsafe { msg_send![super(this), initWithFrame: RECT_ZERO] };
        this.setWantsLayer(true);
        this.setTranslatesAutoresizingMaskIntoConstraints(false);
        set_role(&*this, role::progress_indicator());
        set_label(&*this, "Progress by section");
        this.setFocusRingType(NSFocusRingType::Default);
        this
    }

    // MARK: - Properties

    pub fn segments(&self) -> Rc<Vec<Segment>> {
        self.ivars().segments.borrow().clone()
    }

    /// `segments` with its `didSet`.
    pub fn set_segments(&self, segments: Vec<Segment>) {
        let old_count = self.ivars().segments.borrow().len();
        let new_count = segments.len();
        *self.ivars().segments.borrow_mut() = Rc::new(segments);
        if new_count != old_count {
            self.rebuild_layers();
        }
        self.place_segments(self.window().is_some(), motion::DELIBERATE);
        self.update_accessibility();
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    /// `styleSheet` with its `didSet`.
    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet;
        self.apply_style();
    }

    /// `onSelectSegment`: a click on a segment; the panel scrolls the
    /// matching section into view.
    pub fn set_on_select_segment(&self, handler: Option<Rc<dyn Fn(isize)>>) {
        *self.ivars().on_select_segment.borrow_mut() = handler;
    }

    fn select_segment(&self, index: isize) {
        let handler = self.ivars().on_select_segment.borrow().clone();
        if let Some(handler) = handler {
            handler(index);
        }
    }

    /// `hoveredIndex` with its `didSet`.
    fn set_hovered_index(&self, value: Option<isize>) {
        let old = self.ivars().hovered_index.replace(value);
        if value == old {
            return;
        }
        self.apply_style();
        // The bar answers the pointer physically: the segment under it
        // swells a step, the way a button's face warms on approach.
        self.place_segments(self.window().is_some(), motion::QUICK);
    }

    /// `pressedIndex` with its `didSet`.
    fn set_pressed_index(&self, value: Option<isize>) {
        let old = self.ivars().pressed_index.replace(value);
        if value == old {
            return;
        }
        self.place_segments(self.window().is_some(), motion::QUICK);
    }

    // MARK: - Layout

    /// The segment's full extent, gaps reserved between neighbours.  A segment
    /// never narrows past the bar's own height: one task in a large plan still
    /// earns a visible, clickable block.
    fn segment_frame(&self, segments: &[Segment], index: isize) -> NSRect {
        let count = segments.len() as isize;
        if !(count > 0 && index < count) {
            return RECT_ZERO;
        }
        let bounds = self.bounds();
        let gaps = (count - 1) as CGFloat * Self::GAP;
        let available = smax(0.0, bounds.width() - gaps);
        let mut x: CGFloat = 0.0;
        for segment in &segments[..index as usize] {
            x += available * segment.weight + Self::GAP;
        }
        let width = smax(Self::BAR_HEIGHT, available * segments[index as usize].weight);
        let height = self.bar_height(index);
        let y = (bounds.height() - height) / 2.0;
        rect(x, y, width, height)
    }

    /// Hover swells a segment one step, a press dips it — the same physics a
    /// row's surface answers with, in the bar's own vocabulary.
    fn bar_height(&self, index: isize) -> CGFloat {
        if Some(index) == self.ivars().pressed_index.get() {
            return Self::BAR_HEIGHT - 1.0;
        }
        if Some(index) == self.ivars().hovered_index.get() {
            return Self::BAR_HEIGHT + 1.5;
        }
        Self::BAR_HEIGHT
    }

    fn hit_segment(&self, point: NSPoint) -> Option<isize> {
        let segments = self.segments();
        if segments.is_empty() {
            return None;
        }
        (0..segments.len() as isize)
            .find(|&index| self.segment_frame(&segments, index).inset_by(-Self::GAP / 2.0, -4.0).contains_point(point))
    }

    fn rebuild_layers(&self) {
        let old = self.ivars().segment_layers.borrow().clone();
        for pair in old.iter() {
            pair.track.removeFromSuperlayer();
            pair.fill.removeFromSuperlayer();
        }
        let count = self.ivars().segments.borrow().len();
        let own = self.layer();
        let layers: Vec<SegmentLayers> = (0..count)
            .map(|_| {
                let pair = SegmentLayers::new();
                for layer in [&pair.track, &pair.fill] {
                    layer.setCornerRadius(PanelMetrics::capsule_radius(Self::BAR_HEIGHT));
                    // Geometry animates through the transaction
                    // `place_segments` wraps around each change — a glide when
                    // counts move, a quick swell under the pointer, nothing at
                    // all when it asks for a plain layout pass.
                    if let Some(own) = &own {
                        own.addSublayer(layer);
                    }
                }
                pair
            })
            .collect();
        *self.ivars().segment_layers.borrow_mut() = Rc::new(layers);
        self.apply_style();
    }

    /// `placeSegments(animated:duration:)`; Swift's default duration is
    /// `Motion.deliberate`.
    fn place_segments(&self, animated: bool, duration: f64) {
        let layers = self.ivars().segment_layers.borrow().clone();
        let segments = self.segments();
        if layers.len() != segments.len() {
            return;
        }
        let reduce_motion = self.ivars().style_sheet.borrow().reduce_motion;
        CATransaction::begin();
        if animated && !reduce_motion {
            // Implicit animation of the frame pair gives each segment one
            // smooth glide when a count changes — the travel the old bar had,
            // kept.
            CATransaction::setAnimationDuration(duration);
            CATransaction::setAnimationTimingFunction(Some(&motion::timing(Curve::Structural)));
        } else {
            CATransaction::setDisableActions(true);
        }
        let first_start = CACurrentMediaTime() + 0.02;
        for (index, pair) in layers.iter().enumerate() {
            let frame = self.segment_frame(&segments, index as isize);
            if animated && !reduce_motion && index > 0 {
                // A count changed: the segments cascade in one behind the
                // other, a `previewStagger` apart, so the bar reads as a wave
                // rather than a swap.
                let start = first_start + index as CGFloat * motion::PREVIEW_STAGGER;
                let key = NSString::from_str("kCATransactionAnimationStartTime");
                let value = NSNumber::new_f64(start);
                unsafe {
                    pair.track.setValue_forKey(Some(&value), &key);
                    pair.fill.setValue_forKey(Some(&value), &key);
                }
            }
            // The caps stay perfectly round at whatever height the pointer
            // state gives the segment, so a swell never squares an end off.
            let radius = PanelMetrics::capsule_radius(frame.height());
            pair.track.setCornerRadius(radius);
            pair.fill.setCornerRadius(radius);
            pair.track.setFrame(frame);
            // The fill keeps a minimum stub as soon as anything is done — a
            // full round cap, not a square sliver.  At zero the layer hides
            // outright: a 0-wide layer with a capsule corner radius still
            // rasterises its two semicircles, which meet as an hourglass
            // sliver in the gap between segments.
            let fraction = segments[index].completion;
            let fill_width = if fraction > 0.0 { smax(frame.height(), frame.width() * fraction) } else { 0.0 };
            pair.fill.setHidden(fill_width == 0.0);
            pair.fill.setFrame(rect(frame.min_x(), frame.min_y(), fill_width, frame.height()));
        }
        CATransaction::commit();
    }

    // MARK: - Style

    fn apply_style(&self) {
        let style_sheet = self.style_sheet();
        let contrast = style_sheet.increase_contrast;
        let layers = self.ivars().segment_layers.borrow().clone();
        let hovered_index = self.ivars().hovered_index.get();
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        for (index, pair) in layers.iter().enumerate() {
            let hovered = Some(index as isize) == hovered_index;
            let track = style_sheet.text.panel_alpha(if hovered { 0.26 } else { 0.16 }, contrast);
            pair.track.setBackgroundColor(Some(&cg(&track)));
            pair.fill.setBackgroundColor(Some(&cg(&style_sheet.accent)));
            pair.fill.setOpacity(1.0);
        }
        CATransaction::commit();
    }

    // MARK: - Keyboard

    fn key_down(&self, event: &NSEvent) {
        let ivars = self.ivars();
        match event.keyCode() {
            123 => ivars.keyboard_index.set(0.max(ivars.keyboard_index.get() - 1)),
            124 => {
                let count = ivars.segments.borrow().len() as isize;
                ivars.keyboard_index.set(0.max(count - 1).min(ivars.keyboard_index.get() + 1));
            }
            36 | 49 => {
                let index = ivars.keyboard_index.get();
                let count = ivars.segments.borrow().len() as isize;
                if !(index >= 0 && index < count) {
                    return;
                }
                self.select_segment(index);
            }
            _ => {
                let _: () = unsafe { msg_send![super(self), keyDown: event] };
                return;
            }
        }
        self.setNeedsDisplayInRect(self.bounds());
    }

    // MARK: - Accessibility

    fn update_accessibility(&self) {
        let segments = self.segments();
        let done: isize = segments.iter().fold(0, |sum, segment| sum + segment.done_count);
        let total: isize = segments.iter().fold(0, |sum, segment| sum + segment.task_count);
        set_value(self, &if total > 0 { format!("{done} of {total} tasks done") } else { "No tasks".to_owned() });
        let actions: Vec<Retained<NSAccessibilityCustomAction>> = segments
            .iter()
            .enumerate()
            .map(|(index, segment)| {
                let weak: ObjcWeak<TaskSectionBarView> = ObjcWeak::from(self);
                let handler = RcBlock::new(move || -> Bool {
                    if let Some(this) = weak.load() {
                        this.select_segment(index as isize);
                    }
                    Bool::YES
                });
                NSAccessibilityCustomAction::initWithName_handler(
                    NSAccessibilityCustomAction::alloc(),
                    &NSString::from_str(&format!("Open {}", segment.title)),
                    Some(&handler),
                )
            })
            .collect();
        self.setAccessibilityCustomActions(Some(&NSArray::from_retained_slice(&actions)));
    }
}
