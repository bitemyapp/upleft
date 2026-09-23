//! Port of `Tests/MarkdownRenderTests/ThemeTests.swift`: theming,
//! typography and colour (§11.1, §11.2).

use std::sync::{Arc, Mutex};

use objc2::AllocAnyThread;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor, NSColorSpace,
    NSFontDescriptorSymbolicTraits, NSImage,
};
use objc2_foundation::{NSString, NSUserDefaults};
use upleft_render::core_types::{CalloutKind, ChangeKind};
use upleft_render::render_contracts::{BodyPreset, Theme, ThemeAppearance, ThemeColor};
use upleft_render::swift_compat::pow;
use upleft_render::syntax::syntax_contracts::SyntaxToken;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::{ThemeStore, ThemeStoreError};
use upleft_render::theme::vscode_theme_import::{
    JsoncSanitizer, Rgba, ScopeEntry, VSCodeThemeImporter,
};

const BUNDLED_NAMES: [&str; 6] = [
    "High Contrast",
    "Nord",
    "Paper Light",
    "Solarized Light",
    "System",
    "Warm Dark",
];
const SUITE: &str = "downright.tests.themes";

/// `select(named:)` persists to `UserDefaults`, so every store here runs
/// against an isolated suite, and the tests that select run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn isolated_defaults() -> Retained<NSUserDefaults> {
    NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(&NSString::from_str(SUITE)))
        .expect("suite")
}

fn store() -> Arc<ThemeStore> {
    ThemeStore::new(isolated_defaults())
}

fn bundled() -> Vec<Theme> {
    store()
        .themes()
        .into_iter()
        .filter(|theme| BUNDLED_NAMES.contains(&theme.name.as_str()))
        .collect()
}

fn named(name: &str) -> Theme {
    bundled()
        .into_iter()
        .find(|theme| theme.name == name)
        .expect(name)
}

fn aqua() -> Retained<NSAppearance> {
    NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).unwrap()
}

fn dark_aqua() -> Retained<NSAppearance> {
    NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }).unwrap()
}

fn sheet(theme: &Theme, appearance: &NSAppearance) -> StyleSheet {
    StyleSheet::new(theme.clone(), appearance, None)
}

fn expect_close(actual: f64, expected: f64, tolerance: f64, comment: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{comment} — {actual} vs {expected}"
    );
}

fn srgb(color: &NSColor) -> Retained<NSColor> {
    color
        .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
        .expect("sRGB")
}

#[test]
fn reader_profile_can_only_add_reduced_motion() {
    let sheet = StyleSheet::new(Theme::fallback(), &aqua(), Some(true));
    assert!(sheet.reduce_motion);
}

// MARK: - Bundled themes

#[test]
fn all_six_bundled_themes_decode() {
    let mut names: Vec<String> = bundled().into_iter().map(|theme| theme.name).collect();
    names.sort();
    assert_eq!(names, BUNDLED_NAMES);
}

#[test]
fn every_colour_parses() {
    for theme in bundled() {
        assert!(
            theme.invalid_color_paths().is_empty(),
            "{} has unparseable colours",
            theme.name
        );
        // 28 palette roles + 14 code tokens.
        assert_eq!(
            theme.all_colors().len(),
            28 + 14,
            "{}: unexpected colour count",
            theme.name
        );
        for (path, color) in theme.all_colors() {
            assert!(
                color.validated().is_some(),
                "{}.{path} = {}",
                theme.name,
                color.raw
            );
        }
    }
}

#[test]
fn essential_text_roles_meet_contrast() {
    for theme in bundled() {
        assert!(
            theme.semantic_contrast_failures(None).is_empty(),
            "{} has low-contrast essential text",
            theme.name
        );
    }
}

#[test]
fn start_window_primary_action_contrast() {
    let white = NSColor::whiteColor();
    let black = NSColor::blackColor();
    for theme in bundled() {
        for appearance in [aqua(), dark_aqua()] {
            let sheet = sheet(&theme, &appearance);
            let base = sheet.start_window_primary_action();
            let hover = base
                .blendedColorWithFraction_ofColor(0.08, &white)
                .unwrap_or_else(|| base.clone());
            let pressed = base
                .blendedColorWithFraction_ofColor(0.18, &black)
                .unwrap_or_else(|| base.clone());
            for fill in [&base, &hover, &pressed] {
                assert!(
                    StyleSheet::contrast_ratio(&white, fill) >= 4.5,
                    "{}: start action is low contrast",
                    theme.name
                );
            }
        }
    }
}

