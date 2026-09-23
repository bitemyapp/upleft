//! Port of `Theme/ThemeStore.swift`: the six bundled themes, plus anything the
//! user drops in `~/Library/Application Support/Downright/Themes`,
//! hot-reloading while it is being edited (§11.2).
//!
//! Swift confines mutation to the main thread. The port keeps the same calls
//! on the same threads (the watcher hops to the main queue before reloading)
//! and additionally guards the state with a mutex, so a `StyleSheet` built on
//! any thread can read `revision` safely. Observers run on the thread that
//! caused the change, outside the lock, as in Swift.

use std::collections::HashMap;
use std::ffi::CString;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, Weak};
use std::time::Duration;

use dispatch2::{
    _dispatch_source_type_vnode, DispatchObject, DispatchQueue, DispatchRetained, DispatchSource,
    DispatchTime, dispatch_source_type_t, dispatch_source_vnode_flags_t,
};
use objc2::rc::Retained;
use objc2_foundation::{
    NSArray, NSDirectoryEnumerationOptions, NSFileManager, NSNumber, NSSearchPathDirectory,
    NSSearchPathDomainMask, NSString, NSURL, NSURLFileSizeKey, NSURLIsRegularFileKey,
    NSUserDefaults,
};

use super::vscode_theme_import::VSCodeThemeImporter;
use crate::render_contracts::{
    CodeTheme, Theme, ThemeAppearance, ThemeColor, ThemePalette, TypographyConfig,
};
use crate::swift_compat;

const SELECTION_DEFAULTS_KEY: &str = "downright.theme.selected";

/// A theme is a palette, not a data set: bound every read (8 MiB).
const MAXIMUM_THEME_FILE_BYTES: i64 = 8 * 1024 * 1024;

/// The bundled resource directory (`Sources/MarkdownRender/Themes`), embedded
/// by file name. SwiftPM copies the same files into the resource bundle.
const BUNDLED_THEME_FILES: [(&str, &[u8]); 6] = [
    (
        "high-contrast.json",
        include_bytes!(
            "../../../../vendor/downright/Sources/MarkdownRender/Themes/high-contrast.json"
        ),
    ),
    (
        "nord.json",
        include_bytes!("../../../../vendor/downright/Sources/MarkdownRender/Themes/nord.json"),
    ),
    (
        "paper-light.json",
        include_bytes!(
            "../../../../vendor/downright/Sources/MarkdownRender/Themes/paper-light.json"
        ),
    ),
    (
        "solarized-light.json",
        include_bytes!(
            "../../../../vendor/downright/Sources/MarkdownRender/Themes/solarized-light.json"
        ),
    ),
    (
        "system.json",
        include_bytes!("../../../../vendor/downright/Sources/MarkdownRender/Themes/system.json"),
    ),
    (
        "warm-dark.json",
        include_bytes!("../../../../vendor/downright/Sources/MarkdownRender/Themes/warm-dark.json"),
    ),
];

type Observer = Arc<dyn Fn(&Theme) + Send + Sync>;

struct State {
    /// Bundled first, then user-only themes; both alphabetical.
    themes: Vec<Theme>,
    /// Monotonic token bumped on every theme change.
    revision: i64,
    bundled_themes: Vec<Theme>,
    user_themes: Vec<Theme>,
    selected_name: String,
}

pub struct ThemeStore {
    defaults: Retained<NSUserDefaults>,
    state: Mutex<State>,
    observers: Mutex<Vec<(u64, Observer)>>,
    next_observer: AtomicU64,
    watcher: Mutex<Option<DirectoryWatcher>>,
}

static SHARED: LazyLock<Arc<ThemeStore>> =
    LazyLock::new(|| ThemeStore::new(NSUserDefaults::standardUserDefaults()));

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeStoreError {
    Unreadable(String),
    NotAVSCodeTheme,
    UserThemesUnavailable,
    /// A write the Swift code lets `throw` out of `FileManager`/`Data.write`.
    Io(String),
}

