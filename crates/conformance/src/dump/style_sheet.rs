//! Rust counterpart of `oracle/Sources/downright-oracle/StyleSheetDump.swift`
//! (`stylesheet`), and the shared theme serialisation (`ThemeDump`).

use objc2::rc::Retained;
use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor};
use serde_json::Value;
use upleft_render::core_types::CalloutKind;
use upleft_render::engine::render_metrics::{self as rm, RoundingRule};
use upleft_render::render_contracts::{
    CodeTheme, DecorationPolicy, FragmentKind, MarkdownRenderConfiguration, MarkdownRevealPolicy,
    RenderMode, Theme, ThemeColor, ThemePalette, TypographyConfig, attribute_keys,
};
use upleft_render::syntax::syntax_contracts::SyntaxToken;
use upleft_render::theme::preview_appearance::PreviewAppearance;
use upleft_render::theme::style_sheet::{ColorResolver, StyleSheet};
use upleft_render::theme::theme_store::ThemeStore;
use upleft_render::view::style_sheet_defaults::{GutterChrome, PanelAlpha};

use super::Failure;
use super::attribute_dump::{color_json, font_json};
use super::json::{Object, double};

pub fn appearance_named(dark: bool) -> Retained<NSAppearance> {
    // SAFETY: AppKit exports the appearance names as immutable globals.
    let name = unsafe {
        if dark {
            NSAppearanceNameDarkAqua
        } else {
            NSAppearanceNameAqua
        }
    };
    NSAppearance::appearanceNamed(name).expect("aqua appearances exist")
}

fn optional_string(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |text| Value::String(text.to_owned()))
}

pub mod theme_dump {
    use super::*;
    use upleft_render::theme::theme_validation::ThemeContrastFailure;

    pub fn theme(theme: &Theme) -> Value {
        Object::new()
            .with("name", theme.name.clone())
            .with("appearance", theme.appearance.raw_value())
            .with("palette", palette(&theme.palette))
            .with("code", code(&theme.code))
            .with("typography", typography(&theme.typography))
            .build()
    }

    pub fn palette(p: &ThemePalette) -> Value {
        let mut object = Object::new();
        for (label, color) in [
            ("background", &p.background),
            ("surface", &p.surface),
            ("text", &p.text),
            ("textSecondary", &p.text_secondary),
            ("textFaint", &p.text_faint),
            ("heading", &p.heading),
            ("marker", &p.marker),
            ("accent", &p.accent),
            ("link", &p.link),
            ("rule", &p.rule),
            ("selection", &p.selection),
            ("codeBackground", &p.code_background),
            ("inlineCodeBackground", &p.inline_code_background),
            ("codeRule", &p.code_rule),
            ("railTick", &p.rail_tick),
            ("railTickCurrent", &p.rail_tick_current),
            ("quoteRule", &p.quote_rule),
            ("changeAdded", &p.change_added),
            ("changeRemoved", &p.change_removed),
            ("changeModified", &p.change_modified),
            ("pathMissing", &p.path_missing),
            ("searchHit", &p.search_hit),
            ("searchHitCurrent", &p.search_hit_current),
            ("calloutNote", &p.callout_note),
            ("calloutWarning", &p.callout_warning),
            ("calloutSuccess", &p.callout_success),
            ("calloutDanger", &p.callout_danger),
        ] {
            object = object.with(label, color.raw.clone());
        }
        object
            .with(
                "calloutImportant",
                optional_string(p.callout_important.as_ref().map(|c| c.raw.as_str())),
            )
            .build()
    }

    pub fn code(c: &CodeTheme) -> Value {
        let mut object = Object::new();
        for (label, color) in c.colors() {
            object = object.with(label, color.raw.clone());
        }
        object.build()
    }

    pub fn typography(t: &TypographyConfig) -> Value {
        Object::new()
            .with("preset", t.preset.raw_value())
            .with("bodySize", double(t.body_size))
            .with("scaleRatio", double(t.scale_ratio))
            .with("lineHeightMultiple", double(t.line_height_multiple))
            .with("measureCharacters", double(t.measure_characters))
            .with("monoFamily", t.mono_family.clone())
            .with("monoSizeAdjust", double(t.mono_size_adjust))
            .with("monoLigatures", t.mono_ligatures)
            .with("opticalMargins", t.optical_margins)
            .with("mathScale", double(t.math_scale))
            .build()
    }

