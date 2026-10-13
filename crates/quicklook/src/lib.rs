//! Upleft's port of Downright's `DownrightQL` target (`Sources/DownrightQL`):
//! the Quick Look preview extension's view controller, the resource policy,
//! and the bounded loader. The extension executable is `src/main.rs`.
//!
//! One module per Swift file, same names in snake_case.

pub mod preview_view_controller;
pub mod quick_look_loader;
pub mod quick_look_policy;

// Sidestep supplies the runtime and frameworks objc2 binds, on Linux.
#[cfg(target_os = "linux")]
use sidestep as _;
