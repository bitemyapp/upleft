//! Port of `App/PresentationDrag.swift`: whether the two-finger swipe can
//! afford to drag the next presentation in under the fingers
//! (`PresentationSwitchBudget`), and the drag itself (`PaneDrag`,
//! `PaneDragTrack`).
//!
//! How the Swift maps:
//! - `PresentationSwitchBudget` is a unit struct; its process-wide
//!   `millisecondsPerLine` (a `@MainActor static var`) is an `f64` stored as
//!   bits in an atomic, so it stays one value for the whole process.
//! - `PaneDrag` holds its pane weakly and owns the still `CALayer`.
//! - `PaneDragTrack` is shared as `Rc` so the spring driver's `[weak self]`
//!   closures hold a `std::rc::Weak`; stored properties are `Cell`/`RefCell`s
//!   borrowed only for the statement that uses them.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU64, Ordering};

use objc2::AllocAnyThread;
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSBitmapImageRep, NSDeviceRGBColorSpace, NSGraphicsContext, NSView};
use objc2_core_foundation::{CFType, CGFloat};
use objc2_core_graphics::{CGContext, CGImage};
use objc2_quartz_core::{CALayer, CATransaction, CATransform3D, kCAGravityResize};
use upleft_render::appkit_compat::RectExt;
use upleft_render::motion::{self, SpringDriver, SpringScalar};
use upleft_render::swift_compat::{int_truncating, rounded, smax, smin};
use upleft_render::view::markdown_container_view::MarkdownContainerView;

use crate::app::document_scroll_gestures::{swift_max, transform_identity};

/// Whether the two-finger swipe can afford to drag the *next* presentation in
/// under the fingers, for the document currently in front of the reader.
///
/// It is a question about this document on this machine, not a constant.
/// Source has to be rendered before it can be dragged, and that rebuild is
/// proportional to document length. Below the budget the reader gets the real
/// thing; above it they get the page's give, which promises less and always
/// answers.
pub struct PresentationSwitchBudget;

/// `millisecondsPerLine`, seeded at 0.030 (the bits of that `f64`).
static MILLISECONDS_PER_LINE: AtomicU64 = AtomicU64::new(PresentationSwitchBudget::SEEDED_MILLISECONDS_PER_LINE.to_bits());

impl PresentationSwitchBudget {
    /// Three frames at 60 Hz. Past that, the hand notices that the surface
    /// has stopped answering.
    pub const ENGAGEMENT: f64 = 50.0;

    /// What the drag pays regardless of document length: one half-resolution
    /// still of the outgoing page, and the layout pass the switch leaves
    /// behind. Measured at ~3 ms and ~8 ms respectively, rounded up.
    pub const FIXED_COST: f64 = 14.0;

    /// The seed of `millisecondsPerLine`: ~0.03 ms per line of the document,
    /// from a release-build sweep.
    pub const SEEDED_MILLISECONDS_PER_LINE: f64 = 0.030;

    /// `millisecondsPerLine` (`private(set) static var`): seeded, then refined
    /// by every switch the app actually performs. Lines here means lines of
    /// the file, which is what `lineStarts` counts.
    pub fn milliseconds_per_line() -> f64 {
        f64::from_bits(MILLISECONDS_PER_LINE.load(Ordering::Relaxed))
    }

    fn set_milliseconds_per_line(value: f64) {
        MILLISECONDS_PER_LINE.store(value.to_bits(), Ordering::Relaxed);
    }

    /// Fold a real switch into the estimate. Short documents are ignored:
    /// their cost is mostly the fixed overhead, so dividing it by a small line
    /// count produces a per-line figure that describes nothing.
    pub fn record(cost: f64, lines: isize) {
        if !(lines >= 200) || !(cost > 0.0) || !cost.is_finite() {
            return;
        }
        let sample = cost / lines as f64;
        Self::set_milliseconds_per_line(Self::milliseconds_per_line() * 0.7 + sample * 0.3);
    }

    /// `estimatedCost(lines:)`.
    pub fn estimated_cost(lines: isize) -> f64 {
        Self::FIXED_COST + Self::milliseconds_per_line() * lines.max(0) as f64
    }

