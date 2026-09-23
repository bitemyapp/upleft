//! Port of `App/DocumentWindowController+Share.swift`: Share (§9.5's other
//! half).
//!
//! Export writes a file the user then has to go and find; Share hands the
//! same document straight to Mail, Messages, AirDrop, or Notes. Both end at a
//! real file on disk (see `DocumentShareSource` for why a text blob is the
//! wrong thing to give a share sheet), so the only work here is deciding
//! which file, producing it, and putting the picker somewhere the reader is
//! looking.
//!
//! The `NSSharingServicePickerDelegate`/`NSSharingServiceDelegate` methods
//! are declared in `document_window_controller.rs`'s `define_class!` and
//! forward to the methods below.
//!
//! Main-thread I/O, as in Swift: the share copy (and the PDF, rendered from
//! the print HTML) is written on the main thread.

use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{AllocAnyThread, MainThreadMarker, Message};
use objc2_app_kit::{NSSharingContentScope, NSSharingService, NSSharingServicePicker, NSView, NSWindow};
use objc2_foundation::{NSArray, NSRect, NSRectEdge, NSURL};
use upleft_foundation::url::FileUrl;
use upleft_render::appkit_compat::{RectExt, rect};

use crate::app::document_window_controller::DocumentWindowController;
use crate::app::document_window_controller_support::PrintRenderer;
use crate::export::document_share::{DocumentShareSource, DocumentShareStaging};
use crate::panels::appkit_support::object;

/// The extension's associated-object state (`sharingPicker`), held by the
/// controller as `share_state()`.
#[derive(Default)]
pub struct ShareState {
    /// `sharingPicker`: held for as long as the sheet is up.
    /// `NSSharingServicePicker` does not retain itself, and a picker released
    /// at the end of the command's stack frame takes its popover with it
    /// before the user can pick anything.
    pub(crate) sharing_picker: Option<Retained<NSSharingServicePicker>>,
}

/// `defer { endActivity() }`.
struct ActivityScope<'a>(&'a DocumentWindowController);

impl Drop for ActivityScope<'_> {
    fn drop(&mut self) {
        self.0.end_activity();
    }
}

impl DocumentWindowController {
    // MARK: - Commands

    /// `shareDocument()`.
    pub fn share_document(&self) {
        let document = self.markdown_document();
        let source =
            DocumentShareSource::choose(document.url().as_ref(), document.is_dirty(), &document.display_name());
        let url = match DocumentShareStaging::file_url(
            &source,
            || document.text(),
            document.fidelity(),
            &DocumentShareStaging::temporary_directory(),
        ) {
            Ok(url) => url,
            Err(error) => {
                // Never translate an I/O failure into "nothing happened": the
                // reader chose Share and would otherwise be left watching a
                // menu close on silence.
                self.present_operation_error("Couldn\u{2019}t prepare this document for sharing", &error.to_string());
                return;
            }
        };
        self.present_sharing_picker(&[url]);
    }

    /// `shareDocumentAsPDF()`.
    pub fn share_document_as_pdf(&self) {
        // The same renderer Print and Export PDF use, so what the receiver
        // opens is what the printer would have produced (§9.5).
        let exporter = self.exporter(true);
        self.begin_activity();
        let _activity = ActivityScope(self);
        let url = match DocumentShareStaging::pdf_url(
            &self.markdown_document().display_name(),
            &DocumentShareStaging::temporary_directory(),
        ) {
            Ok(url) => url,
            Err(error) => {
                self.present_operation_error("Couldn\u{2019}t prepare this document for sharing", &error.to_string());
                return;
            }
        };
        if !PrintRenderer::write_pdf(&exporter.html(), &url.to_nsurl(), MainThreadMarker::from(self)) {
            // `NSError(domain: "Upleft.Export", code: 1, userInfo:
            // [NSLocalizedDescriptionKey: …])`: its description is the text.
            self.present_operation_error(
                "Couldn\u{2019}t prepare this document for sharing",
                "The PDF renderer could not produce a file to share.",
            );
            return;
        }
        self.present_sharing_picker(&[url]);
    }

