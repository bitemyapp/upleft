//! Port of `Sources/DownrightApp/Security/DocumentTrust.swift`.
//!
//! Pure policy evaluation: no file reads beyond what `URL` itself does, and it
//! never invokes an effect.

use std::collections::BTreeSet;

use upleft_foundation::decodable::{DecodableValue, DecodingError, Value};
use upleft_foundation::json_encoder::JsonValue;
use upleft_foundation::url::FileUrl;

/// `DocumentTrustState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DocumentTrustState {
    Standard,
    TrustedFolder,
    RawSource,
}

impl DocumentTrustState {
    pub const ALL_CASES: [DocumentTrustState; 3] =
        [DocumentTrustState::Standard, DocumentTrustState::TrustedFolder, DocumentTrustState::RawSource];

    pub fn raw_value(self) -> &'static str {
        match self {
            DocumentTrustState::Standard => "standard",
            DocumentTrustState::TrustedFolder => "trustedFolder",
            DocumentTrustState::RawSource => "rawSource",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<DocumentTrustState> {
        DocumentTrustState::ALL_CASES.into_iter().find(|state| state.raw_value() == raw)
    }

    pub fn title(self) -> &'static str {
        match self {
            DocumentTrustState::Standard => "Standard",
            DocumentTrustState::TrustedFolder => "Trusted Folder",
            DocumentTrustState::RawSource => "Raw Source",
        }
    }
}

/// `TrustEffect`. `Ord` follows declaration order, which is the order the
/// port writes a `Set<TrustEffect>` in (Swift's order changes from run to
/// run).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TrustEffect {
    OpenExternalLink,
    LoadRemoteAsset,
    ReadLocalAsset,
    LaunchPathOrEditor,
    AutomationAppIntent,
}

impl TrustEffect {
    pub const ALL_CASES: [TrustEffect; 5] = [
        TrustEffect::OpenExternalLink,
        TrustEffect::LoadRemoteAsset,
        TrustEffect::ReadLocalAsset,
        TrustEffect::LaunchPathOrEditor,
        TrustEffect::AutomationAppIntent,
    ];

    pub fn raw_value(self) -> &'static str {
        match self {
            TrustEffect::OpenExternalLink => "openExternalLink",
            TrustEffect::LoadRemoteAsset => "loadRemoteAsset",
            TrustEffect::ReadLocalAsset => "readLocalAsset",
            TrustEffect::LaunchPathOrEditor => "launchPathOrEditor",
            TrustEffect::AutomationAppIntent => "automationAppIntent",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<TrustEffect> {
        TrustEffect::ALL_CASES.into_iter().find(|effect| effect.raw_value() == raw)
    }

    pub fn title(self) -> &'static str {
        match self {
            TrustEffect::OpenExternalLink => "Open an external link",
            TrustEffect::LoadRemoteAsset => "Load a remote image",
            TrustEffect::ReadLocalAsset => "Read a local asset",
            TrustEffect::LaunchPathOrEditor => "Open a path or editor",
            TrustEffect::AutomationAppIntent => "Run an app or automation intent",
        }
    }
}

/// `TrustScope`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TrustScope {
    File,
    Folder,
}

impl TrustScope {
    pub const ALL_CASES: [TrustScope; 2] = [TrustScope::File, TrustScope::Folder];

    pub fn raw_value(self) -> &'static str {
        match self {
            TrustScope::File => "file",
            TrustScope::Folder => "folder",
        }
    }

    pub fn from_raw_value(raw: &str) -> Option<TrustScope> {
        TrustScope::ALL_CASES.into_iter().find(|scope| scope.raw_value() == raw)
    }
}

/// `TrustDecision`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrustDecision {
    Allow,
    Deny,
    Ask,
}

impl TrustDecision {
    pub fn raw_value(self) -> &'static str {
        match self {
            TrustDecision::Allow => "allow",
            TrustDecision::Deny => "deny",
            TrustDecision::Ask => "ask",
        }
    }
}

/// `TrustTarget`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustTarget {
    pub display_name: String,
    pub canonical_path: Option<String>,
    pub external_url: Option<String>,
}

impl TrustTarget {
    pub fn new(display_name: &str, canonical_path: Option<&str>, external_url: Option<&str>) -> TrustTarget {
        TrustTarget {
            display_name: display_name.to_owned(),
            canonical_path: canonical_path.map(str::to_owned),
            external_url: external_url.map(str::to_owned),
        }
    }
}

/// `TrustRequest`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustRequest {
    pub effect: TrustEffect,
    pub target: TrustTarget,
    pub document_path: Option<String>,
}

