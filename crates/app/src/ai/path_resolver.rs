//! Port of `Sources/DownrightApp/AI/PathResolver.swift`.
//!
//! Live path resolution (§8.4).
//!
//! Agent docs are dense with file references, and the interesting case is not
//! the convenience of clicking one — it is the *other* case. A completion
//! summary that claims to have touched `src/auth/session.ts` gets a dotted red
//! underline when that file isn't there.
//!
//! Requires being unsandboxed (§3.4): resolution walks up to the git root and
//! stats arbitrary paths with no file-picker ritual. [`PathResolver::warm`]
//! stats on a background queue and calls back on the main queue, as in Swift.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use dispatch2::{DispatchQoS, DispatchQueue, DispatchQueueAttr, DispatchRetained, GlobalQueueIdentifier};
use objc2::AllocAnyThread;
use objc2::rc::Retained;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSAppleScript, NSString, NSURL, NSURLComponents};
use upleft_core::model::PathToken;
use upleft_foundation::file_manager;
use upleft_foundation::url::{self, FileUrl};

use crate::app::document_types;

/// `PathResolver.Resolution`.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolution {
    pub url: Option<FileUrl>,
    pub exists: bool,
    pub is_directory: bool,
    pub line: Option<isize>,
}

struct CacheState {
    cache: HashMap<String, Resolution>,
    generation: u64,
}

struct Inner {
    /// Directory of the document being read.
    document_directory: FileUrl,
    /// Nearest enclosing git root, if any — the second search base.
    git_root: Option<FileUrl>,
    lock: Mutex<CacheState>,
    warm_queue: DispatchRetained<DispatchQueue>,
}

/// `PathResolver`. Cloning shares the resolver, like a Swift class reference.
#[derive(Clone)]
pub struct PathResolver {
    inner: Arc<Inner>,
}

impl PathResolver {
    /// `init(documentURL:)`.
    pub fn new(document_url: Option<&FileUrl>) -> PathResolver {
        let directory = match document_url {
            Some(url) => url.deleting_last_path_component(),
            None => FileUrl::from_path(&url::current_directory_path()),
        };
        let git_root = PathResolver::find_git_root(&directory);
        let user_initiated =
            DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(DispatchQoS::UserInitiated));
        PathResolver {
            inner: Arc::new(Inner {
                document_directory: directory,
                git_root,
                lock: Mutex::new(CacheState { cache: HashMap::new(), generation: 0 }),
                warm_queue: DispatchQueue::new_with_target(
                    "com.ezzy.downright.path-resolve",
                    DispatchQueueAttr::SERIAL,
                    Some(&user_initiated),
                ),
            }),
        }
    }

    /// Invalidate after an external write — a file the agent just created
    /// should stop being underlined in red without reopening the document.
    pub fn invalidate(&self) {
        let mut state = self.inner.lock.lock().unwrap();
        state.cache.clear();
        state.generation = state.generation.wrapping_add(1);
    }

    /// Returns only an answer already in memory. Decoration uses this so a
    /// cache miss stays visually neutral while background warming stats the
    /// file system.
    pub fn cached_resolution(&self, token: &PathToken) -> Option<Resolution> {
        let state = self.inner.lock.lock().unwrap();
        let hit = state.cache.get(&token.raw_path)?;
        Some(Resolution { url: hit.url.clone(), exists: hit.exists, is_directory: hit.is_directory, line: token.line })
    }

    /// Resolves unique path tokens off the main thread, then returns to the
    /// main queue so the document can repaint from the warmed cache.
    pub fn warm(&self, tokens: &[PathToken], completion: impl FnOnce() + Send + 'static) {
        if tokens.is_empty() {
            completion();
            return;
        }
        let (requested_generation, misses) = {
            let state = self.inner.lock.lock().unwrap();
            let mut seen: HashSet<&str> = HashSet::new();
            let misses: Vec<PathToken> = tokens
                .iter()
                .filter(|token| seen.insert(token.raw_path.as_str()) && !state.cache.contains_key(&token.raw_path))
                .cloned()
                .collect();
            (state.generation, misses)
        };
        if misses.is_empty() {
            completion();
            return;
        }
        let inner = Arc::clone(&self.inner);
        self.inner.warm_queue.exec_async(move || {
            let resolutions: Vec<(String, Resolution)> =
                misses.iter().map(|token| (token.raw_path.clone(), inner.compute_resolution(token))).collect();
            let accepted = {
                let mut state = inner.lock.lock().unwrap();
                if state.generation != requested_generation {
                    false
                } else {
                    for (path, resolution) in resolutions {
                        state.cache.insert(path, resolution);
                    }
                    true
                }
            };
            if !accepted {
                return;
            }
            DispatchQueue::main().exec_async(completion);
        });
    }

    pub fn resolve(&self, token: &PathToken) -> Resolution {
        {
            let state = self.inner.lock.lock().unwrap();
            if let Some(hit) = state.cache.get(&token.raw_path) {
                return Resolution {
                    url: hit.url.clone(),
                    exists: hit.exists,
                    is_directory: hit.is_directory,
                    line: token.line,
                };
            }
        }
        let resolution = self.inner.compute_resolution(token);
        self.inner.lock.lock().unwrap().cache.insert(token.raw_path.clone(), resolution.clone());
        resolution
    }

    pub fn find_git_root(directory: &FileUrl) -> Option<FileUrl> {
        let mut current = directory.standardized_file_url();
        for _ in 0..40 {
            if file_manager::file_exists(&current.appending_path_component(".git").path()) {
                return Some(current);
            }
            let parent = current.deleting_last_path_component();
            if parent.path() == current.path() {
                break;
            }
            current = parent;
        }
        None
    }

    /// The directory the resolver searches first.
    pub fn document_directory(&self) -> &FileUrl {
        &self.inner.document_directory
    }

    pub fn git_root(&self) -> Option<&FileUrl> {
        self.inner.git_root.as_ref()
    }
}

