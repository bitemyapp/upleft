//! `MathFont.swift`: the bundled math fonts, registered with Core Text on
//! first use and cached per process (`BundleManager`).

// SwiftMath registers with the (deprecated) graphics-font calls.
#![allow(deprecated)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, RwLock};

use objc2_core_foundation::{CFData, CFRetained, CGFloat};
use objc2_core_graphics::{CGDataProvider, CGFont};
use objc2_core_text::{
    CTFont, CTFontManagerRegisterGraphicsFont, CTFontManagerUnregisterGraphicsFont,
};

use super::math_resource_bundle;
use crate::math_render::mt_font::{MTFont, ct_font_with_graphics_font};
use crate::math_render::mt_font_math_table::RawMathTable;

/// The math fonts SwiftMath ships.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MathFont {
    LatinModernFont,
    KpMathLightFont,
    KpMathSansFont,
    XitsFont,
    TermesFont,
    AsanaFont,
    EulerFont,
    FiraFont,
    NotoSansFont,
    LibertinusFont,
    GaramondFont,
    LeteSansFont,
}

impl MathFont {
    pub const ALL_CASES: [MathFont; 12] = [
        MathFont::LatinModernFont,
        MathFont::KpMathLightFont,
        MathFont::KpMathSansFont,
        MathFont::XitsFont,
        MathFont::TermesFont,
        MathFont::AsanaFont,
        MathFont::EulerFont,
        MathFont::FiraFont,
        MathFont::NotoSansFont,
        MathFont::LibertinusFont,
        MathFont::GaramondFont,
        MathFont::LeteSansFont,
    ];

    /// The resource name (`rawValue`).
    pub fn raw_value(self) -> &'static str {
        match self {
            MathFont::LatinModernFont => "latinmodern-math",
            MathFont::KpMathLightFont => "KpMath-Light",
            MathFont::KpMathSansFont => "KpMath-Sans",
            MathFont::XitsFont => "xits-math",
            MathFont::TermesFont => "texgyretermes-math",
            MathFont::AsanaFont => "Asana-Math",
            MathFont::EulerFont => "Euler-Math",
            MathFont::FiraFont => "FiraMath-Regular",
            MathFont::NotoSansFont => "NotoSansMath-Regular",
            MathFont::LibertinusFont => "LibertinusMath-Regular",
            MathFont::GaramondFont => "Garamond-Math",
            MathFont::LeteSansFont => "LeteSansMath",
        }
    }

    pub fn font_family_name(self) -> &'static str {
        match self {
            MathFont::LatinModernFont => "Latin Modern Math",
            MathFont::KpMathLightFont => "KpMath",
            MathFont::KpMathSansFont => "KpMath",
            MathFont::XitsFont => "XITS Math",
            MathFont::TermesFont => "TeX Gyre Termes Math",
            MathFont::AsanaFont => "Asana Math",
            MathFont::EulerFont => "Euler Math",
            MathFont::FiraFont => "Fira Math",
            MathFont::NotoSansFont => "Noto Sans Math",
            MathFont::LibertinusFont => "Libertinus Math",
            MathFont::GaramondFont => "Garamond Math",
            MathFont::LeteSansFont => "Lete Sans Math",
        }
    }

    pub fn font_name(self) -> &'static str {
        self.raw_value()
    }

    pub fn cg_font(self) -> CFRetained<CGFont> {
        BUNDLE_MANAGER.obtain_cg_font(self)
    }

    pub fn ct_font(self, size: CGFloat) -> CFRetained<CTFont> {
        BUNDLE_MANAGER.obtain_ct_font(self, size)
    }

    pub(crate) fn raw_math_table(self) -> Arc<RawMathTable> {
        BUNDLE_MANAGER.obtain_raw_math_table(self)
    }

    /// `mtfont(size:)`: an `MTFontV2`.
    pub fn mtfont(self, size: CGFloat) -> MTFont {
        let cg_font = self.cg_font();
        let ct_font = self.ct_font(size);
        MTFont::v2(self, cg_font, ct_font, self.raw_math_table(), size)
    }
}

#[derive(Debug)]
pub enum FontError {
    InvalidFontFile,
    FontPathNotFound,
    InitFontError,
    RegisterFailed,
    InvalidMathTable,
}

struct CGFontRef(CFRetained<CGFont>);
struct CTFontRef(CFRetained<CTFont>);
// SAFETY: CGFont and CTFont are immutable, thread-safe Core Foundation objects.
unsafe impl Send for CGFontRef {}
unsafe impl Sync for CGFontRef {}
unsafe impl Send for CTFontRef {}
unsafe impl Sync for CTFontRef {}

#[derive(Default)]
struct Tables {
    cg_fonts: HashMap<MathFont, CGFontRef>,
    ct_fonts: HashMap<(MathFont, u64), CTFontRef>,
    raw_math_tables: HashMap<MathFont, Arc<RawMathTable>>,
}

/// `BundleManager`: registration is serialized behind the write lock.
struct BundleManager {
    tables: RwLock<Tables>,
}

static BUNDLE_MANAGER: LazyLock<BundleManager> = LazyLock::new(|| BundleManager {
    tables: RwLock::new(Tables::default()),
});

