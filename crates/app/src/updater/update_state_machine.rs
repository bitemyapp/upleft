//! Port of `Sources/DownrightApp/Updater/UpdateStateMachine.swift`.

use super::update_metadata::{UpdateFailure, UpdateMetadata};

/// What stage a presented update has reached inside Sparkle. Mirrors
/// `SPUUserUpdateStage` without importing Sparkle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateStage {
    /// Nothing has been downloaded yet.
    NotDownloaded,
    /// Already downloaded in the background (automatic updates) but not begun installing.
    Downloaded,
    /// Already downloaded and begun installing in the background.
    Installing,
}

/// The complete update user flow, as a pure value type. Nothing here touches
/// a view or a network; the UI layer reduces `UpdateEvent`s and renders the
/// resulting `UpdatePhase`, which is what makes every transition unit-testable
/// without Sparkle or AppKit in the loop.
///
/// Capabilities (reply / cancellation / acknowledgement / retry closures) are
/// *not* stored here: the coordinator keeps them in `Capability` boxes so the
/// machine stays `Equatable` and the exactly-once contract lives in one place.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateStateMachine {
    phase: UpdatePhase,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum UpdatePhase {
    #[default]
    Idle,
    /// A check is running. `user_initiated` decides whether the panel and
    /// pill are involved or the check is a quiet background cycle.
    Checking { user_initiated: bool },
    /// An update was found and the user must choose.
    Available(UpdateMetadata, UpdateStage),
    /// Downloading, with the last-known byte counts. `expected` may be
    /// `None` (server sent no content length) and may legitimately disagree
    /// with `received` (invalid or repeated length callbacks).
    Downloading { received: u64, expected: Option<u64> },
    /// Extracting the downloaded archive. `progress` is `None` until Sparkle
    /// reports a 0.0–1.0 figure. (Swift `Double` equality: a NaN progress is
    /// unequal to itself, as `f64` is.)
    Extracting { progress: Option<f64> },
    /// Extraction finished; the update is ready to install and relaunch.
    ReadyToRelaunch,
    /// Installation began but the app is still running (termination was
    /// delayed or cancelled, e.g. a dirty-document save failed). The user
    /// can retry termination or leave the update to install on quit.
    WaitingForTermination,
    /// The app has terminated and the update is being installed.
    Installing,
    /// An informational-only update: release info plus a "Learn More" URL.
    /// Never offers install.
    Informational(UpdateMetadata),
    /// A manual check found nothing new.
    UpToDate,
    /// Something went wrong. `retryable` says whether a Retry button is
    /// meaningful.
    Failed(UpdateFailure, bool),
}

/// Events the driver translates Sparkle callbacks into. Pure, so the
/// machine can be exercised from tests without a driver at all.
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateEvent {
    UserInitiatedCheckBegan,
    AutomaticCheckBegan,
    UpdateFound(UpdateMetadata, UpdateStage),
    ReleaseNotesAvailable,
    ReleaseNotesFailed,
    UpdateNotFound { user_initiated: bool },
    UpdaterError(UpdateFailure),
    DownloadInitiated,
    ExpectedLength(u64),
    DataReceived(u64),
    ExtractionBegan,
    ExtractionProgress(f64),
    ReadyToInstallAndRelaunch,
    InstallingUpdate { application_terminated: bool },
    UpdateInstalled { relaunched: bool },
    /// Sparkle aborted or finished and told us to tear everything down.
    Dismissed,
    /// The user cancelled a check (panel Cancel, or closing the panel
    /// while a check is running). Sparkle calls nothing after a cancelled
    /// check, so the machine must leave `.checking` itself.
    CheckCancelled,
    /// The user cancelled a download. Sparkle follows a cancelled download
    /// with `dismissUpdateInstallation()`, but the machine transitions
    /// immediately so the UI never renders a stale progress state.
    DownloadCancelled,
}

