//! Rust side of the `palette` suite; mirrors
//! `oracle/app/Sources/downright-app-oracle/PaletteDump.swift` section for
//! section: the command table, key bindings and the keybindings file, the
//! palette's ranking, Quick Open, the fuzzy matcher, the recent-commands
//! store, the welcome tour and the integration policies.
//!
//! `store` uses the process-wide `KeybindingStore::shared()`, pointed at a
//! fresh temporary support directory through `DOWNRIGHT_SUPPORT_DIRECTORY`
//! before its first use, and flushes its queued writes before every read of
//! the file. `recentStore` uses a unique `UserDefaults` suite, removed
//! afterwards. No field depends on the clock.

use std::rc::Rc;
use std::sync::{Arc, Mutex};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::AnyThread;
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType};
use objc2_foundation::{
    NSAttributedStringKey, NSDictionary, NSNumber, NSPoint, NSRange as FoundationRange, NSString, NSURL, NSUserDefaults,
};
use serde_json::Value;
use upleft_app::integrations::native_integration::NativeIntegrationPolicy;
use upleft_app::panels::fuzzy_matcher::{FuzzyMatcher, Match};
use upleft_app::support::command_palette_model::{
    CommandPaletteEntry, CommandPaletteModel, CommandPaletteRecentStore, PaletteSearch,
    UserDefaultsCommandPaletteRecentStore, synonyms,
};
use upleft_app::support::commands::{
    Command, CommandContext, CommandPrecondition, CommandScope, KeyBinding, Menu, Modifier, ModifierFlags,
};
use upleft_app::support::keybindings::{KeybindingDefaults, KeybindingError, KeybindingLoad, KeybindingStore, Stored};
use upleft_app::support::quick_open_providers::{
    CurrentDocumentQuickOpenProvider, QuickOpenAction, QuickOpenProvider, QuickOpenProviderKind, QuickOpenQuery,
    QuickOpenResult, RecentFilesQuickOpenProvider, WorkspaceQuickOpenProvider,
};
use upleft_app::support::system_integration::SystemIntegration;
use upleft_app::support::welcome_tour::{WelcomeTour, WelcomeTourError};
use upleft_core::parser::MarkdownParser;
use upleft_foundation::json_decoder;
use upleft_foundation::json_encoder::{self, OutputFormatting};
use upleft_foundation::json_serialization::{self, ReadingOptions};
use upleft_foundation::url::FileUrl;
use upleft_swift_text::{self as swift_text, NSRange};

use super::json::{self, Object};
use super::{Failure, Request};

pub fn run(request: &Request) -> Result<(), Failure> {
    let data = std::fs::read(&request.input)?;
    let spec: Value = serde_json::from_slice(&data).map_err(|error| Failure::Error(error.to_string()))?;
    let spec = spec.as_object().ok_or_else(|| Failure::Error("palette input must be a JSON object".into()))?;
    let mut out = Object::new();
    if spec.get("table").and_then(Value::as_bool) == Some(true) {
        out = out.with("table", table());
    }
    if let Some(contexts) = spec.get("contexts").and_then(Value::as_array) {
        out = out.with("contexts", contexts.iter().map(context_dump).collect::<Vec<_>>());
    }
    if let Some(strings) = spec.get("parse").and_then(Value::as_array) {
        out = out.with(
            "parse",
            strings
                .iter()
                .map(|s| KeyBinding::parsing(str_of(s)).map_or(Value::Null, |b| binding_dump(&b)))
                .collect::<Vec<_>>(),
        );
    }
    if let Some(snippets) = spec.get("decodeBinding").and_then(Value::as_array) {
        out = out.with("decodeBinding", snippets.iter().map(|s| decode_binding_dump(str_of(s))).collect::<Vec<_>>());
    }
    if let Some(files) = spec.get("decodeFile").and_then(Value::as_array) {
        out = out.with(
            "decodeFile",
            files.iter().map(|s| load_dump(&KeybindingLoad::decode(str_of(s).as_bytes()))).collect::<Vec<_>>(),
        );
    }
    if let Some(files) = spec.get("decodeFileHex").and_then(Value::as_array) {
        out = out.with(
            "decodeFileHex",
            files.iter().map(|s| load_dump(&KeybindingLoad::decode(&hex_bytes(str_of(s))))).collect::<Vec<_>>(),
        );
    }
    if let Some(stored) = spec.get("encodeStored").and_then(Value::as_array) {
        out = out.with("encodeStored", stored.iter().map(encode_stored_dump).collect::<Vec<_>>());
    }
    if let Some(pairs) = spec.get("fuzzy").and_then(Value::as_array) {
        out = out.with(
            "fuzzy",
            pairs
                .iter()
                .map(|pair| {
                    let pair = pair.as_array().cloned().unwrap_or_default();
                    fuzzy_dump(str_of(&pair[0]), str_of(&pair[1]))
                })
                .collect::<Vec<_>>(),
        );
    }
    if let Some(searches) = spec.get("search").and_then(Value::as_array) {
        out = out.with(
            "search",
            searches
                .iter()
                .map(|search| {
                    let query = search.get("query").map(str_of).unwrap_or("");
                    let candidates = strings(search.get("candidates"));
                    int_or_null(PaletteSearch::score(query, &candidates))
                })
                .collect::<Vec<_>>(),
        );
    }
    if let Some(queries) = spec.get("quickOpenQueries").and_then(Value::as_array) {
        out = out.with(
            "quickOpenQueries",
            queries
                .iter()
                .map(|raw| {
                    let query = QuickOpenQuery::new(str_of(raw));
                    Object::new().with("filter", query.filter.name()).with("terms", query.terms).build()
                })
                .collect::<Vec<_>>(),
        );
    }
    if let Some(palettes) = spec.get("palettes").and_then(Value::as_array) {
        out = out.with("palettes", palettes.iter().map(palette_dump).collect::<Vec<_>>());
    }
    if let Some(recent) = spec.get("recentStore") {
        out = out.with("recentStore", recent_store_dump(recent));
    }
    if let Some(tours) = spec.get("tour").and_then(Value::as_array) {
        out = out.with("tour", tours.iter().map(|tour| tour_dump(tour, &request.input)).collect::<Vec<_>>());
    }
    if let Some(integration) = spec.get("integration") {
        out = out.with("integration", integration_dump(integration));
    }
    if let Some(store) = spec.get("store") {
        out = out.with("store", store_dump(store)?);
    }
    Ok(json::write(&out.build(), &request.output)?)
}

