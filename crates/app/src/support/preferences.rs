//! Port of `Sources/DownrightApp/Support/Preferences.swift`.
//!
//! App-wide settings. Everything here is a deliberate default, not an
//! accident — the ones that matter are called out in the spec and carry a
//! note here.
//!
//! `preferences.json` is written by a `JSONEncoder` with
//! `[.prettyPrinted, .sortedKeys]`, so the port's bytes equal Swift's.
//!
//! Loading and every change also publish the Quick Look appearance through
//! `PreviewAppearanceStore`, which writes the user's global preferences
//! domain (CFPreferences ignores `CFFIXED_USER_HOME`). [`Preferences::shared`]
//! does that, as Swift's does; [`Preferences::for_testing`] reads and writes
//! only the file it is given.

use std::sync::{Mutex, OnceLock};

use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua};
use objc2_foundation::{NSArray, NSNotificationCenter, NSString};
use upleft_foundation::decodable::{self, DecodableValue, DecodingError, Value};
use upleft_foundation::file_manager;
use upleft_foundation::json_encoder::{self, JsonValue, OutputFormatting};
use upleft_foundation::url::FileUrl;
use upleft_render::render_contracts::{BodyPreset, RenderMode, TypographyConfig};
use upleft_render::theme::preview_appearance::{PreviewAppearance, PreviewAppearanceStore};

use crate::ai::document_state_store::decode_render_mode;
use crate::ai::path_resolver::ExternalEditor;
use crate::ai::snapshot_store::SnapshotStore;
use crate::support::app_paths;

/// `TypographyConfig`'s synthesized `encode(to:)`.
pub fn encode_typography(typography: &TypographyConfig) -> JsonValue {
    JsonValue::object([
        ("preset", JsonValue::from(typography.preset.raw_value())),
        ("bodySize", JsonValue::Double(typography.body_size)),
        ("scaleRatio", JsonValue::Double(typography.scale_ratio)),
        ("lineHeightMultiple", JsonValue::Double(typography.line_height_multiple)),
        ("measureCharacters", JsonValue::Double(typography.measure_characters)),
        ("monoFamily", JsonValue::from(typography.mono_family.as_str())),
        ("monoSizeAdjust", JsonValue::Double(typography.mono_size_adjust)),
        ("monoLigatures", JsonValue::Bool(typography.mono_ligatures)),
        ("opticalMargins", JsonValue::Bool(typography.optical_margins)),
        ("mathScale", JsonValue::Double(typography.math_scale)),
    ])
}

/// `TypographyConfig`'s synthesized `init(from:)`: every key required.
pub fn decode_typography(value: &Value) -> Result<TypographyConfig, DecodingError> {
    let c = value.keyed_container()?;
    Ok(TypographyConfig {
        preset: c.decode("preset", |value| value.raw_string_enum(BodyPreset::from_raw_value))?,
        body_size: c.decode("bodySize", Value::double_value)?,
        scale_ratio: c.decode("scaleRatio", Value::double_value)?,
        line_height_multiple: c.decode("lineHeightMultiple", Value::double_value)?,
        measure_characters: c.decode("measureCharacters", Value::double_value)?,
        mono_family: c.decode("monoFamily", Value::string_value)?,
        mono_size_adjust: c.decode("monoSizeAdjust", Value::double_value)?,
        mono_ligatures: c.decode("monoLigatures", Value::bool_value)?,
        optical_margins: c.decode("opticalMargins", Value::bool_value)?,
        math_scale: c.decode("mathScale", Value::double_value)?,
    })
}

