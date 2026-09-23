//! `InlineMathDisplay.swift`, the parts that do not need the parsed document:
//! `MathRenderer.inlineAttachment` and its baseline arithmetic.
//!
//! `InlineMathDisplay.ranges`/`substitutions` walk a `ParsedDocument` and
//! build `DisplaySubstitution`s; they belong with the MarkdownRender port and
//! call [`inline_attachment`] for each formula.

use objc2::rc::Retained;
use objc2_app_kit::{NSAttributedStringAttachmentConveniences, NSColor, NSFont, NSTextAttachment};
use objc2_core_foundation::{CGFloat, CGPoint, CGRect, CGSize};
use objc2_foundation::{NSAttributedString, NSSize};

use super::math_renderer::MathRenderer;

/// The attachment bounds for an inline formula image: centred on the body
/// font's x-height.
pub fn inline_attachment_bounds(image_size: NSSize, x_height: CGFloat) -> CGRect {
    let baseline_offset = (x_height - image_size.height) / 2.0;
    CGRect::new(
        CGPoint::new(0.0, baseline_offset),
        CGSize::new(image_size.width, image_size.height),
    )
}

/// `MathRenderer.inlineAttachment(latex:pointSize:color:font:)`: a one-character
/// attributed string holding the typeset formula, or nil when it does not typeset.
pub fn inline_attachment(
    latex: &str,
    point_size: CGFloat,
    color: &NSColor,
    font: &NSFont,
) -> Option<Retained<NSAttributedString>> {
    let image = MathRenderer::image(latex, false, point_size, color, 0.0)?;
    let size = image.size();
    if !(size.width > 0.0 && size.height > 0.0) {
        return None;
    }

    let attachment = NSTextAttachment::new();
    attachment.setImage(Some(&image));
    attachment.setBounds(inline_attachment_bounds(size, font.xHeight()));
    Some(NSAttributedString::attributedStringWithAttachment(
        &attachment,
    ))
}