    /// `allowsDrag(lines:)`.
    pub fn allows_drag(lines: isize) -> bool {
        Self::estimated_cost(lines) <= Self::ENGAGEMENT
    }

    /// `resetCalibrationForTesting()`, with Swift's default of 0.030.
    pub fn reset_calibration_for_testing() {
        Self::reset_calibration_for_testing_to(Self::SEEDED_MILLISECONDS_PER_LINE);
    }

    /// `resetCalibrationForTesting(millisecondsPerLine:)`. Test seam: the
    /// estimate is process-wide state, so a test that pushes it has to be
    /// able to put it back.
    pub fn reset_calibration_for_testing_to(milliseconds_per_line: f64) {
        Self::set_milliseconds_per_line(milliseconds_per_line);
    }
}

// MARK: - One pane

/// One pane's drag: the presentation being left, as a still, travelling over
/// the live surface that has already become the presentation being entered.
///
/// The still is captured at half resolution through `CALayer.render`, which
/// composites what is already rasterised rather than re-running TextKit.
pub struct PaneDrag {
    pane: ObjcWeak<MarkdownContainerView>,
    still: Retained<CALayer>,
    was_masking: bool,
    width: CGFloat,
}

impl PaneDrag {
    /// Half. A quarter measured barely faster and starts to show on text.
    const STILL_SCALE: CGFloat = 0.5;

    /// `init?(pane:)`.
    pub fn new(pane: &MarkdownContainerView) -> Option<PaneDrag> {
        // `private let still = CALayer()`: made before the initialiser's body.
        let still = CALayer::new();
        if !(pane.bounds().width() > 1.0) || !(pane.bounds().height() > 1.0) {
            return None;
        }
        pane.setWantsLayer(true);
        pane.scroll_view().setWantsLayer(true);
        let pane_layer = pane.layer()?;
        let scroll_layer = pane.scroll_view().layer()?;
        let image = PaneDrag::still(pane.scroll_view(), PaneDrag::STILL_SCALE)?;

        let weak_pane = ObjcWeak::from(pane);
        let width = pane.scroll_view().bounds().width();
        let was_masking = pane_layer.masksToBounds();
        pane_layer.setMasksToBounds(true);

        let image_object: &CFType = &image;
        let image_object: &AnyObject = image_object.as_ref();
        // SAFETY: a `CGImage` is a valid layer `contents` object.
        unsafe { still.setContents(Some(image_object)) };
        // SAFETY: an immutable QuartzCore global.
        still.setContentsGravity(unsafe { kCAGravityResize });
        still.setFrame(pane.scroll_view().frame());
        // Opaque, because the live surface underneath has already changed mode
        // and must not show through the page that is still covering it.
        still.setOpaque(true);
        still.setBackgroundColor(Some(&pane.style_sheet().background.CGColor()));
        still.setZPosition(scroll_layer.zPosition() + 1.0);
        pane_layer.addSublayer(&still);
        Some(PaneDrag { pane: weak_pane, still, was_masking, width })
    }

    /// `pane` (`private(set) weak var`).
    pub fn pane(&self) -> Option<Retained<MarkdownContainerView>> {
        self.pane.load()
    }

    /// `width`: the scroll view's width when the still was taken.
    pub fn width(&self) -> CGFloat {
        self.width
    }

    /// `translation` is where the outgoing page has travelled; `direction` is
    /// its sign, so the incoming surface can be placed exactly one pane away
    /// and the two tile with no seam.
    pub fn place(&self, translation: CGFloat, direction: CGFloat) {
        let Some(scroll_layer) = self.pane.load().and_then(|pane| pane.scroll_view().layer()) else { return };
        let travel = smin(smax(translation, -self.width), self.width);
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.still.setTransform(CATransform3D::new_translation(travel, 0.0, 0.0));
        scroll_layer.setTransform(CATransform3D::new_translation(travel - direction * self.width, 0.0, 0.0));
        CATransaction::commit();
    }