/// `Preferences.Values`.
#[derive(Clone, Debug, PartialEq)]
pub struct Values {
    pub theme_name: String,
    pub dark_theme_name: String,
    /// When true the light/dark theme pair follows the system appearance.
    pub follows_system_appearance: bool,
    /// Tracks the one-time repair for settings written before following
    /// macOS appearance became the stable default (private in Swift).
    appearance_preference_version: i64,
    /// Quick Look's appearance: follows macOS by default.
    pub preview_appearance: PreviewAppearance,
    pub typography: TypographyConfig,
    /// Text size is app-wide rather than per document (§7.1).
    pub text_size_adjustment: f64,
    /// Defaults **off**: agents and code hate smart quotes (§6.4).
    pub typographic_substitution: bool,
    pub show_invisibles: bool,
    /// Source-wrapped prose reads as one paragraph while source bytes stay
    /// intact.
    pub reflow_hard_wrapped_paragraphs: bool,
    pub typewriter_scrolling: bool,
    pub focus_mode: bool,
    /// Auto-collapse code blocks longer than this in Read mode (§5.1).
    pub code_block_collapse_threshold: i64,
    /// The rendered document stays editable; Source is an explicit choice.
    pub default_mode: RenderMode,
    pub restore_session: bool,
    pub external_editor: ExternalEditor,
    pub resolve_path_tokens: bool,
    /// Extra directories to scan for siblings, relative to the document
    /// (§8.7).
    pub sibling_scan_directories: Vec<String>,
    pub history_maximum_days: i64,
    pub history_maximum_megabytes: i64,
    pub watch_files: bool,
    pub vim_keys: bool,
    /// Off by default: autosave can interfere with agents also writing the
    /// same file.
    pub autosave_enabled: bool,
    /// §14's recommendation: reveal markers at the primary caret only.
    pub reveal_markers_at_all_cursors: bool,
    /// Defaults **off**: DESIGN.md's "Avoid" list names a permanent status
    /// bar outright.
    pub show_status_bar: bool,
    /// Beyond this size Read mode switches to windowed rendering (§15 Q4).
    pub large_file_threshold_megabytes: i64,
    /// How many times the app has been launched, counting this one.
    pub launch_count: i64,
    /// Set the first time the tour is opened, from anywhere.
    pub has_taken_tour: bool,
    /// Set once the first-run setup panel has been answered, either way.
    pub has_answered_setup: bool,
    /// Where the bundle was when LaunchServices and `pluginkit` were last
    /// told about it.
    pub last_registered_bundle_path: String,
}

fn default_sibling_scan_directories() -> Vec<String> {
    ["docs", "plans", ".claude", "notes", "specs"].map(str::to_owned).to_vec()
}

impl Default for Values {
    /// `Values()`.
    fn default() -> Values {
        Values {
            theme_name: "Paper Light".to_owned(),
            dark_theme_name: "Warm Dark".to_owned(),
            follows_system_appearance: true,
            appearance_preference_version: 1,
            preview_appearance: PreviewAppearance::System,
            typography: TypographyConfig::default_config(),
            text_size_adjustment: 0.0,
            typographic_substitution: false,
            show_invisibles: false,
            reflow_hard_wrapped_paragraphs: true,
            typewriter_scrolling: false,
            focus_mode: false,
            code_block_collapse_threshold: 20,
            default_mode: RenderMode::Live,
            restore_session: true,
            external_editor: ExternalEditor::SystemDefault,
            resolve_path_tokens: true,
            sibling_scan_directories: default_sibling_scan_directories(),
            history_maximum_days: 30,
            history_maximum_megabytes: 500,
            watch_files: true,
            vim_keys: false,
            autosave_enabled: false,
            reveal_markers_at_all_cursors: false,
            show_status_bar: false,
            large_file_threshold_megabytes: 5,
            launch_count: 0,
            has_taken_tour: false,
            has_answered_setup: false,
            last_registered_bundle_path: String::new(),
        }
    }
}

/// `ThemePreferenceSlot`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemePreferenceSlot {
    Light,
    Dark,
}

impl Values {
    /// The private `appearancePreferenceVersion`, for tests and dumps.
    pub fn appearance_preference_version(&self) -> i64 {
        self.appearance_preference_version
    }

    /// Theme choice and appearance mode are separate decisions. A palette
    /// selection must never silently stop the app following macOS.
    pub fn select_theme(&mut self, name: &str, slot: ThemePreferenceSlot) {
        match slot {
            ThemePreferenceSlot::Light => self.theme_name = name.to_owned(),
            ThemePreferenceSlot::Dark => self.dark_theme_name = name.to_owned(),
        }
    }

