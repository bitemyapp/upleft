//! Port of the window half of `Tests/DownrightAppTests/AutosaveLifecycleTests.swift`
//! ("Autosave and close lifetime", `.serialized`): the window's occlusion
//! path is part of the autosave feature rather than an unconditional writer.
//! The document half is `autosave_lifecycle_tests.rs`.
//!
//! This binary owns the main thread (`harness = false`, `tests/main_thread`),
//! runs in a sandbox (`tests/document_window_support`), and orders no window
//! in: `windowDidChangeOcclusionState:` is called directly, as in Swift, and
//! a window that was never shown is not visible.
//!
//! Skipped, with the reason: `pathTokenActionsStayInertForMissingPaths`
//! calls `openPathTokenInEditor`/`revealPathTokenInFinder` (`+Delegates`,
//! ported separately).

mod document_window_support;
mod main_thread;

use document_window_support::{Fixture, enter_sandbox, leave_sandbox, mtm};
use objc2_app_kit::NSWindowDidChangeOcclusionStateNotification;
use objc2_foundation::NSNotification;
use upleft_app::app::document_window_controller::DocumentWindowController;
use upleft_app::support::preferences::Preferences;
use upleft_core::NSRange;
use upleft_render::render_contracts::RenderMode;

fn occlusion_save_belongs_to_the_autosave_setting() {
    let fixture = Fixture::new("downright-autosave-lifetime");
    let url = fixture.write("note.md", "before\n");
    let original = Preferences::shared().values().autosave_enabled;

    let controller = DocumentWindowController::new(mtm());
    controller.open(&url, RenderMode::Live).expect("open the fixture");
    let document = controller.markdown_document().clone();
    let length = document.storage().length() as isize;
    assert!(document.replace(NSRange { location: 0, length }, "after\n", Some("Replace")));

    let notification = |controller: &DocumentWindowController| {
        let window = controller.window();
        unsafe {
            NSNotification::notificationWithName_object(
                NSWindowDidChangeOcclusionStateNotification,
                window.as_deref().map(|window| window as &objc2::runtime::AnyObject),
            )
        }
    };

    // Default (autosave off): covering or miniaturizing the window must not
    // commit the buffer — an agent may be editing the same file.
    Preferences::shared().update(|values| values.autosave_enabled = false);
    controller.window_did_change_occlusion_state(&notification(&controller));
    assert_eq!(fixture.read("note.md"), "before\n");
    assert!(document.is_dirty());

    // With autosave enabled the occlusion save still works.
    Preferences::shared().update(|values| values.autosave_enabled = true);
    controller.window_did_change_occlusion_state(&notification(&controller));
    assert_eq!(fixture.read("note.md"), "after\n");
    assert!(!document.is_dirty());

    controller.close();
    Preferences::shared().update(|values| values.autosave_enabled = original);
}

fn main() {
    enter_sandbox();
    main_thread::run(&[(
        "occlusion_save_belongs_to_the_autosave_setting",
        occlusion_save_belongs_to_the_autosave_setting,
    )]);
    leave_sandbox();
}
