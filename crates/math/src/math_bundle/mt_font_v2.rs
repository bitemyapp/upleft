//! `MTFontV2.swift`: an `MTFont` over a registered `MathFont`.
//!
//! `MTFontV2` is [`MTFont`] built by [`MathFont::mtfont`]; its copies go back
//! through `MathFont`, and its math table is the V2 flavour (see
//! `mt_font_math_table_v2`).

pub use super::math_font::MathFont;
pub use crate::math_render::mt_font::MTFont as MTFontV2;
