//! `PreferencesForms` (PreferencesWindowController.swift, "Form definitions"):
//! one row-builder per pane, evaluated afresh every time a pane appears.
//!
//! Swift's `@MainActor` isolation is a `MainThreadMarker` parameter; the row
//! closures capture it.

use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSAlert, NSAlertStyle, NSFontManager, NSWorkspace};
use objc2_foundation::{NSByteCountFormatter, NSByteCountFormatterCountStyle, NSFileManager, NSString};
use upleft_render::appkit_compat::main_async;
use upleft_render::render_contracts::{BodyPreset, RenderMode, ThemeAppearance};
use upleft_render::theme::preview_appearance::PreviewAppearance;
use upleft_render::theme::theme_store::ThemeStore;
use upleft_swift_text as swift_text;

use super::SettingsPane;
use super::preference_row::{ChoiceSelection, PreferenceRow};
use crate::ai::path_resolver::ExternalEditor;
use crate::ai::snapshot_store::SnapshotStore;
use crate::integrations::agent_integration::AgentIntegration;
use crate::support::commands::Command;
use crate::support::keybindings::KeybindingStore;
use crate::support::preferences::Preferences;
use crate::support::system_integration::SystemIntegration;
use crate::updater::update_coordinator::UpdateCoordinator;

fn ns(text: &str) -> objc2::rc::Retained<NSString> {
    NSString::from_str(text)
}

/// `Array.firstIndex(of:)` over `String`s, with Swift's `==`.
fn first_index_of(strings: &[String], value: &str) -> Option<usize> {
    strings.iter().position(|string| swift_text::str_eq(string, value))
}

/// `NSFontManager.shared.availableFontFamilies.sorted()`.
fn sorted_font_families(mtm: MainThreadMarker) -> Vec<String> {
    let families: Vec<String> =
        NSFontManager::sharedFontManager(mtm).availableFontFamilies().iter().map(|family| family.to_string()).collect();
    swift_text::sort::sorted_by(families, |a, b| swift_text::str_less(a, b))
}

/// `PreferencesForms`.
pub struct PreferencesForms;

impl PreferencesForms {
    /// `rows(for:)`: one row-builder per pane, evaluated afresh every time a
    /// pane appears.
    pub fn rows(pane: SettingsPane, mtm: MainThreadMarker) -> Rc<dyn Fn() -> Vec<PreferenceRow>> {
        match pane {
            SettingsPane::General => Rc::new(move || PreferencesForms::general(mtm)),
            SettingsPane::Appearance => Rc::new(move || PreferencesForms::appearance(mtm)),
            SettingsPane::Typography => Rc::new(move || PreferencesForms::typography(mtm)),
            SettingsPane::Editor => Rc::new(move || PreferencesForms::editor(mtm)),
            SettingsPane::History => Rc::new(move || PreferencesForms::history(mtm)),
            SettingsPane::Updates => Rc::new(move || PreferencesForms::updates(mtm)),
            SettingsPane::Keys => Rc::new(Vec::new),
        }
    }

    /// The shortcut a command answers to right now. Settings copy must never
    /// hard-code a chord: every binding here is user-editable.
    pub fn shortcut(command: Command) -> Option<String> {
        let binding = KeybindingStore::shared().primary_binding(command);
        binding.map(|binding| binding.display_string())
    }