fn str_of(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

fn int_or_null(value: Option<isize>) -> Value {
    value.map_or(Value::Null, |value| Value::from(value as i64))
}

fn string_or_null(value: Option<String>) -> Value {
    value.map_or(Value::Null, Value::String)
}

fn hex_bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .filter(|pair| pair.len() == 2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("00"), 16).unwrap_or(0))
        .collect()
}

fn lines(text: &str) -> Value {
    Value::Array(swift_text::components_separated_by(text, "\n").into_iter().map(Value::String).collect())
}

fn range(range: NSRange) -> Value {
    Value::Array(vec![Value::from(range.location as i64), Value::from(range.length as i64)])
}

// MARK: - The command table

fn table() -> Value {
    let commands: Vec<Value> = Command::ALL_CASES
        .iter()
        .map(|&command| {
            Object::new()
                .with("id", command.raw_value())
                .with("title", command.title())
                .with("menu", command.menu().raw_value())
                .with("menuTitle", command.menu().title())
                .with("scopes", command.scopes().iter().map(|scope| Value::from(scope.raw_value())).collect::<Vec<_>>())
                .with("requires", command.requires().raw_value())
                .with(
                    "defaults",
                    KeybindingDefaults::table()
                        .get(&command)
                        .map(|b| b.iter().map(binding_dump).collect::<Vec<_>>())
                        .unwrap_or_default(),
                )
                .with("synonyms", synonyms(command).iter().map(|s| Value::from(*s)).collect::<Vec<_>>())
                .build()
        })
        .collect();
    Object::new()
        .with("commands", commands)
        .with(
            "menus",
            Menu::ALL_CASES
                .iter()
                .map(|m| Object::new().with("id", m.raw_value()).with("title", m.title()).build())
                .collect::<Vec<_>>(),
        )
        .with(
            "scopes",
            CommandScope::ALL_CASES
                .iter()
                .map(|s| Object::new().with("id", s.raw_value()).with("paletteTitle", s.palette_title()).build())
                .collect::<Vec<_>>(),
        )
        .with("preconditions", CommandPrecondition::ALL_CASES.iter().map(|p| Value::from(p.raw_value())).collect::<Vec<_>>())
        .with(
            "modifiers",
            Modifier::ALL_CASES
                .iter()
                .map(|m| Object::new().with("id", m.raw_value()).with("flag", m.flag().raw_value()).build())
                .collect::<Vec<_>>(),
        )
        .with("kinds", QuickOpenProviderKind::ALL_CASES.iter().map(|k| Value::from(k.raw_value())).collect::<Vec<_>>())
        .build()
}

fn context(values: Option<&Value>) -> CommandContext {
    let flag = |name: &str| values.and_then(|v| v.get(name)).and_then(Value::as_bool).unwrap_or(false);
    CommandContext {
        has_document: flag("hasDocument"),
        document_has_file: flag("documentHasFile"),
        has_selection: flag("hasSelection"),
        can_check_for_updates: flag("canCheckForUpdates"),
        has_unsaved_changes: flag("hasUnsavedChanges"),
        has_find_query: flag("hasFindQuery"),
        can_go_back: flag("canGoBack"),
        can_go_forward: flag("canGoForward"),
        is_speaking: flag("isSpeaking"),
        caret_is_in_table: flag("caretIsInTable"),
        has_change_marks: flag("hasChangeMarks"),
        has_quick_look_target: flag("hasQuickLookTarget"),
    }
}

fn context_dump(values: &Value) -> Value {
    let context = context(Some(values));
    Object::new()
        .with(
            "enabled",
            Command::ALL_CASES
                .iter()
                .filter(|c| c.is_enabled(&context))
                .map(|c| Value::from(c.raw_value()))
                .collect::<Vec<_>>(),
        )
        .with(
            "satisfied",
            CommandPrecondition::ALL_CASES.iter().map(|p| Value::from(p.is_satisfied(&context))).collect::<Vec<_>>(),
        )
        .build()
}

