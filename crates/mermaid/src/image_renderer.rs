//! Port of `ImageRenderer.swift` (`MermaidImageRenderer.prepare(from:)`) and
//! `PreparedDiagram` from `Views/MermaidLayer.swift`.

use objc2_core_foundation::CGRect;
use objc2_core_graphics::CGContext;

use crate::cg;
use crate::error::MermaidError;
use crate::layout::GraphLayout;
use crate::parser;
use crate::render::diagram_renderer::DiagramRenderer;
use crate::theme::DiagramTheme;
use crate::types::{LayoutConfig, PositionedGraph};

/// A prepared diagram ready for direct `CGContext` rendering.
#[derive(Debug, Clone)]
pub struct PreparedDiagram {
    /// The bounds of the diagram content.
    pub bounds: CGRect,
    pub positioned: PositionedGraph,
    renderer: DiagramRenderer,
}

impl PreparedDiagram {
    /// The Swift `render` closure: draws into a context that is already
    /// y=0-at-top.
    pub fn render(&self, context: &CGContext, render_bounds: CGRect) {
        self.renderer.render(&self.positioned, context, render_bounds);
    }
}

#[derive(Debug, Clone)]
pub struct MermaidImageRenderer {
    pub theme: DiagramTheme,
    pub layout_config: LayoutConfig,
    pub scale: f64,
}

impl MermaidImageRenderer {
    pub fn new(theme: DiagramTheme, config: LayoutConfig) -> MermaidImageRenderer {
        MermaidImageRenderer { theme, layout_config: config, scale: 2.0 }
    }

    /// `prepare(from:)`: parse, lay out, and capture a renderer.
    pub fn prepare(&self, source: &str) -> Result<Option<PreparedDiagram>, MermaidError> {
        let graph = parser::parse(source)?;
        let layout = GraphLayout::new(self.layout_config);
        let positioned = layout.layout(&graph)?;
        let renderer = DiagramRenderer::new(self.theme.clone());

        let bounds = cg::rect(0.0, 0.0, crate::swift::max(1.0, positioned.width), crate::swift::max(1.0, positioned.height));
        Ok(Some(PreparedDiagram { bounds, positioned, renderer }))
    }
}
