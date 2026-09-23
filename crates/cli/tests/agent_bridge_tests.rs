//! Port of `Tests/MarkdownCLITests/AgentBridgeTests.swift`: the suites
//! "Agent bridge", "Agent watcher planning" and "Agent command parsing".
//! Parameterized Swift tests loop over their arguments.
//!
//! The agent bridge is the one part of the CLI that runs inside somebody
//! else's tool, on somebody else's file, during somebody else's edit. These
//! tests hold the two properties that follow from that: it never mangles a
//! settings file it did not write, and it never reports a target it should
//! not open.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use upleft_cli::agent_bridge::{self, HookScope, TOOL_MATCHER};
use upleft_cli::agent_watcher::{AgentWatcher, Signature, StringSet, WatchPlan};
use upleft_cli::markdown_cli::{self, Action, HookMode};
use upleft_foundation::foundation_io;
use upleft_foundation::json_serialization::{self, AnyJson, ReadingOptions};
use upleft_foundation::url::FileUrl;

// MARK: - Helpers

fn s(text: &str) -> AnyJson {
    AnyJson::String(text.into())
}

fn object(members: &[(&str, AnyJson)]) -> AnyJson {
    AnyJson::Object(members.iter().map(|(key, value)| (key.to_string(), value.clone())).collect())
}

fn array(values: &[AnyJson]) -> AnyJson {
    AnyJson::Array(values.to_vec())
}

/// `["matcher": matcher, "hooks": [["type": "command", "command": command]]]`.
fn entry(matcher: &str, command: &str) -> AnyJson {
    object(&[("matcher", s(matcher)), ("hooks", array(&[object(&[("type", s("command")), ("command", s(command))])]))])
}

/// `settings["hooks"] as? [String: Any]`.
fn hooks(settings: &AnyJson) -> Option<&AnyJson> {
    settings.get("hooks").filter(|hooks| hooks.as_object().is_some())
}

/// `(settings["hooks"] as? [String: Any])?["PostToolUse"] as? [[String: Any]]`.
fn post_tool_use(settings: &AnyJson) -> Option<Vec<AnyJson>> {
    let values = hooks(settings)?.get("PostToolUse")?.as_array()?;
    values.iter().all(|value| value.as_object().is_some()).then(|| values.clone())
}

fn first_command(entry: &AnyJson) -> Option<String> {
    entry.get("hooks")?.as_array()?.first()?.get("command")?.as_str().map(str::to_owned)
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

// MARK: - Hook payloads

#[test]
fn extracts_file_path() {
    let payload = br#"{"hook_event_name":"PostToolUse","tool_name":"Write",
 "tool_input":{"file_path":"/tmp/notes.md","content":"hello"}}"#;
    assert_eq!(agent_bridge::hook_payload_paths(payload), strings(&["/tmp/notes.md"]));
}

#[test]
fn extracts_notebook_path() {
    let payload = br#"{"tool_input":{"notebook_path":"/tmp/a.ipynb"}}"#;
    assert_eq!(agent_bridge::hook_payload_paths(payload), strings(&["/tmp/a.ipynb"]));
}

#[test]
fn extracts_response_path() {
    let payload = br#"{"tool_response":{"filePath":"/tmp/out.md"}}"#;
    assert_eq!(agent_bridge::hook_payload_paths(payload), strings(&["/tmp/out.md"]));
}

#[test]
fn deduplicates_paths() {
    let payload = br#"{"tool_input":{"file_path":"/tmp/a.md"},"tool_response":{"filePath":"/tmp/a.md"}}"#;
    assert_eq!(agent_bridge::hook_payload_paths(payload), strings(&["/tmp/a.md"]));
}

/// A hook that throws blocks the agent's turn, so every malformed input has
/// to degrade to "nothing to do" rather than to an error.
#[test]
fn malformed_payloads_are_empty() {
    for raw in [
        "",
        "not json at all",
        "[]",
        "null",
        "{}",
        r#"{"tool_input":null}"#,
        r#"{"tool_input":{"file_path":""}}"#,
        r#"{"tool_input":{"file_path":123}}"#,
    ] {
        assert!(agent_bridge::hook_payload_paths(raw.as_bytes()).is_empty(), "{raw}");
    }
}

