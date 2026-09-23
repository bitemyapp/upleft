//! Fixtures for the `MarkdownDocument` and `SiblingScanner` tests.
//!
//! Downright's tests build `MarkdownDocument()` on the process-wide
//! `SnapshotStore.shared` / `DocumentStateStore.shared`, which live in the
//! user's real Application Support folder, and read `Preferences.shared`.
//! These binaries never touch the real home: [`sandbox`] points Downright's
//! own `DOWNRIGHT_SUPPORT_DIRECTORY` override at a fresh temporary folder
//! before anything reads it, so the shared stores keep their process-wide
//! semantics inside the sandbox, and documents get a `Preferences::for_testing`
//! instance instead of `Preferences.shared` (whose load publishes the Quick
//! Look appearance to the real user defaults).

#![allow(dead_code)]

use std::sync::OnceLock;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use upleft_app::ai::document_state_store::DocumentStateStore;
use upleft_app::ai::markdown_document::MarkdownDocument;
use upleft_app::ai::markdown_parse_worker::MarkdownParseWorker;
use upleft_app::ai::snapshot_store::SnapshotStore;
use upleft_app::support::preferences::Preferences;
use upleft_foundation::url::FileUrl;

static SANDBOX: OnceLock<FileUrl> = OnceLock::new();
static PREFERENCES: OnceLock<Preferences> = OnceLock::new();

pub fn unique() -> String {
    objc2_foundation::NSUUID::UUID().UUIDString().to_string()
}

/// `FileManager.default.temporaryDirectory`.
pub fn temporary_directory() -> FileUrl {
    FileUrl::from_path_is_directory(&objc2_foundation::NSTemporaryDirectory().to_string(), true)
}

/// Points `AppPaths.supportDirectory` at a fresh temporary folder. Call first
/// thing in `main`, before any thread starts.
pub fn sandbox() -> FileUrl {
    SANDBOX
        .get_or_init(|| {
            let root = temporary_directory()
                .appending_path_component_is_directory(&format!("upleft-app-tests-{}", unique()), true);
            let support = root.appending_path_component_is_directory("support", true);
            std::fs::create_dir_all(support.path()).unwrap();
            // SAFETY: called from `main` before any other thread exists.
            unsafe { std::env::set_var("DOWNRIGHT_SUPPORT_DIRECTORY", support.path()) };
            root
        })
        .clone()
}

/// Removes the sandbox. Call after the tests ran.
pub fn remove_sandbox() {
    if let Some(root) = SANDBOX.get() {
        let _ = std::fs::remove_dir_all(root.path());
    }
}

/// The documents' `Preferences.shared`: defaults, persisted inside the sandbox.
pub fn preferences() -> &'static Preferences {
    PREFERENCES.get_or_init(|| {
        let file = sandbox().appending_path_component("preferences.json");
        Preferences::for_testing(file, None)
    })
}

/// `MarkdownDocument()`.
pub fn document() -> Retained<MarkdownDocument> {
    document_with_worker(MarkdownParseWorker::default())
}

/// `MarkdownDocument(parseWorker:)`.
pub fn document_with_worker(worker: MarkdownParseWorker) -> Retained<MarkdownDocument> {
    let _ = sandbox();
    MarkdownDocument::with_dependencies(
        MainThreadMarker::new().expect("document tests run on the main thread"),
        worker,
        SnapshotStore::shared().clone(),
        DocumentStateStore::shared(),
        Some(preferences()),
    )
}

/// `makeIsolatedDocument(in:)`: stores under `root/support`.
pub fn isolated_document(root: &FileUrl) -> Retained<MarkdownDocument> {
    let support = root.appending_path_component_is_directory("support", true);
    let store: &'static DocumentStateStore = Box::leak(Box::new(DocumentStateStore::new(support.clone())));
    MarkdownDocument::with_dependencies(
        MainThreadMarker::new().expect("document tests run on the main thread"),
        MarkdownParseWorker::default(),
        SnapshotStore::new(support.appending_path_component_is_directory("history", true)),
        store,
        Some(preferences()),
    )
}

/// A folder holding `note.md`, removed on drop.
pub struct Fixture {
    pub root: FileUrl,
    pub url: FileUrl,
}

impl Fixture {
    pub fn new(prefix: &str, name: &str, text: &str) -> Fixture {
        let root = temporary_directory().appending_path_component_is_directory(&format!("{prefix}-{}", unique()), true);
        std::fs::create_dir_all(root.path()).unwrap();
        let url = root.appending_path_component(name);
        std::fs::write(url.path(), text).unwrap();
        Fixture { root, url }
    }

    pub fn remove(&self) {
        let _ = std::fs::remove_dir_all(self.root.path());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.remove();
    }
}

/// `Data(contentsOf:)`.
pub fn read(url: &FileUrl) -> Vec<u8> {
    std::fs::read(url.path()).unwrap()
}

/// `String(contentsOf:encoding: .utf8)`.
pub fn read_text(url: &FileUrl) -> String {
    String::from_utf8(read(url)).unwrap()
}

/// `data.write(to:options: .atomic)`.
pub fn write_atomically(url: &FileUrl, data: &[u8]) {
    upleft_core::document_io::write_data_atomically(data, std::path::Path::new(&url.path())).unwrap();
}

/// `data.write(to:)`.
pub fn write(url: &FileUrl, data: &[u8]) {
    std::fs::write(url.path(), data).unwrap();
}

pub fn exists(url: &FileUrl) -> bool {
    std::path::Path::new(&url.path()).exists()
}

/// `NSRange(location: 0, length: document.storage.length)`.
pub fn whole(document: &MarkdownDocument) -> upleft_swift_text::NSRange {
    upleft_swift_text::NSRange::new(0, document.storage().length() as isize)
}
