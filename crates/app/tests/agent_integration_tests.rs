//! Port of `Tests/DownrightAppTests/AgentIntegrationTests.swift`: the app
//! half of the agent hook.
//!
//! Every test here works against a temporary settings file. The ambient API
//! reads `~/.claude/settings.json`, and a suite that exercised it would be
//! editing the running user's real agent configuration.
//!
//! Not ported here: the three "Setup panel" tests
//! (`agentStepIsNotPreselected`, `everyStepExplainsItself`,
//! `agentStepNamesTheFileItEdits`). They check
//! `SetupWindowController.Step`, which belongs to the setup window's port
//! (`app/setup_window_controller.rs`), not to the integrations.

use std::path::PathBuf;

use upleft_app::integrations::agent_integration::{AgentIntegration, Failure};
use upleft_cli::agent_bridge;
use upleft_foundation::foundation_io;
use upleft_foundation::json_serialization::{self, AnyJson, ReadingOptions};
use upleft_foundation::url::FileUrl;

const EXECUTABLE: &str = "/usr/local/bin/down";

fn make_temporary_directory() -> PathBuf {
    let url = std::env::temp_dir().join(format!("downright-agent-{}", foundation_io::uuid_string()));
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

// MARK: - Location

/// User scope, not project scope: a project-scoped install would write into
/// a repository the user may not want it in, and may well commit.
#[test]
fn settings_path_is_user_scoped() {
    let path = AgentIntegration::settings_url().path();
    assert!(path.ends_with("/.claude/settings.json"));
    assert!(path.starts_with(&foundation_io::ns_home_directory()));
}

// MARK: - Round trip

#[test]
fn install_uninstall_round_trip() {
    let directory = make_temporary_directory();
    let _cleanup = Removing(directory.clone());
    let settings = url(&directory.join("settings.json"));
    let original = AnyJson::Object(vec![(
        "permissions".into(),
        AnyJson::Object(vec![("allow".into(), AnyJson::Array(vec![AnyJson::String("Bash(ls:*)".into())]))]),
    )]);
    std::fs::write(directory.join("settings.json"), agent_bridge::encode(&original).unwrap()).unwrap();

    assert!(!AgentIntegration::is_installed_at(&settings, Some(EXECUTABLE)));
    assert!(AgentIntegration::install_at(&settings, Some(EXECUTABLE)).unwrap());
    assert!(AgentIntegration::is_installed_at(&settings, Some(EXECUTABLE)));
    assert!(AgentIntegration::uninstall_at(&settings, Some(EXECUTABLE)).unwrap());
    assert!(!AgentIntegration::is_installed_at(&settings, Some(EXECUTABLE)));

    let after = json_serialization::json_object(&std::fs::read(directory.join("settings.json")).unwrap(), ReadingOptions::default())
        .unwrap();
    assert!(after.get("permissions").is_some());
    assert!(after.get("hooks").is_none());
}

#[test]
fn install_is_idempotent() {
    let directory = make_temporary_directory();
    let _cleanup = Removing(directory.clone());
    let settings = url(&directory.join("settings.json"));

    assert!(AgentIntegration::install_at(&settings, Some(EXECUTABLE)).unwrap());
    assert!(!AgentIntegration::install_at(&settings, Some(EXECUTABLE)).unwrap());
}

#[test]
fn uninstall_when_absent() {
    let directory = make_temporary_directory();
    let _cleanup = Removing(directory.clone());
    let settings = url(&directory.join("settings.json"));
    assert!(!AgentIntegration::uninstall_at(&settings, Some(EXECUTABLE)).unwrap());
}

/// Installing into a directory that does not exist yet is the common case:
/// a user who has never configured an agent has no `.claude` folder.
#[test]
fn creates_missing_directory() {
    let directory = make_temporary_directory();
    let _cleanup = Removing(directory.clone());
    let path = directory.join("nested/.claude/settings.json");
    let settings = FileUrl::from_path_is_directory(path.to_str().unwrap(), false);

    assert!(AgentIntegration::install_at(&settings, Some(EXECUTABLE)).unwrap());
    assert!(path.exists());
}

// MARK: - Refusals

/// The hook stores an absolute path to `down`; with no CLI there is nothing
/// to point at, and writing a hook that cannot run would be worse than
/// refusing.
#[test]
fn install_requires_the_command_line_tool() {
    let directory = make_temporary_directory();
    let _cleanup = Removing(directory.clone());
    let path = directory.join("settings.json");
    let settings = url(&path);

    assert_eq!(AgentIntegration::install_at(&settings, None), Err(Failure::CommandLineToolMissing));
    assert!(!path.exists());
    assert!(!AgentIntegration::is_installed_at(&settings, None));
}

/// Treating unreadable JSON as an empty object would silently replace a
/// config the user spent time on with one containing nothing but our hook.
#[test]
fn refuses_to_clobber_unparseable_settings() {
    let directory = make_temporary_directory();
    let _cleanup = Removing(directory.clone());
    let path = directory.join("settings.json");
    let damaged = "{ this is not json";
    std::fs::write(&path, damaged).unwrap();

    assert!(AgentIntegration::install_at(&url(&path), Some(EXECUTABLE)).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), damaged);
}

#[test]
fn empty_file_installs_cleanly() {
    let directory = make_temporary_directory();
    let _cleanup = Removing(directory.clone());
    let path = directory.join("settings.json");
    std::fs::write(&path, b"").unwrap();

    assert!(AgentIntegration::install_at(&url(&path), Some(EXECUTABLE)).unwrap());
    assert!(AgentIntegration::is_installed_at(&url(&path), Some(EXECUTABLE)));
}
