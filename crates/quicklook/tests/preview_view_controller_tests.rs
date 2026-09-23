//! Tests for `PreviewViewController` (Downright's DownrightQLTests cover only
//! the policy; see `quick_look_policy_tests.rs`). They drive the controller
//! through its Objective-C `preparePreviewOfFileAtURL:completionHandler:`, as
//! Quick Look does, and run on the main thread, pumping the main run loop
//! while the load runs on its background queue.
//!
//! Windowless in the sense the harness rules require: the controller's view
//! sits in a borderless window at (-30000, -30000) that is never ordered in.
//! Nothing is registered with Quick Look and no extension is launched.

mod main_thread;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyProtocol, NSObject, NSObjectProtocol};
use objc2::{ClassType, MainThreadMarker, MainThreadOnly, msg_send};
use objc2_app_kit::{
    NSBackingStoreType, NSStackView, NSTextField, NSView, NSVisualEffectView, NSWindow, NSWindowStyleMask,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{NSCocoaErrorDomain, NSError, NSString, NSURL};
use upleft_core::ParseOptions;
use upleft_core::parser::MarkdownParser;
use upleft_quicklook::preview_view_controller::PreviewViewController;
use upleft_quicklook::quick_look_policy::QuickLookPolicy;
use upleft_render::render_contracts::RenderMode;

use main_thread::{pump_until, sleep_pumping};

fn mtm() -> MainThreadMarker {
    MainThreadMarker::new().expect("main thread")
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> CGRect {
    CGRect::new(CGPoint::new(x, y), CGSize::new(width, height))
}

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> TemporaryDirectory {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "upleft-ql-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        TemporaryDirectory(path)
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A borderless window outside every screen, never ordered in, holding the
/// controller's view as Quick Look's panel would.
fn host(controller: &PreviewViewController, width: f64, height: f64) -> Retained<NSWindow> {
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm()),
            rect(-30000.0, -30000.0, width, height),
            NSWindowStyleMask::Borderless,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setContentView(Some(&controller.view()));
    window
}

type Completion = Rc<RefCell<Option<Option<(String, isize)>>>>;

/// Calls `preparePreviewOfFileAtURL:completionHandler:` and returns where
/// the handler's outcome lands: `Some(None)` for success, `Some(Some((domain,
/// code)))` for an error.
fn prepare(controller: &PreviewViewController, path: &Path) -> Completion {
    let completion: Completion = Rc::new(RefCell::new(None));
    let sink = completion.clone();
    let handler = RcBlock::new(move |error: *mut NSError| {
        let outcome = unsafe { error.as_ref() }.map(|error| (error.domain().to_string(), error.code()));
        *sink.borrow_mut() = Some(outcome);
    });
    let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().unwrap()));
    let handler_ref: &DynBlock<dyn Fn(*mut NSError)> = &handler;
    let _: () = unsafe { msg_send![controller, preparePreviewOfFileAtURL: &*url, completionHandler: handler_ref] };
    completion
}

fn wait(completion: &Completion) -> Option<(String, isize)> {
    assert!(pump_until(|| completion.borrow().is_some(), Duration::from_secs(20)), "the handler is called");
    completion.borrow().clone().unwrap()
}

fn cocoa(code: isize) -> Option<(String, isize)> {
    Some((unsafe { NSCocoaErrorDomain }.to_string(), code))
}

/// Every `NSTextField` string under `view`.
fn labels(view: &NSView) -> Vec<String> {
    let mut out = Vec::new();
    for subview in view.subviews().iter() {
        if let Some(field) = subview.downcast_ref::<NSTextField>() {
            out.push(field.stringValue().to_string());
        }
        out.extend(labels(&subview));
    }
    out
}

const SAMPLE: &str = "# Title\n\nThe opening paragraph has a few words in it.\n\n## Second\n\nMore text here.\n\n### Third\n\n- [ ] task\n";

// MARK: - Tests

