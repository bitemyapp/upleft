//! Port of `Tests/DownrightAppTests/CommandPaletteNavigationRegressionTests.swift`,
//! the case that exercises `+Actions`' `openInPlace` directly.
//!
//! Skipped, with the reason:
//! - `failedWorkspaceOpenDoesNotSelectDestinationRangeInPreviousDocument`,
//!   `successfulWorkspaceOpenSelectsHeadingInDestination(viaSymlink:)`: they
//!   drive `commandPalette(_:didChoose:)` (`+CommandPalette`, another branch)
//!   with a `CommandPaletteView`; they belong with that port.
//!
//! The Swift suite is `@MainActor`: this binary owns the main thread
//! (`harness = false`). The window is never ordered in (`controller_support`).

mod controller_support;

use controller_support::{Closing, Removing, new_controller, temporary_directory};
use upleft_render::render_contracts::RenderMode;

fn open_in_place_reports_whether_the_intended_destination_opened() {
    let root = temporary_directory("upleft-open-in-place");
    let _remove = Removing(root.clone());
    let current = root.appending_path_component("current.md");
    let destination = root.appending_path_component("destination.md");
    std::fs::write(current.path(), "# Current\n").unwrap();
    std::fs::write(destination.path(), "# Destination\n").unwrap();
    let controller = Closing(new_controller());
    controller.open(&current, RenderMode::Source).unwrap();

    assert!(!controller.open_in_place(&root.appending_path_component("missing.md")));
    assert_eq!(controller.markdown_document().url(), Some(current.resolving_symlinks_in_path()));
    assert!(controller.open_in_place(&destination));
    assert_eq!(controller.markdown_document().url(), Some(destination.resolving_symlinks_in_path()));
}

fn main() {
    controller_support::prepare();
    controller_support::main_thread::run(&[(
        "open_in_place_reports_whether_the_intended_destination_opened",
        open_in_place_reports_whether_the_intended_destination_opened,
    )]);
    controller_support::finish();
}
