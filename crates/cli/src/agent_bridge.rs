//! Port of `Sources/drdownright/AgentBridge.swift`: the agent side of the
//! CLI, the small amount of pure logic that lets a coding agent hand a
//! freshly-written Markdown file to Downright.
//!
//! The product thesis is that markdown is increasingly written by machines
//! and read by people, and the expensive moment is the handover — the agent
//! rewrites a file, and you find out by scrolling back to a document you had
//! already read. `down notify` closes that gap: the agent tells Downright the
//! instant it writes, and the app's existing external-change review surface
//! does the rest.
//!
//! Everything here is pure so it can be tested without an agent, a file
//! system watcher, or a running app. The impure parts (watching, launching)
//! live in the `down` executable.
//!
//! Settings are `[String: Any]` trees read and written by `JSONSerialization`
//! ([`AnyJson`]). Swift's casts are reproduced exactly: `as? [[String: Any]]`
//! fails as a whole when any element is not a dictionary, `as? String` fails
//! on a number, and a failed cast falls back to the empty value (`?? []`).

use upleft_foundation::json_serialization::{self, AnyJson, ReadingOptions, WritingOptions};
use upleft_foundation::url::{self, FileUrl};
use upleft_swift_text as swift_text;

use crate::markdown_cli;

// MARK: - Hook payloads

/// The tool names worth reacting to. A hook that fires on `Read` or `Bash`
/// would wake the app for work that never touched a byte on disk.
pub const TOOL_MATCHER: &str = "Write|Edit|MultiEdit|NotebookEdit";

/// `value as? [String: Any]`.
fn as_dictionary(value: Option<&AnyJson>) -> Option<&Vec<(String, AnyJson)>> {
    value?.as_object()
}

/// `dictionary[key]` on a dictionary bridged from Foundation (an `NSString`
/// key lookup).
fn member<'a>(dictionary: &'a [(String, AnyJson)], key: &str) -> Option<&'a AnyJson> {
    dictionary.iter().find(|(k, _)| k == key).map(|(_, value)| value)
}

/// `value as? String`.
fn as_string(value: Option<&AnyJson>) -> Option<&str> {
    value?.as_str()
}

/// `value as? [[String: Any]]`: every element must be a dictionary.
fn as_dictionary_array(value: Option<&AnyJson>) -> Option<Vec<&Vec<(String, AnyJson)>>> {
    value?.as_array()?.iter().map(AnyJson::as_object).collect()
}

/// `NSDictionary as? [String: Any]` makes a native Swift dictionary, whose
/// keys compare by canonical equivalence: keys that differ only in their
/// Unicode normalization collapse into one entry. Which one survives changes
/// from process to process in Swift (it depends on the per-process hash
/// seed); Upleft keeps the last in Foundation's enumeration order. See
/// docs/KNOWN-DIFFERENCES.md.
pub fn bridged(dictionary: &[(String, AnyJson)]) -> Vec<(String, AnyJson)> {
    let mut out: Vec<(String, AnyJson)> = Vec::with_capacity(dictionary.len());
    for (key, value) in dictionary {
        match out.iter().position(|(existing, _)| swift_text::str_eq(existing, key)) {
            Some(index) => out[index] = (key.clone(), value.clone()),
            None => out.push((key.clone(), value.clone())),
        }
    }
    out
}

/// Swift `==` on `String`s (canonical equivalence).
fn same(a: &str, b: &str) -> bool {
    swift_text::str_eq(a, b)
}

/// Extracts the file a hook payload is reporting on.
///
/// Claude Code delivers hook input as JSON on stdin, with the tool's own
/// arguments under `tool_input`. The key differs per tool (`file_path` for
/// the editing tools, `notebook_path` for notebooks) and some tools echo the
/// resolved path back under `tool_response`, so this checks each known
/// location rather than assuming one shape.
///
/// Returns nothing for malformed input by design. A hook that throws blocks
/// the agent's turn, and no markdown file is worth stalling a coding session
/// over — the caller treats an empty result as "nothing to do" and exits 0.
pub fn hook_payload_paths(data: &[u8]) -> Vec<String> {
    let Ok(root) = json_serialization::json_object(data, ReadingOptions::default()) else { return Vec::new() };
    let Some(root) = root.as_object() else { return Vec::new() };
    let mut found: Vec<String> = Vec::new();
    let mut collect = |container: Option<&Vec<(String, AnyJson)>>| {
        let Some(container) = container else { return };
        for key in ["file_path", "filePath", "notebook_path", "notebookPath", "path"] {
            if let Some(value) = as_string(member(container, key))
                && !value.is_empty()
            {
                found.push(value.to_owned());
            }
        }
    };
    collect(as_dictionary(member(root, "tool_input")));
    collect(as_dictionary(member(root, "tool_response")));
    collect(Some(root));

    // `Set<String>`: canonically equivalent paths collapse.
    let mut seen = std::collections::HashSet::new();
    found.into_iter().filter(|path| seen.insert(swift_text::string_key(path))).collect()
}

