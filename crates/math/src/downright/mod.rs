//! Downright's side of math rendering (`Sources/MarkdownRender/Fragments`):
//! the pieces that do not depend on TextKit. The fragment classes
//! (`MathFragment`, the inline substitutions) belong to the MarkdownRender
//! port and call in here.

pub mod bounded_image_cache;
pub mod inline_math_display;
pub mod math_font_bundle;
pub mod math_renderer;
