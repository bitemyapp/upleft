//! Port of `Sources/MarkdownRender/Fragments` (the parts ported so far).
//!
//! `fragment_base` and `inline_code_pill` came with the view layer; the
//! object fragments (tables, code, callouts, images, lists, front matter,
//! math, Mermaid, thematic breaks) plug into `view::fragment_provider`
//! through its object-fragment constructors.

pub mod footnote_reference_display;
pub mod fragment_base;
pub mod inline_code_pill;
pub mod inline_math_display;
pub mod list_ornament_fragment;
pub mod thematic_break_fragment;
