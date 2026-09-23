//! Port of `Tests/DownrightAppTests/CommandPaletteNavigationRegressionTests.swift`:
//! `openInPlace` (`+Actions`) directly, and the palette's `openAt` result
//! (`commandPalette(_:didChoose:)`, `+CommandPalette`).
//!
//! `successfulWorkspaceOpenSelectsHeadingInDestination(viaSymlink:)` is a
//! parameterised Swift test; its two arguments are two tests here.
//!
//! Swift's `CommandPaletteView()` reads its recents from
//! `UserDefaults.standard`; the palette here gets an in-memory recents store
//! (the controller never reads the palette it is handed).
//!
//! The Swift suite is `@MainActor`: this binary owns the main thread
//! (`harness = false`). The window is never ordered in (`controller_support`).

mod controller_support;

use std::cell::RefCell;
use std::rc::Rc;

use controller_support::{Closing, Removing, mtm, new_controller, temporary_directory};
use objc2::rc::Retained;
use upleft_app::panels::command_palette_view::CommandPaletteView;
use upleft_app::support::command_palette_model::CommandPaletteRecentStore;
use upleft_app::support::commands::Command;
use upleft_app::support::quick_open_providers::{QuickOpenAction, QuickOpenProviderKind, QuickOpenResult};
use upleft_core::NSRange;
use upleft_render::render_contracts::RenderMode;
use upleft_render::theme::style_sheet::StyleSheet;

/// Recents that live only in memory.
#[derive(Default)]
struct InMemoryRecents(RefCell<Vec<Command>>);

impl CommandPaletteRecentStore for InMemoryRecents {
    fn recent_commands(&self) -> Vec<Command> {
        self.0.borrow().clone()
    }

    fn record(&self, command: Command) {
        self.0.borrow_mut().insert(0, command);
    }
}

/// `CommandPaletteView()`, with in-memory recents.
fn palette() -> Retained<CommandPaletteView> {
    CommandPaletteView::new(
        Rc::new(StyleSheet::current(mtm())),
        Rc::new(InMemoryRecents::default()),
        None,
        Vec::new(),
        mtm(),
    )
}

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

fn failed_workspace_open_does_not_select_destination_range_in_previous_document() {
    let root = temporary_directory("upleft-palette-open");
    let _remove = Removing(root.clone());
    let current = root.appending_path_component("current.md");
    std::fs::write(current.path(), "# Current\n\nThe original document remains open.\n").unwrap();
    let controller = Closing(new_controller());
    controller.open(&current, RenderMode::Source).unwrap();
    let destination_range = NSRange::new(15, 5);
    let result = QuickOpenResult::new(
        "missing-heading",
        QuickOpenProviderKind::Symbol,
        "Missing",
        QuickOpenAction::OpenAt(root.appending_path_component("missing.md"), destination_range),
    );
    controller.command_palette_did_choose(&palette(), &result);
    assert_eq!(controller.markdown_document().url(), Some(current.resolving_symlinks_in_path()));
    assert_ne!(controller.container_text_view().source_selected_range(), destination_range);
}

/// `successfulWorkspaceOpenSelectsHeadingInDestination(viaSymlink:)`.
fn successful_workspace_open_selects_heading_in_destination(via_symlink: bool) {
    let root = temporary_directory("upleft-palette-open");
    let _remove = Removing(root.clone());
    let current = root.appending_path_component("current.md");
    let destination = root.appending_path_component("destination.md");
    std::fs::write(current.path(), "# Current\n").unwrap();
    std::fs::write(destination.path(), "# Destination\n\n## Target\n").unwrap();
    let controller = Closing(new_controller());
    controller.open(&current, RenderMode::Source).unwrap();
    let range = NSRange::new(15, 9);
    let target = if via_symlink { root.appending_path_component("alias.md") } else { destination.clone() };
    if via_symlink {
        std::os::unix::fs::symlink(destination.path(), target.path()).unwrap();
    }
    let result =
        QuickOpenResult::new("heading", QuickOpenProviderKind::Symbol, "Target", QuickOpenAction::OpenAt(target, range));
    controller.command_palette_did_choose(&palette(), &result);
    assert_eq!(controller.markdown_document().url(), Some(destination.resolving_symlinks_in_path()));
    assert_eq!(controller.container_text_view().source_selected_range(), range);
}

fn successful_workspace_open_selects_heading_in_destination_via_symlink_false() {
    successful_workspace_open_selects_heading_in_destination(false);
}

fn successful_workspace_open_selects_heading_in_destination_via_symlink_true() {
    successful_workspace_open_selects_heading_in_destination(true);
}

fn main() {
    controller_support::prepare();
    controller_support::main_thread::run(&[
        (
            "open_in_place_reports_whether_the_intended_destination_opened",
            open_in_place_reports_whether_the_intended_destination_opened,
        ),
        (
            "failed_workspace_open_does_not_select_destination_range_in_previous_document",
            failed_workspace_open_does_not_select_destination_range_in_previous_document,
        ),
        (
            "successful_workspace_open_selects_heading_in_destination_via_symlink_false",
            successful_workspace_open_selects_heading_in_destination_via_symlink_false,
        ),
        (
            "successful_workspace_open_selects_heading_in_destination_via_symlink_true",
            successful_workspace_open_selects_heading_in_destination_via_symlink_true,
        ),
    ]);
    controller_support::finish();
}
