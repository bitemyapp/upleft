//! Port of `App/HistorySwipe.swift`: the ⇧ two-finger Back/Forward swipe —
//! its pure decisions (`HistorySwipePolicy`) and the coordinator that drives
//! it across every pane in a window.
//!
//! How the Swift maps:
//! - `HistorySwipePolicy` (a Swift `enum` namespace) is a unit struct;
//!   `HistorySwipePolicy.Direction` is [`Direction`] and
//!   `HistorySwipePolicy.Claim` is re-exported here as [`Claim`].
//! - `HistorySwipeCoordinator.Host`, a struct of closures, is [`Host`]; its
//!   defaulted `performHapticFeedback` is [`Host::default_perform_haptic_feedback`],
//!   and [`Host::new`] is the memberwise initialiser that takes the default.
//!   Swift's `move` is `r#move`.
//! - The coordinator is created as `Rc` (`HistorySwipeCoordinator::new`) so
//!   the settle completion's `[weak self]` holds a `std::rc::Weak`. Its state
//!   lives in `Cell`s, so a host closure may call back into it.

#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::Cell;
use std::rc::{Rc, Weak};

use objc2::rc::Retained;
use objc2_app_kit::{
    NSEvent, NSEventModifierFlags, NSEventPhase, NSHapticFeedbackManager, NSHapticFeedbackPattern,
    NSHapticFeedbackPerformanceTime, NSHapticFeedbackPerformer,
};
use objc2_core_foundation::CGFloat;
use upleft_render::appkit_compat::RectExt;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::view::markdown_container_view::MarkdownContainerView;

pub use crate::app::document_scroll_gestures::Claim;
use crate::app::document_scroll_gestures::{
    DocumentSwipePhysics, PaneGiveTrack, ScrollGestureHandler, ScrollGestureModifiers, TimeInterval, swift_max,
};

/// `HistorySwipePolicy.Direction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Back,
    Forward,
}

/// Decisions for the ⇧ two-finger Back/Forward swipe, kept pure so the feel
/// can be tuned and tested without a trackpad under it.
///
/// Bare two fingers sideways already switch Document↔Source, so this needs a
/// modifier, and ⇧ is the one that is free. The direction follows Safari
/// exactly: push the page right, uncovering what sits to its left, and you go
/// back.
pub struct HistorySwipePolicy;

impl HistorySwipePolicy {
    /// Held for the whole of the deciding part of the gesture. Tested for
    /// equality, so ⇧⌘ is not this gesture and never half-starts it.
    pub const MODIFIERS: NSEventModifierFlags = NSEventModifierFlags::Shift;

    /// `isSpelledCorrectly(_:)`.
    pub fn is_spelled_correctly(held: NSEventModifierFlags) -> bool {
        held == Self::MODIFIERS
    }

    /// Horizontal travel that claims the gesture away from vertical scrolling.
    /// A shade longer than the presentation swipe's.
    pub const INTENT_THRESHOLD: CGFloat = 14.0;
    /// How far the horizontal component must beat the vertical one.
    pub const AXIS_DOMINANCE: CGFloat = 1.5;
    /// Share of the pane that commits on release, bounded at both ends.
    pub const COMMIT_FRACTION: CGFloat = 0.2;
    pub const MINIMUM_COMMIT_DISTANCE: CGFloat = 56.0;
    pub const MAXIMUM_COMMIT_DISTANCE: CGFloat = 120.0;
    /// Points per second that commits a short swipe — the flick.
    pub const FLICK_VELOCITY: CGFloat = 260.0;
    /// How far the page travels under the fingers.
    pub const MAXIMUM_GIVE: CGFloat = 30.0;

    /// `claim(horizontal:vertical:)`.
    pub fn claim(horizontal: CGFloat, vertical: CGFloat) -> Claim {
        DocumentSwipePhysics::claim(horizontal, vertical, Self::INTENT_THRESHOLD, Self::AXIS_DOMINANCE)
    }

