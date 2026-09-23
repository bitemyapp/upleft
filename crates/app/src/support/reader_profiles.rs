//! Port of `Sources/DownrightApp/Support/ReaderProfiles.swift`.
//!
//! A reader profile changes presentation only. It never changes document
//! text, the selected theme, or the document's saved state.
//!
//! The custom-profiles file is written by a `JSONEncoder` without
//! `.sortedKeys` (Swift's key order changes from run to run; the port writes
//! `CodingKeys` order).

use std::sync::Mutex;

use upleft_core::contracts::Uuid;
use upleft_foundation::decodable::{self, DecodableValue, DecodingError, Value};
use upleft_foundation::file_manager;
use upleft_foundation::json_encoder::{self, JsonValue, OutputFormatting};
use upleft_foundation::url::FileUrl;

use crate::ai::change_tracker::uuid_string;

/// `ReaderProfileID`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReaderProfileID {
    Documentation,
    LongForm,
    Academic,
    Specification,
    Github,
    Presentation,
}

impl ReaderProfileID {
    pub const ALL_CASES: [ReaderProfileID; 6] = [
        ReaderProfileID::Documentation,
        ReaderProfileID::LongForm,
        ReaderProfileID::Academic,
        ReaderProfileID::Specification,
        ReaderProfileID::Github,
        ReaderProfileID::Presentation,
    ];

    pub fn raw_value(self) -> &'static str {
        match self {
            ReaderProfileID::Documentation => "documentation",
            ReaderProfileID::LongForm => "long-form",
            ReaderProfileID::Academic => "academic",
            ReaderProfileID::Specification => "specification",
            ReaderProfileID::Github => "github",
            ReaderProfileID::Presentation => "presentation",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<ReaderProfileID> {
        ReaderProfileID::ALL_CASES.into_iter().find(|id| id.raw_value() == raw)
    }
}

/// `ReaderTypographyScale`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReaderTypographyScale {
    Compact,
    Standard,
    Large,
    ExtraLarge,
}

impl ReaderTypographyScale {
    pub const ALL_CASES: [ReaderTypographyScale; 4] = [
        ReaderTypographyScale::Compact,
        ReaderTypographyScale::Standard,
        ReaderTypographyScale::Large,
        ReaderTypographyScale::ExtraLarge,
    ];

    pub fn raw_value(self) -> &'static str {
        match self {
            ReaderTypographyScale::Compact => "compact",
            ReaderTypographyScale::Standard => "standard",
            ReaderTypographyScale::Large => "large",
            ReaderTypographyScale::ExtraLarge => "extra-large",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<ReaderTypographyScale> {
        ReaderTypographyScale::ALL_CASES.into_iter().find(|scale| scale.raw_value() == raw)
    }

    pub fn value(self) -> f64 {
        match self {
            ReaderTypographyScale::Compact => 0.9,
            ReaderTypographyScale::Standard => 1.0,
            ReaderTypographyScale::Large => 1.12,
            ReaderTypographyScale::ExtraLarge => 1.25,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            ReaderTypographyScale::Compact => "Compact",
            ReaderTypographyScale::Standard => "Standard",
            ReaderTypographyScale::Large => "Large",
            ReaderTypographyScale::ExtraLarge => "Extra large",
        }
    }
}

/// `ReaderChromeDensity`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReaderChromeDensity {
    Compact,
    Comfortable,
    Spacious,
}

impl ReaderChromeDensity {
    pub const ALL_CASES: [ReaderChromeDensity; 3] =
        [ReaderChromeDensity::Compact, ReaderChromeDensity::Comfortable, ReaderChromeDensity::Spacious];

    pub fn raw_value(self) -> &'static str {
        match self {
            ReaderChromeDensity::Compact => "compact",
            ReaderChromeDensity::Comfortable => "comfortable",
            ReaderChromeDensity::Spacious => "spacious",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<ReaderChromeDensity> {
        ReaderChromeDensity::ALL_CASES.into_iter().find(|density| density.raw_value() == raw)
    }

    pub fn title(self) -> &'static str {
        match self {
            ReaderChromeDensity::Compact => "Compact",
            ReaderChromeDensity::Comfortable => "Comfortable",
            ReaderChromeDensity::Spacious => "Spacious",
        }
    }
}

