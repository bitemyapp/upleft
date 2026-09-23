//! Port of `App/DocumentScrollGestures.swift`: the plumbing every
//! scroll-driven gesture over the document surface shares.
//!
//! `MarkdownTextView` offers the host every scroll event before the scroll
//! view sees it, and the host has more than one answer: a modifier turns the
//! wheel into a zoom, ⇧ and two fingers move through jump history, and two
//! bare fingers switch Document↔Source. They are asked in order and the first
//! to claim wins.
//!
//! What lives here is only what is genuinely common: the ordering rule, the
//! arithmetic of a sideways swipe, and the page's give. Every threshold stays
//! with the gesture it describes.
//!
//! How the Swift maps:
//! - `ScrollGestureModifiers` and `DocumentSwipePhysics` (Swift `enum`
//!   namespaces) are unit structs with associated functions and constants;
//!   `DocumentSwipePhysics.Claim` is [`Claim`].
//! - The `ScrollGestureHandler` protocol is a trait carrying the protocol
//!   extension's default for `is_claiming_gesture`; the chain holds its
//!   handlers as `Rc<dyn ScrollGestureHandler>` (Swift's strong `[any …]`).
//! - `PaneGive` holds its pane weakly (`objc2::rc::Weak`), as Swift's `weak
//!   var pane` does. `PaneGiveTrack` is shared as `Rc`, so the spring
//!   driver's `[weak self]` closures hold a `std::rc::Weak` to it; its stored
//!   properties are `Cell`/`RefCell`s, never borrowed across a call out.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2_app_kit::{NSEvent, NSEventModifierFlags};
use objc2_core_foundation::CGFloat;
use objc2_quartz_core::{CATransaction, CATransform3D, CATransform3DIdentity};
use upleft_render::appkit_compat::RectExt;
use upleft_render::motion::{self, SpringDriver, SpringScalar};
use upleft_render::swift_compat::{smax, smin};
use upleft_render::view::markdown_container_view::MarkdownContainerView;

/// Swift's `TimeInterval`.
pub type TimeInterval = f64;

// MARK: - Modifiers

/// The modifiers that can change what a scroll over the document means.
///
/// Caps Lock, fn and the numeric-pad flag ride along on perfectly ordinary
/// events and must never decide a gesture, so every handler tests the four
/// that matter and only those — and tests them for equality, not membership,
/// or ⇧⌘-scroll would fire two gestures that disagree about the page.
pub struct ScrollGestureModifiers;

impl ScrollGestureModifiers {
    /// `considered`: `[.command, .option, .control, .shift]`.
    pub const CONSIDERED: NSEventModifierFlags = NSEventModifierFlags(
        NSEventModifierFlags::Command.0
            | NSEventModifierFlags::Option.0
            | NSEventModifierFlags::Control.0
            | NSEventModifierFlags::Shift.0,
    );

    /// `held(in:)`.
    pub fn held(event: &NSEvent) -> NSEventModifierFlags {
        event.modifierFlags().intersection(Self::CONSIDERED)
    }
}

// MARK: - Chain

/// One gesture the document surface can hand a scroll event to.
pub trait ScrollGestureHandler {
    /// Offer the event. Returning `true` consumes it and the scroll view
    /// never sees it; a handler that is still deciding must return `false` so
    /// ordinary scrolling never waits on the decision.
    fn handle(&self, event: &NSEvent) -> bool;

    /// `true` while this handler is physically in the middle of a gesture it
    /// has already taken. A handler that says so is routed the rest of the
    /// gesture on its own: the alternative is a modifier pressed mid-swipe
    /// handing the events to somebody else and leaving a translated pane
    /// nobody owns.
    fn is_claiming_gesture(&self) -> bool {
        false
    }
}

/// The document surface's gestures, in the order they get to claim an event.
///
/// Order is a real decision, not a list. The modifier zooms decide on a
/// single event, so they can be asked first and answer immediately. The two
/// swipes need travel before they can tell themselves apart from a scroll, and
/// while they are deciding they hand the event back — which is exactly why the
/// one that needs ⇧ has to be asked before the one that needs nothing, and why
/// the bare swipe stands down whenever a modifier is held.
pub struct ScrollGestureChain {
    handlers: Vec<Rc<dyn ScrollGestureHandler>>,
}

impl ScrollGestureChain {
    /// `init(_:)`.
    pub fn new(handlers: Vec<Rc<dyn ScrollGestureHandler>>) -> ScrollGestureChain {
        ScrollGestureChain { handlers }
    }

    /// `handle(_:)`.
    pub fn handle(&self, event: &NSEvent) -> bool {
        // A gesture that has already caught keeps every remaining event of it,
        // including the ones it will decline. Polling from the top here would
        // let a late modifier steal a swipe that is already on screen.
        if let Some(owner) = self.handlers.iter().find(|handler| handler.is_claiming_gesture()) {
            return owner.handle(event);
        }
        for handler in &self.handlers {
            if handler.handle(event) {
                return true;
            }
        }
        false
    }
}

// MARK: - Swipe physics