    /// `release()`.
    pub fn release(&self) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.still.removeFromSuperlayer();
        if let Some(layer) = self.pane.load().and_then(|pane| pane.scroll_view().layer()) {
            layer.setTransform(transform_identity());
        }
        if let Some(layer) = self.pane.load().and_then(|pane| pane.layer()) {
            layer.setMasksToBounds(self.was_masking);
        }
        CATransaction::commit();
    }

    /// `NSGraphicsContext` rather than a bare `CGContext`: AppKit owns the
    /// flipped-geometry convention these layers were built under, and going
    /// through it is what keeps the still the right way up.
    fn still(view: &NSView, scale: CGFloat) -> Option<Retained<CGImage>> {
        let layer = view.layer()?;
        let pixels_wide = int_truncating(rounded(view.bounds().width() * scale)) as isize;
        let pixels_high = int_truncating(rounded(view.bounds().height() * scale)) as isize;
        if !(pixels_wide > 1) || !(pixels_high > 1) {
            return None;
        }
        // SAFETY: no planes, so AppKit allocates the storage itself.
        let representation = unsafe {
            NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
                NSBitmapImageRep::alloc(),
                std::ptr::null_mut(),
                pixels_wide,
                pixels_high,
                8,
                4,
                true,
                false,
                NSDeviceRGBColorSpace,
                0,
                0,
            )
        }?;
        let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&representation)?;
        NSGraphicsContext::saveGraphicsState_class();
        NSGraphicsContext::setCurrentContext(Some(&context));
        CGContext::scale_ctm(Some(&context.CGContext()), scale, scale);
        layer.renderInContext(&context.CGContext());
        let image = representation.CGImage();
        // `defer { NSGraphicsContext.restoreGraphicsState() }`: after the
        // return value is read.
        NSGraphicsContext::restoreGraphicsState_class();
        image
    }
}

// MARK: - Every pane

/// Every pane's drag at once, plus the spring that finishes it.
///
/// Mirrors `PaneGiveTrack` deliberately: the two are alternatives chosen by
/// budget, and a coordinator should be able to drive either without caring
/// which it got.
pub struct PaneDragTrack {
    this: Weak<PaneDragTrack>,
    surfaces: RefCell<Vec<PaneDrag>>,
    driver: RefCell<Option<SpringDriver>>,
    /// Critically damped. An overshoot here does not read as bounce, it reads
    /// as a sliver of the wrong page arriving from the opposite edge.
    offset: Cell<SpringScalar>,
    direction: Cell<CGFloat>,
    landing: RefCell<Option<Box<dyn FnOnce()>>>,
    pending_landing: Cell<bool>,
    is_settling: Cell<bool>,
}

impl Drop for PaneDragTrack {
    /// `deinit { driver?.park() }`.
    fn drop(&mut self) {
        if let Some(driver) = self.driver.get_mut().as_ref() {
            driver.park();
        }
    }
}

impl PaneDragTrack {
    /// `PaneDragTrack()`.
    pub fn new() -> Rc<PaneDragTrack> {
        Rc::new_cyclic(|this| PaneDragTrack {
            this: this.clone(),
            surfaces: RefCell::new(Vec::new()),
            driver: RefCell::new(None),
            offset: Cell::new(SpringScalar::with_value(0.0, motion::SPRING_STANDARD)),
            direction: Cell::new(1.0),
            landing: RefCell::new(None),
            pending_landing: Cell::new(false),
            is_settling: Cell::new(false),
        })
    }

    /// `isSettling` (`private(set) var`).
    pub fn is_settling(&self) -> bool {
        self.is_settling.get()
    }

    /// `isEngaged`.
    pub fn is_engaged(&self) -> bool {
        !self.surfaces.borrow().is_empty()
    }

    /// The travel a completed drag ends on: the widest pane, so a narrower one
    /// clamps to its own edge rather than stopping short of it.
    fn completed_travel(&self) -> CGFloat {
        let widest = swift_max(self.surfaces.borrow().iter().map(PaneDrag::width)).unwrap_or(0.0);
        self.direction.get() * widest
    }

    fn snap_offset(&self, value: CGFloat) {
        let mut offset = self.offset.get();
        offset.snap(value);
        self.offset.set(offset);
    }

