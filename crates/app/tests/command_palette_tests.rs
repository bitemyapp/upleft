//! Port of `Tests/DownrightAppTests/CommandPaletteTests.swift`.

use std::rc::Rc;

use objc2::AnyThread;
use objc2_foundation::{NSString, NSUserDefaults};
use upleft_app::support::command_palette_model::{
    CommandPaletteEntry, CommandPaletteModel, CommandPaletteRecentStore, UserDefaultsCommandPaletteRecentStore,
};
use upleft_app::support::commands::{Command, CommandScope};
use upleft_app::support::quick_open_providers::{
    CurrentDocumentQuickOpenProvider, QuickOpenAction, QuickOpenFilter, QuickOpenProvider, QuickOpenProviderKind,
    QuickOpenQuery, QuickOpenResult, WorkspaceQuickOpenProvider,
};
use upleft_core::parser::MarkdownParser;
use upleft_foundation::url::FileUrl;
use upleft_swift_text::NSRange;

const ALL_SCOPES: [CommandScope; 3] = [CommandScope::Read, CommandScope::Live, CommandScope::Source];

fn entries() -> Vec<CommandPaletteEntry> {
    vec![
        CommandPaletteEntry {
            command: Command::Find,
            title: "Find…".into(),
            synonyms: vec!["search".into(), "locate".into()],
            binding: Some("⌘F".into()),
            scopes: ALL_SCOPES.to_vec(),
        },
        CommandPaletteEntry {
            command: Command::FrontMatterEditor,
            title: "Front Matter…".into(),
            synonyms: vec!["metadata".into(), "yaml".into()],
            binding: None,
            scopes: ALL_SCOPES.to_vec(),
        },
        CommandPaletteEntry {
            command: Command::TableEditor,
            title: "Edit Table…".into(),
            synonyms: vec!["grid".into(), "cells".into()],
            binding: Some("⌥T".into()),
            scopes: ALL_SCOPES.to_vec(),
        },
    ]
}

fn commands_of(model: &CommandPaletteModel) -> Vec<Command> {
    model.results().iter().map(|entry| entry.command).collect()
}

#[test]
fn title_and_synonym_search_match_all_terms() {
    let mut model = CommandPaletteModel::new(entries(), vec![], vec![]);
    model.update_query("yaml");
    assert_eq!(commands_of(&model), vec![Command::FrontMatterEditor]);

    model.update_query("edit cells");
    assert_eq!(commands_of(&model), vec![Command::TableEditor]);
}

#[test]
fn fuzzy_search_ranks_prefix_before_loose_match() {
    let mut model = CommandPaletteModel::new(entries(), vec![], vec![]);
    model.update_query("find");
    assert_eq!(commands_of(&model).first(), Some(&Command::Find));
    model.update_query("fm");
    assert_eq!(commands_of(&model).first(), Some(&Command::FrontMatterEditor));
}

#[test]
fn recent_commands_lead_empty_query_and_tie_break_search() {
    let mut model = CommandPaletteModel::new(entries(), vec![Command::TableEditor, Command::Find], vec![]);
    assert_eq!(commands_of(&model), vec![Command::TableEditor, Command::Find, Command::FrontMatterEditor]);
    model.update_query("e");
    assert_eq!(commands_of(&model).first(), Some(&Command::TableEditor));
    model.record(Command::FrontMatterEditor);
    model.update_query("");
    assert_eq!(commands_of(&model).first(), Some(&Command::FrontMatterEditor));
}

#[test]
fn selection_wraps_and_query_resets_selection() {
    let mut model = CommandPaletteModel::new(entries(), vec![], vec![]);
    model.move_selection(-1);
    assert_eq!(model.selected_index(), 2);
    model.move_selection(1);
    assert_eq!(model.selected_index(), 0);
    model.select(2);
    model.update_query("find");
    assert_eq!(model.selected_index(), 0);
    assert_eq!(model.selected_entry().map(|entry| entry.command), Some(Command::Find));
}

#[test]
fn entry_exposes_binding_and_scope_for_accessible_rows() {
    let model = CommandPaletteModel::new(entries(), vec![], vec![]);
    let find = model.entries().iter().find(|entry| entry.command == Command::Find);
    assert_eq!(find.and_then(|entry| entry.binding.as_deref()), Some("⌘F"));
    assert_eq!(find.map(|entry| entry.scope_label()), Some("Document / Source".to_owned()));
    assert_eq!(
        model.entries().iter().find(|entry| entry.command == Command::FrontMatterEditor).and_then(|entry| entry.binding.clone()),
        None
    );
}