// MARK: - Key bindings

fn modifier_flags(value: Option<&Value>) -> ModifierFlags {
    if let Some(raw) = value.and_then(Value::as_u64) {
        return ModifierFlags(raw);
    }
    let mut flags = ModifierFlags::EMPTY;
    for name in strings(value) {
        flags.insert(match name.as_str() {
            "command" => ModifierFlags::COMMAND,
            "shift" => ModifierFlags::SHIFT,
            "option" => ModifierFlags::OPTION,
            "control" => ModifierFlags::CONTROL,
            "numericPad" => ModifierFlags::NUMERIC_PAD,
            "capsLock" => ModifierFlags::CAPS_LOCK,
            "function" => ModifierFlags::FUNCTION,
            "help" => ModifierFlags::HELP,
            _ => ModifierFlags::EMPTY,
        });
    }
    flags
}

fn binding(value: Option<&Value>) -> Option<KeyBinding> {
    let object = value?.as_object()?;
    let key = object.get("key")?.as_str()?;
    Some(KeyBinding::new(key, modifier_flags(object.get("modifiers"))))
}

fn binding_dump(binding: &KeyBinding) -> Value {
    let encoded = json_encoder::encode_string(&binding.encode(), OutputFormatting::SORTED);
    Object::new()
        .with("key", binding.key.clone())
        .with("modifiers", Modifier::names(binding.modifiers).iter().map(|m| Value::from(m.raw_value())).collect::<Vec<_>>())
        .with("raw", binding.modifiers.raw_value())
        .with("serialized", binding.serialized())
        .with("display", binding.display_string())
        .with("menuKeyEquivalent", binding.menu_key_equivalent().chars().map(|c| Value::from(c as u32)).collect::<Vec<_>>())
        .with("encoded", encoded)
        .with(
            "reparsed",
            KeyBinding::parsing(&binding.serialized()).map_or(Value::Null, |parsed| Value::from(parsed == *binding)),
        )
        .build()
}

fn decode_binding_dump(snippet: &str) -> Value {
    match json_decoder::parse(snippet.as_bytes()).and_then(|value| KeyBinding::decode(&value)) {
        Ok(binding) => Object::new().with("binding", binding_dump(&binding)).build(),
        Err(error) => Object::new().with("error", error.code() as i64).build(),
    }
}

fn error_dump(error: &KeybindingError) -> Value {
    Object::new()
        .with("domain", error.domain())
        .with("code", error.code() as i64)
        .with("description", error.localized_description())
        .build()
}

fn overrides_dump(overrides: &std::collections::HashMap<Command, Vec<KeyBinding>>) -> Value {
    Value::Array(
        Command::ALL_CASES
            .iter()
            .filter_map(|command| {
                let bindings = overrides.get(command)?;
                Some(Value::Array(vec![
                    Value::from(command.raw_value()),
                    Value::Array(bindings.iter().map(|b| Value::from(b.serialized())).collect()),
                ]))
            })
            .collect(),
    )
}

fn load_dump(load: &KeybindingLoad) -> Value {
    match load {
        KeybindingLoad::Absent => Object::new().with("state", "absent").build(),
        KeybindingLoad::Loaded { vim_keys_enabled, overrides } => Object::new()
            .with("state", "loaded")
            .with("vimKeysEnabled", *vim_keys_enabled)
            .with("overrides", overrides_dump(overrides))
            .build(),
        KeybindingLoad::Unreadable(error) => {
            Object::new().with("state", "unreadable").with("error", error_dump(error)).build()
        }
    }
}

fn encode_stored_dump(spec: &Value) -> Value {
    let overrides = spec
        .get("overrides")
        .and_then(Value::as_object)
        .map(|members| {
            members
                .iter()
                .map(|(name, bindings)| {
                    let bindings = bindings
                        .as_array()
                        .map(|list| list.iter().filter_map(|b| binding(Some(b))).collect())
                        .unwrap_or_default();
                    (name.clone(), bindings)
                })
                .collect()
        })
        .unwrap_or_default();
    let stored =
        Stored { vim_keys_enabled: spec.get("vimKeysEnabled").and_then(Value::as_bool).unwrap_or(false), overrides };
    let pretty = json_encoder::encode(&stored.encode(), OutputFormatting::PRETTY_SORTED);
    let compact = json_encoder::encode_string(&stored.encode(), OutputFormatting::SORTED);
    Object::new()
        .with("pretty", lines(&String::from_utf8_lossy(&pretty)))
        .with("compact", compact)
        .with("roundTrip", load_dump(&KeybindingLoad::decode(&pretty)))
        .build()
}

// MARK: - Fuzzy matching

const HIGHLIGHT_KEY: &str = "upleft.highlight";

