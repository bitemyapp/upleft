//! Port of `App/DocumentWindowController+Speech.swift`: reading the
//! selection (or the document) aloud with the spoken range highlighted.

use std::rc::{Rc, Weak};

use objc2::rc::Retained;
use upleft_core::NSRange;
use upleft_render::view::markdown_text_view_delegate::ScrollPosition;

use crate::app::document_window_controller::{DocumentWindowController, DocumentWindowControllerDelegates};
use crate::support::speech_coordinator::{SpeechCoordinator, SpeechCoordinatorDelegate};

/// The extension's associated objects: `speechCoordinator` (created on first
/// use) and `speechSourceRange`.
#[derive(Default)]
pub struct SpeechState {
    pub(crate) speech_coordinator: Option<Retained<SpeechCoordinator>>,
    pub(crate) speech_source_range: NSRange,
}

impl DocumentWindowController {
    /// `var isSpeakingDocument`.
    pub fn is_speaking_document(&self) -> bool {
        self.speech_coordinator().is_speaking(self.mtm())
    }

    /// `private var speechCoordinator`: created, with `self` as its
    /// delegate, the first time anything asks for it.
    fn speech_coordinator(&self) -> Retained<SpeechCoordinator> {
        if let Some(coordinator) = self.speech_state().borrow().speech_coordinator.clone() {
            return coordinator;
        }
        let coordinator = SpeechCoordinator::new(self.mtm());
        let delegate: Weak<dyn SpeechCoordinatorDelegate> = Rc::downgrade(&self.delegates()) as _;
        coordinator.set_delegate(Some(delegate), self.mtm());
        self.speech_state().borrow_mut().speech_coordinator = Some(coordinator.clone());
        coordinator
    }

    fn speech_source_range(&self) -> NSRange {
        self.speech_state().borrow().speech_source_range
    }

    fn set_speech_source_range(&self, range: NSRange) {
        self.speech_state().borrow_mut().speech_source_range = range;
    }

    /// `speakSelectionOrDocument()`.
    pub fn speak_selection_or_document(&self) {
        let text_view = self.container_text_view();
        let selected = text_view.source_selected_range();
        let source = if selected.length > 0 {
            selected
        } else {
            NSRange { location: 0, length: self.markdown_document().storage().length() as isize }
        };
        let text = text_view.rendered_string_for_speech(source);
        self.set_speech_source_range(source);
        if !self.speech_coordinator().speak(&text, self.mtm()) {
            self.container_text_view().set_speech_highlight(None);
        }
    }

    /// `stopSpeaking()`.
    pub fn stop_speaking(&self) {
        self.speech_coordinator().stop(self.mtm());
        self.container_text_view().set_speech_highlight(None);
    }

    /// `speechCoordinator(_:willSpeak:)`.
    pub fn speech_coordinator_will_speak(&self, _coordinator: &SpeechCoordinator, range: NSRange) {
        let text_view = self.container_text_view();
        let Some(source) = text_view.source_range_for_speech_range(range, self.speech_source_range()) else { return };
        text_view.set_speech_highlight(Some(source));
        self.container_text_view().scroll_to_offset(source.location, ScrollPosition::Visible, false);
    }

    /// `speechCoordinatorDidFinish(_:)`.
    pub fn speech_coordinator_did_finish(&self, _coordinator: &SpeechCoordinator) {
        self.container_text_view().set_speech_highlight(None);
    }
}

impl SpeechCoordinatorDelegate for DocumentWindowControllerDelegates {
    fn speech_coordinator_will_speak(&self, coordinator: &SpeechCoordinator, range: NSRange) {
        if let Some(controller) = self.controller() {
            controller.speech_coordinator_will_speak(coordinator, range);
        }
    }

    fn speech_coordinator_did_finish(&self, coordinator: &SpeechCoordinator) {
        if let Some(controller) = self.controller() {
            controller.speech_coordinator_did_finish(coordinator);
        }
    }
}
