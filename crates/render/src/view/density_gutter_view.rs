//! Port of `View/DensityGutterView.swift`: the density gutter (§8.6), the
//! scrollbar's replacement.
//!
//! A scrollbar tells you how much is left; this tells you *where the
//! sections are*, and which of them hold something worth going to. The stack
//! is an index of sections: one mark per drawn heading, thinned by depth when
//! the track cannot hold them all, spaced by a pitch derived from the track
//! rather than a fixed gap. Optional diagnostic overlays attach to their
//! section as pips, hidden until a host explicitly enables them.
//!
//! The rail's motion is one physics channel: marks, pips and the whole-rail
//! breathe chase their targets through `motion::SpringScalar` integrators
//! driven by the `SpringSurfaceView` display link.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use block2::RcBlock;
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::{Bool, NSObjectProtocol};
use objc2::{AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAccessibility, NSAccessibilityCustomAction, NSColor, NSColorSpace, NSCursor, NSEvent, NSHapticFeedbackManager,
    NSHapticFeedbackPattern, NSHapticFeedbackPerformanceTime, NSHapticFeedbackPerformer, NSResponder, NSTrackingArea,
    NSTrackingAreaOptions, NSView, NSViewNoIntrinsicMetric,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_core_graphics::CGColor;
use objc2_foundation::{NSArray, NSPoint, NSSize, NSString};
use objc2_quartz_core::{CACurrentMediaTime, CALayer, CATransaction, kCACornerCurveContinuous};

use crate::appkit_compat::{RectExt, WorkItem, rect};
use crate::core_types::{BlockContent, ChangeKind, InlineKind, MDBlock, NSRange, ParsedDocument};
use crate::engine::render_metrics;
use crate::motion::{self, SpringColor, SpringPoint, SpringRect, SpringScalar, SpringSurfaceView};
use crate::swift_compat::{smax, smin};
use crate::theme::style_sheet::StyleSheet;
use crate::view::density_gutter_preview_window::DensityGutterPreviewWindow;
use crate::view::density_outline_window::{DensityOutlineEntry, DensityOutlineWindow};
use crate::view::markdown_container_view::MarkdownContainerView;
use crate::view::style_sheet_defaults::PanelAlpha;
use crate::view::tracking_area::refresh_tracking_area;

/// `DensityGutterDelegate`.
pub trait DensityGutterDelegate {
    fn density_gutter_did_request_scroll_to_fraction(&self, gutter: &DensityGutterView, fraction: CGFloat);
    /// Return a semantic section preview and useful jump context.
    fn density_gutter_preview_at_fraction(
        &self,
        gutter: &DensityGutterView,
        fraction: CGFloat,
    ) -> Option<(String, String, String)>;
}

/// `DensityBand.Kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DensityBandKind {
    Heading { level: isize },
    CodeBlock,
    Table,
    Math,
    TaskList,
    Change(ChangeKind),
    SearchHit,
    Image,
    Callout,
}

/// `DensityBand`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DensityBand {
    pub kind: DensityBandKind,
    /// 0…1 through the document.
    pub start_fraction: CGFloat,
    pub end_fraction: CGFloat,
}

impl DensityBand {
    pub fn new(kind: DensityBandKind, start_fraction: CGFloat, end_fraction: CGFloat) -> DensityBand {
        DensityBand { kind, start_fraction, end_fraction }
    }
}

// MARK: - Simulations

/// One mark's complete visual state — geometry, tint and glow — all
/// integrated from the same clock (`MarkSimulation`).
#[derive(Debug, Clone, Copy)]
struct MarkSimulation {
    frame: SpringRect,
    color: SpringColor,
    glow: SpringScalar,
    /// Whether the next event asked this mark to settle like the current
    /// heading; flips the pace between pointer-quick and structural.
    wants_settle: bool,
}

impl MarkSimulation {
    fn new(frame: CGRect, color: &CGColor, glow: CGFloat) -> MarkSimulation {
        MarkSimulation {
            frame: SpringRect::new(frame, motion::SPRING_QUICK, 0.0),
            color: SpringColor::new(&foreground(color), motion::SPRING_QUICK, 0.0),
            glow: SpringScalar::with_value(glow, motion::SPRING_QUICK),
            wants_settle: false,
        }
    }

    fn retarget(&mut self, frame: CGRect, color: &CGColor, glow: CGFloat, settle: bool) {
        if settle != self.wants_settle {
            self.wants_settle = settle;
            let pace = if settle { motion::SPRING_STANDARD } else { motion::SPRING_QUICK };
            self.frame.retune(pace, None);
            self.color.retune(pace);
            self.glow.retune(pace, None);
        }
        self.frame.target(frame);
        self.color.target(&foreground(color));
        self.glow.target(glow);
    }

    fn snap(&mut self, frame: CGRect, color: &CGColor, glow: CGFloat) {
        self.frame.snap(frame);
        self.color.snap(&foreground(color));
        self.glow.snap(glow);
        if !self.wants_settle {
            return;
        }
        self.wants_settle = false;
        self.frame.retune(motion::SPRING_QUICK, None);
        self.color.retune(motion::SPRING_QUICK);
        self.glow.retune(motion::SPRING_QUICK, None);
    }

    fn advance(&mut self, dt: CGFloat) -> bool {
        let mut moving = false;
        moving = self.frame.advance(dt) || moving;
        moving = self.color.advance(dt) || moving;
        moving = self.glow.advance(dt) || moving;
        moving
    }

    fn view_frame(&self) -> CGRect {
        let size = self.frame.size.value();
        let centre = self.frame.centre.value();
        rect(centre.x - size.width / 2.0, centre.y - size.height / 2.0, size.width, size.height)
    }
}

/// `MarkSimulation.foreground(of:)`: `CGColor.components` are in the
/// *source* colour space, so normalise to sRGB first.
fn foreground(color: &CGColor) -> Retained<NSColor> {
    let normalized = NSColor::colorWithCGColor(color).and_then(|color| color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()));
    let components = normalized.map(|color| {
        let cg = color.CGColor();
        let count = CGColor::number_of_components(Some(&cg));
        let pointer = CGColor::components(Some(&cg));
        // SAFETY: CoreGraphics returns `count` components for the colour.
        let slice: Vec<CGFloat> =
            if pointer.is_null() { Vec::new() } else { unsafe { std::slice::from_raw_parts(pointer, count) }.to_vec() };
        slice
    });
    let Some(components) = components.filter(|components| components.len() >= 3) else {
        return NSColor::colorWithSRGBRed_green_blue_alpha(0.0, 0.0, 0.0, 1.0);
    };
    NSColor::colorWithSRGBRed_green_blue_alpha(
        components[0],
        components[1],
        components[2],
        if components.len() >= 4 { components[3] } else { 1.0 },
    )
}

/// A review pip — the small dots on a mark's leading edge
/// (`DensityGutterView.PipSimulation`). Public so the rail's tests can drive
/// the cascade a frame at a time.
#[derive(Debug, Clone, Copy)]
pub struct PipSimulation {
    pub centre: SpringPoint,
    pub diameter: SpringScalar,
    pub color: SpringColor,
    /// Seconds still to wait before this pip joins the cascade, counted
    /// down by `advance`.
    pub delay_remaining: CGFloat,
    pub engaged: bool,
}

impl PipSimulation {
    pub fn new(centre: CGPoint, diameter: CGFloat, color: &CGColor) -> PipSimulation {
        let quick = motion::SPRING_QUICK;
        PipSimulation {
            centre: SpringPoint::new(centre, quick, 0.0),
            diameter: SpringScalar::with_value(diameter, quick),
            // The pip's alpha is its cascade: it starts invisible.
            color: SpringColor::new(&foreground(color).colorWithAlphaComponent(0.0), quick, 0.0),
            delay_remaining: 0.0,
            engaged: false,
        }
    }

    pub fn retarget(&mut self, centre: CGPoint, diameter: CGFloat, color: &CGColor, delay: CGFloat, release_now: bool) {
        self.centre.target(centre);
        self.diameter.target(diameter);
        self.color.target(&foreground(color));
        if self.engaged {
            return;
        }
        if release_now || delay <= 0.0 {
            self.engage();
        } else {
            self.delay_remaining = delay;
        }
    }

    pub fn snap(&mut self, centre: CGPoint, diameter: CGFloat, color: &CGColor) {
        self.centre.snap(centre);
        self.diameter.snap(diameter);
        self.color.snap(&foreground(color));
        self.delay_remaining = 0.0;
        self.engaged = true;
    }

    /// Join the cascade: the alpha spring takes a velocity kick so the pip
    /// swells into place rather than merely appearing.
    fn engage(&mut self) {
        self.engaged = true;
        self.delay_remaining = 0.0;
        self.color.kick_alpha(18.0);
    }

    pub fn advance(&mut self, dt: CGFloat) -> bool {
        if !self.engaged {
            self.delay_remaining -= dt;
            if !(self.delay_remaining <= 0.0) {
                return true;
            }
            self.engage();
        }
        let mut moving = false;
        moving = self.centre.advance(dt) || moving;
        moving = self.diameter.advance(dt) || moving;
        moving = self.color.advance(dt) || moving;
        moving
    }

    pub fn position(&self) -> CGPoint {
        self.centre.value()
    }

    pub fn current_color(&self) -> Retained<NSColor> {
        self.color.value()
    }
}

// MARK: - Stack model

/// One drawn mark: a heading, plus the review overlays that fall inside its
/// section (`ResolvedMark`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedMark {
    pub band: DensityBand,
    pub y: CGFloat,
    pub pip: Pip,
}

/// `DensityGutterView.Pip`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pip {
    pub change: Option<ChangeKind>,
    pub search_hit: bool,
}

impl Pip {
    pub fn is_empty(&self) -> bool {
        self.change.is_none() && !self.search_hit
    }
}