/// The attribute runs of `highlighted`, as `[location, length, highlighted]`.
fn highlight_runs(haystack: &str, positions: &[isize]) -> Value {
    objc2::rc::autoreleasepool(|_| {
        let key = NSString::from_str(HIGHLIGHT_KEY);
        let base_key = NSString::from_str("upleft.base");
        let zero = NSNumber::new_i64(0);
        let one = NSNumber::new_i64(1);
        let base: Retained<NSDictionary<NSAttributedStringKey, AnyObject>> =
            NSDictionary::from_slices(&[&*base_key], &[&*zero as &AnyObject]);
        let highlight: Retained<NSDictionary<NSAttributedStringKey, AnyObject>> =
            NSDictionary::from_slices(&[&*key], &[&*one as &AnyObject]);
        let attributed = FuzzyMatcher::highlighted(haystack, positions, &base, &highlight);
        let length = attributed.length();
        let mut runs = Vec::new();
        let mut index = 0usize;
        while index < length {
            let mut effective = FoundationRange::new(0, 0);
            let value = unsafe { attributed.attribute_atIndex_effectiveRange(&key, index, &mut effective) };
            runs.push(Value::Array(vec![
                Value::from(effective.location as i64),
                Value::from(effective.length as i64),
                Value::from(value.is_some()),
            ]));
            index = effective.location + effective.length;
        }
        Value::Array(runs)
    })
}

fn match_dump(found: Option<&Match>) -> Value {
    match found {
        None => Value::Null,
        Some(found) => Object::new()
            .with("score", found.score as i64)
            .with("positions", found.positions.iter().map(|p| Value::from(*p as i64)).collect::<Vec<_>>())
            .build(),
    }
}

fn fuzzy_dump(needle: &str, haystack: &str) -> Value {
    let found = FuzzyMatcher::r#match(needle, haystack);
    Object::new()
        .with("match", match_dump(found.as_ref()))
        .with("highlight", found.as_ref().map_or(Value::Null, |m| highlight_runs(haystack, &m.positions)))
        .build()
}

// MARK: - Palette

fn action_dump(action: &QuickOpenAction) -> Value {
    match action {
        QuickOpenAction::Command(command) => Object::new().with("command", command.raw_value()).build(),
        QuickOpenAction::Select(selection) => Object::new().with("select", range(*selection)).build(),
        QuickOpenAction::Open(url) => Object::new().with("open", url.path()).build(),
        QuickOpenAction::OpenAt(url, selection) => {
            Object::new().with("openAt", Value::Array(vec![Value::from(url.path()), range(*selection)])).build()
        }
    }
}

fn result_dump(result: &QuickOpenResult, terms: &str) -> Value {
    let found = FuzzyMatcher::r#match(terms, &result.title);
    Object::new()
        .with("id", result.id.clone())
        .with("kind", result.kind.raw_value())
        .with("title", result.title.clone())
        .with("subtitle", result.subtitle.clone())
        .with("searchText", result.search_text.clone())
        .with("action", action_dump(&result.action))
        .with("score", result.score as i64)
        .with("match", match_dump(found.as_ref()))
        .with("highlight", found.as_ref().map_or(Value::Null, |m| highlight_runs(&result.title, &m.positions)))
        .build()
}

fn entry_dump(entry: &CommandPaletteEntry) -> Value {
    Object::new()
        .with("command", entry.command.raw_value())
        .with("title", entry.title.clone())
        .with("synonyms", entry.synonyms.iter().map(|s| Value::from(s.clone())).collect::<Vec<_>>())
        .with("binding", string_or_null(entry.binding.clone()))
        .with("scopes", entry.scopes.iter().map(|s| Value::from(s.raw_value())).collect::<Vec<_>>())
        .with("scopeLabel", entry.scope_label())
        .build()
}

fn commands(value: Option<&Value>) -> Vec<Command> {
    strings(value).iter().filter_map(|raw| Command::from_raw_value(raw)).collect()
}

