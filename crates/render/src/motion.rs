//! Port of `Motion.swift`: one timing authority for document and chrome
//! motion (§11.4).
//!
//! The system is three durations, two vocabularies on top of them (beziers
//! for `NSAnimationContext`, springs for everything the pointer drives), a
//! per-view display-link driver, and the morph cut. Every constant and every
//! floating-point expression is kept as the Swift writes it.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, Message, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSAnimationContext, NSColor, NSColorSpace, NSView, NSWindow};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize, CGVector};
use objc2_foundation::{NSArray, NSNumber, NSObject, NSRect, NSRunLoop, NSRunLoopCommonModes, NSString, NSValue};
use objc2_quartz_core::{
    CAAnimation, CAAnimationGroup, CACurrentMediaTime, CADisplayLink, CAKeyframeAnimation, CAMediaTiming,
    CAMediaTimingFunction, CATransform3D, CATransform3DIdentity, NSValueCATransform3DAdditions,
    kCAMediaTimingFunctionEaseOut,
};

use crate::swift_compat::{pow, smax, smin};

/// Swift's `TimeInterval`.
pub type TimeInterval = f64;

/// Feedback that must not be noticed: hover, press, a colour warming.
pub const QUICK: TimeInterval = 0.12;
/// A state change the reader is watching: a check drawing, a thumb sliding.
pub const STANDARD: TimeInterval = 0.20;
/// A structural change: a panel's contents arriving, an arc travelling.
pub const DELIBERATE: TimeInterval = 0.32;

/// Perceptual settle for native glass changing topology.
pub const LIQUID_SETTLE: TimeInterval = 0.38;

/// Pointer feedback, the tier below `quick`.
pub const HOVER: TimeInterval = 0.10;
pub const PRESS_IN: TimeInterval = 0.07;
pub const PRESS_OUT: TimeInterval = 0.11;
/// A selection indicator travelling between segments.
pub const SELECTION: TimeInterval = 0.15;
/// A control briefly asserting itself — a value flashing as it commits.
pub const EMPHASIS: TimeInterval = 0.11;

/// Whole-rail breathe on pointer enter / leave.
pub const BREATHE: TimeInterval = QUICK;
/// Preview title lands first; snippet follows.
pub const PREVIEW_STAGGER: TimeInterval = QUICK / 3.0;

/// Crossfade when the rail's preview swaps to a different section.
pub const PREVIEW_CROSSFADE: TimeInterval = 0.06;

/// The staging window inside `MorphCut` before incoming panel content appears.
pub const FLOATING_CONTENT_REVEAL_LEAD: TimeInterval = 0.08;

/// The visible sliver is already a real material edge.
pub const FLOATING_SURFACE_SLIVER_OPACITY: CGFloat = 0.82;
/// Presence is settled by the first quarter of the structural spring.
pub const FLOATING_SURFACE_PRESENCE_FRACTION: CGFloat = 0.25;

/// The density rail's impulse, and the only declared exception.
pub const JUMP_PUNCH_KICK: CGFloat = 480.0;

// MARK: - Springs

pub const SPRING_QUICK: TimeInterval = QUICK;
pub const SPRING_STANDARD: TimeInterval = STANDARD;
pub const SPRING_DELIBERATE: TimeInterval = DELIBERATE;

/// A scalar spring for per-frame integration (`Motion.SpringScalar`).
///
/// The integrator is the damped-harmonic closed form evaluated exactly at the
/// requested `dt`. `bounce` runs 0…1, the parameterisation SwiftUI's `Spring`
/// uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringScalar {
    value: CGFloat,
    velocity: CGFloat,
    pub angular_frequency: CGFloat,
    pub damping_ratio: CGFloat,
    target_value: CGFloat,
    /// Travel-scaled settle band, capped by `MAXIMUM_SETTLE_BAND`.
    settle_band: CGFloat,
}

impl SpringScalar {
    /// Half a point — one device pixel at 2x.
    pub const MAXIMUM_SETTLE_BAND: CGFloat = 0.5;
    /// The floor, for springs whose whole range is a fraction of a unit.
    pub const MINIMUM_SETTLE_BAND: CGFloat = 0.0006;
    /// The ~95% settle constant for a critically damped spring.
    pub const WINDUP: CGFloat = 4.744;

    /// `init(value:velocity:perceptualDuration:bounce:)`.
    pub fn new(value: CGFloat, velocity: CGFloat, perceptual_duration: TimeInterval, bounce: CGFloat) -> SpringScalar {
        let tau = smax(0.02, perceptual_duration);
        SpringScalar {
            value,
            velocity,
            angular_frequency: Self::WINDUP / tau,
            damping_ratio: smax(0.0, 1.0 - bounce * 0.7),
            target_value: value,
            settle_band: Self::MINIMUM_SETTLE_BAND,
        }
    }

