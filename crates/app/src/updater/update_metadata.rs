//! Port of `Sources/DownrightApp/Updater/UpdateMetadata.swift`.
//!
//! `UpdateMetadata` and `UpdateFailure` are plain values. Their `String`
//! fields compare as Swift `String`s do (canonical equivalence, through
//! `upleft-swift-text`), and their URLs are Foundation's own (`NSURL`), so
//! the synthesized `Equatable` of the Swift structs is reproduced exactly.

use std::fmt;

use objc2::rc::Retained;
use objc2_foundation::{NSError, NSObjectProtocol, NSString, NSURL};

/// Swift `URL` for the web URLs the updater carries (`releaseNotesURL`,
/// `infoURL`, the appcast feed): a thin wrapper around Foundation's `NSURL`.
///
/// `URL(string:)` and `NSURL(string:)` parse identically on macOS 26 (a probe
/// over schemes, empty and bracketed hosts, IDNA, spaces, percent escapes and
/// `%00` agreed on every case), except that Swift answers `nil` for the empty
/// string where `NSURL` answers an empty URL. [`Url::from_string`] reproduces
/// that. Equality is `NSURL`'s `isEqual:`, which is what Swift's `URL ==`
/// compares (the relative string and the base).
#[derive(Clone)]
pub struct Url(Retained<NSURL>);

impl Url {
    /// `URL(string:)`.
    pub fn from_string(string: &str) -> Option<Url> {
        Url::from_nsstring(&NSString::from_str(string))
    }

    /// `URL(string:)` on a bridged `NSString`.
    pub fn from_nsstring(string: &NSString) -> Option<Url> {
        if string.length() == 0 {
            return None;
        }
        NSURL::URLWithString(string).map(Url)
    }

    pub fn from_nsurl(url: Retained<NSURL>) -> Url {
        Url(url)
    }

    pub fn as_nsurl(&self) -> &NSURL {
        &self.0
    }

    /// `absoluteString`.
    pub fn absolute_string(&self) -> String {
        self.0.absoluteString().map(|string| string.to_string()).unwrap_or_default()
    }

    /// `scheme`, as written (Foundation does not lower-case it).
    pub fn scheme(&self) -> Option<String> {
        self.0.scheme().map(|scheme| scheme.to_string())
    }

    /// `host` (percent-decoded).
    pub fn host(&self) -> Option<String> {
        self.0.host().map(|host| host.to_string())
    }
}

impl PartialEq for Url {
    fn eq(&self, other: &Url) -> bool {
        self.0.isEqual(Some(&other.0))
    }
}

impl fmt::Debug for Url {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Url({:?})", self.absolute_string())
    }
}

/// Swift `String ==` on optionals.
pub(crate) fn optional_str_eq(a: &Option<String>, b: &Option<String>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => upleft_swift_text::str_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

/// Everything the UI needs to know about one available update. Deliberately a
/// plain value type: Sparkle's `SUAppcastItem` is translated once at the
/// driver boundary so the state machine, the pill, the panel, and the test
/// fakes all speak the same vocabulary.
#[derive(Clone, Debug)]
pub struct UpdateMetadata {
    /// `CFBundleVersion` of the target build — Sparkle's ordering key.
    pub version_string: String,
    /// Human-facing `CFBundleShortVersionString`.
    pub display_version_string: String,
    pub title: Option<String>,
    /// Inline release notes (Markdown, embedded in the appcast `<description>`).
    pub item_description: Option<String>,
    /// Linked release notes URL (HTML) when the appcast uses `<releaseNotesLink>`.
    pub release_notes_url: Option<Url>,
    /// Where "Learn More" goes for informational-only updates.
    pub info_url: Option<Url>,
    /// Byte size of the download, when the appcast provides it.
    pub content_length: u64,
    pub is_information_only: bool,
    pub is_major_upgrade: bool,
    pub is_critical: bool,
    pub minimum_system_version: Option<String>,
}

impl PartialEq for UpdateMetadata {
    /// The synthesized `Equatable`: member by member, in declaration order.
    fn eq(&self, other: &UpdateMetadata) -> bool {
        upleft_swift_text::str_eq(&self.version_string, &other.version_string)
            && upleft_swift_text::str_eq(&self.display_version_string, &other.display_version_string)
            && optional_str_eq(&self.title, &other.title)
            && optional_str_eq(&self.item_description, &other.item_description)
            && self.release_notes_url == other.release_notes_url
            && self.info_url == other.info_url
            && self.content_length == other.content_length
            && self.is_information_only == other.is_information_only
            && self.is_major_upgrade == other.is_major_upgrade
            && self.is_critical == other.is_critical
            && optional_str_eq(&self.minimum_system_version, &other.minimum_system_version)
    }
}

/// A failed update operation, reduced to what the panel actually needs:
/// a plain-language summary, an expandable technical detail, and whether
/// retrying is even meaningful.
#[derive(Clone, Debug)]
pub struct UpdateFailure {
    pub message: String,
    pub technical_detail: Option<String>,
    /// Swift `Int` (`NSInteger`).
    pub code: isize,
    pub retryable: bool,
}

impl PartialEq for UpdateFailure {
    fn eq(&self, other: &UpdateFailure) -> bool {
        upleft_swift_text::str_eq(&self.message, &other.message)
            && optional_str_eq(&self.technical_detail, &other.technical_detail)
            && self.code == other.code
            && self.retryable == other.retryable
    }
}

impl UpdateFailure {
    /// `UpdateFailure.generic`.
    pub fn generic() -> UpdateFailure {
        UpdateFailure { message: "Upleft couldn't update.".into(), technical_detail: None, code: -1, retryable: true }
    }

    /// `init(error:)`. Swift bridges the error to `NSError` first (`error as
    /// NSError`); a Swift error the port models is converted with the same
    /// bridging (see [`crate::updater::update_engine::UpdateStartError`]), so
    /// every caller hands over the `NSError` Swift would have seen.
    pub fn from_error(error: &NSError) -> UpdateFailure {
        let description = error.localizedDescription().to_string();
        let message = if description.is_empty() { "Upleft couldn't update.".to_owned() } else { description };
        let technical_detail = error
            .localizedRecoverySuggestion()
            .or_else(|| error.localizedFailureReason())
            .map(|detail| detail.to_string());
        UpdateFailure {
            message,
            technical_detail,
            code: error.code(),
            // The caller decides whether a retry is meaningful: "no update found"
            // and "authorization cancelled" are terminal paths that never reach
            // the failed state in the first place.
            retryable: true,
        }
    }

    /// `init(message:technicalDetail:code:retryable:)`.
    pub fn new(message: impl Into<String>, technical_detail: Option<String>, code: isize, retryable: bool) -> UpdateFailure {
        UpdateFailure { message: message.into(), technical_detail, code, retryable }
    }
}
