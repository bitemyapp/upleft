//! Upleft's port of Downright's `DownrightThumb` target
//! (`Sources/DownrightThumb`): the Quick Look thumbnail extension. The
//! extension executable is `src/main.rs`.
//!
//! One module per Swift file, same names in snake_case.

pub mod thumbnail_provider;

// Sidestep supplies the runtime and frameworks objc2 binds, on Linux.
#[cfg(target_os = "linux")]
use sidestep as _;