fn principal_class_is_registered_under_the_swift_name() {
    let registered = PreviewViewController::class();
    assert_eq!(registered.name(), c"PreviewViewController");
    // NSExtensionPrincipalClass, as scripts/bundle-upleft-quicklook.sh writes it.
    let class = AnyClass::get(c"PreviewViewController").expect("NSClassFromString(\"PreviewViewController\")");
    assert!(std::ptr::eq(class, registered));
    assert_eq!(class.superclass().unwrap().name(), c"NSViewController");
    let protocol = AnyProtocol::get(c"QLPreviewingController").expect("QuickLookUI is linked");
    assert!(class.conforms_to(protocol));
    // Quick Look instantiates the principal class from Objective-C.
    let instance: Retained<NSObject> = unsafe { msg_send![class, new] };
    assert!(instance.isKindOfClass(registered));
    let controller = instance.downcast::<PreviewViewController>().unwrap();
    assert_eq!(controller.view().frame().size, CGSize::new(720.0, 800.0));
}

fn presents_a_small_document_in_read_mode() {
    let directory = TemporaryDirectory::new();
    let path = directory.write("sample.md", SAMPLE);
    let controller = PreviewViewController::new(mtm());
    let window = host(&controller, 720.0, 800.0);
    let completion = prepare(&controller, &path);
    assert!(completion.borrow().is_none(), "the handler runs after the load, not inside the call");
    assert_eq!(wait(&completion), None);

    let container = controller.container_for_testing().expect("a container");
    assert_eq!(controller.storage_for_testing().string().to_string(), SAMPLE);
    assert_eq!(container.text_view().mode(), RenderMode::Read);
    assert!(container.text_view().isSelectable());
    assert!(controller.notice_bar_for_testing().is_none(), "a full preview has no open-in-app bar");
    assert!(controller.fallback_text_view_for_testing().is_none());
    let subviews = controller.view().subviews();
    assert_eq!(subviews.len(), 1);
    assert!(std::ptr::eq(&*subviews.objectAtIndex(0), &**container as &NSView));

    // The density gutter shows at 720pt (≥ 520pt).
    window.layoutIfNeeded();
    let gutter = controller.density_gutter_for_testing().expect("a gutter");
    let accessory = container.leading_accessory().expect("the gutter is the leading accessory");
    assert!(std::ptr::eq(&*accessory, &**gutter as &NSView));
    assert!(gutter.allows_preview_content_overlap());
    let titles: Vec<String> = gutter.outline_entries().iter().map(|entry| entry.title.clone()).collect();
    assert_eq!(titles, ["Title", "Second", "Third"]);
    let summary = gutter.metrics_summary();
    assert!(summary.ends_with(" characters · 1 min read"), "{summary}");
    assert!(summary.contains(" words · "), "{summary}");
    window.close();
}

fn density_gutter_hides_below_the_minimum_width() {
    let directory = TemporaryDirectory::new();
    let path = directory.write("sample.md", SAMPLE);
    let controller = PreviewViewController::new(mtm());
    let window = host(&controller, 720.0, 800.0);
    assert_eq!(wait(&prepare(&controller, &path)), None);
    window.layoutIfNeeded();
    let container = controller.container_for_testing().unwrap();
    assert!(container.leading_accessory().is_some());

    window.setContentSize(CGSize::new(QuickLookPolicy::MINIMUM_DENSITY_GUTTER_WIDTH - 1.0, 800.0));
    window.layoutIfNeeded();
    assert!(container.leading_accessory().is_none(), "no gutter below 520pt");

    window.setContentSize(CGSize::new(QuickLookPolicy::MINIMUM_DENSITY_GUTTER_WIDTH, 800.0));
    window.layoutIfNeeded();
    assert!(container.leading_accessory().is_some(), "the gutter returns at 520pt");
    window.close();
}

fn gutter_preview_names_the_section() {
    let directory = TemporaryDirectory::new();
    let text = "Intro words before any heading.\n\n# Alpha\n\nAlpha body has five words.\n\n# Beta\n\nBeta.\n";
    let path = directory.write("sections.md", text);
    let controller = PreviewViewController::new(mtm());
    let window = host(&controller, 720.0, 800.0);
    assert_eq!(wait(&prepare(&controller, &path)), None);
    let gutter = controller.density_gutter_for_testing().unwrap();
    let delegate = controller.gutter_delegate_for_testing().expect("the controller is the gutter's delegate");

    let start = delegate.density_gutter_preview_at_fraction(&gutter, 0.0).unwrap();
    assert_eq!(start, ("Document start".to_owned(), String::new(), gutter.metrics_summary()));

    let end = delegate.density_gutter_preview_at_fraction(&gutter, 1.0).unwrap();
    assert_eq!(end.0, "Beta");
    assert!(end.2.starts_with("Section 2 of 2"), "{:?}", end.2);

    let document = MarkdownParser::parse(text);
    let alpha = document.headings[0].range.location as f64 / document.length as f64;
    let middle = delegate.density_gutter_preview_at_fraction(&gutter, alpha).unwrap();
    assert_eq!(middle.0, "Alpha");
    assert_eq!(middle.2, format!("Section 1 of 2 · {} words", document.headings[0].word_count));
    window.close();
}