/// `ReaderMotionPreference`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReaderMotionPreference {
    FollowSystem,
    Reduced,
    Full,
}

impl ReaderMotionPreference {
    pub const ALL_CASES: [ReaderMotionPreference; 3] =
        [ReaderMotionPreference::FollowSystem, ReaderMotionPreference::Reduced, ReaderMotionPreference::Full];

    pub fn raw_value(self) -> &'static str {
        match self {
            ReaderMotionPreference::FollowSystem => "follow-system",
            ReaderMotionPreference::Reduced => "reduced",
            ReaderMotionPreference::Full => "full",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<ReaderMotionPreference> {
        ReaderMotionPreference::ALL_CASES.into_iter().find(|motion| motion.raw_value() == raw)
    }

    pub fn title(self) -> &'static str {
        match self {
            ReaderMotionPreference::FollowSystem => "Follow system",
            ReaderMotionPreference::Reduced => "Reduced",
            ReaderMotionPreference::Full => "Full",
        }
    }
}

/// `ReaderProfile`.
#[derive(Clone, Debug, PartialEq)]
pub struct ReaderProfile {
    pub id: String,
    pub name: String,
    pub is_built_in: bool,
    pub typography_scale: ReaderTypographyScale,
    pub measure_characters: f64,
    pub chrome_density: ReaderChromeDensity,
    pub motion_preference: ReaderMotionPreference,
}

impl ReaderProfile {
    /// The memberwise `init` with its defaults: not built in, standard scale,
    /// 70 characters (clamped to 68…72), comfortable, follow system.
    pub fn new(id: &str, name: &str) -> ReaderProfile {
        ReaderProfile::with(
            id,
            name,
            false,
            ReaderTypographyScale::Standard,
            70.0,
            ReaderChromeDensity::Comfortable,
            ReaderMotionPreference::FollowSystem,
        )
    }

    /// `init(id:name:isBuiltIn:typographyScale:measureCharacters:chromeDensity:motionPreference:)`.
    pub fn with(
        id: &str,
        name: &str,
        is_built_in: bool,
        typography_scale: ReaderTypographyScale,
        measure_characters: f64,
        chrome_density: ReaderChromeDensity,
        motion_preference: ReaderMotionPreference,
    ) -> ReaderProfile {
        // Swift's `min(72, max(68, measureCharacters))`.
        let at_least = if measure_characters >= 68.0 { measure_characters } else { 68.0 };
        let clamped = if at_least < 72.0 { at_least } else { 72.0 };
        ReaderProfile {
            id: id.to_owned(),
            name: name.to_owned(),
            is_built_in,
            typography_scale,
            measure_characters: clamped,
            chrome_density,
            motion_preference,
        }
    }

    /// `ReaderProfile.builtIns`.
    pub fn built_ins() -> Vec<ReaderProfile> {
        use ReaderChromeDensity as D;
        use ReaderMotionPreference as M;
        use ReaderTypographyScale as S;
        vec![
            ReaderProfile::with(ReaderProfileID::Documentation.raw_value(), "Documentation", true, S::Standard, 70.0, D::Comfortable, M::FollowSystem),
            ReaderProfile::with(ReaderProfileID::LongForm.raw_value(), "Long-form", true, S::Large, 68.0, D::Compact, M::FollowSystem),
            ReaderProfile::with(ReaderProfileID::Academic.raw_value(), "Academic", true, S::Standard, 68.0, D::Comfortable, M::FollowSystem),
            ReaderProfile::with(ReaderProfileID::Specification.raw_value(), "Specification", true, S::Compact, 72.0, D::Comfortable, M::FollowSystem),
            ReaderProfile::with(ReaderProfileID::Github.raw_value(), "GitHub", true, S::Standard, 72.0, D::Compact, M::FollowSystem),
            ReaderProfile::with(ReaderProfileID::Presentation.raw_value(), "Presentation", true, S::ExtraLarge, 68.0, D::Spacious, M::FollowSystem),
        ]
    }

    /// `ReaderProfile.custom(name:)`: a fresh `UUID().uuidString` id.
    pub fn custom(name: &str) -> ReaderProfile {
        ReaderProfile::custom_with_id(&uuid_string(&Uuid::new_v4()), name)
    }