    /// `SpringScalar(value:perceptualDuration:)` with no velocity or bounce.
    pub fn with_value(value: CGFloat, perceptual_duration: TimeInterval) -> SpringScalar {
        Self::new(value, 0.0, perceptual_duration, 0.0)
    }

    /// `SpringScalar(perceptualDuration:)`.
    pub fn with_duration(perceptual_duration: TimeInterval) -> SpringScalar {
        Self::new(0.0, 0.0, perceptual_duration, 0.0)
    }

    pub fn value(&self) -> CGFloat {
        self.value
    }

    pub fn velocity(&self) -> CGFloat {
        self.velocity
    }

    /// `target` (the property).
    pub fn target_value(&self) -> CGFloat {
        self.target_value
    }

    /// Re-launch the same state at a new perceptual duration. A non-nil
    /// `bounce` re-tunes the character too; nil keeps the current one.
    pub fn retune(&mut self, perceptual_duration: TimeInterval, bounce: Option<CGFloat>) {
        if !(perceptual_duration > 0.02) {
            return;
        }
        let held_target = self.target_value;
        let held_bounce = bounce.unwrap_or(if self.damping_ratio < 1.0 {
            (1.0 - self.damping_ratio) / 0.7
        } else {
            0.0
        });
        let new_spring = SpringScalar::new(self.value, self.velocity, perceptual_duration, held_bounce);
        *self = new_spring;
        self.target_value = held_target;
    }

    /// Teleport to a value — Reduce Motion, layout passes, brand-new marks.
    pub fn snap(&mut self, target: CGFloat) {
        self.value = target;
        self.velocity = 0.0;
        self.target_value = target;
        self.settle_band = Self::MINIMUM_SETTLE_BAND;
    }

    /// `target(_:)` (the method).
    pub fn target(&mut self, target: CGFloat) {
        if target == self.target_value {
            return;
        }
        self.settle_band = smin(
            smax((target - self.value).abs() * 0.008, Self::MINIMUM_SETTLE_BAND),
            Self::MAXIMUM_SETTLE_BAND,
        );
        self.target_value = target;
    }

    /// A velocity kick, in points per second.
    pub fn kick(&mut self, impulse: CGFloat) {
        self.velocity += impulse;
    }

    /// Integrate exactly through `dt` and settle once inside the band.
    /// Returns `false` once settled, so a driver can park.
    pub fn advance(&mut self, dt: CGFloat) -> bool {
        let dt = smax(0.0, dt);
        let omega = self.angular_frequency;
        let zeta = self.damping_ratio;
        let x0 = self.value - self.target_value;
        let v0 = self.velocity;
        let damped = zeta * omega;
        let exponent = (-damped * dt).exp();

        let x: CGFloat;
        let v: CGFloat;
        if zeta >= 1.0 - 0.000_001 {
            // Critically damped:  x(t) = e^(−ωt) (A + Bt).
            let s = smax(0.0, zeta * zeta - 1.0).sqrt() * omega;
            if s > 0.000_001 {
                // Overdamped (unreachable: dampingRatio ≥ 0.3).
                let lambda_plus = -damped + s;
                let lambda_minus = -damped - s;
                let c1 = (v0 + (damped + s) * x0) / (2.0 * s);
                let c2 = x0 - c1;
                x = c1 * (lambda_plus * dt).exp() + c2 * (lambda_minus * dt).exp();
                v = c1 * lambda_plus * (lambda_plus * dt).exp() + c2 * lambda_minus * (lambda_minus * dt).exp();
            } else {
                let a = x0;
                let b = v0 + omega * x0;
                x = exponent * (a + b * dt);
                v = exponent * (b - omega * (a + b * dt));
            }
        } else {
            // Underdamped:  e^(−ζωt) (A cos ωd t + B sin ωd t).
            let omega_d = omega * smax(0.0, 1.0 - zeta * zeta).sqrt();
            let a = x0;
            let b = if omega_d > 0.000_001 { (v0 + damped * x0) / omega_d } else { 0.0 };
            let ct = (omega_d * dt).cos();
            let st = (omega_d * dt).sin();
            x = exponent * (a * ct + b * st);
            v = exponent * ((b * omega_d - a * damped) * ct - (a * omega_d + b * damped) * st);
        }

        if x.abs() < self.settle_band && v.abs() < self.settle_band * omega {
            self.value = self.target_value;
            self.velocity = 0.0;
            return false;
        }
        self.value = self.target_value + x;
        self.velocity = v;
        true
    }
}