    /// Where a swipe of this translation is heading. Positive pushes the page
    /// right and uncovers what sits to its left, which is where you have
    /// already been.
    pub fn direction(translation: CGFloat) -> Option<Direction> {
        if translation > 0.0 {
            return Some(Direction::Back);
        }
        if translation < 0.0 {
            return Some(Direction::Forward);
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

/// `HistorySwipeCoordinator.Host`: what the coordinator needs from the
/// window, as closures rather than a back-reference.
pub struct Host {
    pub panes: Box<dyn Fn() -> Vec<Retained<MarkdownContainerView>>>,
    pub style_sheet: Box<dyn Fn() -> Rc<StyleSheet>>,
    /// Whether jump history has anywhere to go that way. Asked before the
    /// gesture is claimed, so a swipe into an empty stack never catches.
    pub can_move: Box<dyn Fn(Direction) -> bool>,
    /// Take the step. Called once, on release, and never speculatively.
    pub r#move: Box<dyn Fn(Direction)>,
    /// The detent when the swipe passes the point where releasing would
    /// commit — the same `.alignment` tap the presentation rail uses.
    pub perform_haptic_feedback: Box<dyn Fn()>,
}

impl Host {
    /// The memberwise initialiser with `performHapticFeedback` defaulted.
    pub fn new(
        panes: impl Fn() -> Vec<Retained<MarkdownContainerView>> + 'static,
        style_sheet: impl Fn() -> Rc<StyleSheet> + 'static,
        can_move: impl Fn(Direction) -> bool + 'static,
        r#move: impl Fn(Direction) + 'static,
    ) -> Host {
        Host {
            panes: Box::new(panes),
            style_sheet: Box::new(style_sheet),
            can_move: Box::new(can_move),
            r#move: Box::new(r#move),
            perform_haptic_feedback: Host::default_perform_haptic_feedback(),
        }
    }

    /// `performHapticFeedback`'s default:
    /// `NSHapticFeedbackManager.defaultPerformer.perform(.alignment, performanceTime: .now)`.
    pub fn default_perform_haptic_feedback() -> Box<dyn Fn()> {
        Box::new(|| {
            NSHapticFeedbackManager::defaultPerformer()
                .performFeedbackPattern_performanceTime(NSHapticFeedbackPattern::Alignment, NSHapticFeedbackPerformanceTime::Now);
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Undecided,
    Scrolling,
    Swiping,
    Settling,
}

/// Drives the ⇧ two-finger Back/Forward swipe across every pane in a window.
///
/// The window owns jump history; this owns the gesture. Like its sibling it
/// renders nothing during the gesture: the page gives against the fingers,
/// and the trip happens on release.
pub struct HistorySwipeCoordinator {
    this: Weak<HistorySwipeCoordinator>,
    host: Host,
    state: Cell<State>,
    horizontal_travel: Cell<CGFloat>,
    vertical_travel: Cell<CGFloat>,
    translation: Cell<CGFloat>,
    velocity: Cell<CGFloat>,
    last_timestamp: Cell<TimeInterval>,
    pane_width: Cell<CGFloat>,
    direction: Cell<Direction>,
    passed_commit_point: Cell<bool>,
    swallows_momentum: Cell<bool>,
    gives: Rc<PaneGiveTrack>,
}

impl HistorySwipeCoordinator {
    /// `init(host:)`.
    pub fn new(host: Host) -> Rc<HistorySwipeCoordinator> {
        Rc::new_cyclic(|this| HistorySwipeCoordinator {
            this: this.clone(),
            host,
            state: Cell::new(State::Idle),
            horizontal_travel: Cell::new(0.0),
            vertical_travel: Cell::new(0.0),
            translation: Cell::new(0.0),
            velocity: Cell::new(0.0),
            last_timestamp: Cell::new(0.0),
            pane_width: Cell::new(0.0),
            direction: Cell::new(Direction::Back),
            passed_commit_point: Cell::new(false),
            swallows_momentum: Cell::new(false),
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

    /// Visible for tests: which way a claimed swipe is heading.
    pub fn tracked_direction_for_testing(&self) -> Option<Direction> {
        if self.is_tracking() { Some(self.direction.get()) } else { None }
    }

    /// The single entry point from the chain. Returns `true` when the swipe
    /// has taken the event, in which case the scroll view must not see it.
    pub fn handle(&self, event: &NSEvent) -> bool {
        // Trackpads only. A wheel's ⇧-scroll is a single unphased notch with
        // no travel to measure and no release to commit on.
        if !event.hasPreciseScrollingDeltas() {
            return false;
        }

        // Momentum arrives after the fingers are up. There is nothing left to
        // track, and it must never buy a second trip — but it is swallowed
        // rather than handed back, because the coast belongs to a gesture that
        // has already been answered.
        if !event.momentumPhase().is_empty() {
            let swallow = self.swallows_momentum.get() || self.state.get() == State::Settling;
            if !event.momentumPhase().intersection(NSEventPhase::Ended | NSEventPhase::Cancelled).is_empty() {
                self.swallows_momentum.set(false);
            }
            return swallow;
        }

        // Fingers back down ends the last gesture's coast, whatever this new
        // one turns out to be spelled with.
        if event.phase() == NSEventPhase::Began {
            self.swallows_momentum.set(false);
        }

        // ⇧ has to be held while the gesture is being decided. Once it has
        // caught, letting go of ⇧ must not drop it.
        if !self.is_claiming_gesture() && !HistorySwipePolicy::is_spelled_correctly(ScrollGestureModifiers::held(event)) {
            // A gesture that started with ⇧ and lost it is over, not paused.
            if self.state.get() == State::Undecided {
                self.state.set(State::Scrolling);
            }
            return false;
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
        self.gives.release();
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
        self.passed_commit_point.set(false);
        self.swallows_momentum.set(false);
        self.last_timestamp.set(timestamp);
    }

    fn track(&self, event: &NSEvent) -> bool {
        match self.state.get() {
            State::Idle | State::Scrolling | State::Settling => return false,
            State::Undecided => {
                self.horizontal_travel.set(self.horizontal_travel.get() + event.scrollingDeltaX());
                self.vertical_travel.set(self.vertical_travel.get() + event.scrollingDeltaY());
                match HistorySwipePolicy::claim(self.horizontal_travel.get(), self.vertical_travel.get()) {
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
        let Some(heading) = HistorySwipePolicy::direction(self.translation.get()) else { return false };
        // Nothing that way: hand the gesture back rather than promise a trip
        // that cannot happen.
        if !(self.host.can_move)(heading) {
            return false;
        }
        let widest = swift_max((self.host.panes)().iter().map(|pane| pane.bounds().width())).unwrap_or(0.0);
        if !(widest > 1.0) {
            return false;
        }

        self.direction.set(heading);
        self.pane_width.set(widest);
        self.state.set(State::Swiping);
        // From here the coast after the fingers lift belongs to this gesture,
        // whichever way it ends.
        self.swallows_momentum.set(true);
        if !(self.host.style_sheet)().reduce_motion {
            self.gives.engage(&(self.host.panes)());
        }
        true
    }

    fn apply_translation(&self) {
        self.gives.place(HistorySwipePolicy::give(self.translation.get()));
        // The detent, once per crossing. Coming back under it re-arms, so a
        // reader hovering at the threshold feels the line rather than a burst.
        let past = self.translation.get().abs() >= HistorySwipePolicy::commit_distance(self.pane_width.get());
        if past != self.passed_commit_point.get() {
            self.passed_commit_point.set(past);
            if past {
                (self.host.perform_haptic_feedback)();
            }
        }
    }

    fn end_gesture(&self, cancelled: bool) -> bool {
        if self.state.get() != State::Swiping {
            if self.state.get() != State::Settling {
                self.state.set(State::Idle);
            }
            return false;
        }
        let commit = !cancelled
            && HistorySwipePolicy::should_commit(self.translation.get(), self.velocity.get(), self.pane_width.get())
            // Re-asked at the last moment: a trip that silently does nothing
            // is a worse failure than one that never starts.
            && (self.host.can_move)(self.direction.get());

        if commit {
            // Put the page back before the trip: the destination scroll
            // animates from the pane's resting frame and must not start from
            // a translated layer.
            self.gives.release();
            self.state.set(State::Idle);
            (self.host.r#move)(self.direction.get());
            return true;
        }

        // Nothing was moved, so nothing has to be undone — an abandoned swipe
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
        self.gives.release();
    }
}

impl ScrollGestureHandler for HistorySwipeCoordinator {
    fn handle(&self, event: &NSEvent) -> bool {
        HistorySwipeCoordinator::handle(self, event)
    }

    fn is_claiming_gesture(&self) -> bool {
        HistorySwipeCoordinator::is_claiming_gesture(self)
    }
}
