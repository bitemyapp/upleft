//! Port of `App/DocumentWindowController+Speech.swift`.

use objc2::rc::Retained;
use upleft_core::NSRange;

use crate::support::speech_coordinator::SpeechCoordinator;

/// The extension's associated objects: `speechCoordinator` (created on first
/// use) and `speechSourceRange`.
#[derive(Default)]
pub struct SpeechState {
    pub(crate) speech_coordinator: Option<Retained<SpeechCoordinator>>,
    pub(crate) speech_source_range: NSRange,
}
