//! Mirrors `oracle/Sources/downright-oracle/ViewBench.swift`:
//!
//!   upleft-oracle bench-view <file.md> <out.json> [--mode M] [--theme NAME] [--dark] [--width W] [--height H]
//!
//! The render scene's view work, timed call for call as the Swift times it:
//! `update(document:dirty: .wholesale)`, the first-frame sequence, and one
//! settle pass, over a fresh window, storage and container per run.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use block2::StackBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, ProtocolObject};
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAppearanceCustomization, NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSBackingStoreType,
    NSColorSpace, NSTextElementProvider, NSTextLayoutFragment, NSTextLayoutFragmentEnumerationOptions,
    NSTextSelectionDataSource, NSTextStorage, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use serde_json::Value;
use upleft_core::DirtySet;
use upleft_core::parser::MarkdownParser;
use upleft_render::render_contracts::RenderMode;
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;
use upleft_render::view::markdown_container_view::MarkdownContainerView;
use upleft_render::view::markdown_text_view_delegate::ScrollPosition;

use super::Request;
use super::json::{Object, double, write};

thread_local! {
    static REQUEST: RefCell<Option<Request>> = const { RefCell::new(None) };
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpleftViewBench"]
    struct ViewBenchDelegate;

    unsafe impl NSObjectProtocol for ViewBenchDelegate {}

    unsafe impl NSApplicationDelegate for ViewBenchDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn application_did_finish_launching(&self, _notification: &NSNotification) {
            let request = REQUEST.with(|cell| cell.borrow().clone()).expect("request");
            match bench(&request, self.mtm()) {
                Ok(()) => std::process::exit(0),
                Err(message) => {
                    eprintln!("bench-view failed: {message}");
                    std::process::exit(2)
                }
            }
        }
    }
);

pub fn run(request: &Request) -> ! {
    let mtm = MainThreadMarker::new().expect("bench-view runs on the main thread");
    REQUEST.with(|cell| *cell.borrow_mut() = Some(request.clone()));
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let delegate: Retained<ViewBenchDelegate> = unsafe { msg_send![ViewBenchDelegate::alloc(mtm), init] };
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
    std::process::exit(0)
}

fn percentile(ascending: &[f64], p: f64) -> f64 {
    let rank = (p * ascending.len() as f64).ceil() as isize;
    ascending[(ascending.len() as isize - 1).min(0.max(rank - 1)) as usize]
}

fn bench(request: &Request, mtm: MainThreadMarker) -> Result<(), String> {
    let text = super::markup::read_text(&request.input).map_err(|error| format!("{error:?}"))?;
    let document = MarkdownParser::parse(&text);
    let appearance = crate::capture::appearance(request.dark);
    let themes = ThemeStore::shared().themes();
    let theme = themes.iter().find(|theme| theme.name == request.theme).cloned().ok_or("unknown theme")?;
    let style_sheet = Rc::new(StyleSheet::new(theme, &appearance, Some(true)));
    let mode = RenderMode::from_raw_value(&request.mode).unwrap_or(RenderMode::Live);
    let runs: usize = std::env::var("VIEW_BENCH_RUNS").ok().and_then(|value| value.parse().ok()).unwrap_or(10);
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);

    let (mut update, mut first_frame, mut settle, mut total) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut fragments = 0usize;
    for index in 0..=runs {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(request.width, request.height));
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setAppearance(Some(&appearance));
        window.setColorSpace(Some(&NSColorSpace::sRGBColorSpace()));
        let storage: Retained<NSTextStorage> =
            unsafe { msg_send![NSTextStorage::alloc(), initWithString: &*NSString::from_str(&text)] };
        let container = MarkdownContainerView::new(&storage, style_sheet.clone(), mtm);
        container.setFrame(frame);
        window.setContentView(Some(&container));
        window.layoutIfNeeded();
        container.layoutSubtreeIfNeeded();
        let text_view = container.text_view();
        text_view.set_mode(mode);

        let t0 = Instant::now();
        text_view.update(document.clone(), &DirtySet::wholesale(), true);
        let t1 = Instant::now();
        window.orderFrontRegardless();
        let t2 = Instant::now();
        window.layoutIfNeeded();
        container.layoutSubtreeIfNeeded();
        text_view.resize_to_fit_content();
        text_view.scroll_to_offset(0, ScrollPosition::Top, false);
        text_view.prepare_for_display();
        text_view.displayIfNeeded();
        let t3 = Instant::now();
        container.layoutSubtreeIfNeeded();
        if let Some(layout) = text_view.textLayoutManager() {
            layout.ensureLayoutForRange(&layout.documentRange());
        }
        container.displayIfNeeded();
        let t4 = Instant::now();

        if index > 0 {
            let ms = |duration: std::time::Duration| duration.as_nanos() as f64 / 1e6;
            update.push(ms(t1 - t0));
            first_frame.push(ms(t3 - t2));
            settle.push(ms(t4 - t3));
            total.push(ms((t1 - t0) + (t4 - t2)));
        }
        if let Some(layout) = text_view.textLayoutManager() {
            let count = std::cell::Cell::new(0usize);
            let block = StackBlock::new(|_fragment: std::ptr::NonNull<NSTextLayoutFragment>| -> Bool {
                count.set(count.get() + 1);
                Bool::YES
            });
            layout.enumerateTextLayoutFragmentsFromLocation_options_usingBlock(
                Some(&layout.documentRange().location()),
                NSTextLayoutFragmentEnumerationOptions(0),
                &block,
            );
            fragments = count.get();
        }
        window.orderOut(None);
        window.setContentView(None);
        window.close();
    }

    let stats = |label: &str, values: &mut Vec<f64>| -> (String, Value) {
        values.sort_by(|a, b| a.total_cmp(b));
        let (p50, p95, max) = (percentile(values, 0.50), percentile(values, 0.95), *values.last().unwrap());
        println!("  {label:<44}  p50 {p50:8.3} ms   p95 {p95:8.3} ms   max {max:8.3} ms (n={})", values.len());
        (
            label.to_owned(),
            Object::new()
                .with("p50", double(p50))
                .with("p95", double(p95))
                .with("max", double(max))
                .with("runs", values.len())
                .build(),
        )
    };
    println!("bench-view: {}, {runs} runs", request.input.file_name().unwrap_or_default().to_string_lossy());
    let mut object = Object::new();
    for (label, value) in [
        stats("update(document:) wholesale", &mut update),
        stats("first frame (afterShow)", &mut first_frame),
        stats("settle pass", &mut settle),
        stats("update to settled", &mut total),
    ] {
        object = object.with(&label, value);
    }
    object = object.with("fragments", fragments);
    write(&object.build(), &request.output).map_err(|error| error.to_string())
}

#[allow(dead_code)]
fn _traits(_: &dyn NSTextElementProvider, _: &dyn NSTextSelectionDataSource, _: &dyn NSAppearanceCustomization) {}
