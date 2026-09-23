//! Port of `App/PresentationSwipe.swift`: the two-finger Document↔Source
//! swipe — its pure decisions (`PresentationSwipePolicy`) and the coordinator
//! that drives it across every pane in a window.
//!
//! How the Swift maps:
//! - `PresentationSwipePolicy` (a Swift `enum` namespace) is a unit struct;
//!   `PresentationSwipePolicy.Claim` is re-exported here as [`Claim`].
//! - `PresentationSwipeCoordinator.Host`, a struct of closures, is [`Host`].
//!   Segments are Swift `Int`s, so `isize`.
//! - The coordinator is created as `Rc` (`PresentationSwipeCoordinator::new`)
//!   so the settle completions' `[weak self]` hold a `std::rc::Weak`. Its
//!   state lives in `Cell`s, so a host closure may call back into it (for
//!   example `cancel_in_flight` from a resize) at any point.

use std::cell::Cell;
use std::rc::{Rc, Weak};

use objc2::rc::Retained;
use objc2_app_kit::{NSEvent, NSEventPhase};
use objc2_core_foundation::CGFloat;
use upleft_render::appkit_compat::RectExt;
use upleft_render::swift_compat::{smax, smin};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::view::markdown_container_view::MarkdownContainerView;

pub use crate::app::document_scroll_gestures::Claim;
use crate::app::document_scroll_gestures::{
    DocumentSwipePhysics, PaneGiveTrack, ScrollGestureHandler, ScrollGestureModifiers, TimeInterval, swift_max,
};
use crate::app::presentation_drag::{PaneDragTrack, PresentationSwitchBudget};

/// Decisions for the two-finger Document↔Source swipe, kept pure so the feel
/// can be tuned and tested without a trackpad under it.
///
/// Nothing is rendered during the give: the rail's indicator is welded to the
/// fingers, the page gives against them, and the switch happens on release.
pub struct PresentationSwipePolicy;

impl PresentationSwipePolicy {
    /// Horizontal travel that claims the gesture away from vertical scrolling.
    pub const INTENT_THRESHOLD: CGFloat = 12.0;
    /// How far the horizontal component must beat the vertical one.
    pub const AXIS_DOMINANCE: CGFloat = 1.4;
    /// Share of the pane that commits on release, bounded at both ends.
    pub const COMMIT_FRACTION: CGFloat = 0.25;
    pub const MINIMUM_COMMIT_DISTANCE: CGFloat = 64.0;
    pub const MAXIMUM_COMMIT_DISTANCE: CGFloat = 140.0;
    /// Points per second that commits a short swipe — the flick.
    pub const FLICK_VELOCITY: CGFloat = 260.0;
    /// How far the page itself travels under the fingers.
    pub const MAXIMUM_GIVE: CGFloat = 28.0;

    /// `claim(horizontal:vertical:)`.
    pub fn claim(horizontal: CGFloat, vertical: CGFloat) -> Claim {
        DocumentSwipePhysics::claim(horizontal, vertical, Self::INTENT_THRESHOLD, Self::AXIS_DOMINANCE)
    }

    /// Which segment a swipe is heading for: `0` Document, `1` Source.
    ///
    /// `translation` is accumulated `scrollingDeltaX`, which AppKit has
    /// already flipped for the trackpad's scroll-direction preference.
    pub fn target_segment(translation: CGFloat) -> Option<isize> {
        if translation < 0.0 {
            return Some(1);
        }
        if translation > 0.0 {
            return Some(0);
        }
        None
    }

    /// `commitDistance(paneWidth:)`.
    pub fn commit_distance(pane_width: CGFloat) -> CGFloat {
        DocumentSwipePhysics::commit_distance(
            pane_width,
            Self::COMMIT_FRACTION,
            Self::MINIMUM_COMMIT_DISTANCE,
            Self::MAXIMUM_COMMIT_DISTANCE,
        )
    }

    /// How far through the switch the rail's indicator should read, `0`…`1`.
    /// It reaches the far segment exactly where releasing would commit.
    pub fn rail_progress(translation: CGFloat, pane_width: CGFloat) -> CGFloat {
        let distance = Self::commit_distance(pane_width);
        if !(distance > 0.0) {
            return 0.0;
        }
        smin(1.0, translation.abs() / distance)
    }

