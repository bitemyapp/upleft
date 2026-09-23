//! Shared helpers for the view tests: the Swift suites' private harness
//! functions, and the display-cycle prerequisite.

use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadMarker, msg_send};
use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSTextStorage};
use objc2_core_foundation::{CFRunLoop, CGRect, kCFRunLoopDefaultMode};
use objc2_foundation::{NSPoint, NSSize, NSString};
use upleft_core::parser::MarkdownParser;
use upleft_core::{DirtySet, NSRange, ParsedDocument};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::render_contracts::Theme;
use upleft_render::view::markdown_container_view::MarkdownContainerView;
use upleft_render::view::markdown_text_view::MarkdownTextView;

pub fn rect(x: f64, y: f64, width: f64, height: f64) -> CGRect {
    CGRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

pub fn text_storage(text: &str) -> Retained<NSTextStorage> {
    unsafe { msg_send![NSTextStorage::alloc(), initWithString: &*NSString::from_str(text)] }
}

pub fn parse(text: &str) -> Arc<ParsedDocument> {
    MarkdownParser::parse(text)
}

pub fn storage_string(storage: &NSTextStorage) -> String {
    storage.string().to_string()
}

/// `(text as NSString).range(of: needle)`.
pub fn range_of(text: &str, needle: &str) -> NSRange {
    let range = NSString::from_str(text).rangeOfString(&NSString::from_str(needle));
    NSRange::new(range.location as isize, range.length as isize)
}

/// `(text as NSString).range(of: needle, options: [], range: within)`.
pub fn range_of_in(text: &str, needle: &str, within: NSRange) -> NSRange {
    let range = NSString::from_str(text).rangeOfString_options_range(
        &NSString::from_str(needle),
        objc2_foundation::NSStringCompareOptions(0),
        objc2_foundation::NSRange::new(within.location as usize, within.length as usize),
    );
    NSRange::new(range.location as isize, range.length as isize)
}

pub fn utf16_len(text: &str) -> isize {
    text.encode_utf16().count() as isize
}

/// `StyleSheet(theme: .fallback, appearance: NSAppearance(named: .aqua))`.
pub fn fallback_sheet() -> Rc<StyleSheet> {
    let appearance = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).expect("aqua");
    Rc::new(StyleSheet::new(Theme::fallback(), &appearance, None))
}

/// `MarkdownTextView(frame:storage:)` with `update(document:dirty: .wholesale)`.
pub fn view_with(text: &str, frame: CGRect, mtm: MainThreadMarker) -> (Retained<MarkdownTextView>, Retained<NSTextStorage>) {
    let storage = text_storage(text);
    let view = MarkdownTextView::with_storage(frame, &storage, mtm);
    (view, storage)
}

pub fn wholesale() -> DirtySet {
    DirtySet::wholesale()
}

pub fn dirty(ranges: Vec<NSRange>) -> DirtySet {
    DirtySet::new(ranges, false)
}

/// A laid-out container, the way several suites build one.
pub fn container(text: &str, width: f64, height: f64, mtm: MainThreadMarker) -> (Retained<MarkdownContainerView>, Retained<NSTextStorage>) {
    let storage = text_storage(text);
    let container = MarkdownContainerView::with_storage(&storage, mtm);
    container.setFrame(rect(0.0, 0.0, width, height));
    container.layoutSubtreeIfNeeded();
    container.text_view().update(parse(text), &wholesale(), true);
    container.text_view().resize_to_fit_content();
    (container, storage)
}

/// Runs the main run loop (and so the main dispatch queue) until `condition`
/// holds, like Swift's `pumpMainQueue(until:timeout:)`.
///
/// Swift measures its timeout on the wall clock. On a loaded machine the
/// view's 40–80 ms idle timers fire hundreds of milliseconds late without
/// this process doing any work, which made the wall-clock budget measure
/// the machine rather than the view. The budget here is the process's own
/// CPU time — what the view could spend getting to the condition — with a
/// generous wall-clock backstop so a genuine hang still fails.
pub fn pump_main_queue(condition: impl Fn() -> bool, timeout: Duration) -> bool {
    let cpu_start = process_cpu_time();
    let wall_deadline = Instant::now() + timeout * 30;
    while !condition() && process_cpu_time() - cpu_start < timeout && Instant::now() < wall_deadline {
        CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, 0.005, true);
    }
    condition()
}