    /// Synthesized `encode(to:)`, members in `CodingKeys` order.
    pub fn encode(&self) -> JsonValue {
        JsonValue::object([
            ("themeName", JsonValue::from(self.theme_name.as_str())),
            ("darkThemeName", JsonValue::from(self.dark_theme_name.as_str())),
            ("followsSystemAppearance", JsonValue::Bool(self.follows_system_appearance)),
            ("appearancePreferenceVersion", JsonValue::Int(self.appearance_preference_version)),
            ("previewAppearance", JsonValue::from(self.preview_appearance.raw_value())),
            ("typography", encode_typography(&self.typography)),
            ("textSizeAdjustment", JsonValue::Double(self.text_size_adjustment)),
            ("typographicSubstitution", JsonValue::Bool(self.typographic_substitution)),
            ("showInvisibles", JsonValue::Bool(self.show_invisibles)),
            ("reflowHardWrappedParagraphs", JsonValue::Bool(self.reflow_hard_wrapped_paragraphs)),
            ("typewriterScrolling", JsonValue::Bool(self.typewriter_scrolling)),
            ("focusMode", JsonValue::Bool(self.focus_mode)),
            ("codeBlockCollapseThreshold", JsonValue::Int(self.code_block_collapse_threshold)),
            ("defaultMode", JsonValue::from(self.default_mode.raw_value())),
            ("restoreSession", JsonValue::Bool(self.restore_session)),
            ("externalEditor", JsonValue::from(self.external_editor.raw_value())),
            ("resolvePathTokens", JsonValue::Bool(self.resolve_path_tokens)),
            (
                "siblingScanDirectories",
                JsonValue::Array(self.sibling_scan_directories.iter().map(|name| JsonValue::from(name.as_str())).collect()),
            ),
            ("historyMaximumDays", JsonValue::Int(self.history_maximum_days)),
            ("historyMaximumMegabytes", JsonValue::Int(self.history_maximum_megabytes)),
            ("watchFiles", JsonValue::Bool(self.watch_files)),
            ("vimKeys", JsonValue::Bool(self.vim_keys)),
            ("autosaveEnabled", JsonValue::Bool(self.autosave_enabled)),
            ("revealMarkersAtAllCursors", JsonValue::Bool(self.reveal_markers_at_all_cursors)),
            ("showStatusBar", JsonValue::Bool(self.show_status_bar)),
            ("largeFileThresholdMegabytes", JsonValue::Int(self.large_file_threshold_megabytes)),
            ("launchCount", JsonValue::Int(self.launch_count)),
            ("hasTakenTour", JsonValue::Bool(self.has_taken_tour)),
            ("hasAnsweredSetup", JsonValue::Bool(self.has_answered_setup)),
            ("lastRegisteredBundlePath", JsonValue::from(self.last_registered_bundle_path.as_str())),
        ])
    }