    /// That progress placed on the rail, which runs `0` Document to `1` Source.
    pub fn rail_position(progress: CGFloat, origin: isize, target: isize) -> CGFloat {
        let start = origin.max(0).min(1) as CGFloat;
        let end = target.max(0).min(1) as CGFloat;
        start + (end - start) * smin(smax(progress, 0.0), 1.0)
    }

    /// `shouldCommit(translation:velocity:paneWidth:)`.
    pub fn should_commit(translation: CGFloat, velocity: CGFloat, pane_width: CGFloat) -> bool {
        DocumentSwipePhysics::should_commit(
            translation,
            velocity,
            Self::commit_distance(pane_width),
            Self::FLICK_VELOCITY,
        )
    }

    /// `give(_:)`.
    pub fn give(translation: CGFloat) -> CGFloat {
        DocumentSwipePhysics::give(translation, Self::MAXIMUM_GIVE)
    }
}

// MARK: - Coordinator

/// `PresentationSwipeCoordinator.Host`: what the coordinator needs from the
/// window, as closures rather than a back-reference.
pub struct Host {
    pub panes: Box<dyn Fn() -> Vec<Retained<MarkdownContainerView>>>,
    pub style_sheet: Box<dyn Fn() -> Rc<StyleSheet>>,
    /// The presentation the document is actually in — `0` Document,
    /// `1` Source — not what the rail happens to be drawing mid-swipe.
    pub selected_segment: Box<dyn Fn() -> isize>,
    /// Switch presentation, with the same transition the toolbar rail uses.
    /// Called once, on release, and never speculatively.
    pub commit_segment: Box<dyn Fn(isize)>,
    /// Move the rail's indicator to `position` (`0` Document, `1` Source).
    pub track_rail: Box<dyn Fn(CGFloat)>,
    /// Land the rail on a segment.
    pub settle_rail: Box<dyn Fn(isize)>,
    /// The open document's length, which decides whether the drag is
    /// affordable — see `PresentationSwitchBudget`.
    pub document_lines: Box<dyn Fn() -> isize>,
    /// Switch presentation with no transition of its own. The drag makes the
    /// change behind a still at the top of the gesture, and puts it back the
    /// same way if the gesture is abandoned.
    pub set_segment: Box<dyn Fn(isize)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Undecided,
    Scrolling,
    Swiping,
    Settling,
}

/// Drives the two-finger Document↔Source swipe across every pane in a window
/// and keeps the titlebar rail's indicator travelling with it.
///
/// The window owns the mode; this owns the gesture.
pub struct PresentationSwipeCoordinator {
    this: Weak<PresentationSwipeCoordinator>,
    host: Host,
    state: Cell<State>,
    horizontal_travel: Cell<CGFloat>,
    vertical_travel: Cell<CGFloat>,
    translation: Cell<CGFloat>,
    velocity: Cell<CGFloat>,
    last_timestamp: Cell<TimeInterval>,
    pane_width: Cell<CGFloat>,
    origin_segment: Cell<isize>,
    target_segment: Cell<isize>,
    /// Two ways to answer the fingers, chosen per gesture by what the
    /// document can afford. Under budget the real drag; over budget the give.
    /// Never both.
    drag: Rc<PaneDragTrack>,
    gives: Rc<PaneGiveTrack>,
}

impl PresentationSwipeCoordinator {
    /// `init(host:)`.
    pub fn new(host: Host) -> Rc<PresentationSwipeCoordinator> {
        Rc::new_cyclic(|this| PresentationSwipeCoordinator {
            this: this.clone(),
            host,
            state: Cell::new(State::Idle),
            horizontal_travel: Cell::new(0.0),
            vertical_travel: Cell::new(0.0),
            translation: Cell::new(0.0),
            velocity: Cell::new(0.0),
            last_timestamp: Cell::new(0.0),
            pane_width: Cell::new(0.0),
            origin_segment: Cell::new(0),
            target_segment: Cell::new(0),
            drag: PaneDragTrack::new(),
            gives: PaneGiveTrack::new(),
        })
    }

    /// `isTracking`.
    pub fn is_tracking(&self) -> bool {
        self.state.get() == State::Swiping
    }

