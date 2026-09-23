//! Port of the bitmap path of `Views/MermaidLayer.swift`
//! (`MermaidLayer.renderImage(scale:)`, AppKit branch). Downright never
//! draws through it; its orientation probe uses it as the known-good render.

use objc2::rc::Retained;
use objc2::AnyThread;
use objc2_app_kit::{NSGraphicsContext, NSImage};
use objc2_core_foundation::CGSize;

use crate::cg::{self, Ctx};
use crate::image_renderer::MermaidImageRenderer;
use crate::theme::DiagramTheme;
use crate::types::LayoutConfig;

/// A `MermaidLayer` with `source`, `theme` and `layoutConfig` set, reduced
/// to what `renderImage(scale:)` reads.
pub struct MermaidLayer {
    pub source: String,
    pub theme: DiagramTheme,
    pub layout_config: LayoutConfig,
}

impl MermaidLayer {
    /// `renderImage(scale:)`: an `NSImage` of `bounds * scale` points drawn
    /// with `lockFocus`, flipped to y=0-at-top.
    pub fn render_image(&self, scale: f64) -> Option<Retained<NSImage>> {
        if self.source.is_empty() {
            return None;
        }
        let renderer = MermaidImageRenderer::new(self.theme.clone(), self.layout_config);
        let prepared = renderer.prepare(&self.source).ok().flatten()?;
        let diag_bounds = prepared.bounds;
        if !(cg::width(diag_bounds) > 0.0 && cg::height(diag_bounds) > 0.0) {
            return None;
        }

        let size = CGSize { width: cg::width(diag_bounds) * scale, height: cg::height(diag_bounds) * scale };
        let image = NSImage::initWithSize(NSImage::alloc(), size);
        #[allow(deprecated)]
        image.lockFocus();

        let Some(ns_ctx) = NSGraphicsContext::currentContext() else {
            #[allow(deprecated)]
            image.unlockFocus();
            return None;
        };
        let cg_ctx = ns_ctx.CGContext();
        let ctx = Ctx(&cg_ctx);

        if !self.theme.transparent {
            ctx.set_fill_color(&self.theme.background.CGColor());
            ctx.fill(cg::rect(0.0, 0.0, size.width, size.height));
        }

        // Flip for AppKit (lockFocus context has y=0 at bottom)
        ctx.translate_by(0.0, size.height);
        ctx.scale_by(1.0, -1.0);

        ctx.scale_by(scale, scale);
        ctx.translate_by(-cg::min_x(diag_bounds), -cg::min_y(diag_bounds));

        prepared.render(&cg_ctx, diag_bounds);

        #[allow(deprecated)]
        image.unlockFocus();
        Some(image)
    }
}
