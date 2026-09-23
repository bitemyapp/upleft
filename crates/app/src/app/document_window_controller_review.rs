//! Port of `App/DocumentWindowController+Review.swift`: local reviews
//! (comments and suggested replacements) kept in a sidecar file beside the
//! document.
//!
//! `ReviewPanelViewDelegate` is implemented on the controller's delegate
//! proxy and forwards to the methods below.
//!
//! Main-thread I/O, as in Swift: the sidecar (at most 8 MiB) is read and
//! written on the main thread.

use std::rc::{Rc, Weak};

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSBeep, NSModalResponse, NSStackView, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{NSArray, NSString};
use upleft_foundation::foundation_io::FoundationError;
use upleft_render::appkit_compat::rect;
use upleft_render::view::markdown_text_view_delegate::ScrollPosition;
use upleft_swift_text as swift;

use crate::app::document_window_controller::{DocumentWindowController, DocumentWindowControllerDelegates};
use crate::app::document_window_controller_asset_doctor::make_first_responder;
use crate::panels::appkit_support::set_label;
use crate::panels::panel_chrome::panel_title;
use crate::panels::review_panel_view::{ReviewPanelView, ReviewPanelViewDelegate};
use crate::review::review_anchor_resolver::{CONTEXT_LENGTH, ReviewAnchorResolver};
use crate::review::review_sidecar::{
    LocalReviewSidecarStore, ReviewApplyResult, ReviewItem, ReviewKind, ReviewSidecar, ReviewSidecarEngine,
    ReviewSidecarError, ReviewSidecarStore, ReviewState as ReviewItemState,
};
use crate::support::commands::Command;
use upleft_core::contracts::Uuid;

/// The extension's associated-object state (`reviewPanel`, `reviewStore`),
/// held by the controller as `review_state()`. The store is created on
/// first use, as Swift's getter does.
#[derive(Default)]
pub struct ReviewState {
    /// `reviewPanel`.
    pub(crate) review_panel: Option<Retained<ReviewPanelView>>,
    /// `reviewStore`.
    pub(crate) review_store: Option<Rc<dyn ReviewSidecarStore>>,
}

/// `error.localizedDescription` for what a sidecar store throws.
fn review_error(error: &ReviewSidecarError) -> String {
    match error {
        ReviewSidecarError::FileReadTooLarge => FoundationError::cocoa(263).description,
        ReviewSidecarError::Foundation(error) => error.description.clone(),
        ReviewSidecarError::Decoding(error) => error.localized_description().to_owned(),
    }
}

impl DocumentWindowController {
    fn review_panel(&self) -> Option<Retained<ReviewPanelView>> {
        self.review_state().borrow().review_panel.clone()
    }

    fn set_review_panel(&self, panel: Option<Retained<ReviewPanelView>>) {
        self.review_state().borrow_mut().review_panel = panel;
    }

    /// `reviewStore`: `LocalReviewSidecarStore()`, made on first use.
    fn review_store(&self) -> Rc<dyn ReviewSidecarStore> {
        if let Some(store) = self.review_state().borrow().review_store.clone() {
            return store;
        }
        let store: Rc<dyn ReviewSidecarStore> = Rc::new(LocalReviewSidecarStore::new());
        self.review_state().borrow_mut().review_store = Some(store.clone());
        store
    }

    /// `showReviewPanel()`: a toggle.
    pub fn show_review_panel(&self) {
        if let Some(review_panel) = self.review_panel() {
            self.dismiss_trailing(&review_panel);
            self.set_review_panel(None);
            return;
        }
        let panel = ReviewPanelView::new(self.active_style_sheet(), MainThreadMarker::from(self));
        let delegates = self.delegates();
        panel.set_delegate(Some(Rc::downgrade(&delegates) as Weak<dyn ReviewPanelViewDelegate>));
        self.set_review_panel(Some(panel.clone()));
        self.configure_review_panel(&panel);
        self.install_trailing(&panel, Some(&panel_title(Command::ReviewPanel)));
    }

    /// `ensureReviewPanelVisible()`: command-line review requests are
    /// idempotent, so reopening an already visible panel leaves it visible
    /// rather than toggling it closed.
    pub fn ensure_review_panel_visible(&self) {
        if self.review_panel().is_some() {
            return;
        }
        self.show_review_panel();
    }