    /// `isSettling`.
    pub fn is_settling(&self) -> bool {
        self.state.get() == State::Settling
    }

    /// `isClaimingGesture`.
    pub fn is_claiming_gesture(&self) -> bool {
        self.is_tracking() || self.is_settling()
    }

    /// The single entry point from the text surface. Returns `true` when the
    /// swipe has taken the event, in which case the scroll view must not see
    /// it.
    pub fn handle(&self, event: &NSEvent) -> bool {
        // Trackpads only. A wheel with a horizontal tilt has no phases to
        // build an interactive gesture out of.
        if !event.hasPreciseScrollingDeltas() {
            return false;
        }

        // This is the gesture spelled with nothing held. Once it *has* caught,
        // a modifier pressed halfway through changes nothing.
        if !self.is_claiming_gesture() && !ScrollGestureModifiers::held(event).is_empty() {
            return false;
        }

        // Momentum arrives after the fingers are up. There is nothing left to
        // track — but swallow it while the page settles.
        if !event.momentumPhase().is_empty() {
            return self.state.get() == State::Settling;
        }

        let phase = event.phase();
        if phase == NSEventPhase::Began {
            self.begin_gesture(event.timestamp());
            false
        } else if phase == NSEventPhase::Changed {
            self.track(event)
        } else if phase == NSEventPhase::Ended || phase == NSEventPhase::Cancelled {
            self.end_gesture(event.phase() == NSEventPhase::Cancelled)
        } else {
            self.state.get() == State::Swiping
        }
    }

    /// A resize reflows the page under the give, so the transform stops
    /// describing anything. Put the page back and let the gesture go.
    pub fn cancel_in_flight(&self) {
        let state = self.state.get();
        if !(state == State::Swiping || state == State::Settling) {
            return;
        }
        self.state.set(State::Idle);
        // A drag switched the mode speculatively at the top of the gesture.
        // Grounding it is not a commit, so put the mode back before letting go
        // of the stills that are hiding the change.
        if self.drag.is_engaged() && (self.host.selected_segment)() != self.origin_segment.get() {
            (self.host.set_segment)(self.origin_segment.get());
        }
        self.drag.release();
        self.gives.release();
        (self.host.settle_rail)((self.host.selected_segment)());
    }

    // MARK: - Gesture

    fn begin_gesture(&self, timestamp: TimeInterval) {
        // A new gesture during the settle abandons the tail rather than
        // fighting it: the reader has already started moving again.
        if self.state.get() == State::Settling {
            self.finish_settle();
        }
        self.state.set(State::Undecided);
        self.horizontal_travel.set(0.0);
        self.vertical_travel.set(0.0);
        self.translation.set(0.0);
        self.velocity.set(0.0);
        self.last_timestamp.set(timestamp);
    }

    fn track(&self, event: &NSEvent) -> bool {
        match self.state.get() {
            State::Idle | State::Scrolling | State::Settling => return false,
            State::Undecided => {
                self.horizontal_travel.set(self.horizontal_travel.get() + event.scrollingDeltaX());
                self.vertical_travel.set(self.vertical_travel.get() + event.scrollingDeltaY());
                match PresentationSwipePolicy::claim(self.horizontal_travel.get(), self.vertical_travel.get()) {
                    Claim::Undecided => return false,
                    Claim::Scroll => {
                        self.state.set(State::Scrolling);
                        return false;
                    }
                    Claim::Swipe => {
                        // The travel already spent deciding is real movement the
                        // reader made; start from there rather than dropping it.
                        self.translation.set(self.horizontal_travel.get());
                        self.last_timestamp.set(event.timestamp());
                        if !self.begin_swipe() {
                            self.state.set(State::Scrolling);
                            return false;
                        }
                    }
                }
            }
            State::Swiping => {
                self.translation.set(self.translation.get() + event.scrollingDeltaX());
                self.velocity.set(DocumentSwipePhysics::velocity(
                    self.velocity.get(),
                    event.scrollingDeltaX(),
                    event.timestamp() - self.last_timestamp.get(),
                ));
                self.last_timestamp.set(event.timestamp());
            }
        }

        self.apply_translation();
        true
    }