impl Default for SpringScalar {
    /// `SpringScalar()`: value 0 at the standard duration.
    fn default() -> Self {
        SpringScalar::new(0.0, 0.0, SPRING_STANDARD, 0.0)
    }
}

// MARK: - Composed springs

/// Two scalar springs as one point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringPoint {
    pub x: SpringScalar,
    pub y: SpringScalar,
}

impl SpringPoint {
    pub fn new(value: CGPoint, perceptual_duration: TimeInterval, bounce: CGFloat) -> SpringPoint {
        SpringPoint {
            x: SpringScalar::new(value.x, 0.0, perceptual_duration, bounce),
            y: SpringScalar::new(value.y, 0.0, perceptual_duration, bounce),
        }
    }

    pub fn value(&self) -> CGPoint {
        CGPoint::new(self.x.value(), self.y.value())
    }

    pub fn target_value(&self) -> CGPoint {
        CGPoint::new(self.x.target_value(), self.y.target_value())
    }

    pub fn snap(&mut self, value: CGPoint) {
        self.x.snap(value.x);
        self.y.snap(value.y);
    }

    pub fn target(&mut self, value: CGPoint) {
        self.x.target(value.x);
        self.y.target(value.y);
    }

    pub fn retune(&mut self, perceptual_duration: TimeInterval, bounce: Option<CGFloat>) {
        self.x.retune(perceptual_duration, bounce);
        self.y.retune(perceptual_duration, bounce);
    }

    pub fn advance(&mut self, dt: CGFloat) -> bool {
        let mut moving = false;
        moving = self.x.advance(dt) || moving;
        moving = self.y.advance(dt) || moving;
        moving
    }
}

/// Two scalars as one size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringSize {
    pub width: SpringScalar,
    pub height: SpringScalar,
}

impl SpringSize {
    pub fn new(value: CGSize, perceptual_duration: TimeInterval, bounce: CGFloat) -> SpringSize {
        SpringSize {
            width: SpringScalar::new(value.width, 0.0, perceptual_duration, bounce),
            height: SpringScalar::new(value.height, 0.0, perceptual_duration, bounce),
        }
    }

    pub fn value(&self) -> CGSize {
        CGSize::new(self.width.value(), self.height.value())
    }

    pub fn target_value(&self) -> CGSize {
        CGSize::new(self.width.target_value(), self.height.target_value())
    }

    pub fn snap(&mut self, value: CGSize) {
        self.width.snap(value.width);
        self.height.snap(value.height);
    }

    pub fn target(&mut self, value: CGSize) {
        self.width.target(value.width);
        self.height.target(value.height);
    }

    pub fn retune(&mut self, perceptual_duration: TimeInterval, bounce: Option<CGFloat>) {
        self.width.retune(perceptual_duration, bounce);
        self.height.retune(perceptual_duration, bounce);
    }

    pub fn advance(&mut self, dt: CGFloat) -> bool {
        let mut moving = false;
        moving = self.width.advance(dt) || moving;
        moving = self.height.advance(dt) || moving;
        moving
    }
}

fn mid_x(rect: CGRect) -> CGFloat {
    rect.origin.x + rect.size.width * 0.5
}

fn mid_y(rect: CGRect) -> CGFloat {
    rect.origin.y + rect.size.height * 0.5
}

/// A rect springed as **centre + size**, never as four edges.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringRect {
    pub centre: SpringPoint,
    pub size: SpringSize,
}

impl SpringRect {
    pub fn new(rect: CGRect, perceptual_duration: TimeInterval, bounce: CGFloat) -> SpringRect {
        SpringRect {
            centre: SpringPoint::new(CGPoint::new(mid_x(rect), mid_y(rect)), perceptual_duration, bounce),
            size: SpringSize::new(rect.size, perceptual_duration, bounce),
        }
    }

    pub fn rect(&self) -> CGRect {
        let size = self.size.value();
        CGRect::new(
            CGPoint::new(self.centre.value().x - size.width / 2.0, self.centre.value().y - size.height / 2.0),
            CGSize::new(size.width, size.height),
        )
    }

    /// Where this rect is headed.
    pub fn target_value(&self) -> CGRect {
        let size = self.size.target_value();
        let centre = self.centre.target_value();
        CGRect::new(
            CGPoint::new(centre.x - size.width / 2.0, centre.y - size.height / 2.0),
            CGSize::new(size.width, size.height),
        )
    }

    pub fn snap(&mut self, rect: CGRect) {
        self.centre.snap(CGPoint::new(mid_x(rect), mid_y(rect)));
        self.size.snap(rect.size);
    }

    pub fn target(&mut self, rect: CGRect) {
        self.centre.target(CGPoint::new(mid_x(rect), mid_y(rect)));
        self.size.target(rect.size);
    }

