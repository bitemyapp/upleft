//! Port of Downright's `Sources/MarkdownRender/Fragments/MermaidRendererBridge.swift`:
//! Mermaid source → image, themed from the active `StyleSheet`.
//!
//! The Swift wraps the render in `MarkdownFragmentImageCaches.mermaid`
//! (keyed by [`MermaidCacheKey`]); that cache belongs to the fragment layer,
//! so [`image`] here is the uncached body and [`cache_key`] the key the
//! caller files it under.

use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker};
use objc2_app_kit::{NSImage, NSScreen};
use objc2_core_foundation::{CFRetained, CGFloat, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGBitmapContextGetBitsPerComponent,
    CGBitmapContextGetBytesPerRow, CGBitmapContextGetData, CGBitmapContextGetHeight, CGBitmapContextGetWidth,
    CGBitmapInfo, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
};
use upleft_render::theme::style_sheet::StyleSheet;

use crate::cg::{self, Ctx};
use crate::image_renderer::{MermaidImageRenderer, PreparedDiagram};
use crate::swift;
use crate::theme::DiagramTheme;
use crate::types::LayoutConfig;

/// `MermaidCacheKey`: the trimmed source, the style token and the doubled,
/// rounded backing scale.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MermaidCacheKey {
    pub source: String,
    pub style_token: i64,
    pub scale: i64,
}

/// The trimmed source the bridge renders, or `None` for a blank fence.
pub fn trimmed_source(source: &str) -> Option<&str> {
    let trimmed = swift::trim_whitespaces_and_newlines(source);
    if trimmed.is_empty() { None } else { Some(trimmed) }
}

/// The cache key `image(source:styleSheet:)` looks the diagram up under;
/// `style_token` is `StyleToken.of(styleSheet)`.
pub fn cache_key(source: &str, style_token: i64) -> Option<MermaidCacheKey> {
    let trimmed = trimmed_source(source)?;
    Some(MermaidCacheKey { source: trimmed.to_owned(), style_token, scale: swift::int((scale() * 2.0).round()) })
}

/// The finished image: the ink-cropped bitmap and its size in points.
#[derive(Debug, Clone)]
pub struct MermaidImage {
    pub cg_image: CFRetained<CGImage>,
    pub size: CGSize,
}

impl MermaidImage {
    /// `NSImage(cgImage:size:)`, as the Swift returns it.
    pub fn ns_image(&self) -> Retained<NSImage> {
        NSImage::initWithCGImage_size(NSImage::alloc(), &self.cg_image, self.size)
    }
}

/// `image(source:styleSheet:)` without the cache: render Mermaid source,
/// themed from the style sheet. `None` for a blank source, a parse or
/// layout error, or an empty diagram.
pub fn image(source: &str, style_sheet: &StyleSheet) -> Option<MermaidImage> {
    let trimmed = trimmed_source(source)?;
    let scale = scale();
    // Render via `prepare(from:)` into our own bitmap context, flipped to
    // y=0-at-top first (see the Swift for why `renderImage` is not used).
    let renderer = MermaidImageRenderer::new(theme(style_sheet), LayoutConfig::default());
    let prepared = renderer.prepare(trimmed).ok().flatten()?;
    objc2::rc::autoreleasepool(|_| render(&prepared, scale))
}

/// `render(_:scale:)`: draws into a bitmap flipped to y=0-at-top with 32 pt
/// of slack, then crops to the pixels the diagram inked.
pub fn render(prepared: &PreparedDiagram, scale: CGFloat) -> Option<MermaidImage> {
    let bounds = prepared.bounds;
    if !(cg::width(bounds) > 0.0 && cg::height(bounds) > 0.0) {
        return None;
    }
    // Slack only so a stroke, shadow, or overshooting label near the edge
    // is not clipped before it can be measured; the crop takes it back.
    let slack: CGFloat = 32.0;
    let padded = cg::inset(bounds, -slack, -slack);
    let pixel_width = swift::int((cg::width(padded) * scale).round());
    let pixel_height = swift::int((cg::height(padded) * scale).round());
    if !(pixel_width > 0 && pixel_height > 0) {
        return None;
    }
    let ctx = bitmap_context(pixel_width as usize, pixel_height as usize)?;
    let c = Ctx(&ctx);
    c.translate_by(0.0, pixel_height as CGFloat);
    c.scale_by(1.0, -1.0);
    c.scale_by(scale, scale);
    c.translate_by(-cg::min_x(padded), -cg::min_y(padded));
    prepared.render(&ctx, bounds);
    let cg_image = CGBitmapContextCreateImage(Some(&ctx))?;

    let cropped = ink_bounds(&ctx).and_then(|ink| CGImage::with_image_in_rect(Some(&cg_image), ink));
    match cropped {
        Some(cropped) => {
            let size = CGSize {
                width: CGImage::width(Some(&cropped)) as CGFloat / scale,
                height: CGImage::height(Some(&cropped)) as CGFloat / scale,
            };
            Some(MermaidImage { cg_image: cropped, size })
        }
        None => Some(MermaidImage { cg_image, size: padded.size }),
    }
}

