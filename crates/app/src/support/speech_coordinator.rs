//! Port of `Sources/DownrightApp/Support/SpeechCoordinator.swift`.
//!
//! Small native speech boundary. It owns one `AVSpeechSynthesizer` and one
//! active text, and reports progress to a delegate on the main thread.
//!
//! The Objective-C class is `SpeechCoordinator`, as in Swift, because it is
//! the synthesizer's delegate. AVFoundation may call the delegate methods on
//! any thread (Swift marks them `nonisolated`); they only hop to the main
//! queue, where all state lives, as Swift's `Task { @MainActor … }` does.

use std::cell::{Cell, RefCell};
use std::rc::Weak;

use dispatch2::{DispatchQueue, MainThreadBound};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, MainThreadMarker, define_class, msg_send};
use objc2_avf_audio::{AVSpeechBoundary, AVSpeechSynthesizer, AVSpeechSynthesizerDelegate, AVSpeechUtterance};
use objc2_foundation::{NSObject, NSObjectProtocol, NSRange as FoundationRange};
use upleft_swift_text::{self as swift_text, NSRange};

/// `SpeechCoordinatorDelegate` (main actor).
pub trait SpeechCoordinatorDelegate {
    fn speech_coordinator_will_speak(&self, coordinator: &SpeechCoordinator, range: NSRange);
    fn speech_coordinator_did_finish(&self, coordinator: &SpeechCoordinator);
}

struct State {
    synthesizer: Retained<AVSpeechSynthesizer>,
    is_speaking: Cell<bool>,
    delegate: RefCell<Option<Weak<dyn SpeechCoordinatorDelegate>>>,
}

pub struct SpeechCoordinatorIvars {
    state: MainThreadBound<State>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SpeechCoordinator"]
    #[ivars = SpeechCoordinatorIvars]
    pub struct SpeechCoordinator;

    unsafe impl NSObjectProtocol for SpeechCoordinator {}

    unsafe impl AVSpeechSynthesizerDelegate for SpeechCoordinator {
        #[unsafe(method(speechSynthesizer:willSpeakRangeOfSpeechString:utterance:))]
        fn will_speak_range(
            &self,
            _synthesizer: &AVSpeechSynthesizer,
            character_range: FoundationRange,
            _utterance: &AVSpeechUtterance,
        ) {
            let range = NSRange::new(character_range.location as isize, character_range.length as isize);
            self.on_main(move |coordinator, _| {
                if let Some(delegate) = coordinator.delegate() {
                    delegate.speech_coordinator_will_speak(coordinator, range);
                }
            });
        }

        #[unsafe(method(speechSynthesizer:didFinishSpeechUtterance:))]
        fn did_finish(&self, _synthesizer: &AVSpeechSynthesizer, _utterance: &AVSpeechUtterance) {
            self.finish_speaking();
        }

        #[unsafe(method(speechSynthesizer:didCancelSpeechUtterance:))]
        fn did_cancel(&self, _synthesizer: &AVSpeechSynthesizer, _utterance: &AVSpeechUtterance) {
            self.finish_speaking();
        }
    }
);

impl SpeechCoordinator {
    /// `SpeechCoordinator()`, with a fresh synthesizer.
    pub fn new(mtm: MainThreadMarker) -> Retained<SpeechCoordinator> {
        Self::with_synthesizer(unsafe { AVSpeechSynthesizer::new() }, mtm)
    }

    /// `SpeechCoordinator(synthesizer:)`.
    pub fn with_synthesizer(synthesizer: Retained<AVSpeechSynthesizer>, mtm: MainThreadMarker) -> Retained<SpeechCoordinator> {
        let state = State { synthesizer: synthesizer.clone(), is_speaking: Cell::new(false), delegate: RefCell::new(None) };
        let this = Self::alloc().set_ivars(SpeechCoordinatorIvars { state: MainThreadBound::new(state, mtm) });
        let this: Retained<SpeechCoordinator> = unsafe { msg_send![super(this), init] };
        unsafe { synthesizer.setDelegate(Some(ProtocolObject::from_ref(&*this))) };
        this
    }

    fn state(&self, mtm: MainThreadMarker) -> &State {
        self.ivars().state.get(mtm)
    }

    /// `weak var delegate`.
    pub fn set_delegate(&self, delegate: Option<Weak<dyn SpeechCoordinatorDelegate>>, mtm: MainThreadMarker) {
        *self.state(mtm).delegate.borrow_mut() = delegate;
    }

    fn delegate(&self) -> Option<std::rc::Rc<dyn SpeechCoordinatorDelegate>> {
        let mtm = MainThreadMarker::new()?;
        self.state(mtm).delegate.borrow().as_ref().and_then(Weak::upgrade)
    }

    pub fn is_speaking(&self, mtm: MainThreadMarker) -> bool {
        self.state(mtm).is_speaking.get()
    }

    /// Speaks `text`, replacing whatever was being spoken. False, and silent,
    /// for text that is only whitespace.
    pub fn speak(&self, text: &str, mtm: MainThreadMarker) -> bool {
        self.stop(mtm);
        if swift_text::trim_whitespaces_and_newlines(text).is_empty() {
            return false;
        }
        let state = self.state(mtm);
        let string = swift_text::ns::foundation::ns_from_utf16(&swift_text::ns::utf16(text));
        let utterance = unsafe { AVSpeechUtterance::speechUtteranceWithString(&string) };
        unsafe { state.synthesizer.speakUtterance(&utterance) };
        state.is_speaking.set(true);
        true
    }

    pub fn stop(&self, mtm: MainThreadMarker) {
        let state = self.state(mtm);
        if !(state.is_speaking.get() || unsafe { state.synthesizer.isSpeaking() }) {
            return;
        }
        unsafe { state.synthesizer.stopSpeakingAtBoundary(AVSpeechBoundary::Immediate) };
        state.is_speaking.set(false);
    }

    fn finish_speaking(&self) {
        self.on_main(|coordinator, mtm| {
            coordinator.state(mtm).is_speaking.set(false);
            if let Some(delegate) = coordinator.delegate() {
                delegate.speech_coordinator_did_finish(coordinator);
            }
        });
    }

    /// `Task { @MainActor [weak self] in … }`.
    fn on_main(&self, work: impl FnOnce(&SpeechCoordinator, MainThreadMarker) + Send + 'static) {
        let weak = objc2::rc::Weak::from(self);
        DispatchQueue::main().exec_async(move || {
            let Some(mtm) = MainThreadMarker::new() else { return };
            let Some(coordinator) = weak.load() else { return };
            work(&coordinator, mtm);
        });
    }
}