/// `UserDefaults` ignores `CFFIXED_USER_HOME`: a suite's plist lands in the
/// real home, so the test removes the domain and the file.
fn real_preferences_file(suite: &str) -> Option<String> {
    let entry = unsafe { libc::getpwuid(libc::getuid()) };
    if entry.is_null() {
        return None;
    }
    let home = unsafe { std::ffi::CStr::from_ptr((*entry).pw_dir) }.to_string_lossy().into_owned();
    Some(format!("{home}/Library/Preferences/{suite}.plist"))
}

#[test]
fn recent_store_deduplicates_and_limits_history() {
    let suite = format!("CommandPaletteTests.{}", objc2_foundation::NSUUID::new().UUIDString());
    let defaults = NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(&NSString::from_str(&suite))).unwrap();
    let store = UserDefaultsCommandPaletteRecentStore::new(defaults.clone(), UserDefaultsCommandPaletteRecentStore::DEFAULT_KEY, 2);
    store.record(Command::Find);
    store.record(Command::TableEditor);
    store.record(Command::Find);
    let recent = store.recent_commands();
    defaults.removePersistentDomainForName(&NSString::from_str(&suite));
    if let Some(path) = real_preferences_file(&suite) {
        let _ = std::fs::remove_file(path);
    }
    assert_eq!(recent, vec![Command::Find, Command::TableEditor]);
}

#[test]
fn query_prefixes_select_quick_open_provider() {
    assert_eq!(QuickOpenQuery::new("> format").filter, QuickOpenFilter::Commands);
    assert_eq!(QuickOpenQuery::new("@head").filter, QuickOpenFilter::Symbols);
    assert_eq!(QuickOpenQuery::new("#task fix").filter, QuickOpenFilter::Tasks);
    assert_eq!(QuickOpenQuery::new("file: notes").filter, QuickOpenFilter::Files);
    assert_eq!(QuickOpenQuery::new("asset: logo").filter, QuickOpenFilter::Assets);
    assert_eq!(QuickOpenQuery::new("link: docs").filter, QuickOpenFilter::Links);
}

#[test]
fn current_document_provider_returns_headings_tasks_links_and_assets() {
    let text = "# Plan\n\n- [ ] Write [docs](https://example.com).\n![logo](images/logo.png)";
    let provider = CurrentDocumentQuickOpenProvider::new(MarkdownParser::parse(text));
    let has = |query: &str, kind: QuickOpenProviderKind| {
        provider.results(&QuickOpenQuery::new(query)).iter().any(|result| result.kind == kind)
    };
    assert!(has("@plan", QuickOpenProviderKind::Heading));
    assert!(has("#task write", QuickOpenProviderKind::Task));
    assert!(has("link: docs", QuickOpenProviderKind::Link));
    assert!(has("asset: logo", QuickOpenProviderKind::Asset));
}

#[test]
fn injected_workspace_files_and_symbols_join_one_ranked_list() {
    let url = FileUrl::from_path("/tmp/notes.md");
    let symbol = QuickOpenResult::new(
        "symbol:1",
        QuickOpenProviderKind::Symbol,
        "Project Symbol",
        QuickOpenAction::Select(NSRange::new(3, 2)),
    );
    let provider: Rc<dyn QuickOpenProvider> =
        Rc::new(WorkspaceQuickOpenProvider { files: vec![url], symbols: vec![symbol] });
    let mut model = CommandPaletteModel::new(entries(), vec![], vec![provider]);
    model.update_query("file: notes");
    assert_eq!(model.selected_result().map(|result| result.kind), Some(QuickOpenProviderKind::WorkspaceFile));
    model.update_query("@project");
    assert_eq!(model.quick_results().first().map(|result| result.id.clone()), Some("symbol:1".to_owned()));
}

#[test]
fn quick_open_command_action_preserves_command_recents() {
    let mut model = CommandPaletteModel::new(entries(), vec![], vec![]);
    model.update_query("> find");
    assert_eq!(
        model.selected_result().map(|result| result.action),
        Some(QuickOpenAction::Command(Command::Find)),
        "command prefix must return a command action"
    );
    model.record(Command::Find);
    assert_eq!(model.recent_commands().first(), Some(&Command::Find));
}
