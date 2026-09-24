//! Port of `Render/DiagramRenderer.swift`: dispatch by diagram type, the
//! font helpers, `_drawTextInFlipped` and `_withFittedContext`. The
//! per-type drawing lives in the `diagram_renderer_*` modules, one per
//! `DiagramRenderer+*.swift` extension.

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSColor, NSFont, NSFontManager, NSFontTraitMask, NSFontWeight, NSFontWeightRegular};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect};
use objc2_core_graphics::CGContext;
use objc2_foundation::NSString;

use super::edge_renderer::EdgeRenderer;
use super::label_renderer::{LabelRenderer, TextAlignment};
use super::render_config::RenderConfig;
use super::shape_renderer::NodeShapeRenderer;
use crate::cg::{self, Ctx};
use crate::theme::DiagramTheme;
use crate::types::{DiagramType, PositionedGraph};

#[derive(Debug, Clone)]
pub struct DiagramRenderer {
    pub theme: DiagramTheme,
    pub config: RenderConfig,
    pub shape_renderer: NodeShapeRenderer,
    pub edge_renderer: EdgeRenderer,
    pub label_renderer: LabelRenderer,
}

impl DiagramRenderer {
    /// `DiagramRenderer(theme:config: .shared)`.
    pub fn new(theme: DiagramTheme) -> DiagramRenderer {
        let config = RenderConfig::SHARED;
        DiagramRenderer {
            theme,
            config,
            shape_renderer: NodeShapeRenderer::new(config),
            edge_renderer: EdgeRenderer::new(config),
            label_renderer: LabelRenderer,
        }
    }

    /// `render(_:in:bounds:)`. The context must already be y=0-at-top.
    pub fn render(&self, positioned: &PositionedGraph, context: &CGContext, bounds: CGRect) {
        let ctx = Ctx(context);
        ctx.save_g_state();

        if !self.theme.transparent {
            ctx.set_fill_color(&self.theme.background.CGColor());
            ctx.fill(bounds);
        }

        match positioned.diagram.diagram_type {
            DiagramType::ClassDiagram => self.draw_class(positioned, context, bounds),
            DiagramType::ErDiagram => self.draw_er(positioned, context, bounds),
            DiagramType::SequenceDiagram => self.draw_sequence(positioned, context, bounds),
            DiagramType::StateDiagram | DiagramType::Flowchart => self.draw_flow_or_state(positioned, context, bounds),
            DiagramType::XyChart => self.draw_xy_chart(positioned, context, bounds),
        }

        ctx.restore_g_state();
    }

    // MARK: - Utility

    /// `_monoFont(size:)`.
    pub(crate) fn mono_font(&self, size: CGFloat) -> Retained<NSFont> {
        NSFont::fontWithName_size(&NSString::from_str("Menlo"), size)
            .unwrap_or_else(|| NSFont::monospacedSystemFontOfSize_weight(size, unsafe { NSFontWeightRegular }))
    }

    /// `_italicSystemFont(size:weight:)`: `NSFontManager.shared.convert(_:toHaveTrait: .italicFontMask)`.
    pub(crate) fn italic_system_font(&self, size: CGFloat, weight: CGFloat) -> Retained<NSFont> {
        let base_font = NSFont::systemFontOfSize_weight(size, weight as NSFontWeight);
        let Some(mtm) = MainThreadMarker::new() else {
            // Off the main thread (a hosted view's worker), where
            // `NSFontManager` must not be used: the same italic through the
            // font descriptor. `hosted_transcript` checks the two agree.
            return italic_by_descriptor(&base_font);
        };
        let manager = NSFontManager::sharedFontManager(mtm);
        manager.convertFont_toHaveTrait(&base_font, NSFontTraitMask::ItalicFontMask)
    }

    /// `_italicMonoFont(size:)`.
    pub(crate) fn italic_mono_font(&self, size: CGFloat) -> Retained<NSFont> {
        NSFont::fontWithName_size(&NSString::from_str("Menlo-Italic"), size)
            .unwrap_or_else(|| NSFont::monospacedSystemFontOfSize_weight(size, unsafe { NSFontWeightRegular }))
    }

    /// `_drawTextInFlipped(_:at:context:contentHeight:color:font:alignment:)`.
    pub(crate) fn draw_text_in_flipped(
        &self,
        text: &str,
        point: CGPoint,
        context: &CGContext,
        color: &NSColor,
        font: &NSFont,
        alignment: TextAlignment,
    ) {
        if text.is_empty() {
            return;
        }
        if crate::swift::contains(text, "\n") {
            // A wide rect centred on the point; drawMultilineText centres the block vertically.
            let rect = cg::rect(point.x - 500.0, point.y - 500.0, 1000.0, 1000.0);
            self.label_renderer.draw_multiline_text(text, rect, context, color, font, TextAlignment::Center);
        } else {
            self.label_renderer.draw_text(text, point, context, color, font, alignment);
        }
    }

    /// `_withFittedContext(_:bounds:contentWidth:contentHeight:draw:)`.
    pub(crate) fn with_fitted_context(
        &self,
        context: &CGContext,
        bounds: CGRect,
        content_width: f64,
        content_height: f64,
        draw: impl FnOnce(&CGContext),
    ) {
        let cw = crate::swift::max(1.0, content_width);
        let ch = crate::swift::max(1.0, content_height);
        let scale = crate::swift::min(cg::width(bounds) / cw, cg::height(bounds) / ch);
        let fitted_width = cw * scale;
        let fitted_height = ch * scale;
        let offset_x = cg::min_x(bounds) + (cg::width(bounds) - fitted_width) / 2.0;
        let offset_y = cg::min_y(bounds) + (cg::height(bounds) - fitted_height) / 2.0;

        let ctx = Ctx(context);
        ctx.save_g_state();
        ctx.translate_by(offset_x, offset_y);
        ctx.scale_by(scale, scale);
        draw(context);
        ctx.restore_g_state();
    }
}

/// `font` with the italic trait, through `NSFontDescriptor` (thread-safe),
/// or `font` itself when no italic face exists.
pub fn italic_by_descriptor(font: &NSFont) -> Retained<NSFont> {
    let descriptor = font.fontDescriptor();
    let traits = descriptor.symbolicTraits() | objc2_app_kit::NSFontDescriptorSymbolicTraits::TraitItalic;
    NSFont::fontWithDescriptor_size(&descriptor.fontDescriptorWithSymbolicTraits(traits), font.pointSize())
        .unwrap_or_else(|| objc2::Message::retain(font))
}
