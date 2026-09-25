//! Port of `Theme/StyleSheet.swift`: resolves a theme plus the current system
//! appearance into ready-to-use `NSColor`s and `NSFont`s. This is what the
//! decoration engine consumes.
//!
//! Everything is resolved once, in `new`, and stored. System colours have to
//! be snapshotted *against a specific appearance* to be meaningful off the
//! drawing path, and the decorator asks for the same dozen values thousands of
//! times per document. Assigning to `theme` or `revision` after construction
//! recomputes nothing; build a new `StyleSheet` instead.
//!
//! AppKit types (`NSFont`, `NSColor`, `NSAppearance`) are not `Send` in
//! objc2, so neither is a `StyleSheet`: build and use it on the thread that
//! decorates, as Downright does.

use std::cell::RefCell;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::Message;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSAppearance, NSColor, NSColorSpace, NSFont, NSFontDescriptor, NSFontDescriptorSymbolicTraits,
    NSFontDescriptorSystemDesignSerif, NSFontFamilyAttribute, NSFontFeatureSelectorIdentifierKey,
    NSFontFeatureSettingsAttribute, NSFontFeatureTypeIdentifierKey, NSFontTraitsAttribute,
    NSFontWeight, NSFontWeightBold, NSFontWeightMedium, NSFontWeightRegular, NSFontWeightSemibold,
    NSFontWeightTrait, NSWorkspace,
};
use objc2_core_foundation::CGSize;
use objc2_core_text::{
    CTFont, CTFontOrientation, kCommonLigaturesOffSelector, kContextualAlternatesOffSelector,
    kContextualAlternatesType, kLigaturesType, kRareLigaturesOffSelector,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

use super::theme_store::ThemeStore;
use crate::core_types::{CalloutKind, ChangeKind};
use crate::render_contracts::{BodyPreset, CodeTheme, Theme, ThemeColor, TypographyConfig};
use crate::swift_compat::{pow, smax, smin};
use crate::syntax::syntax_contracts::SyntaxToken;
use crate::view::style_sheet_defaults::PanelAlpha;

#[derive(Debug, Clone)]
pub struct StyleSheet {
    pub theme: Theme,
    pub revision: i64,
    /// The concrete appearance used to snapshot every dynamic colour below.
    pub appearance: Retained<NSAppearance>,

    // Fonts (resolved once)
    body: Retained<NSFont>,
    headings: [Retained<NSFont>; 6],
    mono: Retained<NSFont>,
    /// Indexed by `(bold ? 1 : 0) + (italic ? 2 : 0)`.
    emphasis: [Retained<NSFont>; 4],

    // Metrics
    /// The grid unit every vertical measure is a whole multiple of (§11.1).
    pub baseline_grid: f64,
    pub line_height: f64,
    /// Mean advance of the body font over a prose sample.
    pub average_character_width: f64,
    /// Measure cap in points (§11.1), clamped to 68–72 characters.
    pub measure_width: f64,
    /// Point size for math so it sits optically against body text (§11.3).
    pub math_point_size: f64,

    // Colours
    pub background: Retained<NSColor>,
    pub surface: Retained<NSColor>,
    pub text: Retained<NSColor>,
    pub text_secondary: Retained<NSColor>,
    pub text_faint: Retained<NSColor>,
    pub marker: Retained<NSColor>,
    pub accent: Retained<NSColor>,
    pub link: Retained<NSColor>,
    pub rule: Retained<NSColor>,
    pub code_background: Retained<NSColor>,
    pub inline_code_background: Retained<NSColor>,
    pub code_rule: Retained<NSColor>,
    pub rail_tick: Retained<NSColor>,
    pub rail_tick_current: Retained<NSColor>,
    pub quote_rule: Retained<NSColor>,
    pub path_missing: Retained<NSColor>,
    pub search_hit: Retained<NSColor>,
    pub search_hit_current: Retained<NSColor>,
    pub selection: Retained<NSColor>,

    /// Apple's blue control colour for the start action, not the theme accent.
    start_action_accent: Retained<NSColor>,

    heading_colors: [Retained<NSColor>; 6],
    /// note, warning, success, danger, important
    callout_colors: [Retained<NSColor>; 5],
    /// added, removed, modified
    change_colors: [Retained<NSColor>; 3],
    /// Indexed by `SyntaxToken as usize`.
    code_colors: [Retained<NSColor>; 15],

    // Accessibility (§11.4)
    pub reduce_motion: bool,
    pub increase_contrast: bool,
    pub reduce_transparency: bool,

    /// Host-only typography (`for_host`); the default is Downright's.
    pub host: HostTypography,
}

/// Typography a host sets on top of a theme (an Upleft extension; see
/// docs/EMBEDDING.md). Every field is optional and `None` keeps Downright's
/// behaviour, so theme JSON and every Downright style sheet are unchanged.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HostTypography {
    /// The face of body text and headings. `None` follows the theme's preset
    /// (Working is the system font, Reading is New York).
    pub body_family: Option<BodyFamily>,
    /// Body size in points, in place of the theme's `bodySize`. Everything
    /// derived from the body size (heading scale, code size, math size,
    /// indents) follows it.
    pub body_size: Option<f64>,
    /// Heading sizes for H1–H6 in points, in place of the modular scale.
    pub heading_sizes: [Option<f64>; 6],
    /// Code size in points, in place of `bodySize × monoSizeAdjust`
    /// normalised to SF Mono's x-height.
    pub code_size: Option<f64>,
    /// Line height as a multiple of the body size, exact rather than rounded
    /// to Downright's baseline grid (to the half point). The grid becomes a
    /// quarter of it, so vertical rhythm keeps its proportions.
    pub line_height_multiple: Option<f64>,
    /// Space after a top-level paragraph, in points, in place of
    /// 0.45 × line height on the grid.
    pub paragraph_spacing: Option<f64>,
    /// `NSParagraphStyle.hyphenationFactor` for prose (0–1).
    pub hyphenation_factor: Option<f32>,
    /// The lane code blocks, tables, math and diagrams may use past the
    /// prose's trailing edge (`RenderMetrics.codeBleed`, 88 pt). A chat
    /// column usually wants 0: prose as wide as code.
    pub code_bleed: Option<f64>,
    /// A code block's first row drawn as a header bar (a chat host's
    /// chrome): a tinted strip over a hairline, the language as a pill at
    /// its leading edge, and the copy control always shown at its trailing
    /// edge, with no left rule. `None` or `false` keeps Downright's chip,
    /// hover-only copy control and rule.
    pub code_header: Option<bool>,
}

