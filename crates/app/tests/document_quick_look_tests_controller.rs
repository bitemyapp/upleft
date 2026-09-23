//! Port of `theCommandFollowsTheCaretAndReportsWhenThereIsNothingToShow` from
//! `Tests/DownrightAppTests/DocumentQuickLookTests.swift`: the Quick Look
//! command's availability in `commandContext` (`+Commands`) and
//! `quickLookAtSelection()` on a real window.
//!
//! `document_quick_look_tests_commands.rs` and `…_menu.rs` hold the
//! controller-free cases; the request tests belong to the panels port.
//!
//! The Swift test is `@MainActor`: this binary owns the main thread
//! (`harness = false`). No window is ordered in (`controller_support`); the
//! Swift test never orders its window in either.

mod controller_support;

use controller_support::{Closing, Removing, make_controller, range_of, temporary_directory};
use upleft_app::panels::document_quick_look::QuickLookHost;
use upleft_core::NSRange;
use upleft_render::render_contracts::RenderMode;

fn the_command_follows_the_caret_and_reports_when_there_is_nothing_to_show() {
    let directory = temporary_directory("upleft-quick-look");
    let _remove = Removing(directory.clone());
    let url = directory.appending_path_component("notes.md");
    let source = "See [the design](design.md) here.\n";
    let controller = Closing(make_controller(source, &url, RenderMode::Live));

    controller.container_text_view().set_source_selected_ranges(&[NSRange::new(range_of(source, "here").location, 0)]);
    assert!(!QuickLookHost::has_quick_look_target(&*controller));
    assert!(!controller.command_context().has_quick_look_target);
    // Reports failure rather than swallowing the key, so a trigger that
    // resolves to nothing can still fall through to what it would otherwise
    // have done.
    assert!(!QuickLookHost::quick_look_at_selection(&*controller));

    controller
        .container_text_view()
        .set_source_selected_ranges(&[NSRange::new(range_of(source, "the design").location + 2, 0)]);
    assert!(QuickLookHost::has_quick_look_target(&*controller));
    assert!(controller.command_context().has_quick_look_target);
}

fn main() {
    controller_support::prepare();
    controller_support::main_thread::run(&[(
        "the_command_follows_the_caret_and_reports_when_there_is_nothing_to_show",
        the_command_follows_the_caret_and_reports_when_there_is_nothing_to_show,
    )]);
    controller_support::finish();
}
