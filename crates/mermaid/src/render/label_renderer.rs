//! Port of `Render/LabelRenderer.swift` (the AppKit branch): labels are
//! `NSAttributedString`s drawn with `draw(in:)` through an `NSGraphicsContext`
//! wrapping the diagram's `CGContext`, after a local flip about the text's
//! vertical centre.

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSAttributedStringNSStringDrawing, NSColor, NSFont, NSFontAttributeName, NSForegroundColorAttributeName,
    NSGraphicsContext, NSStringDrawing,
};
use objc2_core_foundation::{CGFloat, CGRect, CGSize};
use objc2_core_graphics::CGContext;
use objc2_foundation::{NSAttributedString, NSDictionary, NSString};

use crate::cg::{self, Ctx};
use crate::swift;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlignment {
    Left,
    Center,
    Right,
}

/// `[.font: font, .foregroundColor: color]`.
pub fn attributes(font: &NSFont, color: Option<&NSColor>) -> Retained<NSDictionary<NSString, AnyObject>> {
    let (font_key, color_key) = unsafe { (NSFontAttributeName, NSForegroundColorAttributeName) };
    match color {
        Some(color) => {
            let values: [&AnyObject; 2] = [font.as_ref(), color.as_ref()];
            NSDictionary::from_slices(&[font_key, color_key], &values)
        }
        None => {
            let values: [&AnyObject; 1] = [font.as_ref()];
            NSDictionary::from_slices(&[font_key], &values)
        }
    }
}

/// `NSAttributedString(string:attributes:)`.
pub fn attributed(text: &str, attributes: &NSDictionary<NSString, AnyObject>) -> Retained<NSAttributedString> {
    unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(text),
            Some(attributes),
        )
    }
}

/// `(text as NSString).size(withAttributes:)`.
pub fn string_size(text: &str, attributes: &NSDictionary<NSString, AnyObject>) -> CGSize {
    unsafe { NSString::from_str(text).sizeWithAttributes(Some(attributes)) }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LabelRenderer;

impl LabelRenderer {
    /// `drawText(_:at:context:color:font:alignment:)`.
    pub fn draw_text(
        &self,
        text: &str,
        point: objc2_core_foundation::CGPoint,
        context: &CGContext,
        color: &NSColor,
        font: &NSFont,
        alignment: TextAlignment,
    ) {
        if text.is_empty() {
            return;
        }

        let attributes = attributes(font, Some(color));
        let attributed_string = attributed(text, &attributes);
        let size = attributed_string.size();

        let mut x = point.x;
        let y = point.y - size.height / 2.0;

        match alignment {
            TextAlignment::Left => {}
            TextAlignment::Center => x = point.x - size.width / 2.0,
            TextAlignment::Right => x = point.x - size.width,
        }

        let rect = cg::rect(x, y, size.width, size.height);
        Self::draw_attributed_string(&attributed_string, rect, context);
    }

    fn draw_attributed_string(attributed_string: &NSAttributedString, rect: CGRect, context: &CGContext) {
        let ctx = Ctx(context);
        ctx.save_g_state();

        let center_y = cg::mid_y(rect);
        ctx.translate_by(0.0, center_y);
        ctx.scale_by(1.0, -1.0);
        ctx.translate_by(0.0, -center_y);

        // Ensure NSGraphicsContext is available (required for NSAttributedString.draw).
        // flipped: false because the local CTM unflip above restored y=0-at-bottom.
        let needs_context = NSGraphicsContext::currentContext().is_none();
        if needs_context {
            let ns_ctx = NSGraphicsContext::graphicsContextWithCGContext_flipped(context, false);
            NSGraphicsContext::setCurrentContext(Some(&ns_ctx));
        }
        attributed_string.drawInRect(rect);
        if needs_context {
            NSGraphicsContext::setCurrentContext(None);
        }

        ctx.restore_g_state();
    }

    /// `drawMultilineText(_:in:context:color:font:alignment:lineSpacing:)`:
    /// lines centred on `rect.midY` at `fontSize * 1.3` apart.
    pub fn draw_multiline_text(
        &self,
        text: &str,
        rect: CGRect,
        context: &CGContext,
        color: &NSColor,
        font: &NSFont,
        alignment: TextAlignment,
    ) {
        if text.is_empty() {
            return;
        }

        let lines = swift::components_separated_by_newline(text);
        let attributes = attributes(font, Some(color));

        let font_size: CGFloat = font.pointSize();
        let line_height = font_size * 1.3;

        let block_height = lines.len() as f64 * line_height;
        let start_y = cg::mid_y(rect) - block_height / 2.0;

        for (i, line) in lines.iter().enumerate() {
            if line.is_empty() {
                continue;
            }
            let attr_str = attributed(line, &attributes);
            let size = attr_str.size();

            let y = start_y + i as f64 * line_height;
            let x = match alignment {
                TextAlignment::Left => cg::min_x(rect),
                TextAlignment::Center => cg::mid_x(rect) - size.width / 2.0,
                TextAlignment::Right => cg::max_x(rect) - size.width,
            };

            let line_rect = cg::rect(x, y, size.width, size.height);
            Self::draw_attributed_string(&attr_str, line_rect, context);
        }
    }

    /// `measureText(_:font:)`.
    pub fn measure_text(&self, text: &str, font: &NSFont) -> CGSize {
        attributed(text, &attributes(font, None)).size()
    }
}