    fn begin_swipe(&self) -> bool {
        let Some(target) = PresentationSwipePolicy::target_segment(self.translation.get()) else { return false };
        self.origin_segment.set((self.host.selected_segment)());
        // Nothing sits beyond the mode you are already in, so there is no
        // switch to promise. Hand the gesture back.
        if target == self.origin_segment.get() {
            return false;
        }
        let widest = swift_max((self.host.panes)().iter().map(|pane| pane.bounds().width())).unwrap_or(0.0);
        if !(widest > 1.0) {
            return false;
        }

        self.target_segment.set(target);
        self.pane_width.set(widest);
        self.state.set(State::Swiping);

        // Reduce Motion asked for no travelling surfaces at all. Honour the
        // gesture, skip the choreography: the switch happens on release the
        // way a click on the rail does.
        if (self.host.style_sheet)().reduce_motion {
            return true;
        }

        // The drag has to render what it is dragging in before a finger moves.
        // Ask the budget whether this document can afford that inside a few
        // frames; if it cannot, the give is the right answer.
        if PresentationSwitchBudget::allows_drag((self.host.document_lines)())
            && self
                .drag
                .engage(&(self.host.panes)(), if self.translation.get() < 0.0 { -1.0 } else { 1.0 })
        {
            // The stills are covering every pane now, so the rebuild happens
            // behind them rather than under the fingers.
            (self.host.set_segment)(target);
        } else {
            self.gives.engage(&(self.host.panes)());
        }
        true
    }

    fn apply_translation(&self) {
        if self.drag.is_engaged() {
            // Welded to the fingers. The page is really going where you are
            // pushing it, so there is nothing to damp.
            self.drag.place(self.translation.get());
        } else {
            self.gives.place(PresentationSwipePolicy::give(self.translation.get()));
        }
        (self.host.track_rail)(PresentationSwipePolicy::rail_position(
            PresentationSwipePolicy::rail_progress(self.translation.get(), self.pane_width.get()),
            self.origin_segment.get(),
            self.target_segment.get(),
        ));
    }

    fn end_gesture(&self, cancelled: bool) -> bool {
        if self.state.get() != State::Swiping {
            if self.state.get() != State::Settling {
                self.state.set(State::Idle);
            }
            return false;
        }
        let commit = !cancelled
            && PresentationSwipePolicy::should_commit(
                self.translation.get(),
                self.velocity.get(),
                self.pane_width.get(),
            );
        (self.host.settle_rail)(if commit { self.target_segment.get() } else { self.origin_segment.get() });

        if self.drag.is_engaged() {
            // The mode was changed at the top of the gesture, so a commit has
            // nothing left to do but fly the outgoing page off. An abandoned
            // drag owes the reader the mode back.
            self.state.set(State::Settling);
            let this = self.this.clone();
            self.drag.settle(commit, self.velocity.get(), move || {
                let Some(this) = this.upgrade() else { return };
                if !commit && (this.host.selected_segment)() != this.origin_segment.get() {
                    (this.host.set_segment)(this.origin_segment.get());
                }
                this.drag.release();
                this.state.set(State::Idle);
            });
            return true;
        }

        if commit {
            // Put the page back before the switch: the mode change draws its
            // own transition from the pane's resting frame and must not start
            // from a translated layer.
            self.gives.release();
            self.state.set(State::Idle);
            (self.host.commit_segment)(self.target_segment.get());
            return true;
        }

        // Nothing was built, so nothing has to be undone — an abandoned swipe
        // costs one spring back to rest.
        if !self.gives.is_engaged() {
            self.state.set(State::Idle);
            return true;
        }
        self.state.set(State::Settling);
        let this = self.this.clone();
        self.gives.settle(move || {
            if let Some(this) = this.upgrade() {
                this.finish_settle();
            }
        });
        true
    }

    fn finish_settle(&self) {
        if self.state.get() != State::Settling {
            return;
        }
        self.state.set(State::Idle);
        self.drag.release();
        self.gives.release();
    }
}

impl ScrollGestureHandler for PresentationSwipeCoordinator {
    fn handle(&self, event: &NSEvent) -> bool {
        PresentationSwipeCoordinator::handle(self, event)
    }

    fn is_claiming_gesture(&self) -> bool {
        PresentationSwipeCoordinator::is_claiming_gesture(self)
    }
}
