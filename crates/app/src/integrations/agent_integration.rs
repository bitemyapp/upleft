//! Port of `Sources/DownrightApp/Integrations/AgentIntegration.swift`: the
//! app's view of the agent hook that `down notify` installs.
//!
//! State lives in the agent's own `settings.json`, never in our preferences.
//! That file is the thing the agent actually reads, and a user who edits it
//! by hand — or a project that checks one in — must not find the app
//! disagreeing with reality. Every accessor re-reads it, so a Settings row
//! states what is true at the moment it is drawn, matching how the System
//! Integration rows already behave.
//!
//! Every operation has an explicit-location form (`*_at`) underneath the
//! ambient one. Without it the only way to exercise this type is to write
//! to the running user's real `~/.claude/settings.json`, which is not
//! something a test suite may do — so the ambient API stays a one-line
//! convenience and the logic is tested against a temporary directory.
//!
//! These calls read and write a small file; callers on the main thread
//! should run them on a worker.

use upleft_cli::agent_bridge::{self, HookScope};
use upleft_foundation::foundation_io;
use upleft_foundation::json_serialization::{self, AnyJson, ReadingOptions};
use upleft_foundation::url::{self, FileUrl};

/// `AgentIntegration.Failure`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    CommandLineToolMissing,
    CannotWrite(String),
}

impl Failure {
    /// `errorDescription`.
    pub fn error_description(&self) -> String {
        match self {
            Failure::CommandLineToolMissing => "Install the down command line tool first — the hook runs it.".into(),
            Failure::CannotWrite(reason) => reason.clone(),
        }
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.error_description())
    }
}

impl std::error::Error for Failure {}

/// `AgentIntegration`.
pub struct AgentIntegration;

impl AgentIntegration {
    /// Where the app installs the hook.
    ///
    /// User scope, deliberately. A project-scoped install would write into a
    /// repository the user may not want it in, and may well commit — a
    /// checkbox in a setup panel is not consent to modify somebody's version
    /// control.
    pub fn settings_url() -> FileUrl {
        HookScope::User.settings_url(
            &FileUrl::from_path(&foundation_io::ns_home_directory()),
            &FileUrl::from_path(&url::current_directory_path()),
        )
    }

    /// The `down` the hook should invoke.
    ///
    /// A hook does not inherit an interactive shell's `PATH`, so an
    /// unqualified `down` can resolve when the user tests it in Terminal and
    /// then silently fail inside the agent. Only an absolute path is
    /// trustworthy here.
    pub fn executable_path() -> Option<String> {
        ["/usr/local/bin/down", "/opt/homebrew/bin/down"]
            .into_iter()
            .find(|path| foundation_io::file_exists(path))
            .map(str::to_owned)
    }

    /// True when the agent's settings already run our hook.
    pub fn is_installed() -> bool {
        Self::is_installed_at(&Self::settings_url(), Self::executable_path().as_deref())
    }

    pub fn is_installed_at(settings_url: &FileUrl, executable: Option<&str>) -> bool {
        let Some(executable) = executable else { return false };
        agent_bridge::is_hook_installed(&Self::load_settings(settings_url), executable)
    }

    /// Returns whether anything changed. Installing over an existing hook is
    /// a no-op rather than a duplicate.
    pub fn install() -> Result<bool, Failure> {
        Self::install_at(&Self::settings_url(), Self::executable_path().as_deref())
    }

    pub fn install_at(settings_url: &FileUrl, executable: Option<&str>) -> Result<bool, Failure> {
        let Some(executable) = executable else { return Err(Failure::CommandLineToolMissing) };
        let (settings, changed) = agent_bridge::installing_hook(&Self::load_settings(settings_url), executable);
        if !changed {
            return Ok(false);
        }
        Self::write(&settings, settings_url)?;
        Ok(true)
    }

    pub fn uninstall() -> Result<bool, Failure> {
        Self::uninstall_at(&Self::settings_url(), Self::executable_path().as_deref())
    }

    pub fn uninstall_at(settings_url: &FileUrl, executable: Option<&str>) -> Result<bool, Failure> {
        let Some(executable) = executable else { return Ok(false) };
        let (settings, changed) = agent_bridge::removing_hook(&Self::load_settings(settings_url), executable);
        if !changed {
            return Ok(false);
        }
        Self::write(&settings, settings_url)?;
        Ok(true)
    }

    /// A settings file we cannot parse is treated as absent for reading, but
    /// `write` refuses to clobber it — see below.
    fn load_settings(url: &FileUrl) -> AnyJson {
        let Some(data) = foundation_io::data_contents_of(url) else { return AnyJson::Object(Vec::new()) };
        match json_serialization::json_object(&data, ReadingOptions::default()) {
            Ok(AnyJson::Object(members)) => AnyJson::Object(agent_bridge::bridged(&members)),
            _ => AnyJson::Object(Vec::new()),
        }
    }

    fn write(settings: &AnyJson, url: &FileUrl) -> Result<(), Failure> {
        // Refuse to write over a file that exists but did not parse. Treating
        // unreadable JSON as an empty object would silently replace a config
        // the user spent time on with one containing nothing but our hook.
        if let Some(data) = foundation_io::data_contents_of(url)
            && !data.is_empty()
            && json_serialization::json_object(&data, ReadingOptions::default()).is_err()
        {
            return Err(Failure::CannotWrite(format!("{} is not valid JSON. Fix or move it, then try again.", url.path())));
        }
        foundation_io::create_directory(&url.deleting_last_path_component(), true)
            .map_err(|error| Failure::CannotWrite(error.description))?;
        let data = agent_bridge::encode(settings).map_err(Failure::CannotWrite)?;
        foundation_io::write_atomically(&data, url).map_err(|error| Failure::CannotWrite(error.description))
    }
}
