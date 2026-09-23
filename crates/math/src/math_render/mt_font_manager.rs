//! `MTFontManager.swift`: the process-wide cache of loaded math fonts.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, RwLock};

use objc2_core_foundation::CGFloat;

use super::mt_font::MTFont;

pub struct MTFontManager {
    name_to_font_map: RwLock<HashMap<String, Arc<MTFont>>>,
}

static MANAGER: LazyLock<MTFontManager> = LazyLock::new(MTFontManager::new);

impl Default for MTFontManager {
    fn default() -> Self {
        Self::new()
    }
}

impl MTFontManager {
    pub const K_DEFAULT_FONT_SIZE: CGFloat = 20.0;

    pub fn new() -> MTFontManager {
        MTFontManager {
            name_to_font_map: RwLock::new(HashMap::new()),
        }
    }

    /// `MTFontManager.fontManager`.
    pub fn font_manager() -> &'static MTFontManager {
        &MANAGER
    }

    /// The cached font for `name`, or a copy of it at `size` when the cached
    /// one is a different size.
    pub fn font(&self, name: &str, size: CGFloat) -> Option<Arc<MTFont>> {
        let cached = self.name_to_font_map.read().unwrap().get(name).cloned();
        let f = match cached {
            Some(font) => font,
            None => {
                let font = MTFont::with_name(name, size);
                self.name_to_font_map
                    .write()
                    .unwrap()
                    .insert(name.to_owned(), font.clone());
                font
            }
        };
        if f.font_size() == size {
            Some(f)
        } else {
            Some(f.copy_with_size(size))
        }
    }

    pub fn latin_modern_font(&self, size: CGFloat) -> Option<Arc<MTFont>> {
        Self::font_manager().font("latinmodern-math", size)
    }

    pub fn kp_math_light_font(&self, size: CGFloat) -> Option<Arc<MTFont>> {
        Self::font_manager().font("KpMath-Light", size)
    }

    pub fn kp_math_sans_font(&self, size: CGFloat) -> Option<Arc<MTFont>> {
        Self::font_manager().font("KpMath-Sans", size)
    }

    pub fn xits_font(&self, size: CGFloat) -> Option<Arc<MTFont>> {
        Self::font_manager().font("xits-math", size)
    }

    pub fn termes_font(&self, size: CGFloat) -> Option<Arc<MTFont>> {
        Self::font_manager().font("texgyretermes-math", size)
    }

    pub fn asana_font(&self, size: CGFloat) -> Option<Arc<MTFont>> {
        Self::font_manager().font("Asana-Math", size)
    }

    pub fn euler_font(&self, size: CGFloat) -> Option<Arc<MTFont>> {
        Self::font_manager().font("Euler-Math", size)
    }

    pub fn fira_regular_font(&self, size: CGFloat) -> Option<Arc<MTFont>> {
        Self::font_manager().font("FiraMath-Regular", size)
    }

    pub fn noto_sans_regular_font(&self, size: CGFloat) -> Option<Arc<MTFont>> {
        Self::font_manager().font("NotoSansMath-Regular", size)
    }

    pub fn libertinus_regular_font(&self, size: CGFloat) -> Option<Arc<MTFont>> {
        Self::font_manager().font("LibertinusMath-Regular", size)
    }

    pub fn garamond_math_font(&self, size: CGFloat) -> Option<Arc<MTFont>> {
        Self::font_manager().font("Garamond-Math", size)
    }

    pub fn lete_sans_font(&self, size: CGFloat) -> Option<Arc<MTFont>> {
        Self::font_manager().font("LeteSansMath", size)
    }

    /// Latin Modern Math at 20pt.
    pub fn default_font(&self) -> Option<Arc<MTFont>> {
        Self::font_manager().latin_modern_font(Self::K_DEFAULT_FONT_SIZE)
    }
}
