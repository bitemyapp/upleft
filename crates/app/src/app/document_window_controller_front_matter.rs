//! Port of `App/DocumentWindowController+FrontMatter.swift`. Front matter
//! editing is a source-preserving panel. The controller owns the document
//! mutation, so each field change is one normal undo step.
//!
//! `FrontMatterEditorDelegate` is implemented on the controller's delegate
//! proxy and forwards to the methods below.

use std::rc::{Rc, Weak};

use objc2::MainThreadMarker;
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2_app_kit::NSBeep;
use upleft_core::editing::front_matter_editing::{FrontMatterEditOperation, FrontMatterEditing};
use upleft_render::appkit_compat::main_async;

use crate::app::document_window_controller::{DocumentWindowController, DocumentWindowControllerDelegates};
use crate::app::document_window_controller_asset_doctor::make_first_responder;
use crate::panels::front_matter_editor_view::{FrontMatterEditorDelegate, FrontMatterEditorView};
use crate::panels::panel_chrome::panel_title;
use crate::support::commands::Command;

impl DocumentWindowController {
    /// `showFrontMatterEditor(focus:)` (`focus` defaults to `nil`).
    pub fn show_front_matter_editor(&self, focus: Option<&str>) {
        let viewport_repairs: Rc<Vec<Box<dyn Fn()>>> = Rc::new(
            self.document_panes()
                .iter()
                .map(|pane| Box::new(pane.text_view().make_viewport_repair()) as Box<dyn Fn()>)
                .collect(),
        );
        self.set_asset_doctor_panel(None);
        let editor: Retained<FrontMatterEditorView> = if let Some(current) = self.front_matter_editor() {
            current
        } else {
            let created = FrontMatterEditorView::new(self.active_style_sheet(), MainThreadMarker::from(self));
            let delegates = self.delegates();
            created.set_delegate(Some(Rc::downgrade(&delegates) as Weak<dyn FrontMatterEditorDelegate>));
            self.set_front_matter_editor(Some(created.clone()));
            created
        };
        editor.set_document(self.markdown_document().parsed());
        self.install_trailing(&editor, Some(&panel_title(Command::FrontMatterEditor)));
        viewport_repairs.iter().for_each(|repair| repair());
        let focus: Option<String> = focus.map(str::to_owned);
        {
            let viewport_repairs = viewport_repairs.clone();
            let weak_editor: ObjcWeak<FrontMatterEditorView> = ObjcWeak::from(&*editor);
            let focus = focus.clone();
            main_async(move || {
                viewport_repairs.iter().for_each(|repair| repair());
                if let Some(editor) = weak_editor.load() {
                    editor.prepare_for_presentation(focus.as_deref());
                }
            });
        }
        // The trailing host performs its own key-loop handoff after layout.
        // Reassert the semantic field on the first settled layout turn, or
        // AX/keyboard users can land on the window itself.
        let weak_self: ObjcWeak<DocumentWindowController> = ObjcWeak::from(self);
        let weak_editor: ObjcWeak<FrontMatterEditorView> = ObjcWeak::from(&*editor);
        main_async(move || {
            let (Some(this), Some(editor)) = (weak_self.load(), weak_editor.load()) else { return };
            if !this
                .front_matter_editor()
                .is_some_and(|current| Retained::as_ptr(&current) == Retained::as_ptr(&editor))
            {
                return;
            }
            if let Some(content_view) = this.window().and_then(|window| window.contentView()) {
                content_view.layoutSubtreeIfNeeded();
            }
            viewport_repairs.iter().for_each(|repair| repair());
            editor.focus_field(focus.as_deref());
        });
    }

    /// `frontMatterEditor(_:didRequest:)`.
    pub fn front_matter_editor_did_request(&self, editor: &FrontMatterEditorView, operation: FrontMatterEditOperation) {
        self.markdown_document().ensure_parsed_current();
        let result = FrontMatterEditing::propose(&self.markdown_document().parsed(), &operation);
        let Some(proposal) = result.proposal else {
            editor.set_document(self.markdown_document().parsed());
            NSBeep();
            return;
        };

        // `apply` groups the replacement and registers the inverse with the
        // document undo manager. The editor never writes NSTextStorage itself.
        self.markdown_document().apply(&[proposal.edit()], &proposal.summary, None);
        self.markdown_document().reparse_now(false);
        editor.set_document(self.markdown_document().parsed());
    }

    /// `frontMatterEditorWantsSourceMode(_:)`.
    pub fn front_matter_editor_wants_source_mode(&self, _editor: &FrontMatterEditorView) {
        if let Some(front) = &self.markdown_document().parsed().front_matter {
            self.primary_container().text_view().focus_source(front.range);
            if let Some(window) = self.window() {
                make_first_responder(&window, self.primary_container().text_view());
            }
        }
    }
}

// MARK: - FrontMatterEditorDelegate

impl FrontMatterEditorDelegate for DocumentWindowControllerDelegates {
    fn front_matter_editor_did_request(&self, editor: &FrontMatterEditorView, operation: FrontMatterEditOperation) {
        if let Some(controller) = self.controller() {
            controller.front_matter_editor_did_request(editor, operation);
        }
    }

    fn front_matter_editor_wants_source_mode(&self, editor: &FrontMatterEditorView) {
        if let Some(controller) = self.controller() {
            controller.front_matter_editor_wants_source_mode(editor);
        }
    }
}