/// A body face (`HostTypography::body_family`).
#[derive(Debug, Clone, PartialEq)]
pub enum BodyFamily {
    /// SF Pro, the system font.
    System,
    /// New York, the system serif.
    NewYork,
    /// SF Mono, the monospaced system font.
    Monospaced,
    /// A family by name (`"Charter"`), falling back to the system font when
    /// it is not installed.
    Named(String),
}

/// `NSFont.Weight` values, read from AppKit's own constants.
fn weight_regular() -> NSFontWeight {
    // SAFETY: AppKit exports these as immutable globals.
    unsafe { NSFontWeightRegular }
}

fn weight_bold() -> NSFontWeight {
    // SAFETY: as above.
    unsafe { NSFontWeightBold }
}

fn weight_medium() -> NSFontWeight {
    // SAFETY: as above.
    unsafe { NSFontWeightMedium }
}

fn weight_semibold() -> NSFontWeight {
    // SAFETY: as above.
    unsafe { NSFontWeightSemibold }
}

/// Whole steps of the modular scale for H1–H4, then compact steps below the
/// body size for H5–H6 (§11.1).
const HEADING_EXPONENTS: [f64; 6] = [3.0, 2.0, 1.25, 0.5, -0.5, -0.75];

/// Latin Modern Math has an x-height of 0.431 em (§11.3).
const MATH_FONT_X_HEIGHT_RATIO: f64 = 0.431;

/// The prose sample `averageCharacterWidth(of:)` measures.
const AVERAGE_WIDTH_SAMPLE: &str =
    "the quick brown fox jumps over the lazy dog, and then it did it again. ";

fn clamp_level(level: i64) -> i64 {
    6.min(1.max(level))
}

impl StyleSheet {
    /// `StyleSheet(theme:appearance:reduceMotionOverride:)`.
    ///
    /// A non-`None` override is used by deterministic previews and tests: it
    /// must be able to force either Reduce Motion branch.
    pub fn new(
        theme: Theme,
        appearance: &NSAppearance,
        reduce_motion_override: Option<bool>,
    ) -> StyleSheet {
        let revision = ThemeStore::shared().revision();
        StyleSheet::build(
            theme,
            appearance,
            reduce_motion_override,
            revision,
            HostTypography::default(),
        )
    }

    /// A style sheet for a host embedding the renderer (docs/EMBEDDING.md).
    /// Unlike `new`, it never touches `ThemeStore`: no preferences are read
    /// or written and no directory is created or watched. `revision` is 0.
    /// Increase Contrast and Reduce Transparency come from `NSWorkspace`, as
    /// in `new`. Build a new one, and hand it to `set_style_sheet`, when the
    /// appearance changes.
    pub fn for_host(
        theme: Theme,
        appearance: &NSAppearance,
        reduce_motion: bool,
        typography: HostTypography,
    ) -> StyleSheet {
        StyleSheet::build(theme, appearance, Some(reduce_motion), 0, typography)
    }

