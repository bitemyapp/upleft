//! Port of the `DocumentWindowController` cases of
//! `Tests/DownrightAppTests/WindowChromeTests.swift` that exercise the
//! controller itself (`App/DocumentWindowController.swift`). The
//! controller-free cases are in `window_chrome_tests.rs`.
//!
//! The Swift suite is `@MainActor` and `.serialized`: this binary owns the
//! main thread (`harness = false`, `tests/main_thread`), runs in a sandbox
//! (`tests/document_window_support`), and orders no window in.
//!
//! Skipped, with the reason:
//! - `missingFileRecoveryOffersOnlyExplicitNativeChoices`: presents the
//!   recovery alert as a sheet on the (never shown) document window, and
//!   AppKit may put the alert's titled window on a screen.
//! - `documentIdentityShowsOnlyExceptionalStates`,
//!   `toolbarUsesNativeCenteredModeAndTrailingMenu`,
//!   `inspectorSelectionAndCloseStayInSyncWithToolbar`: the toolbar items come
//!   from `+Actions` (ported separately).
//! - `splitViewMirrorsPresentationState`, `selectionFindIgnoresAnEmptySelection`:
//!   drive `perform(_:)` (`+Commands`, ported separately).
//! - `documentBarsReserveSpaceAboveTheDocument` (`ChangeSummaryBarView`),
//!   `floatingTaskPanelFitsItsFooterRow` (`TaskPanelView`),
//!   `localFindUsesCompactDocumentBar`, `replaceModePreservesActiveQuery`,
//!   `localFindPreservesViewport`, `findMotionDoesNotShiftDocument`,
//!   `ordinaryFindDoesNotReplaceItsQueryWithDocumentSelection`,
//!   `closingTheFindBarRetiresItsOverlayWithoutRaising`,
//!   `findActionFlushesTheVisibleQueryBeforeTheDebounceFires` (`FindBarView`):
//!   need panels not yet ported on `port/panels`.

mod document_window_support;
mod main_thread;

use std::rc::Rc;

use document_window_support::{enter_sandbox, leave_sandbox, mtm};
use objc2_app_kit::{NSAppearance, NSAppearanceNameDarkAqua, NSSplitView};
use objc2_foundation::NSObjectProtocol;
use upleft_app::app::document_window_controller::DocumentWindowController;
use upleft_app::support::preferences::{Preferences, Values};
use upleft_render::render_contracts::RenderMode;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;

fn split_view_uses_two_visible_side_by_side_document_panes() {
    let controller = DocumentWindowController::new(mtm());

    controller.toggle_split_view();
    let split = controller.split_view_container().expect("splitViewContainer");
    if let Some(content) = controller.window().and_then(|window| window.contentView()) {
        content.layoutSubtreeIfNeeded();
    }

    assert!(split.isVertical());
    let arranged = split.arrangedSubviews();
    assert_eq!(arranged.count(), 2);
    assert!(arranged.iter().all(|view| view.frame().size.width > 0.0));
    assert_eq!(controller.primary_container().text_view().mode(), RenderMode::Live);
    assert_eq!(controller.split_container().map(|container| container.text_view().mode()), Some(RenderMode::Live));
    controller.close();
}

fn split_divider_is_themed_chrome_rather_than_system_grey() {
    let controller = DocumentWindowController::new(mtm());

    controller.toggle_split_view();
    let split = controller.split_view_container().expect("splitViewContainer");

    // The seam between two panes of prose is a rule like any other, and
    // AppKit's default grey reads as nothing against a themed page.
    let split_view: &NSSplitView = &split;
    assert!(split_view.dividerColor().isEqual(Some(&controller.active_style_sheet().rule)));

    // It also has to keep up: a theme change that repaints the panes but
    // leaves the hairline behind is the same bug one repaint later.
    let dark = ThemeStore::shared()
        .themes()
        .into_iter()
        .find(|theme| theme.name == "Warm Dark")
        .expect("the Warm Dark theme");
    let appearance = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua })
        .unwrap_or_else(NSAppearance::currentDrawingAppearance);
    let sheet = Rc::new(StyleSheet::new(dark, &appearance, None));
    split.set_style_sheet(sheet.clone());
    assert!(split_view.dividerColor().isEqual(Some(&sheet.rule)));
    controller.close();
}

/// The status bar is opt-in: DESIGN.md's "Avoid" list names a permanent
/// status bar. It shipped visible and unconditional, with no toggle anywhere.
fn status_bar_is_off_by_default_and_costs_no_height_when_hidden() {
    assert!(!Values::default().show_status_bar);
    let original = Preferences::shared().values();
    Preferences::shared().update(|values| values.show_status_bar = false);

    let controller = DocumentWindowController::new(mtm());
    let _ = controller.window();

    let status_bar = controller.status_bar_view();
    assert!(!status_bar.is_visible());

    status_bar.set_is_visible(false);
    assert!(status_bar.isHidden());
    assert_eq!(status_bar.intrinsicContentSize().height, 0.0);

    status_bar.set_is_visible(true);
    assert!(!status_bar.isHidden());
    assert!(status_bar.intrinsicContentSize().height > 0.0);
    controller.close();
    Preferences::shared().update(|values| *values = original.clone());
}

fn main() {
    enter_sandbox();
    main_thread::run(&[
        (
            "split_view_uses_two_visible_side_by_side_document_panes",
            split_view_uses_two_visible_side_by_side_document_panes,
        ),
        (
            "split_divider_is_themed_chrome_rather_than_system_grey",
            split_divider_is_themed_chrome_rather_than_system_grey,
        ),
        (
            "status_bar_is_off_by_default_and_costs_no_height_when_hidden",
            status_bar_is_off_by_default_and_costs_no_height_when_hidden,
        ),
    ]);
    leave_sandbox();
}