fn thread_name() -> &'static str {
    if objc2_foundation::NSThread::isMainThread_class() {
        "main"
    } else {
        "global"
    }
}

impl BundleManager {
    fn resource(math_font: MathFont, extension: &str) -> Result<PathBuf, FontError> {
        math_resource_bundle::resource_path(math_font.raw_value(), extension)
            .ok_or(FontError::FontPathNotFound)
    }

    fn register_cg_font(tables: &mut Tables, math_font: MathFont) -> Result<(), FontError> {
        let path = Self::resource(math_font, "otf")?;
        let bytes = std::fs::read(&path).map_err(|_| FontError::InvalidFontFile)?;
        let data = CFData::from_bytes(&bytes);
        let data_provider =
            CGDataProvider::with_cf_data(Some(&data)).ok_or(FontError::InvalidFontFile)?;
        let default_cg_font =
            CGFont::with_data_provider(&data_provider).ok_or(FontError::InitFontError)?;

        tables
            .cg_fonts
            .insert(math_font, CGFontRef(default_cg_font.clone()));

        // This does not load the complete math font, it only has about half the glyphs of the full math font.
        // So we first load a CGFont from the file and then convert it to a CTFont.
        if !unsafe { CTFontManagerRegisterGraphicsFont(&default_cg_font, std::ptr::null_mut()) } {
            return Err(FontError::RegisterFailed);
        }
        let postscript = CGFont::post_script_name(Some(&default_cg_font))
            .map(|n| n.to_string())
            .unwrap_or_default();
        let cgfont_name = CGFont::full_name(Some(&default_cg_font))
            .map(|n| n.to_string())
            .unwrap_or_default();
        println!(
            "\"mathFonts bundle resource: {}, font: {}, ps: {} registered on {}.\"",
            math_font.raw_value(),
            cgfont_name,
            postscript,
            thread_name()
        );
        Ok(())
    }

    fn register_math_table(tables: &mut Tables, math_font: MathFont) -> Result<(), FontError> {
        let path = Self::resource(math_font, "plist")?;
        let raw_math_table = RawMathTable::load(&path).ok_or(FontError::InvalidMathTable)?;
        if raw_math_table.version.as_deref() != Some("1.3") {
            return Err(FontError::InvalidMathTable);
        }
        tables
            .raw_math_tables
            .insert(math_font, Arc::new(raw_math_table));
        println!(
            "\"mathFonts bundle resource: {}.plist registered on {}.\"",
            math_font.raw_value(),
            thread_name()
        );
        Ok(())
    }

    fn on_demand_registration(&self, math_font: MathFont) {
        if self
            .tables
            .read()
            .unwrap()
            .cg_fonts
            .contains_key(&math_font)
        {
            return;
        }
        // Note: resourceLoading is now serialized.
        let mut tables = self.tables.write().unwrap();
        if !tables.cg_fonts.contains_key(&math_font) {
            let result = Self::register_cg_font(&mut tables, math_font)
                .and_then(|()| Self::register_math_table(&mut tables, math_font));
            if let Err(error) = result {
                panic!(
                    "MTMathFonts:onDemandRegistration(mathFont:) ondemand loading failed, mathFont {}, reason {:?}",
                    math_font.raw_value(),
                    error
                );
            }
        }
    }

    fn obtain_cg_font(&self, font: MathFont) -> CFRetained<CGFont> {
        self.on_demand_registration(font);
        let tables = self.tables.read().unwrap();
        tables
            .cg_fonts
            .get(&font)
            .map(|f| f.0.clone())
            .unwrap_or_else(|| panic!("unable to locate CGFont {}", font.font_name()))
    }

    fn obtain_ct_font(&self, font: MathFont, size: CGFloat) -> CFRetained<CTFont> {
        self.on_demand_registration(font);
        let key = (font, size.to_bits());
        if let Some(ct_font) = self.tables.read().unwrap().ct_fonts.get(&key) {
            return ct_font.0.clone();
        }
        let mut tables = self.tables.write().unwrap();
        if let Some(ct_font) = tables.ct_fonts.get(&key) {
            return ct_font.0.clone();
        }
        let cg_font = tables
            .cg_fonts
            .get(&font)
            .map(|f| f.0.clone())
            .expect("CGFont to create CTFont");
        let result = ct_font_with_graphics_font(&cg_font, size);
        tables.ct_fonts.insert(key, CTFontRef(result.clone()));
        result
    }

    fn obtain_raw_math_table(&self, font: MathFont) -> Arc<RawMathTable> {
        self.on_demand_registration(font);
        self.tables
            .read()
            .unwrap()
            .raw_math_tables
            .get(&font)
            .cloned()
            .unwrap_or_else(|| panic!("unable to locate mathTable: {}.plist", font.raw_value()))
    }
}

impl Drop for BundleManager {
    fn drop(&mut self) {
        let tables = self.tables.get_mut().unwrap();
        tables.ct_fonts.clear();
        for cg_font in tables.cg_fonts.values() {
            unsafe { CTFontManagerUnregisterGraphicsFont(&cg_font.0, std::ptr::null_mut()) };
        }
        tables.cg_fonts.clear();
    }
}