impl Inner {
    fn compute_resolution(&self, token: &PathToken) -> Resolution {
        let raw = &token.raw_path;
        let mut candidates: Vec<FileUrl> = Vec::new();

        if upleft_swift_text::has_prefix(raw, "/") {
            candidates.push(FileUrl::from_path(raw));
        } else if upleft_swift_text::has_prefix(raw, "~") {
            candidates.push(FileUrl::from_path(&url::expanding_tilde_in_path(raw)));
        } else {
            candidates.push(self.document_directory.appending_path_component(raw));
            if let Some(git_root) = &self.git_root {
                candidates.push(git_root.appending_path_component(raw));
                // Agents habitually write paths relative to the repo root even
                // in a doc that lives in `docs/`, so also try the parent.
                candidates.push(git_root.appending_path_component("src").appending_path_component(raw));
            }
            candidates.push(self.document_directory.deleting_last_path_component().appending_path_component(raw));
        }

        for candidate in &candidates {
            let standardized = candidate.standardized_file_url();
            if let Some(is_directory) = file_manager::file_exists_is_directory(&standardized.path()) {
                return Resolution { url: Some(standardized), exists: true, is_directory, line: token.line };
            }
        }
        Resolution {
            url: candidates.first().map(FileUrl::standardized_file_url),
            exists: false,
            is_directory: false,
            line: token.line,
        }
    }
}

// MARK: - Opening in an editor

/// `ExternalEditor`: the editors §8.4 names, plus `$EDITOR` as the escape
/// hatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExternalEditor {
    Vscode,
    Cursor,
    Zed,
    Xcode,
    Sublime,
    Bbedit,
    Nova,
    SystemDefault,
    EnvEditor,
}

impl ExternalEditor {
    /// `ExternalEditor.allCases`.
    pub const ALL_CASES: [ExternalEditor; 9] = [
        ExternalEditor::Vscode,
        ExternalEditor::Cursor,
        ExternalEditor::Zed,
        ExternalEditor::Xcode,
        ExternalEditor::Sublime,
        ExternalEditor::Bbedit,
        ExternalEditor::Nova,
        ExternalEditor::SystemDefault,
        ExternalEditor::EnvEditor,
    ];

