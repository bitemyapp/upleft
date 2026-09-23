//! `MTConfig.swift`: the platform type aliases (macOS side).

use objc2_foundation::NSEdgeInsets;

pub type MTColor = objc2_app_kit::NSColor;
pub type MTBezierPath = objc2_app_kit::NSBezierPath;
pub type MTEdgeInsets = NSEdgeInsets;
pub type MTRect = objc2_foundation::NSRect;
pub type MTImage = objc2_app_kit::NSImage;

pub const MT_EDGE_INSETS_ZERO: NSEdgeInsets = NSEdgeInsets {
    top: 0.0,
    left: 0.0,
    bottom: 0.0,
    right: 0.0,
};
