//! Port of the parts of `Tests/DownrightAppTests/WindowChromeTests.swift`
//! that exercise the toolbar controls (`App/ToolbarControls.swift`) without
//! a `DocumentWindowController`.
//!
//! The Swift suite is `@MainActor`, and the controls are AppKit views, so
//! this binary owns the main thread (`harness = false`, `tests/main_thread`).
//! No window is ordered in; the one window the identity test needs is
//! borderless, sits at (-30000, -30000) and is never shown.
//!
//! Adapted: `documentIdentityShowsOnlyExceptionalStates` takes the identity
//! view from `controller.toolbarDocumentIdentityView` in Swift. Here it is
//! built the way the controller builds it (`ToolbarDocumentIdentityView(window:)`
//! with the new document's neutral state) on a borderless window; the
//! assertions are unchanged.
//!
//! Skipped, with the reason:
//! - `missingFileRecoveryOffersOnlyExplicitNativeChoices`,
//!   `toolbarUsesNativeCenteredModeAndTrailingMenu`,
//!   `splitViewUsesTwoVisibleSideBySideDocumentPanes`,
//!   `splitDividerIsThemedChromeRatherThanSystemGrey`,
//!   `documentBarsReserveSpaceAboveTheDocument`, `splitViewMirrorsPresentationState`,
//!   `floatingTaskPanelFitsItsFooterRow`,
//!   `inspectorSelectionAndCloseStayInSyncWithToolbar`,
//!   `localFindUsesCompactDocumentBar`, `replaceModePreservesActiveQuery`,
//!   `localFindPreservesViewport`, `findMotionDoesNotShiftDocument`,
//!   `ordinaryFindDoesNotReplaceItsQueryWithDocumentSelection`,
//!   `selectionFindIgnoresAnEmptySelection`,
//!   `closingTheFindBarRetiresItsOverlayWithoutRaising`,
//!   `findActionFlushesTheVisibleQueryBeforeTheDebounceFires`,
//!   `statusBarIsOffByDefaultAndCostsNoHeightWhenHidden`: need
//!   `DocumentWindowController` (not ported). The controller-free assertions
//!   of `toolbarUsesNativeCenteredModeAndTrailingMenu` (the controls' sizes,
//!   titles, spacing and the Find button's glass geometry) are checked in
//!   `toolbar_controls_tests.rs`.
//! - `breadcrumbReservesAStableTextSafeLane`, `breadcrumbAppearsOnlyWhenPresented`,
//!   `breadcrumbShowsOnlyTheCurrentSection`, `breadcrumbPathComparisonAvoidsScrollTimeRebuilds`
//!   (`BreadcrumbView`), `changeSummaryUsesCompactCountedNavigation`
//!   (`ChangeSummaryBarView`), `inspectorHostShowsExactlyOneOwnedSection`
//!   (`InspectorHostView`), `searchInspectorKeepsFindAndReplaceInOneSurface`,
//!   `searchInspectorLaysOutItsFindFieldInsideTheVisibleHeader`
//!   (`SearchInspectorView`), `replaceBarSettledLayout`,
//!   `replaceBarRapidToggleSettlesVisible`, `replaceBarReducedMotionToggleIsAtomic`,
//!   `inspectorFindKeepsTheSameRhythmWithoutADuplicateClose`,
//!   `findOptionsLiveInOneCompactMenu`, `everyFindBarControlDispatchesItsDocumentAction`,
//!   `findBarParksItsMatchActionsUntilThereIsSomethingToWalk` (`FindBarView`),
//!   `chromeGlassAccessibilityFallbackPolicyIsExplicit` (`ChromeGlass`): they
//!   test panel types, which belong to the panels port.

mod main_thread;

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSBackingStoreType, NSWindow, NSWindowStyleMask};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use upleft_app::ai::markdown_document::{Phase, PresentationState};
use upleft_app::app::toolbar_controls::{
    InteractionState, ScrubState, ToolbarChromePolicy, ToolbarDocumentIdentityView, ToolbarPresentationControl,
    ToolbarScrubPhase,
};
use upleft_app::panels::appkit_support::{RectExt, accessibility_label};

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("main thread")
}

/// A borderless window at (-30000, -30000), never ordered in.
fn offscreen_window() -> Retained<NSWindow> {
    let frame = NSRect::new(NSPoint::new(-30000.0, -30000.0), NSSize::new(1020.0, 780.0));
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm()),
            frame,
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window
}

fn tool_tip(identity: &ToolbarDocumentIdentityView) -> Option<String> {
    identity.toolTip().map(|tip| tip.to_string())
}

fn toolbar_chrome_policy_keeps_feedback_subtle_and_contrast_aware() {
    assert_eq!(ToolbarChromePolicy::feedback_opacity(InteractionState::Idle, false), 0.0);
    assert!(
        ToolbarChromePolicy::feedback_opacity(InteractionState::Hover, false)
            < ToolbarChromePolicy::feedback_opacity(InteractionState::Pressed, false)
    );
    assert!(
        ToolbarChromePolicy::feedback_opacity(InteractionState::Hover, true)
            > ToolbarChromePolicy::feedback_opacity(InteractionState::Hover, false)
    );
    assert!(ToolbarChromePolicy::indicator_opacity(true, false) > ToolbarChromePolicy::indicator_opacity(false, false));
    assert!(ToolbarChromePolicy::SELECTION_DURATION < 0.2);
}