    fn build(
        mut theme: Theme,
        appearance: &NSAppearance,
        reduce_motion_override: Option<bool>,
        revision: i64,
        host: HostTypography,
    ) -> StyleSheet {
        if let Some(body_size) = host.body_size {
            theme.typography.body_size = body_size;
        }

        let workspace = NSWorkspace::sharedWorkspace();
        let reduce_motion = reduce_motion_override
            .unwrap_or_else(|| workspace.accessibilityDisplayShouldReduceMotion());
        let increase_contrast = workspace.accessibilityDisplayShouldIncreaseContrast();
        let reduce_transparency = workspace.accessibilityDisplayShouldReduceTransparency();

        let typography = &theme.typography;

        // Fonts, built through locals.
        let face = |size: f64, weight: NSFontWeight| match &host.body_family {
            None => StyleSheet::system_font(typography.preset, size, weight),
            Some(family) => StyleSheet::family_font(family, size, weight),
        };
        let body_font = face(typography.body_size, weight_regular());
        let headings: [Retained<NSFont>; 6] = std::array::from_fn(|index| {
            let level = index as i64 + 1;
            let weight = if level <= 3 {
                weight_bold()
            } else if level == 6 {
                weight_medium()
            } else {
                weight_semibold()
            };
            let size = host.heading_sizes[index]
                .unwrap_or_else(|| StyleSheet::heading_size(level, typography));
            let font = face(size, weight);
            if level == 6 {
                StyleSheet::applying(false, true, &font)
            } else {
                font
            }
        });
        let mono = StyleSheet::mono_font_named(
            &typography.mono_family,
            host.code_size
                .unwrap_or_else(|| StyleSheet::mono_point_size(typography)),
            typography.mono_ligatures,
        );
        let emphasis = [
            body_font.clone(),
            StyleSheet::applying(true, false, &body_font),
            StyleSheet::applying(false, true, &body_font),
            StyleSheet::applying(true, true, &body_font),
        ];

        // Metrics. The grid is quantised in half units so the line height can
        // land on an even point (26pt at a 16pt body).
        let (baseline_grid, line_height) = match host.line_height_multiple {
            // A host's multiple is kept to the half point, and the grid
            // divides it.
            Some(multiple) => {
                let line_height = smax(2.0, (typography.body_size * multiple * 2.0).round() / 2.0);
                (line_height / 4.0, line_height)
            }
            None => {
                let ideal_line_height = typography.body_size * typography.line_height_multiple;
                let grid = smax(2.0, (ideal_line_height / 2.0).round() / 2.0);
                (grid, grid * 4.0)
            }
        };
        let advance = StyleSheet::average_character_width_of(&body_font);
        let average_character_width = advance;
        let measure_width = advance * smin(72.0, smax(68.0, typography.measure_characters));
        let math_point_size = StyleSheet::math_point_size_for(&body_font, typography);

        // Colours
        let palette = &theme.palette;
        let resolver = ColorResolver { appearance };
        let resolved_text = resolver.resolve(&palette.text);
        let boost = increase_contrast;
        let start_action_accent = resolver.resolve(&ThemeColor::new("system:systemBlue"));

        let background = resolver.resolve(&palette.background);
        let surface = resolver.resolve(&palette.surface);
        let text = resolved_text.clone();
        let text_secondary =
            resolver.resolve_towards(&palette.text_secondary, &resolved_text, boost);
        let text_faint = resolver.resolve_towards(&palette.text_faint, &resolved_text, boost);
        let marker = resolver.resolve_towards(&palette.marker, &resolved_text, boost);
        let accent = resolver.resolve(&palette.accent);
        let link = resolver.resolve(&palette.link);
        let rule = resolver.resolve_towards(&palette.rule, &resolved_text, boost);
        let code_background = resolver.resolve(&palette.code_background);
        let inline_code_background = resolver.resolve(&palette.inline_code_background);
        let code_rule = resolver.resolve_towards(&palette.code_rule, &resolved_text, boost);
        let rail_tick = resolver.resolve_towards(&palette.rail_tick, &resolved_text, boost);
        let rail_tick_current =
            resolver.resolve_towards(&palette.rail_tick_current, &resolved_text, boost);
        let quote_rule = resolver.resolve_towards(&palette.quote_rule, &resolved_text, boost);
        let path_missing = resolver.resolve(&palette.path_missing);
        let search_hit = resolver.resolve(&palette.search_hit);
        let search_hit_current = resolver.resolve(&palette.search_hit_current);
        let selection = resolver.resolve(&palette.selection);

        // H1–H3 carry the heading colour; H4–H6 fade toward secondary text.
        let heading_base = resolver.resolve(&palette.heading);
        let heading_colors: [Retained<NSColor>; 6] = std::array::from_fn(|index| {
            let level = index as i64 + 1;
            if level <= 3 {
                return heading_base.clone();
            }
            ColorResolver::blend(
                &heading_base,
                &resolver.resolve(&palette.text_secondary),
                (level - 3) as f64 / 4.0,
            )
        });
        let callout_colors = [
            resolver.resolve(&palette.callout_note),
            resolver.resolve(&palette.callout_warning),
            resolver.resolve(&palette.callout_success),
            resolver.resolve(&palette.callout_danger),
            resolver.resolve(
                palette
                    .callout_important
                    .as_ref()
                    .unwrap_or(&palette.callout_danger),
            ),
        ];
        let change_colors = [
            resolver.resolve(&palette.change_added),
            resolver.resolve(&palette.change_removed),
            resolver.resolve(&palette.change_modified),
        ];
        let code_colors = StyleSheet::code_colors_for(&theme.code, &resolved_text, &resolver);

        StyleSheet {
            revision,
            appearance: appearance.retain(),
            body: body_font,
            headings,
            mono,
            emphasis,
            baseline_grid,
            line_height,
            average_character_width,
            measure_width,
            math_point_size,
            background,
            surface,
            text,
            text_secondary,
            text_faint,
            marker,
            accent,
            link,
            rule,
            code_background,
            inline_code_background,
            code_rule,
            rail_tick,
            rail_tick_current,
            quote_rule,
            path_missing,
            search_hit,
            search_hit_current,
            selection,
            start_action_accent,
            heading_colors,
            callout_colors,
            change_colors,
            code_colors,
            reduce_motion,
            increase_contrast,
            reduce_transparency,
            theme,
            host,
        }
    }

