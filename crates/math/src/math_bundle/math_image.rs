//! `MathImage.swift`: the `MathFont`-based image renderer.

use std::sync::Arc;

use objc2::rc::Retained;
use objc2_app_kit::{NSColor, NSImage};
use objc2_core_foundation::{CGFloat, CGPoint, CGSize};
use objc2_foundation::NSEdgeInsets;

use super::math_font::MathFont;
use crate::math_render::mt_config::MT_EDGE_INSETS_ZERO;
use crate::math_render::mt_math_image::image_drawing;
use crate::math_render::mt_math_list::MTLineStyle;
use crate::math_render::mt_math_list_builder::{MTMathListBuilder, MTParseError};
use crate::math_render::mt_math_list_display::MTDisplay;
use crate::math_render::mt_math_ui_label::{MTMathUILabelMode, MTTextAlignment};
use crate::math_render::mt_typesetter::MTTypesetter;

pub struct MathImage {
    pub font: MathFont,
    pub font_size: CGFloat,
    pub text_color: Retained<NSColor>,
    pub label_mode: MTMathUILabelMode,
    pub text_alignment: MTTextAlignment,
    pub content_insets: NSEdgeInsets,
    pub latex: String,
    intrinsic_content_size: CGSize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayoutInfo {
    pub ascent: CGFloat,
    pub descent: CGFloat,
}

impl MathImage {
    pub fn new(
        latex: &str,
        font_size: CGFloat,
        text_color: Retained<NSColor>,
        label_mode: MTMathUILabelMode,
        text_alignment: MTTextAlignment,
    ) -> MathImage {
        MathImage {
            font: MathFont::LatinModernFont,
            font_size,
            text_color,
            label_mode,
            text_alignment,
            content_insets: MT_EDGE_INSETS_ZERO,
            latex: latex.to_owned(),
            intrinsic_content_size: CGSize::new(0.0, 0.0),
        }
    }

    pub fn intrinsic_content_size(&self) -> CGSize {
        self.intrinsic_content_size
    }

    pub fn current_style(&self) -> MTLineStyle {
        match self.label_mode {
            MTMathUILabelMode::Display => MTLineStyle::Display,
            MTMathUILabelMode::Text => MTLineStyle::Text,
        }
    }

    fn intrinsic_content_size_for(&self, display_list: &MTDisplay) -> CGSize {
        CGSize::new(
            display_list.width() + self.content_insets.left + self.content_insets.right,
            display_list.ascent()
                + display_list.descent()
                + self.content_insets.top
                + self.content_insets.bottom,
        )
    }

    fn layout_image(&self, size: CGSize, display_list: &mut MTDisplay) {
        let insets = self.content_insets;
        let text_x = match self.text_alignment {
            MTTextAlignment::Left => insets.left,
            MTTextAlignment::Center => {
                (size.width - insets.left - insets.right - display_list.width()) / 2.0 + insets.left
            }
            MTTextAlignment::Right => size.width - display_list.width() - insets.right,
        };
        let available_height = size.height - insets.bottom - insets.top;

        // center things vertically
        let mut height = display_list.ascent() + display_list.descent();
        if height < self.font_size / 2.0 {
            height = self.font_size / 2.0; // set height to half the font size
        }
        let text_y = (available_height - height) / 2.0 + display_list.descent() + insets.bottom;
        display_list.set_position(CGPoint::new(text_x, text_y));
    }

    /// `asImage()`: the error, or the image and the formula's ascent and descent.
    pub fn as_image(
        &mut self,
    ) -> (
        Option<MTParseError>,
        Option<Retained<NSImage>>,
        Option<LayoutInfo>,
    ) {
        let mtfont = Arc::new(self.font.mtfont(self.font_size));

        let mut error = None;
        let math_list = MTMathListBuilder::build_from_string_with_error(&self.latex, &mut error);
        let (Some(math_list), None) = (math_list, &error) else {
            return (error, None, None);
        };
        let Some(mut display_list) = MTTypesetter::create_line_for_math_list(
            Some(&math_list),
            &mtfont,
            self.current_style(),
        ) else {
            return (error, None, None);
        };

        self.intrinsic_content_size = self.intrinsic_content_size_for(&display_list);
        display_list.set_text_color(Some(self.text_color.clone()));

        // regularized: whole points.
        let size = CGSize::new(
            self.intrinsic_content_size.width.ceil(),
            self.intrinsic_content_size.height.ceil(),
        );
        self.layout_image(size, &mut display_list);

        let info = LayoutInfo {
            ascent: display_list.ascent(),
            descent: display_list.descent(),
        };
        display_list.strip_atoms();
        (None, Some(image_drawing(display_list, size)), Some(info))
    }
}
