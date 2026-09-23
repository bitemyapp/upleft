//! Port of `Sources/DownrightApp/Review/ReviewSidecar.swift`.
//!
//! Review comments and suggestions live in a sidecar next to the document
//! (`note.md.downright-reviews.json`), written by a `JSONEncoder` with
//! `[.prettyPrinted, .sortedKeys]`, so the port's bytes equal Swift's.

use std::io::Read;

use objc2_foundation::{NSMatchingOptions, NSRange as FRange, NSRegularExpression, NSRegularExpressionOptions, NSString};
use upleft_core::contracts::{TextEdit, Uuid};
use upleft_core::ns_range::NSRange;
use upleft_foundation::decodable::{self, DecodableValue, DecodingError, Value};
use upleft_foundation::file_manager;
use upleft_foundation::json_encoder::{self, JsonValue, OutputFormatting};
use upleft_foundation::url::FileUrl;

use crate::ai::change_tracker::uuid_string;
use crate::review::review_anchor_resolver::{CONTEXT_LENGTH, ReviewAnchorResolver, Source};

/// `ReviewKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReviewKind {
    Comment,
    Suggestion,
}

impl ReviewKind {
    pub const ALL_CASES: [ReviewKind; 2] = [ReviewKind::Comment, ReviewKind::Suggestion];

    pub fn raw_value(self) -> &'static str {
        match self {
            ReviewKind::Comment => "comment",
            ReviewKind::Suggestion => "suggestion",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<ReviewKind> {
        ReviewKind::ALL_CASES.into_iter().find(|kind| kind.raw_value() == raw)
    }
}

/// `ReviewState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReviewState {
    Open,
    Resolved,
    Rejected,
}

impl ReviewState {
    pub const ALL_CASES: [ReviewState; 3] = [ReviewState::Open, ReviewState::Resolved, ReviewState::Rejected];

    pub fn raw_value(self) -> &'static str {
        match self {
            ReviewState::Open => "open",
            ReviewState::Resolved => "resolved",
            ReviewState::Rejected => "rejected",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<ReviewState> {
        ReviewState::ALL_CASES.into_iter().find(|state| state.raw_value() == raw)
    }
}

/// `ReviewAnchorStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReviewAnchorStatus {
    Exact,
    Shifted,
    Stale,
    Orphan,
}

impl ReviewAnchorStatus {
    pub fn raw_value(self) -> &'static str {
        match self {
            ReviewAnchorStatus::Exact => "exact",
            ReviewAnchorStatus::Shifted => "shifted",
            ReviewAnchorStatus::Stale => "stale",
            ReviewAnchorStatus::Orphan => "orphan",
        }
    }
}

/// `ReviewAnchor`: the original UTF-16 range and enough context to find it
/// again after an edit.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ReviewAnchor {
    pub range: NSRange,
    pub selected_text: String,
    pub before_fingerprint: String,
    pub after_fingerprint: String,
}

impl ReviewAnchor {
    /// The custom `encode(to:)`: `location`, `length`, then the texts.
    pub fn encode(&self) -> JsonValue {
        JsonValue::object([
            ("location", JsonValue::Int(self.range.location as i64)),
            ("length", JsonValue::Int(self.range.length as i64)),
            ("selectedText", JsonValue::from(self.selected_text.as_str())),
            ("beforeFingerprint", JsonValue::from(self.before_fingerprint.as_str())),
            ("afterFingerprint", JsonValue::from(self.after_fingerprint.as_str())),
        ])
    }

    /// The custom `init(from:)`: every key required.
    pub fn decode(value: &Value) -> Result<ReviewAnchor, DecodingError> {
        let c = value.keyed_container()?;
        let location = c.decode("location", Value::int_value)? as isize;
        let length = c.decode("length", Value::int_value)? as isize;
        Ok(ReviewAnchor {
            range: NSRange::new(location, length),
            selected_text: c.decode("selectedText", Value::string_value)?,
            before_fingerprint: c.decode("beforeFingerprint", Value::string_value)?,
            after_fingerprint: c.decode("afterFingerprint", Value::string_value)?,
        })
    }
}

/// `ReviewItem`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewItem {
    pub id: Uuid,
    pub kind: ReviewKind,
    pub anchor: ReviewAnchor,
    pub body: String,
    pub replacement: Option<String>,
    pub state: ReviewState,
}

impl ReviewItem {
    /// `ReviewItem(kind:anchor:body:replacement:state:)` with a fresh id.
    pub fn new(kind: ReviewKind, anchor: ReviewAnchor, body: &str, replacement: Option<&str>, state: ReviewState) -> ReviewItem {
        ReviewItem::with_id(Uuid::new_v4(), kind, anchor, body, replacement, state)
    }