    /// `ReaderProfile.custom(id:name:)`.
    pub fn custom_with_id(id: &str, name: &str) -> ReaderProfile {
        let blank = upleft_swift_text::trim_whitespaces_and_newlines(name).is_empty();
        ReaderProfile::new(id, if blank { "Custom" } else { name })
    }

    /// Synthesized `encode(to:)`.
    pub fn encode(&self) -> JsonValue {
        JsonValue::object([
            ("id", JsonValue::from(self.id.as_str())),
            ("name", JsonValue::from(self.name.as_str())),
            ("isBuiltIn", JsonValue::Bool(self.is_built_in)),
            ("typographyScale", JsonValue::from(self.typography_scale.raw_value())),
            ("measureCharacters", JsonValue::Double(self.measure_characters)),
            ("chromeDensity", JsonValue::from(self.chrome_density.raw_value())),
            ("motionPreference", JsonValue::from(self.motion_preference.raw_value())),
        ])
    }

    /// Synthesized `init(from:)`: every key required, no clamping.
    pub fn decode(value: &Value) -> Result<ReaderProfile, DecodingError> {
        let c = value.keyed_container()?;
        Ok(ReaderProfile {
            id: c.decode("id", Value::string_value)?,
            name: c.decode("name", Value::string_value)?,
            is_built_in: c.decode("isBuiltIn", Value::bool_value)?,
            typography_scale: c.decode("typographyScale", |value| value.raw_string_enum(ReaderTypographyScale::from_raw_value))?,
            measure_characters: c.decode("measureCharacters", Value::double_value)?,
            chrome_density: c.decode("chromeDensity", |value| value.raw_string_enum(ReaderChromeDensity::from_raw_value))?,
            motion_preference: c
                .decode("motionPreference", |value| value.raw_string_enum(ReaderMotionPreference::from_raw_value))?,
        })
    }
}

/// `ReaderProfileStore`.
pub trait ReaderProfileStore {
    fn load_custom_profiles(&self) -> Vec<ReaderProfile>;
    fn save_custom_profiles(&self, profiles: &[ReaderProfile]);
}

/// `JSONReaderProfileStore`.
pub struct JSONReaderProfileStore {
    url: FileUrl,
}

impl JSONReaderProfileStore {
    pub fn new(url: FileUrl) -> JSONReaderProfileStore {
        JSONReaderProfileStore { url }
    }
}

impl ReaderProfileStore for JSONReaderProfileStore {
    fn load_custom_profiles(&self) -> Vec<ReaderProfile> {
        let Some(data) = file_manager::data_contents_of(&self.url) else {
            return Vec::new();
        };
        match decodable::parse(&data).and_then(|value| value.array_of(ReaderProfile::decode)) {
            Ok(profiles) => profiles.into_iter().filter(|profile| !profile.is_built_in).collect(),
            Err(_) => Vec::new(),
        }
    }

    fn save_custom_profiles(&self, profiles: &[ReaderProfile]) {
        let custom: Vec<JsonValue> =
            profiles.iter().filter(|profile| !profile.is_built_in).map(ReaderProfile::encode).collect();
        let data = json_encoder::encode(&JsonValue::Array(custom), OutputFormatting::DEFAULT);
        let _ = file_manager::create_directory(&self.url.deleting_last_path_component(), true);
        let _ = file_manager::write_atomic(&data, &self.url);
    }
}

/// `InMemoryReaderProfileStore`.
#[derive(Default)]
pub struct InMemoryReaderProfileStore {
    pub profiles: Mutex<Vec<ReaderProfile>>,
}

impl InMemoryReaderProfileStore {
    pub fn new(profiles: Vec<ReaderProfile>) -> InMemoryReaderProfileStore {
        InMemoryReaderProfileStore { profiles: Mutex::new(profiles) }
    }
}

impl ReaderProfileStore for InMemoryReaderProfileStore {
    fn load_custom_profiles(&self) -> Vec<ReaderProfile> {
        self.profiles.lock().unwrap().clone()
    }

    fn save_custom_profiles(&self, profiles: &[ReaderProfile]) {
        *self.profiles.lock().unwrap() = profiles.to_vec();
    }
}
