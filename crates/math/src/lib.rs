//! upleft-math: SwiftMath (as vendored by Downright, `vendor/downright/Vendor/SwiftMath`)
//! and Downright's math rendering, ported to Rust.
//!
//! The module tree follows the Swift sources file by file:
//!
//! - [`math_render`]: `Sources/SwiftMath/MathRender` — the LaTeX parser
//!   (`MTMathListBuilder`), the math list model, the typesetter, the display
//!   tree and its Core Text / Core Graphics drawing, and the fonts.
//! - [`math_bundle`]: `Sources/SwiftMath/MathBundle` — `MathFont`, `MathImage`
//!   and the resource resolution Downright patched in.
//! - [`downright`]: `Sources/MarkdownRender/Fragments/Math*.swift` — the
//!   cached `MathRenderer`, `MathFontBundle`, and the inline attachment.
//!
//! Output is meant to be pixel-identical to the Swift original: the same
//! algorithms, the same floating-point order, and the same framework calls.
//! The usual entry point is [`downright::math_renderer::MathRenderer::image`].

pub mod downright;
pub mod math_bundle;
pub mod math_render;
pub mod swift;

pub use downright::math_renderer::MathRenderer;
pub use math_render::mt_font::MTFont;
pub use math_render::mt_font_manager::MTFontManager;
pub use math_render::mt_math_image::MTMathImage;
pub use math_render::mt_math_list::{MTLineStyle, MTMathAtom, MTMathAtomType, MTMathList};
pub use math_render::mt_math_list_builder::{MTMathListBuilder, MTParseError, MTParseErrors};
pub use math_render::mt_math_list_display::MTDisplay;
pub use math_render::mt_typesetter::MTTypesetter;