/// Which bands the rail draws, and what hangs off each of them
/// (`DensityGutterView.Selection`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Selection {
    pub marks: Vec<DensityBand>,
    pub pips: Vec<Pip>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SelectionKey {
    revision: isize,
    capacity: isize,
}

struct BandStyle {
    width_points: CGFloat,
    min_height: CGFloat,
    color: Retained<NSColor>,
}

// MARK: - The view

pub struct DensityGutterViewIvars {
    delegate: RefCell<Option<Weak<dyn DensityGutterDelegate>>>,
    style_sheet: RefCell<Rc<StyleSheet>>,
    bands: RefCell<Rc<Vec<DensityBand>>>,
    shows_overlay_pips: Cell<bool>,
    outline_entries: RefCell<Vec<DensityOutlineEntry>>,
    visible_range: Cell<(CGFloat, CGFloat)>,
    read_progress: Cell<CGFloat>,
    metrics_summary: RefCell<String>,
    allows_preview_content_overlap: Cell<bool>,

    preview: Retained<DensityGutterPreviewWindow>,
    outline: Retained<DensityOutlineWindow>,
    is_scrubbing: Cell<bool>,
    tracking_area: RefCell<Option<Retained<NSTrackingArea>>>,
    preview_work_item: RefCell<Option<WorkItem>>,
    preview_hide_work_item: RefCell<Option<WorkItem>>,
    outline_work_item: RefCell<Option<WorkItem>>,
    outline_hide_work_item: RefCell<Option<WorkItem>>,
    pointer_location: Cell<Option<NSPoint>>,
    pointer_is_in_preview: Cell<bool>,
    pointer_is_in_outline: Cell<bool>,
    last_pointer_sample: Cell<Option<(CGFloat, f64)>>,
    pointer_velocity_y: Cell<CGFloat>,
    /// When the rail last tapped, so `DETENT_INTERVAL` is enforced across
    /// both the things that ask for one.
    last_haptic_time: Cell<f64>,
    /// Swapped in tests to count taps; production always reaches the performer.
    perform_haptic_feedback: RefCell<Rc<dyn Fn()>>,
    previous_current_fraction: Cell<Option<CGFloat>>,

    breathe_spring: Cell<SpringScalar>,
    mark_simulations: RefCell<Vec<MarkSimulation>>,
    pip_simulations: RefCell<Vec<PipSimulation>>,

    /// Bumped on every `bands` assignment so the selection cache can be keyed
    /// without comparing the array itself.
    bands_revision: Cell<isize>,
    cached_selection: RefCell<Option<Rc<Selection>>>,
    cached_selection_key: Cell<Option<SelectionKey>>,

    mark_layers: RefCell<Vec<Retained<CALayer>>>,
    pip_layers: RefCell<Vec<Retained<CALayer>>>,
    hovered_band_index: Cell<Option<isize>>,
    did_drag: Cell<bool>,
    mouse_down_location: Cell<Option<NSPoint>>,
}

define_class!(
    // SAFETY: `initWithFrame:` is forwarded to `SpringSurfaceView` in `new`
    // after the ivars are set; overrides keep AppKit's signatures. Drop is
    // on the ivars only.
    #[unsafe(super(SpringSurfaceView, NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "DensityGutterView"]
    #[ivars = DensityGutterViewIvars]
    pub struct DensityGutterView;

    unsafe impl NSObjectProtocol for DensityGutterView {}

    impl DensityGutterView {
        /// Fractions run top-to-bottom through the document, so the view does too.
        #[unsafe(method(isFlipped))]
        fn __is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(intrinsicContentSize))]
        fn __intrinsic_content_size(&self) -> NSSize {
            // SAFETY: AppKit exports the metric as an immutable global.
            NSSize::new(Self::WIDTH, unsafe { NSViewNoIntrinsicMetric })
        }

        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn __view_did_change_effective_appearance(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidChangeEffectiveAppearance] };
            self.update_mark_layers(false);
        }

        #[unsafe(method(springTick:))]
        fn __spring_tick(&self, dt: CGFloat) -> bool {
            let ivars = self.ivars();
            let mut breathe = ivars.breathe_spring.get();
            let mut moving = breathe.advance(dt);
            ivars.breathe_spring.set(breathe);
            for simulation in ivars.mark_simulations.borrow_mut().iter_mut() {
                moving = simulation.advance(dt) || moving;
            }
            for simulation in ivars.pip_simulations.borrow_mut().iter_mut() {
                moving = simulation.advance(dt) || moving;
            }
            moving
        }

        #[unsafe(method(springApply))]
        fn __spring_apply(&self) {
            self.apply_simulations();
        }

        /// Live resize: rebuilding unanimated puts each mark on its new seat
        /// immediately.
        #[unsafe(method(springsSettleImmediately))]
        fn __springs_settle_immediately(&self) {
            self.update_mark_layers(false);
        }

        #[unsafe(method(layout))]
        fn __layout(&self) {
            let _: () = unsafe { msg_send![super(self), layout] };
            self.update_mark_layers(false);
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn __accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(resetCursorRects))]
        fn __reset_cursor_rects(&self) {
            self.addCursorRect_cursor(self.bounds(), &NSCursor::pointingHandCursor());
        }

        #[unsafe(method(updateTrackingAreas))]
        fn __update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            refresh_tracking_area(
                self,
                &self.ivars().tracking_area,
                NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::MouseMoved
                    | NSTrackingAreaOptions::ActiveInKeyWindow
                    | NSTrackingAreaOptions::InVisibleRect,
            );
        }

        #[unsafe(method(mouseDown:))]
        fn __mouse_down(&self, event: &NSEvent) {
            self.mouse_down(event);
        }

        #[unsafe(method(mouseDragged:))]
        fn __mouse_dragged(&self, event: &NSEvent) {
            self.mouse_dragged(event);
        }

        #[unsafe(method(mouseUp:))]
        fn __mouse_up(&self, event: &NSEvent) {
            self.mouse_up(event);
        }

        #[unsafe(method(mouseMoved:))]
        fn __mouse_moved(&self, event: &NSEvent) {
            self.mouse_moved(event);
        }

        #[unsafe(method(mouseEntered:))]
        fn __mouse_entered(&self, event: &NSEvent) {
            self.mouse_entered(event);
        }

        #[unsafe(method(mouseExited:))]
        fn __mouse_exited(&self, event: &NSEvent) {
            self.mouse_exited(event);
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn __view_did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            self.view_did_move_to_window();
        }
    }
);

impl DensityGutterView {
    /// Rail width.
    pub const WIDTH: CGFloat = 72.0;

    /// Scrub preview appears almost immediately; outline bloom waits longer.
    pub const HOVER_DWELL: f64 = 0.02;
    /// Passive hover may begin before the pointer reaches a mark, but never
    /// from an arbitrary point in the rail.
    pub const HOVER_ACTIVATION_SLOP: CGFloat = 22.0;
    /// Once a mark owns the hover, leaving its row dismisses chrome.
    pub const HOVER_DISMISSAL_SLOP: CGFloat = 4.0;
    /// Small bridge only for the physical gap between a mark and its preview.
    pub const PREVIEW_EXIT_DELAY: f64 = 0.06;
    /// Floor between two taps of the Taptic engine.
    pub const DETENT_INTERVAL: f64 = 0.05;
    /// Distance over which mark size / brightness fall off from the pointer.
    pub const PROXIMITY_RADIUS: CGFloat = 36.0;
    /// Maximum magnetic pull of a mark toward the pointer (points).
    pub const MAGNETIC_PULL: CGFloat = 1.5;
    /// Extra magnetic pull scaled by scrub velocity (points at full influence).
    pub const SCRUB_VELOCITY_PULL: CGFloat = 2.0;
    /// Pointer speed (pt/s) that saturates velocity pull.
    pub const SCRUB_VELOCITY_SCALE: CGFloat = 900.0;
    /// Maximum fractional compression of the centred stack near the pointer.
    pub const STACK_COMPRESSION: CGFloat = 0.08;
    /// Whole-rail width scale while the pointer is inside.
    pub const BREATHE_SCALE: CGFloat = 1.08;
    /// Alpha multiplier for marks outside the hovered neighbourhood.
    pub const NEIGHBORHOOD_DIM: CGFloat = 0.82;
    /// Marks within this index distance stay slightly lifted under hover.
    pub const NEIGHBORHOOD_LIFT_RADIUS: isize = 2;

    /// Closest two marks may sit.
    pub const MIN_PITCH: CGFloat = 7.0;
    /// Furthest two marks may sit.
    pub const MAX_PITCH: CGFloat = 11.0;
    /// Share of the track the cluster may span.
    pub const MAX_SPAN_FRACTION: CGFloat = 0.5;
    /// Upper bound on marks however tall the window is.
    pub const STACK_CAPACITY_CEILING: isize = 18;
    /// Below this the rail itself is too short to hold a readable stack.
    pub const MINIMUM_STACK_MARKS: isize = 3;

    /// Overlay dot on a mark's leading edge (§2.3 "coloured pips").
    pub const PIP_DIAMETER: CGFloat = 3.5;
    /// Gap from the resting mark's leading edge to the first pip.
    pub const PIP_LEADING_GAP: CGFloat = 5.0;

    /// Optical boost on click / scrub release (points of extra width).
    pub const JUMP_PUNCH_BOOST: CGFloat = 4.0;
    /// Current-mark glow (same hue, very low opacity).
    pub const CURRENT_GLOW_RADIUS: CGFloat = 10.0;
    pub const CURRENT_GLOW_OPACITY: f32 = 0.12;

    /// Marks sit in a generous invisible hit lane.
    const HORIZONTAL_MARGIN: CGFloat = 16.0;
    const TRACK_INSET: CGFloat = 28.0;
    const SCRUB_ACTIVATION_DISTANCE: CGFloat = 4.0;