fn model(spec: &Value) -> CommandPaletteModel {
    let mut providers: Vec<Rc<dyn QuickOpenProvider>> = Vec::new();
    if let Some(document) = spec.get("document").and_then(Value::as_str) {
        providers.push(Rc::new(CurrentDocumentQuickOpenProvider::new(MarkdownParser::parse(document))));
    }
    if let Some(files) = spec.get("recentFiles").and_then(Value::as_array) {
        providers.push(Rc::new(RecentFilesQuickOpenProvider {
            files: files.iter().map(|f| FileUrl::from_path(str_of(f))).collect(),
        }));
    }
    if let Some(workspace) = spec.get("workspace") {
        let files = strings(workspace.get("files")).iter().map(|f| FileUrl::from_path(f)).collect();
        let symbols = workspace
            .get("symbols")
            .and_then(Value::as_array)
            .map(|symbols| {
                symbols
                    .iter()
                    .map(|symbol| {
                        let text = |name: &str| symbol.get(name).and_then(Value::as_str).unwrap_or("").to_owned();
                        let number = |name: &str| symbol.get(name).and_then(Value::as_i64).unwrap_or(0) as isize;
                        let path = symbol.get("path").and_then(Value::as_str).unwrap_or("/");
                        let mut result = QuickOpenResult::new(
                            text("id"),
                            QuickOpenProviderKind::Symbol,
                            text("title"),
                            QuickOpenAction::OpenAt(
                                FileUrl::from_path(path),
                                NSRange::new(number("location"), number("length")),
                            ),
                        )
                        .with_subtitle(text("subtitle"))
                        .with_search_text(text("searchText"));
                        result.score = number("score");
                        result
                    })
                    .collect()
            })
            .unwrap_or_default();
        providers.push(Rc::new(WorkspaceQuickOpenProvider { files, symbols }));
    }
    let recents = commands(spec.get("recents"));
    if let Some(entries) = spec.get("entries").and_then(Value::as_array) {
        let custom = entries
            .iter()
            .filter_map(|entry| {
                let command = Command::from_raw_value(entry.get("command").and_then(Value::as_str).unwrap_or(""))?;
                Some(CommandPaletteEntry {
                    command,
                    title: entry.get("title").and_then(Value::as_str).unwrap_or("").to_owned(),
                    synonyms: strings(entry.get("synonyms")),
                    binding: entry.get("binding").and_then(Value::as_str).map(str::to_owned),
                    scopes: strings(entry.get("scopes")).iter().filter_map(|s| CommandScope::from_raw_value(s)).collect(),
                })
            })
            .collect();
        return CommandPaletteModel::new(custom, recents, providers);
    }
    let command_list: Vec<Command> = match spec.get("commands") {
        Some(Value::Array(_)) => commands(spec.get("commands")),
        Some(Value::String(mode)) if mode == "enabled" => {
            let context = context(spec.get("context"));
            Command::ALL_CASES.into_iter().filter(|c| c.is_enabled(&context)).collect()
        }
        _ => Command::ALL_CASES.to_vec(),
    };
    match spec.get("bindings").and_then(Value::as_str) {
        Some("none") => CommandPaletteModel::with_commands(&command_list, |_| Vec::new(), recents, providers),
        Some("store") => CommandPaletteModel::with_commands(
            &command_list,
            |c| KeybindingStore::shared().bindings(c),
            recents,
            providers,
        ),
        _ => CommandPaletteModel::with_commands(
            &command_list,
            |c| KeybindingDefaults::table().get(&c).cloned().unwrap_or_default(),
            recents,
            providers,
        ),
    }
}

fn selection_dump(model: &CommandPaletteModel, object: Object) -> Object {
    object
        .with("selectedIndex", model.selected_index() as i64)
        .with("selectedEntry", string_or_null(model.selected_entry().map(|e| e.command.raw_value().to_owned())))
        .with("selectedResult", string_or_null(model.selected_result().map(|r| r.id)))
}

fn query_dump(model: &CommandPaletteModel) -> Value {
    let parsed = QuickOpenQuery::new(model.query());
    let trimmed = swift_text::trim_whitespaces_and_newlines(model.query());
    let results: Vec<Value> = model
        .results()
        .iter()
        .map(|entry| {
            Object::new()
                .with("command", entry.command.raw_value())
                .with("score", int_or_null(PaletteSearch::score(trimmed, &entry.search_candidates())))
                .build()
        })
        .collect();
    let object = Object::new()
        .with("query", model.query().to_owned())
        .with("filter", parsed.filter.name())
        .with("terms", parsed.terms.clone())
        .with("results", results)
        .with("quick", model.quick_results().iter().map(|r| result_dump(r, &parsed.terms)).collect::<Vec<_>>());
    selection_dump(model, object).build()
}

fn palette_dump(spec: &Value) -> Value {
    let mut model = model(spec);
    let mut out = Object::new();
    if spec.get("dumpEntries").and_then(Value::as_bool) == Some(true) {
        out = out.with("entries", model.entries().iter().map(entry_dump).collect::<Vec<_>>());
    }
    out = out.with("recents", model.recent_commands().iter().map(|c| Value::from(c.raw_value())).collect::<Vec<_>>());
    let mut queries = Vec::new();
    for query in strings(spec.get("queries")) {
        model.update_query(&query);
        queries.push(query_dump(&model));
    }
    out = out.with("queries", queries);
    let mut steps = Vec::new();
    for step in spec.get("steps").and_then(Value::as_array).cloned().unwrap_or_default() {
        if let Some(query) = step.get("query").and_then(Value::as_str) {
            model.update_query(query);
        }
        if let Some(offset) = step.get("move").and_then(Value::as_i64) {
            model.move_selection(offset as isize);
        }
        if let Some(index) = step.get("select").and_then(Value::as_i64) {
            model.select(index as isize);
        }
        if let Some(command) = step.get("record").and_then(Value::as_str).and_then(Command::from_raw_value) {
            model.record(command);
        }
        let quick = model.quick_results();
        let object = selection_dump(&model, Object::new())
            .with("recents", model.recent_commands().iter().map(|c| Value::from(c.raw_value())).collect::<Vec<_>>())
            .with("quickCount", quick.len() as i64)
            .with("first", string_or_null(quick.first().map(|r| r.id.clone())));
        steps.push(object.build());
    }
    out.with("steps", steps).build()
}

// MARK: - Recent commands

