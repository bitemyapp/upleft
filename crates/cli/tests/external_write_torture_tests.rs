//! Port of `Tests/MarkdownCLITests/ExternalWriteTortureTests.swift`.
//!
//! Exercises the write shapes used by editors and coding agents without
//! depending on a running app or a particular file-watcher implementation.
//! The app-level watcher tests consume the same guarantees: complete bytes,
//! atomic replacement, and the newest write winning after a burst.

use std::path::PathBuf;

use upleft_foundation::foundation_io;
use upleft_foundation::url::FileUrl;

fn make_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!("down-external-write-{}", foundation_io::uuid_string()));
    std::fs::create_dir_all(&root).unwrap();
    root
}

struct Removing(PathBuf);

impl Drop for Removing {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn atomic_rename_never_exposes_a_staging_file_as_the_document() {
    let root = make_root();
    let _cleanup = Removing(root.clone());
    let document = root.join("plan.md");
    let staging = root.join(".plan.md.tmp");
    let expected = "# Agent plan\n\n- [ ] Review the changed section\n";

    std::fs::write(&staging, expected).unwrap();
    std::fs::rename(&staging, &document).unwrap();

    assert!(document.exists());
    assert!(!staging.exists());
    assert_eq!(std::fs::read_to_string(&document).unwrap(), expected);
}

#[test]
fn rapid_atomic_rewrites_leave_the_last_complete_document() {
    let root = make_root();
    let _cleanup = Removing(root.clone());
    let document = root.join("notes.md");

    for revision in 0..40 {
        let staging = root.join(format!(".notes-{revision}.tmp"));
        let content = format!("# Revision {revision}\n\nagent-write-{revision} ✅\n");
        std::fs::write(&staging, content).unwrap();
        if document.exists() {
            std::fs::remove_file(&document).unwrap();
        }
        std::fs::rename(&staging, &document).unwrap();
    }

    assert_eq!(std::fs::read_to_string(&document).unwrap(), "# Revision 39\n\nagent-write-39 ✅\n");
    let leftovers: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn rewrite_preserves_utf8_and_intentional_line_endings() {
    let root = make_root();
    let _cleanup = Removing(root.clone());
    let document = root.join("mixed.md");
    let expected = "# Café\r\n\r\n- [x] shipped\n- [ ] next — 日本語\r\n";

    foundation_io::write_atomically(expected.as_bytes(), &FileUrl::from_path(document.to_str().unwrap())).unwrap();
    assert_eq!(std::fs::read(&document).unwrap(), expected.as_bytes());
}