/// Swift's generic `max(_:_:)`: `y >= x ? y : x`. Unlike `f64::max`, a NaN
/// first argument survives (`max(.nan, 0)` is NaN) and `max(-0.0, 0)` is `+0`.
pub(crate) fn swift_max<T: PartialOrd>(x: T, y: T) -> T {
    if y >= x { y } else { x }
}

/// Swift's generic `min(_:_:)`: `y < x ? y : x`.
pub(crate) fn swift_min<T: PartialOrd>(x: T, y: T) -> T {
    if y < x { y } else { x }
}

impl UpdateStateMachine {
    pub fn new() -> UpdateStateMachine {
        UpdateStateMachine::default()
    }

    pub fn phase(&self) -> &UpdatePhase {
        &self.phase
    }

    pub fn reduce(&mut self, event: &UpdateEvent) {
        match event {
            UpdateEvent::UserInitiatedCheckBegan => {
                self.phase = UpdatePhase::Checking { user_initiated: true };
            }

            UpdateEvent::AutomaticCheckBegan => {
                self.phase = UpdatePhase::Checking { user_initiated: false };
            }

            UpdateEvent::UpdateFound(metadata, stage) => {
                self.phase = if metadata.is_information_only {
                    UpdatePhase::Informational(metadata.clone())
                } else {
                    UpdatePhase::Available(metadata.clone(), *stage)
                };
            }

            UpdateEvent::ReleaseNotesAvailable | UpdateEvent::ReleaseNotesFailed => {
                // Notes are a detail of the current phase; they never move the
                // machine out of it. The panel listens separately.
            }

            UpdateEvent::UpdateNotFound { user_initiated } => {
                self.phase = if *user_initiated { UpdatePhase::UpToDate } else { UpdatePhase::Idle };
            }

            UpdateEvent::UpdaterError(failure) => {
                self.phase = UpdatePhase::Failed(failure.clone(), failure.retryable);
            }

            UpdateEvent::DownloadInitiated => {
                self.phase = UpdatePhase::Downloading { received: 0, expected: None };
            }

            UpdateEvent::ExpectedLength(length) => {
                // May arrive more than once for the same download and may disagree
                // with what was previously reported; the latest value wins.
                if let UpdatePhase::Downloading { received, .. } = self.phase {
                    self.phase = UpdatePhase::Downloading { received, expected: Some(*length) };
                }
            }

            UpdateEvent::DataReceived(length) => {
                if let UpdatePhase::Downloading { received, expected } = self.phase {
                    let total = received.wrapping_add(*length);
                    // Clamp at the expected size when one is known: a misbehaving
                    // server can over-report, and progress must never exceed 100%.
                    self.phase = match expected {
                        Some(expected) => UpdatePhase::Downloading { received: swift_min(total, expected), expected: Some(expected) },
                        None => UpdatePhase::Downloading { received: total, expected: None },
                    };
                }
            }

            UpdateEvent::ExtractionBegan => {
                self.phase = UpdatePhase::Extracting { progress: None };
            }

            UpdateEvent::ExtractionProgress(progress) => {
                if let UpdatePhase::Extracting { .. } = self.phase {
                    self.phase = UpdatePhase::Extracting { progress: Some(swift_min(swift_max(*progress, 0.0), 1.0)) };
                }
            }

            UpdateEvent::ReadyToInstallAndRelaunch => {
                self.phase = UpdatePhase::ReadyToRelaunch;
            }

            UpdateEvent::InstallingUpdate { application_terminated } => {
                self.phase =
                    if *application_terminated { UpdatePhase::Installing } else { UpdatePhase::WaitingForTermination };
            }

            UpdateEvent::UpdateInstalled { .. } => {
                self.phase = UpdatePhase::Idle;
            }

            UpdateEvent::Dismissed => {
                // A dismissal can arrive from *any* phase (Sparkle aborts cycles);
                // every pending capability is discarded by the coordinator.
                self.phase = UpdatePhase::Idle;
            }

            UpdateEvent::CheckCancelled | UpdateEvent::DownloadCancelled => {
                self.phase = UpdatePhase::Idle;
            }
        }
    }
}