// MARK: - Target filtering

#[test]
fn filters_to_existing_markdown() {
    let present = ["/w/notes.md", "/w/main.swift", "/w/deleted.md"];
    let exists = |path: &str| path != "/w/deleted.md" && present.contains(&path);
    let targets = agent_bridge::openable_targets_with(
        &strings(&["/w/notes.md", "/w/main.swift", "/w/deleted.md", "/w/missing.md"]),
        exists,
    );
    assert_eq!(targets, strings(&["/w/notes.md"]));
}

#[test]
fn accepts_every_markdown_extension() {
    for extension in ["md", "markdown", "mdown", "mkd", "mdx", "mdc", "qmd", "rmd"] {
        let path = format!("/w/doc.{extension}");
        assert_eq!(agent_bridge::openable_targets_with(&[path.clone()], |_| true), vec![path]);
    }
}

#[test]
fn standardises_paths() {
    let targets = agent_bridge::openable_targets_with(&strings(&["/w/./sub/../notes.md"]), |_| true);
    assert_eq!(targets, strings(&["/w/notes.md"]));
}

// MARK: - Settings merge

#[test]
fn installs_into_empty_settings() {
    let (settings, changed) = agent_bridge::installing_hook(&object(&[]), "/usr/local/bin/down");
    assert!(changed);
    let post_tool_use = post_tool_use(&settings).unwrap();
    assert_eq!(post_tool_use.len(), 1);
    assert_eq!(post_tool_use[0].get("matcher").and_then(AnyJson::as_str), Some(TOOL_MATCHER));
}

/// The file belongs to the user, not to us. Unrelated keys and unrelated
/// hooks have to survive an install untouched.
#[test]
fn install_preserves_foreign_content() {
    let existing = object(&[
        ("permissions", object(&[("allow", array(&[s("Bash(ls:*)")]))])),
        (
            "hooks",
            object(&[
                ("PostToolUse", array(&[entry("Bash", "echo mine")])),
                ("PreToolUse", array(&[entry("Read", "echo pre")])),
            ]),
        ),
    ]);
    let (settings, changed) = agent_bridge::installing_hook(&existing, "down");
    assert!(changed);
    assert_eq!(settings.get("permissions"), Some(&object(&[("allow", array(&[s("Bash(ls:*)")]))])));
    let hooks = hooks(&settings).unwrap();
    assert_eq!(hooks.get("PreToolUse").and_then(AnyJson::as_array).map(Vec::len), Some(1));
    let post_tool_use = post_tool_use(&settings).unwrap();
    assert_eq!(post_tool_use.len(), 2);
    assert_eq!(post_tool_use[0].get("matcher").and_then(AnyJson::as_str), Some("Bash"));
}

#[test]
fn reports_installed_state() {
    assert!(!agent_bridge::is_hook_installed(&object(&[]), "down"));
    let installed = agent_bridge::installing_hook(&object(&[]), "down").0;
    assert!(agent_bridge::is_hook_installed(&installed, "down"));
    // A hook installed from a different location is a different hook.
    assert!(!agent_bridge::is_hook_installed(&installed, "/opt/homebrew/bin/down"));
    let removed = agent_bridge::removing_hook(&installed, "down").0;
    assert!(!agent_bridge::is_hook_installed(&removed, "down"));
}

#[test]
fn install_is_idempotent() {
    let first = agent_bridge::installing_hook(&object(&[]), "down");
    let second = agent_bridge::installing_hook(&first.0, "down");
    assert!(first.1);
    assert!(!second.1);
    assert_eq!(post_tool_use(&second.0).map(|entries| entries.len()), Some(1));
}