    /// Capture every pane and take hold. Returns `false` if no pane could be
    /// captured, in which case the caller must fall back to the give — the
    /// stills are the whole mechanism and there is no drag without them.
    pub fn engage(&self, panes: &[Retained<MarkdownContainerView>], direction: CGFloat) -> bool {
        self.release();
        self.direction.set(if direction < 0.0 { -1.0 } else { 1.0 });
        let surfaces: Vec<PaneDrag> = panes.iter().filter_map(|pane| PaneDrag::new(pane)).collect();
        *self.surfaces.borrow_mut() = surfaces;
        self.snap_offset(0.0);
        !self.surfaces.borrow().is_empty()
    }

    /// `place(_:)`.
    pub fn place(&self, value: CGFloat) {
        self.snap_offset(value);
        let direction = self.direction.get();
        for surface in self.surfaces.borrow().iter() {
            surface.place(value, direction);
        }
    }

    /// Finish the drag: `committed` flies the outgoing page all the way off and
    /// leaves the live surface at rest, otherwise it comes home. `completion`
    /// runs once the panes have arrived, and is where the caller undoes a mode
    /// change it made speculatively.
    pub fn settle(&self, committed: bool, velocity: CGFloat, completion: impl FnOnce() + 'static) {
        let empty = self.surfaces.borrow().is_empty();
        if empty {
            completion();
            return;
        }
        self.is_settling.set(true);
        *self.landing.borrow_mut() = Some(Box::new(completion));
        let destination = if committed { self.completed_travel() } else { 0.0 };
        let mut offset = self.offset.get();
        offset.target(destination);
        // Carry the hand's speed, but only when it already points at the
        // destination: a kick the other way pushes even a critically damped
        // spring past its landing, and past the landing is the wrong page.
        let remaining = destination - offset.value();
        if remaining != 0.0 && (velocity < 0.0) == (remaining < 0.0) {
            offset.kick(velocity);
        }
        self.offset.set(offset);

        let anchor = self.surfaces.borrow().first().and_then(PaneDrag::pane);
        let Some(anchor) = anchor.filter(|anchor| anchor.window().is_some()) else {
            self.land(destination);
            return;
        };
        let existing = self.driver.borrow().clone();
        let driver = existing.unwrap_or_else(|| {
            let advancing = self.this.clone();
            let applying = self.this.clone();
            SpringDriver::new(
                &anchor,
                move |dt| advancing.upgrade().map(|this| this.tick(dt)).unwrap_or(false),
                move || {
                    if let Some(this) = applying.upgrade() {
                        this.apply();
                    }
                },
            )
        });
        *self.driver.borrow_mut() = Some(driver.clone());
        if !driver.arm() {
            self.land(destination);
        }
    }

    /// `release()`.
    pub fn release(&self) {
        self.is_settling.set(false);
        self.pending_landing.set(false);
        let landing = self.landing.borrow_mut().take();
        drop(landing);
        let driver = self.driver.borrow().clone();
        if let Some(driver) = driver {
            driver.park();
        }
        let finishing = std::mem::take(&mut *self.surfaces.borrow_mut());
        for surface in finishing {
            surface.release();
        }
    }

    fn land(&self, destination: CGFloat) {
        self.snap_offset(destination);
        self.pending_landing.set(false);
        let direction = self.direction.get();
        for surface in self.surfaces.borrow().iter() {
            surface.place(destination, direction);
        }
        self.finish();
    }

    fn tick(&self, dt: CGFloat) -> bool {
        let mut offset = self.offset.get();
        let moving = offset.advance(dt);
        self.offset.set(offset);
        if !moving {
            self.pending_landing.set(true);
        }
        moving
    }

    fn apply(&self) {
        let direction = self.direction.get();
        for surface in self.surfaces.borrow().iter() {
            surface.place(self.offset.get().value(), direction);
        }
        if self.pending_landing.get() {
            self.pending_landing.set(false);
            self.finish();
        }
    }

    fn finish(&self) {
        self.is_settling.set(false);
        let driver = self.driver.borrow().clone();
        if let Some(driver) = driver {
            driver.park();
        }
        let completion = self.landing.borrow_mut().take();
        if let Some(completion) = completion {
            completion();
        }
    }
}
