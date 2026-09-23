//! Port of `App/ThemedWindowAppearance.swift`: the `NSWindow` extension
//! `applyThemeAppearance(for:)`, as an extension trait on `NSWindow` (every
//! window subclass reaches it through `Deref`).

use objc2_app_kit::{NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSWindow};
use upleft_render::render_contracts::{Theme, ThemeAppearance};

use crate::support::preferences::Preferences;

/// `extension NSWindow { func applyThemeAppearance(for:) }`.
pub trait ThemedWindowAppearance {
    /// Keeps AppKit's system-drawn chrome in step with the theme a window
    /// paints itself with.
    ///
    /// A window that fills itself with `sheet.background` but never declares
    /// an appearance inherits macOS's. Choosing Paper Light under a dark
    /// system therefore draws dark-mode push buttons and text fields onto
    /// cream, and a button title styled for dark chrome disappears against
    /// it. Any window that paints a theme colour owes AppKit the matching
    /// appearance.
    ///
    /// Following macOS is the exception, for the reason document windows
    /// already encode: the theme pair tracks the system there, so pinning
    /// would sever the native appearance chain even when the theme changes
    /// correctly.
    fn apply_theme_appearance(&self, theme: &Theme);
}

impl ThemedWindowAppearance for NSWindow {
    fn apply_theme_appearance(&self, theme: &Theme) {
        let wanted = if Preferences::shared().values().follows_system_appearance {
            None
        } else {
            match theme.appearance {
                // SAFETY: AppKit's appearance-name constants.
                ThemeAppearance::Light => NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }),
                ThemeAppearance::Dark => NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }),
                ThemeAppearance::Auto => None,
            }
        };
        // Callers apply this from `viewDidChangeEffectiveAppearance`, which an
        // assignment here would re-enter. Settling for the value already in
        // place ends that on the first pass.
        let wanted_name = wanted.as_ref().map(|appearance| appearance.name().to_string());
        let current_name = self.appearance().map(|appearance| appearance.name().to_string());
        if wanted_name == current_name {
            return;
        }
        self.setAppearance(wanted.as_deref());
    }
}
