//! Port of `Tests/DownrightAppTests/CommandTableTests.swift`.
//!
//! Skipped, because they build the menu bar (`MainMenu.build()`, an AppKit
//! `NSMenu` tree that arrives with the UI port): everyCommandAppearsInExactlyOneMenu,
//! everyCommandSitsInTheMenuItAdvertises, applicationWindowAndHelpMenusComeFromTheTable,
//! checkForUpdatesIsAnOrdinaryCommandItem, standardMenuItemsExistWithTheirMacOSChords,
//! supportMenuItemOpensGitHubSponsors, validationOnlyClaimsItemsThatCarryACommand.

use std::collections::HashMap;
use std::rc::Rc;

use upleft_app::support::command_palette_model::{CommandPaletteEntry, CommandPaletteModel};
use upleft_app::support::commands::{Command, CommandContext, CommandScope, KeyBinding, ModifierFlags};
use upleft_app::support::keybindings::{KeybindingDefaults, KeybindingLoad, Stored};
use upleft_app::support::quick_open_providers::{
    CurrentDocumentQuickOpenProvider, QuickOpenAction, QuickOpenFilter, QuickOpenProvider, QuickOpenProviderKind,
    QuickOpenQuery, RecentFilesQuickOpenProvider,
};
use upleft_core::parser::MarkdownParser;
use upleft_foundation::json_decoder;
use upleft_foundation::json_encoder::{self, OutputFormatting};
use upleft_foundation::url::FileUrl;

const CMD: ModifierFlags = ModifierFlags::COMMAND;
const SHIFT: ModifierFlags = ModifierFlags::SHIFT;
const OPT: ModifierFlags = ModifierFlags::OPTION;
const CTRL: ModifierFlags = ModifierFlags::CONTROL;

#[test]
fn contents_outline_keeps_one_canonical_command_and_palette_names() {
    assert_eq!(Command::DocumentLens.title(), "Contents / Outline");
    let model = CommandPaletteModel::with_commands(&[Command::DocumentLens], |_| Vec::new(), vec![], vec![]);
    let commands = |model: &CommandPaletteModel| model.results().iter().map(|e| e.command).collect::<Vec<_>>();
    assert_eq!(commands(&model), vec![Command::DocumentLens]);
    let mut outline = model.clone();
    outline.update_query("outline");
    assert_eq!(commands(&outline), vec![Command::DocumentLens]);
    let mut contents = model.clone();
    contents.update_query("contents");
    assert_eq!(commands(&contents), vec![Command::DocumentLens]);
}

// MARK: - Key binding serialisation (the file-destroying bug)

fn every_modifier_combination() -> Vec<ModifierFlags> {
    let parts = [CMD, SHIFT, OPT, CTRL];
    (0..16)
        .map(|mask: u32| {
            parts.iter().enumerate().fold(ModifierFlags::EMPTY, |flags, (offset, part)| {
                if mask & (1 << offset) != 0 { flags.union(*part) } else { flags }
            })
        })
        .collect()
}

const EVERY_KEY: [&str; 16] =
    ["+", "-", "=", "left", "right", "up", "down", "space", "tab", "return", "backslash", "[", "]", ",", "0", "a"];

#[test]
fn every_key_and_modifier_combination_round_trips_through_json() {
    for key in EVERY_KEY {
        for modifiers in every_modifier_combination() {
            let binding = KeyBinding::new(key, modifiers);
            let data = json_encoder::encode(&binding.encode(), OutputFormatting::DEFAULT);
            let decoded = json_decoder::parse(&data).and_then(|value| KeyBinding::decode(&value)).unwrap();
            assert_eq!(decoded, binding, "{} did not survive JSON", binding.serialized());
            assert_eq!(decoded.key, key);
        }
    }
}

