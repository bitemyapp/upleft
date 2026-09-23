//! Upleft's port of the non-view parts of Downright's `DownrightQL` target
//! (`Sources/DownrightQL`): the Quick Look resource policy, the bounded
//! loader, and the pure helpers of `PreviewViewController`. The view
//! controller itself (TextKit, the density gutter, the open-in-app bar) is
//! ported with the UI.
//!
//! One module per Swift file, same names in snake_case.

pub mod preview_view_controller;
pub mod quick_look_loader;
pub mod quick_look_policy;
