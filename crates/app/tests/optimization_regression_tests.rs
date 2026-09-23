//! Port of `Tests/DownrightAppTests/OptimizationRegressionTests.swift`, the
//! tests of this layer's stores.
//!
//! `snapshotStoreObjectInventoryPreservesShardDirectories` enumerates an
//! isolated store with objects in it: Swift's version reads the user's real
//! `SnapshotStore.shared`, which a port must never touch.
//!
//! Not ported here: `documentWordCountIgnoresCode` (Metrics, `upleft-core`),
//! `fuzzyMatcherFindsSubsequence` (palette), `findSessionClears` (find),
//! `workspaceSearchLoadsTextOnDemand` and
//! `workspaceSearchRejectsFilesThatOutgrowTheirIndexedBound` (workspace),
//! `markdownParseCoordinatorAcceptsZeroRevisionOnSubmit` (document layer).

use upleft_app::ai::document_state_store::ScrollAnchoring;
use upleft_app::ai::snapshot_store::{SnapshotKind, SnapshotStore};
use upleft_core::contracts::Uuid;
use upleft_core::document_io::DocumentIO;
use upleft_core::parser::MarkdownParser;
use upleft_foundation::url::FileUrl;

#[test]
fn content_hash_apis_agree() {
    let text = "# Hello\n\nWorld\n";
    assert_eq!(DocumentIO::content_hash(text), SnapshotStore::hash(text));
    assert_eq!(DocumentIO::content_hash_data(text.as_bytes()), SnapshotStore::hash_data(text.as_bytes()));
}

#[test]
fn scroll_anchoring_handles_preamble_offsets() {
    let text = "Introductory preamble paragraph before any heading.\n\n# Heading 1\nSection 1 text\n";
    let document = MarkdownParser::parse(text);
    let anchor = ScrollAnchoring::anchor(10, &document);
    assert_eq!(anchor.heading_slug, "");
    assert_eq!(anchor.heading_index, 0);
    let restored = ScrollAnchoring::offset(&anchor, &document);
    assert!((8..=12).contains(&restored));
}

#[test]
fn snapshot_store_object_inventory_preserves_shard_directories() {
    let root = std::env::temp_dir().join(format!("downright-inventory-{}", Uuid::new_v4()));
    let root = FileUrl::from_path_is_directory(root.to_str().unwrap(), true);
    let store = SnapshotStore::new(root.appending_path_component_is_directory("history", true));
    let document = root.appending_path_component("note.md");
    store.record("one\n", &document, SnapshotKind::Baseline);
    store.record("two\n", &document, SnapshotKind::External);
    store.wait_for_pending_writes();
    let inventory = store.object_inventory();
    assert_eq!(inventory.len(), 2);
    for item in inventory {
        assert_eq!(item.hash.chars().count(), 64);
        assert_eq!(item.url.last_path_component().chars().count(), 64);
        assert_eq!(item.url.deleting_last_path_component().last_path_component(), item.hash[..2]);
    }
    let _ = std::fs::remove_dir_all(root.path());
}