    pub fn retune(&mut self, perceptual_duration: TimeInterval, bounce: Option<CGFloat>) {
        self.centre.retune(perceptual_duration, bounce);
        self.size.retune(perceptual_duration, bounce);
    }

    pub fn advance(&mut self, dt: CGFloat) -> bool {
        let mut moving = false;
        moving = self.centre.advance(dt) || moving;
        moving = self.size.advance(dt) || moving;
        moving
    }
}

/// OK Lab — the colour space colours move in (`Motion.SpringLab`).
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(non_snake_case)]
struct SpringLab {
    L: SpringScalar,
    a: SpringScalar,
    b: SpringScalar,
}

impl SpringLab {
    fn new(perceptual_duration: TimeInterval, bounce: CGFloat) -> SpringLab {
        SpringLab {
            L: SpringScalar::new(0.5, 0.0, perceptual_duration, bounce),
            a: SpringScalar::new(0.0, 0.0, perceptual_duration, bounce),
            b: SpringScalar::new(0.0, 0.0, perceptual_duration, bounce),
        }
    }
}

/// A colour springed in OKLab with a linear alpha channel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringColor {
    lab: SpringLab,
    alpha: SpringScalar,
    pub perceptual_duration: TimeInterval,
    pub bounce: CGFloat,
}

impl SpringColor {
    pub fn new(value: &NSColor, perceptual_duration: TimeInterval, bounce: CGFloat) -> SpringColor {
        let mut spring = SpringColor {
            lab: SpringLab::new(perceptual_duration, bounce),
            alpha: SpringScalar::new(1.0, 0.0, perceptual_duration, bounce),
            perceptual_duration,
            bounce,
        };
        spring.snap(value);
        spring
    }

    /// `SpringColor(value:)` at the quick duration.
    pub fn with_value(value: &NSColor) -> SpringColor {
        Self::new(value, SPRING_QUICK, 0.0)
    }

    pub fn snap(&mut self, color: &NSColor) {
        let (l, a, b) = oklab::oklab(color);
        self.lab.L.snap(l);
        self.lab.a.snap(a);
        self.lab.b.snap(b);
        self.alpha.snap(color.alphaComponent());
    }

    pub fn target(&mut self, color: &NSColor) {
        let (l, a, b) = oklab::oklab(color);
        self.lab.L.target(l);
        self.lab.a.target(a);
        self.lab.b.target(b);
        self.alpha.target(color.alphaComponent());
    }

    pub fn retune(&mut self, perceptual_duration: TimeInterval) {
        self.lab.L.retune(perceptual_duration, None);
        self.lab.a.retune(perceptual_duration, None);
        self.lab.b.retune(perceptual_duration, None);
        self.alpha.retune(perceptual_duration, None);
    }

    /// A velocity kick on the alpha channel alone.
    pub fn kick_alpha(&mut self, impulse: CGFloat) {
        self.alpha.kick(impulse);
    }

    pub fn advance(&mut self, dt: CGFloat) -> bool {
        let mut moving = false;
        moving = self.lab.L.advance(dt) || moving;
        moving = self.lab.a.advance(dt) || moving;
        moving = self.lab.b.advance(dt) || moving;
        moving = self.alpha.advance(dt) || moving;
        moving
    }

    pub fn value(&self) -> Retained<NSColor> {
        oklab::srgb(self.lab.L.value(), self.lab.a.value(), self.lab.b.value(), self.alpha.value())
    }

    /// The colour this spring is heading for.
    pub fn target_value(&self) -> Retained<NSColor> {
        oklab::srgb(
            self.lab.L.target_value(),
            self.lab.a.target_value(),
            self.lab.b.target_value(),
            self.alpha.target_value(),
        )
    }
}

/// The sRGB ↔ OKLab conversions (`Motion.OKLab`).
pub mod oklab {
    use super::*;

    #[inline(always)]
    fn linear(c: CGFloat) -> CGFloat {
        if c <= 0.04045 { c / 12.92 } else { pow((c + 0.055) / 1.055, 2.4) }
    }

    #[inline(always)]
    fn gamma(c: CGFloat) -> CGFloat {
        if c <= 0.0031308 { 12.92 * c } else { 1.055 * pow(c, 1.0 / 2.4) - 0.055 }
    }

    #[inline(always)]
    fn cbrt(x: CGFloat) -> CGFloat {
        if x < 0.0 { -pow(-x, 1.0 / 3.0) } else { pow(x, 1.0 / 3.0) }
    }