#[test]
fn bundled_themes_round_trip() {
    for theme in bundled() {
        let restored =
            Theme::decode_json(theme.encode_pretty_sorted().as_bytes()).expect("decodes");
        assert_eq!(
            restored, theme,
            "{} did not survive a JSON round trip",
            theme.name
        );
    }
}

#[test]
fn system_theme_uses_system_colours() {
    let theme = named("System");
    let colors = theme.all_colors();
    let references = colors
        .iter()
        .filter(|(_, color)| color.raw.starts_with("system:"))
        .count();
    assert_eq!(
        references,
        colors.len(),
        "the System theme exists to track the user's accent colour (§11.2)"
    );
    assert_eq!(theme.appearance, ThemeAppearance::Auto);
}

#[test]
fn warm_dark_is_designed() {
    let theme = named("Warm Dark");
    let background = srgb(&theme.palette.background.validated().unwrap());
    let text = srgb(&theme.palette.text.validated().unwrap());
    assert!(background.redComponent() > 0.02);
    assert!(background.redComponent() > background.blueComponent());
    assert!(text.redComponent() < 0.95);
    assert!(text.redComponent() > 0.70);
    assert!(text.redComponent() > text.blueComponent());
}

#[test]
fn paper_light_is_warm() {
    let theme = named("Paper Light");
    let background = srgb(&theme.palette.background.validated().unwrap());
    assert!(background.redComponent() > background.blueComponent());
    assert!(background.blueComponent() < 1.0);
}

// MARK: - Store

#[test]
fn selection_and_revision() {
    let _guard = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let store = store();
    store.select("Paper Light");
    let before = store.revision();
    store.select("Nord");
    assert_eq!(store.current().name, "Nord");
    assert!(store.revision() > before);

    // Selecting the same theme again is not a change.
    let steady = store.revision();
    store.select("Nord");
    assert_eq!(store.revision(), steady);

    // An unknown name is ignored rather than clearing the selection.
    store.select("Nope");
    assert_eq!(store.current().name, "Nord");
}

#[test]
fn observers_fire_and_cancel() {
    let _guard = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let store = store();
    store.select("Paper Light");
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = seen.clone();
    let observation = store.observe(move |theme| sink.lock().unwrap().push(theme.name.clone()));
    store.select("Nord");
    store.select("Solarized Light");
    assert_eq!(*seen.lock().unwrap(), ["Nord", "Solarized Light"]);

    observation.cancel();
    store.select("Paper Light");
    assert_eq!(
        *seen.lock().unwrap(),
        ["Nord", "Solarized Light"],
        "a cancelled observation still fired"
    );
}

#[test]
fn export_round_trips() {
    let store = store();
    let theme = store.current();
    let path = std::env::temp_dir().join(format!("downright-export-{}.json", std::process::id()));
    let path_text = path.to_string_lossy().into_owned();
    store.export(&theme, &path_text).expect("export");
    let restored = Theme::decode_json(&std::fs::read(&path).unwrap()).expect("decodes");
    let _ = std::fs::remove_file(&path);
    assert_eq!(restored, theme);
}

// MARK: - Typography (§11.1)

#[test]
fn heading_sizes_follow_the_scale() {
    let aqua = aqua();
    for theme in bundled() {
        let sheet = sheet(&theme, &aqua);
        let sizes: Vec<f64> = (1..=6)
            .map(|level| sheet.heading_font(level).pointSize())
            .collect();
        for level in 1..4 {
            assert!(
                sizes[level - 1] > sizes[level],
                "{}: H{level} is not larger than H{}",
                theme.name,
                level + 1
            );
        }
        let body = theme.typography.body_size;
        let ratio = theme.typography.scale_ratio;
        for (index, exponent) in [3.0, 2.0, 1.25, 0.5, -0.5, -0.75].into_iter().enumerate() {
            expect_close(
                sizes[index],
                body * pow(ratio, exponent),
                0.001,
                &format!("{}: H{}", theme.name, index + 1),
            );
        }
    }
}