/// `~/Library/Preferences/<suite>.plist` in the real home: `UserDefaults`
/// ignores `CFFIXED_USER_HOME`.
fn real_preferences_file(suite: &str) -> Option<String> {
    let entry = unsafe { libc::getpwuid(libc::getuid()) };
    if entry.is_null() {
        return None;
    }
    let home = unsafe { std::ffi::CStr::from_ptr((*entry).pw_dir) }.to_string_lossy().into_owned();
    Some(format!("{home}/Library/Preferences/{suite}.plist"))
}

fn recent_store_dump(spec: &Value) -> Value {
    let suite = format!("upleft.conformance.palette.{}", objc2_foundation::NSUUID::new().UUIDString());
    let defaults = NSUserDefaults::initWithSuiteName(NSUserDefaults::alloc(), Some(&NSString::from_str(&suite)))
        .expect("a suite name other than the app's own");
    let key = spec
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or(UserDefaultsCommandPaletteRecentStore::DEFAULT_KEY)
        .to_owned();
    if let Some(initial) = spec.get("initial") {
        let bytes = serde_json::to_vec(initial).unwrap_or_default();
        if let Ok(object) = json_serialization::json_object(&bytes, ReadingOptions { fragments_allowed: true }) {
            let foundation = object.to_foundation();
            unsafe { defaults.setObject_forKey(Some(&foundation), &NSString::from_str(&key)) };
        }
    }
    let limit = spec.get("limit").and_then(Value::as_i64).unwrap_or(12) as isize;
    let store = UserDefaultsCommandPaletteRecentStore::new(defaults.clone(), &key, limit);
    let stored = || -> Value {
        objc2::rc::autoreleasepool(|_| {
            let Some(array) = defaults.arrayForKey(&NSString::from_str(&key)) else { return Value::Null };
            Value::Array(
                array
                    .iter()
                    .map(|value| match value.downcast_ref::<NSString>() {
                        Some(string) => Value::from(swift_text::ns::foundation::to_string(string)),
                        None => Value::from("<non-string>"),
                    })
                    .collect(),
            )
        })
    };
    let snapshot = |store: &UserDefaultsCommandPaletteRecentStore| {
        Object::new()
            .with("recent", store.recent_commands().iter().map(|c| Value::from(c.raw_value())).collect::<Vec<_>>())
            .with("stored", stored())
            .build()
    };
    let mut steps = vec![snapshot(&store)];
    for raw in strings(spec.get("record")) {
        let Some(command) = Command::from_raw_value(&raw) else { continue };
        store.record(command);
        steps.push(snapshot(&store));
    }
    defaults.removePersistentDomainForName(&NSString::from_str(&suite));
    if let Some(path) = real_preferences_file(&suite) {
        let _ = std::fs::remove_file(path);
    }
    Value::Array(steps)
}

// MARK: - Welcome tour

/// `file` is relative to the input's folder, as the Swift dump reads it.
fn tour_dump(spec: &Value, input: &std::path::Path) -> Value {
    let source = match spec.get("file").and_then(Value::as_str) {
        Some(file) => {
            let path = input.parent().map(|folder| folder.join(file)).unwrap_or_default();
            std::fs::read_to_string(path).unwrap_or_default()
        }
        None => spec.get("source").and_then(Value::as_str).unwrap_or("").to_owned(),
    };
    let custom: Option<std::collections::HashMap<Command, KeyBinding>> =
        spec.get("bindings").and_then(Value::as_object).map(|table| {
            table
                .iter()
                .filter_map(|(name, value)| Some((Command::from_raw_value(name)?, binding(Some(value))?)))
                .collect()
        });
    let none = spec.get("bindings").and_then(Value::as_str) == Some("none");
    let lookup = |command: Command| -> Option<KeyBinding> {
        if none {
            return None;
        }
        match &custom {
            Some(table) => table.get(&command).cloned(),
            None => KeybindingDefaults::table().get(&command).and_then(|b| b.first().cloned()),
        }
    };
    let rendered = match WelcomeTour::render(&source, lookup) {
        Ok(text) => Object::new().with("text", lines(&text)).build(),
        Err(WelcomeTourError::MalformedToken(token)) => Object::new().with("malformedToken", token).build(),
        Err(WelcomeTourError::UnknownCommand(name)) => Object::new().with("unknownCommand", name).build(),
        Err(WelcomeTourError::MissingBinding(command)) => {
            Object::new().with("missingBinding", command.raw_value()).build()
        }
    };
    Object::new()
        .with("tokens", WelcomeTour::tokens(&source).into_iter().map(Value::String).collect::<Vec<_>>())
        .with("render", rendered)
        .build()
}

// MARK: - Integrations

