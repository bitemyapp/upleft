//! Port of `Sources/DownrightApp/Support/AppPaths.swift`.
//!
//! Where Downright keeps its state. Unsandboxed (§3.4), so these are real
//! paths in Application Support rather than a container.

use objc2_foundation::{NSFileManager, NSSearchPathDirectory, NSSearchPathDomainMask};
use upleft_foundation::url::FileUrl;

pub const BUNDLE_IDENTIFIER: &str = "com.ezzy.downright";

/// `AppPaths.supportDirectory`.
pub fn support_directory() -> FileUrl {
    if let Ok(value) = std::env::var("DOWNRIGHT_SUPPORT_DIRECTORY")
        && !value.is_empty()
    {
        return FileUrl::from_path_is_directory(&value, true).standardized_file_url();
    }
    let manager = NSFileManager::defaultManager();
    let urls = manager.URLsForDirectory_inDomains(
        NSSearchPathDirectory::ApplicationSupportDirectory,
        NSSearchPathDomainMask::UserDomainMask,
    );
    let base = urls.firstObject().and_then(|url| FileUrl::from_nsurl(&url)).unwrap_or_else(|| {
        FileUrl::from_path(&objc2_foundation::NSHomeDirectory().to_string())
            .appending_path_component("Library/Application Support")
    });
    base.appending_path_component_is_directory("Downright", true)
}

/// Content-addressed snapshot store for local time-travel (§8.3).
pub fn history_directory() -> FileUrl {
    support_directory().appending_path_component_is_directory("history", true)
}

/// Per-document reading state: scroll position, mode, zoom, folds (§8.2).
pub fn state_directory() -> FileUrl {
    support_directory().appending_path_component_is_directory("state", true)
}

pub fn themes_directory() -> FileUrl {
    support_directory().appending_path_component_is_directory("Themes", true)
}

pub fn preferences_file() -> FileUrl {
    support_directory().appending_path_component("preferences.json")
}

pub fn keybindings_file() -> FileUrl {
    support_directory().appending_path_component("keybindings.json")
}

pub fn session_file() -> FileUrl {
    support_directory().appending_path_component("session.json")
}

/// What a support directory is for, in the words the user would use. When
/// the directory cannot be created the warning has to name the feature that
/// stops working, not the path that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Purpose {
    Support,
    History,
    State,
    Themes,
}

impl Purpose {
    /// `Purpose.allCases`.
    pub const ALL_CASES: [Purpose; 4] = [Purpose::Support, Purpose::History, Purpose::State, Purpose::Themes];

    pub fn directory(self) -> FileUrl {
        match self {
            Purpose::Support => support_directory(),
            Purpose::History => history_directory(),
            Purpose::State => state_directory(),
            Purpose::Themes => themes_directory(),
        }
    }

    /// Reads as the tail of "Downright can't save …".
    pub fn feature_description(self) -> &'static str {
        match self {
            Purpose::Support => "your settings, keyboard shortcuts, or the last session",
            Purpose::History => "version history, so change review has nothing to compare against",
            Purpose::State => "reading positions, folds, or the recent files list",
            Purpose::Themes => "imported themes",
        }
    }
}

/// A directory Downright could not create, and what that costs the user.
#[derive(Debug)]
pub struct PreparationFailure {
    pub purpose: Purpose,
    /// `error.localizedDescription`.
    pub error: String,
}

/// `FileManager.default.createDirectory(at:withIntermediateDirectories: true)`.
pub fn create(directory: &FileUrl) -> Result<(), String> {
    unsafe {
        NSFileManager::defaultManager().createDirectoryAtURL_withIntermediateDirectories_attributes_error(
            &directory.to_nsurl(),
            true,
            None,
        )
    }
    .map_err(|error| error.localizedDescription().to_string())
}

/// Best-effort creation for call sites that are about to write anyway and
/// will report their own write failure. Callers that need to know use
/// [`create`].
pub fn ensure(directory: FileUrl) -> FileUrl {
    let _ = create(&directory);
    directory
}

/// Creates every directory the app needs and reports the ones it could not.
/// A failure here disables a whole feature for the rest of the session, so
/// the caller is expected to say so rather than let it fail silently.
pub fn prepare_all() -> Vec<PreparationFailure> {
    Purpose::ALL_CASES
        .iter()
        .filter_map(|purpose| match create(&purpose.directory()) {
            Ok(()) => None,
            Err(error) => Some(PreparationFailure { purpose: *purpose, error }),
        })
        .collect()
}