#[test]
fn measure_width_is_capped() {
    let aqua = aqua();
    for theme in bundled() {
        let sheet = sheet(&theme, &aqua);
        assert!(sheet.average_character_width > 0.0);
        let characters = sheet.measure_width / sheet.average_character_width;
        assert!(
            characters >= 68.0 - 0.001,
            "{}: measure is {characters} characters",
            theme.name
        );
        assert!(
            characters <= 72.0 + 0.001,
            "{}: measure is {characters} characters",
            theme.name
        );
    }
}

#[test]
fn measure_width_clamps() {
    let aqua = aqua();
    let mut theme = bundled().into_iter().next().unwrap();
    theme.typography.measure_characters = 400.0;
    let sheet = StyleSheet::new(theme.clone(), &aqua, None);
    expect_close(
        sheet.measure_width / sheet.average_character_width,
        72.0,
        0.001,
        "clamped down to 72",
    );

    theme.typography.measure_characters = 10.0;
    let sheet = StyleSheet::new(theme, &aqua, None);
    expect_close(
        sheet.measure_width / sheet.average_character_width,
        68.0,
        0.001,
        "clamped up to 68",
    );
}

#[test]
fn vertical_rhythm_is_on_the_grid() {
    let aqua = aqua();
    for theme in bundled() {
        let sheet = sheet(&theme, &aqua);
        assert!(sheet.baseline_grid > 0.0);
        let name = &theme.name;
        expect_close(
            sheet.baseline_grid * 2.0,
            (sheet.baseline_grid * 2.0).round(),
            0.0001,
            &format!("{name}: half-point grid"),
        );
        expect_close(
            sheet.line_height,
            sheet.line_height.round(),
            0.0001,
            &format!("{name}: whole-point line height"),
        );
        expect_close(
            sheet.line_height % 2.0,
            0.0,
            0.0001,
            &format!("{name}: even line height"),
        );
        expect_close(
            sheet.line_height % sheet.baseline_grid,
            0.0,
            0.001,
            &format!("{name}: line height on the grid"),
        );
        for level in 1..=6 {
            let (before, after) = sheet.heading_spacing(level);
            expect_close(
                before % sheet.baseline_grid,
                0.0,
                0.001,
                &format!("{name}: H{level} space before"),
            );
            expect_close(
                after % sheet.baseline_grid,
                0.0,
                0.001,
                &format!("{name}: H{level} space after"),
            );
        }
    }
}

#[test]
fn presets_pick_different_faces() {
    let aqua = aqua();
    let mut theme = bundled().into_iter().next().unwrap();
    theme.typography.preset = BodyPreset::Working;
    let working = StyleSheet::new(theme.clone(), &aqua, None).body_font();
    theme.typography.preset = BodyPreset::Reading;
    let reading = StyleSheet::new(theme.clone(), &aqua, None).body_font();
    assert_ne!(
        reading.fontName(),
        working.fontName(),
        "Reading is a serif face, Working is SF Pro (§11.1)"
    );
    assert_eq!(reading.pointSize(), theme.typography.body_size);
}

#[test]
fn mono_fallback_and_ligatures() {
    let aqua = aqua();
    let mut theme = bundled().into_iter().next().unwrap();
    theme.typography.mono_family = "This Face Does Not Exist".into();
    let sheet = StyleSheet::new(theme.clone(), &aqua, None);
    let font = sheet.mono_font(None);
    assert!(
        font.isFixedPitch(),
        "the mono fallback chain must end at a monospaced face"
    );
    expect_close(
        font.pointSize(),
        theme.typography.body_size * theme.typography.mono_size_adjust,
        0.001,
        "mono size adjust",
    );
    assert_eq!(sheet.mono_font_attributes(None).1, 0);

    theme.typography.mono_ligatures = true;
    assert_eq!(
        StyleSheet::new(theme, &aqua, None)
            .mono_font_attributes(None)
            .1,
        1
    );
}