    pub fn should_begin_scrub(start: NSPoint, current: NSPoint) -> bool {
        (current.x - start.x).hypot(current.y - start.y) >= Self::SCRUB_ACTIVATION_DISTANCE
    }

    // MARK: - Init

    /// `init()`: hosts build panels before they have a theme in hand and
    /// assign `styleSheet` immediately afterwards.
    pub fn new_current(mtm: MainThreadMarker) -> Retained<DensityGutterView> {
        Self::new(Rc::new(StyleSheet::current(mtm)), mtm)
    }

    /// `init(styleSheet:)`.
    pub fn new(style_sheet: Rc<StyleSheet>, mtm: MainThreadMarker) -> Retained<DensityGutterView> {
        let preview = DensityGutterPreviewWindow::new(style_sheet.clone(), mtm);
        let outline = DensityOutlineWindow::new(style_sheet.clone(), mtm);
        let this = Self::alloc(mtm).set_ivars(DensityGutterViewIvars {
            delegate: RefCell::new(None),
            style_sheet: RefCell::new(style_sheet),
            bands: RefCell::new(Rc::new(Vec::new())),
            shows_overlay_pips: Cell::new(false),
            outline_entries: RefCell::new(Vec::new()),
            visible_range: Cell::new((0.0, 1.0)),
            read_progress: Cell::new(0.0),
            metrics_summary: RefCell::new(String::new()),
            allows_preview_content_overlap: Cell::new(false),
            preview,
            outline,
            is_scrubbing: Cell::new(false),
            tracking_area: RefCell::new(None),
            preview_work_item: RefCell::new(None),
            preview_hide_work_item: RefCell::new(None),
            outline_work_item: RefCell::new(None),
            outline_hide_work_item: RefCell::new(None),
            pointer_location: Cell::new(None),
            pointer_is_in_preview: Cell::new(false),
            pointer_is_in_outline: Cell::new(false),
            last_pointer_sample: Cell::new(None),
            pointer_velocity_y: Cell::new(0.0),
            last_haptic_time: Cell::new(-f64::MAX),
            perform_haptic_feedback: RefCell::new(Rc::new(|| {
                let performer = NSHapticFeedbackManager::defaultPerformer();
                performer.performFeedbackPattern_performanceTime(
                    NSHapticFeedbackPattern::Alignment,
                    NSHapticFeedbackPerformanceTime::Now,
                );
            })),
            previous_current_fraction: Cell::new(None),
            breathe_spring: Cell::new(SpringScalar::with_value(1.0, motion::SPRING_QUICK)),
            mark_simulations: RefCell::new(Vec::new()),
            pip_simulations: RefCell::new(Vec::new()),
            bands_revision: Cell::new(0),
            cached_selection: RefCell::new(None),
            cached_selection_key: Cell::new(None),
            mark_layers: RefCell::new(Vec::new()),
            pip_layers: RefCell::new(Vec::new()),
            hovered_band_index: Cell::new(None),
            did_drag: Cell::new(false),
            mouse_down_location: Cell::new(None),
        });
        let this: Retained<DensityGutterView> =
            unsafe { msg_send![super(this), initWithFrame: rect(0.0, 0.0, Self::WIDTH, 100.0)] };

        this.setWantsLayer(true);

        let weak: ObjcWeak<DensityGutterView> = ObjcWeak::from(&*this);
        this.ivars().preview.set_on_pointer_presence(Some(Box::new(move |is_inside| {
            let Some(this) = weak.load() else { return };
            this.ivars().pointer_is_in_preview.set(is_inside);
            if is_inside {
                this.cancel_preview_hide();
                this.ivars().preview.cancel_hide_animation();
            } else if !this.ivars().is_scrubbing.get() {
                this.schedule_preview_hide();
            }
        })));

        let weak: ObjcWeak<DensityGutterView> = ObjcWeak::from(&*this);
        this.ivars().outline.set_on_select(Some(Box::new(move |fraction| {
            let Some(this) = weak.load() else { return };
            if let Some(delegate) = this.delegate() {
                delegate.density_gutter_did_request_scroll_to_fraction(&this, fraction);
            }
        })));
        let weak: ObjcWeak<DensityGutterView> = ObjcWeak::from(&*this);
        this.ivars().outline.set_on_pointer_presence(Some(Box::new(move |is_inside| {
            let Some(this) = weak.load() else { return };
            this.ivars().pointer_is_in_outline.set(is_inside);
            if is_inside {
                this.cancel_outline_hide();
                this.ivars().outline.cancel_dismiss_animation();
            } else if !this.ivars().is_scrubbing.get() {
                this.schedule_outline_hide();
            }
        })));

        // Fully custom-drawn with no accessible subviews, so it has to declare
        // itself an element or VoiceOver never sees it (§11.4).
        this.setAccessibilityElement(true);
        // SAFETY: AppKit exports the role as an immutable global.
        this.setAccessibilityRole(Some(unsafe { objc2_app_kit::NSAccessibilityScrollBarRole }));
        this.setAccessibilityLabel(Some(&NSString::from_str("Document map")));
        this.setAccessibilityValueDescription(Some(&NSString::from_str("0 percent read")));
        let weak: ObjcWeak<DensityGutterView> = ObjcWeak::from(&*this);
        let show_outline = RcBlock::new(move || -> Bool {
            let Some(this) = weak.load() else { return Bool::NO };
            if this.window().is_none() || this.ivars().outline_entries.borrow().is_empty() {
                return Bool::NO;
            }
            this.present_outline_for_keyboard();
            Bool::YES
        });
        let actions = NSArray::from_retained_slice(&[NSAccessibilityCustomAction::initWithName_handler(
            NSAccessibilityCustomAction::alloc(),
            &NSString::from_str("Show document outline"),
            Some(&show_outline),
        )]);
        this.setAccessibilityCustomActions(Some(&actions));
        this.update_mark_layers(false);
        this
    }

    // MARK: - Properties