    /// `RenderMetrics.codeBleed`, unless a host set its own
    /// (`HostTypography::code_bleed`).
    pub fn code_bleed(&self) -> f64 {
        self.host
            .code_bleed
            .unwrap_or(crate::engine::render_metrics::CODE_BLEED)
    }

    /// A host's body face (`HostTypography::body_family`) at a size and
    /// weight.
    fn family_font(family: &BodyFamily, size: f64, weight: NSFontWeight) -> Retained<NSFont> {
        match family {
            BodyFamily::System => StyleSheet::system_font(BodyPreset::Working, size, weight),
            BodyFamily::NewYork => StyleSheet::system_font(BodyPreset::Reading, size, weight),
            BodyFamily::Monospaced => NSFont::monospacedSystemFontOfSize_weight(size, weight),
            BodyFamily::Named(name) => {
                // SAFETY: AppKit exports these keys as immutable globals.
                let (family_key, traits_key, weight_key) = unsafe {
                    (
                        NSFontFamilyAttribute,
                        NSFontTraitsAttribute,
                        NSFontWeightTrait,
                    )
                };
                let weight_value = NSNumber::new_f64(weight);
                let traits: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::from_slices(
                    &[weight_key],
                    &[weight_value.as_ref() as &AnyObject],
                );
                let family_name = NSString::from_str(name);
                let attributes: Retained<NSDictionary<NSString, AnyObject>> =
                    NSDictionary::from_slices(
                        &[family_key, traits_key],
                        &[family_name.as_ref() as &AnyObject, traits.as_ref()],
                    );
                // SAFETY: the attributes map descriptor keys to values of
                // the types AppKit documents for them.
                let descriptor = unsafe {
                    NSFontDescriptor::fontDescriptorWithFontAttributes(Some(&attributes))
                };
                NSFont::fontWithDescriptor_size(&descriptor, size)
                    .filter(|font| {
                        font.familyName()
                            .is_some_and(|resolved| resolved.to_string() == *name)
                    })
                    .unwrap_or_else(|| NSFont::systemFontOfSize_weight(size, weight))
            }
        }
    }

    /// The start action's fill: Apple's blue control colour, darkened only as
    /// far as needed to keep a white label at 5.2:1.
    pub fn start_window_primary_action(&self) -> Retained<NSColor> {
        let black = NSColor::blackColor();
        let white = NSColor::whiteColor();
        for step in 4..=14 {
            let amount = step as f64 * 0.05;
            let candidate = self
                .start_action_accent
                .blendedColorWithFraction_ofColor(amount, &black)
                .unwrap_or_else(|| self.start_action_accent.clone());
            if StyleSheet::contrast_ratio(&white, &candidate) >= 5.2 {
                return candidate;
            }
        }
        NSColor::colorWithCalibratedRed_green_blue_alpha(0.08, 0.28, 0.58, 1.0)
    }