/// `DocumentSwipePhysics.Claim`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    /// Still ambiguous. The event belongs to the scroll view for now, so
    /// vertical scrolling never waits on this decision.
    Undecided,
    /// A sideways gesture: the document surface stops scrolling.
    Swipe,
    /// Ordinary scrolling. Stop testing for the rest of the gesture.
    Scroll,
}

/// The arithmetic behind a sideways swipe over the document surface.
///
/// Stateless and parameterised: each gesture states its own numbers and gets
/// the same physics, so the Document↔Source swipe and the Back/Forward swipe
/// resolve an axis, a commit and a give the same way while feeling like the
/// different things they are.
pub struct DocumentSwipePhysics;

impl DocumentSwipePhysics {
    /// `claim(horizontal:vertical:intentThreshold:axisDominance:)`.
    pub fn claim(horizontal: CGFloat, vertical: CGFloat, intent_threshold: CGFloat, axis_dominance: CGFloat) -> Claim {
        let across = horizontal.abs();
        let along = vertical.abs();
        if across >= intent_threshold && across >= along * axis_dominance {
            return Claim::Swipe;
        }
        if along >= intent_threshold {
            return Claim::Scroll;
        }
        Claim::Undecided
    }

    /// A share of the pane, bounded at both ends: a narrow split pane must not
    /// commit on a twitch, and a full-width window must not demand a swipe
    /// longer than the trackpad.
    pub fn commit_distance(pane_width: CGFloat, fraction: CGFloat, minimum: CGFloat, maximum: CGFloat) -> CGFloat {
        smin(smax(pane_width * fraction, minimum), maximum)
    }

    /// `shouldCommit(translation:velocity:distance:flickVelocity:)`.
    pub fn should_commit(translation: CGFloat, velocity: CGFloat, distance: CGFloat, flick_velocity: CGFloat) -> bool {
        // `guard translation != 0`: NaN passes, as in Swift.
        if translation == 0.0 {
            return false;
        }
        if translation.abs() >= distance {
            return true;
        }
        // A flick commits even when short — but a reversal at the end of the
        // gesture means "put it back", however fast the hand was moving.
        velocity.abs() >= flick_velocity && (velocity < 0.0) == (translation < 0.0)
    }

    /// The page's give: asymptotic, so it always answers the fingers and never
    /// arrives anywhere. A linear give would need a clamp, and a clamp is a
    /// dead stop the hand can feel.
    pub fn give(translation: CGFloat, limit: CGFloat) -> CGFloat {
        if !(limit > 0.0) {
            return 0.0;
        }
        let sign: CGFloat = if translation < 0.0 { -1.0 } else { 1.0 };
        let distance = translation.abs();
        sign * limit * (1.0 - 1.0 / (distance / (limit * 0.9) + 1.0))
    }

    /// Smoothed instantaneous velocity, in points per second.
    ///
    /// One 8 ms frame is far too short a window to tell a flick from a jitter
    /// at the end of a slow drag, so each sample only moves the estimate part
    /// of the way.
    pub fn velocity(current: CGFloat, delta: CGFloat, elapsed: TimeInterval) -> CGFloat {
        if !(elapsed > 0.0005) {
            return current;
        }
        let instantaneous = delta / elapsed;
        if current == 0.0 { instantaneous } else { current * 0.6 + instantaneous * 0.4 }
    }
}

/// `Sequence.max()` over `CGFloat`: the first element, replaced by each later
/// element it is `<`.
pub(crate) fn swift_max(values: impl IntoIterator<Item = CGFloat>) -> Option<CGFloat> {
    let mut iterator = values.into_iter();
    let mut result = iterator.next()?;
    for element in iterator {
        if result < element {
            result = element;
        }
    }
    Some(result)
}

/// `CATransform3DIdentity`.
pub(crate) fn transform_identity() -> CATransform3D {
    // SAFETY: an immutable QuartzCore global.
    unsafe { CATransform3DIdentity }
}

// MARK: - Give

/// One pane's give: the live text surface translated against the fingers.
///
/// A layer transform, not a frame change and not a snapshot — nothing is
/// re-rendered, nothing is laid out, and the compositor does the whole job.
/// The gutter and footnote rail stay put, the way a scroller does: they
/// annotate the page rather than travel with it.
pub struct PaneGive {
    pane: ObjcWeak<MarkdownContainerView>,
    was_masking: bool,
}

impl PaneGive {
    /// `init?(pane:)`.
    pub fn new(pane: &MarkdownContainerView) -> Option<PaneGive> {
        if !(pane.bounds().width() > 1.0) {
            return None;
        }
        // Asked for rather than assumed. The document window is layer-backed
        // in practice, but a give that silently does nothing when it is not
        // is worse than a one-off backing store on the first swipe.
        pane.setWantsLayer(true);
        pane.scroll_view().setWantsLayer(true);
        let pane_layer = pane.layer()?;
        pane.scroll_view().layer()?;
        let weak_pane = ObjcWeak::from(pane);
        // The page has to be clipped to its own frame while it travels, or the
        // give paints over whatever sits beside it.
        let was_masking = pane_layer.masksToBounds();
        pane_layer.setMasksToBounds(true);
        Some(PaneGive { pane: weak_pane, was_masking })
    }