    pub fn delegate(&self) -> Option<Rc<dyn DensityGutterDelegate>> {
        self.ivars().delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn set_delegate(&self, delegate: Option<Weak<dyn DensityGutterDelegate>>) {
        *self.ivars().delegate.borrow_mut() = delegate;
    }

    pub fn style_sheet(&self) -> Rc<StyleSheet> {
        self.ivars().style_sheet.borrow().clone()
    }

    pub fn set_style_sheet(&self, style_sheet: Rc<StyleSheet>) {
        *self.ivars().style_sheet.borrow_mut() = style_sheet.clone();
        self.ivars().preview.set_style_sheet(style_sheet.clone());
        self.ivars().outline.set_style_sheet(style_sheet);
        self.update_mark_layers(false);
    }

    pub fn bands(&self) -> Rc<Vec<DensityBand>> {
        self.ivars().bands.borrow().clone()
    }

    pub fn set_bands(&self, bands: Vec<DensityBand>) {
        let ivars = self.ivars();
        *ivars.bands.borrow_mut() = Rc::new(bands);
        ivars.hovered_band_index.set(None);
        // The stack is derived from these, and the current mark is derived
        // from the stack — both caches are stale the moment bands change.
        ivars.bands_revision.set(ivars.bands_revision.get().wrapping_add(1));
        ivars.previous_current_fraction.set(None);
        self.update_mark_layers(false);
    }

    pub fn shows_overlay_pips(&self) -> bool {
        self.ivars().shows_overlay_pips.get()
    }

    pub fn set_shows_overlay_pips(&self, shows: bool) {
        let ivars = self.ivars();
        let old_value = ivars.shows_overlay_pips.replace(shows);
        if shows == old_value {
            return;
        }
        ivars.bands_revision.set(ivars.bands_revision.get().wrapping_add(1));
        *ivars.cached_selection.borrow_mut() = None;
        ivars.cached_selection_key.set(None);
        self.update_mark_layers(false);
    }

    pub fn outline_entries(&self) -> Vec<DensityOutlineEntry> {
        self.ivars().outline_entries.borrow().clone()
    }

    pub fn set_outline_entries(&self, entries: Vec<DensityOutlineEntry>) {
        *self.ivars().outline_entries.borrow_mut() = entries.clone();
        self.ivars().outline.set_entries(entries);
    }

    /// Visible viewport, as `lower…upper` fractions.
    pub fn visible_range(&self) -> (CGFloat, CGFloat) {
        self.ivars().visible_range.get()
    }

    pub fn set_visible_range(&self, range: (CGFloat, CGFloat)) {
        let old_value = self.ivars().visible_range.replace(range);
        // Hosts drive this from a scroll observer, which fires far more
        // often than the value actually moves.
        if range == old_value {
            return;
        }
        // Only animate if the *current* heading changed.
        let current_now = self.current_heading_fraction();
        let current_changed = current_now != self.ivars().previous_current_fraction.get();
        self.update_mark_layers(current_changed);
    }

    pub fn read_progress(&self) -> CGFloat {
        self.ivars().read_progress.get()
    }

    pub fn set_read_progress(&self, progress: CGFloat) {
        let old_value = self.ivars().read_progress.replace(progress);
        if progress == old_value {
            return;
        }
        let percent = (progress * 100.0).round() as isize;
        self.setAccessibilityValueDescription(Some(&NSString::from_str(&format!("{percent} percent read"))));
        self.update_mark_layers(false);
    }

    /// Document word count, character count and read time (§9.6), shown as
    /// a footer line in the hover tooltip.
    pub fn metrics_summary(&self) -> String {
        self.ivars().metrics_summary.borrow().clone()
    }

    pub fn set_metrics_summary(&self, summary: String) {
        *self.ivars().metrics_summary.borrow_mut() = summary;
    }

    pub fn preferred_width(&self) -> CGFloat {
        Self::WIDTH
    }

    pub fn allows_preview_content_overlap(&self) -> bool {
        self.ivars().allows_preview_content_overlap.get()
    }

    pub fn set_allows_preview_content_overlap(&self, allows: bool) {
        self.ivars().allows_preview_content_overlap.set(allows);
    }

    pub fn is_scrubbing(&self) -> bool {
        self.ivars().is_scrubbing.get()
    }

    /// Swaps the haptic performer (`performHapticFeedback`), for tests.
    pub fn set_perform_haptic_feedback(&self, perform: Rc<dyn Fn()>) {
        *self.ivars().perform_haptic_feedback.borrow_mut() = perform;
    }

    /// The preview window, for hosts and harnesses that inspect it.
    pub fn preview_window(&self) -> &Retained<DensityGutterPreviewWindow> {
        &self.ivars().preview
    }

    /// The outline window, for hosts and harnesses that inspect it.
    pub fn outline_window(&self) -> &Retained<DensityOutlineWindow> {
        &self.ivars().outline
    }

    // MARK: - Stack model

    fn current_heading_fraction(&self) -> Option<CGFloat> {
        // Derived from the *drawn* marks rather than from every heading.
        let selection = self.selection(self.bounds().height());
        Self::current_heading_fraction_in(&selection.marks, self.visible_range())
    }

    /// `currentHeadingFraction(in:at:)`.
    pub fn current_heading_fraction_in(bands: &[DensityBand], visible_range: (CGFloat, CGFloat)) -> Option<CGFloat> {
        let headings: Vec<&DensityBand> =
            bands.iter().filter(|band| matches!(band.kind, DensityBandKind::Heading { .. })).collect();
        headings
            .iter()
            .rev()
            .find(|band| band.start_fraction <= visible_range.0)
            .map(|band| band.start_fraction)
            .or_else(|| headings.first().map(|band| band.start_fraction))
    }

    /// The usable span of the rail, inset from both ends.
    pub fn track_range(height: CGFloat, track_inset: CGFloat) -> (CGFloat, CGFloat) {
        let top = smin(track_inset, smax(0.0, height / 2.0));
        (top, smax(top, height - track_inset))
    }

    /// How many marks this track can hold at a legible pitch.
    pub fn stack_capacity(track: CGFloat) -> isize {
        if !(track > 0.0) {
            return 0;
        }
        let fit = (track * Self::MAX_SPAN_FRACTION / Self::MIN_PITCH).floor() as isize + 1;
        0isize.max(Self::STACK_CAPACITY_CEILING.min(fit))
    }

    /// Spread `count` marks across the allowed share of the track, then clamp.
    pub fn mark_pitch(track: CGFloat, count: isize) -> CGFloat {
        if !(count > 1) {
            return Self::MAX_PITCH;
        }
        let ideal = track * Self::MAX_SPAN_FRACTION / (count - 1) as CGFloat;
        smin(Self::MAX_PITCH, smax(Self::MIN_PITCH, ideal))
    }

    /// `centeredBandYPositions(height:count:trackInset:markGap:pointerY:compression:proximityRadius:)`.
    #[allow(clippy::too_many_arguments)]
    pub fn centered_band_y_positions(
        height: CGFloat,
        count: isize,
        track_inset: CGFloat,
        mark_gap: CGFloat,
        pointer_y: Option<CGFloat>,
        compression: CGFloat,
        proximity_radius: CGFloat,
    ) -> Vec<CGFloat> {
        if !(count > 0) {
            return Vec::new();
        }

        let track = Self::track_range(height, track_inset);
        let track_height = smax(1.0, track.1 - track.0);
        let gap = smin(mark_gap, track_height / 1isize.max(count - 1) as CGFloat);
        let group_height = 0isize.max(count - 1) as CGFloat * gap;
        let start = track.0 + smax(0.0, (track_height - group_height) / 2.0);
        let base: Vec<CGFloat> = (0..count).map(|index| start + index as CGFloat * gap).collect();

        let Some(pointer_y) = pointer_y else { return base };
        if !(count > 1) || !(compression > 0.0) {
            return base;
        }

        // Soft Dock-like compression: gaps near the pointer shrink a little.
        let mut compressed: Vec<CGFloat> = vec![base[0]];
        for index in 1..count as usize {
            let mid = (base[index - 1] + base[index]) / 2.0;
            let influence = Self::proximity_influence((mid - pointer_y).abs(), proximity_radius);
            let local_gap = gap * (1.0 - compression * influence);
            compressed.push(compressed[index - 1] + local_gap);
        }
        let compressed_span = compressed.last().copied().unwrap_or(0.0) - compressed[0];
        let recenter = start + smax(0.0, (group_height - compressed_span) / 2.0) - compressed[0];
        compressed.iter().map(|value| value + recenter).collect()
    }

    /// `centeredBandYPositions` with its defaults (inset 28, `maxPitch`, no
    /// pointer).
    pub fn centered_band_y_positions_default(height: CGFloat, count: isize) -> Vec<CGFloat> {
        Self::centered_band_y_positions(
            height,
            count,
            28.0,
            Self::MAX_PITCH,
            None,
            Self::STACK_COMPRESSION,
            Self::PROXIMITY_RADIUS,
        )
    }

    /// Ease-out falloff in 0…1 for pointer distance.
    pub fn proximity_influence(distance: CGFloat, radius: CGFloat) -> CGFloat {
        if !(radius > 0.0) {
            return if distance <= 0.0 { 1.0 } else { 0.0 };
        }
        let t = smin(1.0, smax(0.0, 1.0 - distance / radius));
        t * t * (3.0 - 2.0 * t) // smoothstep
    }

    /// Every resting heading uses one length.
    pub fn heading_mark_width(level: isize, emphasized: bool) -> CGFloat {
        let _ = level;
        if emphasized { 32.0 } else { 26.0 }
    }

    /// Cached because pointer movement must not re-derive it.
    fn selection(&self, height: CGFloat) -> Rc<Selection> {
        let ivars = self.ivars();
        let track = Self::track_range(height, Self::TRACK_INSET);
        let capacity = Self::stack_capacity(track.1 - track.0);
        let key = SelectionKey { revision: ivars.bands_revision.get(), capacity };
        if Some(key) == ivars.cached_selection_key.get()
            && let Some(cached) = ivars.cached_selection.borrow().as_ref()
        {
            return cached.clone();
        }
        let bands = self.bands();
        let resolved = Rc::new(Self::selection_for(&bands, capacity, ivars.shows_overlay_pips.get()));
        *ivars.cached_selection.borrow_mut() = Some(resolved.clone());
        ivars.cached_selection_key.set(Some(key));
        resolved
    }

    /// `selection(for:capacity:includeOverlays:)`.
    pub fn selection_for(bands: &[DensityBand], capacity: isize, include_overlays: bool) -> Selection {
        let mut headings: Vec<DensityBand> =
            bands.iter().filter(|band| matches!(band.kind, DensityBandKind::Heading { .. })).copied().collect();
        sort_by_start(&mut headings);
        let overlays: Vec<DensityBand> = if include_overlays {
            let mut overlays: Vec<DensityBand> = bands.iter().filter(|band| Self::is_overlay(band.kind)).copied().collect();
            sort_by_start(&mut overlays);
            overlays
        } else {
            Vec::new()
        };

        // A document with no headings has no sections to index, so the review
        // overlays become the stack rather than vanishing along with it.
        if headings.is_empty() {
            let marks = Self::stride_sampled(&overlays, capacity);
            if !(capacity >= Self::MINIMUM_STACK_MARKS) {
                return Selection::default();
            }
            let count = marks.len();
            return Selection { marks, pips: vec![Pip::default(); count] };
        }

        let marks = Self::select_headings(&headings, capacity);
        if !(capacity >= Self::MINIMUM_STACK_MARKS) {
            return Selection::default();
        }
        let pips = Self::pips(&overlays, &marks);
        Selection { marks, pips }
    }

    /// Thins by *depth* before it thins by position.
    pub fn select_headings(headings: &[DensityBand], capacity: isize) -> Vec<DensityBand> {
        if !(capacity > 0) {
            return Vec::new();
        }
        if !(headings.len() as isize > capacity) {
            return headings.to_vec();
        }

        let mut present: Vec<isize> = headings.iter().map(heading_level).collect();
        present.sort_unstable();
        present.dedup();
        let mut chosen: Vec<DensityBand> = Vec::new();
        let mut fill_index = present.len();
        for (index, depth) in present.iter().enumerate() {
            let candidates: Vec<DensityBand> =
                headings.iter().filter(|band| heading_level(band) <= *depth).copied().collect();
            if candidates.len() as isize > capacity {
                fill_index = index;
                break;
            }
            chosen = candidates;
        }

        // Spare slots go to the next depth down, evenly sampled.
        let remaining = capacity - chosen.len() as isize;
        if !(remaining > 0) || !(fill_index < present.len()) {
            return chosen;
        }
        let fill_level = present[fill_index];
        let level_bands: Vec<DensityBand> =
            headings.iter().filter(|band| heading_level(band) == fill_level).copied().collect();
        let picked = Self::stride_sampled(&level_bands, remaining);
        let mut result = chosen;
        result.extend(picked);
        sort_by_start(&mut result);
        result
    }

    /// Evenly thins to `limit`, always keeping the first and last.
    pub fn stride_sampled(bands: &[DensityBand], limit: isize) -> Vec<DensityBand> {
        if !(limit > 0) {
            return Vec::new();
        }
        if !(bands.len() as isize > limit) {
            return bands.to_vec();
        }
        if !(limit > 1) {
            return vec![bands[0]];
        }
        let last = bands.len() as isize - 1;
        let step = last as f64 / (limit - 1) as f64;
        let mut picked: Vec<DensityBand> = Vec::with_capacity(limit as usize);
        let mut previous_index: isize = -1;
        for slot in 0..limit {
            let index = last.min((slot as f64 * step).round() as isize);
            if index == previous_index {
                continue;
            }
            picked.push(bands[index as usize]);
            previous_index = index;
        }
        picked
    }

    /// An overlay belongs to the section it falls in, so it attaches to the
    /// last mark at or before it.
    pub fn pips(overlays: &[DensityBand], marks: &[DensityBand]) -> Vec<Pip> {
        let mut result = vec![Pip::default(); marks.len()];
        if marks.is_empty() {
            return result;
        }
        let fractions: Vec<CGFloat> = marks.iter().map(|band| band.start_fraction).collect();
        for overlay in overlays {
            let index = Self::section_index(overlay.start_fraction, &fractions);
            match overlay.kind {
                DensityBandKind::SearchHit => result[index].search_hit = true,
                DensityBandKind::Change(kind) => {
                    // Mixed kinds in one section report as "changed".
                    result[index].change = Some(match result[index].change {
                        Some(existing) => {
                            if existing == kind {
                                kind
                            } else {
                                ChangeKind::Modified
                            }
                        }
                        None => kind,
                    });
                }
                _ => {}
            }
        }
        result
    }

    /// Last index whose fraction is at or before `fraction`, or 0 for anything
    /// above the first mark.
    pub fn section_index(fraction: CGFloat, fractions: &[CGFloat]) -> usize {
        let mut low: isize = 0;
        let mut high: isize = fractions.len() as isize - 1;
        let mut result: isize = 0;
        while low <= high {
            let mid = (low + high) / 2;
            if fractions[mid as usize] <= fraction {
                result = mid;
                low = mid + 1;
            } else {
                high = mid - 1;
            }
        }
        result as usize
    }

    /// Headings become marks; changes and search hits become pips on them.
    pub fn is_overlay(kind: DensityBandKind) -> bool {
        match kind {
            DensityBandKind::Change(_) | DensityBandKind::SearchHit => true,
            DensityBandKind::Heading { .. }
            | DensityBandKind::CodeBlock
            | DensityBandKind::Table
            | DensityBandKind::Math
            | DensityBandKind::TaskList
            | DensityBandKind::Image
            | DensityBandKind::Callout => false,
        }
    }

    fn resolved_stack(&self, height: CGFloat) -> Vec<ResolvedMark> {
        let selected = self.selection(height);
        if selected.marks.is_empty() {
            return Vec::new();
        }
        let track = Self::track_range(height, Self::TRACK_INSET);
        let positions = Self::centered_band_y_positions(
            height,
            selected.marks.len() as isize,
            Self::TRACK_INSET,
            Self::mark_pitch(track.1 - track.0, selected.marks.len() as isize),
            self.ivars().pointer_location.get().map(|point| point.y),
            Self::STACK_COMPRESSION,
            Self::PROXIMITY_RADIUS,
        );
        selected
            .marks
            .iter()
            .enumerate()
            .map(|(index, band)| ResolvedMark { band: *band, y: positions[index], pip: selected.pips[index] })
            .collect()
    }

    fn style(&self, style_sheet: &StyleSheet, kind: DensityBandKind, contrast: bool, emphasized: bool) -> BandStyle {
        let max_width = smax(2.0, self.bounds().width() - Self::HORIZONTAL_MARGIN * 2.0);
        match kind {
            DensityBandKind::Heading { level } => BandStyle {
                width_points: smin(Self::heading_mark_width(level, emphasized), max_width),
                min_height: 2.5,
                color: style_sheet.rail_tick.clone(),
            },
            DensityBandKind::SearchHit => BandStyle {
                // Rare sparks: a touch wider / brighter than heading ticks.
                width_points: smin(14.0, max_width),
                min_height: 2.5,
                color: style_sheet.search_hit.colorWithAlphaComponent(if contrast { 1.0 } else { 0.92 }),
            },
            DensityBandKind::Change(change_kind) => {
                let (width, height): (CGFloat, CGFloat) = match change_kind {
                    ChangeKind::Inserted => (16.0, 3.0),
                    ChangeKind::Modified => (12.0, 5.0),
                    ChangeKind::Deleted => (6.0, 6.0),
                };
                BandStyle {
                    width_points: smin(width, max_width),
                    min_height: height,
                    color: style_sheet.change_color(change_kind).colorWithAlphaComponent(if contrast { 1.0 } else { 0.95 }),
                }
            }
            DensityBandKind::CodeBlock
            | DensityBandKind::Table
            | DensityBandKind::Math
            | DensityBandKind::TaskList
            | DensityBandKind::Image
            | DensityBandKind::Callout => BandStyle {
                // Body-shape bands stay in the data model but are not drawn at rest.
                width_points: smin(8.0, max_width),
                min_height: 2.0,
                color: style_sheet.text_secondary.panel_alpha(0.4, contrast),
            },
        }
    }

    /// Renders one small layer per visible document mark.
    fn update_mark_layers(&self, animated: bool) {
        let bounds = self.bounds();
        if !(bounds.height() > 0.0) {
            return;
        }
        let ivars = self.ivars();
        let style_sheet = self.style_sheet();
        let entries = self.resolved_stack(bounds.height());
        let current = self.current_heading_fraction();
        let current_changed = current != ivars.previous_current_fraction.get();
        ivars.previous_current_fraction.set(current);
        let available = smax(1.0, bounds.width() - Self::HORIZONTAL_MARGIN * 2.0);
        let pointer_y = ivars.pointer_location.get().map(|point| point.y);
        let velocity_influence = smin(1.0, ivars.pointer_velocity_y.get().abs() / Self::SCRUB_VELOCITY_SCALE);
        let velocity_pull = if ivars.is_scrubbing.get() { Self::SCRUB_VELOCITY_PULL * velocity_influence } else { 0.0 };
        let magnetic_cap = Self::MAGNETIC_PULL + velocity_pull;
        let reduce_motion = style_sheet.reduce_motion;

        {
            let mut mark_layers = ivars.mark_layers.borrow_mut();
            while mark_layers.len() < entries.len() {
                let mark = CALayer::new();
                // SAFETY: Core Animation exports the curve as an immutable global.
                mark.setCornerCurve(unsafe { kCACornerCurveContinuous });
                mark.setMasksToBounds(false);
                if let Some(layer) = self.layer() {
                    layer.addSublayer(&mark);
                }
                mark_layers.push(mark);
            }
        }
        {
            let mut simulations = ivars.mark_simulations.borrow_mut();
            if simulations.len() > entries.len() {
                simulations.truncate(entries.len());
            }
            if simulations.len() < entries.len() {
                let clear = CGColor::constant_color(Some(unsafe { objc2_core_graphics::kCGColorClear }))
                    .expect("the clear constant colour");
                while simulations.len() < entries.len() {
                    simulations.push(MarkSimulation::new(crate::appkit_compat::RECT_ZERO, &clear, 0.0));
                }
            }
        }

        let hovered = ivars.hovered_band_index.get();
        let read_progress = ivars.read_progress.get();
        for (index, entry) in entries.iter().enumerate() {
            let is_current = match current {
                Some(current) => {
                    matches!(entry.band.kind, DensityBandKind::Heading { .. }) && entry.band.start_fraction == current
                }
                None => false,
            };
            let is_primary = Some(index as isize) == hovered;
            let emphasized = is_current || is_primary;
            let mut band_style = self.style(&style_sheet, entry.band.kind, style_sheet.increase_contrast, emphasized);
            let distance = pointer_y.map(|pointer_y| (entry.y - pointer_y).abs());
            let influence = distance.map_or(0.0, |distance| Self::proximity_influence(distance, Self::PROXIMITY_RADIUS));
            let focus = if is_primary { smax(influence, 0.78) } else { influence * 0.72 };
            let neighborhood = Self::neighborhood_factor(index as isize, hovered);

            if is_current {
                band_style.color = style_sheet.rail_tick_current.colorWithAlphaComponent(0.92);
                band_style.min_height = smax(band_style.min_height, 3.0);
            } else if matches!(entry.band.kind, DensityBandKind::Heading { .. }) {
                // Every mark stays a solid line; read state is a quiet alpha step.
                band_style.color = style_sheet
                    .rail_tick
                    .colorWithAlphaComponent(if entry.band.start_fraction <= read_progress { 0.56 } else { 0.44 });
            }

            if focus > 0.02 {
                match entry.band.kind {
                    DensityBandKind::Heading { .. } => {
                        let alpha = (if is_current { 0.92 } else { 0.52 }) + focus * (if is_current { 0.06 } else { 0.42 });
                        band_style.color = style_sheet.rail_tick_current.colorWithAlphaComponent(smin(0.98, alpha));
                    }
                    DensityBandKind::SearchHit | DensityBandKind::Change(_) => {
                        band_style.color = band_style.color.colorWithAlphaComponent(smin(1.0, 0.88 + focus * 0.12));
                    }
                    _ => {}
                }
            }

            if neighborhood < 1.0 && !is_primary {
                band_style.color = band_style.color.colorWithAlphaComponent(band_style.color.alphaComponent() * neighborhood);
            }

            let mark_width = smin(available, band_style.width_points + focus * 12.0);
            let mark_height = band_style.min_height + focus * 2.2;
            let magnetic = pointer_y.map_or(0.0, |pointer_y| pointer_y - entry.y)
                * focus
                * (magnetic_cap / smax(1.0, Self::PROXIMITY_RADIUS));
            let mark_y = entry.y + smax(-magnetic_cap, smin(magnetic_cap, magnetic));
            // One optical spine: every mark grows symmetrically from midX.
            let mark_x = bounds.mid_x() - mark_width / 2.0;
            let frame = rect(mark_x, mark_y - mark_height / 2.0, mark_width, mark_height);
            let glow: CGFloat =
                if !is_current || reduce_motion { 0.0 } else { CGFloat::from(Self::CURRENT_GLOW_OPACITY) };
            if is_current {
                let mark = ivars.mark_layers.borrow()[index].clone();
                mark.setShadowColor(Some(&style_sheet.rail_tick_current.CGColor()));
            }

            let color = band_style.color.CGColor();
            let mut simulations = ivars.mark_simulations.borrow_mut();
            if animated && !reduce_motion {
                simulations[index].retarget(frame, &color, glow, is_current && current_changed);
            } else {
                simulations[index].snap(frame, &color, glow);
            }
        }

        let extra: Vec<Retained<CALayer>> = ivars.mark_layers.borrow().iter().skip(entries.len()).cloned().collect();
        for mark in extra {
            mark.removeAllAnimations();
            mark.setShadowOpacity(0.0);
            mark.setHidden(true);
        }

        self.update_pip_layers(&style_sheet, &entries, animated);
        if animated && !reduce_motion {
            self.arm_rail_driver();
        } else {
            self.apply_simulations();
        }
    }

    /// Review overlays sit on a fixed leading offset from the *resting* mark
    /// width.
    fn update_pip_layers(&self, style_sheet: &StyleSheet, entries: &[ResolvedMark], animated: bool) {
        let ivars = self.ivars();
        let contrast = style_sheet.increase_contrast;
        let mut wanted: Vec<(CGFloat, CGFloat, Retained<NSColor>)> = Vec::new();
        let diameter = Self::PIP_DIAMETER;
        let resting_half = Self::heading_mark_width(1, false) / 2.0;
        let mid_x = self.bounds().mid_x();

        for entry in entries.iter().filter(|entry| !entry.pip.is_empty()) {
            let mut trailing_edge = mid_x - resting_half - Self::PIP_LEADING_GAP;
            let mut colors: Vec<Retained<NSColor>> = Vec::new();
            if let Some(change) = entry.pip.change {
                colors.push(style_sheet.change_color(change).colorWithAlphaComponent(if contrast { 1.0 } else { 0.95 }));
            }
            if entry.pip.search_hit {
                colors.push(style_sheet.search_hit.colorWithAlphaComponent(if contrast { 1.0 } else { 0.92 }));
            }
            for color in colors {
                wanted.push((trailing_edge - diameter, entry.y - diameter / 2.0, color));
                trailing_edge -= diameter + 2.5;
            }
        }

        {
            let mut pip_layers = ivars.pip_layers.borrow_mut();
            while pip_layers.len() < wanted.len() {
                let pip = CALayer::new();
                // SAFETY: Core Animation exports the curve as an immutable global.
                pip.setCornerCurve(unsafe { kCACornerCurveContinuous });
                if let Some(layer) = self.layer() {
                    layer.addSublayer(&pip);
                }
                pip_layers.push(pip);
            }
        }
        {
            let mut simulations = ivars.pip_simulations.borrow_mut();
            if simulations.len() > wanted.len() {
                simulations.truncate(wanted.len());
            }
        }
        for pip in ivars.pip_layers.borrow().iter().skip(wanted.len()) {
            pip.setHidden(true);
        }

        let release_now = !animated || style_sheet.reduce_motion;
        let mut simulations = ivars.pip_simulations.borrow_mut();
        for (index, (x, y, color)) in wanted.iter().enumerate() {
            let color = color.CGColor();
            if index >= simulations.len() {
                simulations.push(PipSimulation::new(CGPoint::new(*x, *y), diameter, &color));
            }
            if release_now {
                simulations[index].snap(CGPoint::new(*x, *y), diameter, &color);
            } else {
                simulations[index].retarget(
                    CGPoint::new(*x, *y),
                    diameter,
                    &color,
                    motion::PREVIEW_STAGGER * index as CGFloat,
                    false,
                );
            }
        }
    }

    // MARK: - Rail driver

    fn arm_rail_driver(&self) {
        if self.window().is_none() || self.style_sheet().reduce_motion {
            return;
        }
        self.arm_springs();
    }

    /// Draw what the springs currently say, in one transaction.
    fn apply_simulations(&self) {
        let ivars = self.ivars();
        let breathe = ivars.breathe_spring.get().value();
        let style_sheet = self.style_sheet();
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        {
            let simulations = ivars.mark_simulations.borrow();
            for (index, mark) in ivars.mark_layers.borrow().iter().enumerate() {
                if !(index < simulations.len()) {
                    continue;
                }
                let simulation = &simulations[index];
                let stateless = simulation.view_frame();
                let width = stateless.width() * breathe;
                mark.setFrame(rect(stateless.mid_x() - width / 2.0, stateless.min_y(), width, stateless.height()));
                mark.setCornerRadius(stateless.height() / 2.0);
                mark.setBackgroundColor(Some(&simulation.color.value().CGColor()));
                mark.setHidden(false);
                let glow = simulation.glow.value();
                if glow > 0.01 {
                    mark.setShadowColor(Some(&style_sheet.rail_tick_current.CGColor()));
                    mark.setShadowOpacity(glow as f32);
                    mark.setShadowRadius(Self::CURRENT_GLOW_RADIUS);
                    mark.setShadowOffset(CGSize::new(0.0, 0.0));
                } else {
                    mark.setShadowOpacity(0.0);
                    mark.setShadowRadius(0.0);
                }
            }
        }
        {
            let pip_layers = ivars.pip_layers.borrow();
            for (index, pip) in ivars.pip_simulations.borrow().iter().enumerate() {
                if !(index < pip_layers.len()) {
                    continue;
                }
                let layer = &pip_layers[index];
                let d = pip.diameter.value() * breathe;
                let position = pip.position();
                layer.setFrame(rect(position.x - d / 2.0, position.y - d / 2.0, d, d));
                layer.setCornerRadius(d / 2.0);
                let color = pip.current_color();
                layer.setBackgroundColor(Some(&color.CGColor()));
                layer.setHidden(color.alphaComponent() < 0.01);
            }
        }
        CATransaction::commit();
    }

    pub fn neighborhood_factor(index: isize, hovered_index: Option<isize>) -> CGFloat {
        let Some(hovered_index) = hovered_index else { return 1.0 };
        let distance = (index - hovered_index).abs();
        if distance == 0 {
            return 1.0;
        }
        if distance <= Self::NEIGHBORHOOD_LIFT_RADIUS {
            return 0.92;
        }
        Self::NEIGHBORHOOD_DIM
    }

    // MARK: - Pointer (§7.1 "click to jump, drag to scrub")

    /// The rail has exactly one coordinate system: the marks.
    fn fraction_at(&self, point: NSPoint) -> CGFloat {
        let bounds = self.bounds();
        if !(bounds.height() > 0.0) {
            return 0.0;
        }
        let entries = self.resolved_stack(bounds.height());
        let (Some(first), Some(last)) = (entries.first(), entries.last()) else {
            // No stack: the track is all there is to read.
            let track = Self::track_range(bounds.height(), Self::TRACK_INSET);
            return smin(1.0, smax(0.0, (point.y - track.0) / smax(1.0, track.1 - track.0)));
        };
        if point.y <= first.y {
            return first.band.start_fraction;
        }
        if point.y >= last.y {
            return last.band.start_fraction;
        }
        if let Some(nearest) = first_minimum(&entries, |entry| (entry.y - point.y).abs())
            && (nearest.y - point.y).abs() <= Self::HOVER_ACTIVATION_SLOP
        {
            return nearest.band.start_fraction;
        }
        let Some(upper) = entries.iter().position(|entry| entry.y > point.y).filter(|upper| *upper > 0) else {
            return last.band.start_fraction;
        };
        let low = entries[upper - 1];
        let high = entries[upper];
        let t = (point.y - low.y) / smax(1.0, high.y - low.y);
        low.band.start_fraction + (high.band.start_fraction - low.band.start_fraction) * t
    }

    /// Scrub release always settles on the nearest mark.
    fn snap_fraction_at(&self, point: NSPoint) -> CGFloat {
        let entries = self.resolved_stack(self.bounds().height());
        if let Some(nearest) = first_minimum(&entries, |entry| (entry.y - point.y).abs()) {
            return nearest.band.start_fraction;
        }
        self.fraction_at(point)
    }

    fn sample_pointer_velocity(&self, y: CGFloat) {
        let ivars = self.ivars();
        let now = CACurrentMediaTime();
        if let Some((last_y, last_time)) = ivars.last_pointer_sample.get() {
            let dt = now - last_time;
            if dt > 0.001 && dt < 0.25 {
                let raw = (y - last_y) / dt;
                ivars.pointer_velocity_y.set(ivars.pointer_velocity_y.get() * 0.35 + raw * 0.65);
            }
        }
        ivars.last_pointer_sample.set(Some((y, now)));
    }

    fn set_rail_breathe(&self, target: CGFloat, animated: bool) {
        let ivars = self.ivars();
        let mut spring = ivars.breathe_spring.get();
        if !((spring.value() - target).abs() > 0.001) {
            return;
        }
        if animated && !self.style_sheet().reduce_motion {
            spring.target(target);
            ivars.breathe_spring.set(spring);
            self.arm_rail_driver();
        } else {
            spring.snap(target);
            ivars.breathe_spring.set(spring);
            self.apply_simulations();
        }
    }

    /// The landing punch is a velocity kick on the mark's own width spring.
    fn perform_jump_punch(&self, index: Option<isize>) {
        let ivars = self.ivars();
        let Some(index) = index else { return };
        {
            let mut simulations = ivars.mark_simulations.borrow_mut();
            if !(index >= 0 && (index as usize) < simulations.len()) {
                return;
            }
            let simulation = &mut simulations[index as usize];
            simulation.frame.size.width.kick(motion::JUMP_PUNCH_KICK);
            simulation.frame.size.height.kick(motion::JUMP_PUNCH_KICK * 0.5);
        }
        self.perform_rail_haptic();
        self.arm_rail_driver();
    }

    /// Every tap the rail makes goes through here.
    fn perform_rail_haptic(&self) {
        if self.style_sheet().reduce_motion {
            return;
        }
        let ivars = self.ivars();
        let now = CACurrentMediaTime();
        if !(now - ivars.last_haptic_time.get() >= Self::DETENT_INTERVAL) {
            return;
        }
        ivars.last_haptic_time.set(now);
        let perform = ivars.perform_haptic_feedback.borrow().clone();
        perform();
    }

    /// Which hover transitions earn a detent.
    pub fn is_detent_crossing(previous: Option<isize>, next: Option<isize>) -> bool {
        let Some(next) = next else { return false };
        Some(next) != previous
    }

    fn update_hovered_band(&self, point: Option<NSPoint>, animated: bool) {
        let ivars = self.ivars();
        let bounds = self.bounds();
        let next_index: Option<isize> = match point {
            Some(point) if bounds.height() > 0.0 => {
                let positions: Vec<CGFloat> = self.resolved_stack(bounds.height()).iter().map(|entry| entry.y).collect();
                Self::next_hovered_band_index(
                    point.y,
                    &positions,
                    ivars.hovered_band_index.get(),
                    Self::HOVER_ACTIVATION_SLOP,
                    Self::dismissal_slop(&positions),
                )
            }
            _ => None,
        };

        let index_changed = next_index != ivars.hovered_band_index.get();
        if Self::is_detent_crossing(ivars.hovered_band_index.get(), next_index) {
            self.perform_rail_haptic();
        }
        ivars.hovered_band_index.set(next_index);
        if index_changed || point.is_some() || ivars.pointer_location.get().is_some() {
            self.update_mark_layers(animated);
        }
    }

    /// Drive the hover funnel without a window or a synthesised event
    /// (`driveHoverForTesting(toY:)`).
    pub fn drive_hover_for_testing(&self, y: CGFloat) {
        self.update_hovered_band(Some(NSPoint::new(self.bounds().mid_x(), y)), false);
    }

    /// The drawn mark positions (`markPositionsForTesting`).
    pub fn mark_positions_for_testing(&self) -> Vec<CGFloat> {
        self.resolved_stack(self.bounds().height()).iter().map(|entry| entry.y).collect()
    }

    /// A mark's hover row is half the gap to its neighbour.
    pub fn dismissal_slop(positions: &[CGFloat]) -> CGFloat {
        if !(positions.len() > 1) {
            return Self::HOVER_DISMISSAL_SLOP;
        }
        let mut smallest: Option<CGFloat> = None;
        for pair in positions.windows(2) {
            let gap = pair[1] - pair[0];
            smallest = Some(match smallest {
                Some(current) if !(gap < current) => current,
                _ => gap,
            });
        }
        let smallest_gap = smallest.unwrap_or(Self::HOVER_DISMISSAL_SLOP);
        smax(Self::HOVER_DISMISSAL_SLOP, smallest_gap / 2.0)
    }

    pub fn next_hovered_band_index(
        y: CGFloat,
        positions: &[CGFloat],
        current_index: Option<isize>,
        activation_slop: CGFloat,
        dismissal_slop: CGFloat,
    ) -> Option<isize> {
        let indices: Vec<usize> = (0..positions.len()).collect();
        let nearest = *first_minimum(&indices, |index| (positions[*index] - y).abs())?;

        if let Some(current_index) = current_index
            && current_index >= 0
            && (current_index as usize) < positions.len()
        {
            let current_distance = (positions[current_index as usize] - y).abs();
            if current_distance <= dismissal_slop {
                return Some(current_index);
            }
            if nearest as isize == current_index {
                return None;
            }
        }
        if (positions[nearest] - y).abs() <= activation_slop { Some(nearest as isize) } else { None }
    }

    fn location(&self, event: &NSEvent) -> NSPoint {
        self.convertPoint_fromView(event.locationInWindow(), None)
    }

    fn mouse_down(&self, event: &NSEvent) {
        let ivars = self.ivars();
        self.cancel_preview_hide();
        self.cancel_outline_show();
        self.cancel_outline_hide();
        ivars.pointer_is_in_preview.set(false);
        ivars.pointer_is_in_outline.set(false);
        ivars.outline.dismiss();
        ivars.did_drag.set(false);
        ivars.is_scrubbing.set(false);
        ivars.pointer_velocity_y.set(0.0);
        ivars.last_pointer_sample.set(None);
        let point = self.location(event);
        ivars.mouse_down_location.set(Some(point));
        ivars.pointer_location.set(Some(point));
        self.sample_pointer_velocity(point.y);
        // Mouse-down only previews. Navigation starts on mouse-up.
        self.update_hovered_band(Some(point), true);
        self.show_preview(point, true, false);
    }

    fn mouse_dragged(&self, event: &NSEvent) {
        let ivars = self.ivars();
        self.cancel_preview_hide();
        self.cancel_outline_show();
        ivars.pointer_is_in_preview.set(false);
        ivars.pointer_is_in_outline.set(false);
        if ivars.outline.isVisible() {
            ivars.outline.dismiss();
        }
        let point = self.location(event);
        let begins = ivars.mouse_down_location.get().map(|start| Self::should_begin_scrub(start, point)) == Some(true);
        if !(ivars.did_drag.get() || begins) {
            ivars.pointer_location.set(Some(point));
            self.update_hovered_band(Some(point), true);
            self.show_preview(point, true, false);
            return;
        }
        ivars.did_drag.set(true);
        ivars.is_scrubbing.set(true);
        ivars.pointer_location.set(Some(point));
        self.sample_pointer_velocity(point.y);
        self.scrub(event, true, false);
    }

    fn mouse_up(&self, event: &NSEvent) {
        let ivars = self.ivars();
        let was_dragging = ivars.did_drag.get();
        ivars.mouse_down_location.set(None);
        ivars.is_scrubbing.set(false);
        ivars.did_drag.set(false);
        let point = self.location(event);
        ivars.pointer_location.set(Some(point));
        self.update_hovered_band(Some(point), true);
        let target = if was_dragging { self.snap_fraction_at(point) } else { self.fraction_at(point) };
        if let Some(delegate) = self.delegate() {
            delegate.density_gutter_did_request_scroll_to_fraction(self, target);
        }
        let punch_index = self
            .resolved_stack(self.bounds().height())
            .iter()
            .position(|entry| (entry.band.start_fraction - target).abs() < 0.000_1)
            .map(|index| index as isize)
            .or(ivars.hovered_band_index.get());
        self.perform_jump_punch(punch_index);
        ivars.pointer_velocity_y.set(0.0);
        ivars.last_pointer_sample.set(None);
        if self.bounds().contains_point(point) && ivars.hovered_band_index.get().is_some() {
            self.show_preview(point, true, true);
        } else {
            self.cancel_preview_hide();
            ivars.preview.hide();
            self.update_hovered_band(None, true);
            ivars.pointer_location.set(None);
        }
        if was_dragging {
            self.setNeedsDisplay(true);
        }
    }

    fn mouse_moved(&self, event: &NSEvent) {
        let ivars = self.ivars();
        if ivars.is_scrubbing.get() {
            return;
        }
        let point = self.location(event);
        self.cancel_preview_hide();
        ivars.pointer_is_in_preview.set(false);
        ivars.pointer_location.set(Some(point));
        self.sample_pointer_velocity(point.y);
        self.update_hovered_band(Some(point), true);
        if ivars.hovered_band_index.get().is_none() {
            if let Some(work) = ivars.preview_work_item.borrow_mut().take() {
                work.cancel();
            }
            ivars.preview.hide();
            return;
        }
        if ivars.preview.isVisible() {
            self.show_preview(point, true, true);
        } else {
            self.schedule_preview(point);
        }
    }

    fn mouse_entered(&self, event: &NSEvent) {
        let ivars = self.ivars();
        let point = self.location(event);
        self.cancel_preview_hide();
        ivars.pointer_is_in_preview.set(false);
        ivars.pointer_location.set(Some(point));
        self.set_rail_breathe(Self::BREATHE_SCALE, true);
        // Entering springs like every other pointer move.
        self.update_hovered_band(Some(point), true);
        if ivars.hovered_band_index.get().is_some() {
            self.schedule_preview(point);
        }
    }

    fn mouse_exited(&self, _event: &NSEvent) {
        let ivars = self.ivars();
        if ivars.is_scrubbing.get() {
            return;
        }
        if let Some(work) = ivars.preview_work_item.borrow_mut().take() {
            work.cancel();
        }
        ivars.pointer_location.set(None);
        ivars.pointer_velocity_y.set(0.0);
        ivars.last_pointer_sample.set(None);
        self.set_rail_breathe(1.0, true);
        self.update_mark_layers(true);
        if !ivars.preview.isVisible() {
            self.update_hovered_band(None, true);
            return;
        }
        self.schedule_preview_hide();
    }

    pub fn present_outline_for_keyboard(&self) {
        let Some(window) = self.window() else { return };
        let ivars = self.ivars();
        self.cancel_preview_hide();
        self.cancel_outline_show();
        ivars.pointer_is_in_preview.set(false);
        ivars.preview.hide();
        ivars.outline.set_entries(self.outline_entries());
        ivars.outline.show(self, &window, true, None);
    }

    fn schedule_preview(&self, point: NSPoint) {
        let ivars = self.ivars();
        if let Some(work) = ivars.preview_work_item.borrow().as_ref() {
            work.cancel();
        }
        self.cancel_preview_hide();
        let weak: ObjcWeak<DensityGutterView> = ObjcWeak::from(self);
        let work = WorkItem::new(move || {
            let Some(this) = weak.load() else { return };
            if this.window().is_none() {
                return;
            }
            let ivars = this.ivars();
            if ivars.pointer_location.get() != Some(point) {
                return;
            }
            if !(this.bounds().contains_point(point) && !ivars.bands.borrow().is_empty() && ivars.hovered_band_index.get().is_some())
            {
                ivars.preview.hide();
                return;
            }
            this.show_preview(point, true, true);
        });
        *ivars.preview_work_item.borrow_mut() = Some(work.clone());
        work.dispatch_main_after(Self::HOVER_DWELL);
    }

    fn cancel_outline_show(&self) {
        if let Some(work) = self.ivars().outline_work_item.borrow_mut().take() {
            work.cancel();
        }
    }

    fn schedule_preview_hide(&self) {
        let ivars = self.ivars();
        if let Some(work) = ivars.preview_hide_work_item.borrow().as_ref() {
            work.cancel();
        }
        let weak: ObjcWeak<DensityGutterView> = ObjcWeak::from(self);
        let work = WorkItem::new(move || {
            let Some(this) = weak.load() else { return };
            if this.ivars().pointer_is_in_preview.get() {
                return;
            }
            this.ivars().preview.hide();
            this.update_hovered_band(None, true);
            this.ivars().pointer_location.set(None);
        });
        *ivars.preview_hide_work_item.borrow_mut() = Some(work.clone());
        work.dispatch_main_after(Self::PREVIEW_EXIT_DELAY);
    }

    fn cancel_preview_hide(&self) {
        if let Some(work) = self.ivars().preview_hide_work_item.borrow_mut().take() {
            work.cancel();
        }
    }

    fn schedule_outline_hide(&self) {
        let ivars = self.ivars();
        if let Some(work) = ivars.outline_hide_work_item.borrow().as_ref() {
            work.cancel();
        }
        let weak: ObjcWeak<DensityGutterView> = ObjcWeak::from(self);
        let work = WorkItem::new(move || {
            let Some(this) = weak.load() else { return };
            if this.ivars().pointer_is_in_outline.get() {
                return;
            }
            this.ivars().outline.dismiss();
        });
        *ivars.outline_hide_work_item.borrow_mut() = Some(work.clone());
        work.dispatch_main_after(DensityOutlineWindow::HIDE_DELAY);
    }

    fn cancel_outline_hide(&self) {
        if let Some(work) = self.ivars().outline_hide_work_item.borrow_mut().take() {
            work.cancel();
        }
    }

    fn scrub(&self, event: &NSEvent, shows_snippet: bool, interactive: bool) {
        let point = self.location(event);
        self.update_hovered_band(Some(point), true);
        let fraction = self.fraction_at(point);
        if let Some(delegate) = self.delegate() {
            delegate.density_gutter_did_request_scroll_to_fraction(self, fraction);
        }
        self.show_preview(point, shows_snippet, interactive);
    }

    fn show_preview(&self, point: NSPoint, shows_snippet: bool, interactive: bool) {
        let ivars = self.ivars();
        if !(ivars.is_scrubbing.get() || ivars.hovered_band_index.get().is_some()) {
            ivars.preview.hide();
            return;
        }
        let Some(window) = self.window() else {
            ivars.preview.hide();
            return;
        };
        let content = self
            .delegate()
            .and_then(|delegate| delegate.density_gutter_preview_at_fraction(self, self.fraction_at(point)));
        let Some((title, snippet, context)) = content else {
            ivars.preview.hide();
            return;
        };
        let anchor = window.convertPointToScreen(self.convertPoint_toView(NSPoint::new(self.bounds().width(), point.y), None));
        let maximum_trailing_x: Option<CGFloat> = (|| {
            // SAFETY: the superview is only read.
            let container = unsafe { self.superview() }?.downcast::<MarkdownContainerView>().ok()?;
            let leading = container.leading_accessory()?;
            if !std::ptr::eq((&*leading as *const NSView).cast::<u8>(), (self as *const Self).cast::<u8>()) {
                return None;
            }
            let scroll_view = container.scroll_view();
            let text_origin =
                scroll_view.frame().min_x() + scroll_view.contentInsets().left + render_metrics::REVEAL_SLACK;
            let boundary = container.convertPoint_toView(NSPoint::new(text_origin - 12.0, container.bounds().min_y()), None);
            Some(window.convertPointToScreen(boundary).x)
        })();
        let footer = if context.is_empty() { self.metrics_summary() } else { context };
        ivars.preview.show(
            &title,
            if shows_snippet { &snippet } else { "" },
            &footer,
            anchor,
            &window,
            maximum_trailing_x,
            self.style_sheet().reduce_motion,
            interactive,
            ivars.allows_preview_content_overlap.get(),
        );
    }

    /// A child-window preview does not participate in AppKit view layout.
    /// Re-resolve it when its container moves or resizes.
    pub fn container_geometry_did_change(&self) {
        let ivars = self.ivars();
        if !ivars.preview.isVisible() {
            return;
        }
        let Some(point) = ivars.pointer_location.get().filter(|point| {
            self.bounds().contains_point(*point) && ivars.hovered_band_index.get().is_some()
        }) else {
            ivars.preview.hide();
            return;
        };
        self.show_preview(point, true, true);
    }

    fn view_did_move_to_window(&self) {
        if self.window().is_some() {
            return;
        }
        let ivars = self.ivars();
        if let Some(work) = ivars.preview_work_item.borrow_mut().take() {
            work.cancel();
        }
        self.cancel_preview_hide();
        self.cancel_outline_show();
        self.cancel_outline_hide();
        self.park_springs();
        let mut breathe = ivars.breathe_spring.get();
        breathe.snap(1.0);
        ivars.breathe_spring.set(breathe);
        ivars.pointer_location.set(None);
        ivars.pointer_velocity_y.set(0.0);
        ivars.last_pointer_sample.set(None);
        ivars.pointer_is_in_preview.set(false);
        ivars.pointer_is_in_outline.set(false);
        ivars.preview.hide();
        ivars.outline.dismiss();
        self.update_hovered_band(None, false);
    }

    // MARK: - Band construction

    /// Builds bands from a parsed document plus overlays.
    pub fn bands_for(document: &ParsedDocument, changes: &[(ChangeKind, NSRange)], search_hits: &[NSRange]) -> Vec<DensityBand> {
        let length = 1isize.max(document.length) as CGFloat;
        let band = |kind: DensityBandKind, range: NSRange| -> DensityBand {
            let start = smin(1.0, smax(0.0, range.location as CGFloat / length));
            let end = smin(1.0, smax(start, range.upper_bound() as CGFloat / length));
            DensityBand::new(kind, start, end)
        };

        let mut result: Vec<DensityBand> = Vec::new();
        document.root.walk_pruning(&mut |block| match &block.content {
            BlockContent::Heading { level } => {
                result.push(band(DensityBandKind::Heading { level: *level }, block.range));
                false
            }
            BlockContent::CodeBlock { .. } => {
                result.push(band(DensityBandKind::CodeBlock, block.range));
                false
            }
            BlockContent::Mermaid { .. } => {
                // A diagram is a figure, not code, once it is rendered (§6.2).
                result.push(band(DensityBandKind::Image, block.range));
                false
            }
            BlockContent::MathBlock { .. } => {
                result.push(band(DensityBandKind::Math, block.range));
                false
            }
            BlockContent::Table(_) => {
                result.push(band(DensityBandKind::Table, block.range));
                false
            }
            BlockContent::Callout { .. } => {
                // Pruned: a callout is a single visual object in the rail.
                result.push(band(DensityBandKind::Callout, block.range));
                false
            }
            BlockContent::List { .. } => {
                if !contains_checkbox(block) {
                    return true;
                }
                result.push(band(DensityBandKind::TaskList, block.range));
                false
            }
            BlockContent::Paragraph => {
                if contains_image(block) {
                    result.push(band(DensityBandKind::Image, block.range));
                }
                false
            }
            _ => true,
        });

        for (kind, range) in changes {
            result.push(band(DensityBandKind::Change(*kind), *range));
        }
        for hit in search_hits {
            result.push(band(DensityBandKind::SearchHit, *hit));
        }
        result
    }
}

fn contains_checkbox(block: &MDBlock) -> bool {
    block.children.iter().any(|child| matches!(&child.content, BlockContent::ListItem { checkbox, .. } if checkbox.is_some()))
}

fn contains_image(block: &MDBlock) -> bool {
    let mut found = false;
    for inline in &block.inlines {
        inline.walk(&mut |span| {
            if matches!(span.kind, InlineKind::Image { .. }) {
                found = true;
            }
        });
        if found {
            break;
        }
    }
    found
}

fn heading_level(band: &DensityBand) -> isize {
    if let DensityBandKind::Heading { level } = band.kind {
        return level;
    }
    isize::MAX
}

/// Swift's stable `sorted { $0.startFraction < $1.startFraction }`.
fn sort_by_start(bands: &mut [DensityBand]) {
    bands.sort_by(|a, b| {
        if a.start_fraction < b.start_fraction {
            std::cmp::Ordering::Less
        } else if b.start_fraction < a.start_fraction {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
}

/// Swift's `min(by:)`: the first element no later element is strictly
/// below.
fn first_minimum<T>(items: &[T], key: impl Fn(&T) -> CGFloat) -> Option<&T> {
    let mut iterator = items.iter();
    let mut best = iterator.next()?;
    let mut best_key = key(best);
    for item in iterator {
        let item_key = key(item);
        if item_key < best_key {
            best = item;
            best_key = item_key;
        }
    }
    Some(best)
}

#[allow(dead_code)]
fn _haptic_performer_in_scope(_: &dyn NSHapticFeedbackPerformer) {}