/// `CGContext(data: nil, width:height:bitsPerComponent: 8, bytesPerRow: 0,
/// space: DeviceRGB, bitmapInfo: premultipliedLast | byteOrder32Big)`.
fn bitmap_context(width: usize, height: usize) -> Option<CFRetained<CGContext>> {
    let space = CGColorSpace::new_device_rgb()?;
    // `CGBitmapInfo.byteOrder32Big` (kCGBitmapByteOrder32Big).
    #[allow(deprecated)]
    let info = CGImageAlphaInfo::PremultipliedLast.0 | CGBitmapInfo::ByteOrder32Big.0;
    unsafe { CGBitmapContextCreate(std::ptr::null_mut(), width, height, 8, 0, Some(&space), info) }
}

/// `inkBounds(of:)`: the smallest pixel rect holding every pixel whose alpha
/// is above the noise floor (8), in `CGImage.cropping(to:)` coordinates.
pub fn ink_bounds(ctx: &CGContext) -> Option<CGRect> {
    let base = CGBitmapContextGetData(Some(ctx));
    if base.is_null() || CGBitmapContextGetBitsPerComponent(Some(ctx)) != 8 {
        return None;
    }
    let width = CGBitmapContextGetWidth(Some(ctx));
    let height = CGBitmapContextGetHeight(Some(ctx));
    let stride = CGBitmapContextGetBytesPerRow(Some(ctx));
    let pixels = unsafe { std::slice::from_raw_parts(base as *const u8, stride * height) };
    let alpha_floor: u8 = 8;

    let (mut min_x, mut min_y, mut max_x, mut max_y) = (width as i64, height as i64, -1i64, -1i64);
    for y in 0..height {
        let row = &pixels[y * stride..y * stride + width * 4];
        let mut row_min_x: i64 = -1;
        let mut row_max_x: i64 = -1;
        // First and last inked pixel of the row (the Swift scans every pixel;
        // the extremes are the same). Rows are mostly blank, so test eight
        // pixels at a time first: alpha is the high byte of each
        // little-endian RGBA word.
        if row_has_ink(row, alpha_floor) {
            if let Some(first) = row.chunks_exact(4).position(|p| p[3] > alpha_floor) {
                row_min_x = first as i64;
                let last = row.chunks_exact(4).rposition(|p| p[3] > alpha_floor).unwrap();
                row_max_x = last as i64;
            }
        }
        if row_min_x < 0 {
            continue;
        }
        min_x = swift::min(min_x, row_min_x);
        max_x = swift::max(max_x, row_max_x);
        min_y = swift::min(min_y, y as i64);
        max_y = swift::max(max_y, y as i64);
    }
    if !(max_x >= min_x && max_y >= min_y) {
        return None;
    }
    Some(cg::rect(min_x as f64, min_y as f64, (max_x - min_x + 1) as f64, (max_y - min_y + 1) as f64))
}

/// Whether any pixel of an RGBA row has alpha above `floor`.
fn row_has_ink(row: &[u8], floor: u8) -> bool {
    let threshold = ((floor as u32) << 24) | 0x00FF_FFFF;
    let mut chunks = row.chunks_exact(32);
    for chunk in &mut chunks {
        let mut high = 0u32;
        for pixel in chunk.chunks_exact(4) {
            high = high.max(u32::from_le_bytes([pixel[0], pixel[1], pixel[2], pixel[3]]));
        }
        if high > threshold {
            return true;
        }
    }
    chunks.remainder().chunks_exact(4).any(|p| p[3] > floor)
}

/// `theme(from:)`: the style sheet's palette as a diagram theme.
pub fn theme(style_sheet: &StyleSheet) -> DiagramTheme {
    DiagramTheme {
        background: style_sheet.background.clone(),
        foreground: style_sheet.text.clone(),
        line: Some(style_sheet.rule.clone()),
        accent: Some(style_sheet.accent.clone()),
        muted: Some(style_sheet.text_secondary.clone()),
        surface: Some(style_sheet.code_background.clone()),
        border: Some(style_sheet.code_rule.clone()),
        font: style_sheet.body_font(),
        line_width: if style_sheet.increase_contrast { 2.25 } else { 1.75 },
        corner_radius: 8.0,
        // Composited over the document background, which the fragment
        // already fills; an opaque diagram card would read as a bordered box.
        transparent: true,
    }
}

/// `scale()`: the main screen's backing scale factor, or 2.
pub fn scale() -> CGFloat {
    // The Swift reads `NSScreen.main` from whatever thread renders.
    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    NSScreen::mainScreen(mtm).map_or(2.0, |screen| screen.backingScaleFactor())
}
