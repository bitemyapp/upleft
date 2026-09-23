//! Port of `Tests/MarkdownRenderTests/MotionSystemTests.swift`.
//!
//! The motion system's own arithmetic, separate from any surface that uses
//! it.

use objc2_app_kit::{NSColor, NSColorSpace};
use objc2_core_foundation::CGPoint;
use upleft_render::view::density_gutter_view::PipSimulation;
use upleft_render::motion::{
    self, SpringColor, SpringScalar, morph_cut, oklab, scroll_duration, SPRING_DELIBERATE, SPRING_QUICK,
    SPRING_STANDARD,
};

// MARK: - Settle

#[test]
fn long_trips_do_not_snap_visibly() {
    let mut spring = SpringScalar::with_value(0.0, SPRING_DELIBERATE);
    spring.target(800.0);

    let mut last_value = spring.value();
    let mut last_velocity = spring.velocity();
    let mut frames = 0;
    while spring.advance(1.0 / 120.0) && frames < 1000 {
        last_value = spring.value();
        last_velocity = spring.velocity();
        frames += 1;
    }

    let visible_gap = (800.0 - last_value).abs();
    let frame_travel = last_velocity.abs() / 120.0;
    assert!(visible_gap <= SpringScalar::MAXIMUM_SETTLE_BAND + frame_travel);
    assert!(visible_gap < 1.0);
    assert!(last_velocity.abs() < 25.0);
    assert_eq!(spring.value(), 800.0);
}

#[test]
fn cap_does_not_coarsen_small_travel() {
    let mut glow = SpringScalar::with_value(0.0, SPRING_QUICK);
    glow.target(0.12);
    let mut steps = 0;
    while glow.advance(1.0 / 60.0) && steps < 1000 {
        steps += 1;
    }
    assert!(steps > 4);
}

// MARK: - Interruption

#[test]
fn retargeting_keeps_speed() {
    let mut spring = SpringScalar::with_value(0.0, SPRING_STANDARD);
    spring.target(500.0);
    for _ in 0..6 {
        spring.advance(1.0 / 120.0);
    }
    let speed_before = spring.velocity();
    let position_before = spring.value();
    assert!(speed_before > 0.0);

    spring.target(900.0);
    assert_eq!(spring.value(), position_before);
    assert_eq!(spring.velocity(), speed_before);
}

#[test]
fn snapping_grounds() {
    let mut spring = SpringScalar::with_value(0.0, SPRING_STANDARD);
    spring.target(500.0);
    for _ in 0..6 {
        spring.advance(1.0 / 120.0);
    }
    spring.snap(42.0);
    assert_eq!(spring.value(), 42.0);
    assert_eq!(spring.velocity(), 0.0);
}

// MARK: - Morph cut

#[test]
fn morph_cut_has_an_empty_gap() {
    assert_eq!(morph_cut::outgoing(0.0), 1.0);
    assert_eq!(morph_cut::outgoing(morph_cut::HANDOFF), 0.0);
    assert_eq!(morph_cut::outgoing(1.0), 0.0);

    assert_eq!(morph_cut::incoming(0.0), 0.0);
    assert_eq!(morph_cut::incoming(morph_cut::HANDOFF), 0.0);
    assert_eq!(morph_cut::incoming(1.0), 1.0);

    let in_gap = 0.32;
    assert!(morph_cut::in_flight(in_gap));
    assert_eq!(morph_cut::outgoing(in_gap), 0.0);
    assert_eq!(morph_cut::incoming(in_gap), 0.0);

    for step in 0..=100 {
        let p = step as f64 / 100.0;
        let both = morph_cut::outgoing(p) > 0.0 && morph_cut::incoming(p) > 0.0;
        assert!(!both, "contents overlap at progress {p}");
    }
}

#[test]
fn handoff_is_mid_flight() {
    const { assert!(morph_cut::HANDOFF > 0.0) };
    const { assert!(morph_cut::HANDOFF < 1.0) };
    assert!(!morph_cut::in_flight(0.0));
    assert!(!morph_cut::in_flight(1.0));
}

