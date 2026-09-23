//! Port of `Mermaid/src_xychart_colors.swift` (from `original/src/xychart/colors.ts`).

use crate::swift::{self, max, min};

pub const CHART_ACCENT_FALLBACK: &str = "#3b82f6";

/// `Int(h.dropFirst(n).prefix(2), radix: 16) ?? 0`, on `Character`s.
fn hex_pair(h: &str, skip: usize) -> i64 {
    let rest = swift::drop_first(h, skip);
    let two = rest.len() - swift::drop_first(rest, 2).len();
    swift::parse_int_hex(&rest[..two]).unwrap_or(0)
}

fn hex_to_hsl(hex: &str) -> (f64, f64, f64) {
    let h = hex.replace('#', "");
    let ri = hex_pair(&h, 0) as f64 / 255.0;
    let gi = hex_pair(&h, 2) as f64 / 255.0;
    let bi = hex_pair(&h, 4) as f64 / 255.0;

    let max_c = swift::max_n(&[ri, gi, bi]);
    let min_c = swift::min_n(&[ri, gi, bi]);
    let l = (max_c + min_c) / 2.0;

    if max_c == min_c {
        return (0.0, 0.0, l * 100.0);
    }

    let d = max_c - min_c;
    let s = if l > 0.5 { d / (2.0 - max_c - min_c) } else { d / (max_c + min_c) };

    let hue = if max_c == ri {
        ((gi - bi) / d + if gi < bi { 6.0 } else { 0.0 }) / 6.0
    } else if max_c == gi {
        ((bi - ri) / d + 2.0) / 6.0
    } else {
        ((ri - gi) / d + 4.0) / 6.0
    };

    (hue * 360.0, s * 100.0, l * 100.0)
}

/// Swift's `round(_:)` (C `round`) then `Int(_:)`.
fn round_int(v: f64) -> i64 {
    swift::int(v.round())
}

fn hsl_to_hex(h: f64, s: f64, l: f64) -> String {
    let si = s / 100.0;
    let li = l / 100.0;

    let c = (1.0 - (2.0 * li - 1.0).abs()) * si;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = li - c / 2.0;

    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    let to_hex = |v: f64| swift::format_i64("%02x", round_int((v + m) * 255.0));
    format!("#{}{}{}", to_hex(r), to_hex(g), to_hex(b))
}

fn hex_to_rgb(hex: &str) -> (i64, i64, i64) {
    let h = hex.replace('#', "");
    (hex_pair(&h, 0), hex_pair(&h, 2), hex_pair(&h, 4))
}

fn rgb_to_hex(r: f64, g: f64, b: f64) -> String {
    let to_hex = |v: f64| swift::format_i64("%02x", round_int(max(0.0, min(255.0, v))));
    format!("#{}{}{}", to_hex(r), to_hex(g), to_hex(b))
}

/// `isValidHex(_:)`.
pub fn is_valid_hex(color: &str) -> bool {
    swift::regex_test(r"^#[0-9a-fA-F]{6}$", color, false)
}

/// `isDarkBackground(_:)`.
pub fn is_dark_background(bg_hex: &str) -> bool {
    hex_to_hsl(bg_hex).2 < 50.0
}

/// `mixHexColors(_:_:_:)`.
pub fn mix_hex_colors(bg_hex: &str, fg_hex: &str, ratio: f64) -> String {
    let bg = hex_to_rgb(bg_hex);
    let fg = hex_to_rgb(fg_hex);
    let inv = 1.0 - ratio;
    rgb_to_hex(
        bg.0 as f64 * inv + fg.0 as f64 * ratio,
        bg.1 as f64 * inv + fg.1 as f64 * ratio,
        bg.2 as f64 * inv + fg.2 as f64 * ratio,
    )
}

/// `getSeriesColor(_:_:_:)`.
pub fn get_series_color(index: i64, accent_color: &str, bg_color: Option<&str>) -> String {
    if index == 0 {
        return accent_color.to_owned();
    }
    let safe_accent = if is_valid_hex(accent_color) { accent_color } else { CHART_ACCENT_FALLBACK };
    let safe_bg = bg_color.filter(|c| is_valid_hex(c));
    let hsl = hex_to_hsl(safe_accent);
    let chart_s = max(55.0, min(85.0, hsl.1));

    let tier = (index as f64 / 2.0).ceil() as i64;
    let odd_index = index % 2 == 1;

    let dark = match safe_bg {
        Some(bg) if is_dark_background(bg) => !odd_index,
        _ => odd_index,
    };
    let l = if dark { max(25.0, 48.0 - tier as f64 * 13.0) } else { min(78.0, 55.0 + tier as f64 * 11.0) };

    let h_shift = (if dark { -8.0 } else { 12.0 }) * tier as f64;
    let new_h = ((hsl.0 + h_shift) % 360.0 + 360.0) % 360.0;

    hsl_to_hex(new_h, chart_s, l)
}