    pub fn raw_value(self) -> &'static str {
        match self {
            ExternalEditor::Vscode => "vscode",
            ExternalEditor::Cursor => "cursor",
            ExternalEditor::Zed => "zed",
            ExternalEditor::Xcode => "xcode",
            ExternalEditor::Sublime => "sublime",
            ExternalEditor::Bbedit => "bbedit",
            ExternalEditor::Nova => "nova",
            ExternalEditor::SystemDefault => "systemDefault",
            ExternalEditor::EnvEditor => "envEditor",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<ExternalEditor> {
        ExternalEditor::ALL_CASES.into_iter().find(|editor| editor.raw_value() == raw)
    }

    pub fn title(self) -> &'static str {
        match self {
            ExternalEditor::Vscode => "Visual Studio Code",
            ExternalEditor::Cursor => "Cursor",
            ExternalEditor::Zed => "Zed",
            ExternalEditor::Xcode => "Xcode",
            ExternalEditor::Sublime => "Sublime Text",
            ExternalEditor::Bbedit => "BBEdit",
            ExternalEditor::Nova => "Nova",
            ExternalEditor::SystemDefault => "System Default",
            ExternalEditor::EnvEditor => "$EDITOR (Terminal)",
        }
    }

    pub fn bundle_identifier(self) -> Option<&'static str> {
        match self {
            ExternalEditor::Vscode => Some("com.microsoft.VSCode"),
            ExternalEditor::Cursor => Some("com.todesktop.230313mzl4w4u92"),
            ExternalEditor::Zed => Some("dev.zed.Zed"),
            ExternalEditor::Xcode => Some("com.apple.dt.Xcode"),
            ExternalEditor::Sublime => Some("com.sublimetext.4"),
            ExternalEditor::Bbedit => Some("com.barebones.bbedit"),
            ExternalEditor::Nova => Some("com.panic.Nova"),
            ExternalEditor::SystemDefault | ExternalEditor::EnvEditor => None,
        }
    }

    pub fn is_installed(self) -> bool {
        let Some(id) = self.bundle_identifier() else {
            return true;
        };
        NSWorkspace::sharedWorkspace().URLForApplicationWithBundleIdentifier(&NSString::from_str(id)).is_some()
    }

    /// Editors that can be handed a line number via their URL scheme.
    ///
    /// Assigning `URLComponents.path` percent-encodes exactly the characters a
    /// path may not contain and leaves `/` and `:` alone, so the trailing
    /// `:42` the editors parse survives. (`NSURLComponents` encodes the same
    /// set; probed over every printable ASCII character.)
    pub fn url(self, file: &FileUrl, line: Option<isize>) -> Option<Retained<NSURL>> {
        let scheme = match self {
            ExternalEditor::Vscode => "vscode",
            ExternalEditor::Cursor => "cursor",
            ExternalEditor::Zed => "zed",
            _ => return None,
        };
        let components = NSURLComponents::new();
        components.setScheme(Some(&NSString::from_str(scheme)));
        components.setHost(Some(&NSString::from_str("file")));
        let path = file.path() + &line.map(|line| format!(":{line}")).unwrap_or_default();
        components.setPath(Some(&NSString::from_str(&path)));
        components.URL()
    }

    pub fn open(self, file: &FileUrl, line: Option<isize>) {
        let workspace = NSWorkspace::sharedWorkspace();
        // Every editor path below means "edit this file", never "run it".
        if document_types::executes_when_opened(file) {
            workspace.selectFile_inFileViewerRootedAtPath(
                Some(&NSString::from_str(&file.path())),
                &NSString::from_str(&file.deleting_last_path_component().path()),
            );
            return;
        }
        if let Some(scheme_url) = self.url(file, line) {
            workspace.openURL(&scheme_url);
            return;
        }
        match self {
            ExternalEditor::Xcode => {
                let arguments = match line {
                    Some(line) => vec!["-l".to_owned(), line.to_string(), file.path()],
                    None => vec![file.path()],
                };
                run_tool(first_executable(&["/usr/bin/xed"]), file, &arguments);
            }
            ExternalEditor::Sublime => {
                let tool = first_executable(&[
                    "/usr/local/bin/subl",
                    "/opt/homebrew/bin/subl",
                    "/Applications/Sublime Text.app/Contents/SharedSupport/bin/subl",
                ]);
                let argument = file.path() + &line.map(|line| format!(":{line}")).unwrap_or_default();
                run_tool(tool, file, &[argument]);
            }
            ExternalEditor::Bbedit => {
                let tool = first_executable(&[
                    "/usr/local/bin/bbedit",
                    "/opt/homebrew/bin/bbedit",
                    "/Applications/BBEdit.app/Contents/Helpers/bbedit_tool",
                ]);
                let arguments = match line {
                    Some(line) => vec![format!("+{line}"), file.path()],
                    None => vec![file.path()],
                };
                run_tool(tool, file, &arguments);
            }
            ExternalEditor::EnvEditor => open_in_terminal_editor(file, line),
            ExternalEditor::Nova | ExternalEditor::SystemDefault | ExternalEditor::Vscode | ExternalEditor::Cursor | ExternalEditor::Zed => {
                workspace.openURL(&file.to_nsurl());
            }
        }
    }

    /// The characters a trusted `$EDITOR` token may contain.
    const EDITOR_PATH_CHARACTERS: &'static str =
        "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789/._+-";

    /// A bare path token of letters, digits, `/`, `.`, `-`, `_` and `+`, or
    /// `None`: `$EDITOR` lands in the middle of a `do script` line.
    pub fn sanitized_editor(raw: &str) -> Option<&str> {
        if raw.is_empty() || !raw.chars().all(|scalar| Self::EDITOR_PATH_CHARACTERS.contains(scalar)) {
            return None;
        }
        Some(raw)
    }

    /// `"'" + path.replacingOccurrences(of: "'", with: "'\\''") + "'"`.
    pub fn shell_quoted(path: &str) -> String {
        format!("'{}'", replacing_occurrences(path, "'", "'\\''"))
    }

    /// First installed editor, preferring the ones people actually run agents
    /// next to.
    pub fn best_available() -> ExternalEditor {
        [
            ExternalEditor::Cursor,
            ExternalEditor::Vscode,
            ExternalEditor::Zed,
            ExternalEditor::Sublime,
            ExternalEditor::Nova,
            ExternalEditor::Bbedit,
            ExternalEditor::Xcode,
        ]
        .into_iter()
        .find(|candidate| candidate.is_installed())
        .unwrap_or(ExternalEditor::SystemDefault)
    }
}

