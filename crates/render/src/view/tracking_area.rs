//! Port of `View/TrackingArea.swift`: `NSView.refreshTrackingArea(_:options:)`.

use std::cell::RefCell;

use objc2::AllocAnyThread;
use objc2::rc::Retained;
use objc2_app_kit::{NSTrackingArea, NSTrackingAreaOptions, NSView};

/// Swaps `area` for a fresh one covering the view's current bounds, owned by
/// the view. `bounds` moves on every resize and a stale area keeps reporting
/// the old rect, so every hovering view rebuilds one the same way.
pub fn refresh_tracking_area(view: &NSView, area: &RefCell<Option<Retained<NSTrackingArea>>>, options: NSTrackingAreaOptions) {
    if let Some(existing) = area.borrow_mut().take() {
        view.removeTrackingArea(&existing);
    }
    // SAFETY: the owner is the view itself, which outlives its own area.
    let replacement = unsafe {
        NSTrackingArea::initWithRect_options_owner_userInfo(
            NSTrackingArea::alloc(),
            view.bounds(),
            options,
            Some(view),
            None,
        )
    };
    view.addTrackingArea(&replacement);
    *area.borrow_mut() = Some(replacement);
}