    /// The custom `init(from:)`: each key falls back to its default when it is
    /// missing, `null`, or of the wrong type (`try?`), so only a non-object
    /// (or unparseable) file fails as a whole.
    pub fn decode(value: &Value) -> Result<Values, DecodingError> {
        let c = value.keyed_container()?;
        fn get<'a, T>(
            c: &decodable::Keyed<'a>,
            key: &str,
            fallback: T,
            decode: impl Fn(&'a Value) -> Result<T, DecodingError>,
        ) -> T {
            c.decode_if_present(key, decode).ok().flatten().unwrap_or(fallback)
        }
        let string = |value: &Value| value.string_value();
        let mut values = Values {
            theme_name: get(&c, "themeName", "Paper Light".to_owned(), string),
            dark_theme_name: get(&c, "darkThemeName", "Warm Dark".to_owned(), string),
            follows_system_appearance: get(&c, "followsSystemAppearance", true, Value::bool_value),
            appearance_preference_version: get(&c, "appearancePreferenceVersion", 0, Value::int_value),
            ..Values::default()
        };
        if values.appearance_preference_version < 1 {
            values.follows_system_appearance = true;
            values.appearance_preference_version = 1;
        }
        values.preview_appearance = get(&c, "previewAppearance", PreviewAppearance::System, |value| {
            value.raw_string_enum(PreviewAppearance::from_raw_value)
        });
        values.typography = get(&c, "typography", TypographyConfig::default_config(), decode_typography);
        values.text_size_adjustment = get(&c, "textSizeAdjustment", 0.0, Value::double_value);
        values.typographic_substitution = get(&c, "typographicSubstitution", false, Value::bool_value);
        values.show_invisibles = get(&c, "showInvisibles", false, Value::bool_value);
        values.reflow_hard_wrapped_paragraphs = get(&c, "reflowHardWrappedParagraphs", true, Value::bool_value);
        values.typewriter_scrolling = get(&c, "typewriterScrolling", false, Value::bool_value);
        values.focus_mode = get(&c, "focusMode", false, Value::bool_value);
        values.code_block_collapse_threshold = get(&c, "codeBlockCollapseThreshold", 20, Value::int_value);
        values.default_mode = get(&c, "defaultMode", RenderMode::Live, decode_render_mode).normalized_for_editing();
        values.restore_session = get(&c, "restoreSession", true, Value::bool_value);
        values.external_editor = get(&c, "externalEditor", ExternalEditor::SystemDefault, |value| {
            value.raw_string_enum(ExternalEditor::from_raw_value)
        });
        values.resolve_path_tokens = get(&c, "resolvePathTokens", true, Value::bool_value);
        values.sibling_scan_directories =
            get(&c, "siblingScanDirectories", default_sibling_scan_directories(), |value| value.array_of(string));
        values.history_maximum_days = get(&c, "historyMaximumDays", 30, Value::int_value);
        values.history_maximum_megabytes = get(&c, "historyMaximumMegabytes", 500, Value::int_value);
        values.watch_files = get(&c, "watchFiles", true, Value::bool_value);
        values.vim_keys = get(&c, "vimKeys", false, Value::bool_value);
        values.autosave_enabled = get(&c, "autosaveEnabled", false, Value::bool_value);
        values.reveal_markers_at_all_cursors = get(&c, "revealMarkersAtAllCursors", false, Value::bool_value);
        values.show_status_bar = get(&c, "showStatusBar", false, Value::bool_value);
        values.large_file_threshold_megabytes = get(&c, "largeFileThresholdMegabytes", 5, Value::int_value);
        values.launch_count = get(&c, "launchCount", 0, Value::int_value);
        values.has_taken_tour = get(&c, "hasTakenTour", false, Value::bool_value);
        values.has_answered_setup = get(&c, "hasAnsweredSetup", false, Value::bool_value);
        values.last_registered_bundle_path = get(&c, "lastRegisteredBundlePath", String::new(), string);
        Ok(values)
    }

    /// `JSONDecoder().decode(Values.self, from: data)`.
    pub fn decoded(data: &[u8]) -> Result<Values, DecodingError> {
        Values::decode(&decodable::parse(data)?)
    }

    /// What `persist()` writes: `JSONEncoder` with
    /// `[.prettyPrinted, .sortedKeys]`.
    pub fn persisted_data(&self) -> Vec<u8> {
        json_encoder::encode(&self.encode(), OutputFormatting::PRETTY_SORTED)
    }
}

/// `Preferences.Load`: how the settings file was read at launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Load {
    Absent,
    Loaded,
    /// The file existed but could not be decoded. The original was moved
    /// aside so the user can inspect or repair it.
    Recovered { backup: Option<FileUrl> },
}

/// `Preferences.didChange`.
pub const DID_CHANGE: &str = "com.bitemyapp.upleft.preferencesDidChange";

type LoadFaultHandler = Box<dyn Fn(&Load) + Send + Sync>;
type PersistenceFailureHandler = Box<dyn Fn(&str) + Send + Sync>;