/// Narrows hook paths to Markdown files that actually exist on disk.
///
/// The existence check is not defensive padding: `Edit` fires its hook after
/// the write, but a tool that failed still emits a payload, and asking
/// `open` to launch the app for a path that is not there produces a Finder
/// error dialog in front of somebody who is not even looking at the app.
pub fn openable_targets_with(paths: &[String], file_exists: impl Fn(&str) -> bool) -> Vec<String> {
    paths
        .iter()
        .filter_map(|path| {
            let expanded = url::expanding_tilde_in_path(path);
            let standardized = FileUrl::from_path(&expanded).standardized_file_url().path();
            (markdown_cli::is_markdown_path(&standardized) && file_exists(&standardized)).then_some(standardized)
        })
        .collect()
}

/// `openableTargets(in:)` with the default `FileManager.default.fileExists`.
pub fn openable_targets(paths: &[String]) -> Vec<String> {
    openable_targets_with(paths, upleft_foundation::foundation_io::file_exists)
}

// MARK: - Hook installation

/// Where a generated hook is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HookScope {
    /// `~/.claude/settings.json` — every project this user opens.
    User,
    /// `.claude/settings.json` under the current project.
    Project,
}

impl HookScope {
    pub const ALL_CASES: [HookScope; 2] = [HookScope::User, HookScope::Project];

    pub fn raw_value(&self) -> &'static str {
        match self {
            HookScope::User => "user",
            HookScope::Project => "project",
        }
    }

    /// `HookScope(rawValue:)`.
    pub fn from_raw_value(raw: &str) -> Option<HookScope> {
        HookScope::ALL_CASES.into_iter().find(|scope| same(scope.raw_value(), raw))
    }

    pub fn relative_path(&self) -> &'static str {
        match self {
            HookScope::User => ".claude/settings.json",
            HookScope::Project => ".claude/settings.json",
        }
    }

    pub fn settings_url(&self, home: &FileUrl, working_directory: &FileUrl) -> FileUrl {
        match self {
            HookScope::User => home.appending_path_component(self.relative_path()),
            HookScope::Project => working_directory.appending_path_component(self.relative_path()),
        }
    }
}

/// The shell command a hook runs. `notify` reads the payload on stdin, so
/// the hook needs no `jq`, no shell quoting, and no per-tool special casing.
///
/// The executable is POSIX single-quoted (with the usual `'\''` escape)
/// rather than double-quoted: a double-quoted string still expands `$`,
/// backticks, and backslashes when the agent's hook runner shells out, so
/// an unusual install path could smuggle command substitution into
/// settings.json.
pub fn hook_command(executable: &str) -> String {
    let quoted = format!("'{}'", swift_text::replacing_occurrences(executable, "'", "'\\''"));
    format!("{quoted} notify")
}

/// Command strings earlier builds wrote for this executable: the bare path
/// and a double-quoted variant. Recognizing them lets an upgrade find its own
/// previous hook instead of appending a duplicate next to it, and lets an
/// uninstall remove it instead of leaving debris.
fn legacy_hook_commands(executable: &str) -> Vec<String> {
    vec![format!("{executable} notify"), format!("\"{executable}\" notify")]
}

/// Every command string that counts as this app's hook for the executable:
/// the canonical quoted form plus the pre-quoting legacy forms.
fn owned_hook_commands(executable: &str) -> Vec<String> {
    let mut commands = legacy_hook_commands(executable);
    commands.push(hook_command(executable));
    commands
}

/// `Set<String>.contains`.
fn set_contains(set: &[String], value: &str) -> bool {
    set.iter().any(|member| same(member, value))
}

