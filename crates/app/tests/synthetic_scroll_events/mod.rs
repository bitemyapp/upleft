//! Port of `Tests/DownrightAppTests/SyntheticScrollEvents.swift`: synthetic
//! trackpad and wheel scroll events.
//!
//! Every scroll gesture over the document surface is decided by reading an
//! `NSEvent` — its phase, its momentum phase, whether its deltas are precise,
//! which modifiers are held, and how far it travelled. That decoding is part
//! of what the tests are testing, so they drive real `NSEvent`s built from
//! `CGEvent`s rather than a stand-in.
//!
//! Swift's `SyntheticScroll.event(deltaX:deltaY:phase:momentum:precise:modifiers:at:)`
//! takes defaulted arguments; here they are a builder with the same defaults:
//! `SyntheticScroll::new().delta_x(-20.0).phase(Some(CGScrollPhase::Began)).at(1.0).event()`.

#![allow(dead_code)]

use objc2::rc::Retained;
use objc2_app_kit::{NSEvent, NSEventModifierFlags};
use objc2_core_foundation::CGFloat;
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGMomentumScrollPhase, CGScrollEventUnit, CGScrollPhase,
};

/// `SyntheticScroll.Unavailable`.
#[derive(Debug)]
pub struct Unavailable;

/// The arguments of `SyntheticScroll.event(…)`, with Swift's defaults.
#[derive(Clone, Copy, Debug)]
pub struct SyntheticScroll {
    delta_x: CGFloat,
    delta_y: CGFloat,
    phase: Option<CGScrollPhase>,
    momentum: CGMomentumScrollPhase,
    precise: bool,
    modifiers: NSEventModifierFlags,
    seconds: f64,
}

impl Default for SyntheticScroll {
    fn default() -> Self {
        SyntheticScroll {
            delta_x: 0.0,
            delta_y: 0.0,
            phase: Some(CGScrollPhase::Changed),
            momentum: CGMomentumScrollPhase::None,
            precise: true,
            modifiers: NSEventModifierFlags::empty(),
            seconds: 0.0,
        }
    }
}

impl SyntheticScroll {
    pub fn new() -> SyntheticScroll {
        SyntheticScroll::default()
    }

    pub fn delta_x(mut self, value: CGFloat) -> Self {
        self.delta_x = value;
        self
    }

    pub fn delta_y(mut self, value: CGFloat) -> Self {
        self.delta_y = value;
        self
    }

    pub fn phase(mut self, value: Option<CGScrollPhase>) -> Self {
        self.phase = value;
        self
    }

    pub fn momentum(mut self, value: CGMomentumScrollPhase) -> Self {
        self.momentum = value;
        self
    }

    /// `true` for a trackpad, whose deltas are points and whose gestures have
    /// phases; `false` for a wheel, whose deltas are lines and which has
    /// neither phases nor a momentum tail.
    pub fn precise(mut self, value: bool) -> Self {
        self.precise = value;
        self
    }

    pub fn modifiers(mut self, value: NSEventModifierFlags) -> Self {
        self.modifiers = value;
        self
    }

    pub fn at(mut self, seconds: f64) -> Self {
        self.seconds = seconds;
        self
    }

    /// `SyntheticScroll.event(…)`, throwing `Unavailable`.
    pub fn try_event(&self) -> Result<Retained<NSEvent>, Unavailable> {
        let created = CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Pixel, 2, 0, 0, 0).ok_or(Unavailable)?;
        let event = Some(&*created);
        CGEvent::set_integer_value_field(event, CGEventField::ScrollWheelEventIsContinuous, if self.precise { 1 } else { 0 });
        CGEvent::set_double_value_field(event, CGEventField::ScrollWheelEventPointDeltaAxis1, self.delta_y);
        CGEvent::set_double_value_field(event, CGEventField::ScrollWheelEventPointDeltaAxis2, self.delta_x);
        if !self.precise {
            // A wheel's `scrollingDelta` is read out of the line fields, not
            // the point ones — the difference the whole coarse-device path
            // turns on, so it has to be real here too.
            CGEvent::set_integer_value_field(event, CGEventField::ScrollWheelEventDeltaAxis1, self.delta_y.round() as i64);
            CGEvent::set_integer_value_field(event, CGEventField::ScrollWheelEventDeltaAxis2, self.delta_x.round() as i64);
        }
        if let Some(phase) = self.phase {
            CGEvent::set_integer_value_field(event, CGEventField::ScrollWheelEventScrollPhase, phase.0 as i64);
        }
        CGEvent::set_integer_value_field(event, CGEventField::ScrollWheelEventMomentumPhase, self.momentum.0 as i64);
        // Always set, including to nothing: a `CGEvent` built with no source
        // is entitled to pick up whatever the keyboard happens to be doing,
        // and a stray ⌘ on the developer's hand must not decide a test.
        CGEvent::set_flags(event, flags(self.modifiers));
        // `max(0, seconds)`.
        let seconds = if self.seconds >= 0.0 { self.seconds } else { 0.0 };
        CGEvent::set_timestamp(event, (seconds * 1_000_000_000.0) as u64);
        NSEvent::eventWithCGEvent(&created).ok_or(Unavailable)
    }

    /// `try SyntheticScroll.event(…)` inside a test: a failure fails the test.
    pub fn event(&self) -> Retained<NSEvent> {
        self.try_event().expect("SyntheticScroll.Unavailable")
    }
}

fn flags(modifiers: NSEventModifierFlags) -> CGEventFlags {
    let mut flags = CGEventFlags::empty();
    if modifiers.contains(NSEventModifierFlags::Command) {
        flags.insert(CGEventFlags::MaskCommand);
    }
    if modifiers.contains(NSEventModifierFlags::Option) {
        flags.insert(CGEventFlags::MaskAlternate);
    }
    if modifiers.contains(NSEventModifierFlags::Control) {
        flags.insert(CGEventFlags::MaskControl);
    }
    if modifiers.contains(NSEventModifierFlags::Shift) {
        flags.insert(CGEventFlags::MaskShift);
    }
    if modifiers.contains(NSEventModifierFlags::CapsLock) {
        flags.insert(CGEventFlags::MaskAlphaShift);
    }
    if modifiers.contains(NSEventModifierFlags::Function) {
        flags.insert(CGEventFlags::MaskSecondaryFn);
    }
    flags
}
