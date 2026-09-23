//! Ports of `Sources/BeautifulMermaidSwift/Render/*.swift`, the CoreGraphics
//! renderers. `ArrowRenderer.swift` and `SVGHelpers.swift` (except `_hex`,
//! in [`crate::cross_platform::hex`]) are not reachable from the image path.

pub mod diagram_renderer;
pub mod diagram_renderer_class;
pub mod diagram_renderer_er;
pub mod diagram_renderer_flow;
pub mod diagram_renderer_sequence;
pub mod diagram_renderer_xy_chart;
pub mod edge_renderer;
pub mod label_renderer;
pub mod render_adapters;
pub mod render_config;
pub mod shape_renderer;