/// `String.replacingOccurrences(of:with:)`, which is `NSString`'s.
fn replacing_occurrences(text: &str, target: &str, replacement: &str) -> String {
    NSString::from_str(text)
        .stringByReplacingOccurrencesOfString_withString(&NSString::from_str(target), &NSString::from_str(replacement))
        .to_string()
}

fn first_executable(paths: &[&str]) -> Option<String> {
    paths.iter().find(|path| file_manager::is_executable_file(path)).map(|path| (*path).to_owned())
}

fn run_tool(path: Option<String>, fallback_file: &FileUrl, arguments: &[String]) {
    let Some(path) = path.filter(|path| file_manager::is_executable_file(path)) else {
        NSWorkspace::sharedWorkspace().openURL(&fallback_file.to_nsurl());
        return;
    };
    let _ = std::process::Command::new(path).args(arguments).spawn();
}

fn open_in_terminal_editor(file: &FileUrl, line: Option<isize>) {
    let editor = std::env::var("EDITOR")
        .ok()
        .and_then(|raw| ExternalEditor::sanitized_editor(&raw).map(str::to_owned))
        .unwrap_or_else(|| "vi".to_owned());
    let line_argument = line.map(|line| format!("+{line} ")).unwrap_or_default();
    let command = format!("{editor} {line_argument}{}", ExternalEditor::shell_quoted(&file.path()));
    // Inside the AppleScript string literal, backslash is an escape character
    // too. Escape backslashes, quotes, and newlines.
    let mut apple_script_string = replacing_occurrences(&command, "\\", "\\\\");
    apple_script_string = replacing_occurrences(&apple_script_string, "\"", "\\\"");
    apple_script_string = replacing_occurrences(&apple_script_string, "\r", "\\r");
    apple_script_string = replacing_occurrences(&apple_script_string, "\n", "\\n");
    let script = format!(
        "tell application \"Terminal\"\n    activate\n    do script \"{apple_script_string}\"\nend tell"
    );
    if let Some(script) = NSAppleScript::initWithSource(NSAppleScript::alloc(), &NSString::from_str(&script)) {
        unsafe {
            let _ = script.executeAndReturnError(None);
        }
    }
}