    pub fn validation(theme: &Theme) -> Value {
        let failures = |appearance: Option<&NSAppearance>| -> Value {
            Value::Array(
                theme
                    .semantic_contrast_failures(appearance)
                    .iter()
                    .map(|failure: &ThemeContrastFailure| {
                        Object::new()
                            .with("path", failure.path.clone())
                            .with("ratio", double(failure.ratio))
                            .with("minimum", double(failure.minimum))
                            .build()
                    })
                    .collect(),
            )
        };
        Object::new()
            .with(
                "allColors",
                Value::Array(
                    theme
                        .all_colors()
                        .iter()
                        .map(|(path, color)| {
                            Object::new()
                                .with("path", path.clone())
                                .with("raw", color.raw.clone())
                                .with(
                                    "validated",
                                    color.validated().map_or(Value::Null, |c| color_json(&c)),
                                )
                                .build()
                        })
                        .collect(),
                ),
            )
            .with(
                "invalidColorPaths",
                Value::Array(
                    theme
                        .invalid_color_paths()
                        .into_iter()
                        .map(Value::from)
                        .collect(),
                ),
            )
            .with("contrastDefault", failures(None))
            .with("contrastAqua", failures(Some(&appearance_named(false))))
            .with("contrastDarkAqua", failures(Some(&appearance_named(true))))
            .build()
    }
}

const COLOR_PROBES: [&str; 26] = [
    "#11223344",
    "#112233",
    "112233",
    "  #aBcDeF\t",
    "#+12345",
    "#-00000",
    "#-00001",
    "#ggg",
    "#fff",
    "#12345",
    "#1234567",
    "#123456789",
    "",
    "#",
    "system:label",
    "system:labelColor",
    "system:accent",
    "system:controlAccentColor",
    "system:notAColour",
    "system:",
    "System:label",
    "system:systemGray",
    "system:underPageBackground",
    "#\u{301}112233",
    "\u{200B}#112233\u{200B}",
    "#１２３４５６",
];

type TypographyEdit = fn(&mut TypographyConfig);

const TYPOGRAPHY_VARIANTS: [(&str, TypographyEdit); 7] = [
    ("working", |t| {
        t.preset = upleft_render::render_contracts::BodyPreset::Working
    }),
    ("menlo-ligatures", |t| {
        t.mono_family = "Menlo".into();
        t.mono_ligatures = true;
    }),
    ("missing-face", |t| {
        t.mono_family = "This Face Does Not Exist".into()
    }),
    ("empty-face", |t| t.mono_family = String::new()),
    ("wide-measure", |t| {
        t.measure_characters = 400.0;
        t.body_size = 19.0;
        t.line_height_multiple = 1.3;
    }),
    ("narrow-measure", |t| {
        t.measure_characters = 10.0;
        t.body_size = 13.5;
        t.scale_ratio = 1.333;
    }),
    ("tiny", |t| {
        t.body_size = 3.0;
        t.line_height_multiple = 1.0;
        t.math_scale = 1.2;
    }),
];

