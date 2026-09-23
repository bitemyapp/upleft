//! The DownrightThumb extension executable: Downright's generated
//! `main.swift` (Scripts/bundle-quicklook.sh).
//!
//! `NSExtensionMain` reads `NSExtension` from the Info.plist, instantiates
//! `NSExtensionPrincipalClass` (`ThumbnailProvider`), and services the XPC
//! connection. It never returns. See `upleft-quicklook`'s `main.rs` for why
//! the C `main` is ours.

#![no_main]

use std::ffi::{c_char, c_int};

// Foundation exports `NSExtensionMain`; no public header declares it.
#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
    fn NSExtensionMain(argc: c_int, argv: *mut *mut c_char) -> c_int;
}

#[unsafe(no_mangle)]
pub extern "C" fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    // Register the principal class before NSExtensionMain looks it up by
    // name: objc2 registers a `define_class!` class on first use.
    let _ = <upleft_thumb::thumbnail_provider::ThumbnailProvider as objc2::ClassType>::class();
    // SAFETY: `argc`/`argv` are the process's own arguments.
    unsafe { NSExtensionMain(argc, argv) }
}
