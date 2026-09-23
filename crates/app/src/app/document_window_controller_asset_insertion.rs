//! Port of `App/DocumentWindowController+AssetInsertion.swift`: everything
//! that puts a new asset into the document. A drag dropped on the text
//! surface, and (through the one shared insertion below) a Continuity Camera
//! capture.
//!
//! The two arrive by completely different routes and have exactly one thing
//! in common: a file lands next to the document and one Markdown reference
//! to it lands in the source, as a single explicit mutation with its own undo
//! boundary. That last part is why they share
//! [`DocumentWindowController::insert_asset_markdown`] rather than each
//! having their own copy.
//!
//! The two drop methods are `MarkdownTextViewDelegate` methods in Swift. The
//! trait is implemented on the delegate proxy by `+Delegates`, whose
//! `can_accept_drop`/`did_accept_drop` forward to
//! [`DocumentWindowController::markdown_text_view_can_accept_drop`] and
//! [`DocumentWindowController::markdown_text_view_did_accept_drop`].
//!
//! Main-thread I/O, as in Swift: a drop writes or copies its files on the
//! main thread, because AppKit needs the drop's answer synchronously.

use objc2::rc::autoreleasepool;
use objc2_foundation::{NSData, NSDataWritingOptions, NSError, NSFileManager};
use upleft_core::NSRange;
use upleft_core::contracts::TextEdit;
use upleft_foundation::foundation_io;
use upleft_foundation::url::FileUrl;
use upleft_render::view::markdown_text_view::MarkdownTextView;
use upleft_render::view::markdown_text_view_delegate::DocumentDrop;

use crate::app::document_window_controller::DocumentWindowController;
use crate::assets::captured_image::InsertionEdit;
use crate::assets::dropped_asset::{DroppedAsset, Insertion, Payload, WriteContents};

/// An error that already carries its `localizedDescription`: what Swift's
/// `presentOperationError(_:error:)` shows for a Foundation `NSError`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LocalizedError(pub(crate) String);

impl LocalizedError {
    /// `(error as NSError).localizedDescription`.
    pub(crate) fn from_ns(error: &NSError) -> LocalizedError {
        LocalizedError(error.localizedDescription().to_string())
    }
}

impl std::fmt::Display for LocalizedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LocalizedError {}

impl DocumentWindowController {
    // MARK: - Drops (§7.1)

    /// `markdownTextView(_:canAcceptDrop:)`: asked once per drag, before
    /// anything is drawn.
    ///
    /// This is where the never-saved window is answered. A drag carrying real
    /// *files* is always accepted: even with no folder to be relative to, a
    /// `file:` destination points at something that exists. A drag carrying
    /// only image *bytes* is refused, because there is nowhere to put them.
    pub fn markdown_text_view_can_accept_drop(&self, _view: &MarkdownTextView, drop: &DocumentDrop) -> bool {
        match DroppedAsset::payload(&drop.pasteboard) {
            Some(Payload::Files(_)) => true,
            Some(Payload::ImageData(_)) => self.markdown_document().url().is_some(),
            None => false,
        }
    }

    /// `markdownTextView(_:didAcceptDrop:)`.
    pub fn markdown_text_view_did_accept_drop(&self, _view: &MarkdownTextView, drop: &DocumentDrop) -> bool {
        let Some(payload) = DroppedAsset::payload(&drop.pasteboard) else { return false };
        let directory = self.markdown_document().url().map(|url| url.deleting_last_path_component());
        let insertions = DroppedAsset::plan(
            &payload,
            directory.as_ref(),
            &self
                .markdown_document()
                .url()
                .map(|url| url.deleting_path_extension().last_path_component())
                .unwrap_or_else(|| self.markdown_document().display_name()),
            |name| {
                let Some(directory) = &directory else { return false };
                foundation_io::file_exists(&directory.appending_path_component(name).path())
            },
        );
        if insertions.is_empty() {
            return false;
        }
        self.commit_drop(&insertions, directory.as_ref(), drop.source_offset)
    }