#[test]
fn emphasis_fonts() {
    let sheet = sheet(&bundled()[0], &aqua());
    let traits = |bold, italic| {
        sheet
            .emphasis_font(bold, italic)
            .fontDescriptor()
            .symbolicTraits()
    };
    assert!(traits(true, false).contains(NSFontDescriptorSymbolicTraits::TraitBold));
    assert!(traits(false, true).contains(NSFontDescriptorSymbolicTraits::TraitItalic));
    let both = traits(true, true);
    assert!(
        both.contains(NSFontDescriptorSymbolicTraits::TraitBold)
            && both.contains(NSFontDescriptorSymbolicTraits::TraitItalic)
    );
    assert_eq!(sheet.emphasis_font(false, false), sheet.body_font());
}

#[test]
fn deep_headings_have_distinct_treatments() {
    let sheet = sheet(&bundled()[0], &aqua());
    let h5 = sheet.heading_font(5);
    let h6 = sheet.heading_font(6);
    assert!(h5.pointSize() > h6.pointSize());
    assert!(
        h6.fontDescriptor()
            .symbolicTraits()
            .contains(NSFontDescriptorSymbolicTraits::TraitItalic)
    );
    assert_ne!(h5, h6);
}

/// §11.3: sizing math against the x-height lands it near the body size.
#[test]
fn math_size_is_optical() {
    let aqua = aqua();
    for theme in bundled() {
        let sheet = sheet(&theme, &aqua);
        let body = theme.typography.body_size;
        assert!(sheet.math_point_size >= body * 0.89, "{}", theme.name);
        assert!(sheet.math_point_size <= body * 1.11, "{}", theme.name);
    }
}

// MARK: - Colours

#[test]
fn every_style_sheet_colour_resolves() {
    for theme in bundled() {
        for appearance in [aqua(), dark_aqua()] {
            let sheet = sheet(&theme, &appearance);
            let mut colors: Vec<Retained<NSColor>> = [
                &sheet.background,
                &sheet.surface,
                &sheet.text,
                &sheet.text_secondary,
                &sheet.text_faint,
                &sheet.marker,
                &sheet.accent,
                &sheet.link,
                &sheet.rule,
                &sheet.code_background,
                &sheet.code_rule,
                &sheet.quote_rule,
                &sheet.path_missing,
                &sheet.search_hit,
                &sheet.search_hit_current,
                &sheet.selection,
            ]
            .into_iter()
            .cloned()
            .collect();
            colors.extend((1..=6).map(|level| sheet.heading_color(level)));
            colors.extend(
                CalloutKind::ALL_CASES
                    .iter()
                    .map(|kind| sheet.callout_color(*kind)),
            );
            colors.extend(
                ChangeKind::ALL_KINDS
                    .iter()
                    .map(|kind| sheet.change_color(*kind)),
            );
            colors.extend(
                SyntaxToken::ALL_CASES
                    .iter()
                    .map(|token| sheet.code_color(*token)),
            );
            for color in colors {
                assert!(
                    color
                        .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
                        .is_some(),
                    "{}: a colour failed to resolve",
                    theme.name
                );
            }
        }
    }
}

#[test]
fn heading_colours_soften() {
    let sheet = sheet(&named("Paper Light"), &aqua());
    assert_eq!(sheet.heading_color(1), sheet.heading_color(3));
    assert_ne!(sheet.heading_color(3), sheet.heading_color(6));
    assert_eq!(sheet.heading_color(0), sheet.heading_color(1));
    assert_eq!(sheet.heading_color(99), sheet.heading_color(6));
}

#[test]
fn callout_symbols_exist() {
    if NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str("info.circle"),
        None,
    )
    .is_none()
    {
        return; // SF Symbols unavailable in this environment.
    }
    let sheet = sheet(&bundled()[0], &aqua());
    for kind in CalloutKind::ALL_CASES {
        let symbol = sheet.callout_symbol(kind);
        assert!(!symbol.is_empty(), "{kind:?} has no symbol");
        assert!(
            NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str(symbol),
                None
            )
            .is_some(),
            "{kind:?}: {symbol} is not an SF Symbol on this system"
        );
    }
}

