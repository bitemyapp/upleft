//! `MTFont.swift`: a math font — a `CGFont` loaded from the bundled `.otf`,
//! a `CTFont` at a size, and the font's math table.

use std::ffi::CString;
use std::sync::Arc;

use objc2_core_foundation::{CFRetained, CFString, CGFloat};
use objc2_core_graphics::{CGDataProvider, CGFont, CGGlyph};
use objc2_core_text::CTFont;

use super::mt_font_math_table::{MTFontMathTable, MathTableFlavor, RawMathTable};
use crate::math_bundle::math_font::MathFont;
use crate::math_bundle::math_resource_bundle;

/// `CGFont.name(for:)`, or "" when the glyph has no name.
pub fn glyph_name(font: &CGFont, glyph: CGGlyph) -> String {
    CGFont::glyph_name_for_glyph(Some(font), glyph)
        .map(|name| name.to_string())
        .unwrap_or_default()
}

/// `CGFont.getGlyphWithGlyphName(name:)`.
pub fn glyph_with_name(font: &CGFont, name: &str) -> CGGlyph {
    let name = CFString::from_str(name);
    CGFont::glyph_with_glyph_name(Some(font), Some(&name))
}

/// `CTFontCreateWithGraphicsFont(cgFont, size, nil, nil)`.
pub fn ct_font_with_graphics_font(font: &CGFont, size: CGFloat) -> CFRetained<CTFont> {
    unsafe { CTFont::with_graphics_font(font, size, std::ptr::null(), None) }
}

#[derive(Debug)]
pub struct MTFont {
    default_cg_font: CFRetained<CGFont>,
    ct_font: CFRetained<CTFont>,
    math_table: Option<MTFontMathTable>,
    raw_math_table: Arc<RawMathTable>,
    /// Set for an `MTFontV2`, which copies itself through `MathFont`.
    v2_font: Option<MathFont>,
}

// SAFETY: CGFont and CTFont are immutable, thread-safe Core Foundation
// objects; the math table is read-only after construction.
unsafe impl Send for MTFont {}
unsafe impl Sync for MTFont {}

impl MTFont {
    /// `MTFont(fontWithName:size:)`: loads `name.otf` through a
    /// `CGDataProvider(filename:)` (the font is not registered), then the
    /// `name.plist` math table. Traps, as SwiftMath does, when either is missing.
    pub fn with_name(name: &str, size: CGFloat) -> Arc<MTFont> {
        let font_path = math_resource_bundle::resource_path(name, "otf")
            .unwrap_or_else(|| panic!("mathFonts.bundle has no {name}.otf"));
        let filename = CString::new(font_path.to_str().expect("UTF-8 font path")).unwrap();
        let provider = unsafe { CGDataProvider::with_filename(filename.as_ptr()) }
            .expect("CGDataProvider(filename:)");
        let default_cg_font =
            CGFont::with_data_provider(&provider).expect("CGFont(fontDataProvider)");
        let ct_font = ct_font_with_graphics_font(&default_cg_font, size);

        let plist = math_resource_bundle::resource_path(name, "plist")
            .unwrap_or_else(|| panic!("mathFonts.bundle has no {name}.plist"));
        let raw_math_table =
            Arc::new(RawMathTable::load(&plist).expect("NSDictionary(contentsOf: mathTablePlist)"));
        let math_table = MTFontMathTable::new(
            default_cg_font.clone(),
            ct_font.clone(),
            raw_math_table.clone(),
            MathTableFlavor::V1,
            size,
        );
        Arc::new(MTFont {
            default_cg_font,
            ct_font,
            math_table: Some(math_table),
            raw_math_table,
            v2_font: None,
        })
    }

    /// An `MTFontV2`: the table is a `MTFontMathTableV2` over a registered font.
    pub(crate) fn v2(
        math_font: MathFont,
        default_cg_font: CFRetained<CGFont>,
        ct_font: CFRetained<CTFont>,
        raw_math_table: Arc<RawMathTable>,
        size: CGFloat,
    ) -> MTFont {
        let math_table = MTFontMathTable::new(
            default_cg_font.clone(),
            ct_font.clone(),
            raw_math_table.clone(),
            MathTableFlavor::V2,
            size,
        );
        MTFont {
            default_cg_font,
            ct_font,
            math_table: Some(math_table),
            raw_math_table,
            v2_font: Some(math_font),
        }
    }

    /// The `MathFont` of an `MTFontV2`.
    pub fn math_font(&self) -> Option<MathFont> {
        self.v2_font
    }

    /// Returns a copy of this font but with a different size.
    pub fn copy_with_size(&self, size: CGFloat) -> Arc<MTFont> {
        if let Some(math_font) = self.v2_font {
            // MTFontV2.copy(withSize:) is MTFontV2(font: font, size: size).
            return Arc::new(math_font.mtfont(size));
        }
        let ct_font = ct_font_with_graphics_font(&self.default_cg_font, size);
        let raw_math_table = self.raw_math_table.clone();
        let math_table = MTFontMathTable::new(
            self.default_cg_font.clone(),
            ct_font.clone(),
            raw_math_table.clone(),
            MathTableFlavor::V1,
            size,
        );
        Arc::new(MTFont {
            default_cg_font: self.default_cg_font.clone(),
            ct_font,
            math_table: Some(math_table),
            raw_math_table,
            v2_font: None,
        })
    }

    pub fn get_name_for_glyph(&self, glyph: CGGlyph) -> String {
        glyph_name(&self.default_cg_font, glyph)
    }

    pub fn get_glyph_with_name(&self, name: &str) -> CGGlyph {
        glyph_with_name(&self.default_cg_font, name)
    }

    /// The size of this font in points.
    pub fn font_size(&self) -> CGFloat {
        unsafe { self.ct_font.size() }
    }

    pub fn ct_font(&self) -> &CTFont {
        &self.ct_font
    }

    pub fn default_cg_font(&self) -> &CGFont {
        &self.default_cg_font
    }

    pub fn math_table(&self) -> Option<&MTFontMathTable> {
        self.math_table.as_ref()
    }

    /// The math table, which every font SwiftMath builds has.
    pub fn table(&self) -> &MTFontMathTable {
        self.math_table.as_ref().expect("font.mathTable!")
    }
}