    // MARK: - Fonts

    pub fn body_font(&self) -> Retained<NSFont> {
        self.body.clone()
    }

    pub fn heading_font(&self, level: i64) -> Retained<NSFont> {
        self.headings[(clamp_level(level) - 1) as usize].clone()
    }

    /// `monoFont(size:)`: the resolved mono face, re-sized when asked.
    pub fn mono_font(&self, size: Option<f64>) -> Retained<NSFont> {
        let Some(size) = size else {
            return self.mono.clone();
        };
        if size == self.mono.pointSize() {
            return self.mono.clone();
        }
        NSFont::fontWithDescriptor_size(&self.mono.fontDescriptor(), size)
            .unwrap_or_else(|| self.mono.clone())
    }

    /// `monoFontAttributes(size:)` as its two values: the font, and the
    /// `.ligature` attribute (1 when the theme wants ligatures, else 0).
    pub fn mono_font_attributes(&self, size: Option<f64>) -> (Retained<NSFont>, isize) {
        (
            self.mono_font(size),
            if self.theme.typography.mono_ligatures {
                1
            } else {
                0
            },
        )
    }

    /// `monoFontAttributes(size:)` as the attribute dictionary Swift returns.
    pub fn mono_font_attributes_dictionary(
        &self,
        size: Option<f64>,
    ) -> Retained<NSDictionary<NSString, AnyObject>> {
        let (font, ligature) = self.mono_font_attributes(size);
        let number = NSNumber::new_isize(ligature);
        // SAFETY: AppKit exports these keys as immutable globals.
        let (font_key, ligature_key) = unsafe {
            (
                objc2_app_kit::NSFontAttributeName,
                objc2_app_kit::NSLigatureAttributeName,
            )
        };
        let values: [&AnyObject; 2] = [font.as_ref(), number.as_ref()];
        NSDictionary::from_slices(&[font_key, ligature_key], &values)
    }

    pub fn emphasis_font(&self, bold: bool, italic: bool) -> Retained<NSFont> {
        self.emphasis[(if bold { 1 } else { 0 }) + (if italic { 2 } else { 0 })].clone()
    }

    /// `headingSize(level:typography:)`.
    pub fn heading_size(level: i64, typography: &TypographyConfig) -> f64 {
        let exponent = HEADING_EXPONENTS[(clamp_level(level) - 1) as usize];
        typography.body_size * pow(typography.scale_ratio, exponent)
    }

    /// Vertical rhythm in whole grid units (§11.1): `(before, after)`.
    pub fn heading_spacing(&self, level: i64) -> (f64, f64) {
        const BEFORE: [f64; 6] = [6.0, 6.0, 6.0, 3.0, 3.0, 3.0];
        const AFTER: [f64; 6] = [2.0, 2.0, 2.0, 1.0, 1.0, 1.0];
        let index = (clamp_level(level) - 1) as usize;
        (
            BEFORE[index] * self.baseline_grid,
            AFTER[index] * self.baseline_grid,
        )
    }

    fn system_font(preset: BodyPreset, size: f64, weight: NSFontWeight) -> Retained<NSFont> {
        let base = NSFont::systemFontOfSize_weight(size, weight);
        if preset != BodyPreset::Reading {
            return base; // Working = SF Pro Text
        }
        // Reading = New York, reached through the descriptor's serif design.
        // SAFETY: AppKit exports the design name as an immutable global.
        let design = unsafe { NSFontDescriptorSystemDesignSerif };
        let Some(descriptor) = base.fontDescriptor().fontDescriptorWithDesign(design) else {
            return base;
        };
        let Some(serif) = NSFont::fontWithDescriptor_size(&descriptor, size) else {
            return base;
        };
        serif
    }

    /// `monoFont(family:size:ligatures:)`: configured face, SF Mono under both
    /// of its names, Menlo, then the system monospace face.
    fn mono_font_named(family: &str, size: f64, ligatures: bool) -> Retained<NSFont> {
        let candidates = [family, "SF Mono", "SFMono-Regular", "Menlo"];
        let mut resolved: Option<Retained<NSFont>> = None;
        for name in candidates {
            if name.is_empty() {
                continue;
            }
            if let Some(font) = NSFont::fontWithName_size(&NSString::from_str(name), size) {
                resolved = Some(font);
                break;
            }
        }
        let font = resolved
            .unwrap_or_else(|| NSFont::monospacedSystemFontOfSize_weight(size, weight_regular()));
        StyleSheet::applying_ligatures(ligatures, &font)
    }

