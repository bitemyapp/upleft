//! Port of `Theme/PreviewAppearance.swift`: appearance and theme names shared
//! by the host app and its Quick Look extension, through the global
//! preferences domain (`kCFPreferencesAnyApplication`).

use objc2::rc::Retained;
use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua};
use objc2_core_foundation::{
    CFPreferencesCopyValue, CFPreferencesSetValue, CFPreferencesSynchronize, CFRetained, CFString,
    kCFPreferencesAnyApplication, kCFPreferencesAnyHost, kCFPreferencesCurrentUser,
};
use objc2_foundation::NSArray;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreviewAppearance {
    System,
    Light,
    Dark,
}

impl PreviewAppearance {
    pub const ALL_CASES: [PreviewAppearance; 3] = [
        PreviewAppearance::System,
        PreviewAppearance::Light,
        PreviewAppearance::Dark,
    ];

    pub const fn raw_value(self) -> &'static str {
        match self {
            PreviewAppearance::System => "system",
            PreviewAppearance::Light => "light",
            PreviewAppearance::Dark => "dark",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<Self> {
        PreviewAppearance::ALL_CASES
            .into_iter()
            .find(|appearance| appearance.raw_value() == raw)
    }

    pub const fn title(self) -> &'static str {
        match self {
            PreviewAppearance::System => "System",
            PreviewAppearance::Light => "Light",
            PreviewAppearance::Dark => "Dark",
        }
    }

    pub fn ns_appearance(self) -> Option<Retained<NSAppearance>> {
        // SAFETY: AppKit exports the appearance names as immutable globals.
        match self {
            PreviewAppearance::System => None,
            PreviewAppearance::Light => {
                NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua })
            }
            PreviewAppearance::Dark => {
                NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua })
            }
        }
    }
}

/// Deliberately tiny bridge between the host app and Quick Look. Missing or
/// malformed values mean System.
pub struct PreviewAppearanceStore;

impl PreviewAppearanceStore {
    pub const APPEARANCE_KEY: &str = "com.ezzy.downright.quickLook.appearance";
    pub const LIGHT_THEME_KEY: &str = "com.ezzy.downright.quickLook.lightTheme";
    pub const DARK_THEME_KEY: &str = "com.ezzy.downright.quickLook.darkTheme";

    fn domain() -> (&'static CFString, &'static CFString, &'static CFString) {
        // SAFETY: CoreFoundation exports these as immutable globals.
        unsafe {
            (
                kCFPreferencesAnyApplication,
                kCFPreferencesCurrentUser,
                kCFPreferencesAnyHost,
            )
        }
    }

    fn value(key: &str) -> Option<String> {
        let (application, user, host) = PreviewAppearanceStore::domain();
        let value = CFPreferencesCopyValue(&CFString::from_str(key), application, user, host)?;
        // `as? String`: only a string value counts.
        let string: CFRetained<CFString> = value.downcast::<CFString>().ok()?;
        Some(string.to_string())
    }

    pub fn appearance() -> PreviewAppearance {
        PreviewAppearanceStore::value(PreviewAppearanceStore::APPEARANCE_KEY)
            .and_then(|raw| PreviewAppearance::from_raw_value(&raw))
            .unwrap_or(PreviewAppearance::System)
    }

    pub fn theme_name(appearance: &NSAppearance) -> Option<String> {
        // SAFETY: AppKit exports the appearance names as immutable globals.
        let (aqua, dark) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
        let names = NSArray::from_slice(&[aqua, dark]);
        let is_dark = appearance
            .bestMatchFromAppearancesWithNames(&names)
            .is_some_and(|best| &*best == dark);
        PreviewAppearanceStore::value(if is_dark {
            PreviewAppearanceStore::DARK_THEME_KEY
        } else {
            PreviewAppearanceStore::LIGHT_THEME_KEY
        })
    }

    pub fn write(appearance: PreviewAppearance, light_theme_name: &str, dark_theme_name: &str) {
        let (application, user, host) = PreviewAppearanceStore::domain();
        let values = [
            (
                PreviewAppearanceStore::APPEARANCE_KEY,
                appearance.raw_value(),
            ),
            (PreviewAppearanceStore::LIGHT_THEME_KEY, light_theme_name),
            (PreviewAppearanceStore::DARK_THEME_KEY, dark_theme_name),
        ];
        for (key, value) in values {
            let value = CFString::from_str(value);
            // SAFETY: a CFString is a valid property-list value.
            unsafe {
                CFPreferencesSetValue(
                    &CFString::from_str(key),
                    Some(&value),
                    application,
                    user,
                    host,
                )
            };
        }
        CFPreferencesSynchronize(application, user, host);
    }
}
