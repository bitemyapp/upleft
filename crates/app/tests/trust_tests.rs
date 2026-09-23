//! Port of `Tests/DownrightAppTests/TrustTests.swift`: the whole
//! `DocumentTrustTests` suite.
//!
//! Not ported: `TrustPromptViewTests` (`TrustPromptView`, an AppKit view, and
//! `MarkdownLinkDestination`, ported with the UI).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use upleft_app::security::document_trust::{
    DocumentTrust, DocumentTrustState, TrustDecision, TrustEffect, TrustGrant, TrustRequest, TrustScope, TrustTarget,
};
use upleft_app::security::trust_store::{InMemoryTrustStorePersistence, TrustStore, TrustStorePersistence};
use upleft_core::contracts::Uuid;
use upleft_foundation::decodable::{self, DecodableValue};
use upleft_foundation::url::FileUrl;

fn temporary(prefix: &str) -> FileUrl {
    let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::new_v4().hyphenated().to_string().to_uppercase()));
    FileUrl::from_path_is_directory(path.to_str().unwrap(), true)
}

#[test]
fn standard_asks_and_raw_source_denies_every_effect() {
    let request = TrustRequest::new(
        TrustEffect::OpenExternalLink,
        TrustTarget::new("https://example.com", None, Some("https://example.com")),
        Some(&FileUrl::from_path("/tmp/notes.md")),
    );
    assert_eq!(DocumentTrust::default().decision(&request), TrustDecision::Ask);
    assert_eq!(DocumentTrust::new(DocumentTrustState::RawSource, vec![]).decision(&request), TrustDecision::Deny);
}

#[test]
fn folder_and_file_grants_match_canonical_paths_only() {
    let root = temporary("downright-trust");
    let folder = root.appending_path_component_is_directory("workspace", true);
    let child = folder.appending_path_component("assets/image.png");
    let sibling = root.appending_path_component("workspace-other/image.png");
    std::fs::create_dir_all(child.deleting_last_path_component().path()).unwrap();
    std::fs::write(child.path(), "image").unwrap();

    let store = TrustStore::new(Box::new(InMemoryTrustStorePersistence::default()));
    assert!(store.grant(TrustScope::Folder, &folder, [TrustEffect::ReadLocalAsset], None));
    assert_eq!(store.state(Some(&root.appending_path_component("notes.md"))), DocumentTrustState::Standard);
    assert_eq!(store.state(Some(&folder.appending_path_component("notes.md"))), DocumentTrustState::TrustedFolder);
    let allowed = TrustRequest::new(
        TrustEffect::ReadLocalAsset,
        TrustTarget::new(&child.path(), Some(&child.path()), None),
        Some(&root.appending_path_component("notes.md")),
    );
    let blocked = TrustRequest::new(
        TrustEffect::ReadLocalAsset,
        TrustTarget::new(&sibling.path(), Some(&sibling.path()), None),
        Some(&root.appending_path_component("notes.md")),
    );
    assert_eq!(store.policy(DocumentTrustState::Standard).decision(&allowed), TrustDecision::Allow);
    assert_eq!(store.policy(DocumentTrustState::Standard).decision(&blocked), TrustDecision::Ask);

    assert!(store.grant(TrustScope::File, &child, [TrustEffect::LaunchPathOrEditor], None));
    let file_request = TrustRequest::new(
        TrustEffect::LaunchPathOrEditor,
        TrustTarget::new(&child.path(), Some(&child.path()), None),
        None,
    );
    assert_eq!(store.policy(DocumentTrustState::Standard).decision(&file_request), TrustDecision::Allow);
    store.revoke(TrustScope::File, &child);
    assert_eq!(store.policy(DocumentTrustState::Standard).decision(&file_request), TrustDecision::Ask);
    let _ = std::fs::remove_dir_all(root.path());
}

#[test]
fn web_link_permission_does_not_authorize_remote_image_loading() {
    let document = FileUrl::from_path("/workspace/README.md");
    let grant = TrustGrant::new(TrustScope::File, &document.path(), [TrustEffect::OpenExternalLink], None);
    let remote_image = TrustRequest::new(
        TrustEffect::LoadRemoteAsset,
        TrustTarget::new("https://tracker.example/pixel.png", None, Some("https://tracker.example/pixel.png")),
        Some(&document),
    );
    assert_eq!(DocumentTrust::new(DocumentTrustState::Standard, vec![grant]).decision(&remote_image), TrustDecision::Ask);
}

