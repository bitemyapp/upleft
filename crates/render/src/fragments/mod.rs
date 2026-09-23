//! Port of `Sources/MarkdownRender/Fragments` (the parts ported so far).
//!
//! `fragment_base` and `inline_code_pill` came with the view layer; the
//! object fragments (tables, code, callouts, images, lists, front matter,
//! math, Mermaid, thematic breaks) plug into `view::fragment_provider`
//! through its object-fragment constructors.

pub mod bounded_image_cache;
pub mod callout_fragment;
pub mod code_block_fragment;
pub mod footnote_reference_display;
pub mod front_matter_fragment;
pub mod fragment_base;
pub mod image_fragment;
pub mod inline_code_pill;
pub mod inline_math_display;
pub mod list_ornament_fragment;
pub mod local_asset_policy;
pub mod math_fragment;
pub mod mermaid_fragment;
pub mod table_fragment;
pub mod thematic_break_fragment;
