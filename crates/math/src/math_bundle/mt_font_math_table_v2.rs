//! `MTFontMathTableV2.swift`: the math table of an `MTFontV2`.
//!
//! It overrides every lookup of `MTFontMathTable` to read zero (or skip) where
//! the original traps on a missing plist entry, and to scale by the size it
//! was asked for. That is [`MathTableFlavor::V2`] of the one table type.

pub use crate::math_render::mt_font_math_table::{
    MTFontMathTable as MTFontMathTableV2, MathTableFlavor,
};