#[test]
fn folder_grant_for_an_external_url_authorizes_only_that_url() {
    let document = FileUrl::from_path("/workspace/README.md");
    let approved = "vscode://file/tmp/note.md";
    let other_scheme = "shortcuts://run-shortcut?name=Deploy";
    let store = TrustStore::new(Box::new(InMemoryTrustStorePersistence::default()));
    assert!(store.grant(
        TrustScope::Folder,
        &FileUrl::from_path("/workspace"),
        [TrustEffect::AutomationAppIntent],
        Some(approved)
    ));
    let same_url = TrustRequest::new(
        TrustEffect::AutomationAppIntent,
        TrustTarget::new(approved, None, Some(approved)),
        Some(&document),
    );
    let different_url = TrustRequest::new(
        TrustEffect::AutomationAppIntent,
        TrustTarget::new(other_scheme, None, Some(other_scheme)),
        Some(&document),
    );
    assert_eq!(store.policy(DocumentTrustState::Standard).decision(&same_url), TrustDecision::Allow);
    assert_eq!(store.policy(DocumentTrustState::Standard).decision(&different_url), TrustDecision::Ask);
}

#[test]
fn legacy_external_grant_fails_closed_for_external_effects_but_keeps_local_ones() {
    let document = DocumentTrust::canonical_file_path(&FileUrl::from_path("/tmp/downright-trust-legacy/notes.md")).unwrap();
    let legacy = TrustGrant::new(
        TrustScope::Folder,
        &document.deleting_last_path_component().path(),
        [TrustEffect::OpenExternalLink],
        None,
    );
    let external = TrustRequest::new(
        TrustEffect::OpenExternalLink,
        TrustTarget::new("https://example.com", None, Some("https://example.com")),
        Some(&document),
    );
    let local = TrustRequest::new(
        TrustEffect::LaunchPathOrEditor,
        TrustTarget::new("/tmp/downright-trust-legacy/notes.md", Some(&document.path()), None),
        Some(&document),
    );
    assert_eq!(DocumentTrust::new(DocumentTrustState::Standard, vec![legacy.clone()]).decision(&external), TrustDecision::Ask);
    let store = TrustStore::new(Box::new(InMemoryTrustStorePersistence::new(vec![legacy])));
    assert!(store.grant(TrustScope::File, &document, [TrustEffect::LaunchPathOrEditor], None));
    assert_eq!(store.policy(DocumentTrustState::Standard).decision(&local), TrustDecision::Allow);
}

#[test]
fn persisted_grants_without_the_url_entry_still_decode() {
    let json = r#"
    [
      {
        "scope" : "file",
        "canonicalPath" : "/tmp/downright-trust-decode/notes.md",
        "effects" : ["readLocalAsset"]
      }
    ]
    "#;
    let grants = decodable::parse(json.as_bytes()).unwrap().array_of(TrustGrant::decode).unwrap();
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].external_url, None);
    assert_eq!(grants[0].effects, [TrustEffect::ReadLocalAsset].into_iter().collect());
}

#[test]
fn symlink_resolves_to_real_path_and_traversal_stays_outside() {
    let root = temporary("downright-trust-symlink");
    let real = root.appending_path_component_is_directory("real", true);
    let link = root.appending_path_component_is_directory("link", true);
    std::fs::create_dir_all(real.path()).unwrap();
    std::os::unix::fs::symlink(real.path(), link.path()).unwrap();
    assert_eq!(DocumentTrust::canonical_file_path(&link).map(|url| url.path()), Some(real.path()));
    let escaped = root.appending_path_component_is_directory("real/../outside", true);
    assert!(!DocumentTrust::is_within(&escaped, &real));
    let _ = std::fs::remove_dir_all(root.path());
}

/// Delays the first save long enough for a second caller to expose reversed
/// persistence.
struct DelayedFirstSaveTrustPersistence {
    first_save_started: Mutex<Option<mpsc::Sender<()>>>,
    save_count: AtomicUsize,
    stored: Mutex<Vec<TrustGrant>>,
}

impl TrustStorePersistence for DelayedFirstSaveTrustPersistence {
    fn load(&self) -> Result<Vec<TrustGrant>, String> {
        Ok(Vec::new())
    }