/// `KeyBinding("+", .command).serialized == "cmd++"`, which the old parser
/// read back as "no key at all".
#[test]
fn legacy_string_form_round_trips_including_plus() {
    for key in EVERY_KEY {
        for modifiers in every_modifier_combination() {
            let binding = KeyBinding::new(key, modifiers);
            let parsed = KeyBinding::parsing(&binding.serialized())
                .unwrap_or_else(|| panic!("failed to parse {}", binding.serialized()));
            assert_eq!(parsed, binding);
        }
    }
    assert_eq!(KeyBinding::parsing("cmd++"), Some(KeyBinding::new("+", CMD)));
    assert_eq!(KeyBinding::parsing("+"), Some(KeyBinding::new("+", ModifierFlags::EMPTY)));
    assert_eq!(KeyBinding::parsing("shift+cmd++"), Some(KeyBinding::new("+", CMD.union(SHIFT))));
}

/// Files written by older builds hold a plain string.
#[test]
fn legacy_string_bindings_still_decode() {
    let data = br#"{"overrides":{"save":["opt+cmd+s"]},"vimKeysEnabled":true}"#;
    let KeybindingLoad::Loaded { vim_keys_enabled, overrides } = KeybindingLoad::decode(data) else {
        panic!("a legacy string file must still load");
    };
    assert!(vim_keys_enabled);
    assert_eq!(overrides.get(&Command::Save), Some(&vec![KeyBinding::new("s", CMD.union(OPT))]));
}

/// The whole point: recording ⌘⇧+ must not cost the user every other
/// override and their vim setting.
#[test]
fn a_plus_override_survives_a_write_and_read_cycle() {
    let stored = Stored {
        vim_keys_enabled: true,
        overrides: vec![
            (Command::IncreaseTextSize.raw_value().to_owned(), vec![KeyBinding::new("+", CMD.union(SHIFT))]),
            (Command::Save.raw_value().to_owned(), vec![KeyBinding::new("s", CMD)]),
        ],
    };
    let data = json_encoder::encode(&stored.encode(), OutputFormatting::DEFAULT);
    let KeybindingLoad::Loaded { vim_keys_enabled, overrides } = KeybindingLoad::decode(&data) else {
        panic!("a `+` binding must not make the file undecodable");
    };
    assert!(vim_keys_enabled);
    assert_eq!(overrides.get(&Command::IncreaseTextSize), Some(&vec![KeyBinding::new("+", CMD.union(SHIFT))]));
    assert_eq!(overrides.get(&Command::Save), Some(&vec![KeyBinding::new("s", CMD)]));
}

