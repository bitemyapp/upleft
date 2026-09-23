//! Port of `menuAndRingOpenTasks` ("The View command and ring use the same
//! floating entry point") from
//! `Tests/DownrightAppTests/DocumentChromeLayoutTests.swift`: the command a
//! menu item carries reaches `performDownrightCommand(_:)` (`+Commands`), and
//! the task ring's `onActivate`, wired by the toolbar's cluster item
//! (`+Actions`), opens the same floating surface.
//!
//! The rest of that file tests the main controller's floating surface and
//! layout, not these extensions; it belongs with that port.
//!
//! The Swift suite is `@MainActor` and `.serialized`: this binary owns the
//! main thread (`harness = false`).
//!
//! Written but not run: the Tasks panel opens as the floating surface, whose
//! borderless child window (`addChildWindow(_:ordered:)` then `orderFront`)
//! orders the titled document window in too, even parked at
//! (-30000, -30000); titled windows must never reach a display
//! (`controller_support`). It is registered once the floating path can run
//! without ordering its parent in.

mod controller_support;

use controller_support::{Closing, Removing, mtm, new_controller, temporary_directory};
use upleft_app::app::main_menu::MainMenu;
use upleft_app::panels::inspector_host_view::InspectorSection;
use upleft_app::support::commands::Command;
use upleft_foundation::url::FileUrl;
use upleft_render::render_contracts::RenderMode;

fn make_document() -> (FileUrl, Removing) {
    let directory = temporary_directory("upleft-chrome");
    let file = directory.appending_path_component("plan.md");
    std::fs::write(
        file.path(),
        "# A Fairly Long Document Title Here\n\n\
         ## An Equally Long Second Level Heading\n\n\
         ### And A Third Level Heading To Fill The Trail\n\n\
         - [ ] First open task\n\
         - [ ] Second open task\n\
         - [x] A finished one\n\n\
         Prose under the deepest heading so the breadcrumb has a full trail.",
    )
    .unwrap();
    (file, Removing(directory))
}

#[allow(dead_code)]
fn menu_and_ring_open_tasks() {
    let (file, _cleanup) = make_document();
    let controller = Closing(new_controller());
    controller.open(&file, RenderMode::Live).unwrap();
    if let Some(window) = controller.window() {
        window.layoutIfNeeded();
    }

    let menu_item = MainMenu::command_item(Command::TaskPanel, mtm());
    // SAFETY: the controller answers `performDownrightCommand:`.
    unsafe { menu_item.setTarget(Some(&controller.0)) };
    controller.perform_downright_command(Some(&menu_item));
    controller_support::assert_document_window_never_shown(&controller);
    assert!(controller.floating_surface().is_some());
    controller.close_task_panel();

    if let Some(on_activate) = controller.progress_ring().on_activate() {
        on_activate();
    }
    assert!(controller.floating_surface().is_some());
    assert_eq!(controller.inspector_host().and_then(|host| host.selected_section()), Some(InspectorSection::Tasks));
}

fn main() {
    controller_support::prepare();
    // `menu_and_ring_open_tasks` is not registered; see the module notes.
    controller_support::main_thread::run(&[]);
    controller_support::finish();
}