    pub fn general(mtm: MainThreadMarker) -> Vec<PreferenceRow> {
        // Offering an editor that isn't installed makes "Open in Editor" fail
        // with no explanation, so the list is what this Mac actually has.
        let editors: Rc<Vec<ExternalEditor>> =
            Rc::new(ExternalEditor::ALL_CASES.into_iter().filter(|editor| editor.is_installed()).collect());
        let mut rows = vec![
            PreferenceRow::section("On open"),
            PreferenceRow::toggle(
                "Restore windows from the last session",
                None,
                || Preferences::shared().values().restore_session,
                |value| Preferences::shared().update(|values| values.restore_session = value),
            ),
            PreferenceRow::section("Working with agents"),
            PreferenceRow::toggle(
                "Watch files for external changes",
                Some("Marks what changed in the document while you were reading it."),
                || Preferences::shared().values().watch_files,
                |value| Preferences::shared().update(|values| values.watch_files = value),
            ),
            PreferenceRow::toggle(
                "Resolve file paths in documents",
                Some("Underlines paths that aren't there, so a file an agent claims to have written is easy to check."),
                || Preferences::shared().values().resolve_path_tokens,
                |value| Preferences::shared().update(|values| values.resolve_path_tokens = value),
            ),
            PreferenceRow::choice(
                "Open code files in",
                Some("Only apps installed on this Mac are listed."),
                editors.iter().map(|editor| editor.title().to_owned()).collect(),
                {
                    let editors = editors.clone();
                    move || {
                        let current = Preferences::shared().values().external_editor;
                        if let Some(index) = editors.iter().position(|editor| *editor == current) {
                            return ChoiceSelection::Index(index as isize);
                        }
                        ChoiceSelection::Missing(current.title().to_owned())
                    }
                },
                {
                    let editors = editors.clone();
                    move |index| {
                        if !(index >= 0 && (index as usize) < editors.len()) {
                            return;
                        }
                        let editor = editors[index as usize];
                        Preferences::shared().update(|values| values.external_editor = editor);
                    }
                },
            ),
            PreferenceRow::text(
                "Extra sibling folders",
                Some(
                    "Folder names, separated by commas, scanned one level down from the document. Reopen a document for a change here to reach it.",
                ),
                || Preferences::shared().values().sibling_scan_directories.join(", "),
                |value| {
                    Preferences::shared().update(|values| {
                        values.sibling_scan_directories = swift_text::split_default(&value, ',')
                            .into_iter()
                            .map(|part| swift_text::trim_whitespaces(part).to_owned())
                            .filter(|part| !part.is_empty())
                            .collect();
                    })
                },
            ),
        ];
        rows.extend(PreferencesForms::system_integration(mtm));
        rows
    }

