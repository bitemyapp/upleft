//! `UPLEFT_HEADLESS_SMOKE=<file.md>`: a launch check with nothing on screen.
//! Mirrors nothing in Swift (docs/KNOWN-DIFFERENCES.md): it builds what a
//! launch builds — the main menu, the updater bridge, a document window over
//! the given file — without activating the app or ordering any window in,
//! prints a JSON summary on stdout, and returns the exit status.

use std::path::Path;

use objc2::MainThreadMarker;
use objc2::runtime::AnyClass;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::NSBundle;
use upleft_app::app::app_delegate::WelcomeDocument;
use upleft_app::app::document_window_controller::DocumentWindowController;
use upleft_app::app::main_menu::MainMenu;
use upleft_foundation::url::FileUrl;
use upleft_render::render_contracts::RenderMode;

pub fn run(document: &Path, mtm: MainThreadMarker) -> i32 {
    let application = NSApplication::sharedApplication(mtm);
    // Never `.regular`, never activated: nothing takes focus.
    application.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    let menu = MainMenu::build(mtm);
    application.setMainMenu(Some(&menu));

    let controller = DocumentWindowController::new(mtm);
    let url = FileUrl::from_path(&document.to_string_lossy());
    let opened = controller.open(&url, RenderMode::Live);
    let (title, content) = match controller.window() {
        Some(window) => {
            // Laid out, never ordered in.
            if let Some(content) = window.contentView() {
                content.layoutSubtreeIfNeeded();
            }
            (window.title().to_string(), window.contentView().map(|view| view.frame().size))
        }
        None => (String::new(), None),
    };
    let bundle = NSBundle::mainBundle();
    let summary = serde_json::json!({
        "bundleIdentifier": bundle.bundleIdentifier().map(|id| id.to_string()),
        "sparkleLoaded": AnyClass::get(c"SPUUpdater").is_some(),
        "mathFonts": upleft_math::math_bundle::math_resource_bundle::math_fonts_directory()
            .map(|path| path.to_string_lossy().into_owned()),
        "welcomeAvailable": WelcomeDocument::is_available(),
        "menuItems": menu.numberOfItems(),
        "documentOpened": opened.is_ok(),
        "documentCharacters": controller.markdown_document().storage().length(),
        "windowTitle": title,
        "contentSize": content.map(|size| [size.width, size.height]),
    });
    println!("{summary}");
    let _ = controller.document_will_close();
    if opened.is_ok() { 0 } else { 1 }
}