struct State {
    values: Values,
    load: Load,
    last_persistence_error: Option<String>,
    on_load_fault: Option<std::sync::Arc<LoadFaultHandler>>,
    on_persistence_failure: Option<std::sync::Arc<PersistenceFailureHandler>>,
}

/// `Preferences`.
pub struct Preferences {
    state: Mutex<State>,
    preferences_file: FileUrl,
    /// `Preferences.shared` publishes the Quick Look appearance and pushes
    /// history limits into `SnapshotStore.shared`; a testing instance does
    /// neither unless given a store.
    publishes_preview_appearance: bool,
    snapshot_store: Option<SnapshotStore>,
    posts_notifications: bool,
}

static SHARED: OnceLock<Preferences> = OnceLock::new();

impl Preferences {
    /// `Preferences.shared`.
    pub fn shared() -> &'static Preferences {
        SHARED.get_or_init(|| {
            Preferences::load_from(
                app_paths::preferences_file(),
                true,
                Some(SnapshotStore::shared().clone()),
                true,
            )
        })
    }

    /// A `Preferences` reading and writing `preferences_file` only: no Quick
    /// Look publication and no notification. `snapshot_store`, when given,
    /// receives the history limits as `SnapshotStore.shared` does.
    pub fn for_testing(preferences_file: FileUrl, snapshot_store: Option<SnapshotStore>) -> Preferences {
        Preferences::load_from(preferences_file, false, snapshot_store, false)
    }

    fn load_from(
        preferences_file: FileUrl,
        publishes_preview_appearance: bool,
        snapshot_store: Option<SnapshotStore>,
        posts_notifications: bool,
    ) -> Preferences {
        let (loaded, load) = Preferences::read(&preferences_file);
        let values = match loaded {
            Some(loaded) => loaded,
            None => Values {
                // No usable settings: adopt whichever editor the user
                // actually has (§8.4).
                external_editor: ExternalEditor::best_available(),
                ..Values::default()
            },
        };
        let preferences = Preferences {
            state: Mutex::new(State {
                values: values.clone(),
                load,
                last_persistence_error: None,
                on_load_fault: None,
                on_persistence_failure: None,
            }),
            preferences_file,
            publishes_preview_appearance,
            snapshot_store,
            posts_notifications,
        };
        preferences.publish_preview_appearance(&values);
        preferences.apply_history_limits(&values);
        preferences
    }

    /// Reads the settings file, preserving a file that exists but cannot be
    /// decoded.
    fn read(url: &FileUrl) -> (Option<Values>, Load) {
        let Some(data) = file_manager::data_contents_of(url) else {
            return (None, Load::Absent);
        };
        if let Ok(decoded) = Values::decoded(&data) {
            return (Some(decoded), Load::Loaded);
        }
        (None, Load::Recovered { backup: Preferences::move_aside(url) })
    }

    /// Moves an undecodable settings file to `preferences.json.bad` so the
    /// next write does not destroy it.
    fn move_aside(url: &FileUrl) -> Option<FileUrl> {
        let backup = url.appending_path_extension("bad");
        let _ = file_manager::remove_item(&backup);
        file_manager::move_item(url, &backup).ok().map(|()| backup)
    }

    pub fn values(&self) -> Values {
        self.state.lock().unwrap().values.clone()
    }

    /// What happened when the settings file was read.
    pub fn load(&self) -> Load {
        self.state.lock().unwrap().load.clone()
    }

    /// True when this launch found no settings file at all.
    pub fn is_first_run(&self) -> bool {
        self.load() == Load::Absent
    }

    /// Installs the load-fault handler; assigning it after the fact reports
    /// immediately, which it must, because loading happens at construction.
    pub fn set_on_load_fault(&self, handler: Option<LoadFaultHandler>) {
        let (handler, load) = {
            let mut state = self.state.lock().unwrap();
            state.on_load_fault = handler.map(std::sync::Arc::new);
            (state.on_load_fault.clone(), state.load.clone())
        };
        if let (Some(handler), Load::Recovered { .. }) = (handler, &load) {
            handler(&load);
        }
    }

    /// Last settings write failure, if any.
    pub fn last_persistence_error(&self) -> Option<String> {
        self.state.lock().unwrap().last_persistence_error.clone()
    }

    /// Called on the *first* failed write of a run of failures.
    pub fn set_on_persistence_failure(&self, handler: Option<PersistenceFailureHandler>) {
        self.state.lock().unwrap().on_persistence_failure = handler.map(std::sync::Arc::new);
    }

    /// Single mutation point, so persistence and the change notification can
    /// never be forgotten at a call site.
    pub fn update(&self, mutate: impl FnOnce(&mut Values)) {
        let (copy, changed) = {
            let mut state = self.state.lock().unwrap();
            let mut copy = state.values.clone();
            mutate(&mut copy);
            let changed = copy != state.values;
            state.values = copy.clone();
            (copy, changed)
        };
        if changed {
            self.persist(&copy);
            self.post_did_change();
        }
        self.apply_history_limits(&copy);
    }

    fn persist(&self, values: &Values) {
        let _ = app_paths::create(&self.preferences_file.deleting_last_path_component());
        let result = file_manager::write_atomic(&values.persisted_data(), &self.preferences_file);
        match result {
            Ok(()) => {
                self.publish_preview_appearance(values);
                self.state.lock().unwrap().last_persistence_error = None;
            }
            Err(error) => {
                // Edge-triggered: report the first failure of a run and stay
                // quiet until a write succeeds again.
                let handler = {
                    let mut state = self.state.lock().unwrap();
                    let is_first_of_run = state.last_persistence_error.is_none();
                    state.last_persistence_error = Some(error.clone());
                    if is_first_of_run { state.on_persistence_failure.clone() } else { None }
                };
                if let Some(handler) = handler {
                    handler(&error);
                }
            }
        }
    }

    fn publish_preview_appearance(&self, values: &Values) {
        if self.publishes_preview_appearance {
            PreviewAppearanceStore::write(values.preview_appearance, &values.theme_name, &values.dark_theme_name);
        }
    }

    fn apply_history_limits(&self, values: &Values) {
        if let Some(store) = &self.snapshot_store {
            store.set_maximum_age(values.history_maximum_days as f64 * 86_400.0);
            store.set_maximum_bytes((values.history_maximum_megabytes * 1024 * 1024) as isize);
        }
    }

    fn post_did_change(&self) {
        if self.posts_notifications {
            let center = NSNotificationCenter::defaultCenter();
            unsafe { center.postNotificationName_object(&NSString::from_str(DID_CHANGE), None) };
        }
    }

    // MARK: Derived

    /// Typography with the app-wide text size adjustment folded in (§7.1).
    pub fn effective_typography(&self) -> TypographyConfig {
        let values = self.values();
        effective_typography(&values)
    }

    pub fn large_file_threshold_bytes(&self) -> i64 {
        self.values().large_file_threshold_megabytes * 1024 * 1024
    }

    pub fn theme_name(&self, appearance: &NSAppearance) -> String {
        let values = self.values();
        if !values.follows_system_appearance {
            return values.theme_name;
        }
        // SAFETY: AppKit exports the appearance names as immutable globals.
        let (aqua, dark) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
        let is_dark = appearance
            .bestMatchFromAppearancesWithNames(&NSArray::from_slice(&[aqua, dark]))
            .is_some_and(|best| &*best == dark);
        if is_dark { values.dark_theme_name } else { values.theme_name }
    }
}

/// `effectiveTypography` over a `Values`.
pub fn effective_typography(values: &Values) -> TypographyConfig {
    let mut typography = values.typography.clone();
    let adjusted = typography.body_size + values.text_size_adjustment;
    // Swift's `max(10, min(28, x))`.
    let clamped_high = if adjusted < 28.0 { adjusted } else { 28.0 };
    typography.body_size = if clamped_high >= 10.0 { clamped_high } else { 10.0 };
    typography
}