pub fn dump(theme_name: &str, dark: bool) -> Result<Value, Failure> {
    let appearance = appearance_named(dark);
    let store = ThemeStore::shared();
    let Some(theme) = store
        .themes()
        .into_iter()
        .find(|theme| theme.name == theme_name)
    else {
        let names: Vec<String> = store.themes().into_iter().map(|theme| theme.name).collect();
        return Err(Failure::Error(format!(
            "unknown theme {theme_name}; have {names:?}"
        )));
    };
    let sheet = StyleSheet::new(theme.clone(), &appearance, Some(true));

    let mut variants = Vec::new();
    for (label, edit) in TYPOGRAPHY_VARIANTS {
        let mut modified = theme.clone();
        edit(&mut modified.typography);
        let variant = StyleSheet::new(modified.clone(), &appearance, Some(false));
        variants.push(
            Object::new()
                .with("label", label)
                .with("typography", theme_dump::typography(&modified.typography))
                .with("fonts", fonts(&variant))
                .with("metrics", metrics(&variant))
                .build(),
        );
    }

    let resolver = ColorResolver {
        appearance: &appearance,
    };
    let probes: Vec<Value> = COLOR_PROBES
        .iter()
        .map(|raw| {
            let color = ThemeColor::new(raw);
            Object::new()
                .with("raw", *raw)
                .with(
                    "validated",
                    color.validated().map_or(Value::Null, |c| color_json(&c)),
                )
                .with("resolved", color_json(&color.resolved()))
                .with("snapshot", color_json(&resolver.resolve(&color)))
                .build()
        })
        .collect();

    let slugs: Vec<Value> = [
        theme.name.as_str(),
        "Été à Paris",
        "  --a__b--  ",
        "",
        "日本語テーマ",
        "x\u{301}y",
    ]
    .iter()
    .map(|name| Value::String(ThemeStore::slug(name)))
    .collect();

    Ok(Object::new()
        .with("theme", theme_dump::theme(&theme))
        .with("revision", sheet.revision)
        .with("appearance", sheet.appearance.name().to_string())
        .with(
            "accessibility",
            Object::new()
                .with("reduceMotion", sheet.reduce_motion)
                .with("increaseContrast", sheet.increase_contrast)
                .with("reduceTransparency", sheet.reduce_transparency)
                .build(),
        )
        .with("metrics", metrics(&sheet))
        .with("fonts", fonts(&sheet))
        .with("colors", colors(&sheet))
        .with("typographyVariants", Value::Array(variants))
        .with("validation", theme_dump::validation(&theme))
        .with("colorProbes", Value::Array(probes))
        .with("boosted", {
            let p = &theme.palette;
            let text = resolver.resolve(&p.text);
            Value::Array(
                [
                    &p.text_secondary,
                    &p.text_faint,
                    &p.marker,
                    &p.rule,
                    &p.code_rule,
                    &p.rail_tick,
                    &p.rail_tick_current,
                    &p.quote_rule,
                ]
                .into_iter()
                .map(|color| color_json(&resolver.resolve_towards(color, &text, true)))
                .collect(),
            )
        })
        .with(
            "renderMetrics",
            render_metrics(theme.typography.body_size, sheet.baseline_grid),
        )
        .with(
            "themeStore",
            Object::new()
                .with(
                    "themes",
                    Value::Array(
                        store
                            .themes()
                            .into_iter()
                            .map(|t| Value::String(t.name))
                            .collect(),
                    ),
                )
                .with("current", store.current().name)
                .with("revision", store.revision())
                .with("slugs", Value::Array(slugs))
                .build(),
        )
        .with("fallback", theme_dump::theme(&Theme::fallback()))
        .with("contracts", contracts())
        .build())
}

fn metrics(sheet: &StyleSheet) -> Value {
    Object::new()
        .with("baselineGrid", double(sheet.baseline_grid))
        .with("lineHeight", double(sheet.line_height))
        .with(
            "averageCharacterWidth",
            double(sheet.average_character_width),
        )
        .with("measureWidth", double(sheet.measure_width))
        .with("mathPointSize", double(sheet.math_point_size))
        .with(
            "headingSpacing",
            Value::Array(
                (0..=7)
                    .map(|level| {
                        let (before, after) = sheet.heading_spacing(level);
                        Value::Array(vec![double(before), double(after)])
                    })
                    .collect(),
            ),
        )
        .with(
            "headingSizes",
            Value::Array(
                (0..=7)
                    .map(|level| double(StyleSheet::heading_size(level, &sheet.theme.typography)))
                    .collect(),
            ),
        )
        .build()
}