#[test]
fn missing_file_is_absent_and_bad_file_is_unreadable() {
    let directory = std::env::temp_dir()
        .join(format!("CommandTableTests-{}", objc2_foundation::NSUUID::new().UUIDString()));
    std::fs::create_dir_all(&directory).unwrap();
    let missing = directory.join("keybindings.json");
    let url = FileUrl::from_path(&missing.to_string_lossy());
    let absent = matches!(KeybindingLoad::read(&url), KeybindingLoad::Absent);

    std::fs::write(&missing, "{ this is not json").unwrap();
    let corrupt = matches!(KeybindingLoad::read(&url), KeybindingLoad::Unreadable(_));

    // An unknown modifier name is corruption too: dropping it would change
    // what the user's shortcut does without telling them.
    std::fs::write(&missing, r#"{"overrides":{"save":[{"key":"s","modifiers":["hyper"]}]},"vimKeysEnabled":false}"#)
        .unwrap();
    let unknown_modifier = matches!(KeybindingLoad::read(&url), KeybindingLoad::Unreadable(_));
    let _ = std::fs::remove_dir_all(&directory);

    assert!(absent, "a missing file is the normal first run, not a failure");
    assert!(corrupt, "a corrupt file must be reported, not silently replaced by defaults");
    assert!(unknown_modifier, "an unknown modifier must not decode");
}

// MARK: - Preconditions drive menu validation

#[test]
fn menu_validation_follows_preconditions() {
    let none = CommandContext::application_only(false);
    assert!(Command::Open.is_enabled(&none));
    assert!(Command::Preferences.is_enabled(&none));
    assert!(!Command::Save.is_enabled(&none));
    assert!(!Command::PrintDocument.is_enabled(&none));
    assert!(!Command::ExportPdf.is_enabled(&none));
    assert!(!Command::CheckForUpdates.is_enabled(&none));
    assert!(Command::CheckForUpdates.is_enabled(&CommandContext::application_only(true)));

    let unsaved = CommandContext { has_document: true, ..CommandContext::default() };
    assert!(Command::Save.is_enabled(&unsaved));
    assert!(Command::PrintDocument.is_enabled(&unsaved));
    assert!(!Command::RevealInFinder.is_enabled(&unsaved));
    assert!(!Command::VersionTimeline.is_enabled(&unsaved));
    assert!(!Command::ExportSelectionAsImage.is_enabled(&unsaved));

    let saved =
        CommandContext { has_document: true, document_has_file: true, has_selection: true, ..CommandContext::default() };
    assert!(Command::RevealInFinder.is_enabled(&saved));
    assert!(Command::VersionTimeline.is_enabled(&saved));
    assert!(Command::ExportSelectionAsImage.is_enabled(&saved));
    assert!(Command::UseSelectionForFind.is_enabled(&saved));
}

// MARK: - Shortcuts

/// The reverse index keeps the first claim and drops the rest silently, so a
/// collision inside one scope is a shortcut that quietly stops working.
#[test]
fn default_bindings_do_not_collide_within_a_scope() {
    let resolved = KeybindingDefaults::table();
    for scope in CommandScope::ALL_CASES {
        let mut claimed: HashMap<KeyBinding, Command> = HashMap::new();
        for command in Command::ALL_CASES.into_iter().filter(|command| command.scopes().contains(&scope)) {
            for binding in resolved.get(&command).cloned().unwrap_or_default() {
                if let Some(owner) = claimed.get(&binding) {
                    panic!(
                        "{} is claimed by {} and {} in .{}",
                        binding.display_string(),
                        owner.raw_value(),
                        command.raw_value(),
                        scope.raw_value()
                    );
                }
                claimed.insert(binding, command);
            }
        }
    }
}

/// Chords macOS reserves, which the app used to take.
#[test]
fn mac_os_convention_chords_are_left_alone() {
    let reserved = [
        (KeyBinding::new("f", CTRL.union(CMD)), "Enter Full Screen"),
        (KeyBinding::new("p", CMD.union(SHIFT)), "Page Setup"),
        (KeyBinding::new("v", CMD.union(SHIFT)), "Paste and Match Style"),
        (KeyBinding::new("t", CMD), "New Tab"),
        (KeyBinding::new("t", CMD.union(SHIFT)), "Reopen Closed Tab"),
        (KeyBinding::new("d", CMD), "Duplicate"),
        (KeyBinding::new("up", OPT), "moveParagraphBackward:"),
        (KeyBinding::new("down", OPT), "moveParagraphForward:"),
    ];
    let all: Vec<KeyBinding> = KeybindingDefaults::table().values().flatten().cloned().collect();
    for (binding, owner) in reserved {
        assert!(!all.contains(&binding), "{} belongs to {owner}", binding.display_string());
    }
    // ⌘0 is Actual Size on macOS.
    assert_eq!(KeybindingDefaults::table().get(&Command::ResetTextSize), Some(&vec![KeyBinding::new("0", CMD)]));
}

/// Every command the user is likely to reach for daily has a keyboard path.
#[test]
fn high_frequency_actions_have_bindings() {
    for command in [
        Command::ToggleTaskAtCaret,
        Command::IndentList,
        Command::OutdentList,
        Command::NextHeading,
        Command::PreviousHeading,
        Command::ZoomLevel1,
        Command::ZoomLevel5,
        Command::ZoomIn,
        Command::ZoomOut,
    ] {
        assert!(
            KeybindingDefaults::table().get(&command).is_some_and(|bindings| !bindings.is_empty()),
            "{} has no binding",
            command.raw_value()
        );
    }
    // Heading jumps and structural zoom must work with a caret in the
    // document, not only in Read mode.
    for command in [Command::NextHeading, Command::PreviousHeading, Command::ZoomLevel3] {
        assert!(command.scopes().contains(&CommandScope::Live));
        let primary = KeybindingDefaults::table().get(&command).and_then(|bindings| bindings.first());
        assert_eq!(primary.map(|binding| binding.modifiers.contains(CMD)), Some(true));
    }
}

// MARK: - Palette ranking

fn palette_model(providers: Vec<Rc<dyn QuickOpenProvider>>) -> CommandPaletteModel {
    CommandPaletteModel::with_commands(&Command::ALL_CASES, |_| Vec::new(), vec![], providers)
}

fn commands(model: &CommandPaletteModel) -> Vec<Command> {
    model
        .quick_results()
        .iter()
        .filter_map(|result| match result.action {
            QuickOpenAction::Command(command) => Some(command),
            _ => None,
        })
        .collect()
}

/// Scoring used to run over `"\(title) \(subtitle)"`, and every command's
/// subtitle ends in "Document", so `doc` matched essentially everything.
#[test]
fn quick_results_do_not_match_the_rendered_subtitle() {
    let mut model = palette_model(vec![]);
    model.update_query("doc");
    let commands = commands(&model);
    assert!(commands.len() < Command::ALL_CASES.len() / 4);
    assert!(!commands.contains(&Command::Save));
    assert!(!commands.contains(&Command::PrintDocument));
    assert!(commands.contains(&Command::DocumentLens));
    assert!(commands.contains(&Command::DocumentHealth));
}

/// Results carry the score the matcher computed, so the list is ordered by
/// the ranking that actually ran.
#[test]
fn quick_results_carry_their_score_and_stay_sorted() {
    let mut model = palette_model(vec![]);
    model.update_query("timeline");
    let scores: Vec<isize> = model.quick_results().iter().map(|result| result.score).collect();
    assert!(scores.first().copied().unwrap_or(0) > 0);
    let mut sorted = scores.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(scores, sorted);
    let found: std::collections::HashSet<Command> = commands(&model).into_iter().collect();
    assert_eq!(found, [Command::VersionTimeline].into_iter().collect());
}

/// Recency was computed in one code path and dropped in the one that ran.
#[test]
fn recency_breaks_ties_in_the_list_that_is_actually_shown() {
    // Same matched prefix, so the two entries score identically and only the
    // tie-break can separate them.
    let entries = vec![
        CommandPaletteEntry {
            command: Command::DocumentLens,
            title: "Outline A".into(),
            synonyms: vec![],
            binding: None,
            scopes: vec![CommandScope::Live],
        },
        CommandPaletteEntry {
            command: Command::TaskPanel,
            title: "Outline B".into(),
            synonyms: vec![],
            binding: None,
            scopes: vec![CommandScope::Live],
        },
    ];
    let mut model = CommandPaletteModel::new(entries, vec![], vec![]);
    model.update_query("outline");
    let results = model.quick_results();
    assert!(results.iter().all(|result| result.score == results[0].score));
    assert_eq!(commands(&model).first(), Some(&Command::DocumentLens));

    model.record(Command::TaskPanel);
    assert_eq!(commands(&model).first(), Some(&Command::TaskPanel));
}

/// `#` had no producer, so `.headings` was unreachable and the provider's
/// handling of it was dead code.
#[test]
fn hash_prefix_filters_headings() {
    assert_eq!(QuickOpenQuery::new("#intro").filter, QuickOpenFilter::Headings);
    assert_eq!(QuickOpenQuery::new("#intro").terms, "intro");
    assert_eq!(QuickOpenQuery::new("#task fix").filter, QuickOpenFilter::Tasks);
    assert_eq!(QuickOpenQuery::new("#tasks").filter, QuickOpenFilter::Tasks);

    let document = MarkdownParser::parse("# Intro\n\n- [ ] ship it\n");
    let provider = CurrentDocumentQuickOpenProvider::new(document);
    let headings = provider.results(&QuickOpenQuery::new("#intro"));
    assert!(headings.iter().all(|result| result.kind == QuickOpenProviderKind::Heading));
    assert!(headings.iter().any(|result| result.title == "Intro"));
}

#[test]
fn provider_results_match_search_text_but_not_subtitle() {
    let url = FileUrl::from_path("/tmp/notes/design.md");
    let mut model = palette_model(vec![Rc::new(RecentFilesQuickOpenProvider { files: vec![url.clone()] })]);
    model.update_query("file: notes");
    assert!(model.quick_results().iter().any(|result| result.action == QuickOpenAction::Open(url.clone())));
}