    /// sRGB (gamma-encoded) to OKLab.
    pub fn oklab(color: &NSColor) -> (CGFloat, CGFloat, CGFloat) {
        let Some(srgb) = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()) else {
            return (0.5, 0.0, 0.0);
        };
        let r = linear(srgb.redComponent());
        let g = linear(srgb.greenComponent());
        let b = linear(srgb.blueComponent());
        let l = 0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
        let m = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
        let s = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;
        let (l_, m_, s_) = (cbrt(l), cbrt(m), cbrt(s));
        (
            0.210_454_255_3 * l_ + 0.793_617_785_0 * m_ - 0.004_072_046_8 * s_,
            1.977_998_495_1 * l_ - 2.428_592_205_0 * m_ + 0.450_593_709_9 * s_,
            0.025_904_037_1 * l_ + 0.782_771_766_2 * m_ - 0.808_675_766_0 * s_,
        )
    }

    /// OKLab back to sRGB, gamma-encoded, clamped into the displayable cube.
    #[allow(non_snake_case)]
    pub fn srgb(L: CGFloat, a: CGFloat, b: CGFloat, alpha: CGFloat) -> Retained<NSColor> {
        let l_ = L + 0.396_337_777_4 * a + 0.215_803_757_3 * b;
        let m_ = L - 0.105_561_345_8 * a - 0.063_854_172_8 * b;
        let s_ = L - 0.089_484_177_5 * a - 1.291_485_548_0 * b;
        let (l, m, s) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
        let r = 4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s;
        let g = -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s;
        let b = -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s;
        NSColor::colorWithSRGBRed_green_blue_alpha(
            smin(1.0, smax(0.0, gamma(r))),
            smin(1.0, smax(0.0, gamma(g))),
            smin(1.0, smax(0.0, gamma(b))),
            smin(1.0, smax(0.0, alpha)),
        )
    }
}

// MARK: - Driver

/// The driver's elapsed-time accounting (`Motion.FrameClock`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FrameClock {
    last_tick: f64,
    is_running: bool,
}

impl FrameClock {
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// Begins timing from `now`. Returns whether this call started it.
    pub fn start(&mut self, now: f64) -> bool {
        if self.is_running {
            return false;
        }
        self.last_tick = now;
        self.is_running = true;
        true
    }

    pub fn stop(&mut self) {
        self.is_running = false;
        self.last_tick = 0.0;
    }

    /// Seconds since the previous tick, advancing the mark to `now`.
    pub fn tick(&mut self, now: f64) -> CGFloat {
        let dt = smax(0.0, now - self.last_tick);
        self.last_tick = now;
        dt
    }
}

struct SpringDriverState {
    view: ObjcWeak<NSView>,
    link: RefCell<Option<Retained<CADisplayLink>>>,
    clock: Cell<FrameClock>,
    advance: RefCell<Box<dyn FnMut(CGFloat) -> bool>>,
    apply: RefCell<Box<dyn FnMut()>>,
}

pub struct SpringDriverTargetIvars {
    driver: Rc<SpringDriverState>,
}

define_class!(
    /// The display link's target: the Swift driver is itself the target, and
    /// the link retains it, so this object holds the driver strongly.
    // SAFETY: NSObject has no subclassing requirements; the class does not
    // implement Drop.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpleftSpringDriverTarget"]
    #[ivars = SpringDriverTargetIvars]
    struct SpringDriverTarget;

    unsafe impl NSObjectProtocol for SpringDriverTarget {}

    impl SpringDriverTarget {
        #[unsafe(method(step:))]
        fn step(&self, _link: &CADisplayLink) {
            let driver = self.ivars().driver.clone();
            SpringDriver::step(&driver);
        }
    }
);

/// One display-link driver per view (`Motion.SpringDriver`, §11.4).
///
/// `advance(dt)` integrates and returns `false` when every spring has
/// settled; `apply()` draws the frame's state and also runs on the settle
/// tick. The driver parks itself when `advance` asks, when the view leaves
/// its window, and when the owner drops it.
#[derive(Clone)]
pub struct SpringDriver {
    state: Rc<SpringDriverState>,
}

impl SpringDriver {
    pub fn new(
        view: &NSView,
        advance: impl FnMut(CGFloat) -> bool + 'static,
        apply: impl FnMut() + 'static,
    ) -> SpringDriver {
        SpringDriver {
            state: Rc::new(SpringDriverState {
                view: ObjcWeak::from(view),
                link: RefCell::new(None),
                clock: Cell::new(FrameClock::default()),
                advance: RefCell::new(Box::new(advance)),
                apply: RefCell::new(Box::new(apply)),
            }),
        }
    }

    pub fn is_running(&self) -> bool {
        self.state.clock.get().is_running()
    }