impl ThemeStoreError {
    /// `errorDescription`.
    pub fn error_description(&self) -> String {
        match self {
            ThemeStoreError::Unreadable(last_path_component) => {
                format!("Could not read {last_path_component}.")
            }
            ThemeStoreError::NotAVSCodeTheme => "That file is not a VS Code colour theme.".into(),
            ThemeStoreError::UserThemesUnavailable => {
                "The Upleft themes folder is unavailable.".into()
            }
            ThemeStoreError::Io(message) => message.clone(),
        }
    }
}

impl std::fmt::Display for ThemeStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.error_description())
    }
}

impl std::error::Error for ThemeStoreError {}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poison| poison.into_inner())
}

impl ThemeStore {
    pub fn shared() -> &'static Arc<ThemeStore> {
        &SHARED
    }

    /// `ThemeStore(defaults:)`: `defaults` lets a test isolate theme
    /// selection from the user's real preferences.
    pub fn new(defaults: Retained<NSUserDefaults>) -> Arc<ThemeStore> {
        let selected_name = defaults
            .stringForKey(&NSString::from_str(SELECTION_DEFAULTS_KEY))
            .map(|name| name.to_string())
            .unwrap_or_else(|| "Paper Light".to_owned());
        let store = Arc::new(ThemeStore {
            defaults,
            state: Mutex::new(State {
                themes: Vec::new(),
                revision: 1,
                bundled_themes: ThemeStore::load_bundled_themes(),
                user_themes: Vec::new(),
                selected_name,
            }),
            observers: Mutex::new(Vec::new()),
            next_observer: AtomicU64::new(1),
            watcher: Mutex::new(None),
        });
        ThemeStore::rebuild(&mut lock(&store.state));
        store.reload_user_themes();
        store
    }

    pub fn themes(&self) -> Vec<Theme> {
        lock(&self.state).themes.clone()
    }

    pub fn revision(&self) -> i64 {
        lock(&self.state).revision
    }

    /// The selected theme, else the first, else `Theme::fallback()`.
    pub fn current(&self) -> Theme {
        ThemeStore::current_of(&lock(&self.state))
    }

    fn current_of(state: &State) -> Theme {
        state
            .themes
            .iter()
            .find(|theme| swift_compat::string_eq(&theme.name, &state.selected_name))
            .or_else(|| state.themes.first())
            .cloned()
            .unwrap_or_else(Theme::fallback)
    }

    // MARK: - Selection

    pub fn select(&self, name: &str) {
        {
            let mut state = lock(&self.state);
            if !state
                .themes
                .iter()
                .any(|theme| swift_compat::string_eq(&theme.name, name))
                || swift_compat::string_eq(name, &state.selected_name)
            {
                return;
            }
            state.selected_name = name.to_owned();
        }
        let value = NSString::from_str(name);
        // SAFETY: an NSString is a valid property-list value.
        unsafe {
            self.defaults
                .setObject_forKey(Some(&value), &NSString::from_str(SELECTION_DEFAULTS_KEY))
        };
        self.bump_and_notify();
    }

    // MARK: - Loading

    /// User themes live in ~/Library/Application Support/Downright/Themes.
    pub fn reload_user_themes(self: &Arc<Self>) {
        let Some(directory) = ThemeStore::user_themes_directory() else {
            return;
        };
        let _ = create_directory(&directory);
        let user_themes = ThemeStore::load_themes(&directory);
        lock(&self.state).user_themes = user_themes;
        self.start_watching(&directory);
        ThemeStore::rebuild(&mut lock(&self.state));
        self.bump_and_notify();
    }

    fn rebuild(state: &mut State) {
        let mut overrides: HashMap<String, Theme> = HashMap::new();
        for theme in &state.user_themes {
            overrides.insert(swift_compat::string_key(&theme.name), theme.clone());
        }
        let bundled_names: Vec<String> = state
            .bundled_themes
            .iter()
            .map(|theme| swift_compat::string_key(&theme.name))
            .collect();
        let mut bundled: Vec<Theme> = state
            .bundled_themes
            .iter()
            .map(|theme| {
                overrides
                    .get(&swift_compat::string_key(&theme.name))
                    .cloned()
                    .unwrap_or_else(|| theme.clone())
            })
            .collect();
        bundled.sort_by(|a, b| swift_compat::string_cmp(&a.name, &b.name));
        let mut user_only: Vec<Theme> = state
            .user_themes
            .iter()
            .filter(|theme| !bundled_names.contains(&swift_compat::string_key(&theme.name)))
            .cloned()
            .collect();
        user_only.sort_by(|a, b| swift_compat::string_cmp(&a.name, &b.name));
        bundled.extend(user_only);
        state.themes = bundled;
    }

    fn load_bundled_themes() -> Vec<Theme> {
        // `decode(_:)` sorts by file name and skips empty or oversized files;
        // the embedded list is already in that order and every file qualifies.
        BUNDLED_THEME_FILES
            .iter()
            .filter_map(|(_, data)| Theme::decode_json(data).ok())
            .collect()
    }

    fn load_themes(directory: &NSURL) -> Vec<Theme> {
        let manager = NSFileManager::defaultManager();
        let contents: Vec<Retained<NSURL>> = manager
            .contentsOfDirectoryAtURL_includingPropertiesForKeys_options_error(
                directory,
                None,
                NSDirectoryEnumerationOptions::SkipsHiddenFiles,
            )
            .map(|array| array.to_vec())
            .unwrap_or_default();
        let json: Vec<Retained<NSURL>> = contents
            .into_iter()
            .filter(|url| {
                url.pathExtension().is_some_and(|extension| {
                    swift_compat::lowercased(&extension.to_string()) == "json"
                })
            })
            .collect();
        ThemeStore::decode(json)
    }

    /// A theme that fails to decode is skipped rather than surfaced: with hot
    /// reload the file is very often half-written.
    fn decode(urls: Vec<Retained<NSURL>>) -> Vec<Theme> {
        let mut named: Vec<(String, Retained<NSURL>)> = urls
            .into_iter()
            .map(|url| {
                (
                    url.lastPathComponent()
                        .map(|name| name.to_string())
                        .unwrap_or_default(),
                    url,
                )
            })
            .filter(|(name, _)| !swift_compat::has_ascii_prefix(name, "."))
            .collect();
        named.sort_by(|a, b| swift_compat::string_cmp(&a.0, &b.0));
        named
            .into_iter()
            .filter_map(|(_, url)| {
                // SAFETY: the keys are valid NSURLResourceKey constants.
                let keys =
                    unsafe { NSArray::from_slice(&[NSURLIsRegularFileKey, NSURLFileSizeKey]) };
                let values = url.resourceValuesForKeys_error(&keys).ok()?;
                // SAFETY: as above.
                let (regular_key, size_key) = unsafe { (NSURLIsRegularFileKey, NSURLFileSizeKey) };
                let is_regular = values
                    .objectForKey(regular_key)
                    .and_then(|value| value.downcast::<NSNumber>().ok())
                    .is_some_and(|number| number.boolValue());
                let size = values
                    .objectForKey(size_key)
                    .and_then(|value| value.downcast::<NSNumber>().ok())
                    .map(|number| number.integerValue() as i64)?;
                if !is_regular || size <= 0 || size > MAXIMUM_THEME_FILE_BYTES {
                    return None;
                }
                let path = url.path()?.to_string();
                let data = std::fs::read(path).ok()?;
                Theme::decode_json(&data).ok()
            })
            .collect()
    }

    pub fn user_themes_directory() -> Option<Retained<NSURL>> {
        let urls = NSFileManager::defaultManager().URLsForDirectory_inDomains(
            NSSearchPathDirectory::ApplicationSupportDirectory,
            NSSearchPathDomainMask::UserDomainMask,
        );
        let base = urls.firstObject()?;
        base.URLByAppendingPathComponent_isDirectory(&NSString::from_str("Upleft/Themes"), true)
    }

    // MARK: - Hot reload (§11.2)

    fn start_watching(self: &Arc<Self>, directory: &NSURL) {
        let mut watcher = lock(&self.watcher);
        if watcher.is_some() {
            return;
        }
        let Some(path) = directory.path().map(|path| path.to_string()) else {
            return;
        };
        let store: Weak<ThemeStore> = Arc::downgrade(self);
        // Swift reloads on the main queue, reading the folder there. The port
        // is stricter about the main thread: the coalesced reload arrives on
        // main as in Swift, hands the folder read to a serial queue (so
        // reloads stay ordered), and hops back to main only to swap the
        // themes in and notify.
        let loader = DispatchQueue::new("com.downright.theme-load", None);
        *watcher = DirectoryWatcher::new(&path, move || {
            let store = store.clone();
            loader.exec_async(move || {
                let Some(directory) = ThemeStore::user_themes_directory() else {
                    return;
                };
                let user_themes = ThemeStore::load_themes(&directory);
                DispatchQueue::main().exec_async(move || {
                    let Some(store) = store.upgrade() else { return };
                    {
                        let mut state = lock(&store.state);
                        state.user_themes = user_themes;
                        ThemeStore::rebuild(&mut state);
                    }
                    store.bump_and_notify();
                });
            });
        });
    }

    /// Hot-reload: fires whenever the selected theme's file changes on disk.
    pub fn observe(
        self: &Arc<Self>,
        handler: impl Fn(&Theme) + Send + Sync + 'static,
    ) -> ThemeObservation {
        let id = self.next_observer.fetch_add(1, Ordering::Relaxed);
        lock(&self.observers).push((id, Arc::new(handler)));
        ThemeObservation {
            store: Mutex::new(Arc::downgrade(self)),
            id,
        }
    }

    fn remove_observer(&self, id: u64) {
        lock(&self.observers).retain(|(existing, _)| *existing != id);
    }

    fn bump_and_notify(&self) {
        let theme = {
            let mut state = lock(&self.state);
            state.revision = state.revision.wrapping_add(1);
            ThemeStore::current_of(&state)
        };
        let handlers: Vec<Observer> = lock(&self.observers)
            .iter()
            .map(|(_, handler)| handler.clone())
            .collect();
        for handler in handlers {
            handler(&theme);
        }
    }

    // MARK: - Import / export

    /// Imports a VS Code / Shiki theme and installs it as a user theme, so code
    /// blocks and mermaid diagrams share one palette (§11.2).
    pub fn import_vscode_theme(self: &Arc<Self>, path: &str) -> Result<Theme, ThemeStoreError> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        let last_path_component = url
            .lastPathComponent()
            .map(|name| name.to_string())
            .unwrap_or_default();
        let data =
            std::fs::read(path).map_err(|_| ThemeStoreError::Unreadable(last_path_component))?;
        let fallback_name = url
            .URLByDeletingPathExtension()
            .and_then(|url| url.lastPathComponent())
            .map(|name| name.to_string())
            .unwrap_or_default();
        let theme = VSCodeThemeImporter::theme(&data, &fallback_name)?;
        let directory =
            ThemeStore::user_themes_directory().ok_or(ThemeStoreError::UserThemesUnavailable)?;
        create_directory(&directory)?;
        let file = directory
            .URLByAppendingPathComponent(&NSString::from_str(
                &(ThemeStore::slug(&theme.name) + ".json"),
            ))
            .and_then(|url| url.path())
            .ok_or(ThemeStoreError::UserThemesUnavailable)?;
        self.export(&theme, &file.to_string())?;
        self.reload_user_themes();
        Ok(theme)
    }

    /// `JSONEncoder` with pretty printing, sorted keys and unescaped slashes,
    /// written atomically.
    pub fn export(&self, theme: &Theme, path: &str) -> Result<(), ThemeStoreError> {
        let text = theme.encode_pretty_sorted();
        let temporary = format!("{path}.upleft-tmp-{}", std::process::id());
        std::fs::write(&temporary, text.as_bytes())
            .map_err(|error| ThemeStoreError::Io(error.to_string()))?;
        std::fs::rename(&temporary, path).map_err(|error| ThemeStoreError::Io(error.to_string()))
    }

    /// `slug(_:)`: letters and numbers kept, every other `Character` a dash,
    /// runs of dashes collapsed.
    pub fn slug(name: &str) -> String {
        let lowered = swift_compat::lowercased(name);
        let allowed: String = upleft_swift_text::graphemes(&lowered)
            .map(|grapheme| {
                let first = grapheme.chars().next().unwrap_or('-');
                if swift_compat::is_letter(first) || swift_compat::is_number(first) {
                    grapheme
                } else {
                    "-"
                }
            })
            .collect();
        let collapsed = swift_compat::split_on_character(&allowed, '-').join("-");
        if collapsed.is_empty() {
            "theme".to_owned()
        } else {
            collapsed
        }
    }
}

