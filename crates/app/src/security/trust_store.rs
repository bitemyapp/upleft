//! Port of `Sources/DownrightApp/Security/TrustStore.swift`.
//!
//! Persisted grants are canonical path values only. The store is injected in
//! tests and can be replaced by an app host without changing policy logic.
//!
//! `trust.json` is written by a `JSONEncoder` with
//! `[.prettyPrinted, .sortedKeys]`; the only bytes that differ from Swift's are
//! the elements of each `effects` array, a `Set` that Swift writes in its
//! per-process hash order and the port writes in declaration order.

use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

use upleft_foundation::decodable::{self, DecodableValue};
use upleft_foundation::file_manager;
use upleft_foundation::json_encoder::{self, JsonValue, OutputFormatting};
use upleft_foundation::url::FileUrl;

use crate::security::document_trust::{DocumentTrust, DocumentTrustState, TrustEffect, TrustGrant, TrustScope};
use crate::support::app_paths;

/// `TrustStorePersistence`. Errors are Swift's thrown errors, described.
pub trait TrustStorePersistence: Send + Sync {
    fn load(&self) -> Result<Vec<TrustGrant>, String>;
    fn save(&self, grants: &[TrustGrant]) -> Result<(), String>;
}

/// `InMemoryTrustStorePersistence`.
#[derive(Default)]
pub struct InMemoryTrustStorePersistence {
    values: Mutex<Vec<TrustGrant>>,
}

impl InMemoryTrustStorePersistence {
    pub fn new(values: Vec<TrustGrant>) -> InMemoryTrustStorePersistence {
        InMemoryTrustStorePersistence { values: Mutex::new(values) }
    }

    pub fn values(&self) -> Vec<TrustGrant> {
        self.values.lock().unwrap().clone()
    }
}

impl TrustStorePersistence for InMemoryTrustStorePersistence {
    fn load(&self) -> Result<Vec<TrustGrant>, String> {
        Ok(self.values())
    }

    fn save(&self, grants: &[TrustGrant]) -> Result<(), String> {
        *self.values.lock().unwrap() = grants.to_vec();
        Ok(())
    }
}

/// `JSONTrustStorePersistence` (private in Swift; the store behind
/// `TrustStore.shared`).
pub struct JSONTrustStorePersistence {
    url: FileUrl,
}

impl JSONTrustStorePersistence {
    pub fn new(url: FileUrl) -> JSONTrustStorePersistence {
        JSONTrustStorePersistence { url }
    }

    /// What `save` writes.
    pub fn encoded(grants: &[TrustGrant]) -> Vec<u8> {
        json_encoder::encode(&JsonValue::Array(grants.iter().map(TrustGrant::encode).collect()), OutputFormatting::PRETTY_SORTED)
    }
}

impl TrustStorePersistence for JSONTrustStorePersistence {
    fn load(&self) -> Result<Vec<TrustGrant>, String> {
        if !file_manager::file_exists(&self.url.path()) {
            return Ok(Vec::new());
        }
        let data = file_manager::data_contents_of(&self.url).ok_or_else(|| "The file couldn\u{2019}t be opened.".to_owned())?;
        decodable::parse(&data)
            .and_then(|value| value.array_of(TrustGrant::decode))
            .map_err(|error| error.localized_description().to_owned())
    }

    fn save(&self, grants: &[TrustGrant]) -> Result<(), String> {
        let data = Self::encoded(grants);
        file_manager::create_directory(&self.url.deleting_last_path_component(), true)?;
        file_manager::write_atomic(&data, &self.url)
    }
}

struct State {
    values: Vec<TrustGrant>,
    load_failed: bool,
}

/// `TrustStore`.
pub struct TrustStore {
    persistence: Box<dyn TrustStorePersistence>,
    state: Mutex<State>,
}

static SHARED: OnceLock<TrustStore> = OnceLock::new();

impl TrustStore {
    /// `TrustStore.shared`: `trust.json` in the support directory.
    pub fn shared() -> &'static TrustStore {
        SHARED.get_or_init(|| {
            TrustStore::new(Box::new(JSONTrustStorePersistence::new(
                app_paths::support_directory().appending_path_component("trust.json"),
            )))
        })
    }

    pub fn new(persistence: Box<dyn TrustStorePersistence>) -> TrustStore {
        let (values, load_failed) = match persistence.load() {
            Ok(values) => (values, false),
            Err(_) => (Vec::new(), true),
        };
        TrustStore { persistence, state: Mutex::new(State { values, load_failed }) }
    }

    pub fn grants(&self) -> Vec<TrustGrant> {
        self.state.lock().unwrap().values.clone()
    }

    pub fn policy(&self, state: DocumentTrustState) -> DocumentTrust {
        DocumentTrust::new(state, self.grants())
    }

    pub fn state(&self, document_url: Option<&FileUrl>) -> DocumentTrustState {
        let Some(document) = document_url.and_then(DocumentTrust::canonical_file_path) else {
            return DocumentTrustState::Standard;
        };
        let trusted = self.grants().iter().any(|grant| {
            grant.scope == TrustScope::Folder && DocumentTrust::is_within(&document, &FileUrl::from_path(&grant.canonical_path))
        });
        if trusted { DocumentTrustState::TrustedFolder } else { DocumentTrustState::Standard }
    }

    pub fn grant(
        &self,
        scope: TrustScope,
        path: &FileUrl,
        effects: impl IntoIterator<Item = TrustEffect>,
        external_url: Option<&str>,
    ) -> bool {
        let Some(canonical) = DocumentTrust::canonical_file_path(path) else {
            return false;
        };
        let canonical = canonical.path();
        let external_url = external_url.map(str::to_owned);
        let mut state = self.state.lock().unwrap();
        if state.load_failed {
            return false;
        }
        let previous = state.values.clone();
        // External grants are keyed by their approved URL as well.
        let is_same = |grant: &TrustGrant| {
            grant.scope == scope && grant.canonical_path == canonical && grant.external_url == external_url
        };
        let existing = state.values.iter().find(|grant| is_same(grant)).cloned();
        state.values.retain(|grant| !is_same(grant));
        // A new grant extends the effects already allowed for this path.
        let mut union: BTreeSet<TrustEffect> = effects.into_iter().collect();
        if let Some(existing) = existing {
            union.extend(existing.effects);
        }
        state.values.push(TrustGrant { scope, canonical_path: canonical.clone(), effects: union, external_url: external_url.clone() });
        // Keep mutation and persistence ordered: the save runs under the lock.
        match self.persistence.save(&state.values) {
            Ok(()) => true,
            Err(_) => {
                state.values = previous;
                false
            }
        }
    }

    pub fn revoke(&self, scope: TrustScope, path: &FileUrl) -> bool {
        let Some(canonical) = DocumentTrust::canonical_file_path(path) else {
            return false;
        };
        let canonical = canonical.path();
        let mut state = self.state.lock().unwrap();
        state.values.retain(|grant| !(grant.scope == scope && grant.canonical_path == canonical));
        if state.load_failed {
            return false;
        }
        // Revocation is fail-closed for this process even when persistence
        // fails.
        self.persistence.save(&state.values).is_ok()
    }
}