    /// Arm the link. A parked driver armed again gets a fresh link; an
    /// already-running driver is left strictly alone (never re-based).
    pub fn arm(&self) -> bool {
        if self.state.clock.get().is_running() {
            return true;
        }
        let Some(view) = self.state.view.load() else { return false };
        if view.window().is_none() {
            return false;
        }
        let mut clock = self.state.clock.get();
        clock.start(CACurrentMediaTime());
        self.state.clock.set(clock);
        let mtm = view.mtm();
        let target = SpringDriverTarget::alloc(mtm).set_ivars(SpringDriverTargetIvars { driver: self.state.clone() });
        let target: Retained<SpringDriverTarget> = unsafe { msg_send![super(target), init] };
        // SAFETY: `step:` is implemented by the target and takes the link.
        let link = unsafe { view.displayLinkWithTarget_selector(&target, sel!(step:)) };
        unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
        *self.state.link.borrow_mut() = Some(link);
        true
    }

    /// Tear the link down immediately.
    pub fn park(&self) {
        Self::park_state(&self.state);
    }

    fn park_state(state: &SpringDriverState) {
        if let Some(link) = state.link.borrow_mut().take() {
            link.invalidate();
        }
        let mut clock = state.clock.get();
        clock.stop();
        state.clock.set(clock);
    }

    /// The lifecycle hook surfaces call from `viewDidMoveToWindow`.
    pub fn view_did_move_to_window(&self, window: Option<&NSWindow>) {
        if window.is_none() {
            self.park();
        }
    }

    fn step(state: &Rc<SpringDriverState>) {
        let mut clock = state.clock.get();
        let dt = clock.tick(CACurrentMediaTime());
        state.clock.set(clock);
        let moving = (state.advance.borrow_mut())(dt);
        (state.apply.borrow_mut())();
        if !moving {
            Self::park_state(state);
        }
    }

}

// MARK: - SpringSurfaceView

pub struct SpringSurfaceIvars {
    spring_driver: RefCell<Option<SpringDriver>>,
}

define_class!(
    /// A view whose springs step on one per-view driver
    /// (`Motion.SpringSurfaceView`).
    ///
    /// Subclasses override `springTick:` (return `true` to keep ticking),
    /// `springApply` and `springsSettleImmediately` as Objective-C methods, so
    /// the base dispatches to them the way Swift's `open` methods do.
    // SAFETY: NSView's designated initialiser is `initWithFrame:`, which the
    // override below forwards to after setting the ivars. No Drop impl; the
    // driver parks in `dealloc` through the ivars' own drop.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "SpringSurfaceView"]
    #[ivars = SpringSurfaceIvars]
    pub struct SpringSurfaceView;

    unsafe impl NSObjectProtocol for SpringSurfaceView {}

    impl SpringSurfaceView {
        #[unsafe(method_id(initWithFrame:))]
        fn init_with_frame(this: objc2::rc::Allocated<Self>, frame: NSRect) -> Retained<Self> {
            let this = this.set_ivars(SpringSurfaceIvars { spring_driver: RefCell::new(None) });
            unsafe { msg_send![super(this), initWithFrame: frame] }
        }

        /// The per-frame integration hook.
        #[unsafe(method(springTick:))]
        fn spring_tick(&self, _dt: CGFloat) -> bool {
            false
        }

        /// The per-frame draw of that integration.
        #[unsafe(method(springApply))]
        fn spring_apply(&self) {}

        /// Put every spring on its target now and draw that.
        #[unsafe(method(springsSettleImmediately))]
        fn springs_settle_immediately(&self) {}

        #[unsafe(method(viewDidMoveToWindow))]
        fn view_did_move_to_window(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidMoveToWindow] };
            if let Some(driver) = self.ivars().spring_driver.borrow().as_ref() {
                driver.view_did_move_to_window(self.window().as_deref());
            }
        }

        #[unsafe(method(viewWillStartLiveResize))]
        fn view_will_start_live_resize(&self) {
            let _: () = unsafe { msg_send![super(self), viewWillStartLiveResize] };
            self.park_springs();
            let _: () = unsafe { msg_send![self, springsSettleImmediately] };
        }

        #[unsafe(method(viewDidEndLiveResize))]
        fn view_did_end_live_resize(&self) {
            let _: () = unsafe { msg_send![super(self), viewDidEndLiveResize] };
            let _: () = unsafe { msg_send![self, springsSettleImmediately] };
        }
    }
);

impl Drop for SpringSurfaceIvars {
    fn drop(&mut self) {
        if let Some(driver) = self.spring_driver.get_mut().take() {
            driver.park();
        }
    }
}

