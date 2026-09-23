//! Port of `App/DocumentWindowController+TableEditor.swift`. The visual table
//! editor is transient. It writes through `MarkdownDocument`, then refreshes
//! both surfaces from the new parsed snapshot.
//!
//! `TableEditorDelegate` is implemented on the controller's delegate proxy
//! and forwards to the methods below.

use std::rc::{Rc, Weak};

use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSBackingStoreType, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSSize, NSString};
use upleft_core::editing::table_editing::TableEditProposal;
use upleft_core::{BlockContent, NSRange};
use upleft_render::appkit_compat::rect;

use crate::app::document_window_controller::{DocumentWindowController, DocumentWindowControllerDelegates};
use crate::app::document_window_controller_asset_doctor::make_first_responder;
use crate::panels::table_editor_view::{TableEditorDelegate, TableEditorView};

impl DocumentWindowController {
    /// `caretIsInTable`.
    pub fn caret_is_in_table(&self) -> bool {
        self.table_index(self.caret_offset()).is_some()
    }

    /// `presentTableEditor()`.
    pub fn present_table_editor(&self) {
        let Some(window) = self.window() else { return };
        self.markdown_document().ensure_parsed_current();
        let index = self.table_index(self.caret_offset()).unwrap_or(0);
        let mtm = MainThreadMarker::from(self);
        let editor = TableEditorView::new(self.markdown_document().parsed(), index, self.current_style_sheet(), mtm);
        let delegates = self.delegates();
        editor.set_delegate(Some(Rc::downgrade(&delegates) as Weak<dyn TableEditorDelegate>));

        // SAFETY: `releasedWhenClosed` is turned off below, before the
        // window can be closed, so the `Retained` references stay valid.
        let sheet_window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(0.0, 0.0, 720.0, 480.0),
                NSWindowStyleMask::Titled | NSWindowStyleMask::Closable | NSWindowStyleMask::Resizable,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // Not in Swift, where ARC owns the sheet: a Rust-owned window must not
        // release itself on close. The sheet is only ever ended, never
        // closed, so nothing observable changes.
        // SAFETY: plain property.
        unsafe { sheet_window.setReleasedWhenClosed(false) };
        sheet_window.setTitle(&NSString::from_str("Edit Table"));
        sheet_window.setMinSize(NSSize::new(520.0, 320.0));
        sheet_window.setContentView(Some(&editor));
        self.set_table_editor_window(Some(sheet_window.clone()));
        window.beginSheet_completionHandler(&sheet_window, None);
    }

    /// `tableEditor(_:didApply:)`.
    pub fn table_editor_did_apply(&self, editor: &TableEditorView, proposal: &TableEditProposal) {
        if proposal.applying(&self.markdown_document().text()).is_none() {
            editor.update(self.markdown_document().parsed());
            return;
        }
        if !self.markdown_document().replace(proposal.range, &proposal.replacement, Some(&proposal.summary)) {
            return;
        }
        self.markdown_document().reparse_now(false);
        editor.update(self.markdown_document().parsed());
    }

    /// `tableEditor(_:didRequestSource:)`.
    pub fn table_editor_did_request_source(&self, _editor: &TableEditorView, range: NSRange) {
        self.close_table_editor();
        self.container_text_view().focus_source(range);
        if let Some(window) = self.window() {
            make_first_responder(&window, &self.container_text_view());
        }
    }

    /// `tableEditorDidFinish(_:)`.
    pub fn table_editor_did_finish(&self, _editor: &TableEditorView) {
        self.close_table_editor();
    }

    /// `tableEditorDidCancel(_:)`: every accepted operation was exactly one
    /// undo step.
    pub fn table_editor_did_cancel(&self, editor: &TableEditorView) {
        for _ in 0..editor.applied_edit_count() {
            if self.markdown_document().undo_manager().canUndo() {
                self.markdown_document().undo_manager().undo();
            }
        }
        self.markdown_document().reparse_now(false);
        self.close_table_editor();
    }

    /// `closeTableEditor()`.
    pub fn close_table_editor(&self) {
        let Some(sheet_window) = self.table_editor_window() else { return };
        if let Some(window) = self.window() {
            window.endSheet(&sheet_window);
        }
        self.set_table_editor_window(None);
    }

    /// `tableIndex(at:)`: the index, among the document's tables in walk
    /// order, of the first table whose range touches `offset`.
    fn table_index(&self, offset: isize) -> Option<isize> {
        let mut index = 0;
        let mut result: Option<isize> = None;
        let parsed = self.markdown_document().parsed();
        parsed.root.walk(&mut |block| {
            let BlockContent::Table(_) = block.content else { return };
            if result.is_none() && block.range.touches(offset) {
                result = Some(index);
            }
            index += 1;
        });
        result
    }
}

// MARK: - TableEditorDelegate

impl TableEditorDelegate for DocumentWindowControllerDelegates {
    fn table_editor_did_apply(&self, editor: &TableEditorView, proposal: &TableEditProposal) {
        if let Some(controller) = self.controller() {
            controller.table_editor_did_apply(editor, proposal);
        }
    }

    fn table_editor_did_request_source(&self, editor: &TableEditorView, range: NSRange) {
        if let Some(controller) = self.controller() {
            controller.table_editor_did_request_source(editor, range);
        }
    }

    fn table_editor_did_finish(&self, editor: &TableEditorView) {
        if let Some(controller) = self.controller() {
            controller.table_editor_did_finish(editor);
        }
    }

    fn table_editor_did_cancel(&self, editor: &TableEditorView) {
        if let Some(controller) = self.controller() {
            controller.table_editor_did_cancel(editor);
        }
    }
}