    /// `commitDrop(_:into:at:)`: writes first, then inserts, and inserts
    /// nothing at all if a write fails.
    ///
    /// The files this operation created are removed again on failure (a
    /// rollback of this operation's own writes, not a deletion of anything
    /// the reader owns), and the error is reported rather than being quietly
    /// turned into "nothing happened" (§3.1).
    fn commit_drop(&self, insertions: &[Insertion], directory: Option<&FileUrl>, offset: isize) -> bool {
        let writes: Vec<_> = insertions.iter().filter_map(|insertion| insertion.write.as_ref()).collect();
        // Only a plan that writes needs a folder, and `DroppedAsset` never
        // produces one that does without checking. Refusing here as well is
        // the same invariant enforced at the point of the write.
        if !(writes.is_empty() || directory.is_some()) {
            return false;
        }

        let mut created: Vec<FileUrl> = Vec::new();
        for write in writes {
            let Some(directory) = directory else { break };
            let target = directory.appending_path_component(&write.file_name);
            let outcome = autoreleasepool(|_| match &write.contents {
                // `.withoutOverwriting` closes the gap between picking a free
                // name and using it: a drop must never be able to replace a
                // file that appeared in between.
                WriteContents::Data(data) => NSData::with_bytes(data)
                    .writeToURL_options_error(&target.to_nsurl(), NSDataWritingOptions::WithoutOverwriting)
                    .map_err(|error| LocalizedError::from_ns(&error)),
                WriteContents::CopyOf(origin) => NSFileManager::defaultManager()
                    .copyItemAtURL_toURL_error(&origin.to_nsurl(), &target.to_nsurl())
                    .map_err(|error| LocalizedError::from_ns(&error)),
            });
            match outcome {
                Ok(()) => created.push(target),
                Err(error) => {
                    remove_items(&created);
                    self.present_operation_error("Couldn\u{2019}t add the dropped file", &error);
                    return false;
                }
            }
        }

        let Some(edit) = DroppedAsset::edit(insertions, &self.markdown_document().text(), offset) else {
            remove_items(&created);
            return false;
        };
        // The name is what the reader reads in Edit ▸ Undo, so it names what
        // they did rather than what the code did.
        let action_name = if insertions.len() > 1 {
            "Insert Files"
        } else if insertions[0].is_block {
            "Insert Image"
        } else {
            "Insert Link"
        };
        self.insert_asset_markdown(edit, action_name);
        true
    }

    // MARK: - The one insertion

    /// `insertAssetMarkdown(_:actionName:)`: one insertion, with an undo
    /// boundary, and nothing else.
    ///
    /// Deliberately inserts at a zero-length range rather than replacing the
    /// selection: a drop lands where the pointer is, not where the caret is,
    /// and a capture arrives seconds after the reader last looked at the
    /// document. Undo takes the reference back out and leaves the written
    /// file where it is.
    pub fn insert_asset_markdown(&self, insertion: InsertionEdit, action_name: &str) {
        self.apply_in_place_document_edits(
            &[TextEdit::new(NSRange::new(insertion.origin, 0), insertion.replacement, action_name, None)],
            action_name,
            None,
        );
        self.container_text_view().set_source_selected_ranges(&[NSRange::new(insertion.caret, 0)]);
        // The file appeared after the last render pass, so the image fragment
        // is holding a "missing asset" result for a path that now exists.
        for pane in self.document_panes() {
            pane.text_view().refresh_local_assets();
        }
    }
}

/// `created.forEach { try? FileManager.default.removeItem(at: $0) }`.
fn remove_items(created: &[FileUrl]) {
    for url in created {
        autoreleasepool(|_| {
            let _ = NSFileManager::defaultManager().removeItemAtURL_error(&url.to_nsurl());
        });
    }
}
