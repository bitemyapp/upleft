//! Port of `Tests/DownrightAppTests/DocumentChromeLayoutTests.swift`: chrome
//! the window floats over the document has to coexist with chrome the
//! document container reserves space for.
//!
//! The Swift suite is `@MainActor` and `.serialized`: this binary owns the
//! main thread (`harness = false`, `tests/main_thread`), runs in a sandbox
//! (`tests/document_window_support`), and orders no window in.
//!
//! Skipped, with the reason:
//! - `changeBarClearsBreadcrumbLane`, `changeToastIgnoresTheBreadcrumbLane`:
//!   `ChangeSummaryBarView` is not yet ported on `port/panels`.
//! - `documentMapStaysOnLeadingWall`, `taskPanelOpensUsable`,
//!   `taskPanelRendersBodyAndHeader`, `floatingSurfaceLeavesDocumentRendered`,
//!   `menuAndRingOpenTasks`, `attachmentDoesNotReplaceGlass`,
//!   `nativeGlassHasNoOpaqueFirstFrame`,
//!   `taskGlassOwnsItsContentAtFullOpticalStrength`,
//!   `pointerHitReachesCloseButton`, `taskGlassUsesChildCompositorFromFirstFrame`,
//!   `surfaceFloatsInsideTheContentMargins`, `surfaceFitsItsContent`,
//!   `surfaceMeasurementIncludesEveryTaskRow`, `childWindowOwnsSurface`,
//!   `fittedSurfaceNeverCutsItsLastRow`, `arrivalContentIsPresentBeforeTheBodyGrows`,
//!   `arrivalKeepsOneGlassSurface`, `undoPillReservesScrollClearance`,
//!   `insideClickStaysInsideSurface`, `refitTracksWindowWidthAndHeight`,
//!   `surfaceCapsItsHeightAtSixtyPercent`, `closeDismissesTheSurface`,
//!   `rapidTaskPanelToggleRetargetsOneSurface`,
//!   `interruptedAnchorFlightDoesNotFallToWindowOrigin`,
//!   `panelSurvivesWindowActivationCycle`, `reduceMotionPresentsInstantly`:
//!   they open the Tasks panel (`TaskPanelView`, not yet ported on
//!   `port/panels`); several also order the floating child window in, which
//!   needs the document window at (-30000, -30000) first.
//! - `TaskProgressRingAccessibilityTests` (same Swift file): panel-only, with
//!   `TaskProgressRing` (`panels_task_tests`).

mod document_window_support;
mod main_thread;

use document_window_support::{enter_sandbox, leave_sandbox, mtm};
use objc2::MainThreadOnly;
use objc2_app_kit::{NSTextStorage, NSView};
use objc2_foundation::NSRect;
use std::rc::Rc;
use upleft_render::appkit_compat::rect;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::view::markdown_container_view::MarkdownContainerView;

/// One formula for the lane, shared by the container that reserves it and
/// the chrome that has to clear it.
fn overlaying_accessory_reserves_nothing() {
    let container =
        MarkdownContainerView::new(&NSTextStorage::new(), Rc::new(StyleSheet::current(mtm())), mtm());
    let accessory = NSView::initWithFrame(NSView::alloc(mtm()), rect(0.0, 0.0, 100.0, 28.0));
    container.set_top_accessory(Some(accessory.clone()));

    container.set_top_accessory_overlays_content(true);
    assert_eq!(container.top_lane_height(), 0.0);

    container.set_top_accessory_overlays_content(false);
    assert!(container.top_lane_height() > 0.0);

    accessory.setHidden(true);
    assert_eq!(container.top_lane_height(), 0.0);
}

fn no_accessory_reserves_nothing() {
    let container =
        MarkdownContainerView::new(&NSTextStorage::new(), Rc::new(StyleSheet::current(mtm())), mtm());
    container.set_top_accessory_overlays_content(false);
    assert_eq!(container.top_lane_height(), 0.0);
}

fn main() {
    enter_sandbox();
    let _ = NSRect::ZERO;
    main_thread::run(&[
        ("overlaying_accessory_reserves_nothing", overlaying_accessory_reserves_nothing),
        ("no_accessory_reserves_nothing", no_accessory_reserves_nothing),
    ]);
    leave_sandbox();
}
