//! Port of `App/ScrollZoom.swift`: ⌘-scroll and ⌥-scroll over the document
//! surface as text-size and structural-detail steps — the pure detent
//! (`ScrollZoomPolicy`) and the coordinator.
//!
//! How the Swift maps:
//! - `ScrollZoomPolicy` (a Swift `enum` namespace) is a unit struct;
//!   `ScrollZoomPolicy.Intent` is [`Intent`]. Steps are Swift `Int`s, so
//!   `isize`.
//! - `ScrollZoomCoordinator.Host`, a struct of closures, is [`Host`]; its
//!   defaulted `performHapticFeedback` is [`Host::default_perform_haptic_feedback`],
//!   and [`Host::new`] is the memberwise initialiser that takes the default.
//! - The coordinator is created as `Rc` (`ScrollZoomCoordinator::new`), like
//!   its siblings, so the window and the gesture chain can share it.

#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::Cell;
use std::rc::Rc;

use objc2_app_kit::{
    NSEvent, NSEventModifierFlags, NSEventPhase, NSHapticFeedbackManager, NSHapticFeedbackPattern,
    NSHapticFeedbackPerformanceTime, NSHapticFeedbackPerformer,
};
use objc2_core_foundation::CGFloat;
use upleft_render::swift_compat::{int_truncating, smax, smin};

use crate::app::document_scroll_gestures::{ScrollGestureHandler, ScrollGestureModifiers, TimeInterval};

/// `ScrollZoomPolicy.Intent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// ⌘ — the reading size of the text, the scale pinch already drives.
    TextSize,
    /// ⌥ — the structural detail level, the scale ⌃⌥⌘1…5 drives.
    StructuralDetail,
}

/// Decisions for the two modifier zooms on the wheel, kept pure so the detent
/// can be tuned and tested without a trackpad or a mouse under it.
///
/// Text size is a fine ramp with a cheap reflow; structural detail is five
/// stops, each a full relayout, so a gesture is allowed exactly one.
pub struct ScrollZoomPolicy;

impl ScrollZoomPolicy {
    /// Exactly ⌘ or exactly ⌥. ⇧⌘ belongs to jump history and ⌃⌥ belongs to
    /// nothing at all; a zoom that answered to any superset would fire
    /// underneath both of them.
    pub fn intent(held: NSEventModifierFlags) -> Option<Intent> {
        if held == NSEventModifierFlags::Command {
            return Some(Intent::TextSize);
        }
        if held == NSEventModifierFlags::Option {
            return Some(Intent::StructuralDetail);
        }
        None
    }

    /// Points of trackpad travel per text-size step.
    pub const TEXT_SIZE_STEP_POINTS: CGFloat = 26.0;

    /// Points of trackpad travel before the single structural step a gesture
    /// is allowed.
    pub const DETAIL_STEP_POINTS: CGFloat = 60.0;

    /// A coarse wheel measures in lines, not points, and its notches are
    /// already detents — so one notch is one step.
    pub const WHEEL_STEP_LINES: CGFloat = 1.0;

    /// Each wheel event contributes at most one notch, so the number of steps
    /// can only ever be the number of notches the hand actually turned.
    pub const MAXIMUM_WHEEL_LINES_PER_EVENT: CGFloat = 1.0;

    /// Structural steps a single gesture may fire.
    pub const DETAIL_STEPS_PER_GESTURE: isize = 1;

    /// A wheel has no phases, so "one gesture" has to be inferred from the
    /// quiet between events.
    pub const WHEEL_GESTURE_GAP: TimeInterval = 0.2;

    /// Travel per step, in whatever unit the device reports.
    pub fn step_threshold(intent: Intent, precise: bool) -> CGFloat {
        if !precise {
            return Self::WHEEL_STEP_LINES;
        }
        match intent {
            Intent::TextSize => Self::TEXT_SIZE_STEP_POINTS,
            Intent::StructuralDetail => Self::DETAIL_STEP_POINTS,
        }
    }

    /// What one event adds to the gesture's travel. Precise deltas are points
    /// and are taken as they come; coarse deltas lose macOS's acceleration.
    pub fn contribution(delta_y: CGFloat, precise: bool) -> CGFloat {
        if precise {
            return delta_y;
        }
        smin(smax(delta_y, -Self::MAXIMUM_WHEEL_LINES_PER_EVENT), Self::MAXIMUM_WHEEL_LINES_PER_EVENT)
    }

    /// Whether a gesture that has already fired `spent` steps may fire more.
    pub fn allows_further_steps(intent: Intent, spent: isize) -> bool {
        match intent {
            Intent::TextSize => true,
            Intent::StructuralDetail => spent.abs() < Self::DETAIL_STEPS_PER_GESTURE,
        }
    }

    /// The steps `accumulated` travel has earned, capped by what this gesture
    /// has left to spend. Positive is toward more: bigger text, more detail.
    pub fn steps(intent: Intent, accumulated: CGFloat, spent: isize, precise: bool) -> isize {
        let threshold = Self::step_threshold(intent, precise);
        if !(threshold > 0.0) || !Self::allows_further_steps(intent, spent) {
            return 0;
        }
        let earned = int_truncating(accumulated / threshold) as isize;
        if earned == 0 {
            return 0;
        }
        match intent {
            Intent::TextSize => earned,
            Intent::StructuralDetail => {
                // One event can cross the threshold twice over; it still only
                // buys the one step the gesture is allowed.
                let remaining = Self::DETAIL_STEPS_PER_GESTURE - spent.abs();
                (-remaining).max(remaining.min(earned))
            }
        }
    }