    pub fn with_id(
        id: Uuid,
        kind: ReviewKind,
        anchor: ReviewAnchor,
        body: &str,
        replacement: Option<&str>,
        state: ReviewState,
    ) -> ReviewItem {
        ReviewItem { id, kind, anchor, body: body.to_owned(), replacement: replacement.map(str::to_owned), state }
    }

    pub fn title(&self) -> &'static str {
        if self.kind == ReviewKind::Comment { "Comment" } else { "Suggestion" }
    }

    /// Synthesized `encode(to:)`; `replacement` only when present.
    pub fn encode(&self) -> JsonValue {
        let mut members = vec![
            ("id".to_owned(), JsonValue::from(uuid_string(&self.id))),
            ("kind".to_owned(), JsonValue::from(self.kind.raw_value())),
            ("anchor".to_owned(), self.anchor.encode()),
            ("body".to_owned(), JsonValue::from(self.body.as_str())),
        ];
        JsonValue::push_if_present(&mut members, "replacement", self.replacement.as_deref().map(JsonValue::from));
        members.push(("state".to_owned(), JsonValue::from(self.state.raw_value())));
        JsonValue::Object(members)
    }

    /// Synthesized `init(from:)`.
    pub fn decode(value: &Value) -> Result<ReviewItem, DecodingError> {
        let c = value.keyed_container()?;
        Ok(ReviewItem {
            id: Uuid::from_bytes(c.decode("id", Value::uuid_value)?),
            kind: c.decode("kind", |value| value.raw_string_enum(ReviewKind::from_raw_value))?,
            anchor: c.decode("anchor", ReviewAnchor::decode)?,
            body: c.decode("body", Value::string_value)?,
            replacement: c.decode_if_present("replacement", Value::string_value)?,
            state: c.decode("state", |value| value.raw_string_enum(ReviewState::from_raw_value))?,
        })
    }
}

/// `ReviewSidecar`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewSidecar {
    pub version: i64,
    pub reviews: Vec<ReviewItem>,
}

impl Default for ReviewSidecar {
    fn default() -> Self {
        ReviewSidecar { version: 1, reviews: Vec::new() }
    }
}

impl ReviewSidecar {
    pub fn encode(&self) -> JsonValue {
        JsonValue::object([
            ("version", JsonValue::Int(self.version)),
            ("reviews", JsonValue::Array(self.reviews.iter().map(ReviewItem::encode).collect())),
        ])
    }

    /// Synthesized `init(from:)`: both keys required, defaults aside.
    pub fn decode(value: &Value) -> Result<ReviewSidecar, DecodingError> {
        let c = value.keyed_container()?;
        Ok(ReviewSidecar {
            version: c.decode("version", Value::int_value)?,
            reviews: c.decode("reviews", |value| value.array_of(ReviewItem::decode))?,
        })
    }
}

/// Why a sidecar could not be read or written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewSidecarError {
    /// `CocoaError(.fileReadTooLarge)`.
    FileReadTooLarge,
    /// The file handle could not be opened or read.
    Read(String),
    Decoding(DecodingError),
    Write(String),
}

impl ReviewSidecarError {
    /// `(error as NSError).code` where the port knows it: 263 for
    /// `fileReadTooLarge`, the decoding error's code. `None` otherwise.
    pub fn code(&self) -> Option<isize> {
        match self {
            ReviewSidecarError::FileReadTooLarge => Some(263),
            ReviewSidecarError::Decoding(error) => Some(error.code()),
            _ => None,
        }
    }
}

/// `ReviewSidecarStore`.
pub trait ReviewSidecarStore {
    fn load(&self, document_url: &FileUrl) -> Result<ReviewSidecar, ReviewSidecarError>;
    fn save(&self, sidecar: &ReviewSidecar, document_url: &FileUrl) -> Result<(), ReviewSidecarError>;
}

/// `LocalReviewSidecarStore`.
#[derive(Default)]
pub struct LocalReviewSidecarStore;

impl LocalReviewSidecarStore {
    /// Sidecars are repository-controlled input. Bound the read before JSON
    /// decoding so a checkout cannot make opening a small Markdown file pull
    /// an arbitrarily large adjacent review file into memory.
    pub const MAXIMUM_BYTES: usize = 8 * 1024 * 1024;

    pub fn new() -> LocalReviewSidecarStore {
        LocalReviewSidecarStore
    }

    pub fn sidecar_url(document_url: &FileUrl) -> FileUrl {
        document_url.appending_path_extension("downright-reviews.json")
    }