    /// `configureReviewPanel(_:)`.
    pub fn configure_review_panel(&self, panel: &ReviewPanelView) {
        panel.set_source_text(&self.markdown_document().text());
        let Some(url) = self.markdown_document().url() else {
            panel.set_reviews(Vec::new());
            return;
        };
        match self.review_store().load(&url) {
            Ok(sidecar) => panel.set_reviews(sidecar.reviews),
            Err(error) => {
                panel.set_reviews(Vec::new());
                self.present_operation_error("Couldn\u{2019}t read document reviews", &review_error(&error));
            }
        }
    }

    /// `refreshReviewPanelIfVisible()`.
    pub fn refresh_review_panel_if_visible(&self) {
        let Some(review_panel) = self.review_panel() else { return };
        self.configure_review_panel(&review_panel);
    }

    /// `addReview(kind:body:replacement:)`: create a local review for the
    /// current source selection. The caller supplies the text shown to the
    /// reviewer; no network account is needed.
    pub fn add_review(&self, kind: ReviewKind, body: &str, replacement: Option<&str>) -> Option<ReviewItem> {
        let url = self.markdown_document().url()?;
        let review = ReviewSidecarEngine::make_review(
            kind,
            &self.markdown_document().text(),
            self.container_text_view().source_selected_range(),
            body,
            replacement,
        )?;
        let loaded: ReviewSidecar = match self.review_store().load(&url) {
            Ok(loaded) => loaded,
            Err(error) => {
                self.present_operation_error("Couldn\u{2019}t read document reviews", &review_error(&error));
                return None;
            }
        };
        let mut sidecar = loaded;
        sidecar.reviews.push(review.clone());
        if let Err(error) = self.review_store().save(&sidecar, &url) {
            self.present_operation_error("Couldn\u{2019}t save document reviews", &review_error(&error));
            return None;
        }
        if let Some(review_panel) = self.review_panel() {
            self.configure_review_panel(&review_panel);
        }
        Some(review)
    }

    /// `reviewPanel(_:didSelect:)`.
    pub fn review_panel_did_select(&self, _panel: &ReviewPanelView, review: &ReviewItem) {
        let resolution =
            ReviewAnchorResolver::resolve(&review.anchor, &self.markdown_document().text(), CONTEXT_LENGTH);
        let Some(range) = resolution.range else { return };
        if !(range.upper_bound() <= self.markdown_document().storage().length() as isize) {
            return;
        }
        self.container_text_view().set_source_selected_ranges(&[range]);
        self.container_text_view().scroll_to_offset(range.location, ScrollPosition::Visible, true);
        if let Some(window) = self.window() {
            make_first_responder(&window, &self.container_text_view());
        }
    }

    /// `reviewPanel(_:didApply:)`.
    pub fn review_panel_did_apply(&self, panel: &ReviewPanelView, review: &ReviewItem) {
        match ReviewSidecarEngine::apply_suggestion(review, &self.markdown_document().text()) {
            ReviewApplyResult::Stale(_) => self.configure_review_panel(panel),
            ReviewApplyResult::Applied(edit) => {
                self.markdown_document().apply(&[edit], "Apply Suggestion", None);
                self.markdown_document().reparse_now(false);
                self.update_review(review.id, ReviewItemState::Resolved);
                self.configure_review_panel(panel);
            }
        }
    }

    /// `reviewPanel(_:didResolve:)`.
    pub fn review_panel_did_resolve(&self, panel: &ReviewPanelView, review: &ReviewItem) {
        self.update_review(review.id, ReviewItemState::Resolved);
        self.configure_review_panel(panel);
    }

    /// `reviewPanel(_:didReject:)`.
    pub fn review_panel_did_reject(&self, panel: &ReviewPanelView, review: &ReviewItem) {
        self.update_review(review.id, ReviewItemState::Rejected);
        self.configure_review_panel(panel);
    }

