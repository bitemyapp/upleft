//! Speech synthesis is unavailable without AVFAudio.
use std::rc::Weak;
use objc2::{AnyThread, MainThreadMarker, define_class, msg_send};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol};
use upleft_swift_text::NSRange;
pub trait SpeechCoordinatorDelegate {
    fn speech_coordinator_will_speak(&self, coordinator: &SpeechCoordinator, range: NSRange);
    fn speech_coordinator_did_finish(&self, coordinator: &SpeechCoordinator);
}
define_class!(
    #[unsafe(super(NSObject))]
    #[name = "SpeechCoordinator"]
    pub struct SpeechCoordinator;
    unsafe impl NSObjectProtocol for SpeechCoordinator {}
);
impl SpeechCoordinator {
    pub fn new(_: MainThreadMarker) -> Retained<Self> { unsafe { msg_send![Self::alloc(), init] } }
    pub fn set_delegate(&self, _: Option<Weak<dyn SpeechCoordinatorDelegate>>, _: MainThreadMarker) {}
    pub fn is_speaking(&self, _: MainThreadMarker) -> bool { false }
    pub fn speak(&self, _: &str, _: MainThreadMarker) -> bool { false }
    pub fn stop(&self, _: MainThreadMarker) {}
}
