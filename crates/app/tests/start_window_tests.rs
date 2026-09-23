//! Port of `Tests/DownrightAppTests/StartWindowTests.swift`, the one test of
//! `DocumentStateStore.canonicalPath`. Every other test there builds the
//! start window (AppKit), ported with the UI.

use upleft_app::ai::document_state_store::DocumentStateStore;
use upleft_core::contracts::Uuid;

#[test]
fn recent_path_canonicalization_collapses_symlinks() {
    let directory = std::env::temp_dir().join(format!("downright-recents-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let target = directory.join("notes.md");
    std::fs::write(&target, "# Notes\n").unwrap();
    let link = directory.join("alias.md");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let via_target = DocumentStateStore::canonical_path(target.to_str().unwrap());
    let via_link = DocumentStateStore::canonical_path(link.to_str().unwrap());
    assert_eq!(via_target, via_link);
    assert_eq!(via_target, DocumentStateStore::canonical_path(&via_target));
    assert_eq!(via_link, DocumentStateStore::canonical_path(&via_link));

    let other = directory.join("other.md");
    std::fs::write(&other, "# Other\n").unwrap();
    assert_ne!(DocumentStateStore::canonical_path(other.to_str().unwrap()), via_target);
    let _ = std::fs::remove_dir_all(&directory);
}
