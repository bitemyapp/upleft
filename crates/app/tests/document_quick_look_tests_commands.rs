//! Port of `quickLookIsOfferedOnlyWithATargetUnderTheCaret` from
//! `Tests/DownrightAppTests/DocumentQuickLookTests.swift` (the rest of that
//! file needs a window or belongs to other ports).

use upleft_app::support::commands::{Command, CommandContext};

/// Enabled only when there is something to preview, so the menu never offers
/// a command that would be a no-op.
#[test]
fn quick_look_is_offered_only_with_a_target_under_the_caret() {
    assert!(!Command::QuickLook.is_enabled(&CommandContext { has_document: true, ..CommandContext::default() }));
    assert!(Command::QuickLook.is_enabled(&CommandContext {
        has_document: true,
        has_quick_look_target: true,
        ..CommandContext::default()
    }));
    assert!(!Command::QuickLook.is_enabled(&CommandContext {
        has_document: false,
        has_quick_look_target: true,
        ..CommandContext::default()
    }));
}