    /// The first-run setup panel's steps, kept reachable for good.
    ///
    /// The panel is shown once and answering "Not now" is meant to stick, so
    /// this is where someone goes when they change their mind, when Quick
    /// Look stops working after a macOS update, or when they moved the app
    /// and the registration went stale. Rows are rebuilt every time the pane
    /// appears, so what they say is what is true right now.
    pub fn system_integration(mtm: MainThreadMarker) -> Vec<PreferenceRow> {
        let mut rows = vec![PreferenceRow::section("System integration")];

        if SystemIntegration::is_default_markdown_handler() {
            rows.push(PreferenceRow::note("Markdown files open in Upleft."));
        } else {
            rows.push(PreferenceRow::note(PreferencesForms::default_handler_description()));
            rows.push(PreferenceRow::button("Open Markdown Files with Upleft", move || {
                // `Task { @MainActor in … }`.
                main_async(move || {
                    SystemIntegration::make_default_markdown_handler(
                        move |failure| {
                            if SystemIntegration::is_default_markdown_handler() {
                                PreferencesForms::report("Markdown files now open in Upleft.", None, mtm);
                            } else {
                                let detail = failure.map(|failure| failure.localized_description).unwrap_or_else(|| {
                                    "Finder’s Get Info → Open With → Change All can set it directly.".to_owned()
                                });
                                PreferencesForms::report("Couldn’t change the default app.", Some(&detail), mtm);
                            }
                        },
                        mtm,
                    );
                });
            }));
        }

        if SystemIntegration::command_line_tool_is_bundled() {
            let title = if SystemIntegration::is_command_line_tool_installed() {
                "Reinstall the down Command Line Tool"
            } else {
                "Install the down Command Line Tool"
            };
            rows.push(PreferenceRow::button(title, move || match SystemIntegration::install_command_line_tool() {
                Ok(result) => {
                    if result.linked.is_empty() {
                        PreferencesForms::report(
                            "Nothing was installed.",
                            Some(&format!(
                                "Something else already owns {} in {}.",
                                result.skipped.join(" and "),
                                result.directory.path()
                            )),
                            mtm,
                        );
                        return;
                    }
                    PreferencesForms::report(
                        &format!("Installed {} in {}.", result.linked.join(" and "), result.directory.path()),
                        if result.is_on_path {
                            None
                        } else {
                            Some("That folder isn’t on your PATH — add it to your shell profile to use the command.")
                        },
                        mtm,
                    );
                }
                Err(error) => {
                    PreferencesForms::report(
                        "Couldn’t install the command line tool.",
                        Some(&error.localized_description),
                        mtm,
                    );
                }
            }));
        }

        if SystemIntegration::quick_look_extensions_are_bundled() {
            rows.push(PreferenceRow::button("Re-register Quick Look Previews and Icons", move || {
                SystemIntegration::register_with_system(
                    true,
                    move |enabled| {
                        PreferencesForms::report(
                            if enabled {
                                "Quick Look previews and Finder icons are on."
                            } else {
                                "Quick Look needs one switch from you."
                            },
                            Some(if enabled {
                                "Press space on a Markdown file to try it."
                            } else {
                                "System Settings → General → Login Items & Extensions → Quick Look, then tick Upleft."
                            }),
                            mtm,
                        );
                    },
                    mtm,
                );
            }));
            rows.push(PreferenceRow::button("Open Quick Look Settings", SystemIntegration::open_quick_look_settings));
        } else {
            // A SwiftPM dev build genuinely cannot carry an `.appex`; saying so
            // beats offering a button that can only ever fail.
            rows.push(PreferenceRow::note("Quick Look previews are available in the installed release of Upleft."));
        }

        rows.extend(PreferencesForms::agent_integration(mtm));

        rows
    }

    /// Coding agents rewriting Markdown under the reader is the case this app
    /// exists for, so the hook that makes them hand the file over belongs
    /// beside the other system registrations.
    ///
    /// Like every row above it, this reads live state rather than a stored
    /// preference: the truth is in the agent's own settings file, which the
    /// user may edit by hand or replace entirely.
    pub fn agent_integration(mtm: MainThreadMarker) -> Vec<PreferenceRow> {
        // The hook invokes `down` by absolute path, so without the CLI there
        // is nothing to install and a button would only ever fail.
        if AgentIntegration::executable_path().is_none() {
            return vec![PreferenceRow::note(
                "Install the down command line tool above to let coding agents open Markdown here.",
            )];
        }

        if !AgentIntegration::is_installed() {
            return vec![
                PreferenceRow::note("Let coding agents open Markdown in Upleft as they write it."),
                PreferenceRow::button("Open Agent Edits in Upleft", move || match AgentIntegration::install() {
                    Ok(_) => PreferencesForms::report(
                        "Agent edits now open in Upleft.",
                        Some(&format!("Added to {}.", AgentIntegration::settings_url().path())),
                        mtm,
                    ),
                    Err(error) => PreferencesForms::report(
                        "Couldn’t update the agent settings.",
                        Some(&error.error_description()),
                        mtm,
                    ),
                }),
            ];
        }

        vec![
            PreferenceRow::note("Coding agents open Markdown in Upleft as they write it."),
            PreferenceRow::button("Stop Opening Agent Edits", move || match AgentIntegration::uninstall() {
                Ok(_) => PreferencesForms::report("Agent edits no longer open in Upleft.", None, mtm),
                Err(error) => {
                    PreferencesForms::report("Couldn’t update the agent settings.", Some(&error.error_description()), mtm)
                }
            }),
        ]
    }