fn fonts(sheet: &StyleSheet) -> Value {
    let mono = sheet.mono_font(None);
    let sizes: [Option<f64>; 8] = [
        None,
        Some(9.0),
        Some(11.0),
        Some(12.5),
        Some(13.0),
        Some(14.0),
        Some(mono.pointSize()),
        Some(20.0),
    ];
    Object::new()
        .with("body", font_json(&sheet.body_font()))
        .with(
            "headings",
            Value::Array(
                (0..=7)
                    .map(|level| font_json(&sheet.heading_font(level)))
                    .collect(),
            ),
        )
        .with("mono", font_json(&mono))
        .with(
            "monoSized",
            Value::Array(
                sizes
                    .iter()
                    .map(|size| font_json(&sheet.mono_font(*size)))
                    .collect(),
            ),
        )
        .with(
            "monoAttributes",
            Value::Array(
                sizes
                    .iter()
                    .map(|size| {
                        let (font, ligature) = sheet.mono_font_attributes(*size);
                        Object::new()
                            .with("font", font_json(&font))
                            .with("ligature", ligature as i64)
                            .build()
                    })
                    .collect(),
            ),
        )
        .with(
            "emphasis",
            Value::Array(
                [(false, false), (true, false), (false, true), (true, true)]
                    .iter()
                    .map(|(bold, italic)| font_json(&sheet.emphasis_font(*bold, *italic)))
                    .collect(),
            ),
        )
        .build()
}

fn colors(sheet: &StyleSheet) -> Value {
    let named: [(&str, &NSColor); 19] = [
        ("background", &sheet.background),
        ("surface", &sheet.surface),
        ("text", &sheet.text),
        ("textSecondary", &sheet.text_secondary),
        ("textFaint", &sheet.text_faint),
        ("marker", &sheet.marker),
        ("accent", &sheet.accent),
        ("link", &sheet.link),
        ("rule", &sheet.rule),
        ("codeBackground", &sheet.code_background),
        ("inlineCodeBackground", &sheet.inline_code_background),
        ("codeRule", &sheet.code_rule),
        ("railTick", &sheet.rail_tick),
        ("railTickCurrent", &sheet.rail_tick_current),
        ("quoteRule", &sheet.quote_rule),
        ("pathMissing", &sheet.path_missing),
        ("searchHit", &sheet.search_hit),
        ("searchHitCurrent", &sheet.search_hit_current),
        ("selection", &sheet.selection),
    ];
    let mut palette = Object::new();
    let mut luminance = Object::new();
    for (label, color) in named {
        palette = palette.with(label, color_json(color));
        luminance = luminance.with(label, double(StyleSheet::relative_luminance(color)));
    }
    let white = NSColor::whiteColor();
    let label = NSColor::labelColor();
    Object::new()
        .with("palette", palette.build())
        .with("luminance", luminance.build())
        .with(
            "headings",
            Value::Array(
                (0..=7)
                    .map(|level| color_json(&sheet.heading_color(level)))
                    .collect(),
            ),
        )
        .with(
            "callouts",
            Value::Array(
                CalloutKind::ALL_CASES
                    .iter()
                    .map(|kind| {
                        Object::new()
                            .with("kind", kind.raw_value())
                            .with("color", color_json(&sheet.callout_color(*kind)))
                            .with("symbol", sheet.callout_symbol(*kind))
                            .build()
                    })
                    .collect(),
            ),
        )
        .with(
            "changes",
            Value::Array(
                upleft_render::core_types::CHANGE_KINDS
                    .iter()
                    .map(|kind| {
                        Object::new()
                            .with("kind", kind.raw_value())
                            .with("color", color_json(&sheet.change_color(*kind)))
                            .build()
                    })
                    .collect(),
            ),
        )
        .with(
            "code",
            Value::Array(
                SyntaxToken::ALL_CASES
                    .iter()
                    .map(|token| {
                        Object::new()
                            .with("token", token.raw_value())
                            .with("color", color_json(&sheet.code_color(*token)))
                            .build()
                    })
                    .collect(),
            ),
        )
        .with(
            "startWindowPrimaryAction",
            color_json(&sheet.start_window_primary_action()),
        )
        .with("onAccent", color_json(&sheet.on_accent()))
        .with("taskFieldColor", color_json(&sheet.task_field_color()))
        .with("taskRingChecked", color_json(&sheet.task_ring_color(true)))
        .with("taskRingOpen", color_json(&sheet.task_ring_color(false)))
        .with("taskTickColor", color_json(&sheet.task_tick_color()))
        .with(
            "panelAlpha",
            Value::Array(
                [(0.12, false), (0.12, true), (0.7, true), (1.0, false)]
                    .iter()
                    .map(|(alpha, boost)| color_json(&sheet.accent.panel_alpha(*alpha, *boost)))
                    .collect(),
            ),
        )
        .with(
            "contrast",
            Value::Array(
                [
                    StyleSheet::contrast_ratio(&sheet.text, &sheet.background),
                    StyleSheet::contrast_ratio(&white, &sheet.start_window_primary_action()),
                    StyleSheet::contrast_ratio(&sheet.accent, &sheet.surface),
                    StyleSheet::contrast_ratio(&label, &sheet.background),
                ]
                .into_iter()
                .map(double)
                .collect(),
            ),
        )
        .with(
            "blend",
            Value::Array(
                [-0.5, 0.0, 0.25, 1.0 / 3.0, 0.5, 1.0, 2.0]
                    .into_iter()
                    .map(|t| color_json(&ColorResolver::blend(&sheet.text, &sheet.accent, t)))
                    .collect(),
            ),
        )
        .with(
            "blendCatalog",
            color_json(&ColorResolver::blend(&label, &sheet.background, 0.5)),
        )
        .build()
}

