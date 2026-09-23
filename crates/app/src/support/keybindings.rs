//! Port of `Sources/DownrightApp/Support/Keybindings.swift`.
//!
//! The default binding table, the keybindings file's wire format, and the
//! store that resolves and persists bindings.
//!
//! Threading: Swift writes `keybindings.json` synchronously from whatever
//! thread edits the store (the main thread, in Settings). Here the bytes are
//! encoded synchronously and written on a private serial queue, so recording
//! a shortcut never blocks the main thread on `fsync`;
//! [`KeybindingStore::flush`] waits for pending writes. Writes stay in order,
//! so the file ends up byte-identical. The store itself sits behind a
//! `Mutex`, so the UI port can warm [`KeybindingStore::shared`] (which reads
//! the file) off the main thread at launch.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard};

use dispatch2::{DispatchQueue, DispatchRetained};
use objc2_app_kit::NSEvent;
use objc2_foundation::{NSCocoaErrorDomain, NSData, NSDataReadingOptions};
use upleft_foundation::json_decoder::{self, DecodingError, Value};
use upleft_foundation::json_encoder::{self, JsonValue, OutputFormatting};
use upleft_foundation::url::FileUrl;

use super::app_paths;
use super::commands::{Command, CommandScope, KeyBinding, ModifierFlags};

/// The default binding table, transcribed from §7.2, plus the bindings the
/// commands added by §9 need.
///
/// Read mode's single-letter bindings are only reachable in `.read` scope
/// because there is no caret there to swallow them — that is the whole reason
/// §7.2 can spend the bare letter keys at all.
///
/// Chords are chosen so nothing shadows a macOS convention: ⌘0 is Actual Size,
/// ⌘⇧P is Page Setup, ⌘⇧V is Paste and Match Style, ⌃⌘F is Enter Full Screen,
/// and ⌘T / ⌘⇧T stay free for the tab chords users reach for reflexively.
pub struct KeybindingDefaults;

const CMD: ModifierFlags = ModifierFlags::COMMAND;
const SHIFT: ModifierFlags = ModifierFlags::SHIFT;
const OPT: ModifierFlags = ModifierFlags::OPTION;
const CTRL: ModifierFlags = ModifierFlags::CONTROL;
const NONE: ModifierFlags = ModifierFlags::EMPTY;

const fn m(a: ModifierFlags, b: ModifierFlags) -> ModifierFlags {
    a.union(b)
}

const fn m3(a: ModifierFlags, b: ModifierFlags, c: ModifierFlags) -> ModifierFlags {
    a.union(b).union(c)
}

type DefaultEntry = (Command, &'static [(&'static str, ModifierFlags)]);

