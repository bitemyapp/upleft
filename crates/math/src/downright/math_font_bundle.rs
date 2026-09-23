//! `MathFontBundle.swift`: whether SwiftMath can reach its fonts, answered
//! once, before any formula is handed to it.
//!
//! SwiftMath traps when `latinmodern-math.otf` is missing; Downright guards
//! every call with this predicate so a broken install degrades to "Formula
//! could not be typeset". Upleft resolves `mathFonts.bundle` itself
//! (`math_resource_bundle`), so the predicate asks the same question of the
//! same candidates the resolver uses: is the `.otf` in the first
//! `mathFonts.bundle` directory it would accept?

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::math_bundle::math_resource_bundle;

/// Latin Modern is the face every render starts from (`MTFontManager.latinModernFont`).
const PROBE_FILE: &str = "latinmodern-math.otf";

static IS_AVAILABLE: LazyLock<bool> = LazyLock::new(|| {
    math_resource_bundle::math_fonts_directory()
        .is_some_and(|directory| directory.join(PROBE_FILE).exists())
});

pub struct MathFontBundle;

impl MathFontBundle {
    /// `true` when SwiftMath's resolver will find its fonts in this process.
    pub fn is_available() -> bool {
        *IS_AVAILABLE
    }

    /// The predicate itself, over an explicit list of `mathFonts.bundle`
    /// candidates: the font file, not only the directory over it, so an
    /// incompletely copied bundle reads as a miss.
    pub fn probe(roots: &[PathBuf]) -> bool {
        roots.iter().any(|root| root.join(PROBE_FILE).exists())
    }

    /// What the resolver would find for the same candidates: the first
    /// existing directory, and then the font in it.
    pub fn resolver_would_find(roots: &[PathBuf]) -> bool {
        roots
            .iter()
            .find(|root| root.is_dir())
            .is_some_and(|root: &PathBuf| Path::new(root).join(PROBE_FILE).exists())
    }
}
