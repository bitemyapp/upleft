//! Port of `markReviewedIsReachable` from
//! `Tests/DownrightAppTests/ChangeReviewTests.swift` (the rest of that file
//! belongs to other ports).

use upleft_app::support::commands::{Command, CommandContext, Menu};
use upleft_app::support::keybindings::KeybindingDefaults;

/// Mark Changes Reviewed is a first-class command.
#[test]
fn mark_reviewed_is_reachable() {
    assert_eq!(Command::MarkChangesReviewed.title(), "Mark Changes Reviewed");
    assert_eq!(Command::MarkChangesReviewed.menu(), Menu::Navigate);
    assert!(KeybindingDefaults::table().get(&Command::MarkChangesReviewed).is_some_and(|b| !b.is_empty()));

    // Enabled only when there is something to retire.
    let with_marks = CommandContext { has_document: true, has_change_marks: true, ..CommandContext::default() };
    let without = CommandContext { has_document: true, has_change_marks: false, ..CommandContext::default() };
    assert!(Command::MarkChangesReviewed.is_enabled(&with_marks));
    assert!(!Command::MarkChangesReviewed.is_enabled(&without));
    assert!(!Command::NextChange.is_enabled(&without));
}
