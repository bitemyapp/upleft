//! Main-thread checks for the Objective-C classes this port defines
//! (`SpeechCoordinator`, `DownrightServicesProvider`): they register with the
//! runtime, and their entry points behave as the Swift ones do. No Swift test
//! covers them; nothing here speaks aloud.
//!
//! `harness = false`: `main` runs on the process's main thread, which
//! `MainThreadMarker` requires.

use std::cell::RefCell;
use std::rc::Rc;

use objc2::MainThreadMarker;
use objc2::msg_send;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;
use upleft_app::integrations::native_integration::{
    DownrightServicesProvider, IntegrationRegistry, ServiceInputResolver,
};
use upleft_app::support::speech_coordinator::SpeechCoordinator;
use upleft_foundation::url::FileUrl;

fn speech(mtm: MainThreadMarker) {
    let coordinator = SpeechCoordinator::new(mtm);
    assert_eq!(objc2::runtime::AnyObject::class(&coordinator).name().to_str().unwrap(), "SpeechCoordinator");
    assert!(!coordinator.is_speaking(mtm));
    // Whitespace is never spoken, and stopping while idle is a no-op.
    assert!(!coordinator.speak(" \n\t ", mtm));
    assert!(!coordinator.is_speaking(mtm));
    coordinator.stop(mtm);
    assert!(!coordinator.is_speaking(mtm));
}

fn open_markdown(provider: &DownrightServicesProvider, pasteboard: &NSPasteboard) -> Option<String> {
    objc2::rc::autoreleasepool(|_| {
        let mut error: *mut NSString = std::ptr::null_mut();
        let _: () = unsafe {
            msg_send![provider, openMarkdownInDownright: pasteboard, userData: None::<&NSString>, error: &mut error]
        };
        unsafe { error.as_ref() }.map(|message| message.to_string())
    })
}

fn services(mtm: MainThreadMarker) {
    let provider = DownrightServicesProvider::new(mtm);
    assert_eq!(
        objc2::runtime::AnyObject::class(&provider).name().to_str().unwrap(),
        "DownrightServicesProvider"
    );
    let pasteboard = NSPasteboard::pasteboardWithUniqueName();
    pasteboard.clearContents();
    assert_eq!(open_markdown(&provider, &pasteboard).as_deref(), Some("The selection does not contain a Markdown file."));

    let temporary = objc2_foundation::NSTemporaryDirectory().to_string();
    let file = FileUrl::from_path(&temporary)
        .appending_path_component(&format!("services-{}.md", objc2_foundation::NSUUID::new().UUIDString()));
    std::fs::write(file.path(), "# Services\n").unwrap();
    pasteboard.clearContents();
    let text = NSString::from_str(&format!("\n  {}  \n/not/a/file.md\n/tmp/readme.txt\n", file.path()));
    pasteboard.setString_forType(&text, unsafe { NSPasteboardTypeString });
    assert_eq!(ServiceInputResolver::urls(&pasteboard), vec![file.standardized_file_url()]);

    let registry = IntegrationRegistry::shared(mtm);
    registry.set_open_handler(None);
    assert_eq!(open_markdown(&provider, &pasteboard).as_deref(), Some("Downright is not ready to open this file."));

    let routed: Rc<RefCell<Vec<FileUrl>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&routed);
    registry.set_open_handler(Some(Rc::new(move |url: &FileUrl| sink.borrow_mut().push(url.clone()))));
    assert_eq!(open_markdown(&provider, &pasteboard), None);
    assert_eq!(*routed.borrow(), vec![file.standardized_file_url()]);

    registry.set_open_handler(None);
    let _: () = unsafe { msg_send![&*pasteboard, releaseGlobally] };
    let _ = std::fs::remove_file(file.path());
}

fn main() {
    let mtm = MainThreadMarker::new().expect("a harness = false test runs on the main thread");
    speech(mtm);
    services(mtm);
    println!("main_thread_objects: speech coordinator and services provider ok");
}
