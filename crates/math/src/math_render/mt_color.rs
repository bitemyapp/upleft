//! `MTColor.swift`: `MTColor(fromHexString:)`.

use objc2::rc::Retained;
use objc2_app_kit::NSColor;
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSCharacterSet, NSScanner, NSString};

/// `MTColor(fromHexString:)`: nil unless the string starts with `#`; the
/// rest is read by `Scanner.scanHexInt64` (so `#12zz` is `0x12`).
pub fn color_from_hex_string(hex_string: &str) -> Option<Retained<NSColor>> {
    if hex_string.is_empty() {
        return None;
    }
    if !hex_string.starts_with('#') {
        return None;
    }

    let mut rgb_value: u64 = 0;
    let scanner = NSScanner::scannerWithString(&NSString::from_str(hex_string));
    scanner.setCharactersToBeSkipped(Some(&NSCharacterSet::characterSetWithCharactersInString(
        &NSString::from_str("#"),
    )));
    unsafe { scanner.scanHexLongLong(&mut rgb_value) };
    Some(NSColor::colorWithRed_green_blue_alpha(
        ((rgb_value & 0xFF0000) >> 16) as CGFloat / 255.0,
        ((rgb_value & 0xFF00) >> 8) as CGFloat / 255.0,
        (rgb_value & 0xFF) as CGFloat / 255.0,
        1.0,
    ))
}
