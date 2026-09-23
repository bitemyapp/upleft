//! `MathResourceBundle.swift` (Downright's patch to SwiftMath): where
//! `mathFonts.bundle` lives.
//!
//! SwiftMath resolves the fonts through `Bundle`; Upleft resolves a
//! directory. The candidates, in order:
//!
//! 1. a directory set with [`set_math_fonts_directory`] (an app bundle sets
//!    its `Contents/Resources/mathFonts.bundle` at launch),
//! 2. `$UPLEFT_MATH_FONTS`,
//! 3. `mathFonts.bundle` in the resources of the `.app` the executable is in
//!    (`../Resources` from `Contents/MacOS`), and beside the executable,
//! 4. the submodule copy, `vendor/downright/Vendor/SwiftMath/Sources/SwiftMath/mathFonts.bundle`,
//!    at the path this crate was built from (development and tests).
//!
//! As in the Swift resolver, a candidate is accepted only when the directory
//! is really there; which fonts it holds is `MathFontBundle`'s question.

use std::path::{Path, PathBuf};
use std::sync::{LazyLock, RwLock};

static OVERRIDE: RwLock<Option<PathBuf>> = RwLock::new(None);

/// The directory this crate's source tree ships the fonts in.
pub fn development_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/downright/Vendor/SwiftMath/Sources/SwiftMath/mathFonts.bundle")
}

/// Points the resolver at `directory` (a `mathFonts.bundle`). Must be called
/// before the first font is loaded to have an effect on it.
pub fn set_math_fonts_directory(directory: impl Into<PathBuf>) {
    *OVERRIDE.write().unwrap() = Some(directory.into());
}

/// The candidate directories, in the order they are tried.
pub fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(directory) = OVERRIDE.read().unwrap().clone() {
        out.push(directory);
    }
    if let Some(directory) = std::env::var_os("UPLEFT_MATH_FONTS") {
        out.push(PathBuf::from(directory));
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        out.push(directory.join("../Resources/mathFonts.bundle"));
        out.push(directory.join("mathFonts.bundle"));
    }
    out.push(development_directory());
    out
}

fn resolve() -> Option<PathBuf> {
    candidates()
        .into_iter()
        .find(|candidate| candidate.is_dir())
}

static RESOLVED: LazyLock<Option<PathBuf>> = LazyLock::new(resolve);

/// `MathResourceBundle.resources.url(forResource: "mathFonts", withExtension: "bundle")`,
/// resolved once, on first use (unless an override is set, which always wins).
pub fn math_fonts_directory() -> Option<PathBuf> {
    if let Some(directory) = OVERRIDE.read().unwrap().clone() {
        return Some(directory);
    }
    RESOLVED.clone()
}

/// `Bundle(url: mathFonts.bundle).path(forResource: name, ofType: ext)`.
pub fn resource_path(name: &str, extension: &str) -> Option<PathBuf> {
    let path = math_fonts_directory()?.join(format!("{name}.{extension}"));
    path.exists().then_some(path)
}