    /// `pane` (`private(set) weak var`).
    pub fn pane(&self) -> Option<Retained<MarkdownContainerView>> {
        self.pane.load()
    }

    /// `place(_:)`.
    pub fn place(&self, offset: CGFloat) {
        let Some(layer) = self.pane.load().and_then(|pane| pane.scroll_view().layer()) else { return };
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        layer.setTransform(CATransform3D::new_translation(offset, 0.0, 0.0));
        CATransaction::commit();
    }

    /// `release()`.
    pub fn release(&self) {
        let Some(pane) = self.pane.load() else { return };
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        if let Some(layer) = pane.scroll_view().layer() {
            layer.setTransform(transform_identity());
        }
        if let Some(layer) = pane.layer() {
            layer.setMasksToBounds(self.was_masking);
        }
        CATransaction::commit();
    }
}

/// Every pane's give at once, plus the spring that puts them back.
///
/// A sideways gesture over a split window moves both panes together — the mode
/// and the reading position are the window's, not one pane's — and an
/// abandoned gesture owes the reader a return, not a snap. Both swipes want
/// exactly this, so neither of them owns it.
pub struct PaneGiveTrack {
    this: Weak<PaneGiveTrack>,
    surfaces: RefCell<Vec<PaneGive>>,
    driver: RefCell<Option<SpringDriver>>,
    /// Critically damped: the page returning past its own resting place would
    /// read as a bounce nothing caused.
    offset: Cell<SpringScalar>,
    landing: RefCell<Option<Box<dyn FnOnce()>>>,
    pending_landing: Cell<bool>,
    is_settling: Cell<bool>,
}

impl Drop for PaneGiveTrack {
    /// `deinit { driver?.park() }`.
    fn drop(&mut self) {
        if let Some(driver) = self.driver.get_mut().as_ref() {
            driver.park();
        }
    }
}

impl PaneGiveTrack {
    /// `PaneGiveTrack()`.
    pub fn new() -> Rc<PaneGiveTrack> {
        Rc::new_cyclic(|this| PaneGiveTrack {
            this: this.clone(),
            surfaces: RefCell::new(Vec::new()),
            driver: RefCell::new(None),
            offset: Cell::new(SpringScalar::with_value(0.0, motion::SPRING_QUICK)),
            landing: RefCell::new(None),
            pending_landing: Cell::new(false),
            is_settling: Cell::new(false),
        })
    }

    /// `true` from `settle(completion:)` until the panes are back at rest.
    pub fn is_settling(&self) -> bool {
        self.is_settling.get()
    }

    /// `isEngaged`.
    pub fn is_engaged(&self) -> bool {
        !self.surfaces.borrow().is_empty()
    }

    fn snap_offset(&self, value: CGFloat) {
        let mut offset = self.offset.get();
        offset.snap(value);
        self.offset.set(offset);
    }

    /// Take hold of the panes. Nothing is rendered and nothing is laid out;
    /// the cost is one backing store per pane, once.
    pub fn engage(&self, panes: &[Retained<MarkdownContainerView>]) {
        self.release();
        let surfaces: Vec<PaneGive> = panes.iter().filter_map(|pane| PaneGive::new(pane)).collect();
        *self.surfaces.borrow_mut() = surfaces;
        self.snap_offset(0.0);
    }

    /// `place(_:)`.
    pub fn place(&self, value: CGFloat) {
        self.snap_offset(value);
        for surface in self.surfaces.borrow().iter() {
            surface.place(value);
        }
    }

    /// Spring the panes home and call `completion` when they arrive. With no
    /// give in flight there is nothing to wait for, so `completion` runs now.
    pub fn settle(&self, completion: impl FnOnce() + 'static) {
        let empty = self.surfaces.borrow().is_empty();
        if empty {
            completion();
            return;
        }
        self.is_settling.set(true);
        *self.landing.borrow_mut() = Some(Box::new(completion));
        let mut offset = self.offset.get();
        offset.target(0.0);
        self.offset.set(offset);
        let anchor = self.surfaces.borrow().first().and_then(PaneGive::pane);
        let Some(anchor) = anchor.filter(|anchor| anchor.window().is_some()) else {
            self.land();
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
            self.land();
        }
    }

    /// Hand the panes back where they were with no spring at all. A commit
    /// takes this path: the transition that follows draws from the pane's
    /// resting frame and must not start from a translated layer.
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

    /// No display link to spring on — off-screen, or a window already gone.
    fn land(&self) {
        self.snap_offset(0.0);
        self.pending_landing.set(false);
        for surface in self.surfaces.borrow().iter() {
            surface.place(0.0);
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
        for surface in self.surfaces.borrow().iter() {
            surface.place(self.offset.get().value());
        }
        if self.pending_landing.get() {
            self.pending_landing.set(false);
            self.finish();
        }
    }

    fn finish(&self) {
        if !self.is_settling.get() {
            return;
        }
        let completion = self.landing.borrow_mut().take();
        self.is_settling.set(false);
        if let Some(completion) = completion {
            completion();
        }
    }
}
