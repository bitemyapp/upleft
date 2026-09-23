//! The DownrightQL extension executable: Downright's generated `main.swift`
//! (Scripts/bundle-quicklook.sh).
//!
//! An `.appex` is an ordinary bundle whose executable hands control to
//! `NSExtensionMain`. The extension host launches the binary and
//! `NSExtensionMain` takes over: it reads `NSExtension` from the Info.plist,
//! instantiates `NSExtensionPrincipalClass` (`PreviewViewController`), and
//! services the XPC connection. It never returns.
//!
//! `no_main`: the C `main` is ours, so the process starts exactly as the
//! Swift one does (no Rust runtime set-up such as ignoring SIGPIPE), and
//! `argc`/`argv` are passed through (Swift's `@_silgen_name` declaration
//! calls it without them).

#![no_main]

use std::ffi::{c_char, c_int};

// Foundation exports `NSExtensionMain`; no public header declares it.
#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
    fn NSExtensionMain(argc: c_int, argv: *mut *mut c_char) -> c_int;
}

#[unsafe(no_mangle)]
pub extern "C" fn main(argc: c_int, argv: *mut *mut c_char) -> c_int {
    // The render layer reaches Mermaid through a hook (upleft-mermaid depends
    // on upleft-render); every host installs it once at start-up.
    upleft_mermaid::downright::mermaid_renderer_bridge::install_fragment_renderer();
    // Register the principal class before NSExtensionMain looks it up by
    // name: objc2 registers a `define_class!` class on first use.
    let _ = <upleft_quicklook::preview_view_controller::PreviewViewController as objc2::ClassType>::class();
    // SAFETY: `argc`/`argv` are the process's own arguments.
    unsafe { NSExtensionMain(argc, argv) }
}