/// The table literal, in source order. A command may carry more than one
/// binding: `=` and `+` both enlarge text.
const DEFAULT_TABLE: &[DefaultEntry] = &[
    // Global (§7.2)
    (Command::SourceMode, &[("e", m(CMD, SHIFT))]),
    (Command::UseSelectionForFind, &[("e", CMD)]),
    (Command::VersionTimeline, &[("v", m(CMD, OPT))]),
    (Command::CommandPalette, &[("k", m(CMD, SHIFT))]),
    (Command::NextChange, &[("down", m(OPT, SHIFT))]),
    (Command::PreviousChange, &[("up", m(OPT, SHIFT))]),
    (Command::MarkChangesReviewed, &[("r", m(CMD, SHIFT))]),
    (Command::Find, &[("f", CMD)]),
    (Command::FindNext, &[("g", CMD)]),
    (Command::FindPrevious, &[("g", m(CMD, SHIFT))]),
    (Command::FindInSiblings, &[("f", m(CMD, SHIFT))]),
    (Command::FindReplace, &[("f", m(CMD, OPT))]),
    (Command::PromoteHeading, &[("[", CMD)]),
    (Command::DemoteHeading, &[("]", CMD)]),
    (Command::HeadingToBody, &[("0", m(CMD, OPT))]),
    (Command::HeadingLevel1, &[("1", m(CMD, OPT))]),
    (Command::HeadingLevel2, &[("2", m(CMD, OPT))]),
    (Command::HeadingLevel3, &[("3", m(CMD, OPT))]),
    (Command::HeadingLevel4, &[("4", m(CMD, OPT))]),
    (Command::HeadingLevel5, &[("5", m(CMD, OPT))]),
    (Command::HeadingLevel6, &[("6", m(CMD, OPT))]),
    (Command::MoveBlockUp, &[("up", m(CMD, OPT))]),
    (Command::MoveBlockDown, &[("down", m(CMD, OPT))]),
    (Command::SplitView, &[("backslash", CMD)]),
    (Command::CopyAsMarkdown, &[("c", m3(CMD, OPT, SHIFT))]),
    (Command::CopySection, &[("c", m(CMD, OPT))]),
    (Command::PrintDocument, &[("p", CMD)]),
    (Command::ExportHtml, &[("e", m(CMD, CTRL))]),
    // Share joins Export HTML's ⌃⌘ family — "the same document, sent
    // somewhere else". Share as PDF stays unbound.
    (Command::Share, &[("s", m(CMD, CTRL))]),
    // Panels share one ⌥⌘-number family, the way a navigator normally does.
    (Command::DocumentLens, &[("2", m3(CMD, OPT, SHIFT))]),
    (Command::TaskPanel, &[("3", m3(CMD, OPT, SHIFT))]),
    // Structural zoom shares one ⌃⌘ family, reachable while editing (§5.2).
    (Command::ZoomLevel1, &[("1", m3(CMD, CTRL, OPT))]),
    (Command::ZoomLevel2, &[("2", m3(CMD, CTRL, OPT))]),
    (Command::ZoomLevel3, &[("3", m3(CMD, CTRL, OPT))]),
    (Command::ZoomLevel4, &[("4", m3(CMD, CTRL, OPT))]),
    (Command::ZoomLevel5, &[("5", m3(CMD, CTRL, OPT))]),
    (Command::ZoomIn, &[("=", m3(CMD, CTRL, OPT))]),
    (Command::ZoomOut, &[("-", m3(CMD, CTRL, OPT))]),
    (Command::NextHeading, &[("n", m3(CMD, CTRL, OPT))]),
    (Command::PreviousHeading, &[("p", m3(CMD, CTRL, OPT))]),
    (Command::FollowLinkAtCaret, &[("return", CMD)]),
    (Command::NextLink, &[("]", m(CMD, OPT))]),
    (Command::PreviousLink, &[("[", m(CMD, OPT))]),
    // Navigation chords survive an editable text view. AppKit still owns
    // the unmodified arrows, Space and Page Up/Down.
    (Command::PageDown, &[("space", OPT)]),
    (Command::PageUp, &[("space", m(OPT, SHIFT))]),
    (Command::ScrollDown, &[("down", m(CTRL, OPT))]),
    (Command::ScrollUp, &[("up", m(CTRL, OPT))]),
    (Command::DocumentStart, &[("up", CMD)]),
    (Command::DocumentEnd, &[("down", CMD)]),
    // Files and editing
    (Command::NewDocument, &[("n", CMD)]),
    (Command::Open, &[("o", CMD)]),
    (Command::Save, &[("s", CMD)]),
    (Command::SaveAs, &[("s", m(CMD, SHIFT))]),
    (Command::Close, &[("w", CMD)]),
    // ⌘Y is Quick Look everywhere else on the system. Deliberately a chord
    // and not the bare Space Finder also accepts (see the Swift comment).
    (Command::QuickLook, &[("y", CMD)]),
    (Command::ToggleBold, &[("b", CMD)]),
    (Command::ToggleItalic, &[("i", CMD)]),
    (Command::InsertLink, &[("k", m(CMD, OPT))]),
    // Ticking a box is a headline action, so it gets a one-modifier chord.
    (Command::ToggleTaskAtCaret, &[("l", CMD)]),
    // Tab in a list item; the text view decides whether the caret is in one
    // and otherwise types a tab.
    (Command::IndentList, &[("tab", NONE)]),
    (Command::OutdentList, &[("tab", SHIFT)]),
    // ⌘⇧= reports "+" on a US layout, so the second chord needs ⇧ to match.
    (Command::IncreaseTextSize, &[("=", CMD), ("+", m(CMD, SHIFT))]),
    (Command::DecreaseTextSize, &[("-", CMD)]),
    (Command::ResetTextSize, &[("0", CMD)]),
    (Command::Preferences, &[(",", CMD)]),
    (Command::GoBack, &[("[", m(CMD, CTRL))]),
    (Command::GoForward, &[("]", m(CMD, CTRL))]),
    (Command::ExportPdf, &[("p", m(CMD, OPT))]),
    (Command::CompareFiles, &[("d", m(CMD, SHIFT))]),
    (Command::TidyDocument, &[("t", m(CMD, CTRL))]),
    (Command::FocusMode, &[("return", m(CMD, SHIFT))]),
    (Command::GoToLine, &[("j", CMD)]),
];