#[test]
fn code_and_change_colours() {
    let theme = named("Nord");
    let sheet = sheet(&theme, &dark_aqua());
    let expected = |color: &ThemeColor| srgb(&color.validated().unwrap());
    assert_eq!(
        sheet.code_color(SyntaxToken::Keyword),
        expected(&theme.code.keyword)
    );
    assert_eq!(
        sheet.code_color(SyntaxToken::DiffAdded),
        expected(&theme.code.diff_added)
    );
    assert_eq!(sheet.code_color(SyntaxToken::Plain), sheet.text);
    assert_eq!(
        sheet.change_color(ChangeKind::Inserted),
        expected(&theme.palette.change_added)
    );
    assert_eq!(
        sheet.change_color(ChangeKind::Deleted),
        expected(&theme.palette.change_removed)
    );
    assert_eq!(
        sheet.change_color(ChangeKind::Modified),
        expected(&theme.palette.change_modified)
    );
}

#[test]
fn invalid_colour_is_detectable() {
    let broken = ThemeColor::new("#ggg");
    assert!(broken.validated().is_none());
    assert_eq!(broken.resolved(), NSColor::labelColor());
    assert!(ThemeColor::new("system:notAColour").validated().is_none());
    assert!(
        ThemeColor::new("system:controlAccentColor")
            .validated()
            .is_some()
    );
    assert!(ThemeColor::new("#11223344").validated().is_some());
}

// MARK: - VS Code import (§11.2)

const SAMPLE_VSCODE_THEME: &str = r##"{
  // A comment, because shipped themes are JSONC.
  "name": "Downright Import Fixture",
  "type": "dark",
  "colors": {
    "editor.background": "#101418",
    "editor.foreground": "#d0d6dd",
    "editor.selectionBackground": "#2a3a4a80",
    "textLink.foreground": "#7aa2f7",
    "editorError.foreground": "#e06c75",
    "editorWarning.foreground": "#e5c07b",
    "gitDecoration.addedResourceForeground": "#98c379",
    "unrelated.null": null,
  },
  "tokenColors": [
    { "scope": "comment", "settings": { "foreground": "#5c6370", "fontStyle": "italic" } },
    { "scope": ["string", "string.quoted.double"], "settings": { "foreground": "#98c379" } },
    { "scope": "keyword.control, storage.type", "settings": { "foreground": "#c678dd" } },
    { "scope": "constant.numeric", "settings": { "foreground": "#d19a66" } },
    { "scope": "entity.name.function", "settings": { "foreground": "#61afef" } },
    { "scope": "entity.name.type", "settings": { "foreground": "#e5c07b" } },
    { "scope": "markup.inserted", "settings": { "foreground": "#98c379" } },
    { "scope": "markup.deleted", "settings": { "foreground": "#e06c75" } },
  ]
}"##;

