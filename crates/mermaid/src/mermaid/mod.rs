//! Ports of `Sources/BeautifulMermaidSwift/Mermaid/src_*.swift` (themselves
//! transpiled from the TypeScript beautiful-mermaid), one module per file.
//! The ASCII renderers, the SVG renderers and the Shiki theme import are not
//! reachable from Downright's image path and are not ported.

pub mod src_class_layout;
pub mod src_class_parser;
pub mod src_elk_instance;
pub mod src_er_layout;
pub mod src_er_parser;
pub mod src_layout;
pub mod src_multiline_utils;
pub mod src_parser;
pub mod src_sequence_layout;
pub mod src_sequence_parser;
pub mod src_styles;
pub mod src_text_metrics;
pub mod src_types;
pub mod src_xychart_colors;
pub mod src_xychart_layout;
pub mod src_xychart_parser;
pub mod src_xychart_types;