fn render_metrics(body_size: f64, grid: f64) -> Value {
    let values = [0.0, 0.4, 1.0, 12.5, 13.0, 25.99, 26.0, 26.01, 39.0, -7.5];
    let constants = [
        rm::GUTTER_WIDTH,
        rm::REVEAL_SLACK,
        rm::VERTICAL_INSET,
        rm::CODE_BLEED,
        rm::MINIMUM_PROSE_WIDTH,
        rm::CODE_INSET_X,
        rm::CODE_INSET_Y,
        rm::CODE_HEADER_HEIGHT,
        rm::CODE_RULE_WIDTH,
        rm::CODE_CORNER_RADIUS,
        rm::INLINE_CODE_CORNER_RADIUS,
        rm::CODE_BLOCK_GAP,
        rm::TASK_BOX_SIDE,
        rm::TASK_BOX_GAP,
        rm::TASK_BOX_CLEARANCE,
        rm::task_marker_column(),
        rm::TASK_BOX_CORNER_RATIO,
        rm::TASK_BOX_STROKE_RATIO,
        rm::TASK_TICK_STROKE_RATIO,
        rm::CHIP_HEIGHT,
        rm::CALLOUT_RULE_WIDTH,
        rm::CALLOUT_INSET_X,
        rm::CALLOUT_ICON_INSET_X,
        rm::CALLOUT_INSET_Y,
        rm::CALLOUT_CORNER_RADIUS,
        rm::QUOTE_RULE_WIDTH,
        rm::TABLE_ROW_PADDING,
        rm::TABLE_COLUMN_GAP,
        rm::TABLE_RULE_WIDTH,
        rm::IMAGE_CORNER_RADIUS,
        rm::IMAGE_SHADOW_RADIUS,
        rm::IMAGE_CAPTION_GAP,
        rm::THEMATIC_BREAK_SPACE,
    ];
    let doubles = |iter: &mut dyn Iterator<Item = f64>| Value::Array(iter.map(double).collect());
    Object::new()
        .with("constants", doubles(&mut constants.into_iter()))
        .with(
            "integers",
            Value::Array(vec![
                rm::CODE_TAB_COLUMNS.into(),
                rm::CODE_COLLAPSE_LINE_COUNT.into(),
            ]),
        )
        .with(
            "taskTick",
            Value::Array(
                rm::TASK_TICK
                    .iter()
                    .map(|point| Value::Array(vec![double(point.x), double(point.y)]))
                    .collect(),
            ),
        )
        .with(
            "indentUnit",
            doubles(
                &mut [body_size, 16.0, 13.5, 11.0]
                    .into_iter()
                    .map(rm::indent_unit),
            ),
        )
        .with(
            "snapUp",
            doubles(&mut values.into_iter().map(|value| rm::snap_up(value, grid))),
        )
        .with(
            "snapDown",
            doubles(
                &mut values
                    .into_iter()
                    .map(|value| rm::snap(value, grid, RoundingRule::Down)),
            ),
        )
        .with(
            "snapSmallGrid",
            doubles(&mut values.into_iter().map(|value| rm::snap_up(value, 0.5))),
        )
        .build()
}