    fn applying_ligatures(enabled: bool, font: &NSFont) -> Retained<NSFont> {
        if enabled {
            return font.retain();
        }
        // SAFETY: AppKit exports these keys as immutable globals.
        let (type_key, selector_key, settings_key) = unsafe {
            (
                NSFontFeatureTypeIdentifierKey,
                NSFontFeatureSelectorIdentifierKey,
                NSFontFeatureSettingsAttribute,
            )
        };
        let setting = |kind: i32, selector: u32| -> Retained<NSDictionary<NSString, NSNumber>> {
            let values = [
                NSNumber::new_isize(kind as isize),
                NSNumber::new_isize(selector as isize),
            ];
            NSDictionary::from_retained_objects(&[type_key, selector_key], &values)
        };
        let settings = NSArray::from_retained_slice(&[
            setting(kLigaturesType, kCommonLigaturesOffSelector),
            setting(kLigaturesType, kRareLigaturesOffSelector),
            setting(kContextualAlternatesType, kContextualAlternatesOffSelector),
        ]);
        let attributes: Retained<NSDictionary<NSString, AnyObject>> =
            NSDictionary::from_slices(&[settings_key], &[settings.as_ref() as &AnyObject]);
        // SAFETY: the attributes dictionary holds a valid feature-settings array.
        let descriptor = unsafe {
            font.fontDescriptor()
                .fontDescriptorByAddingAttributes(&attributes)
        };
        NSFont::fontWithDescriptor_size(&descriptor, font.pointSize())
            .unwrap_or_else(|| font.retain())
    }

    fn applying(bold: bool, italic: bool, font: &NSFont) -> Retained<NSFont> {
        if !(bold || italic) {
            return font.retain();
        }
        let mut traits = font.fontDescriptor().symbolicTraits();
        if bold {
            traits |= NSFontDescriptorSymbolicTraits::TraitBold;
        }
        if italic {
            traits |= NSFontDescriptorSymbolicTraits::TraitItalic;
        }
        let descriptor: Retained<NSFontDescriptor> = font
            .fontDescriptor()
            .fontDescriptorWithSymbolicTraits(traits);
        NSFont::fontWithDescriptor_size(&descriptor, font.pointSize())
            .unwrap_or_else(|| font.retain())
    }

    /// The mean advance over a prose sample (§11.1).
    fn average_character_width_of(font: &NSFont) -> f64 {
        let sample: Vec<u16> = AVERAGE_WIDTH_SAMPLE.encode_utf16().collect();
        let mut glyphs = vec![0u16; sample.len()];
        // SAFETY: NSFont is toll-free bridged to CTFont.
        let ct_font: &CTFont = unsafe { &*(font as *const NSFont as *const CTFont) };
        // SAFETY: both buffers hold `sample.len()` elements.
        let mapped = unsafe {
            ct_font.glyphs_for_characters(
                NonNull::new(sample.as_ptr() as *mut u16).unwrap(),
                NonNull::new(glyphs.as_mut_ptr()).unwrap(),
                sample.len() as isize,
            )
        };
        if !mapped {
            return font.pointSize() * 0.5;
        }
        let mut advances = vec![CGSize::new(0.0, 0.0); sample.len()];
        // SAFETY: both buffers hold `sample.len()` elements.
        unsafe {
            ct_font.advances_for_glyphs(
                CTFontOrientation::Horizontal,
                NonNull::new(glyphs.as_ptr() as *mut u16).unwrap(),
                advances.as_mut_ptr(),
                sample.len() as isize,
            );
        }
        let total = advances
            .iter()
            .fold(0.0f64, |sum, advance| sum + advance.width);
        if total > 0.0 {
            total / sample.len() as f64
        } else {
            font.pointSize() * 0.5
        }
    }

    /// Math matched on x-height rather than point size, clamped to 0.90–1.10×
    /// the body size (§11.3).
    fn math_point_size_for(body: &NSFont, typography: &TypographyConfig) -> f64 {
        let optical = body.xHeight() / MATH_FONT_X_HEIGHT_RATIO;
        let clamped = smin(
            smax(optical, typography.body_size * 0.90),
            typography.body_size * 1.10,
        );
        clamped * typography.math_scale
    }

