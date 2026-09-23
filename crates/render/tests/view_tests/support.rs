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
/// holds or `timeout` passes, like Swift's `pumpMainQueue(until:)`.
pub fn pump_main_queue(condition: impl Fn() -> bool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while !condition() && Instant::now() < deadline {
        CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, 0.005, true);
    }
    condition()
}

pub fn pump(condition: impl Fn() -> bool) -> bool {
    pump_main_queue(condition, Duration::from_secs(1))
}

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

