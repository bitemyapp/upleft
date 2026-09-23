//! Port of `View/StyleSheetDefaults.swift`: the style a view is born with,
//! the density gutter's chrome constants, and `NSColor.panelAlpha`.

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSAnimationContext, NSAppearance, NSApplication, NSColor, NSFont, NSFontWeightSemibold};

use crate::swift_compat::smin;
use crate::theme::style_sheet::StyleSheet;
use crate::theme::theme_store::ThemeStore;

unsafe extern "C" {
    /// AppKit's `NSApp` global: nil until `NSApplication.shared` is first
    /// created. Swift's `NSApp?` reads it without creating the application.
    #[link_name = "NSApp"]
    static NS_APP: *mut NSApplication;
}

impl StyleSheet {
    /// `StyleSheet.current`: the current theme, against the application's
    /// effective appearance or else the current drawing appearance.
    pub fn current(mtm: MainThreadMarker) -> StyleSheet {
        let _ = mtm;
        // SAFETY: read on the main thread, where AppKit writes it.
        let app = unsafe { NS_APP.as_ref() };
        let appearance: Retained<NSAppearance> = match app {
            Some(app) => app.effectiveAppearance(),
            None => NSAppearance::currentDrawingAppearance(),
        };
        StyleSheet::new(ThemeStore::shared().current(), &appearance, None)
    }
}

/// Small chrome constants the density gutter needs.
pub struct GutterChrome;

impl GutterChrome {
    pub fn title_font() -> Retained<NSFont> {
        // SAFETY: AppKit exports the weight as an immutable global.
        NSFont::systemFontOfSize_weight(14.0, unsafe { NSFontWeightSemibold })
    }

    pub fn body_font() -> Retained<NSFont> {
        NSFont::systemFontOfSize(11.0)
    }

    /// Full respect for Reduce Motion (§11.4): `Motion.run` with its default
    /// curve.
    pub fn animate(
        reduce_motion: bool,
        duration: f64,
        body: impl Fn(&NSAnimationContext) + 'static,
        completion: Option<Box<dyn Fn() + 'static>>,
    ) {
        crate::motion::run(reduce_motion, duration, crate::motion::Curve::Decelerate, body, completion);
    }
}

/// `NSColor.panelAlpha(_:increaseContrast:)`: alpha that respects Increase
/// Contrast (§11.4).
pub trait PanelAlpha {
    fn panel_alpha(&self, alpha: f64, increase_contrast: bool) -> Retained<NSColor>;
}

impl PanelAlpha for NSColor {
    fn panel_alpha(&self, alpha: f64, increase_contrast: bool) -> Retained<NSColor> {
        self.colorWithAlphaComponent(if increase_contrast {
            smin(1.0, alpha * 1.8)
        } else {
            alpha
        })
    }
}