fn integration_dump(spec: &Value) -> Value {
    let accepts: Vec<Value> = strings(spec.get("accepts"))
        .iter()
        .map(|value| {
            if value.contains("://") {
                match NSURL::URLWithString(&NSString::from_str(value)) {
                    Some(url) => Value::from(NativeIntegrationPolicy::accepts_url(&url)),
                    None => Value::Null,
                }
            } else {
                Value::from(NativeIntegrationPolicy::accepts(&FileUrl::from_path(value)))
            }
        })
        .collect();
    let normalized: Vec<Value> = strings(spec.get("normalized"))
        .iter()
        .map(|value| string_or_null(NativeIntegrationPolicy::normalized_path(value).map(|url| url.path())))
        .collect();
    let pluginkit: Vec<Value> = spec
        .get("pluginkit")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    let text = |name: &str| item.get(name).and_then(Value::as_str).unwrap_or("");
                    Value::from(SystemIntegration::is_enabled(text("listing"), text("identifier")))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut markdown: Vec<&str> = NativeIntegrationPolicy::MARKDOWN_EXTENSIONS.to_vec();
    markdown.sort_by(|a, b| swift_text::str_cmp(a, b));
    Object::new()
        .with("accepts", accepts)
        .with("normalized", normalized)
        .with("pluginkit", pluginkit)
        .with(
            "claimedExtensions",
            SystemIntegration::CLAIMED_EXTENSIONS.iter().map(|e| Value::from(*e)).collect::<Vec<_>>(),
        )
        .with(
            "claimedTypes",
            SystemIntegration::claimed_types()
                .iter()
                .map(|t| Value::from(t.identifier().to_string()))
                .collect::<Vec<_>>(),
        )
        .with(
            "commandLineNames",
            SystemIntegration::COMMAND_LINE_NAMES.iter().map(|n| Value::from(*n)).collect::<Vec<_>>(),
        )
        .with(
            "extensionIdentifiers",
            vec![
                Value::from(SystemIntegration::PREVIEW_EXTENSION_IDENTIFIER),
                Value::from(SystemIntegration::THUMBNAIL_EXTENSION_IDENTIFIER),
            ],
        )
        .with("markdownExtensions", markdown.into_iter().map(Value::from).collect::<Vec<_>>())
        .build()
}

// MARK: - The keybinding store

/// `(keyCode, characters)` for an event that produces `key`.
fn event_key(key: &str) -> (u16, String) {
    let named = |code: u16, characters: &str| (code, characters.to_owned());
    match key {
        "space" => named(49, " "),
        "left" => named(123, "\u{F702}"),
        "right" => named(124, "\u{F703}"),
        "down" => named(125, "\u{F701}"),
        "up" => named(126, "\u{F700}"),
        "return" => named(36, "\r"),
        "enter" => named(76, "\u{3}"),
        "tab" => named(48, "\t"),
        "escape" => named(53, "\u{1B}"),
        "delete" => named(51, "\u{7F}"),
        "pageup" => named(116, "\u{F72C}"),
        "pagedown" => named(121, "\u{F72D}"),
        _ => (0, key.to_owned()),
    }
}

fn event(key_code: u16, characters: &str, modifiers: ModifierFlags) -> Option<Retained<NSEvent>> {
    let characters = swift_text::ns::foundation::ns_from_utf16(&swift_text::ns::utf16(characters));
    NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        NSEventType::KeyDown,
        NSPoint::new(0.0, 0.0),
        NSEventModifierFlags(modifiers.raw_value() as usize),
        0.0,
        0,
        None,
        &characters,
        &characters,
        false,
        key_code,
    )
}

fn resolve(event: Option<&NSEvent>, store: &KeybindingStore) -> Value {
    Value::Array(
        CommandScope::ALL_CASES
            .iter()
            .map(|scope| {
                string_or_null(
                    event.and_then(|event| store.command_for_event(event, *scope)).map(|c| c.raw_value().to_owned()),
                )
            })
            .collect(),
    )
}

fn file_dump(path: &std::path::Path) -> Value {
    let Ok(metadata) = std::fs::metadata(path) else { return Value::Null };
    if metadata.is_dir() {
        return Object::new().with("directory", true).build();
    }
    let Ok(data) = std::fs::read(path) else { return Object::new().with("unreadable", true).build() };
    // `String(data:encoding: .utf8)` drops a leading byte-order mark.
    let body = data.strip_prefix(b"\xEF\xBB\xBF").map(<[u8]>::to_vec).unwrap_or(data);
    match String::from_utf8(body) {
        Ok(text) => Object::new().with("lines", lines(&text)).build(),
        Err(error) => Object::new()
            .with("hex", error.as_bytes().iter().map(|b| format!("{b:02x}")).collect::<String>())
            .build(),
    }
}