fn large_file_renders_a_bounded_prefix_with_the_open_in_app_bar() {
    let directory = TemporaryDirectory::new();
    // Seventy short blocks, then one fenced block that carries the file past
    // the large-file threshold (cheap to parse in a debug build).
    let mut text = String::new();
    for index in 0..70 {
        text.push_str(&format!("## Section {index}\n\nParagraph {index} with some words.\n\n"));
    }
    text.push_str("```\n");
    let line = "let filler = \"a line of code that pads the file\";\n";
    while text.len() as isize <= QuickLookPolicy::LARGE_FILE_THRESHOLD_BYTES {
        text.push_str(line);
    }
    text.push_str("```\n");
    let path = directory.write("large.md", &text);
    let controller = PreviewViewController::new(mtm());
    let window = host(&controller, 720.0, 800.0);
    assert_eq!(wait(&prepare(&controller, &path)), None);

    // `presentTruncated`: the first 60 top-level blocks of the structure-only
    // parse, bounded by the render caps.
    let head = &text[..QuickLookPolicy::PREFIX_READ_LIMIT_BYTES.min(text.len() as isize) as usize];
    let structure = MarkdownParser::parse_with(head, ParseOptions::STRUCTURE_ONLY);
    let cutoff = structure.root.children[QuickLookPolicy::PREFIX_BLOCK_COUNT as usize - 1].range.upper_bound();
    let expected = PreviewViewController::bounded_prefix(
        head,
        cutoff.min(QuickLookPolicy::PREFIX_RENDER_LIMIT_UTF16),
        QuickLookPolicy::PREFIX_RENDER_LIMIT_BYTES,
    );
    assert_eq!(controller.storage_for_testing().string().to_string(), expected);
    assert_eq!(MarkdownParser::parse(&expected).root.children.len(), 60);

    let bar = controller.notice_bar_for_testing().expect("the open-in-app bar");
    assert!(bar.downcast_ref::<NSVisualEffectView>().is_some());
    assert!(bar.subviews().objectAtIndex(0).downcast_ref::<NSStackView>().is_some());
    assert_eq!(labels(&bar), ["Showing the first 60 blocks"]);
    let buttons: Vec<String> = bar
        .subviews()
        .objectAtIndex(0)
        .subviews()
        .iter()
        .filter_map(|view| view.downcast::<objc2_app_kit::NSButton>().ok().map(|button| button.title().to_string()))
        .collect();
    assert_eq!(buttons, ["Open in Upleft"]);
    window.layoutIfNeeded();
    let container = controller.container_for_testing().unwrap();
    assert!(
        (container.frame().origin.y - bar.frame().size.height).abs() < 0.5 || container.frame().size.height < 800.0,
        "the content stops above the bar"
    );
    window.close();
}

fn missing_file_fails_with_a_corrupt_file_error() {
    let directory = TemporaryDirectory::new();
    let controller = PreviewViewController::new(mtm());
    let window = host(&controller, 720.0, 800.0);
    assert_eq!(wait(&prepare(&controller, &directory.0.join("absent.md"))), cocoa(259));
    assert!(controller.container_for_testing().is_none());
    assert_eq!(controller.view().subviews().len(), 0);
    window.close();
}

fn a_superseded_preview_is_cancelled() {
    let directory = TemporaryDirectory::new();
    let first = directory.write("first.md", "# First\n");
    let second = directory.write("second.md", "# Second\n");
    let controller = PreviewViewController::new(mtm());
    let window = host(&controller, 720.0, 800.0);
    let first_completion = prepare(&controller, &first);
    let second_completion = prepare(&controller, &second);
    assert_eq!(wait(&second_completion), None);
    assert_eq!(wait(&first_completion), cocoa(3072));
    assert_eq!(controller.storage_for_testing().string().to_string(), "# Second\n");
    window.close();
}