    fn save(&self, grants: &[TrustGrant]) -> Result<(), String> {
        let call = self.save_count.fetch_add(1, Ordering::SeqCst) + 1;
        if call == 1 {
            if let Some(sender) = self.first_save_started.lock().unwrap().take() {
                let _ = sender.send(());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        *self.stored.lock().unwrap() = grants.to_vec();
        Ok(())
    }
}

struct Shared(Arc<DelayedFirstSaveTrustPersistence>);

impl TrustStorePersistence for Shared {
    fn load(&self) -> Result<Vec<TrustGrant>, String> {
        self.0.load()
    }

    fn save(&self, grants: &[TrustGrant]) -> Result<(), String> {
        self.0.save(grants)
    }
}

#[test]
fn concurrent_mutations_persist_in_memory_order() {
    let (sender, receiver) = mpsc::channel();
    let persistence = Arc::new(DelayedFirstSaveTrustPersistence {
        first_save_started: Mutex::new(Some(sender)),
        save_count: AtomicUsize::new(0),
        stored: Mutex::new(Vec::new()),
    });
    let store = Arc::new(TrustStore::new(Box::new(Shared(Arc::clone(&persistence)))));
    let first = FileUrl::from_path("/tmp/downright-trust-first");
    let second = FileUrl::from_path("/tmp/downright-trust-second");
    let first_store = Arc::clone(&store);
    let one = std::thread::spawn(move || {
        first_store.grant(TrustScope::File, &first, [TrustEffect::ReadLocalAsset], None);
    });
    assert!(receiver.recv_timeout(Duration::from_secs(1)).is_ok());
    let second_store = Arc::clone(&store);
    let two = std::thread::spawn(move || {
        second_store.grant(TrustScope::File, &second, [TrustEffect::LaunchPathOrEditor], None);
    });
    one.join().unwrap();
    two.join().unwrap();
    let persisted: std::collections::HashSet<TrustGrant> = persistence.stored.lock().unwrap().iter().cloned().collect();
    let current: std::collections::HashSet<TrustGrant> = store.grants().into_iter().collect();
    assert_eq!(persisted, current);
}

struct FailingSaveTrustPersistence(Vec<TrustGrant>);

impl TrustStorePersistence for FailingSaveTrustPersistence {
    fn load(&self) -> Result<Vec<TrustGrant>, String> {
        Ok(self.0.clone())
    }

    fn save(&self, _grants: &[TrustGrant]) -> Result<(), String> {
        Err("unavailable".into())
    }
}

struct FailingLoadTrustPersistence;

impl TrustStorePersistence for FailingLoadTrustPersistence {
    fn load(&self) -> Result<Vec<TrustGrant>, String> {
        Err("unavailable".into())
    }

    fn save(&self, _grants: &[TrustGrant]) -> Result<(), String> {
        panic!("save must not follow a failed load");
    }
}

#[test]
fn failed_persistence_never_claims_a_grant_and_revocation_stays_fail_closed() {
    let path = FileUrl::from_path("/tmp/downright-trust-failure");
    let existing = TrustGrant::new(
        TrustScope::File,
        &DocumentTrust::canonical_file_path(&path).unwrap().path(),
        [TrustEffect::ReadLocalAsset],
        None,
    );
    let store = TrustStore::new(Box::new(FailingSaveTrustPersistence(vec![existing.clone()])));
    assert!(!store.grant(TrustScope::File, &path, [TrustEffect::LaunchPathOrEditor], None));
    assert_eq!(store.grants(), vec![existing]);
    assert!(!store.revoke(TrustScope::File, &path));
    assert!(store.grants().is_empty());
}

#[test]
fn corrupt_persistence_cannot_be_overwritten_by_a_new_grant() {
    let store = TrustStore::new(Box::new(FailingLoadTrustPersistence));
    assert!(!store.grant(TrustScope::Folder, &FileUrl::from_path("/tmp/downright-corrupt-trust"), [TrustEffect::ReadLocalAsset], None));
    assert!(store.grants().is_empty());
}

#[test]
fn folder_scope_grants_a_directory_itself_but_a_parent_for_a_file() {
    let directory = FileUrl::from_path("/workspace/assets");
    let file = directory.appending_path_component("logo.png");
    assert_eq!(DocumentTrust::folder_scope(&file, false).path(), directory.path());
    assert_eq!(DocumentTrust::folder_scope(&directory, true).path(), directory.path());
}

#[test]
fn granting_a_second_effect_extends_the_existing_grant() {
    let store = TrustStore::new(Box::new(InMemoryTrustStorePersistence::default()));
    let path = FileUrl::from_path("/tmp/downright-trust-effects");
    assert!(store.grant(TrustScope::File, &path, [TrustEffect::ReadLocalAsset], None));
    assert!(store.grant(TrustScope::File, &path, [TrustEffect::LaunchPathOrEditor], None));
    let grants = store.grants();
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].effects, [TrustEffect::ReadLocalAsset, TrustEffect::LaunchPathOrEditor].into_iter().collect());
}
