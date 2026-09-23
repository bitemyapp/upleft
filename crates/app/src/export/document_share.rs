//! Port of `Sources/DownrightApp/Export/DocumentShare.swift`: which file
//! Share hands to the sharing picker, and where a throwaway copy of it goes.
//!
//! Everything shared is a *file*: a saved, clean document shares itself; a
//! dirty or never-saved one shares a snapshot of the buffer, written with the
//! document's own byte fidelity into a fresh folder under the temporary
//! directory, so the receiver sees the document's own name.

use std::path::Path;

use objc2_foundation::NSTemporaryDirectory;
use upleft_core::document_io::{DocumentIO, DynError};
use upleft_core::{ByteFidelity, Uuid};
use upleft_foundation::url::FileUrl;
use upleft_swift_text::{self as swift, CharSet};

/// `DocumentShareSource`.
#[derive(Clone, Debug)]
pub enum DocumentShareSource {
    /// The document's own file, byte for byte what is on disk. Used only when
    /// the buffer and the file agree.
    DocumentFile(FileUrl),
    /// A copy of the live buffer, written under `file_name` in a staging
    /// directory.
    BufferSnapshot { file_name: String },
}

impl PartialEq for DocumentShareSource {
    /// Swift's synthesized `Equatable`: `URL ==` and `String ==`.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (DocumentShareSource::DocumentFile(a), DocumentShareSource::DocumentFile(b)) => a == b,
            (DocumentShareSource::BufferSnapshot { file_name: a }, DocumentShareSource::BufferSnapshot { file_name: b }) => {
                swift::str_eq(a, b)
            }
            _ => false,
        }
    }
}

impl DocumentShareSource {
    /// A saved, clean document shares itself; anything else shares a
    /// snapshot. Share is not a save, so a dirty document never writes its
    /// own file.
    pub fn choose(url: Option<&FileUrl>, has_unsaved_changes: bool, display_name: &str) -> DocumentShareSource {
        if let Some(url) = url
            && !has_unsaved_changes
        {
            return DocumentShareSource::DocumentFile(url.clone());
        }
        DocumentShareSource::BufferSnapshot { file_name: Self::snapshot_file_name(url, display_name) }
    }

    /// The name the receiver sees: the document's own file name, extension
    /// included, when there is one.
    pub fn snapshot_file_name(url: Option<&FileUrl>, display_name: &str) -> String {
        if let Some(url) = url {
            let last = url.last_path_component();
            if !last.is_empty() {
                return last;
            }
        }
        let base = Self::sanitized_file_base_name(display_name);
        if base.is_empty() { "Untitled.md".into() } else { base + ".md" }
    }

    /// Strips what a file name cannot carry: `/`, `:` and NUL become `-`,
    /// surrounding whitespace goes, then every leading `.` or `-`.
    pub fn sanitized_file_base_name(name: &str) -> String {
        let joined = swift::components_separated_by_set(name, CharSet::Chars("/:\0")).join("-");
        let mut cleaned = swift::trim_whitespaces_and_newlines(&joined);
        while let Some(first) = swift::first(cleaned) {
            if !(swift::char_is(first, '.') || swift::char_is(first, '-')) {
                break;
            }
            cleaned = &cleaned[first.len()..];
        }
        cleaned.to_owned()
    }
}

/// `DocumentShareStaging`: where throwaway share copies live, and how they
/// get written.
pub struct DocumentShareStaging;

impl DocumentShareStaging {
    /// `FileManager.default.temporaryDirectory`.
    pub fn temporary_directory() -> FileUrl {
        FileUrl::from_path_is_directory(&NSTemporaryDirectory().to_string(), true)
    }

    /// A fresh `Downright-Share/<UUID>/` folder under `root`, one per share.
    pub fn make_directory(root: &FileUrl) -> Result<FileUrl, DynError> {
        let uuid = Uuid::new_v4().hyphenated().to_string().to_uppercase();
        let directory = root
            .appending_path_component_is_directory("Upleft-Share", true)
            .appending_path_component_is_directory(&uuid, true);
        std::fs::create_dir_all(directory.path())?;
        Ok(directory)
    }

    /// Materialises `source` into something the share sheet can attach. The
    /// snapshot keeps the document's own fidelity, as Save As does (§3.1).
    pub fn file_url(
        source: &DocumentShareSource,
        text: impl FnOnce() -> String,
        fidelity: ByteFidelity,
        root: &FileUrl,
    ) -> Result<FileUrl, DynError> {
        match source {
            DocumentShareSource::DocumentFile(url) => Ok(url.clone()),
            DocumentShareSource::BufferSnapshot { file_name } => {
                let directory = Self::make_directory(root)?;
                let destination = directory.appending_path_component(file_name);
                DocumentIO::write(&text(), Path::new(&destination.path()), fidelity)?;
                Ok(destination)
            }
        }
    }

    /// The path a rendered PDF is staged at. Always a snapshot.
    pub fn pdf_url(display_name: &str, root: &FileUrl) -> Result<FileUrl, DynError> {
        let base = DocumentShareSource::sanitized_file_base_name(display_name);
        let directory = Self::make_directory(root)?;
        Ok(directory.appending_path_component(&format!("{}.pdf", if base.is_empty() { "Untitled" } else { &base })))
    }
}
