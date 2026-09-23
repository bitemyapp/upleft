//! Port of `CrossPlatform.swift` (the AppKit branch): `BMColor` is `NSColor`,
//! `BMFont` is `NSFont`, and `NSBezierPath.bm_cgPath`.

use objc2::rc::Retained;
use objc2_app_kit::{NSBezierPath, NSBezierPathElement, NSColor, NSColorSpace};
use objc2_core_foundation::{CGAffineTransform, CGFloat, CGPoint, CGRect};
use objc2_core_graphics::{CGMutablePath, CGPath};
use objc2_foundation::{NSScanner, NSString};

use crate::swift;

/// The identity transform the Swift `CGMutablePath` overlay passes by
/// pointer when its `transform:` argument is defaulted.
pub const IDENTITY: CGAffineTransform = CGAffineTransform { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx: 0.0, ty: 0.0 };

/// `BMColor(hex:)`.
pub fn color_from_hex(hex: &str) -> Retained<NSColor> {
    let raw = swift::trim_whitespaces_and_newlines(hex).replace('#', "");
    let mut value: u64 = 0;
    let scanner = NSScanner::scannerWithString(&NSString::from_str(&raw));
    #[allow(deprecated)]
    unsafe {
        scanner.scanHexLongLong(&mut value);
    }

    let (r, g, b, a): (CGFloat, CGFloat, CGFloat, CGFloat) = match swift::character_count(&raw) {
        6 => (
            ((value & 0xFF0000) >> 16) as f64 / 255.0,
            ((value & 0x00FF00) >> 8) as f64 / 255.0,
            (value & 0x0000FF) as f64 / 255.0,
            1.0,
        ),
        8 => (
            ((value & 0xFF000000) >> 24) as f64 / 255.0,
            ((value & 0x00FF0000) >> 16) as f64 / 255.0,
            ((value & 0x0000FF00) >> 8) as f64 / 255.0,
            (value & 0x000000FF) as f64 / 255.0,
        ),
        _ => (0.0, 0.0, 0.0, 1.0),
    };

    NSColor::colorWithRed_green_blue_alpha(r, g, b, a)
}

/// `getRed(_:green:blue:alpha:)` after `usingColorSpace(.deviceRGB)`; zeros
/// when the conversion fails.
fn device_rgb(color: &NSColor) -> Option<(CGFloat, CGFloat, CGFloat, CGFloat)> {
    let converted = color.colorUsingColorSpace(&NSColorSpace::deviceRGBColorSpace())?;
    let (mut r, mut g, mut b, mut a) = (0.0, 0.0, 0.0, 0.0);
    unsafe { converted.getRed_green_blue_alpha(&mut r, &mut g, &mut b, &mut a) };
    Some((r, g, b, a))
}

/// `BMColor.mixed(with:amount:)`.
pub fn mixed(color: &NSColor, other: &NSColor, amount: CGFloat) -> Retained<NSColor> {
    let (mut r1, mut g1, mut b1, mut a1) = (0.0, 0.0, 0.0, 0.0);
    let (mut r2, mut g2, mut b2, mut a2) = (0.0, 0.0, 0.0, 0.0);
    if let (Some(c1), Some(c2)) = (
        color.colorUsingColorSpace(&NSColorSpace::deviceRGBColorSpace()),
        other.colorUsingColorSpace(&NSColorSpace::deviceRGBColorSpace()),
    ) {
        unsafe {
            c1.getRed_green_blue_alpha(&mut r1, &mut g1, &mut b1, &mut a1);
            c2.getRed_green_blue_alpha(&mut r2, &mut g2, &mut b2, &mut a2);
        }
    }

    let t = swift::max(0.0, swift::min(1.0, amount));
    NSColor::colorWithRed_green_blue_alpha(r1 + (r2 - r1) * t, g1 + (g2 - g1) * t, b1 + (b2 - b1) * t, a1 + (a2 - a1) * t)
}

/// `_hex(_:)` (SVGHelpers.swift): `#RRGGBB`, rounded and clamped.
pub fn hex(color: &NSColor) -> Option<String> {
    let (r, g, b, _) = device_rgb(color)?;
    let component = |v: CGFloat| swift::int(swift::max(0.0, swift::min(255.0, (v * 255.0).round())));
    Some(format!(
        "#{}{}{}",
        swift::format_i64("%02X", component(r)),
        swift::format_i64("%02X", component(g)),
        swift::format_i64("%02X", component(b))
    ))
}

/// `ColorMix`.
pub mod color_mix {
    pub const TEXT: f64 = 1.0;
    pub const TEXT_SEC: f64 = 0.60;
    pub const TEXT_MUTED: f64 = 0.40;
    pub const TEXT_FAINT: f64 = 0.25;
    pub const LINE: f64 = 0.50;
    pub const ARROW: f64 = 0.85;
    pub const NODE_FILL: f64 = 0.03;
    pub const NODE_STROKE: f64 = 0.20;
    pub const GROUP_HEADER: f64 = 0.05;
    pub const INNER_STROKE: f64 = 0.12;
    pub const KEY_BADGE: f64 = 0.10;
}

/// `NSBezierPath(rect:)`.
pub fn bezier_rect(rect: CGRect) -> Retained<NSBezierPath> {
    NSBezierPath::bezierPathWithRect(rect)
}

/// `NSBezierPath(roundedRect:cornerRadius:)`.
pub fn bezier_rounded_rect(rect: CGRect, corner_radius: CGFloat) -> Retained<NSBezierPath> {
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(rect, corner_radius, corner_radius)
}

/// `NSBezierPath.bm_cgPath`.
pub fn bm_cg_path(path: &NSBezierPath) -> objc2_core_foundation::CFRetained<CGMutablePath> {
    let out = CGMutablePath::new();
    let mut points = [CGPoint::ZERO; 3];
    for i in 0..path.elementCount() {
        let element = unsafe { path.elementAtIndex_associatedPoints(i, points.as_mut_ptr()) };
        match element {
            NSBezierPathElement::MoveTo => unsafe {
                CGMutablePath::move_to_point(Some(&out), &IDENTITY, points[0].x, points[0].y)
            },
            NSBezierPathElement::LineTo => unsafe {
                CGMutablePath::add_line_to_point(Some(&out), &IDENTITY, points[0].x, points[0].y)
            },
            NSBezierPathElement::CubicCurveTo => unsafe {
                CGMutablePath::add_curve_to_point(
                    Some(&out),
                    &IDENTITY,
                    points[0].x,
                    points[0].y,
                    points[1].x,
                    points[1].y,
                    points[2].x,
                    points[2].y,
                )
            },
            NSBezierPathElement::QuadraticCurveTo => unsafe {
                CGMutablePath::add_quad_curve_to_point(Some(&out), &IDENTITY, points[0].x, points[0].y, points[1].x, points[1].y)
            },
            NSBezierPathElement::ClosePath => CGMutablePath::close_subpath(Some(&out)),
            _ => {}
        }
    }
    out
}

/// `CGPath` from a `CGMutablePath` reference.
pub fn as_path(path: &CGMutablePath) -> &CGPath {
    path
}