    /// Code sized to sit level with the prose beside it whatever face
    /// resolves: the requested size normalised onto SF Mono's x-height.
    fn mono_point_size(typography: &TypographyConfig) -> f64 {
        let requested = typography.body_size * typography.mono_size_adjust;
        let resolved = StyleSheet::mono_font_named(&typography.mono_family, requested, true);
        let reference = StyleSheet::mono_font_named("SF Mono", requested, true);
        if !(resolved.xHeight() > 0.0 && reference.xHeight() > 0.0) {
            return requested;
        }
        let corrected = requested * (reference.xHeight() / resolved.xHeight());
        smin(smax(corrected, requested * 0.92), requested * 1.08)
    }

    // MARK: - Colours

    pub fn heading_color(&self, level: i64) -> Retained<NSColor> {
        self.heading_colors[(clamp_level(level) - 1) as usize].clone()
    }

    /// Fourteen callout kinds share five palette slots, grouped by what the
    /// reader is meant to *do*.
    pub fn callout_color(&self, kind: CalloutKind) -> Retained<NSColor> {
        let index = match kind {
            CalloutKind::Note
            | CalloutKind::Info
            | CalloutKind::Abstract
            | CalloutKind::Quote
            | CalloutKind::Example => 0,
            CalloutKind::Warning | CalloutKind::Question | CalloutKind::Todo => 1,
            CalloutKind::Tip | CalloutKind::Success => 2,
            CalloutKind::Caution | CalloutKind::Danger | CalloutKind::Bug => 3,
            CalloutKind::Important => 4,
        };
        self.callout_colors[index].clone()
    }

    pub fn callout_symbol(&self, kind: CalloutKind) -> &'static str {
        match kind {
            CalloutKind::Note => "text.alignleft",
            CalloutKind::Tip => "lightbulb",
            CalloutKind::Important => "exclamationmark.circle",
            CalloutKind::Warning => "exclamationmark.triangle",
            CalloutKind::Caution => "hand.raised",
            CalloutKind::Info => "info.circle",
            CalloutKind::Success => "checkmark.circle",
            CalloutKind::Question => "questionmark.circle",
            CalloutKind::Danger => "exclamationmark.octagon",
            CalloutKind::Example => "list.bullet.rectangle",
            CalloutKind::Quote => "quote.opening",
            CalloutKind::Abstract => "doc.text",
            CalloutKind::Bug => "ladybug",
            CalloutKind::Todo => "checklist",
        }
    }

    pub fn change_color(&self, kind: ChangeKind) -> Retained<NSColor> {
        match kind {
            ChangeKind::Inserted => self.change_colors[0].clone(),
            ChangeKind::Deleted => self.change_colors[1].clone(),
            ChangeKind::Modified => self.change_colors[2].clone(),
        }
    }

    pub fn code_color(&self, token: SyntaxToken) -> Retained<NSColor> {
        self.code_colors[token as usize].clone()
    }

    /// Borrowing form of `code_color`, for hot loops.
    pub fn code_color_ref(&self, token: SyntaxToken) -> &NSColor {
        &self.code_colors[token as usize]
    }

    /// What a mark drawn *on* the accent is painted in: whichever of the page
    /// and its text stands further from the accent.
    pub fn on_accent(&self) -> Retained<NSColor> {
        let accent = StyleSheet::relative_luminance(&self.accent);
        let distance = |color: &NSColor| (StyleSheet::relative_luminance(color) - accent).abs();
        if distance(&self.background) >= distance(&self.text) {
            self.background.clone()
        } else {
            self.text.clone()
        }
    }

    // MARK: - Task checkbox (§8.5, §11.4)

    const TASK_FIELD_ALPHA: f64 = 0.14;

    /// Wash behind a completed box.
    pub fn task_field_color(&self) -> Retained<NSColor> {
        self.accent
            .panel_alpha(StyleSheet::TASK_FIELD_ALPHA, self.increase_contrast)
    }

    /// The box's outline.
    pub fn task_ring_color(&self, checked: bool) -> Retained<NSColor> {
        if checked {
            self.accent.panel_alpha(0.55, self.increase_contrast)
        } else {
            self.text_secondary
                .panel_alpha(0.70, self.increase_contrast)
        }
    }

    /// The tick: the accent, pulled toward the text colour when the accent
    /// cannot carry a stroke against the field it lands on.
    pub fn task_tick_color(&self) -> Retained<NSColor> {
        let field =
            ColorResolver::blend(&self.background, &self.accent, StyleSheet::TASK_FIELD_ALPHA);
        let ratio = StyleSheet::contrast_ratio(&self.accent, &field);
        let base = if ratio < 4.0 {
            ColorResolver::blend(&self.accent, &self.text, 0.5)
        } else {
            self.accent.clone()
        };
        base.panel_alpha(0.90, self.increase_contrast)
    }

    pub fn contrast_ratio(a: &NSColor, b: &NSColor) -> f64 {
        let first = StyleSheet::relative_luminance(a);
        let second = StyleSheet::relative_luminance(b);
        (smax(first, second) + 0.05) / (smin(first, second) + 0.05)
    }

    /// WCAG relative luminance, on the sRGB components.
    pub fn relative_luminance(color: &NSColor) -> f64 {
        let Some(srgb) = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()) else {
            return 0.5;
        };
        let channel = |value: f64| -> f64 {
            if value <= 0.03928 {
                value / 12.92
            } else {
                pow((value + 0.055) / 1.055, 2.4)
            }
        };
        0.2126 * channel(srgb.redComponent())
            + 0.7152 * channel(srgb.greenComponent())
            + 0.0722 * channel(srgb.blueComponent())
    }

    fn code_colors_for(
        code: &CodeTheme,
        text: &Retained<NSColor>,
        resolver: &ColorResolver<'_>,
    ) -> [Retained<NSColor>; 15] {
        let color = |token: SyntaxToken| -> Retained<NSColor> {
            match token {
                SyntaxToken::Plain => text.clone(),
                SyntaxToken::Keyword => resolver.resolve(&code.keyword),
                SyntaxToken::String => resolver.resolve(&code.string),
                SyntaxToken::Number => resolver.resolve(&code.number),
                SyntaxToken::Comment => resolver.resolve(&code.comment),
                SyntaxToken::Type => resolver.resolve(&code.r#type),
                SyntaxToken::Function => resolver.resolve(&code.function),
                SyntaxToken::Variable => resolver.resolve(&code.variable),
                SyntaxToken::Constant => resolver.resolve(&code.constant),
                SyntaxToken::Operator => resolver.resolve(&code.operator),
                SyntaxToken::Punctuation => resolver.resolve(&code.punctuation),
                SyntaxToken::Attribute => resolver.resolve(&code.attribute),
                SyntaxToken::DiffAdded => resolver.resolve(&code.diff_added),
                SyntaxToken::DiffRemoved => resolver.resolve(&code.diff_removed),
                SyntaxToken::DiffHeader => resolver.resolve(&code.diff_header),
            }
        };
        // The Swift dictionary literal resolves its values in this order.
        std::array::from_fn(|index| color(SyntaxToken::ALL_CASES[index]))
    }
}