static DEFAULTS: LazyLock<HashMap<Command, Vec<KeyBinding>>> = LazyLock::new(|| {
    DEFAULT_TABLE
        .iter()
        .map(|(command, bindings)| {
            (*command, bindings.iter().map(|(key, modifiers)| KeyBinding::new(*key, *modifiers)).collect())
        })
        .collect()
});

impl KeybindingDefaults {
    /// `KeybindingDefaults.table`.
    pub fn table() -> &'static HashMap<Command, Vec<KeyBinding>> {
        &DEFAULTS
    }

    /// The table's commands in the literal's order (a Swift dictionary has
    /// no order; this is for listings and tests).
    pub fn commands() -> impl Iterator<Item = Command> {
        DEFAULT_TABLE.iter().map(|(command, _)| *command)
    }
}

// MARK: - Loading

/// Why the keybindings file could not be used: the error Swift's
/// `KeybindingLoad.unreadable` carries, reduced to its `NSError` face.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeybindingError {
    /// `Data(contentsOf:)` failed for a reason other than a missing file.
    Read { domain: String, code: isize, localized_description: String },
    Decoding(DecodingError),
}

impl KeybindingError {
    pub fn domain(&self) -> &str {
        match self {
            KeybindingError::Read { domain, .. } => domain,
            KeybindingError::Decoding(_) => "NSCocoaErrorDomain",
        }
    }

    pub fn code(&self) -> isize {
        match self {
            KeybindingError::Read { code, .. } => *code,
            KeybindingError::Decoding(error) => error.code(),
        }
    }

    /// `error.localizedDescription`.
    pub fn localized_description(&self) -> String {
        match self {
            KeybindingError::Read { localized_description, .. } => localized_description.clone(),
            KeybindingError::Decoding(error) => error.localized_description().to_owned(),
        }
    }
}

impl std::fmt::Display for KeybindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.localized_description())
    }
}

impl std::error::Error for KeybindingError {}

/// The outcome of reading the keybindings file. "Absent" and "corrupt" are
/// different events and must not share a code path: the first is the normal
/// first-run case, the second is a file the user may have hand-edited and must
/// never be overwritten behind their back.
#[derive(Clone, Debug)]
pub enum KeybindingLoad {
    Absent,
    Loaded { vim_keys_enabled: bool, overrides: HashMap<Command, Vec<KeyBinding>> },
    Unreadable(KeybindingError),
}

/// `KeybindingLoad.Stored`: the wire format. Hand-editable by design (§7.2).
#[derive(Clone, Debug, PartialEq)]
pub struct Stored {
    pub vim_keys_enabled: bool,
    /// `[String: [KeyBinding]]`; entries in document order when decoded.
    pub overrides: Vec<(String, Vec<KeyBinding>)>,
}