/// Two installs from different locations are two different commands, so
/// the idempotence check keys on the command rather than on the matcher.
#[test]
fn different_executable_is_distinct() {
    let first = agent_bridge::installing_hook(&object(&[]), "/usr/local/bin/down");
    let second = agent_bridge::installing_hook(&first.0, "/opt/homebrew/bin/down");
    assert!(second.1);
    assert_eq!(post_tool_use(&second.0).map(|entries| entries.len()), Some(2));
}

/// Our hook is identified by its matcher *and* its command. A foreign entry
/// that runs `down notify` under another matcher is the user's own hook, so
/// it must not count as installed and must not block our install.
#[test]
fn foreign_matcher_with_same_command_does_not_block_install() {
    let foreign = object(&[("hooks", object(&[("PostToolUse", array(&[entry("Bash", "down notify")]))]))]);
    assert!(!agent_bridge::is_hook_installed(&foreign, "down"));
    let (settings, changed) = agent_bridge::installing_hook(&foreign, "down");
    assert!(changed);
    assert_eq!(post_tool_use(&settings).map(|entries| entries.len()), Some(2));
    assert!(agent_bridge::is_hook_installed(&settings, "down"));
}

#[test]
fn uninstall_preserves_foreign_matcher_with_same_command() {
    let foreign = object(&[("hooks", object(&[("PostToolUse", array(&[entry("Bash", "down notify")]))]))]);
    let installed = agent_bridge::installing_hook(&foreign, "down").0;
    let (settings, changed) = agent_bridge::removing_hook(&installed, "down");
    assert!(changed);
    let post_tool_use = post_tool_use(&settings).unwrap();
    assert_eq!(post_tool_use.len(), 1);
    assert_eq!(post_tool_use[0].get("matcher").and_then(AnyJson::as_str), Some("Bash"));
    assert_eq!(first_command(&post_tool_use[0]).as_deref(), Some("down notify"));
}

#[test]
fn uninstall_removes_only_ours() {
    let seeded = object(&[("hooks", object(&[("PostToolUse", array(&[entry("Bash", "echo mine")]))]))]);
    let installed = agent_bridge::installing_hook(&seeded, "down");
    let (settings, changed) = agent_bridge::removing_hook(&installed.0, "down");
    assert!(changed);
    let post_tool_use = post_tool_use(&settings).unwrap();
    assert_eq!(post_tool_use.len(), 1);
    assert_eq!(post_tool_use[0].get("matcher").and_then(AnyJson::as_str), Some("Bash"));
}

/// Uninstalling should leave no trace — an empty `PostToolUse` array or an
/// empty `hooks` object is debris the user would have to clean up by hand.
#[test]
fn uninstall_prunes_empty_containers() {
    let installed = agent_bridge::installing_hook(&object(&[("permissions", object(&[("allow", array(&[]))]))]), "down");
    let (settings, changed) = agent_bridge::removing_hook(&installed.0, "down");
    assert!(changed);
    assert!(settings.get("hooks").is_none());
    assert!(settings.get("permissions").is_some());
}

#[test]
fn uninstall_is_safe_when_absent() {
    assert!(!agent_bridge::removing_hook(&object(&[]), "down").1);
    let foreign = object(&[("hooks", object(&[("PostToolUse", array(&[entry("Bash", "other")]))]))]);
    assert!(!agent_bridge::removing_hook(&foreign, "down").1);
}

/// This file is usually in version control, so an unstable key order would
/// turn every install into a noisy diff.
#[test]
fn encoding_is_stable() {
    let settings = agent_bridge::installing_hook(&object(&[]), "down").0;
    let first = agent_bridge::encode(&settings).unwrap();
    let second = agent_bridge::encode(&settings).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.last(), Some(&0x0A));
    let text = String::from_utf8(first).unwrap();
    assert!(!text.contains("\\/"));
}

#[test]
fn snippet_is_valid_json() {
    let snippet = agent_bridge::hook_snippet("/usr/local/bin/down");
    let object = json_serialization::json_object(snippet.as_bytes(), ReadingOptions::default()).unwrap();
    let post_tool_use = post_tool_use(&object).unwrap();
    let command = first_command(&post_tool_use[0]);
    // The executable is POSIX single-quoted so no character in an unusual
    // install path can open a shell expansion context.
    assert_eq!(command.as_deref(), Some("'/usr/local/bin/down' notify"));
}

