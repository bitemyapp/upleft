//! `upleft-render`: the Rust port of Downright's `MarkdownRender` module.
//!
//! One module per Swift file, in the Swift folder layout:
//!
//! | Swift | Rust |
//! |---|---|
//! | `RenderContracts.swift` | [`render_contracts`] |
//! | `Engine/RenderMetrics.swift` | [`engine::render_metrics`] |
//! | `Engine/SyntaxRunCache.swift` | [`engine::syntax_run_cache`] |
//! | `Syntax/*.swift` | [`syntax`] |
//! | `Theme/*.swift` | [`theme`] |
//! | `View/StyleSheetDefaults.swift` | [`view::style_sheet_defaults`] |
//!
//! [`core_types`] holds the few MarkdownCore types the contracts name until
//! `upleft-core` lands; [`swift_compat`] reproduces the Swift standard
//! library and Foundation behaviours the port depends on.

pub mod core_types;
pub mod engine;
pub mod render_contracts;
pub mod swift_compat;
pub mod syntax;
pub mod theme;
pub mod view;
