//! Helpers shared by the export/find/workspace test ports.
#![allow(dead_code)]

use std::path::PathBuf;

use upleft_foundation::url::FileUrl;

/// `FileManager.default.temporaryDirectory.appendingPathComponent("<prefix>-<UUID>")`.
pub fn temporary_path(prefix: &str) -> PathBuf {
    std::env::temp_dir().join(format!("{prefix}-{}", upleft_core::Uuid::new_v4().hyphenated().to_string().to_uppercase()))
}

/// Removes a path when dropped (`defer { try? FileManager.default.removeItem(at:) }`).
pub struct Removing(pub PathBuf);

impl Drop for Removing {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A fresh directory and the guard that removes it.
pub fn temporary_directory(prefix: &str) -> (FileUrl, Removing) {
    let path = temporary_path(prefix);
    std::fs::create_dir_all(&path).unwrap();
    (FileUrl::from_path_is_directory(&path.to_string_lossy(), true), Removing(path))
}

/// `(text as NSString).range(of: value)` for literal needles.
pub fn ns_range_of(text: &str, value: &str) -> upleft_core::NSRange {
    let haystack: Vec<u16> = text.encode_utf16().collect();
    let needle: Vec<u16> = value.encode_utf16().collect();
    let location = haystack.windows(needle.len()).position(|window| window == needle.as_slice()).expect("present");
    upleft_core::NSRange::new(location as isize, needle.len() as isize)
}