// MARK: - Colour resolution

/// Turns a `ThemeColor` into a concrete colour for one appearance.
///
/// System colours are dynamic catalogue colours; reading their components
/// without a current appearance gives whatever the process last drew in.
/// Snapshotting them here is what lets a `StyleSheet` be built off the drawing
/// path and still be right (§11.2).
pub struct ColorResolver<'a> {
    pub appearance: &'a NSAppearance,
}

impl ColorResolver<'_> {
    pub fn resolve(&self, theme_color: &ThemeColor) -> Retained<NSColor> {
        ColorResolver::snapshot(&theme_color.resolved(), self.appearance)
    }

    /// Increase Contrast (§11.4) pulls the *quiet* colours a third of the way
    /// toward the primary text colour.
    pub fn resolve_towards(
        &self,
        theme_color: &ThemeColor,
        target: &NSColor,
        enabled: bool,
    ) -> Retained<NSColor> {
        let base = self.resolve(theme_color);
        if !enabled {
            return base;
        }
        ColorResolver::blend(&base, target, 1.0 / 3.0)
    }

    fn snapshot(color: &NSColor, appearance: &NSAppearance) -> Retained<NSColor> {
        let resolved = RefCell::new(color.retain());
        let block = RcBlock::new(|| {
            let converted = color
                .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
                .unwrap_or_else(|| color.retain());
            *resolved.borrow_mut() = converted;
        });
        appearance.performAsCurrentDrawingAppearance(&block);
        drop(block);
        resolved.into_inner()
    }

    pub fn blend(a: &NSColor, b: &NSColor, t: f64) -> Retained<NSColor> {
        let srgb = NSColorSpace::sRGBColorSpace();
        let (Some(lhs), Some(rhs)) = (a.colorUsingColorSpace(&srgb), b.colorUsingColorSpace(&srgb))
        else {
            return a.retain();
        };
        let mix = smin(smax(t, 0.0), 1.0);
        NSColor::colorWithSRGBRed_green_blue_alpha(
            lhs.redComponent() + (rhs.redComponent() - lhs.redComponent()) * mix,
            lhs.greenComponent() + (rhs.greenComponent() - lhs.greenComponent()) * mix,
            lhs.blueComponent() + (rhs.blueComponent() - lhs.blueComponent()) * mix,
            lhs.alphaComponent() + (rhs.alphaComponent() - lhs.alphaComponent()) * mix,
        )
    }
}