fn create_directory(directory: &NSURL) -> Result<(), ThemeStoreError> {
    // SAFETY: no attributes dictionary is passed.
    unsafe {
        NSFileManager::defaultManager()
            .createDirectoryAtURL_withIntermediateDirectories_attributes_error(
                directory, true, None,
            )
            .map_err(|error| ThemeStoreError::Io(error.localizedDescription().to_string()))
    }
}

impl Theme {
    /// `Theme.fallback`: built entirely from system colours so it is correct
    /// in both appearances without a file behind it.
    pub fn fallback() -> Theme {
        let c = ThemeColor::new;
        Theme {
            name: "System".into(),
            appearance: ThemeAppearance::Auto,
            palette: ThemePalette {
                background: c("system:textBackground"),
                surface: c("system:controlBackground"),
                text: c("system:label"),
                text_secondary: c("system:secondaryLabel"),
                text_faint: c("system:tertiaryLabel"),
                heading: c("system:label"),
                marker: c("system:quaternaryLabel"),
                accent: c("system:accent"),
                link: c("system:link"),
                rule: c("system:separator"),
                selection: c("system:selectedTextBackground"),
                code_background: c("system:controlBackground"),
                inline_code_background: c("system:controlBackground"),
                code_rule: c("system:separator"),
                rail_tick: c("system:tertiaryLabel"),
                rail_tick_current: c("system:label"),
                quote_rule: c("system:quaternaryLabel"),
                change_added: c("system:systemGreen"),
                change_removed: c("system:systemRed"),
                change_modified: c("system:systemBlue"),
                path_missing: c("system:systemRed"),
                search_hit: c("system:systemBlue"),
                search_hit_current: c("system:systemBlue"),
                callout_note: c("system:systemBlue"),
                callout_warning: c("system:systemOrange"),
                callout_success: c("system:systemGreen"),
                callout_danger: c("system:systemRed"),
                callout_important: Some(c("system:systemPurple")),
            },
            code: CodeTheme {
                keyword: c("system:systemPink"),
                string: c("system:systemRed"),
                number: c("system:systemBlue"),
                comment: c("system:systemGreen"),
                r#type: c("system:systemTeal"),
                function: c("system:systemBlue"),
                variable: c("system:label"),
                constant: c("system:systemPurple"),
                operator: c("system:secondaryLabel"),
                punctuation: c("system:tertiaryLabel"),
                attribute: c("system:systemBlue"),
                diff_added: c("system:systemGreen"),
                diff_removed: c("system:systemRed"),
                diff_header: c("system:secondaryLabel"),
            },
            typography: TypographyConfig::default_config(),
        }
    }
}