fn document_identity_shows_only_exceptional_states() {
    let window = offscreen_window();
    let identity = ToolbarDocumentIdentityView::new(&window, mtm());
    identity.set_document_state(PresentationState::NEUTRAL);
    let cases = [
        (Phase::ChangedOnDisk, "Changed externally"),
        (Phase::Conflict, "Conflict"),
        (Phase::SaveFailed, "Save failed"),
    ];
    for (phase, label) in cases {
        identity.set_document_state(PresentationState::new(phase, Some("Paste".into()), Some("Example".into())));
        assert_eq!(accessibility_label(&*identity).map(|text| text.contains(label)), Some(true));
        assert_eq!(accessibility_label(&*identity).map(|text| text.contains("Paste")), Some(true));
        assert_eq!(tool_tip(&identity).map(|tip| tip.contains(label)), Some(true));
    }
    for phase in [Phase::Neutral, Phase::Edited, Phase::Saving, Phase::Saved] {
        identity.set_document_state(PresentationState::new(phase, Some("Paste".into()), Some("Example".into())));
        assert_ne!(accessibility_label(&*identity).map(|text| text.contains("Paste")), Some(true));
        assert_ne!(tool_tip(&identity).map(|tip| tip.contains("Paste")), Some(true));
    }

    identity.set_document_state(PresentationState::new(Phase::ChangedOnDisk, None, Some("File missing".into())));
    assert_eq!(accessibility_label(&*identity).map(|text| text.contains("File missing")), Some(true));
    assert_eq!(accessibility_label(&*identity).map(|text| text.contains("Changed externally")), Some(false));
    assert_eq!(tool_tip(&identity).map(|tip| tip.contains("File missing: File missing")), Some(false));
    window.close();
}

fn toolbar_scrub_policy_clamps_movement_and_crosses_at_the_midpoint() {
    assert_eq!(
        ToolbarChromePolicy::scrub_state(-20.0, 44.0, 132.0),
        ScrubState { indicator_center_x: 44.0, segment: 0 }
    );
    assert_eq!(
        ToolbarChromePolicy::scrub_state(87.0, 44.0, 132.0),
        ScrubState { indicator_center_x: 87.0, segment: 0 }
    );
    assert_eq!(
        ToolbarChromePolicy::scrub_state(88.0, 44.0, 132.0),
        ScrubState { indicator_center_x: 88.0, segment: 1 }
    );
    assert_eq!(
        ToolbarChromePolicy::scrub_state(240.0, 44.0, 132.0),
        ScrubState { indicator_center_x: 132.0, segment: 1 }
    );
}

fn toolbar_scrub_commits_once_on_release() {
    let changes: Rc<RefCell<Vec<isize>>> = Rc::new(RefCell::new(Vec::new()));
    let haptic_count = Rc::new(Cell::new(0));
    let recorded = changes.clone();
    let haptics = haptic_count.clone();
    let control = ToolbarPresentationControl::new_with_haptics(
        move |segment| recorded.borrow_mut().push(segment),
        move || haptics.set(haptics.get() + 1),
        mtm(),
    );
    control.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(184.0, 34.0)));
    control.layoutSubtreeIfNeeded();

    control.update_scrub(44.0, ToolbarScrubPhase::Began);
    control.update_scrub(132.0, ToolbarScrubPhase::Changed);
    assert_eq!(control.selected_segment(), 0);
    assert!(changes.borrow().is_empty());
    assert_eq!(haptic_count.get(), 1);

    control.update_scrub(132.0, ToolbarScrubPhase::Ended);
    assert_eq!(control.selected_segment(), 1);
    assert_eq!(*changes.borrow(), vec![1]);
}

fn toolbar_indicator_is_centered_under_the_selected_label_after_layout() {
    let control = ToolbarPresentationControl::new(|_| {}, mtm());
    control.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(184.0, 34.0)));
    control.layoutSubtreeIfNeeded();

    assert!(
        (control.selection_indicator_frame_for_testing().mid_x() - control.selected_segment_center_for_testing()).abs()
            < 0.01
    );
    assert_eq!(control.selection_indicator_frame_for_testing().width(), 34.0);
    assert!(control.selection_indicator_frame_for_testing().min_x() > 1.0);

    control.set_selected_segment(1);
    assert!(
        (control.selection_indicator_frame_for_testing().mid_x() - control.selected_segment_center_for_testing()).abs()
            < 0.01
    );
    assert!(control.selection_indicator_frame_for_testing().max_x() < 183.0);
}

fn main() {
    main_thread::run(&[
        (
            "toolbar_chrome_policy_keeps_feedback_subtle_and_contrast_aware",
            toolbar_chrome_policy_keeps_feedback_subtle_and_contrast_aware,
        ),
        ("document_identity_shows_only_exceptional_states", document_identity_shows_only_exceptional_states),
        (
            "toolbar_scrub_policy_clamps_movement_and_crosses_at_the_midpoint",
            toolbar_scrub_policy_clamps_movement_and_crosses_at_the_midpoint,
        ),
        ("toolbar_scrub_commits_once_on_release", toolbar_scrub_commits_once_on_release),
        (
            "toolbar_indicator_is_centered_under_the_selected_label_after_layout",
            toolbar_indicator_is_centered_under_the_selected_label_after_layout,
        ),
    ]);
}