fn a_released_controller_reports_an_invalid_value() {
    let directory = TemporaryDirectory::new();
    let path = directory.write("sample.md", SAMPLE);
    let completion = {
        let controller = PreviewViewController::new(mtm());
        prepare(&controller, &path)
    };
    assert_eq!(wait(&completion), cocoa(4866));
}

fn reuse_releases_the_prior_surface() {
    let directory = TemporaryDirectory::new();
    let first = directory.write("first.md", SAMPLE);
    let second = directory.write("second.md", "# Other\n\nBody.\n");
    let controller = PreviewViewController::new(mtm());
    let window = host(&controller, 720.0, 800.0);
    assert_eq!(wait(&prepare(&controller, &first)), None);
    let old_container = controller.container_for_testing().unwrap();
    assert_eq!(wait(&prepare(&controller, &second)), None);
    let new_container = controller.container_for_testing().unwrap();
    assert!(!std::ptr::eq(&*old_container, &*new_container));
    assert!(unsafe { old_container.superview() }.is_none(), "the previous container left the view");
    assert_eq!(controller.view().subviews().len(), 1);
    assert_eq!(controller.storage_for_testing().string().to_string(), "# Other\n\nBody.\n");
    window.close();
}

fn plain_text_fallback_shows_the_source() {
    let directory = TemporaryDirectory::new();
    let path = directory.write("sample.md", SAMPLE);
    let controller = PreviewViewController::new(mtm());
    let window = host(&controller, 720.0, 800.0);
    assert_eq!(wait(&prepare(&controller, &path)), None);
    controller.fall_back_to_plain_text_for_testing();
    let text_view = controller.fallback_text_view_for_testing().expect("the plain-text view");
    assert_eq!(text_view.string().to_string(), SAMPLE);
    assert!(!text_view.isEditable());
    assert!(text_view.isSelectable());
    assert!(controller.container_for_testing().is_none());
    assert!(controller.density_gutter_for_testing().is_none());
    let bar = controller.notice_bar_for_testing().expect("the open-in-app bar");
    assert_eq!(labels(&bar), ["Plain text preview — open in Upleft for full rendering"]);
    // Falling back twice changes nothing.
    controller.fall_back_to_plain_text_for_testing();
    let again = controller.fallback_text_view_for_testing().unwrap();
    assert!(std::ptr::eq(&*again, &*text_view));
    window.close();
}

fn memory_watch_starts_after_the_first_layout_turn() {
    let directory = TemporaryDirectory::new();
    let path = directory.write("sample.md", SAMPLE);
    let controller = PreviewViewController::new(mtm());
    let window = host(&controller, 720.0, 800.0);
    assert_eq!(wait(&prepare(&controller, &path)), None);
    assert!(!controller.has_memory_timer_for_testing(), "the baseline waits a second");
    sleep_pumping(Duration::from_millis(1100));
    assert!(controller.has_memory_timer_for_testing() || controller.fallback_text_view_for_testing().is_some());
    window.close();
}

fn main() {
    // As the extension's `main` does.
    upleft_mermaid::downright::mermaid_renderer_bridge::install_fragment_renderer();
    main_thread::run(&[
        ("principal_class_is_registered_under_the_swift_name", principal_class_is_registered_under_the_swift_name),
        ("presents_a_small_document_in_read_mode", presents_a_small_document_in_read_mode),
        ("density_gutter_hides_below_the_minimum_width", density_gutter_hides_below_the_minimum_width),
        ("gutter_preview_names_the_section", gutter_preview_names_the_section),
        (
            "large_file_renders_a_bounded_prefix_with_the_open_in_app_bar",
            large_file_renders_a_bounded_prefix_with_the_open_in_app_bar,
        ),
        ("missing_file_fails_with_a_corrupt_file_error", missing_file_fails_with_a_corrupt_file_error),
        ("a_superseded_preview_is_cancelled", a_superseded_preview_is_cancelled),
        ("a_released_controller_reports_an_invalid_value", a_released_controller_reports_an_invalid_value),
        ("reuse_releases_the_prior_surface", reuse_releases_the_prior_surface),
        ("plain_text_fallback_shows_the_source", plain_text_fallback_shows_the_source),
        ("memory_watch_starts_after_the_first_layout_turn", memory_watch_starts_after_the_first_layout_turn),
    ]);
}