/// One `PostToolUse` entry in Claude Code's settings schema.
pub fn hook_entry(executable: &str) -> AnyJson {
    AnyJson::Object(vec![
        ("matcher".into(), AnyJson::String(TOOL_MATCHER.into())),
        (
            "hooks".into(),
            AnyJson::Array(vec![AnyJson::Object(vec![
                ("type".into(), AnyJson::String("command".into())),
                ("command".into(), AnyJson::String(hook_command(executable))),
            ])]),
        ),
    ])
}

/// `(entry["hooks"] as? [[String: Any]] ?? []).compactMap { $0["command"] as? String }`.
fn commands_in(entry: &[(String, AnyJson)]) -> Vec<&str> {
    as_dictionary_array(member(entry, "hooks"))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|hook| as_string(member(hook, "command")))
        .collect()
}

fn has_our_matcher(entry: &[(String, AnyJson)]) -> bool {
    as_string(member(entry, "matcher")).is_some_and(|matcher| same(matcher, TOOL_MATCHER))
}

fn members(settings: &AnyJson) -> &[(String, AnyJson)] {
    settings.as_object().map(Vec::as_slice).unwrap_or(&[])
}

/// Whether these settings already run our hook.
///
/// The agent's `settings.json` is the only source of truth for this — not a
/// stored preference — because it is the file the agent actually reads, and
/// a user who edits it by hand or checks one into a project must not find
/// the app disagreeing with reality.
pub fn is_hook_installed(settings: &AnyJson, executable: &str) -> bool {
    let commands = owned_hook_commands(executable);
    let entries = as_dictionary(member(members(settings), "hooks"))
        .and_then(|hooks| as_dictionary_array(member(hooks, "PostToolUse")))
        .unwrap_or_default();
    entries.iter().any(|entry| {
        // Our hook is identified by its matcher *and* its command. A foreign
        // entry that happens to run `down notify` under another matcher must
        // not count as installed — matching on the command alone would block
        // a needed install and let uninstall delete it.
        has_our_matcher(entry) && commands_in(entry).iter().any(|command| set_contains(&commands, command))
    })
}

/// Merges the Downright hook into an existing settings object.
///
/// Idempotent, and deliberately additive: a user's `settings.json` is their
/// own file with their own hooks in it, so this preserves every unrelated
/// key, appends rather than replaces the `PostToolUse` array, and returns
/// the input unchanged when the canonical hook is already installed. A
/// legacy-format entry of ours is migrated in place — replaced by the
/// canonical quoted command at the same position — rather than duplicated.
pub fn installing_hook(settings: &AnyJson, executable: &str) -> (AnyJson, bool) {
    let mut hooks: Vec<(String, AnyJson)> =
        as_dictionary(member(members(settings), "hooks")).map(|hooks| bridged(hooks)).unwrap_or_default();
    let mut post_tool_use: Vec<Vec<(String, AnyJson)>> = as_dictionary_array(member(&hooks, "PostToolUse"))
        .map(|entries| entries.into_iter().map(|entry| bridged(entry)).collect())
        .unwrap_or_default();

    let canonical = hook_command(executable);
    let legacy = legacy_hook_commands(executable);

    // Canonical already present: nothing to do, even if a legacy entry
    // somehow sits beside it (that combination means hands edited the file,
    // and hands win).
    if post_tool_use
        .iter()
        .any(|entry| has_our_matcher(entry) && commands_in(entry).iter().any(|command| same(command, &canonical)))
    {
        return (settings.clone(), false);
    }

    // A legacy entry of ours: migrate it to the canonical form where it
    // stands instead of appending a second hook that fires twice.
    let mut migrated = false;
    for entry in post_tool_use.iter_mut() {
        if !(has_our_matcher(entry) && commands_in(entry).iter().any(|command| set_contains(&legacy, command))) {
            continue;
        }
        migrated = true;
        let updated_hooks: Vec<AnyJson> = as_dictionary_array(member(entry, "hooks"))
            .unwrap_or_default()
            .into_iter()
            .map(|hook| {
                let mut replacement = bridged(hook);
                if let Some(command) = as_string(member(hook, "command"))
                    && set_contains(&legacy, command)
                {
                    AnyJson::set(&mut replacement, "command", AnyJson::String(canonical.clone()));
                }
                AnyJson::Object(replacement)
            })
            .collect();
        AnyJson::set(entry, "hooks", AnyJson::Array(updated_hooks));
    }
    let mut settings_members = members(settings).to_vec();
    if migrated {
        AnyJson::set(&mut hooks, "PostToolUse", AnyJson::Array(post_tool_use.into_iter().map(AnyJson::Object).collect()));
        AnyJson::set(&mut settings_members, "hooks", AnyJson::Object(hooks));
        return (AnyJson::Object(settings_members), true);
    }

    let mut entries: Vec<AnyJson> = post_tool_use.into_iter().map(AnyJson::Object).collect();
    entries.push(hook_entry(executable));
    AnyJson::set(&mut hooks, "PostToolUse", AnyJson::Array(entries));
    AnyJson::set(&mut settings_members, "hooks", AnyJson::Object(hooks));
    (AnyJson::Object(settings_members), true)
}