    /// Names the app that currently owns Markdown, so the row explains what
    /// it would be changing rather than only what it would be setting.
    fn default_handler_description() -> String {
        let handler = SystemIntegration::claimed_types()
            .into_iter()
            .next()
            .and_then(|kind| NSWorkspace::sharedWorkspace().URLForApplicationToOpenContentType(&kind));
        let Some(handler) = handler else { return "No app is set to open Markdown files.".to_owned() };
        let path = handler.path().map(|path| path.to_string()).unwrap_or_default();
        let name = NSFileManager::defaultManager().displayNameAtPath(&ns(&path));
        format!("Markdown files currently open in {name}.")
    }

    fn report(message: &str, detail: Option<&str>, mtm: MainThreadMarker) {
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&ns(message));
        if let Some(detail) = detail {
            alert.setInformativeText(&ns(detail));
        }
        alert.setAlertStyle(NSAlertStyle::Informational);
        alert.runModal();
    }

    pub fn typography(mtm: MainThreadMarker) -> Vec<PreferenceRow> {
        vec![
            PreferenceRow::section("Body"),
            PreferenceRow::choice(
                "Preset",
                Some("Reading is set in New York, Working in SF Pro Text."),
                BodyPreset::ALL_CASES.iter().map(|preset| preset.title().to_owned()).collect(),
                || {
                    let presets = BodyPreset::ALL_CASES;
                    let current = Preferences::shared().values().typography.preset;
                    ChoiceSelection::Index(presets.iter().position(|preset| *preset == current).unwrap_or(0) as isize)
                },
                |index| {
                    if !(index >= 0 && (index as usize) < BodyPreset::ALL_CASES.len()) {
                        return;
                    }
                    Preferences::shared().update(|values| {
                        values.typography.preset = BodyPreset::ALL_CASES[index as usize];
                    });
                },
            ),
            PreferenceRow::stepper(
                "Size",
                None,
                11.0..=24.0,
                1.0,
                || Preferences::shared().values().typography.body_size,
                |value| Preferences::shared().update(|values| values.typography.body_size = value),
            ),
            PreferenceRow::stepper(
                "Text size adjustment",
                Some("A quick app-wide nudge, also on the View menu."),
                -4.0..=10.0,
                1.0,
                || Preferences::shared().values().text_size_adjustment,
                |value| Preferences::shared().update(|values| values.text_size_adjustment = value),
            ),
            PreferenceRow::choice(
                "Type scale",
                Some("One ratio sets every heading size."),
                vec!["1.200".to_owned(), "1.250".to_owned(), "1.333".to_owned()],
                || {
                    let ratio = Preferences::shared().values().typography.scale_ratio;
                    ChoiceSelection::Index(if ratio < 1.22 {
                        0
                    } else if ratio < 1.29 {
                        1
                    } else {
                        2
                    })
                },
                |index| {
                    let ratios: [f64; 3] = [1.2, 1.25, 1.333];
                    if !(index >= 0 && (index as usize) < ratios.len()) {
                        return;
                    }
                    Preferences::shared().update(|values| {
                        values.typography.scale_ratio = ratios[index as usize];
                    });
                },
            ),
            PreferenceRow::stepper(
                "Line height",
                None,
                1.2..=2.0,
                0.05,
                || Preferences::shared().values().typography.line_height_multiple,
                |value| Preferences::shared().update(|values| values.typography.line_height_multiple = value),
            ),
            PreferenceRow::stepper(
                "Measure (characters)",
                Some(
                    "How much text fits on a line before it wraps. Around 68 to 72 reads best; full-width text is the most common thing Markdown viewers get wrong.",
                ),
                60.0..=80.0,
                1.0,
                || Preferences::shared().values().typography.measure_characters,
                |value| Preferences::shared().update(|values| values.typography.measure_characters = value),
            ),
            PreferenceRow::section("Code"),
            PreferenceRow::choice(
                "Monospace family",
                None,
                sorted_font_families(mtm),
                move || {
                    let fonts = sorted_font_families(mtm);
                    let current = Preferences::shared().values().typography.mono_family;
                    first_index_of(&fonts, &current)
                        .map(|index| ChoiceSelection::Index(index as isize))
                        .unwrap_or(ChoiceSelection::Missing(current))
                },
                move |index| {
                    let fonts = sorted_font_families(mtm);
                    if !(index >= 0 && (index as usize) < fonts.len()) {
                        return;
                    }
                    let family = fonts[index as usize].clone();
                    Preferences::shared().update(|values| values.typography.mono_family = family);
                },
            ),
            PreferenceRow::toggle(
                "Ligatures",
                None,
                || Preferences::shared().values().typography.mono_ligatures,
                |value| Preferences::shared().update(|values| values.typography.mono_ligatures = value),
            ),
            PreferenceRow::section("Detail"),
            PreferenceRow::toggle(
                "Hanging punctuation and optical margins",
                Some("Makes text look set rather than merely laid out."),
                || Preferences::shared().values().typography.optical_margins,
                |value| Preferences::shared().update(|values| values.typography.optical_margins = value),
            ),
            PreferenceRow::stepper(
                "Math scale",
                Some("Sizes formulas to sit evenly against the body text."),
                0.8..=1.3,
                0.05,
                || Preferences::shared().values().typography.math_scale,
                |value| Preferences::shared().update(|values| values.typography.math_scale = value),
            ),
        ]
    }

    pub fn appearance(_mtm: MainThreadMarker) -> Vec<PreferenceRow> {
        // Both lists are read here, on every rebuild, and again inside the
        // setters — an imported or reloaded theme must not leave the popup
        // indices pointing at names that have moved.
        //
        // Swift reads `ThemeStore.shared.themes` twice on the main actor, where
        // a reload also lands, so both lists always come from one theme set.
        // The Rust store can land a hot reload from its background queue, so
        // the port reads the set once to keep that guarantee.
        let themes = ThemeStore::shared().themes();
        let light: Rc<Vec<String>> = Rc::new(
            themes
                .iter()
                .filter(|theme| theme.appearance != ThemeAppearance::Dark)
                .map(|theme| theme.name.clone())
                .collect(),
        );
        let dark: Rc<Vec<String>> = Rc::new(
            themes
                .iter()
                .filter(|theme| theme.appearance != ThemeAppearance::Light)
                .map(|theme| theme.name.clone())
                .collect(),
        );
        vec![
            PreferenceRow::choice(
                "Quick Look appearance",
                Some("Preview files in System appearance by default, or keep a fixed Light or Dark surface."),
                PreviewAppearance::ALL_CASES.iter().map(|appearance| appearance.title().to_owned()).collect(),
                || {
                    let current = Preferences::shared().values().preview_appearance;
                    ChoiceSelection::Index(
                        PreviewAppearance::ALL_CASES.iter().position(|appearance| *appearance == current).unwrap_or(0)
                            as isize,
                    )
                },
                |index| {
                    if !(index >= 0 && (index as usize) < PreviewAppearance::ALL_CASES.len()) {
                        return;
                    }
                    Preferences::shared().update(|values| {
                        values.preview_appearance = PreviewAppearance::ALL_CASES[index as usize];
                    });
                },
            ),
            PreferenceRow::section("Themes"),
            PreferenceRow::choice(
                "Light theme",
                Some("Used when macOS is in Light appearance."),
                light.as_ref().clone(),
                {
                    let light = light.clone();
                    move || {
                        let name = Preferences::shared().values().theme_name;
                        first_index_of(&light, &name)
                            .map(|index| ChoiceSelection::Index(index as isize))
                            .unwrap_or(ChoiceSelection::Missing(name))
                    }
                },
                {
                    let light = light.clone();
                    move |index| {
                        if !(index >= 0 && (index as usize) < light.len()) {
                            return;
                        }
                        let name = light[index as usize].clone();
                        Preferences::shared().update(|values| values.theme_name = name);
                    }
                },
            ),
            PreferenceRow::choice(
                "Dark theme",
                Some("Used when macOS is in Dark appearance."),
                dark.as_ref().clone(),
                {
                    let dark = dark.clone();
                    move || {
                        let name = Preferences::shared().values().dark_theme_name;
                        first_index_of(&dark, &name)
                            .map(|index| ChoiceSelection::Index(index as isize))
                            .unwrap_or(ChoiceSelection::Missing(name))
                    }
                },
                {
                    let dark = dark.clone();
                    move |index| {
                        if !(index >= 0 && (index as usize) < dark.len()) {
                            return;
                        }
                        let name = dark[index as usize].clone();
                        Preferences::shared().update(|values| values.dark_theme_name = name);
                    }
                },
            ),
            PreferenceRow::toggle(
                "Follow system appearance",
                Some(
                    "Switch between the light and dark themes as macOS does. Turn this off to keep the light theme in both.",
                ),
                || Preferences::shared().values().follows_system_appearance,
                |value| Preferences::shared().update(|values| values.follows_system_appearance = value),
            ),
            PreferenceRow::ThemePreview,
            PreferenceRow::note(
                "Themes live in the Upleft folder in Application Support. Import Theme and Reload Themes are on the View menu.",
            ),
        ]
    }

    pub fn editor(_mtm: MainThreadMarker) -> Vec<PreferenceRow> {
        vec![
            PreferenceRow::section("Saving"),
            PreferenceRow::toggle(
                "Autosave while editing",
                Some(
                    "Writes changes to disk as you type. Turn this off when an agent or external tool is also writing the same file — the default is off for that reason.",
                ),
                || Preferences::shared().values().autosave_enabled,
                |value| Preferences::shared().update(|values| values.autosave_enabled = value),
            ),
            PreferenceRow::section("Typing"),
            PreferenceRow::toggle(
                "Typewriter scrolling",
                None,
                || Preferences::shared().values().typewriter_scrolling,
                |value| Preferences::shared().update(|values| values.typewriter_scrolling = value),
            ),
            PreferenceRow::toggle(
                "Use typographic substitutions",
                Some("Convert straight quotes and dashes while typing. Off keeps Markdown source exact."),
                || Preferences::shared().values().typographic_substitution,
                |value| Preferences::shared().update(|values| values.typographic_substitution = value),
            ),
            PreferenceRow::section("Display"),
            PreferenceRow::toggle(
                "Show spaces and tabs",
                Some("Draw whitespace markers in the visible part of the document."),
                || Preferences::shared().values().show_invisibles,
                |value| Preferences::shared().update(|values| values.show_invisibles = value),
            ),
            PreferenceRow::toggle(
                "Reflow wrapped paragraphs",
                Some("Join source-wrapped prose visually without changing a byte of the file."),
                || Preferences::shared().values().reflow_hard_wrapped_paragraphs,
                |value| Preferences::shared().update(|values| values.reflow_hard_wrapped_paragraphs = value),
            ),
            PreferenceRow::toggle(
                "Reveal syntax at every cursor",
                Some("Show inline markers for extra insertion cursors too."),
                || Preferences::shared().values().reveal_markers_at_all_cursors,
                |value| Preferences::shared().update(|values| values.reveal_markers_at_all_cursors = value),
            ),
            PreferenceRow::toggle(
                "Focus mode on open",
                Some("Hide the surrounding chrome and panels for a single-column writing surface."),
                || Preferences::shared().values().focus_mode,
                |value| Preferences::shared().update(|values| values.focus_mode = value),
            ),
            PreferenceRow::choice(
                "Default mode",
                Some("How a document looks when you open it."),
                RenderMode::USER_FACING_MODES.iter().map(|mode| mode.title().to_owned()).collect(),
                || {
                    let current = Preferences::shared().values().default_mode;
                    ChoiceSelection::Index(
                        RenderMode::USER_FACING_MODES.iter().position(|mode| *mode == current).unwrap_or(0) as isize,
                    )
                },
                |index| {
                    if !(index >= 0 && (index as usize) < RenderMode::USER_FACING_MODES.len()) {
                        return;
                    }
                    Preferences::shared().update(|values| {
                        values.default_mode = RenderMode::USER_FACING_MODES[index as usize];
                    });
                },
            ),
            PreferenceRow::section("Performance"),
            PreferenceRow::stepper(
                "Large-file threshold",
                Some(
                    "Megabytes. Above this, Upleft estimates the layout from line counts instead of laying out the whole file before showing you anything.",
                ),
                1.0..=1024.0,
                1.0,
                || Preferences::shared().values().large_file_threshold_megabytes as f64,
                |value| Preferences::shared().update(|values| values.large_file_threshold_megabytes = value as i64),
            ),
        ]
    }

    pub fn updates(mtm: MainThreadMarker) -> Vec<PreferenceRow> {
        let coordinator = UpdateCoordinator::shared(mtm);
        if !coordinator.is_update_configuration_present() {
            return vec![PreferenceRow::note(
                "This build carries no update configuration, so automatic updates are off. Install a release build to see update settings here.",
            )];
        }
        vec![
            PreferenceRow::section("Updates"),
            PreferenceRow::note(
                "Upleft checks a signed update feed over HTTPS. A downloaded update installs when you quit, so an editing session is never interrupted.",
            ),
            PreferenceRow::toggle(
                "Automatically check for updates",
                Some(
                    "Checks about every hour, and watches the release feed while the app is open. The menu command still works while this is off.",
                ),
                move || UpdateCoordinator::shared(mtm).automatically_checks_for_updates(),
                move |value| UpdateCoordinator::shared(mtm).set_automatically_checks_for_updates(value),
            ),
            PreferenceRow::toggle(
                "Automatically download and install updates",
                Some(
                    "Downloads in the background and installs on the next normal quit. Turn it off to review each update first.",
                ),
                move || UpdateCoordinator::shared(mtm).automatically_downloads_updates(),
                move |value| UpdateCoordinator::shared(mtm).set_automatically_downloads_updates(value),
            ),
            PreferenceRow::button("Check Now", move || UpdateCoordinator::shared(mtm).check_for_updates()),
            PreferenceRow::section("Status"),
            PreferenceRow::note(UpdateCoordinator::shared(mtm).status_line()),
        ]
    }

    pub fn history(_mtm: MainThreadMarker) -> Vec<PreferenceRow> {
        let timeline =
            PreferencesForms::shortcut(Command::VersionTimeline).map(|shortcut| format!(" ({shortcut})")).unwrap_or_default();
        let used = NSByteCountFormatter::stringFromByteCount_countStyle(
            SnapshotStore::shared().total_bytes() as i64,
            NSByteCountFormatterCountStyle::File,
        )
        .to_string();
        vec![
            PreferenceRow::section("Version history"),
            PreferenceRow::note(format!(
                "Upleft keeps a copy of the document every time something else writes to it, stored locally and deduplicated, so nothing is kept twice. That is where Version Timeline{timeline} gets the versions it shows you."
            )),
            PreferenceRow::stepper(
                "Keep versions for",
                Some("Days."),
                1.0..=365.0,
                1.0,
                || Preferences::shared().values().history_maximum_days as f64,
                |value| Preferences::shared().update(|values| values.history_maximum_days = value as i64),
            ),
            PreferenceRow::stepper(
                "Maximum size",
                Some("Megabytes."),
                50.0..=5000.0,
                50.0,
                || Preferences::shared().values().history_maximum_megabytes as f64,
                |value| Preferences::shared().update(|values| values.history_maximum_megabytes = value as i64),
            ),
            PreferenceRow::note(format!("Currently using {used}.")),
        ]
    }
}