/// Dropping the token cancels the observation.
pub struct ThemeObservation {
    store: Mutex<Weak<ThemeStore>>,
    id: u64,
}

impl ThemeObservation {
    pub fn cancel(&self) {
        let mut store = lock(&self.store);
        if let Some(live) = store.upgrade() {
            live.remove_observer(self.id);
        }
        *store = Weak::new();
    }
}

impl Drop for ThemeObservation {
    fn drop(&mut self) {
        self.cancel();
    }
}

// MARK: - Directory watching

/// A coarse vnode watch on the user themes directory. Coarse on purpose: the
/// reload re-reads every file anyway, and an editor that saves by rename would
/// defeat per-file watches.
pub struct DirectoryWatcher {
    inner: Arc<WatcherInner>,
}

struct WatcherInner {
    path: CString,
    on_change: Box<dyn Fn() + Send + Sync>,
    queue: DispatchRetained<DispatchQueue>,
    /// Watch state is touched from the constructing thread, the watch queue,
    /// and `Drop`; lock-guarded rather than queue-confined.
    state: Mutex<WatchState>,
}

#[derive(Default)]
struct WatchState {
    source: Option<DispatchRetained<DispatchSource>>,
    descriptor: i32,
    /// The pending coalesced reload's cancellation flag.
    pending: Option<Arc<AtomicBool>>,
}