/// Removes a previously installed Downright hook, pruning any matcher group
/// left empty so uninstalling does not leave debris behind.
pub fn removing_hook(settings: &AnyJson, executable: &str) -> (AnyJson, bool) {
    let Some(hooks) = as_dictionary(member(members(settings), "hooks")) else { return (settings.clone(), false) };
    let Some(post_tool_use) = as_dictionary_array(member(hooks, "PostToolUse")) else {
        return (settings.clone(), false);
    };
    let mut hooks = bridged(hooks);

    let commands = owned_hook_commands(executable);
    let mut changed = false;
    let mut remaining: Vec<AnyJson> = Vec::new();
    for entry in post_tool_use {
        // Only entries under our matcher are ours to edit. A foreign entry
        // with the same command string is the user's own hook and must
        // survive an uninstall untouched.
        if !has_our_matcher(entry) {
            remaining.push(AnyJson::Object(bridged(entry)));
            continue;
        }
        let entry_hooks = as_dictionary_array(member(entry, "hooks")).unwrap_or_default();
        let kept: Vec<AnyJson> = entry_hooks
            .iter()
            .filter(|hook| match as_string(member(hook, "command")) {
                None => true,
                Some(command) => !set_contains(&commands, command),
            })
            .map(|hook| AnyJson::Object(bridged(hook)))
            .collect();
        if kept.len() != entry_hooks.len() {
            changed = true;
        }
        if kept.is_empty() {
            continue;
        }
        let mut entry = bridged(entry);
        AnyJson::set(&mut entry, "hooks", AnyJson::Array(kept));
        remaining.push(AnyJson::Object(entry));
    }
    if !changed {
        return (settings.clone(), false);
    }

    if remaining.is_empty() {
        AnyJson::remove(&mut hooks, "PostToolUse");
    } else {
        AnyJson::set(&mut hooks, "PostToolUse", AnyJson::Array(remaining));
    }
    let mut settings_members = members(settings).to_vec();
    if hooks.is_empty() {
        AnyJson::remove(&mut settings_members, "hooks");
    } else {
        AnyJson::set(&mut settings_members, "hooks", AnyJson::Object(hooks));
    }
    (AnyJson::Object(settings_members), true)
}

/// Serialises a settings object back to disk-ready JSON.
///
/// Sorted keys and pretty printing are not cosmetic: this file is usually
/// under version control, and an unstable key order would make every hook
/// install produce a meaningless diff.
pub fn encode(settings: &AnyJson) -> Result<Vec<u8>, String> {
    let mut data = json_serialization::data(
        settings,
        WritingOptions { pretty_printed: true, sorted_keys: true, without_escaping_slashes: true, ..Default::default() },
    )?;
    data.push(0x0A);
    Ok(data)
}

/// The snippet printed by `down hook --print`, for anyone wiring an agent
/// that is not Claude Code, or who would rather paste it themselves.
pub fn hook_snippet(executable: &str) -> String {
    let object = AnyJson::Object(vec![(
        "hooks".into(),
        AnyJson::Object(vec![("PostToolUse".into(), AnyJson::Array(vec![hook_entry(executable)]))]),
    )]);
    let Ok(data) = encode(&object) else { return "{}".into() };
    let Ok(text) = String::from_utf8(data) else { return "{}".into() };
    swift_text::trimming(&text, swift_text::CharSet::Newlines).to_owned()
}
