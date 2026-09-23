//! Port of `Theme/ThemeValidation.swift`.
//!
//! `ThemeColor::resolved()` deliberately falls back to `labelColor` so a typo
//! in a hand-edited theme degrades instead of crashing mid-edit. That is the
//! right runtime behaviour and the wrong *validation* behaviour; these helpers
//! make the failure visible.

use objc2::Message;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor, NSColorSpace,
};

use super::style_sheet::ColorResolver;
use crate::render_contracts::{
    Theme, ThemeAppearance, ThemeColor, color_from_hex_string, system_color_named,
};
use crate::swift_compat::{self, pow, smax, smin};

#[derive(Debug, Clone, PartialEq)]
pub struct ThemeContrastFailure {
    pub path: String,
    pub ratio: f64,
    pub minimum: f64,
}

impl ThemeColor {
    /// `None` when `raw` is neither a parseable hex literal nor a known
    /// `system:` colour name.
    pub fn validated(&self) -> Option<Retained<NSColor>> {
        if swift_compat::has_ascii_prefix(&self.raw, "system:") {
            return system_color_named(&self.raw["system:".len()..]);
        }
        color_from_hex_string(&self.raw)
    }

    pub fn is_valid(&self) -> bool {
        self.validated().is_some()
    }
}

impl Theme {
    /// Every `ThemeColor` in the theme with the path it was found at, in
    /// declaration (`Mirror`) order: palette, then code.
    pub fn all_colors(&self) -> Vec<(String, ThemeColor)> {
        let mut out: Vec<(String, ThemeColor)> = Vec::new();
        for (label, color) in self.palette.colors() {
            out.push((format!("palette.{label}"), color.clone()));
        }
        for (label, color) in self.code.colors() {
            out.push((format!("code.{label}"), color.clone()));
        }
        out
    }

    /// Paths of every colour that would silently fall back at runtime.
    pub fn invalid_color_paths(&self) -> Vec<String> {
        self.all_colors()
            .into_iter()
            .filter(|(_, color)| !color.is_valid())
            .map(|(path, _)| path)
            .collect()
    }

    /// Essential text roles checked against the surfaces the renderer draws
    /// them on.
    pub fn semantic_contrast_failures(
        &self,
        appearance: Option<&NSAppearance>,
    ) -> Vec<ThemeContrastFailure> {
        let owned;
        let appearance = match appearance {
            Some(appearance) => appearance,
            None => {
                // SAFETY: AppKit exports the appearance names as immutable globals.
                let name = unsafe {
                    if self.appearance == ThemeAppearance::Dark {
                        NSAppearanceNameDarkAqua
                    } else {
                        NSAppearanceNameAqua
                    }
                };
                let Some(named) = NSAppearance::appearanceNamed(name) else {
                    return Vec::new();
                };
                owned = named;
                &owned
            }
        };
        let resolver = ColorResolver { appearance };
        let palette = &self.palette;
        let code = &self.code;
        let mut roles: Vec<(&str, &ThemeColor, &ThemeColor, f64)> = vec![
            ("palette.text", &palette.text, &palette.background, 4.5),
            (
                "palette.textSecondary",
                &palette.text_secondary,
                &palette.background,
                4.5,
            ),
            (
                "palette.heading",
                &palette.heading,
                &palette.background,
                4.5,
            ),
            ("palette.link", &palette.link, &palette.background, 3.0),
            (
                "palette.textOnCode",
                &palette.text,
                &palette.code_background,
                4.5,
            ),
            (
                "palette.textOnInlineCode",
                &palette.text,
                &palette.inline_code_background,
                4.5,
            ),
            (
                "palette.textOnSelection",
                &palette.text,
                &palette.selection,
                4.0,
            ),
            // Comments and markers are intentionally quiet chrome, but they
            // still need a measurable floor against their actual surfaces.
            (
                "code.commentOnCode",
                &code.comment,
                &palette.code_background,
                2.0,
            ),
            (
                "palette.markerOnBackground",
                &palette.marker,
                &palette.background,
                1.5,
            ),
            (
                "palette.calloutNoteOnBackground",
                &palette.callout_note,
                &palette.background,
                2.0,
            ),
            (
                "palette.calloutWarningOnBackground",
                &palette.callout_warning,
                &palette.background,
                2.0,
            ),
            (
                "palette.calloutSuccessOnBackground",
                &palette.callout_success,
                &palette.background,
                2.0,
            ),
            (
                "palette.calloutDangerOnBackground",
                &palette.callout_danger,
                &palette.background,
                2.0,
            ),
        ];
        if let Some(important) = &palette.callout_important {
            roles.push((
                "palette.calloutImportantOnBackground",
                important,
                &palette.background,
                2.0,
            ));
        }
        roles
            .into_iter()
            .filter_map(|(path, foreground, background, minimum)| {
                if !(foreground.is_valid() && background.is_valid()) {
                    return None;
                }
                let ratio = theme_contrast::ratio(
                    &resolver.resolve(foreground),
                    &resolver.resolve(background),
                );
                #[allow(clippy::neg_cmp_op_on_partial_ord)] // `guard ratio < minimum else`
                if !(ratio < minimum) {
                    return None;
                }
                Some(ThemeContrastFailure {
                    path: path.to_owned(),
                    ratio,
                    minimum,
                })
            })
            .collect()
    }
}

/// `private enum ThemeContrast`.
pub mod theme_contrast {
    use super::*;

    pub fn ratio(foreground: &NSColor, background: &NSColor) -> f64 {
        let foreground = relative_luminance(foreground);
        let background = relative_luminance(background);
        let lighter = smax(foreground, background);
        let darker = smin(foreground, background);
        (lighter + 0.05) / (darker + 0.05)
    }

    fn relative_luminance(color: &NSColor) -> f64 {
        let color = color
            .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
            .unwrap_or_else(|| color.retain());
        let linear = |component: f64| -> f64 {
            if component <= 0.04045 {
                component / 12.92
            } else {
                pow((component + 0.055) / 1.055, 2.4)
            }
        };
        0.2126 * linear(color.redComponent())
            + 0.7152 * linear(color.greenComponent())
            + 0.0722 * linear(color.blueComponent())
    }
}
