//! `MTMathImage.swift`: LaTeX → `NSImage`, the path Downright uses.

use std::sync::Arc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_app_kit::{NSColor, NSGraphicsContext, NSImage};
use objc2_core_foundation::{CGFloat, CGPoint, CGSize};
use objc2_core_graphics::CGContext;
use objc2_foundation::{NSEdgeInsets, NSRect};

use super::mt_config::MT_EDGE_INSETS_ZERO;
use super::mt_font::MTFont;
use super::mt_font_manager::MTFontManager;
use super::mt_math_list::MTLineStyle;
use super::mt_math_list_builder::{MTMathListBuilder, MTParseError};
use super::mt_math_list_display::MTDisplay;
use super::mt_math_ui_label::{MTMathUILabelMode, MTTextAlignment};
use super::mt_typesetter::MTTypesetter;

pub struct MTMathImage {
    pub font: Option<Arc<MTFont>>,
    font_size: CGFloat,
    pub text_color: Retained<NSColor>,
    pub label_mode: MTMathUILabelMode,
    pub text_alignment: MTTextAlignment,
    pub content_insets: NSEdgeInsets,
    pub latex: String,
    intrinsic_content_size: CGSize,
}

/// The typeset formula `asImage` draws, and the image size it computed.
pub struct MTMathImageLayout {
    pub display_list: MTDisplay,
    pub size: CGSize,
}

/// A display tree handed to an `NSImage` drawing handler. AppKit may run the
/// handler on whichever thread draws the image; the tree is not mutated after
/// layout and holds only Core Foundation and AppKit objects (the math atoms
/// the lines index are stripped first).
struct DrawOnly(MTDisplay);

unsafe impl Send for DrawOnly {}
unsafe impl Sync for DrawOnly {}

impl MTMathImage {
    /// `MTMathImage(latex:fontSize:textColor:labelMode:textAlignment:)`.
    pub fn new(
        latex: &str,
        font_size: CGFloat,
        text_color: Retained<NSColor>,
        label_mode: MTMathUILabelMode,
        text_alignment: MTTextAlignment,
    ) -> MTMathImage {
        let mut image = MTMathImage {
            font: MTFontManager::font_manager().default_font(),
            font_size: 0.0,
            latex: latex.to_owned(),
            text_color,
            label_mode,
            text_alignment,
            content_insets: MT_EDGE_INSETS_ZERO,
            intrinsic_content_size: CGSize::new(0.0, 0.0),
        };
        image.set_font_size(font_size);
        image
    }

    pub fn font_size(&self) -> CGFloat {
        self.font_size
    }

    /// The `fontSize` setter: also replaces the font with a copy at that size.
    pub fn set_font_size(&mut self, font_size: CGFloat) {
        self.font_size = font_size;
        let font = self
            .font
            .as_ref()
            .map(|font| font.copy_with_size(font_size));
        self.font = font; // also forces an update
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

    /// Everything `asImage` does before creating the image: parse, typeset,
    /// size, colour and position the display list.
    pub fn layout(&mut self) -> Result<MTMathImageLayout, Option<MTParseError>> {
        let mut error = None;
        let math_list = MTMathListBuilder::build_from_string_with_error(&self.latex, &mut error);
        let (Some(math_list), None) = (math_list, &error) else {
            return Err(error);
        };
        let font = self.font.clone().expect("font");
        let Some(mut display_list) =
            MTTypesetter::create_line_for_math_list(Some(&math_list), &font, self.current_style())
        else {
            return Err(error);
        };

        self.intrinsic_content_size = self.intrinsic_content_size_for(&display_list);
        display_list.set_text_color(Some(self.text_color.clone()));

        let size = self.intrinsic_content_size;
        self.layout_image(size, &mut display_list);
        Ok(MTMathImageLayout { display_list, size })
    }

    /// `asImage()`: the error, or an `NSImage` whose drawing handler draws the
    /// formula into the current graphics context.
    pub fn as_image(&mut self) -> (Option<MTParseError>, Option<Retained<NSImage>>) {
        match self.layout() {
            Err(error) => (error, None),
            Ok(MTMathImageLayout {
                mut display_list,
                size,
            }) => {
                display_list.strip_atoms();
                (None, Some(image_drawing(display_list, size)))
            }
        }
    }
}

/// `NSImage(size: size, flipped: false) { … displayList.draw(context) … }`.
pub(crate) fn image_drawing(display_list: MTDisplay, size: CGSize) -> Retained<NSImage> {
    let display_list = DrawOnly(display_list);
    let handler = RcBlock::new(move |_bounds: NSRect| -> Bool {
        let Some(current) = NSGraphicsContext::currentContext() else {
            return Bool::NO;
        };
        let context: Retained<CGContext> = current.CGContext();
        CGContext::save_g_state(Some(&context));
        display_list.0.draw(&context);
        CGContext::restore_g_state(Some(&context));
        Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(size, false, &handler)
}
