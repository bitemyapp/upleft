//! Port of `Tests/DownrightAppTests/ChangeReviewTests.swift`, its "Expiry"
//! section (the change tracker on its own).
//!
//! Not ported: the counting, headline, position, distribution, accessibility
//! and bar tests (`ChangeSummaryBarView`, `ConflictBarView`: AppKit views,
//! ported with the UI) and `markReviewedIsReachable` (the command table,
//! ported by the palette agent).

use std::cell::Cell;
use std::rc::Rc;

use upleft_app::ai::change_tracker::{ChangeTracker, PersistedMark, PersistedRange};
use upleft_core::contracts::{ChangeKind, Uuid};
use upleft_core::ns_range::NSRange;
use upleft_foundation::date::Date;

fn tracker(ages: &[f64]) -> ChangeTracker {
    let tracker = ChangeTracker::new();
    let now = Date::now();
    let persisted: Vec<PersistedMark> = ages
        .iter()
        .enumerate()
        .map(|(index, age)| PersistedMark {
            id: Uuid::new_v4(),
            kind: ChangeKind::Modified.raw_value().to_owned(),
            range: PersistedRange::new(NSRange::new(index as isize * 20, 10)),
            word_ranges: Vec::new(),
            deleted_text: String::new(),
            created: now.adding(-age),
            visited: false,
        })
        .collect();
    tracker.restore(&persisted, 1000, now);
    tracker
}

#[test]
fn expired_marks_are_not_decorated() {
    let tracker = tracker(&[30.0, 10_000.0]);
    assert_eq!(tracker.count(), 1, "an expired mark never comes back from disk");
    let fresh = ChangeTracker::new();
    fresh.apply(&[], "", "", true);
    assert!(fresh.decorated_marks().is_empty());
}

#[test]
fn restore_drops_expired_marks() {
    assert_eq!(tracker(&[1.0, 2.0, 3.0]).count(), 3);
    assert_eq!(tracker(&[1.0, 10_000.0, 3.0]).count(), 2);
    assert_eq!(tracker(&[10_000.0, 20_000.0]).count(), 0);
}

#[test]
fn expiry_does_not_advance_the_baseline() {
    let tracker = tracker(&[30.0]);
    let reviewed = Rc::new(Cell::new(0));
    let changed = Rc::new(Cell::new(0));
    let reviewed_counter = Rc::clone(&reviewed);
    let changed_counter = Rc::clone(&changed);
    tracker.set_on_reviewed(Some(Box::new(move || reviewed_counter.set(reviewed_counter.get() + 1))));
    tracker.set_on_change(Some(Box::new(move || changed_counter.set(changed_counter.get() + 1))));

    assert!(!tracker.drop_expired_marks(Date::now()));
    assert_eq!(changed.get(), 0);

    assert!(tracker.drop_expired_marks(Date::now().adding(tracker.lifetime() + 1.0)));
    assert!(tracker.is_empty());
    assert_eq!(changed.get(), 1);
    assert_eq!(reviewed.get(), 0, "a mark ageing out is not a review");

    tracker.clear();
    assert_eq!(reviewed.get(), 1, "explicitly finishing review is");
}

#[test]
fn navigation_skips_expired_marks() {
    assert!(tracker(&[10_000.0]).next(0).is_none());
    assert!(tracker(&[10_000.0]).previous(500).is_none());
    assert!(tracker(&[30.0]).next(0).is_some());
}
