//! The Upleft application: port of `Sources/DownrightApp/main.swift`.
//!
//! SwiftPM builds Downright's app target as a plain executable, so there is
//! no Info.plist telling AppKit it is an app until the bundle script wraps it
//! in one; setting the activation policy here makes the binary behave
//! correctly either way. Two start-up steps have no line in main.swift:
//! installing the Mermaid renderer the render layer reaches through a hook
//! (upleft-render's view PORTING notes), and installing the Sparkle bridge
//! (Swift links Sparkle's classes directly).
//!
//! `UPLEFT_HEADLESS_SMOKE=<file.md>` mirrors nothing in Swift: a launch
//! check for tests and `just upleft-app` that builds the app's objects the
//! way a launch does, never activates, never orders a window in, prints a
//! JSON summary and exits (`smoke`). See docs/KNOWN-DIFFERENCES.md.

use objc2::MainThreadMarker;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use upleft_app::app::app_delegate::AppDelegate;

mod smoke;

fn main() {
    upleft_mermaid::downright::mermaid_renderer_bridge::install_fragment_renderer();
    let mtm = MainThreadMarker::new().expect("the application runs on the main thread");
    upleft_app::updater::sparkle::install(mtm);

    if let Some(document) = std::env::var_os("UPLEFT_HEADLESS_SMOKE") {
        std::process::exit(smoke::run(std::path::Path::new(&document), mtm));
    }

    let application = NSApplication::sharedApplication(mtm);
    application.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let delegate = AppDelegate::new(mtm);
    application.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    #[allow(deprecated)]
    application.activateIgnoringOtherApps(true);
    application.run();
}