impl SpringSurfaceView {
    pub fn springs_are_running(&self) -> bool {
        self.ivars().spring_driver.borrow().as_ref().is_some_and(SpringDriver::is_running)
    }

    /// Start (or keep) the driver. Refused during a live resize.
    pub fn arm_springs(&self) -> bool {
        if self.inLiveResize() {
            let _: () = unsafe { msg_send![self, springsSettleImmediately] };
            return false;
        }
        let driver = self.ivars().spring_driver.borrow().clone();
        let driver = driver.unwrap_or_else(|| self.make_spring_driver());
        let armed = driver.arm();
        if !armed {
            let _: () = unsafe { msg_send![self, springsSettleImmediately] };
        }
        armed
    }

    /// Stop the driver immediately.
    pub fn park_springs(&self) {
        if let Some(driver) = self.ivars().spring_driver.borrow().as_ref() {
            driver.park();
        }
    }

    fn make_spring_driver(&self) -> SpringDriver {
        let weak_tick: ObjcWeak<SpringSurfaceView> = ObjcWeak::from(self);
        let weak_apply = weak_tick.clone();
        let driver = SpringDriver::new(
            self,
            move |dt| match weak_tick.load() {
                Some(view) => unsafe { msg_send![&*view, springTick: dt] },
                None => false,
            },
            move || {
                if let Some(view) = weak_apply.load() {
                    let _: () = unsafe { msg_send![&*view, springApply] };
                }
            },
        );
        *self.ivars().spring_driver.borrow_mut() = Some(driver.clone());
        driver
    }
}

// MARK: - Curves

/// Three curves, because there are three lengths of movement (`Motion.Curve`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Curve {
    EaseOut,
    #[default]
    Decelerate,
    Snap,
    Structural,
}

fn control_points(curve: Curve) -> (CGFloat, CGFloat, CGFloat, CGFloat) {
    match curve {
        Curve::EaseOut => (0.0, 0.0, 0.58, 1.0),
        Curve::Decelerate => (0.22, 0.82, 0.28, 1.0),
        Curve::Snap => (0.16, 1.0, 0.3, 1.0),
        Curve::Structural => (0.30, 0.30, 0.20, 1.0),
    }
}

/// `Motion.timing(_:)`.
pub fn timing(curve: Curve) -> Retained<CAMediaTimingFunction> {
    match curve {
        Curve::EaseOut => unsafe { CAMediaTimingFunction::functionWithName(kCAMediaTimingFunctionEaseOut) },
        Curve::Decelerate | Curve::Snap | Curve::Structural => {
            let p = control_points(curve);
            CAMediaTimingFunction::functionWithControlPoints(p.0 as f32, p.1 as f32, p.2 as f32, p.3 as f32)
        }
    }
}

// MARK: - Scroll

/// A programmatic scroll's trip, scaled by distance.
pub fn scroll_duration(distance: CGFloat) -> TimeInterval {
    let mut duration = 0.0115 * smax(0.0, distance).sqrt();
    duration = smin(0.55, smax(0.18, duration));
    duration
}

/// `Motion.pop(from:overshoot:duration:travelAxis:cornerRadius:)`.
pub fn pop(
    start: CGFloat,
    overshoot: CGFloat,
    duration: TimeInterval,
    travel_axis: Option<CGVector>,
    corner_radius: Option<CGFloat>,
) -> Retained<CAAnimation> {
    let peak_along = overshoot;
    let peak_cross = if travel_axis.is_none() { overshoot } else { 1.0 - (overshoot - 1.0) * 0.45 };
    let timing_functions = NSArray::from_retained_slice(&[timing(Curve::Decelerate), timing(Curve::EaseOut)]);

    let scale: Retained<CAKeyframeAnimation>;
    if let Some(travel_axis) = travel_axis {
        let length = travel_axis.dx.hypot(travel_axis.dy);
        if !(length > 0.000_1) {
            return pop(start, overshoot, duration, None, corner_radius);
        }
        let ux = travel_axis.dx / length;
        let uy = travel_axis.dy / length;
        let transform = |along: CGFloat, cross: CGFloat| -> CATransform3D {
            let sx = 1.0 + (along - 1.0) * ux * ux + (cross - 1.0) * uy * uy;
            let sy = 1.0 + (along - 1.0) * uy * uy + (cross - 1.0) * ux * ux;
            CATransform3D::new_scale(sx, sy, 1.0)
        };
        scale = CAKeyframeAnimation::animationWithKeyPath(Some(&NSString::from_str("transform")));
        let values = [
            transform_value(transform(start, start)),
            transform_value(transform(peak_along, peak_cross)),
            transform_value(unsafe { CATransform3DIdentity }),
        ];
        let values = NSArray::from_retained_slice(&values);
        unsafe { scale.setValues(Some(objc2::rc::Retained::cast_unchecked::<NSArray>(values).as_ref())) };
    } else {
        scale = CAKeyframeAnimation::animationWithKeyPath(Some(&NSString::from_str("transform.scale")));
        let values = [NSNumber::new_f64(start), NSNumber::new_f64(peak_along), NSNumber::new_f64(1.0)];
        let values = NSArray::from_retained_slice(&values);
        unsafe { scale.setValues(Some(Retained::cast_unchecked::<NSArray>(values).as_ref())) };
    }
    let key_times = NSArray::from_retained_slice(&[
        NSNumber::new_f64(0.0),
        NSNumber::new_f64(0.55),
        NSNumber::new_f64(1.0),
    ]);
    scale.setKeyTimes(Some(&key_times));
    scale.setDuration(duration);
    scale.setTimingFunctions(Some(&timing_functions));

    let Some(corner_radius) = corner_radius.filter(|radius| *radius > 0.0) else {
        return Retained::into_super(Retained::into_super(scale));
    };
    pop_group(&scale, corner_radius, duration)
}