impl DirectoryWatcher {
    pub fn new(
        path: &str,
        on_change: impl Fn() + Send + Sync + 'static,
    ) -> Option<DirectoryWatcher> {
        let inner = Arc::new(WatcherInner {
            path: CString::new(path).ok()?,
            on_change: Box::new(on_change),
            queue: DispatchQueue::new("com.downright.theme-watch", None),
            state: Mutex::new(WatchState {
                descriptor: -1,
                ..WatchState::default()
            }),
        });
        WatcherInner::start(&inner);
        Some(DirectoryWatcher { inner })
    }
}

impl Drop for DirectoryWatcher {
    fn drop(&mut self) {
        self.inner.stop();
    }
}

impl WatcherInner {
    fn start(this: &Arc<WatcherInner>) {
        // SAFETY: a valid NUL-terminated path.
        let opened = unsafe { libc::open(this.path.as_ptr(), libc::O_EVTONLY) };
        if opened < 0 {
            return;
        }
        let mask = dispatch_source_vnode_flags_t::DISPATCH_VNODE_WRITE.0
            | dispatch_source_vnode_flags_t::DISPATCH_VNODE_EXTEND.0
            | dispatch_source_vnode_flags_t::DISPATCH_VNODE_ATTRIB.0
            | dispatch_source_vnode_flags_t::DISPATCH_VNODE_RENAME.0
            | dispatch_source_vnode_flags_t::DISPATCH_VNODE_DELETE.0;
        let kind: dispatch_source_type_t = (&raw const _dispatch_source_type_vnode).cast_mut();
        // SAFETY: a vnode source on an open descriptor, delivered on our queue.
        let source =
            unsafe { DispatchSource::new(kind, opened as usize, mask as usize, Some(&this.queue)) };
        let weak = Arc::downgrade(this);
        let events = source.clone();
        let handler = block2::RcBlock::new(move || {
            let Some(inner) = weak.upgrade() else { return };
            // A rename or delete invalidates the descriptor: an editor that
            // saves atomically replaces the directory entry.
            let data = events.data() as std::ffi::c_ulong;
            let renamed_or_deleted = dispatch_source_vnode_flags_t::DISPATCH_VNODE_RENAME.0
                | dispatch_source_vnode_flags_t::DISPATCH_VNODE_DELETE.0;
            if data & renamed_or_deleted != 0 {
                inner.restart();
            }
            inner.schedule_reload();
        });
        let cancel = block2::RcBlock::new(move || {
            // SAFETY: closes exactly the descriptor this source was built on.
            unsafe { libc::close(opened) };
        });
        // SAFETY: the blocks are valid for the source's lifetime (copied by
        // libdispatch).
        unsafe {
            source.set_event_handler_with_block(&*handler as *const _ as *mut _);
            source.set_cancel_handler_with_block(&*cancel as *const _ as *mut _);
        }
        let mut state = lock(&this.state);
        state.descriptor = opened;
        source.resume();
        state.source = Some(source);
    }