    /// `presentAddReview(kind:)`.
    pub fn present_add_review(&self, kind: ReviewKind) {
        let Some(window) = self.window() else {
            NSBeep();
            return;
        };
        if !(self.container_text_view().source_selected_range().length > 0) {
            NSBeep();
            return;
        }
        let mtm = MainThreadMarker::from(self);
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(if kind == ReviewKind::Comment {
            "Add Comment"
        } else {
            "Suggest Replacement"
        }));
        alert.setInformativeText(&NSString::from_str("This review stays in a local sidecar file."));
        alert.addButtonWithTitle(&NSString::from_str("Add"));
        alert.addButtonWithTitle(&NSString::from_str("Cancel"));

        let body = NSTextField::textFieldWithString(&NSString::from_str(""), mtm);
        body.setPlaceholderString(Some(&NSString::from_str(if kind == ReviewKind::Comment {
            "Comment"
        } else {
            "Reason"
        })));
        let label = body.placeholderString().map(|text| text.to_string()).unwrap_or_else(|| "Review text".to_owned());
        set_label(&*body, &label);
        let replacement: Option<Retained<NSTextField>>;
        let fields: Retained<NSArray<NSView>>;
        if kind == ReviewKind::Suggestion {
            let field = NSTextField::textFieldWithString(&NSString::from_str(""), mtm);
            field.setPlaceholderString(Some(&NSString::from_str("Replacement text")));
            set_label(&*field, "Replacement text");
            fields = NSArray::from_slice(&[&**body as &NSView, &**field as &NSView]);
            replacement = Some(field);
        } else {
            replacement = None;
            fields = NSArray::from_slice(&[&**body as &NSView]);
        }
        let stack = NSStackView::stackViewWithViews(&fields, mtm);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        stack.setSpacing(8.0);
        stack.setFrame(rect(0.0, 0.0, 360.0, if kind == ReviewKind::Suggestion { 60.0 } else { 28.0 }));
        alert.setAccessoryView(Some(&stack));
        let weak_self: ObjcWeak<DocumentWindowController> = ObjcWeak::from(self);
        let handler = RcBlock::new(move |response: NSModalResponse| {
            if response != NSAlertFirstButtonReturn {
                return;
            }
            let body_text = body.stringValue().to_string();
            let review_body = swift::trim_whitespaces_and_newlines(&body_text).to_owned();
            let replacement_text = replacement.as_ref().map(|field| field.stringValue().to_string());
            if let Some(this) = weak_self.load() {
                let _ = this.add_review(
                    kind,
                    if review_body.is_empty() && kind == ReviewKind::Suggestion {
                        "Suggested replacement"
                    } else {
                        &review_body
                    },
                    replacement_text.as_deref(),
                );
            }
        });
        alert.beginSheetModalForWindow_completionHandler(&window, Some(&handler));
    }

    /// `updateReview(_:state:)`.
    fn update_review(&self, id: Uuid, state: ReviewItemState) {
        let Some(url) = self.markdown_document().url() else { return };
        let loaded: ReviewSidecar = match self.review_store().load(&url) {
            Ok(loaded) => loaded,
            Err(error) => {
                self.present_operation_error("Couldn\u{2019}t read document reviews", &review_error(&error));
                return;
            }
        };
        let mut sidecar = loaded;
        let Some(index) = sidecar.reviews.iter().position(|review| review.id == id) else { return };
        sidecar.reviews[index].state = state;
        if let Err(error) = self.review_store().save(&sidecar, &url) {
            self.present_operation_error("Couldn\u{2019}t save document reviews", &review_error(&error));
        }
    }
}

// MARK: - ReviewPanelViewDelegate

impl ReviewPanelViewDelegate for DocumentWindowControllerDelegates {
    fn review_panel_did_select(&self, panel: &ReviewPanelView, review: &ReviewItem) {
        if let Some(controller) = self.controller() {
            controller.review_panel_did_select(panel, review);
        }
    }

    fn review_panel_did_apply(&self, panel: &ReviewPanelView, review: &ReviewItem) {
        if let Some(controller) = self.controller() {
            controller.review_panel_did_apply(panel, review);
        }
    }

    fn review_panel_did_reject(&self, panel: &ReviewPanelView, review: &ReviewItem) {
        if let Some(controller) = self.controller() {
            controller.review_panel_did_reject(panel, review);
        }
    }

    fn review_panel_did_resolve(&self, panel: &ReviewPanelView, review: &ReviewItem) {
        if let Some(controller) = self.controller() {
            controller.review_panel_did_resolve(panel, review);
        }
    }
}