/// A delayed spring releases from elapsed frames, not from an event
/// (`delayedSpringsReleaseOnTheirOwn`).
#[test]
fn delayed_springs_release_on_their_own() {
    let red = NSColor::systemRedColor().CGColor();
    let mut pip = PipSimulation::new(CGPoint::new(0.0, 0.0), 4.0, &red);
    pip.retarget(CGPoint::new(0.0, 0.0), 4.0, &red, motion::PREVIEW_STAGGER * 3.0, false);
    assert!(!pip.engaged);

    // Nothing but frames — no retarget, no event, no clock.
    let mut frames = 0;
    while !pip.engaged && frames < 600 {
        let _ = pip.advance(1.0 / 120.0);
        frames += 1;
    }
    assert!(pip.engaged, "a scheduled step never joined the cascade");

    // And having joined, it must eventually settle so the driver can park.
    let mut settling = 0;
    while pip.advance(1.0 / 120.0) && settling < 2000 {
        settling += 1;
    }
    assert!(settling < 2000, "the driver would spin for ever on this pip");
}

// MARK: - Colour

fn srgb(color: &NSColor) -> objc2::rc::Retained<NSColor> {
    color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()).expect("sRGB")
}

#[test]
fn colour_round_trips() {
    let probes = [
        NSColor::colorWithSRGBRed_green_blue_alpha(0.0, 0.0, 0.0, 1.0),
        NSColor::colorWithSRGBRed_green_blue_alpha(1.0, 1.0, 1.0, 1.0),
        NSColor::colorWithSRGBRed_green_blue_alpha(0.20, 0.45, 0.78, 1.0),
        NSColor::colorWithSRGBRed_green_blue_alpha(0.93, 0.71, 0.13, 0.6),
        NSColor::colorWithSRGBRed_green_blue_alpha(0.5, 0.5, 0.5, 1.0),
    ];
    for probe in &probes {
        let settled = SpringColor::with_value(probe).value();
        let (a, b) = (srgb(probe), srgb(&settled));
        assert!((a.redComponent() - b.redComponent()).abs() < 0.001);
        assert!((a.greenComponent() - b.greenComponent()).abs() < 0.001);
        assert!((a.blueComponent() - b.blueComponent()).abs() < 0.001);
        assert!((a.alphaComponent() - b.alphaComponent()).abs() < 0.001);
    }
}

#[test]
fn greyscale_colours_survive() {
    let mut spring = SpringColor::with_value(&NSColor::whiteColor());
    let white = srgb(&spring.value());
    assert!(white.redComponent() > 0.95);
    assert!(white.alphaComponent() > 0.95);

    spring.target(&NSColor::blackColor());
    spring.advance(1.0 / 120.0);
    let stepped = srgb(&spring.value());
    assert!(stepped.redComponent() > 0.5);
}

#[test]
fn hue_transition_keeps_its_lightness() {
    let from = NSColor::colorWithSRGBRed_green_blue_alpha(0.10, 0.45, 0.85, 1.0);
    let to = NSColor::colorWithSRGBRed_green_blue_alpha(0.95, 0.72, 0.15, 1.0);
    let floor = oklab::oklab(&from).0.min(oklab::oklab(&to).0);

    let mut spring = SpringColor::new(&from, SPRING_STANDARD, 0.0);
    spring.target(&to);
    let mut sampled = 0;
    while spring.advance(1.0 / 120.0) && sampled < 1000 {
        sampled += 1;
        let lightness = oklab::oklab(&spring.value()).0;
        assert!(lightness >= floor - 0.01, "darkened to {lightness} below {floor}");
    }
    assert!(sampled > 4);
}

// MARK: - Scroll

#[test]
fn scroll_duration_scales() {
    let hop = scroll_duration(40.0);
    let page = scroll_duration(900.0);
    let chapter = scroll_duration(6000.0);
    assert!(hop < page);
    assert!(page < chapter);
    assert!(hop >= 0.18);
    assert!(chapter <= 0.55);
    assert_eq!(scroll_duration(0.0), 0.18);
    assert_eq!(scroll_duration(-100.0), 0.18);
}

#[test]
fn curves_build() {
    // The bezier vocabulary resolves to real timing functions.
    for curve in [motion::Curve::EaseOut, motion::Curve::Decelerate, motion::Curve::Snap, motion::Curve::Structural] {
        let _ = motion::timing(curve);
    }
    let _ = motion::pop(0.9, 1.06, motion::STANDARD, None, Some(6.0));
    let _ = motion::pop(
        0.9,
        1.06,
        motion::STANDARD,
        Some(objc2_core_foundation::CGVector { dx: 1.0, dy: 0.5 }),
        None,
    );
}