#[test]
fn hook_command_quotes_hostile_paths() {
    // Inside POSIX single quotes nothing expands: `$`, backticks, double
    // quotes, and backslashes are all literal. The only character that
    // needs care is the single quote itself, escaped the standard way.
    let hostile = "/opt/My Tools/\"$(touch /tmp/pwned)\"/down";
    let command = agent_bridge::hook_command(hostile);
    assert!(command.starts_with("'/opt/My Tools/"));
    assert!(command.ends_with("/down' notify"));
    assert!(command.contains("\"$(touch /tmp/pwned)\""), "the payload must survive verbatim inside the quotes");

    let tricky = "/opt/o'brien/down";
    assert_eq!(agent_bridge::hook_command(tricky), "'/opt/o'\\''brien/down' notify");
}

/// Hooks written by earlier builds used unquoted or double-quoted paths. An
/// upgrade must recognize them as ours — reporting state accurately,
/// migrating them to the canonical quoted form instead of appending a
/// duplicate that would fire twice per event, and removing them cleanly.
#[test]
fn legacy_hook_is_recognized_and_migrated() {
    let executable = "/usr/local/bin/down";
    for legacy_command in ["/usr/local/bin/down notify", "\"/usr/local/bin/down\" notify"] {
        let seeded = object(&[("hooks", object(&[("PostToolUse", array(&[entry(TOOL_MATCHER, legacy_command)]))]))]);
        // State reporting counts the legacy hook as installed.
        assert!(agent_bridge::is_hook_installed(&seeded, executable));

        // Installing migrates it in place rather than appending a twin.
        let (migrated, changed) = agent_bridge::installing_hook(&seeded, executable);
        assert!(changed);
        let post_tool_use = post_tool_use(&migrated).unwrap();
        assert_eq!(post_tool_use.len(), 1);
        assert_eq!(first_command(&post_tool_use[0]).as_deref(), Some("'/usr/local/bin/down' notify"));

        // A second install is a no-op, and an uninstall removes it.
        assert!(!agent_bridge::installing_hook(&migrated, executable).1);
        let (removed, removed_changed) = agent_bridge::removing_hook(&migrated, executable);
        assert!(removed_changed);
        assert!(removed.get("hooks").is_none());
    }
}

#[test]
fn scope_resolves_settings_path() {
    let home = FileUrl::from_path("/Users/x");
    let project = FileUrl::from_path("/w/proj");
    assert_eq!(HookScope::User.settings_url(&home, &project).path(), "/Users/x/.claude/settings.json");
    assert_eq!(HookScope::Project.settings_url(&home, &project).path(), "/w/proj/.claude/settings.json");
}

// MARK: - Agent watcher planning
//
// The watcher's planning step decides what FSEvents is pointed at, which is
// the part that goes wrong silently — a watch on the wrong path simply
// never fires.

fn make_temporary_directory() -> PathBuf {
    let url = std::env::temp_dir().join(format!("downright-watch-{}", foundation_io::uuid_string()));
    std::fs::create_dir_all(&url).unwrap();
    url
}

struct Removing(PathBuf);