impl TrustRequest {
    /// `TrustRequest(effect:target:documentURL:)`.
    pub fn new(effect: TrustEffect, target: TrustTarget, document_url: Option<&FileUrl>) -> TrustRequest {
        TrustRequest {
            effect,
            target,
            document_path: document_url.and_then(DocumentTrust::canonical_file_path).map(|url| url.path()),
        }
    }
}

/// `TrustGrant`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TrustGrant {
    pub scope: TrustScope,
    pub canonical_path: String,
    pub effects: BTreeSet<TrustEffect>,
    /// For external effects the grant is pinned to the exact URL the user
    /// approved; `None` for purely local effects and for legacy grants.
    pub external_url: Option<String>,
}

impl TrustGrant {
    pub fn new(
        scope: TrustScope,
        canonical_path: &str,
        effects: impl IntoIterator<Item = TrustEffect>,
        external_url: Option<&str>,
    ) -> TrustGrant {
        TrustGrant {
            scope,
            canonical_path: canonical_path.to_owned(),
            effects: effects.into_iter().collect(),
            external_url: external_url.map(str::to_owned),
        }
    }

    /// Synthesized `encode(to:)`; `externalURL` only when present.
    pub fn encode(&self) -> JsonValue {
        let mut members = vec![
            ("scope".to_owned(), JsonValue::from(self.scope.raw_value())),
            ("canonicalPath".to_owned(), JsonValue::from(self.canonical_path.as_str())),
            (
                "effects".to_owned(),
                JsonValue::Array(self.effects.iter().map(|effect| JsonValue::from(effect.raw_value())).collect()),
            ),
        ];
        JsonValue::push_if_present(&mut members, "externalURL", self.external_url.as_deref().map(JsonValue::from));
        JsonValue::Object(members)
    }

    /// Synthesized `init(from:)`.
    pub fn decode(value: &Value) -> Result<TrustGrant, DecodingError> {
        let c = value.keyed_container()?;
        Ok(TrustGrant {
            scope: c.decode("scope", |value| value.raw_string_enum(TrustScope::from_raw_value))?,
            canonical_path: c.decode("canonicalPath", Value::string_value)?,
            effects: c
                .decode("effects", |value| value.array_of(|element| element.raw_string_enum(TrustEffect::from_raw_value)))?
                .into_iter()
                .collect(),
            external_url: c.decode_if_present("externalURL", Value::string_value)?,
        })
    }
}

/// `DocumentTrust`: pure policy evaluation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentTrust {
    pub state: DocumentTrustState,
    pub grants: Vec<TrustGrant>,
}

impl Default for DocumentTrust {
    fn default() -> Self {
        DocumentTrust { state: DocumentTrustState::Standard, grants: Vec::new() }
    }
}

impl DocumentTrust {
    pub fn new(state: DocumentTrustState, grants: Vec<TrustGrant>) -> DocumentTrust {
        DocumentTrust { state, grants }
    }

    pub fn decision(&self, request: &TrustRequest) -> TrustDecision {
        if self.state == DocumentTrustState::RawSource {
            return TrustDecision::Deny;
        }
        let Some(scope_path) = request.target.canonical_path.as_ref().or(request.document_path.as_ref()) else {
            return TrustDecision::Ask;
        };
        let target = FileUrl::from_path(scope_path);
        let matching = self.grants.iter().any(|grant| {
            if !grant.effects.contains(&request.effect) {
                return false;
            }
            // An external grant answers only for the exact URL that was
            // approved.
            if request.target.external_url != grant.external_url {
                return false;
            }
            let root = FileUrl::from_path(&grant.canonical_path);
            match grant.scope {
                TrustScope::File => target.path() == root.path(),
                TrustScope::Folder => DocumentTrust::is_within(&target, &root),
            }
        });
        if matching { TrustDecision::Allow } else { TrustDecision::Ask }
    }

    /// `canonicalFilePath(_:)`: standardized, symlinks resolved,
    /// standardized again. Every `FileUrl` is a file URL.
    pub fn canonical_file_path(url: &FileUrl) -> Option<FileUrl> {
        Some(url.standardized_file_url().resolving_symlinks_in_path().standardized_file_url())
    }

    pub fn is_within(child: &FileUrl, root: &FileUrl) -> bool {
        let child_parts = child.standardized_file_url().path_components();
        let root_parts = root.standardized_file_url().path_components();
        if child_parts.len() < root_parts.len() {
            return false;
        }
        child_parts[..root_parts.len()] == root_parts[..]
    }

    /// The folder a "Allow for Folder" grant covers for a target: a file's
    /// containing directory, or a directory itself.
    pub fn folder_scope(target: &FileUrl, is_directory: bool) -> FileUrl {
        if is_directory { target.clone() } else { target.deleting_last_path_component() }
    }
}