fn transform_value(transform: CATransform3D) -> Retained<NSValue> {
    // SAFETY: a plain value conversion.
    unsafe { NSValue::valueWithCATransform3D(transform) }
}

fn pop_group(scale: &CAKeyframeAnimation, corner_radius: CGFloat, duration: TimeInterval) -> Retained<CAAnimation> {
    let radius = CAKeyframeAnimation::animationWithKeyPath(Some(&NSString::from_str("cornerRadius")));
    let values = NSArray::from_retained_slice(&[
        NSNumber::new_f64(corner_radius),
        NSNumber::new_f64(corner_radius * 1.15),
        NSNumber::new_f64(corner_radius),
    ]);
    unsafe { radius.setValues(Some(Retained::cast_unchecked::<NSArray>(values).as_ref())) };
    radius.setKeyTimes(scale.keyTimes().as_deref());
    radius.setDuration(duration);
    radius.setTimingFunctions(scale.timingFunctions().as_deref());
    let group = CAAnimationGroup::animation();
    let animations: Retained<NSArray<CAAnimation>> = NSArray::from_retained_slice(&[
        Retained::into_super(Retained::into_super(scale.retain())),
        Retained::into_super(Retained::into_super(radius)),
    ]);
    group.setAnimations(Some(&animations));
    group.setDuration(duration);
    group.setBeginTime(scale.beginTime());
    group.setFillMode(&scale.fillMode());
    Retained::into_super(group)
}

// MARK: - Driver (bezier carriage)

/// Run an `NSAnimationContext` group — the bezier carriage.
pub fn run(
    reduce_motion: bool,
    duration: TimeInterval,
    curve: Curve,
    changes: impl Fn(&NSAnimationContext) + 'static,
    completion: Option<Box<dyn Fn() + 'static>>,
) {
    let changes = RcBlock::new(move |context: std::ptr::NonNull<NSAnimationContext>| {
        let context = unsafe { context.as_ref() };
        context.setDuration(if reduce_motion { 0.0 } else { duration });
        context.setAllowsImplicitAnimation(!reduce_motion);
        context.setTimingFunction(Some(&timing(curve)));
        changes(context);
    });
    let completion = completion.map(RcBlock::new);
    NSAnimationContext::runAnimationGroup_completionHandler(&changes, completion.as_deref());
}

// MARK: - Morph

/// Everything a morphing surface anchors to (`Motion.MorphAnchor`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MorphAnchor {
    pub frame: CGRect,
    pub corner_radius: CGFloat,
    pub tint: Option<Retained<NSColor>>,
}

/// Content-cut windows for a morph (`Motion.MorphCut`).
pub mod morph_cut {
    use super::*;

    /// The progress at which the outgoing content has fully handed off.
    pub const HANDOFF: CGFloat = 0.30;

    /// p ∈ [0, 0.30], 1 → 0.
    pub fn outgoing(p: CGFloat) -> CGFloat {
        clamp01(1.0 - p / HANDOFF)
    }

    /// p ∈ [0.35, 0.75], 0 → 1.
    pub fn incoming(p: CGFloat) -> CGFloat {
        if !(p > 0.35) {
            return 0.0;
        }
        if !(p < 0.75) {
            return 1.0;
        }
        (p - 0.35) / 0.40
    }

    /// The glass is alone and resting: p ∈ (0.30, 0.35).
    pub fn in_flight(p: CGFloat) -> bool {
        p > 0.30 && p < 0.35
    }

    fn clamp01(v: CGFloat) -> CGFloat {
        smin(1.0, smax(0.0, v))
    }
}