    /// What `save` writes.
    pub fn encoded(sidecar: &ReviewSidecar) -> Vec<u8> {
        json_encoder::encode(&sidecar.encode(), OutputFormatting::PRETTY_SORTED)
    }
}

impl ReviewSidecarStore for LocalReviewSidecarStore {
    fn load(&self, document_url: &FileUrl) -> Result<ReviewSidecar, ReviewSidecarError> {
        let url = Self::sidecar_url(document_url);
        if !file_manager::file_exists(&url.path()) {
            return Ok(ReviewSidecar::default());
        }
        // `FileHandle(forReadingFrom:)` then `read(upToCount: max + 1)`.
        let file = std::fs::File::open(url.path()).map_err(|error| ReviewSidecarError::Read(error.to_string()))?;
        let mut data = Vec::new();
        file.take(Self::MAXIMUM_BYTES as u64 + 1)
            .read_to_end(&mut data)
            .map_err(|error| ReviewSidecarError::Read(error.to_string()))?;
        if data.len() > Self::MAXIMUM_BYTES {
            return Err(ReviewSidecarError::FileReadTooLarge);
        }
        decodable::parse(&data).and_then(|value| ReviewSidecar::decode(&value)).map_err(ReviewSidecarError::Decoding)
    }

    fn save(&self, sidecar: &ReviewSidecar, document_url: &FileUrl) -> Result<(), ReviewSidecarError> {
        let url = Self::sidecar_url(document_url);
        file_manager::create_directory(&url.deleting_last_path_component(), true).map_err(ReviewSidecarError::Write)?;
        file_manager::write_atomic(&Self::encoded(sidecar), &url).map_err(ReviewSidecarError::Write)
    }
}

/// `ReviewResolution`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReviewResolution {
    pub status: ReviewAnchorStatus,
    pub range: Option<NSRange>,
}

/// `ReviewApplyResult`.
#[derive(Clone, Debug, PartialEq)]
pub enum ReviewApplyResult {
    Applied(TextEdit),
    Stale(ReviewAnchorStatus),
}

/// `ReviewSidecarEngine`.
pub struct ReviewSidecarEngine;

impl ReviewSidecarEngine {
    pub fn make_review(
        kind: ReviewKind,
        text: &str,
        range: NSRange,
        body: &str,
        replacement: Option<&str>,
    ) -> Option<ReviewItem> {
        if upleft_swift_text::trim_whitespaces_and_newlines(body).is_empty() {
            return None;
        }
        let anchor = ReviewAnchorResolver::make_anchor(text, range, CONTEXT_LENGTH)?;
        Some(ReviewItem::new(kind, anchor, body, replacement, ReviewState::Open))
    }

    pub fn apply_suggestion(review: &ReviewItem, text: &str) -> ReviewApplyResult {
        let (ReviewKind::Suggestion, Some(replacement)) = (review.kind, review.replacement.as_deref()) else {
            return ReviewApplyResult::Stale(ReviewAnchorStatus::Orphan);
        };
        let resolution = ReviewAnchorResolver::resolve(&review.anchor, text, CONTEXT_LENGTH);
        let Some(range) = resolution.range.filter(|_| {
            resolution.status == ReviewAnchorStatus::Exact || resolution.status == ReviewAnchorStatus::Shifted
        }) else {
            return ReviewApplyResult::Stale(resolution.status);
        };
        let source = Source::new(text).substring(range);
        if !upleft_swift_text::str_eq(&source, &review.anchor.selected_text) {
            return ReviewApplyResult::Stale(ReviewAnchorStatus::Stale);
        }
        ReviewApplyResult::Applied(TextEdit::new(range, replacement, "Apply Suggestion", None))
    }

    pub fn critic_markup_ranges(text: &str) -> Vec<NSRange> {
        let patterns = ["\\+\\+[^+]+\\+\\+", "\\{--[^-]+--\\}", "==[^=]+=="];
        let source = NSString::from_str(text);
        let length = source.length();
        patterns
            .iter()
            .flat_map(|pattern| {
                let Ok(expression) = NSRegularExpression::regularExpressionWithPattern_options_error(
                    &NSString::from_str(pattern),
                    NSRegularExpressionOptions::empty(),
                ) else {
                    return Vec::new();
                };
                expression
                    .matchesInString_options_range(&source, NSMatchingOptions::empty(), FRange::new(0, length))
                    .iter()
                    .map(|result| {
                        let range = result.range();
                        NSRange::new(range.location as isize, range.length as isize)
                    })
                    .collect()
            })
            .collect()
    }
}
