//! `Sources/SwiftMath/MathRender`: parsing, typesetting and drawing.
//!
//! Not ported: `MTMathUILabel` (an `NSView` Downright never creates) apart
//! from its two enums, `MTLabel` (an `NSTextField` used only by the label),
//! `MTBezierPath.swift` (`addLine(to:)` is `lineToPoint:`, and the view
//! background helper is label-only), and `RWLock.swift` (a `std` lock).

pub mod mt_color;
pub mod mt_config;
pub mod mt_font;
pub mod mt_font_manager;
pub mod mt_font_math_table;
pub mod mt_math_atom_factory;
pub mod mt_math_image;
pub mod mt_math_list;
pub mod mt_math_list_builder;
pub mod mt_math_list_display;
pub mod mt_math_list_index;
pub mod mt_math_ui_label;
pub mod mt_typesetter;
pub mod mt_unicode;