    fn restart(self: &Arc<Self>) {
        self.stop();
        let weak = Arc::downgrade(self);
        let when = DispatchTime::try_from(Duration::from_millis(200)).unwrap_or(DispatchTime::NOW);
        let _ = self.queue.after(when, move || {
            if let Some(inner) = weak.upgrade() {
                WatcherInner::start(&inner);
            }
        });
    }

    fn stop(&self) {
        let mut state = lock(&self.state);
        let source = state.source.take();
        state.descriptor = -1;
        // Cancelling inside the lock keeps stop atomic against a racing start.
        if let Some(source) = source {
            source.cancel();
        }
    }

    /// One save produces a burst of events; coalescing keeps the reload to one
    /// per burst.
    fn schedule_reload(self: &Arc<Self>) {
        let flag = Arc::new(AtomicBool::new(false));
        {
            let mut state = lock(&self.state);
            if let Some(previous) = state.pending.replace(flag.clone()) {
                previous.store(true, Ordering::SeqCst);
            }
        }
        let weak = Arc::downgrade(self);
        let when = DispatchTime::try_from(Duration::from_millis(150)).unwrap_or(DispatchTime::NOW);
        let _ = DispatchQueue::main().after(when, move || {
            if flag.load(Ordering::SeqCst) {
                return;
            }
            if let Some(inner) = weak.upgrade() {
                (inner.on_change)();
            }
        });
    }
}