impl Drop for Removing {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn url(path: &std::path::Path) -> FileUrl {
    FileUrl::from_path(path.to_str().unwrap())
}

/// Agents write atomically — temp file, then rename over the target — which
/// unlinks the inode any file-level watch was holding. A file root must
/// therefore resolve to its parent directory.
#[test]
fn file_root_watches_parent() {
    let directory = make_temporary_directory();
    let _cleanup = Removing(directory.clone());
    let file = directory.join("notes.md");
    std::fs::write(&file, "# hi").unwrap();

    let plan = AgentWatcher::plan(&[url(&file)]);
    assert_eq!(plan.directories, vec![url(&directory).standardized_file_url().path()]);
    assert_eq!(plan.allowed_files, StringSet::from([url(&file).standardized_file_url().path().as_str()]));
}

#[test]
fn directory_root_watches_itself() {
    let directory = make_temporary_directory();
    let _cleanup = Removing(directory.clone());

    let plan = AgentWatcher::plan(&[url(&directory)]);
    assert_eq!(plan.directories, vec![url(&directory).standardized_file_url().path()]);
    assert!(plan.allowed_files.is_empty());
}

/// Agents create files as well as rewrite them, so a target that does not
/// exist yet still has to be watched.
#[test]
fn missing_file_still_plans() {
    let directory = make_temporary_directory();
    let _cleanup = Removing(directory.clone());
    let file = directory.join("not-written-yet.md");

    let plan = AgentWatcher::plan(&[url(&file)]);
    assert_eq!(plan.directories, vec![url(&directory).standardized_file_url().path()]);
    assert_eq!(plan.allowed_files, StringSet::from([url(&file).standardized_file_url().path().as_str()]));
}

#[test]
fn duplicate_roots_collapse() {
    let directory = make_temporary_directory();
    let _cleanup = Removing(directory.clone());
    let a = directory.join("a.md");
    let b = directory.join("b.md");

    let plan = AgentWatcher::plan(&[url(&a), url(&b)]);
    assert_eq!(plan.directories.len(), 1);
    assert_eq!(plan.allowed_files.len(), 2);
}

/// Mixing a directory root with a file root must not let the file
/// allow-list suppress everything the directory asked for.
#[test]
fn directory_root_subsumes_file_root() {
    let directory = make_temporary_directory();
    let _cleanup = Removing(directory.clone());
    let file = directory.join("one.md");

    let plan = AgentWatcher::plan(&[url(&directory), url(&file)]);
    assert!(plan.allowed_files.is_empty());
    assert!(AgentWatcher::accepts(directory.join("other.md").to_str().unwrap(), &plan));
}

/// A folder of agent output churns constantly with lockfiles, build
/// artefacts and `.DS_Store`; only Markdown should wake the app.
#[test]
fn rejects_non_markdown() {
    let plan = WatchPlan { directories: strings(&["/w"]), allowed_files: StringSet::new() };
    assert!(AgentWatcher::accepts("/w/notes.md", &plan));
    assert!(AgentWatcher::accepts("/w/deep/nested/spec.mdx", &plan));
    assert!(!AgentWatcher::accepts("/w/.DS_Store", &plan));
    assert!(!AgentWatcher::accepts("/w/main.swift", &plan));
    assert!(!AgentWatcher::accepts("/w/package-lock.json", &plan));
}

#[test]
fn allow_list_rejects_siblings() {
    let plan = WatchPlan { directories: strings(&["/w"]), allowed_files: StringSet::from(["/w/notes.md"]) };
    assert!(AgentWatcher::accepts("/w/notes.md", &plan));
    assert!(!AgentWatcher::accepts("/w/other.md", &plan));
}

/// The signature is the guard against a feedback loop: opening a document
/// updates the file's metadata, macOS reports that as a fresh event on the
/// same path, and a watcher that trusted the event would open it again —
/// forever. Metadata must not move the signature.
#[test]
fn metadata_touch_leaves_signature_alone() {
    let directory = make_temporary_directory();
    let _cleanup = Removing(directory.clone());
    let file = directory.join("notes.md");
    std::fs::write(&file, "# hi").unwrap();

    let before = Signature::new(file.to_str().unwrap()).unwrap();
    // Reading the document and restamping its metadata is what launching
    // the app does; both move atime/ctime and neither is a content change.
    let _ = std::fs::read(&file).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let after = Signature::new(file.to_str().unwrap()).unwrap();

    assert_eq!(before, after);
}

#[test]
fn rewrite_changes_signature() {
    let directory = make_temporary_directory();
    let _cleanup = Removing(directory.clone());
    let file = directory.join("notes.md");
    std::fs::write(&file, "# hi").unwrap();
    let before = Signature::new(file.to_str().unwrap()).unwrap();

    std::fs::write(&file, "# hi, at a different length").unwrap();
    let after = Signature::new(file.to_str().unwrap()).unwrap();

    assert_ne!(before, after);
}

#[test]
fn missing_file_has_no_signature() {
    assert_eq!(Signature::new("/nowhere/at/all/ghost.md"), None);
}

// MARK: - Agent command parsing
//
// Parsing lives in the library so the agent surface can be exercised
// without spawning a process, watching a folder, or launching the app.

fn parse(values: &[&str]) -> Action {
    markdown_cli::parse(&strings(values)).unwrap()
}

#[test]
fn notify_defaults() {
    let Action::Notify(options) = parse(&["notify"]) else { panic!("expected notify") };
    assert!(!options.focus);
    assert!(!options.dry_run);
}

#[test]
fn notify_flags() {
    let Action::Notify(options) = parse(&["notify", "--focus", "--dry-run"]) else { panic!("expected notify") };
    assert!(options.focus);
    assert!(options.dry_run);
}

#[test]
fn watch_defaults() {
    let Action::Watch(options, paths) = parse(&["watch"]) else { panic!("expected watch") };
    assert!(paths.is_empty());
    assert_eq!(options.debounce, AgentWatcher::DEFAULT_DEBOUNCE);
    assert!(!options.focus);
}

#[test]
fn watch_debounce() {
    let Action::Watch(options, paths) = parse(&["watch", "--debounce", "750", "Docs"]) else { panic!("expected watch") };
    assert_eq!(options.debounce, 0.75);
    assert_eq!(paths, strings(&["Docs"]));
}

#[test]
fn watch_rejects_bad_debounce() {
    assert!(markdown_cli::parse(&strings(&["watch", "--debounce", "soon"])).is_err());
}

#[test]
fn watch_requires_debounce_value() {
    assert_eq!(
        markdown_cli::parse(&strings(&["watch", "--debounce"])),
        Err(markdown_cli::ParseError::MissingValue("--debounce".into()))
    );
}

#[test]
fn watch_honours_separator() {
    let Action::Watch(_, paths) = parse(&["watch", "--", "--weird-name.md"]) else { panic!("expected watch") };
    assert_eq!(paths, strings(&["--weird-name.md"]));
}

#[test]
fn hook_defaults() {
    let Action::Hook(options) = parse(&["hook"]) else { panic!("expected hook") };
    assert_eq!(options.mode, HookMode::Print);
    assert_eq!(options.scope, HookScope::Project);
}

#[test]
fn hook_flags() {
    let cases: [(&[&str], HookMode, HookScope); 4] = [
        (&["hook", "--install"], HookMode::Install, HookScope::Project),
        (&["hook", "--uninstall"], HookMode::Uninstall, HookScope::Project),
        (&["hook", "--install", "--scope", "user"], HookMode::Install, HookScope::User),
        (&["hook", "--uninstall", "--scope", "USER"], HookMode::Uninstall, HookScope::User),
    ];
    for (argv, mode, scope) in cases {
        let Action::Hook(options) = parse(argv) else { panic!("expected hook") };
        assert_eq!(options.mode, mode);
        assert_eq!(options.scope, scope);
    }
}

#[test]
fn hook_rejects_unknown_scope() {
    assert!(markdown_cli::parse(&strings(&["hook", "--scope", "global"])).is_err());
}

#[test]
fn usage_documents_agent_commands() {
    let usage = markdown_cli::usage();
    assert!(usage.contains("notify"));
    assert!(usage.contains("watch"));
    assert!(usage.contains("hook"));
}

/// `down README.md` must keep meaning "open this file", so the new
/// subcommand names must not shadow a real path.
#[test]
fn file_argument_still_opens() {
    let Action::Open(_, paths) = parse(&["README.md"]) else { panic!("expected open") };
    assert_eq!(paths, strings(&["README.md"]));
}