fn process_cpu_time() -> Duration {
    let mut spec = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: `spec` is a valid out parameter.
    unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut spec) };
    Duration::new(spec.tv_sec as u64, spec.tv_nsec as u32)
}

pub fn pump(condition: impl Fn() -> bool) -> bool {
    pump_main_queue(condition, Duration::from_secs(1))
}

/// Unused so far: no ported test asserts pixels, but one that does must
/// declare this prerequisite, as Swift's do.
#[allow(dead_code)]
/// `RenderSmokeTests.viewportLayoutRuns()`: whether this process gets the
/// TextKit 2 viewport pass that view-level geometry assertions need.
pub fn viewport_layout_runs(mtm: MainThreadMarker) -> bool {
    let probe_text = "# Probe\n\nBody text.\n\n## Later\n\nMore";
    let storage = text_storage(probe_text);
    let probe = MarkdownContainerView::with_storage(&storage, mtm);
    probe.setFrame(rect(0.0, 0.0, 900.0, 700.0));
    probe.text_view().update(parse(probe_text), &wholesale(), true);
    probe.layoutSubtreeIfNeeded();
    probe.text_view().prepare_for_display();
    let later = range_of(probe_text, "## Later").location;
    probe.text_view().top_visible_offset() < later
}

/// `#expect` with a message: records the failure and panics so the harness
/// reports the test as failed.
#[macro_export]
macro_rules! expect {
    ($condition:expr) => {
        assert!($condition, "expectation failed: {}", stringify!($condition))
    };
    ($condition:expr, $($message:tt)+) => {
        assert!($condition, $($message)+)
    };
}


/// `makeContainer(_:)` of the fragment suites (ListOrnamentTests,
/// CalloutGeometryTests, CodeBlockGeometryTests): a 1000×900 container in
/// Read mode, laid out and sized to its content.
pub fn read_container(text: &str, mtm: MainThreadMarker) -> Retained<MarkdownContainerView> {
    let storage = text_storage(text);
    let container = MarkdownContainerView::with_storage(&storage, mtm);
    container.setFrame(rect(0.0, 0.0, 1000.0, 900.0));
    container.layoutSubtreeIfNeeded();
    container.text_view().set_mode(upleft_render::render_contracts::RenderMode::Read);
    container.text_view().update(parse(text), &wholesale(), true);
    container.text_view().resize_to_fit_content();
    container
}

/// Every laid-out fragment of `view`, in document order, after
/// `ensureLayout(for: documentRange)`.
pub fn layout_fragments(view: &MarkdownTextView) -> Vec<Retained<objc2_app_kit::NSTextLayoutFragment>> {
    use objc2_app_kit::{NSTextLayoutFragment, NSTextLayoutFragmentEnumerationOptions, NSTextSelectionDataSource};
    let Some(layout) = view.textLayoutManager() else { return Vec::new() };
    layout.ensureLayoutForRange(&layout.documentRange());
    let fragments = std::cell::RefCell::new(Vec::new());
    let block = block2::StackBlock::new(|fragment: std::ptr::NonNull<NSTextLayoutFragment>| -> objc2::runtime::Bool {
        // SAFETY: TextKit hands a live fragment for the call.
        fragments.borrow_mut().push(objc2::Message::retain(unsafe { fragment.as_ref() }));
        objc2::runtime::Bool::YES
    });
    layout.enumerateTextLayoutFragmentsFromLocation_options_usingBlock(
        Some(&layout.documentRange().location()),
        NSTextLayoutFragmentEnumerationOptions::EnsuresLayout,
        &block,
    );
    fragments.into_inner()
}

/// The fragments of one Swift class (`fragment as? CalloutFragment`), as
/// the `DownrightFragment`s they are.
pub fn fragments_of_class(
    view: &MarkdownTextView,
    class_name: &str,
) -> Vec<Retained<upleft_render::fragments::fragment_base::DownrightFragment>> {
    layout_fragments(view)
        .into_iter()
        .filter(|fragment| {
            let object: &objc2::runtime::AnyObject = fragment;
            object.class().name().to_str() == Ok(class_name)
        })
        .filter_map(|fragment| fragment.downcast::<upleft_render::fragments::fragment_base::DownrightFragment>().ok())
        .collect()
}