fn check_imported_fixture(theme: &Theme) {
    assert_eq!(theme.name, "Downright Import Fixture");
    assert_eq!(theme.appearance, ThemeAppearance::Dark);
    assert_eq!(theme.palette.background.raw, "#101418");
    assert_eq!(theme.palette.text.raw, "#d0d6dd");
    assert_eq!(theme.palette.link.raw, "#7aa2f7");
    assert_eq!(
        theme.palette.selection.raw, "#2a3a4a80",
        "translucent highlight colours keep their alpha"
    );

    // TextMate scopes map onto the small `SyntaxToken` palette.
    assert_eq!(theme.code.comment.raw, "#5c6370");
    assert_eq!(theme.code.string.raw, "#98c379");
    assert_eq!(theme.code.keyword.raw, "#c678dd");
    assert_eq!(theme.code.number.raw, "#d19a66");
    assert_eq!(theme.code.function.raw, "#61afef");
    assert_eq!(theme.code.r#type.raw, "#e5c07b");
    assert_eq!(theme.code.diff_added.raw, "#98c379");
    assert_eq!(theme.code.diff_removed.raw, "#e06c75");

    // Colours the theme never states are derived, not left blank.
    assert!(theme.invalid_color_paths().is_empty());
    assert_ne!(
        theme.palette.code_background.raw,
        theme.palette.background.raw
    );
    let _ = StyleSheet::new(theme.clone(), &dark_aqua(), None);
}

/// The importer half of `vsCodeThemeImport`, without touching the user's
/// themes folder.
#[test]
fn vscode_theme_imports_and_maps_scopes() {
    let theme =
        VSCodeThemeImporter::theme(SAMPLE_VSCODE_THEME.as_bytes(), "fallback").expect("imports");
    check_imported_fixture(&theme);
}

/// The whole of `vsCodeThemeImport`: like the Swift test, this installs the
/// fixture into `~/Library/Application Support/Downright/Themes` and removes
/// it again, so it only runs on request.
#[test]
#[ignore = "writes to the real user themes folder, as the Swift test does"]
fn vscode_theme_import_installs() {
    let path = std::env::temp_dir().join(format!("downright-vscode-{}.json", std::process::id()));
    std::fs::write(&path, SAMPLE_VSCODE_THEME).unwrap();
    let store = store();
    let result = store.import_vscode_theme(&path.to_string_lossy());
    let _ = std::fs::remove_file(&path);
    let theme = result.expect("imports");
    check_imported_fixture(&theme);
    assert!(
        store
            .themes()
            .iter()
            .any(|candidate| candidate.name == theme.name)
    );
    if let Some(directory) = ThemeStore::user_themes_directory().and_then(|url| url.path()) {
        let file = std::path::Path::new(&directory.to_string())
            .join(ThemeStore::slug(&theme.name) + ".json");
        let _ = std::fs::remove_file(file);
    }
}

#[test]
fn import_rejects_non_themes() {
    let path =
        std::env::temp_dir().join(format!("downright-not-a-theme-{}.json", std::process::id()));
    std::fs::write(&path, "{\"unrelated\": 1}").unwrap();
    let result = store().import_vscode_theme(&path.to_string_lossy());
    let _ = std::fs::remove_file(&path);
    assert_eq!(result.unwrap_err(), ThemeStoreError::NotAVSCodeTheme);
}

#[test]
fn jsonc_sanitizer() {
    let source = r#"{"a": "http://x/y // not a comment", /* dropped */ "b": [1, 2,], }"#;
    let cleaned = JsoncSanitizer::strip(source.as_bytes());
    let object: serde_json::Value = serde_json::from_slice(&cleaned).expect("valid JSON");
    assert_eq!(object["a"], "http://x/y // not a comment");
    assert_eq!(object["b"], serde_json::json!([1, 2]));
}

#[test]
fn jsonc_sanitizer_trailing_comma_with_comment() {
    let source = "{\n  \"colors\": {\n    \"focusBorder\": \"#007acc\", // primary border\n  }\n}";
    let cleaned = JsoncSanitizer::strip(source.as_bytes());
    let object: serde_json::Value = serde_json::from_slice(&cleaned).expect("valid JSON");
    assert_eq!(object["colors"]["focusBorder"], "#007acc");
}

#[test]
fn scope_matching() {
    let entries = [
        ScopeEntry {
            selector: "comment".into(),
            color: Rgba::parse("#111111").unwrap(),
        },
        ScopeEntry {
            selector: "comment.line.double-slash".into(),
            color: Rgba::parse("#222222").unwrap(),
        },
        ScopeEntry {
            selector: "string".into(),
            color: Rgba::parse("#333333").unwrap(),
        },
    ];
    assert_eq!(
        VSCodeThemeImporter::foreground("comment", &entries)
            .unwrap()
            .hex_string(),
        "#111111"
    );
    assert_eq!(
        VSCodeThemeImporter::foreground("comment.line.double-slash", &entries)
            .unwrap()
            .hex_string(),
        "#222222"
    );
    assert!(VSCodeThemeImporter::foreground("keyword", &entries).is_none());
}

// MARK: - Port-specific checks

/// The isolated suite never touches the standard domain's selection.
#[test]
fn isolated_suite_leaves_standard_defaults_alone() {
    let _guard = SERIAL.lock().unwrap_or_else(|poison| poison.into_inner());
    let key = NSString::from_str("downright.theme.selected");
    let before = NSUserDefaults::standardUserDefaults().stringForKey(&key);
    let store = store();
    store.select("Nord");
    store.select("Paper Light");
    assert_eq!(
        NSUserDefaults::standardUserDefaults().stringForKey(&key),
        before
    );
    isolated_defaults().removePersistentDomainForName(&NSString::from_str(SUITE));
}

#[test]
fn slugs_follow_characters() {
    assert_eq!(
        ThemeStore::slug("Downright Import Fixture"),
        "downright-import-fixture"
    );
    assert_eq!(ThemeStore::slug("  --a__b--  "), "a-b");
    assert_eq!(ThemeStore::slug(""), "theme");
    assert_eq!(ThemeStore::slug("Été à Paris"), "été-à-paris");
}