    // MARK: - The picker

    fn sharing_picker(&self) -> Option<Retained<NSSharingServicePicker>> {
        self.share_state().borrow().sharing_picker.clone()
    }

    fn set_sharing_picker(&self, picker: Option<Retained<NSSharingServicePicker>>) {
        self.share_state().borrow_mut().sharing_picker = picker;
    }

    /// `presentSharingPicker(items:)`.
    fn present_sharing_picker(&self, items: &[FileUrl]) {
        let Some((anchor, anchor_rect)) = self.sharing_anchor() else { return };
        // A second Share while one is open would leave the first picker
        // orphaned with no way to dismiss it.
        if let Some(picker) = self.sharing_picker() {
            picker.close();
        }
        let urls: Vec<Retained<NSURL>> = items.iter().map(FileUrl::to_nsurl).collect();
        let items = NSArray::from_retained_slice(&urls);
        // SAFETY: an `NSArray<NSURL>` is an `NSArray` of objects.
        let items: Retained<NSArray> = unsafe { Retained::cast_unchecked(items) };
        // SAFETY: the items are file URLs, which the picker accepts.
        let picker = unsafe { NSSharingServicePicker::initWithItems(NSSharingServicePicker::alloc(), &items) };
        picker.setDelegate(Some(ProtocolObject::from_ref(self)));
        self.set_sharing_picker(Some(picker.clone()));
        picker.showRelativeToRect_ofView_preferredEdge(anchor_rect, &anchor, NSRectEdge::MaxY);
    }

    /// `sharingAnchor()`: where the sheet flies out of.
    ///
    /// The `···` overflow is the toolbar control that already carries Export,
    /// so a share invoked from the File menu appears from the same corner as
    /// one invoked from the toolbar. When the window is too narrow for the
    /// cluster the button is gone, and the top edge of the document is the
    /// honest fallback.
    fn sharing_anchor(&self) -> Option<(Retained<NSView>, NSRect)> {
        if let Some(button) = self.toolbar_overflow_button()
            && button.window().is_some()
            && !button.isHiddenOrHasHiddenAncestor()
        {
            let view: &NSView = &button;
            return Some((view.retain(), button.bounds()));
        }
        let content = self.window()?.contentView()?;
        let bounds = content.bounds();
        let anchor_rect = rect(bounds.mid_x(), bounds.max_y() - 1.0, 1.0, 1.0);
        Some((content, anchor_rect))
    }

    // MARK: - NSSharingServicePickerDelegate

    /// `sharingServicePicker(_:delegateFor:)`.
    pub fn sharing_service_picker_delegate_for(
        &self,
        _picker: &NSSharingServicePicker,
        _service: &NSSharingService,
    ) -> Option<Retained<AnyObject>> {
        Some(object(self).retain())
    }

    /// `sharingServicePicker(_:didChoose:)`: called with a nil service when
    /// the picker was dismissed, so this is the one place that reliably fires
    /// either way.
    pub fn sharing_service_picker_did_choose(
        &self,
        sharing_service_picker: &NSSharingServicePicker,
        _service: Option<&NSSharingService>,
    ) {
        if self.sharing_picker().is_some_and(|picker| std::ptr::eq(&*picker, sharing_service_picker)) {
            self.set_sharing_picker(None);
        }
    }

    // MARK: - NSSharingServiceDelegate

    /// `sharingService(_:sourceWindowForShareItems:sharingContentScope:)`.
    /// `.full`: the item being shared is the whole document, not a fragment
    /// of it, which is what makes the Mail/Messages composer animate out of
    /// this window rather than the middle of the screen.
    pub fn sharing_service_source_window(
        &self,
        _service: &NSSharingService,
        _items: &NSArray,
        scope: NonNull<NSSharingContentScope>,
    ) -> Option<Retained<NSWindow>> {
        // SAFETY: AppKit passes a valid out-pointer.
        unsafe { scope.as_ptr().write(NSSharingContentScope::Full) };
        self.window()
    }
}