fn state_dump(store: &KeybindingStore, file: &std::path::Path, events: &[Value], object: Object) -> Object {
    store.flush();
    let lookup: Vec<Value> = objc2::rc::autoreleasepool(|_| {
        Command::ALL_CASES
            .iter()
            .flat_map(|&command| store.bindings(command).into_iter().map(move |binding| (command, binding)))
            .map(|(command, binding)| {
                let (code, characters) = event_key(&binding.key);
                let event = event(code, &characters, binding.modifiers);
                Value::Array(vec![
                    Value::from(command.raw_value()),
                    Value::from(binding.serialized()),
                    resolve(event.as_deref(), store),
                ])
            })
            .collect()
    });
    let explicit: Vec<Value> = objc2::rc::autoreleasepool(|_| {
        events
            .iter()
            .map(|spec| {
                let key_code = spec.get("keyCode").and_then(Value::as_u64).unwrap_or(0) as u16;
                let characters = spec.get("characters").and_then(Value::as_str).unwrap_or("");
                let event = event(key_code, characters, modifier_flags(spec.get("modifiers")));
                resolve(event.as_deref(), store)
            })
            .collect()
    });
    object
        .with("loadFailure", store.load_failure().map_or(Value::Null, error_dump))
        .with("lastPersistenceError", store.last_persistence_error().is_some())
        .with("vimKeysEnabled", store.vim_keys_enabled())
        .with(
            "overridden",
            Command::ALL_CASES
                .iter()
                .filter(|c| store.is_overridden(**c))
                .map(|c| Value::from(c.raw_value()))
                .collect::<Vec<_>>(),
        )
        .with(
            "bindings",
            Command::ALL_CASES
                .iter()
                .map(|&c| {
                    Value::Array(vec![
                        Value::from(c.raw_value()),
                        Value::Array(store.bindings(c).iter().map(|b| Value::from(b.serialized())).collect()),
                    ])
                })
                .collect::<Vec<_>>(),
        )
        .with(
            "primary",
            Command::ALL_CASES
                .iter()
                .map(|&c| string_or_null(store.primary_binding(c).map(|b| b.display_string())))
                .collect::<Vec<_>>(),
        )
        .with("lookup", lookup)
        .with("events", explicit)
        .with("file", file_dump(file))
}

fn operation_name(operation: &Value) -> String {
    if let Some(raw) = operation.get("set").and_then(Value::as_str) {
        return format!("set {raw}");
    }
    if operation.get("reset").is_some() {
        return "reset".into();
    }
    if let Some(vim) = operation.get("vim").and_then(Value::as_bool) {
        return format!("vim {vim}");
    }
    if operation.get("conflicts").is_some() {
        return "conflicts".into();
    }
    "unknown".into()
}

fn store_dump(spec: &Value) -> Result<Value, Failure> {
    let root =
        std::env::temp_dir().join(format!("upleft-palette-{}", objc2_foundation::NSUUID::new().UUIDString()));
    std::fs::create_dir_all(&root)?;
    let result = store_dump_in(spec, &root);
    let _ = std::fs::remove_dir_all(&root);
    result
}

fn store_dump_in(spec: &Value, root: &std::path::Path) -> Result<Value, Failure> {
    let support = root.join("support");
    let file = support.join("keybindings.json");
    if spec.get("supportIsFile").and_then(Value::as_bool) == Some(true) {
        std::fs::write(&support, "not a directory")?;
    } else if spec.get("file").is_some() || spec.get("fileHex").is_some() || spec.get("fileIsDirectory").is_some() {
        std::fs::create_dir_all(&support)?;
    }
    if let Some(text) = spec.get("file").and_then(Value::as_str) {
        std::fs::write(&file, text)?;
    }
    if let Some(hex) = spec.get("fileHex").and_then(Value::as_str) {
        std::fs::write(&file, hex_bytes(hex))?;
    }
    if spec.get("fileIsDirectory").and_then(Value::as_bool) == Some(true) {
        std::fs::create_dir_all(&file)?;
    }
    // SAFETY: the oracle is single-threaded until the store is first used.
    unsafe { std::env::set_var("DOWNRIGHT_SUPPORT_DIRECTORY", &support) };

    let mut store = KeybindingStore::shared();
    let events = spec.get("events").and_then(Value::as_array).cloned().unwrap_or_default();
    let reported = Arc::new(Mutex::new(Vec::<String>::new()));
    if spec.get("onLoadFailure").and_then(Value::as_bool) == Some(true) {
        let sink = Arc::clone(&reported);
        store.set_on_load_failure(Some(Box::new(move |error: &KeybindingError| {
            sink.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(error.localized_description());
        })));
    }
    let mut states = vec![state_dump(&store, &file, &events, Object::new().with("op", "load")).build()];
    for operation in spec.get("operations").and_then(Value::as_array).cloned().unwrap_or_default() {
        let mut object = Object::new().with("op", operation_name(&operation));
        if let Some(command) = operation.get("set").and_then(Value::as_str).and_then(Command::from_raw_value) {
            store.set_binding(binding(operation.get("binding")), command);
        } else if operation.get("reset").and_then(Value::as_bool) == Some(true) {
            store.reset_to_defaults();
        } else if let Some(vim) = operation.get("vim").and_then(Value::as_bool) {
            store.set_vim_keys_enabled(vim);
        } else if let (Some(target), Some(excluding)) = (
            binding(operation.get("conflicts")),
            operation.get("excluding").and_then(Value::as_str).and_then(Command::from_raw_value),
        ) {
            object = object.with(
                "conflicts",
                store.conflicts(&target, excluding).iter().map(|c| Value::from(c.raw_value())).collect::<Vec<_>>(),
            );
        }
        states.push(state_dump(&store, &file, &events, object).build());
    }
    let reported: Vec<Value> =
        reported.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).iter().cloned().map(Value::String).collect();
    Ok(Object::new().with("states", states).with("reported", reported).build())
}