    /// Whether an event this far from the last one starts a fresh gesture.
    /// Only a coarse wheel has to ask — a trackpad says so in its phases.
    pub fn starts_new_wheel_gesture(elapsed: TimeInterval) -> bool {
        elapsed >= Self::WHEEL_GESTURE_GAP
    }
}

// MARK: - Coordinator

/// `ScrollZoomCoordinator.Host`: what the coordinator needs from the window.
pub struct Host {
    /// Step the reading text size — the path pinch-to-zoom already takes.
    pub step_text_size: Box<dyn Fn(isize)>,
    /// Step the structural detail level — the path `.zoomIn` / `.zoomOut`
    /// take, so the gesture and the chord cannot drift apart.
    pub step_detail: Box<dyn Fn(isize)>,
    /// The detent under the fingers when a detail level lands.
    pub perform_haptic_feedback: Box<dyn Fn()>,
}

impl Host {
    /// The memberwise initialiser with `performHapticFeedback` defaulted.
    pub fn new(step_text_size: impl Fn(isize) + 'static, step_detail: impl Fn(isize) + 'static) -> Host {
        Host {
            step_text_size: Box::new(step_text_size),
            step_detail: Box::new(step_detail),
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

/// Turns ⌘-scroll and ⌥-scroll over the document surface into text-size and
/// structural-detail steps. It renders nothing and animates nothing itself:
/// both scales are existing commands with their own transitions, and this
/// only decides *when* to ask for one.
pub struct ScrollZoomCoordinator {
    host: Host,
    intent: Cell<Option<Intent>>,
    accumulated: Cell<CGFloat>,
    spent: Cell<isize>,
    last_timestamp: Cell<TimeInterval>,
}

impl ScrollZoomCoordinator {
    /// `init(host:)`.
    pub fn new(host: Host) -> Rc<ScrollZoomCoordinator> {
        Rc::new(ScrollZoomCoordinator {
            host,
            intent: Cell::new(None),
            accumulated: Cell::new(0.0),
            spent: Cell::new(0),
            last_timestamp: Cell::new(-f64::MAX),
        })
    }

    /// Visible for tests: the travel banked toward the next step.
    pub fn accumulated_travel_for_testing(&self) -> CGFloat {
        self.accumulated.get()
    }

    /// Visible for tests: steps this gesture has already fired.
    pub fn steps_fired_for_testing(&self) -> isize {
        self.spent.get()
    }

    /// The single entry point from the chain. Returns `true` when a zoom has
    /// taken the event, in which case the scroll view must not see it.
    pub fn handle(&self, event: &NSEvent) -> bool {
        // No modifier, no claim, and not one branch of thinking about it: an
        // unmodified scroll is the overwhelmingly common case and must cost
        // nothing on its way past.
        let Some(intent) = ScrollZoomPolicy::intent(ScrollGestureModifiers::held(event)) else {
            self.intent.set(None);
            return false;
        };

        let precise = event.hasPreciseScrollingDeltas();
        if Some(intent) != self.intent.get() {
            self.begin_gesture(intent, event.timestamp());
        }
        // A trackpad announces the gesture; a wheel is only ever inferred from
        // the quiet before it.
        if precise {
            if event.phase() == NSEventPhase::Began {
                self.begin_gesture(intent, event.timestamp());
            }
        } else if ScrollZoomPolicy::starts_new_wheel_gesture(event.timestamp() - self.last_timestamp.get()) {
            self.begin_gesture(intent, event.timestamp());
        }
        self.last_timestamp.set(event.timestamp());

        // The momentum tail is the trackpad coasting, not the reader zooming.
        // It is still swallowed: handing it back would scroll the page out
        // from under a size the reader just settled on.
        if !event.momentumPhase().is_empty() {
            return true;
        }

        if event.phase() == NSEventPhase::Ended || event.phase() == NSEventPhase::Cancelled {
            self.end_gesture();
            return true;
        }

        self.accumulated.set(self.accumulated.get() + ScrollZoomPolicy::contribution(event.scrollingDeltaY(), precise));
        let steps = ScrollZoomPolicy::steps(intent, self.accumulated.get(), self.spent.get(), precise);
        if steps == 0 {
            return true;
        }
        self.accumulated
            .set(self.accumulated.get() - steps as CGFloat * ScrollZoomPolicy::step_threshold(intent, precise));
        self.spent.set(self.spent.get() + steps);

        match intent {
            Intent::TextSize => (self.host.step_text_size)(steps),
            Intent::StructuralDetail => {
                (self.host.step_detail)(steps);
                (self.host.perform_haptic_feedback)();
            }
        }
        true
    }

    fn begin_gesture(&self, intent: Intent, timestamp: TimeInterval) {
        self.intent.set(Some(intent));
        self.accumulated.set(0.0);
        self.spent.set(0);
        self.last_timestamp.set(timestamp);
    }

    /// The fingers are up. Nothing is banked across gestures: travel that did
    /// not earn a step was the reader changing their mind.
    fn end_gesture(&self) {
        self.intent.set(None);
        self.accumulated.set(0.0);
        self.spent.set(0);
    }
}

impl ScrollGestureHandler for ScrollZoomCoordinator {
    fn handle(&self, event: &NSEvent) -> bool {
        ScrollZoomCoordinator::handle(self, event)
    }
}