impl Stored {
    /// The synthesized `init(from:)`: `vimKeysEnabled`, then `overrides`.
    pub fn decode(value: &Value) -> Result<Stored, DecodingError> {
        let members = json_decoder::keyed(value)?;
        let vim_keys_enabled = json_decoder::decode_bool(json_decoder::member(members, "vimKeysEnabled")?)?;
        let mut overrides = Vec::new();
        for (name, bindings) in json_decoder::entries(json_decoder::member(members, "overrides")?)? {
            let bindings =
                json_decoder::unkeyed(bindings)?.iter().map(KeyBinding::decode).collect::<Result<Vec<_>, _>>()?;
            overrides.push((name.clone(), bindings));
        }
        Ok(Stored { vim_keys_enabled, overrides })
    }

    /// The synthesized `encode(to:)`.
    pub fn encode(&self) -> JsonValue {
        JsonValue::object([
            ("vimKeysEnabled", JsonValue::Bool(self.vim_keys_enabled)),
            (
                "overrides",
                JsonValue::Object(
                    self.overrides
                        .iter()
                        .map(|(name, bindings)| {
                            (name.clone(), JsonValue::Array(bindings.iter().map(KeyBinding::encode).collect()))
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

/// `NSFileReadNoSuchFileError` and `NSFileNoSuchFileError`.
const NO_SUCH_FILE_CODES: [isize; 2] = [260, 4];

impl KeybindingLoad {
    /// `KeybindingLoad.read(contentsOf:)`.
    pub fn read(url: &FileUrl) -> KeybindingLoad {
        let data = objc2::rc::autoreleasepool(|_| {
            NSData::dataWithContentsOfURL_options_error(&url.to_nsurl(), NSDataReadingOptions::empty())
                .map(|data| data.to_vec())
                .map_err(|error| (error.domain().to_string(), error.code(), error.localizedDescription().to_string()))
        });
        match data {
            Ok(data) => KeybindingLoad::decode(&data),
            Err((domain, code, localized_description)) => {
                // Any read failure other than "no such file" is still a
                // reason not to clobber the file, so only a missing file
                // counts as absent.
                let cocoa = unsafe { NSCocoaErrorDomain }.to_string();
                if domain == cocoa && NO_SUCH_FILE_CODES.contains(&code) {
                    KeybindingLoad::Absent
                } else {
                    KeybindingLoad::Unreadable(KeybindingError::Read { domain, code, localized_description })
                }
            }
        }
    }

    /// `KeybindingLoad.decode(_:)`.
    pub fn decode(data: &[u8]) -> KeybindingLoad {
        let stored = match json_decoder::parse(data).and_then(|value| Stored::decode(&value)) {
            Ok(stored) => stored,
            Err(error) => return KeybindingLoad::Unreadable(KeybindingError::Decoding(error)),
        };
        let mut overrides = HashMap::new();
        for (name, bindings) in stored.overrides {
            // A command that no longer exists is not corruption: it is an
            // older file naming a command this build removed.
            let Some(command) = Command::from_raw_value(&name) else { continue };
            overrides.insert(command, bindings);
        }
        KeybindingLoad::Loaded { vim_keys_enabled: stored.vim_keys_enabled, overrides }
    }
}

// MARK: - Store

/// `onLoadFailure`'s type.
pub type LoadFailureHandler = Box<dyn Fn(&KeybindingError) + Send>;

/// Loads, resolves, and persists key bindings. One table in, one lookup out.
pub struct KeybindingStore {
    bindings: HashMap<Command, Vec<KeyBinding>>,
    /// Reverse index, rebuilt whenever bindings change.
    lookup: HashMap<CommandScope, HashMap<KeyBinding, Command>>,
    overrides: HashMap<Command, Vec<KeyBinding>>,
    /// Set when the keybindings file exists but could not be read. While this
    /// is set the store refuses to write, so a file the user can still repair
    /// by hand is never replaced by defaults.
    load_failure: Option<KeybindingError>,
    /// Last write failure, if any. Bindings stay correct in memory.
    last_persistence_error: Arc<Mutex<Option<String>>>,
    on_load_failure: Option<LoadFailureHandler>,
    vim_keys_enabled: bool,
    /// Reading the file assigns the same properties the user's edits do. The
    /// hold stops that round-tripping straight back to disk.
    is_loading: bool,
    /// `None` for the shared store, which follows `AppPaths.keybindingsFile`.
    file: Option<FileUrl>,
    writer: DispatchRetained<DispatchQueue>,
}

static SHARED: LazyLock<Mutex<KeybindingStore>> = LazyLock::new(|| Mutex::new(KeybindingStore::new(None)));

impl KeybindingStore {
    /// `KeybindingStore.shared`: loads `AppPaths.keybindingsFile` on first
    /// use. The callbacks it runs (`on_load_failure`) must not re-enter it.
    pub fn shared() -> MutexGuard<'static, KeybindingStore> {
        SHARED.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// A store over `file` instead of `AppPaths.keybindingsFile`, so tests
    /// never touch the user's support directory.
    pub fn with_file(file: FileUrl) -> KeybindingStore {
        KeybindingStore::new(Some(file))
    }

    fn new(file: Option<FileUrl>) -> KeybindingStore {
        let mut store = KeybindingStore {
            bindings: HashMap::new(),
            lookup: HashMap::new(),
            overrides: HashMap::new(),
            load_failure: None,
            last_persistence_error: Arc::new(Mutex::new(None)),
            on_load_failure: None,
            vim_keys_enabled: false,
            is_loading: false,
            file,
            writer: DispatchQueue::new("com.ezzy.downright.keybindings", None),
        };
        store.is_loading = true;
        store.load();
        store.is_loading = false;
        store.rebuild();
        store
    }

    fn keybindings_file(&self) -> FileUrl {
        self.file.clone().unwrap_or_else(app_paths::keybindings_file)
    }

    fn support_directory(&self) -> FileUrl {
        match &self.file {
            Some(file) => file.deleting_last_path_component(),
            None => app_paths::support_directory(),
        }
    }

    // MARK: Lookup

    pub fn bindings(&self, command: Command) -> Vec<KeyBinding> {
        self.bindings.get(&command).cloned().unwrap_or_default()
    }

    pub fn primary_binding(&self, command: Command) -> Option<KeyBinding> {
        self.bindings.get(&command).and_then(|bindings| bindings.first()).cloned()
    }

    /// `command(for:scope:)`: resolves a key event to a command within a mode.
    pub fn command_for_event(&self, event: &NSEvent, scope: CommandScope) -> Option<Command> {
        let key = KeyBinding::key_for_event(event)?;
        let flags = ModifierFlags(event.modifierFlags().0 as u64);
        let binding = KeyBinding::new(key, flags.intersection(ModifierFlags::DEVICE_INDEPENDENT_FLAGS_MASK));
        self.command_for(&binding, scope)
    }

    /// The reverse-index lookup behind
    /// [`command_for_event`](Self::command_for_event).
    pub fn command_for(&self, binding: &KeyBinding, scope: CommandScope) -> Option<Command> {
        self.lookup.get(&scope).and_then(|index| index.get(binding)).copied()
    }

    pub fn load_failure(&self) -> Option<&KeybindingError> {
        self.load_failure.as_ref()
    }

    /// `lastPersistenceError`, as its description. Updated when a queued
    /// write finishes; call [`flush`](Self::flush) first to see the latest.
    pub fn last_persistence_error(&self) -> Option<String> {
        self.last_persistence_error.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
    }

    /// `onLoadFailure`. Installed by the app so the failure reaches the user.
    /// Assigning it after a failure has already been recorded reports it
    /// immediately, which it must, because loading happens when the shared
    /// store is first used.
    pub fn set_on_load_failure(&mut self, handler: Option<LoadFailureHandler>) {
        self.on_load_failure = handler;
        if let (Some(failure), Some(handler)) = (&self.load_failure, &self.on_load_failure) {
            handler(failure);
        }
    }

    pub fn vim_keys_enabled(&self) -> bool {
        self.vim_keys_enabled
    }

    pub fn set_vim_keys_enabled(&mut self, value: bool) {
        let old_value = self.vim_keys_enabled;
        self.vim_keys_enabled = value;
        if value == old_value {
            return;
        }
        self.rebuild();
        self.persist();
    }

    // MARK: Editing

    pub fn set_binding(&mut self, binding: Option<KeyBinding>, command: Command) {
        match binding {
            Some(binding) => self.overrides.insert(command, vec![binding]),
            None => self.overrides.insert(command, Vec::new()),
        };
        self.rebuild();
        // Recording a shortcut is an explicit instruction to write the file,
        // so it also clears a stale "do not touch this file" hold.
        self.load_failure = None;
        self.persist();
    }

    pub fn reset_to_defaults(&mut self) {
        self.overrides.clear();
        self.rebuild();
        self.load_failure = None;
        self.persist();
    }

    pub fn is_overridden(&self, command: Command) -> bool {
        self.overrides.contains_key(&command)
    }

    /// Commands whose binding collides with `binding` in any shared scope.
    pub fn conflicts(&self, binding: &KeyBinding, excluding: Command) -> Vec<Command> {
        Command::ALL_CASES
            .into_iter()
            .filter(|&other| {
                other != excluding
                    && other.scopes().iter().any(|scope| excluding.scopes().contains(scope))
                    && self.bindings.get(&other).is_some_and(|bindings| bindings.contains(binding))
            })
            .collect()
    }

    // MARK: Building

    fn rebuild(&mut self) {
        let mut resolved = KeybindingDefaults::table().clone();
        for (command, bindings) in &self.overrides {
            resolved.insert(*command, bindings.clone());
        }

        // A later command must not silently steal an earlier one's binding, so
        // build the reverse index in a defined order and keep the first claim.
        let mut built: HashMap<CommandScope, HashMap<KeyBinding, Command>> =
            CommandScope::ALL_CASES.into_iter().map(|scope| (scope, HashMap::new())).collect();
        for command in Command::ALL_CASES {
            for binding in resolved.get(&command).map(Vec::as_slice).unwrap_or_default() {
                for scope in command.scopes() {
                    if let Some(index) = built.get_mut(scope) {
                        index.entry(binding.clone()).or_insert(command);
                    }
                }
            }
        }
        self.bindings = resolved;
        self.lookup = built;
    }

    // MARK: Persistence

    fn load(&mut self) {
        match KeybindingLoad::read(&self.keybindings_file()) {
            KeybindingLoad::Absent => {}
            KeybindingLoad::Loaded { vim_keys_enabled, overrides } => {
                self.overrides = overrides;
                // Swift's `didSet` would rebuild and persist here; the loading
                // hold stops the write, and `new` rebuilds once loading is done.
                self.vim_keys_enabled = vim_keys_enabled;
            }
            KeybindingLoad::Unreadable(error) => {
                if let Some(handler) = &self.on_load_failure {
                    handler(&error);
                }
                self.load_failure = Some(error);
            }
        }
    }

    /// The bytes `persist()` writes: `JSONEncoder` with
    /// `[.prettyPrinted, .sortedKeys]` over `Stored`.
    pub fn encoded_file(&self) -> Vec<u8> {
        let stored = Stored {
            vim_keys_enabled: self.vim_keys_enabled,
            overrides: self
                .overrides
                .iter()
                .map(|(command, bindings)| (command.raw_value().to_owned(), bindings.clone()))
                .collect(),
        };
        json_encoder::encode(&stored.encode(), OutputFormatting::PRETTY_SORTED)
    }

    fn persist(&mut self) {
        if self.is_loading || self.load_failure.is_some() {
            return;
        }
        let bytes = self.encoded_file();
        let support = self.support_directory();
        let file = self.keybindings_file();
        let slot = Arc::clone(&self.last_persistence_error);
        self.writer.exec_async(move || {
            app_paths::ensure(support);
            let result = upleft_core::document_io::write_data_atomically(&bytes, Path::new(&file.path()));
            *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = result.err().map(|error| error.to_string());
        });
    }

    /// Waits until every queued write of the keybindings file has finished.
    pub fn flush(&self) {
        self.writer.exec_sync(|| {});
    }
}