fn contracts() -> Value {
    let policy = |p: DecorationPolicy| -> Value {
        Value::Array(
            [
                p.shows_insertion_point,
                p.hides_block_markers,
                p.hides_inline_markers,
                p.reveals_at_caret,
                p.reveals_at_all_cursors,
                p.shows_gutter_markers,
                p.highlights_markers,
                p.renders_fragments,
                p.collapses_long_code_blocks,
            ]
            .into_iter()
            .map(Value::Bool)
            .collect(),
        )
    };
    let mut configuration = MarkdownRenderConfiguration::new(
        false,
        MarkdownRevealPolicy::PrimaryCaret,
        false,
        false,
        true,
        0,
        5000,
    );
    let clamped_init = [
        configuration.code_collapse_threshold(),
        configuration.large_file_threshold_megabytes(),
    ];
    configuration.set_code_collapse_threshold(20_000);
    configuration.set_large_file_threshold_megabytes(-3);
    let defaults = MarkdownRenderConfiguration::default();
    Object::new()
        .with(
            "modes",
            Value::Array(
                RenderMode::ALL_CASES
                    .iter()
                    .map(|mode| {
                        Object::new()
                            .with("raw", mode.raw_value())
                            .with("title", mode.title())
                            .with("normalized", mode.normalized_for_editing().raw_value())
                            .with("policy", policy(mode.policy()))
                            .build()
                    })
                    .collect(),
            ),
        )
        .with(
            "userFacingModes",
            Value::Array(
                RenderMode::USER_FACING_MODES
                    .iter()
                    .map(|m| m.raw_value().into())
                    .collect(),
            ),
        )
        .with(
            "fragmentKinds",
            Value::Array(
                FragmentKind::ALL_CASES
                    .iter()
                    .map(|kind| {
                        Value::Array(vec![kind.raw_value().into(), kind.replaces_glyphs().into()])
                    })
                    .collect(),
            ),
        )
        .with(
            "revealPolicies",
            Value::Array(
                MarkdownRevealPolicy::ALL_CASES
                    .iter()
                    .map(|p| p.raw_value().into())
                    .collect(),
            ),
        )
        .with(
            "configuration",
            Value::Array(vec![
                defaults.show_invisibles.into(),
                defaults.reveal_policy.raw_value().into(),
                defaults.typographic_substitution.into(),
                defaults.typewriter_scrolling.into(),
                defaults.reflow_hard_wrapped_paragraphs.into(),
                defaults.code_collapse_threshold().into(),
                defaults.large_file_threshold_megabytes().into(),
                clamped_init[0].into(),
                clamped_init[1].into(),
                configuration.code_collapse_threshold().into(),
                configuration.large_file_threshold_megabytes().into(),
            ]),
        )
        .with(
            "attributeKeys",
            Value::Array(
                [
                    attribute_keys::dr_hidden(),
                    attribute_keys::dr_marker(),
                    attribute_keys::dr_fragment(),
                    attribute_keys::dr_block(),
                    attribute_keys::dr_heading(),
                    attribute_keys::dr_link(),
                    attribute_keys::dr_path_token(),
                    attribute_keys::dr_path_exists(),
                    attribute_keys::dr_checkbox(),
                    attribute_keys::dr_change(),
                    attribute_keys::dr_change_ghost(),
                    attribute_keys::dr_reference(),
                    attribute_keys::dr_elided(),
                    attribute_keys::dr_gutter_marker(),
                    attribute_keys::dr_search_hit(),
                    attribute_keys::dr_current_search_hit(),
                    attribute_keys::dr_speech_highlight(),
                    attribute_keys::dr_inline_code(),
                    attribute_keys::dr_source_focus(),
                    attribute_keys::dr_invisible(),
                ]
                .iter()
                .map(|key| Value::String(key.to_string()))
                .collect(),
            ),
        )
        .with(
            "previewAppearances",
            Value::Array(
                PreviewAppearance::ALL_CASES
                    .iter()
                    .map(|appearance| {
                        Value::Array(vec![
                            appearance.raw_value().into(),
                            appearance.title().into(),
                            appearance
                                .ns_appearance()
                                .map_or(Value::Null, |a| a.name().to_string().into()),
                        ])
                    })
                    .collect(),
            ),
        )
        .with(
            "gutterChrome",
            Value::Array(vec![
                font_json(&GutterChrome::title_font()),
                font_json(&GutterChrome::body_font()),
            ]),
        )
        .build()
}
