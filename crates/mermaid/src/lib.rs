//! `upleft-mermaid`: beautiful-mermaid-swift (lukilabs, 1.0.4 @ 6a23a29) as
//! Downright drives it, ported to Rust, plus Downright's
//! `MermaidRendererBridge` in [`downright`].
//!
//! One module per Swift file, in the Swift folder layout:
//!
//! | Swift | Rust |
//! |---|---|
//! | `Parser.swift` | [`parser`] |
//! | `Layout.swift` | [`layout`] |
//! | `Types.swift` | [`types`] |
//! | `Theme.swift` | [`theme`] |
//! | `CrossPlatform.swift` | [`cross_platform`] |
//! | `ImageRenderer.swift` (+ `PreparedDiagram`) | [`image_renderer`] |
//! | `Mermaid/src_*.swift` | [`mermaid`]`::src_*` |
//! | `Render/*.swift` | [`render`] |
//! | Downright `MermaidRendererBridge.swift` | [`downright::mermaid_renderer_bridge`] |
//!
//! [`swift`] reproduces the Swift/Foundation behaviours the port relies on
//! (ICU regular expressions, grapheme-cluster string operations, sorting),
//! [`cg`] the CoreGraphics overlay calls, [`error`] the thrown errors.

pub mod cg;
pub mod cross_platform;
pub mod downright;
pub mod error;
pub mod image_renderer;
pub mod layout;
pub mod mermaid;
pub mod parser;
pub mod render;
pub mod swift;
pub mod theme;
pub mod types;

pub use error::MermaidError;
pub use image_renderer::{MermaidImageRenderer, PreparedDiagram};
pub use layout::GraphLayout;
pub use theme::DiagramTheme;
pub use types::{DiagramType, LayoutConfig, MermaidGraph, Payload, PositionedContent, PositionedGraph};
