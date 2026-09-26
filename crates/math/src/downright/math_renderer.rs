//! `MathRenderer.swift`: LaTeX → image, through the one cached door into
//! SwiftMath.

use objc2::rc::Retained;
use objc2::{AnyThread, Message};
use objc2_app_kit::{NSColor, NSColorSpace, NSImage};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{NSCharacterSet, NSPoint, NSRect, NSSize, NSString};

use super::bounded_image_cache::{MATH, MathRendererCacheKey};
use super::math_font_bundle::MathFontBundle;
use crate::math_render::mt_math_image::MTMathImage;
use crate::math_render::mt_math_ui_label::{MTMathUILabelMode, MTTextAlignment};
use crate::swift;

pub struct MathRenderer;

impl MathRenderer {
    /// Typeset LaTeX to an image. `display` selects display vs inline style.
    ///
    /// `point_size` should come from `StyleSheet.mathPointSize` (the body font
    /// measured against its x-height). A padding above zero draws the formula
    /// into a larger image with `padding` points of air on every edge.
    pub fn image(
        latex: &str,
        display: bool,
        point_size: CGFloat,
        color: &NSColor,
        padding: CGFloat,
    ) -> Option<Retained<NSImage>> {
        let trimmed = Self::source(latex)?;
        // SwiftMath traps rather than fails when it cannot find its fonts, so
        // the question has to be settled before we call into it.
        if !MathFontBundle::is_available() {
            return None;
        }
        let key = Self::key(trimmed, display, point_size, color, padding);
        let key_cost = key.source.len();
        MATH.image(&key, key_cost, || {
            let mut renderer = MTMathImage::new(
                &Self::swift_math_source(&key.source),
                key.point_size,
                color.retain(),
                if display {
                    MTMathUILabelMode::Display
                } else {
                    MTMathUILabelMode::Text
                },
                if display {
                    MTTextAlignment::Center
                } else {
                    MTTextAlignment::Left
                },
            );
            let (error, image) = renderer.as_image();
            // A malformed formula is agent output, not a crash: it falls back
            // to its source text in the fragment.
            let image = image?;
            if error.is_some() {
                return None;
            }
            let size = image.size();
            if !(size.width > 0.0 && size.height > 0.0) {
                return None;
            }
            if !(padding > 0.0) {
                return Some(image);
            }
            // SwiftMath crops its bitmap tightly to the glyph bounds, so a
            // formula can sit flush against (or past) its own frame.
            let padded_size = NSSize::new(size.width + padding * 2.0, size.height + padding * 2.0);
            let padded = NSImage::initWithSize(NSImage::alloc(), padded_size);
            #[allow(deprecated)]
            padded.lockFocus();
            image.drawInRect(NSRect::new(
                NSPoint::new(padding, padding),
                NSSize::new(size.width, size.height),
            ));
            #[allow(deprecated)]
            padded.unlockFocus();
            Some(padded)
        })
    }

    fn key(
        trimmed: String,
        display: bool,
        point_size: CGFloat,
        color: &NSColor,
        padding: CGFloat,
    ) -> MathRendererCacheKey {
        MathRendererCacheKey {
            display,
            point_size: (point_size * 4.0).round() / 4.0,
            color_token: Self::color_token(color),
            padding: (padding * 2.0).round(),
            source: trimmed,
        }
    }

    /// The key `image` files this formula under in the shared cache, or
    /// `None` for a blank one. An Upleft extension for hosted views, which
    /// look an image up without typesetting it.
    pub fn cache_key(
        latex: &str,
        display: bool,
        point_size: CGFloat,
        color: &NSColor,
        padding: CGFloat,
    ) -> Option<MathRendererCacheKey> {
        Some(Self::key(Self::source(latex)?, display, point_size, color, padding))
    }

    /// The LaTeX `image` typesets for `latex` (and files its image under),
    /// or `None` for a blank formula: the source trimmed of whitespace and
    /// newlines. An Upleft extension, so a copy of the formula can be checked
    /// against exactly what was drawn.
    pub fn source(latex: &str) -> Option<String> {
        let trimmed = trimming_whitespaces_and_newlines(latex);
        if trimmed.is_empty() {
            return None;
        }
        Some(trimmed)
    }

    /// The cached image for `key`, never typesetting (see `cache_key`).
    pub fn cached_image(key: &MathRendererCacheKey) -> Option<Retained<NSImage>> {
        MATH.cached(key)
    }

    /// SwiftMath does not implement TeX's `\mathop{...}` wrapper; dropping the
    /// wrapper keeps the visible math instead of failing the formula.
    pub fn swift_math_source(latex: &str) -> String {
        let characters: Vec<&str> = swift::characters(latex).collect();
        let mut output = String::with_capacity(latex.len());
        let mut cursor = 0;

        while cursor < characters.len() {
            if characters[cursor] != "\\" {
                output.push_str(characters[cursor]);
                cursor += 1;
                continue;
            }

            let command_start = cursor;
            cursor += 1;
            while cursor < characters.len() && swift::is_letter(characters[cursor]) {
                cursor += 1;
            }
            let command: String = characters[command_start + 1..cursor].concat();

            let closing_brace =
                if command == "mathop" && cursor < characters.len() && characters[cursor] == "{" {
                    Self::matching_brace(&characters, cursor)
                } else {
                    None
                };
            let Some(closing_brace) = closing_brace else {
                output.push_str(&characters[command_start..cursor].concat());
                continue;
            };

            let content_start = cursor + 1;
            output.push_str(&characters[content_start..closing_brace].concat());
            cursor = closing_brace + 1;
        }

        output
    }

    fn matching_brace(source: &[&str], opening: usize) -> Option<usize> {
        let mut depth = 0;
        let mut cursor = opening;

        while cursor < source.len() {
            if source[cursor] == "\\" {
                cursor += 1;
                if cursor < source.len() {
                    cursor += 1;
                }
                continue;
            }
            if source[cursor] == "{" {
                depth += 1;
            } else if source[cursor] == "}" {
                depth -= 1;
                if depth == 0 {
                    return Some(cursor);
                }
            }
            cursor += 1;
        }

        None
    }

    /// `String(format: "%.3f,%.3f,%.3f,%.3f", …)` of the sRGB components.
    pub fn color_token(color: &NSColor) -> String {
        let rgb = color
            .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
            .unwrap_or_else(|| color.retain());
        format!(
            "{:.3},{:.3},{:.3},{:.3}",
            rgb.redComponent(),
            rgb.greenComponent(),
            rgb.blueComponent(),
            rgb.alphaComponent()
        )
    }
}

/// `String.trimmingCharacters(in: .whitespacesAndNewlines)`, through Foundation.
pub fn trimming_whitespaces_and_newlines(text: &str) -> String {
    let first = text.chars().next();
    let last = text.chars().next_back();
    let untrimmed = |c: Option<char>| {
        c.is_some_and(|c| c.is_ascii() && !c.is_ascii_whitespace() && !c.is_ascii_control())
    };
    if untrimmed(first) && untrimmed(last) {
        return text.to_owned();
    }
    NSString::from_str(text)
        .stringByTrimmingCharactersInSet(&NSCharacterSet::whitespaceAndNewlineCharacterSet())
        .to_string()
}
