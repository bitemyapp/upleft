//! Hosted embedding, end to end (docs/EMBEDDING.md): a chat transcript made
//! of hosted `MarkdownTextView`s stacked in one `NSScrollView`, the last one
//! streamed into token by token.
//!
//!   cargo run --release -p upleft-render --example hosted_transcript -- \
//!       --doc markdown-stress.md [--out DIR] [--kb 200] [--style modern|editorial|terminal]
//!   cargo run --release -p upleft-render --example hosted_transcript -- --doc FILE --baseline
//!
//! Every window is borderless and placed at (-30000, -30000); the app runs
//! with the Accessory policy and is never activated. Nothing appears on
//! screen.
//!
//! 1. **Thread safety.** Renders each Mermaid diagram and display formula of
//!    the document on the main thread and on a worker and compares the
//!    bitmaps; checks the worker's italic system font against
//!    `NSFontManager`'s.
//! 2. **Proof.** Streams the document into the last row of one transcript
//!    (`set_streaming(true)`, token-sized appends, each parsed on a worker
//!    and applied with `update` in the same turn as the storage edit), and
//!    gives a second, identical transcript the whole text at once. Once both
//!    settle, compares the rows' heights and fragment frames and the pixels
//!    of both transcripts at every scroll position over the last row, and
//!    writes PNGs of the streamed transcript.
//! 3. **Performance.** Streams the document again onto the end of a message
//!    already `--kb` kilobytes long and prints p50/p99/max per append for
//!    `update` (with the storage edit and the host's restack), for
//!    `prepare_for_display`, and for the window's draw.
//!
//! `--baseline` instead measures step 3 on Downright's document path (one
//! plain `MarkdownTextView` as a scroll view's document view, Read mode).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use block2::StackBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, ProtocolObject};
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameAqua, NSApplication, NSApplicationActivationPolicy,
    NSApplicationDelegate, NSBackingStoreType, NSBitmapImageFileType, NSBitmapImageRep, NSColorSpace, NSFont,
    NSFontManager, NSFontTraitMask, NSResponder, NSScrollView, NSTextLayoutFragment,
    NSTextLayoutFragmentEnumerationOptions, NSTextStorage, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_core_foundation::{CFRunLoop, CGFloat, CGRect, kCFRunLoopDefaultMode};
use objc2_foundation::{NSDictionary, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
use upleft_core::ast_diff::ASTDiff;
use upleft_core::parser::MarkdownParser;
use upleft_core::{BlockContent, DirtySet, ParsedDocument};
use upleft_math::MathRenderer;
use upleft_mermaid::downright::mermaid_renderer_bridge;
use upleft_render::fragments::async_objects;
use upleft_render::render_contracts::{RenderMode, Theme};
use upleft_render::theme::style_sheet::{BodyFamily, HostTypography, StyleSheet};
use upleft_render::theme::theme_store::ThemeStore;
use upleft_render::view::markdown_text_view::MarkdownTextView;
use upleft_render::view::markdown_text_view_delegate::MarkdownTextViewDelegate;

const WINDOW_WIDTH: CGFloat = 760.0;
const WINDOW_HEIGHT: CGFloat = 900.0;
const MARGIN: CGFloat = 24.0;
const GAP: CGFloat = 16.0;

thread_local! {
    static ARGS: RefCell<Option<Args>> = const { RefCell::new(None) };
}

#[derive(Clone)]
struct Args {
    doc: PathBuf,
    out: PathBuf,
    kilobytes: usize,
    style: String,
    baseline: bool,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpleftHostedTranscriptApp"]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn application_did_finish_launching(&self, _notification: &NSNotification) {
            let args = ARGS.with(|cell| cell.borrow().clone()).expect("args");
            let passed = if args.baseline { baseline(&args, self.mtm()) } else { run(&args, self.mtm()) };
            std::process::exit(if passed { 0 } else { 1 });
        }
    }
);

define_class!(
    /// The transcript's document view: rows stack from the top.
    // SAFETY: NSView's initialiser is `initWithFrame:`; the override keeps
    // AppKit's signature. No Drop impl.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "UpleftHostedTranscriptStack"]
    struct FlippedStack;

    impl FlippedStack {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

fn main() {
    let mut args = Args {
        doc: PathBuf::new(),
        out: std::env::temp_dir().join("hosted-transcript"),
        kilobytes: 200,
        style: "modern".to_owned(),
        baseline: false,
    };
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--doc" => args.doc = PathBuf::from(iter.next().expect("--doc PATH")),
            "--out" => args.out = PathBuf::from(iter.next().expect("--out DIR")),
            "--kb" => args.kilobytes = iter.next().expect("--kb N").parse().expect("--kb N"),
            "--style" => args.style = iter.next().expect("--style NAME"),
            "--baseline" => args.baseline = true,
            _ => panic!("unknown argument {arg}"),
        }
    }
    let mtm = MainThreadMarker::new().expect("main thread");
    // The start-up hook every host calls once (docs/EMBEDDING.md).
    mermaid_renderer_bridge::install_fragment_renderer();
    ARGS.with(|cell| *cell.borrow_mut() = Some(args));
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    let delegate: Retained<AppDelegate> = unsafe { msg_send![AppDelegate::alloc(mtm), init] };
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
}

// MARK: - The host

/// The host's side of one transcript: rows, their frames, the scroll view.
struct Transcript {
    window: Retained<NSWindow>,
    scroll: Retained<NSScrollView>,
    stack: Retained<FlippedStack>,
    rows: RefCell<Vec<Row>>,
    restacks: Cell<usize>,
    /// Keep the view scrolled to the bottom as rows grow, as a chat does.
    follows_bottom: Cell<bool>,
    delegate: RefCell<Option<Rc<RowDelegate>>>,
}

struct Row {
    view: Retained<MarkdownTextView>,
    storage: Retained<NSTextStorage>,
    x: CGFloat,
}

struct RowDelegate {
    transcript: Weak<Transcript>,
}

impl MarkdownTextViewDelegate for RowDelegate {
    fn did_change_content_height(&self, _view: &MarkdownTextView, _height: f64) {
        if let Some(transcript) = self.transcript.upgrade() {
            transcript.restack();
        }
    }
}

impl Transcript {
    fn new(style_sheet: &Rc<StyleSheet>, messages: &[(bool, &str)], mtm: MainThreadMarker) -> Rc<Transcript> {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WINDOW_WIDTH, WINDOW_HEIGHT));
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
        window.setAppearance(Some(&style_sheet.appearance));
        window.setColorSpace(Some(&NSColorSpace::sRGBColorSpace()));
        window.setFrameOrigin(NSPoint::new(-30000.0, -30000.0));
        let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), frame);
        scroll.setHasVerticalScroller(false);
        scroll.setBackgroundColor(&style_sheet.background);
        let stack: Retained<FlippedStack> = unsafe { msg_send![FlippedStack::alloc(mtm), initWithFrame: frame] };
        scroll.setDocumentView(Some(&stack));
        window.setContentView(Some(&scroll));
        window.orderFrontRegardless();
        let transcript = Rc::new(Transcript {
            window,
            scroll,
            stack,
            rows: RefCell::new(Vec::new()),
            restacks: Cell::new(0),
            follows_bottom: Cell::new(false),
            delegate: RefCell::new(None),
        });
        let delegate = Rc::new(RowDelegate { transcript: Rc::downgrade(&transcript) });
        *transcript.delegate.borrow_mut() = Some(delegate.clone());
        let delegate: Rc<dyn MarkdownTextViewDelegate> = delegate;
        let weak: Weak<dyn MarkdownTextViewDelegate> = Rc::downgrade(&delegate);
        for (is_user, text) in messages {
            // User turns sit right, narrower, on the surface colour, with a
            // small inset; assistant turns span the column.
            let width = if *is_user { 520.0 } else { WINDOW_WIDTH - MARGIN * 2.0 };
            let storage: Retained<NSTextStorage> =
                unsafe { msg_send![NSTextStorage::alloc(), initWithString: &*NSString::from_str(text)] };
            let view = MarkdownTextView::new_hosted(&storage, style_sheet.clone(), width, mtm);
            view.set_markdown_delegate(Some(weak.clone()));
            if *is_user {
                view.set_hosted_insets(NSSize::new(12.0, 8.0));
                view.setBackgroundColor(&style_sheet.surface);
            } else {
                view.setDrawsBackground(false);
            }
            let x = if *is_user { WINDOW_WIDTH - MARGIN - width - 24.0 } else { MARGIN };
            transcript.stack.addSubview(&view);
            transcript.rows.borrow_mut().push(Row { view: view.clone(), storage, x });
            view.update(MarkdownParser::parse(text), &DirtySet::wholesale(), false);
        }
        transcript.restack();
        transcript
    }

    fn last(&self) -> (Retained<MarkdownTextView>, Retained<NSTextStorage>) {
        let rows = self.rows.borrow();
        let row = rows.last().expect("rows");
        (row.view.clone(), row.storage.clone())
    }

    /// The host's reaction to `did_change_content_height`: restack every row
    /// under the one above it, in the same run-loop pass.
    fn restack(&self) {
        self.restacks.set(self.restacks.get() + 1);
        let Ok(rows) = self.rows.try_borrow() else { return };
        let mut y = GAP;
        for row in rows.iter() {
            row.view.setFrameOrigin(NSPoint::new(row.x, y));
            y += row.view.frame().size.height + GAP;
        }
        let height = y.max(self.scroll.contentSize().height);
        self.stack.setFrameSize(NSSize::new(WINDOW_WIDTH, height));
        if self.follows_bottom.get() {
            self.scroll_to(height - self.scroll.contentSize().height);
        }
    }

    fn scroll_to(&self, y: CGFloat) {
        let clip = self.scroll.contentView();
        let max = (self.stack.frame().size.height - clip.bounds().size.height).max(0.0);
        clip.scrollToPoint(NSPoint::new(0.0, y.clamp(0.0, max)));
        self.scroll.reflectScrolledClipView(&clip);
    }

    /// The whole window content as PNG bytes, drawn off screen.
    fn capture(&self) -> Vec<u8> {
        self.window.displayIfNeeded();
        let view: Retained<NSView> = self.window.contentView().expect("content view");
        let bounds = view.bounds();
        let rep = view.bitmapImageRepForCachingDisplayInRect(bounds).expect("bitmap");
        view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
        png(&rep)
    }
}

fn png(rep: &NSBitmapImageRep) -> Vec<u8> {
    unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }
        .expect("PNG")
        .to_vec()
}

fn spin(seconds: f64) {
    CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, seconds, false);
}

/// Runs the main queue until no diagram or formula is still rendering.
fn settle() {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        spin(0.02);
        if async_objects::pending_count() == 0 {
            // Let the landing handlers and any scheduled relayout run.
            spin(0.05);
            if async_objects::pending_count() == 0 {
                return;
            }
        }
        if Instant::now() > deadline {
            eprintln!("warning: renders still pending after 60 s");
            return;
        }
    }
}

/// Token-sized pieces, 1–8 characters, from a fixed seed.
fn chunks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut current = String::new();
    let mut want = 0usize;
    for ch in text.chars() {
        if want == 0 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            want = 1 + ((seed >> 33) % 8) as usize;
        }
        current.push(ch);
        want -= 1;
        if want == 0 {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

type Parsed = (String, Arc<ParsedDocument>, DirtySet);

/// Parses successive prefixes on a worker, as a host must: the main thread
/// only applies them.
fn parse_ahead(base: String, pieces: Vec<String>) -> mpsc::Receiver<Parsed> {
    let (sender, receiver) = mpsc::sync_channel(8);
    std::thread::spawn(move || {
        let mut text = base;
        let mut previous = MarkdownParser::parse(&text);
        for piece in pieces {
            text.push_str(&piece);
            let fresh = MarkdownParser::parse(&text);
            let dirty = ASTDiff::dirty_set(Some(&previous), &fresh);
            previous = fresh.clone();
            if sender.send((piece, fresh, dirty)).is_err() {
                return;
            }
        }
    });
    receiver
}

fn percentile(ascending: &[f64], p: f64) -> f64 {
    let rank = (p * ascending.len() as f64).ceil() as isize;
    ascending[(ascending.len() as isize - 1).min(0.max(rank - 1)) as usize]
}

fn report(label: &str, values: &mut [f64]) {
    values.sort_by(f64::total_cmp);
    println!(
        "  {label:<34} p50 {:7.3} ms  p99 {:7.3} ms  max {:7.3} ms  (n={})",
        percentile(values, 0.5),
        percentile(values, 0.99),
        values.last().copied().unwrap_or(0.0),
        values.len()
    );
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// The three chat styles a host such as Omperor offers.
fn host_typography(style: &str) -> HostTypography {
    let headings = |sizes: [f64; 4]| [Some(sizes[0]), Some(sizes[1]), Some(sizes[2]), Some(sizes[3]), None, None];
    match style {
        "editorial" => HostTypography {
            body_family: Some(BodyFamily::NewYork),
            body_size: Some(15.5),
            heading_sizes: headings([23.0, 20.0, 17.0, 15.5]),
            line_height_multiple: Some(1.55),
            paragraph_spacing: Some(13.0),
            hyphenation_factor: Some(0.9),
            code_bleed: Some(0.0),
            ..HostTypography::default()
        },
        "terminal" => HostTypography {
            body_family: Some(BodyFamily::Monospaced),
            body_size: Some(14.0),
            heading_sizes: headings([17.0, 16.0, 15.0, 14.0]),
            code_size: Some(13.0),
            line_height_multiple: Some(1.45),
            paragraph_spacing: Some(9.0),
            code_bleed: Some(0.0),
            ..HostTypography::default()
        },
        _ => HostTypography {
            body_family: Some(BodyFamily::System),
            body_size: Some(15.0),
            heading_sizes: headings([23.0, 20.0, 17.0, 15.0]),
            line_height_multiple: Some(1.5),
            paragraph_spacing: Some(11.0),
            code_bleed: Some(0.0),
            ..HostTypography::default()
        },
    }
}

fn host_style_sheet(style: &str) -> Rc<StyleSheet> {
    let appearance = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).expect("aqua");
    let theme = ThemeStore::bundled_themes()
        .into_iter()
        .find(|theme| theme.name == "Paper Light")
        .unwrap_or_else(Theme::fallback);
    Rc::new(StyleSheet::for_host(theme, &appearance, true, host_typography(style)))
}

const OPENING: [(bool, &str); 3] = [
    (true, "Show me every Markdown construct the transcript draws, *with* diagrams and math."),
    (
        false,
        "Sure. First, the usual suspects:\n\n- **Lists** and `inline code`\n- [links](https://example.com)\n\n```sh\ncargo run --release\n```\n\nThe full answer follows.",
    ),
    (true, "Go ahead, stream it."),
];

/// The first character whose attributes differ between two views' storage,
/// ignoring fragment payload identity.
fn first_attribute_difference(a: &MarkdownTextView, b: &MarkdownTextView) -> Option<String> {
    let (sa, sb) = unsafe { (a.textStorage()?, b.textStorage()?) };
    if sa.string().to_string() != sb.string().to_string() {
        return Some("text differs".to_owned());
    }
    let length = sa.length();
    let mut index = 0usize;
    while index < length {
        let mut ra = objc2_foundation::NSRange::new(0, 0);
        let mut rb = objc2_foundation::NSRange::new(0, 0);
        let da = unsafe { sa.attributesAtIndex_effectiveRange(index, &mut ra) };
        let db = unsafe { sb.attributesAtIndex_effectiveRange(index, &mut rb) };
        let describe = |d: &NSDictionary<NSString, objc2::runtime::AnyObject>| {
            let (keys, objects) = d.to_vecs();
            let mut entries: Vec<String> = keys
                .iter()
                .zip(objects.iter())
                .map(|(key, value)| {
                    let key = key.to_string();
                    let value = if ["drFragment", "drBlock", "NSAttachment"].contains(&key.as_str()) {
                        "<payload>".to_owned()
                    } else {
                        let description: Retained<NSString> = unsafe { msg_send![&**value, description] };
                        description.to_string().replace('\n', " ")
                    };
                    format!("{key}={value}")
                })
                .collect();
            entries.sort();
            entries.join("; ")
        };
        let (xa, xb) = (describe(&da), describe(&db));
        if xa != xb {
            let text = sa.string().to_string();
            let start = text.encode_utf16().take(index).count();
            let context: String = String::from_utf16_lossy(&text.encode_utf16().skip(start.saturating_sub(20)).take(40).collect::<Vec<_>>());
            return Some(format!("at {index} ({context:?}):\n    streamed {xa}\n    whole    {xb}"));
        }
        index = (ra.location + ra.length).min(rb.location + rb.length).max(index + 1);
    }
    None
}

/// Every layout fragment's class and frame, in order, in 1/64 pt.
fn fragment_frames(view: &MarkdownTextView) -> Vec<(String, [i64; 4])> {
    use objc2_app_kit::NSTextSelectionDataSource;
    let Some(layout) = view.textLayoutManager() else { return Vec::new() };
    layout.ensureLayoutForRange(&layout.documentRange());
    let frames = RefCell::new(Vec::new());
    let block = StackBlock::new(|fragment: std::ptr::NonNull<NSTextLayoutFragment>| -> Bool {
        let fragment = unsafe { fragment.as_ref() };
        let frame: CGRect = fragment.layoutFragmentFrame();
        let class = fragment.class().name().to_string_lossy().into_owned();
        let q = |v: f64| (v * 64.0).round() as i64;
        frames.borrow_mut().push((class, [q(frame.origin.x), q(frame.origin.y), q(frame.size.width), q(frame.size.height)]));
        Bool::YES
    });
    layout.enumerateTextLayoutFragmentsFromLocation_options_usingBlock(
        Some(&layout.documentRange().location()),
        NSTextLayoutFragmentEnumerationOptions::EnsuresLayout,
        &block,
    );
    frames.into_inner()
}

// MARK: - Run

fn run(args: &Args, mtm: MainThreadMarker) -> bool {
    let text = std::fs::read_to_string(&args.doc).expect("--doc: the document to stream");
    std::fs::create_dir_all(&args.out).expect("--out");
    let style_sheet = host_style_sheet(&args.style);
    let mut passed = true;

    println!("== thread safety ==");
    passed &= thread_safety(&text, &style_sheet, mtm);

    println!("== proof: streamed vs whole ({}, {} bytes) ==", args.style, text.len());
    passed &= proof(&text, &style_sheet, &args.out, mtm);

    println!("== host typography ==");
    for style in ["modern", "editorial", "terminal"] {
        let transcript = Transcript::new(&host_style_sheet(style), &[OPENING[0], (false, text.as_str())], mtm);
        settle();
        transcript.restack();
        let path = args.out.join(format!("style-{style}.png"));
        std::fs::write(&path, transcript.capture()).expect("write PNG");
        println!("  {style:<10} {}", path.display());
    }

    println!("== performance: appends to a {} KB message ==", args.kilobytes);
    performance(&text, args.kilobytes, &style_sheet, mtm);
    println!("{}", if passed { "PASS" } else { "FAIL" });
    passed
}

/// A style sheet handed to a worker thread by this harness (see
/// `upleft_render::fragments::async_objects` for why that is sound).
struct SendStyle(StyleSheet);
unsafe impl Send for SendStyle {}

fn thread_safety(text: &str, style_sheet: &Rc<StyleSheet>, mtm: MainThreadMarker) -> bool {
    let document = MarkdownParser::parse(text);
    let mut diagrams = Vec::new();
    let mut formulas = Vec::new();
    document.root.walk(&mut |block| match &block.content {
        BlockContent::Mermaid { source_range } => diagrams.push(document.substring(*source_range)),
        BlockContent::MathBlock { latex_range } => formulas.push(document.substring(*latex_range)),
        _ => {}
    });
    let mut passed = true;

    // Mermaid: the synchronous path on the main thread against the worker path.
    let scale = objc2_app_kit::NSScreen::mainScreen(mtm).map_or(2.0, |screen| screen.backingScaleFactor());
    let mut same = 0;
    for source in &diagrams {
        let main = mermaid_renderer_bridge::image(source, style_sheet)
            .map(|image| png(&NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &image.cg_image)));
        let style = SendStyle((**style_sheet).clone());
        let source_owned = source.clone();
        let worker = std::thread::spawn(move || {
            let style = style;
            mermaid_renderer_bridge::image_at_scale(&source_owned, &style.0, scale)
                .map(|image| png(&NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &image.cg_image)))
        })
        .join()
        .expect("worker");
        if main.is_some() && main == worker {
            same += 1;
        } else {
            passed = false;
            println!("  mermaid differs off the main thread: {}", source.lines().next().unwrap_or(""));
        }
    }
    println!("  mermaid: {same}/{} diagrams identical on a worker (scale {scale})", diagrams.len());

    // The italic system font: NSFontManager (main thread) against the
    // descriptor the worker uses, at the sizes the renderers ask for.
    let manager = NSFontManager::sharedFontManager(mtm);
    let mut fonts_agree = true;
    for (size, weight) in [(10.0, 0.23), (11.0, 0.0), (12.0, 0.0), (14.0, 0.3)] {
        let base = NSFont::systemFontOfSize_weight(size, weight);
        let by_manager = manager.convertFont_toHaveTrait(&base, NSFontTraitMask::ItalicFontMask);
        let by_descriptor = upleft_mermaid::render::diagram_renderer::italic_by_descriptor(&base);
        let other: &objc2::runtime::AnyObject = by_descriptor.as_ref();
        let agree = by_manager.fontName().to_string() == by_descriptor.fontName().to_string()
            && by_manager.pointSize() == by_descriptor.pointSize()
            && by_manager.isEqual(Some(other));
        if !agree {
            println!("  italic {size}/{weight}: manager {} vs descriptor {}", by_manager.fontName(), by_descriptor.fontName());
        }
        fonts_agree &= agree;
    }
    println!("  italic system font by descriptor equals NSFontManager's: {fonts_agree}");
    passed &= fonts_agree;

    // Display math, as `MathFragment` asks for it, typeset and padded on the
    // main thread and on a worker (the shared cache emptied in between).
    let point_size = style_sheet.math_point_size * 1.12;
    let math_cache = &upleft_math::downright::bounded_image_cache::MATH;
    let mut same = 0;
    for latex in &formulas {
        math_cache.remove_all();
        let main = MathRenderer::image(latex, true, point_size, &style_sheet.text, 8.0)
            .and_then(|image| image.TIFFRepresentation())
            .map(|data| data.to_vec());
        math_cache.remove_all();
        let style = SendStyle((**style_sheet).clone());
        let latex_owned = latex.clone();
        let worker = std::thread::spawn(move || {
            let style = style;
            MathRenderer::image(&latex_owned, true, point_size, &style.0.text, 8.0)
                .and_then(|image| image.TIFFRepresentation())
                .map(|data| data.to_vec())
        })
        .join()
        .expect("worker");
        if main.is_some() && main == worker {
            same += 1;
        } else {
            passed = false;
            println!("  math differs off the main thread: {}", latex.trim());
        }
    }
    math_cache.remove_all();
    println!("  math: {same}/{} formulas identical on a worker", formulas.len());
    passed
}

fn proof(text: &str, style_sheet: &Rc<StyleSheet>, out: &Path, mtm: MainThreadMarker) -> bool {
    let mut messages: Vec<(bool, &str)> = OPENING.to_vec();
    // A: the last row streams in from empty.
    messages.push((false, ""));
    let streamed = Transcript::new(style_sheet, &messages, mtm);
    // B: the same transcript with the whole answer at once.
    messages.pop();
    messages.push((false, text));
    let whole = Transcript::new(style_sheet, &messages, mtm);
    settle();

    let (view, storage) = streamed.last();
    streamed.follows_bottom.set(true);
    view.set_streaming(true);
    let pieces = chunks(text);
    let appends = pieces.len();
    let restacks_before = streamed.restacks.get();
    let mut synchronous = 0usize;
    let started = Instant::now();
    for (piece, fresh, dirty) in parse_ahead(String::new(), pieces) {
        let length = storage.length();
        storage.beginEditing();
        storage.replaceCharactersInRange_withString(objc2_foundation::NSRange::new(length, 0), &NSString::from_str(&piece));
        storage.endEditing();
        let before = streamed.restacks.get();
        let height_before = view.frame().size.height;
        view.update(fresh, &dirty, true);
        if streamed.restacks.get() > before || view.frame().size.height == height_before {
            synchronous += 1;
        }
        view.prepare_for_display();
        streamed.window.displayIfNeeded();
        spin(0.0);
    }
    view.set_streaming(false);
    println!(
        "  streamed {appends} appends in {:.0} ms; {} height reports; after {synchronous}/{appends} updates the host had already restacked (or nothing changed)",
        ms(started.elapsed()),
        streamed.restacks.get() - restacks_before,
    );
    settle();
    streamed.follows_bottom.set(false);
    streamed.restack();
    whole.restack();

    let mut passed = synchronous == appends;
    let (fresh_view, _) = whole.last();
    let (a, b) = (view.content_height(), fresh_view.content_height());
    println!("  content height: streamed {a}, whole {b}, frame {}", view.frame().size.height);
    if a != b || view.frame().size.height != a {
        passed = false;
        println!("  FAIL: heights differ");
    }
    if let Some(report) = first_attribute_difference(&view, &fresh_view) {
        passed = false;
        println!("  FAIL: storage attributes differ: {report}");
    }
    // Pixels, at viewport-sized steps over the streamed row.
    let row = view.frame();
    let clip_height = streamed.scroll.contentSize().height;
    let mut y = (row.origin.y - GAP).max(0.0);
    let mut positions = 0;
    let mut identical = 0;
    let mut written = Vec::new();
    loop {
        streamed.scroll_to(y);
        whole.scroll_to(y);
        let (pa, pb) = (streamed.capture(), whole.capture());
        positions += 1;
        let at_end = y + clip_height >= streamed.stack.frame().size.height;
        if pa == pb {
            identical += 1;
        } else {
            passed = false;
            std::fs::write(out.join(format!("mismatch-{positions:02}-streamed.png")), &pa).ok();
            std::fs::write(out.join(format!("mismatch-{positions:02}-whole.png")), &pb).ok();
        }
        if positions == 1 || at_end || positions % 3 == 0 {
            let path = out.join(format!("stack-{positions:02}-y{}.png", y as i64));
            std::fs::write(&path, &pa).expect("write PNG");
            written.push(path);
        }
        if at_end {
            break;
        }
        y += clip_height * 0.8;
    }
    // TextKit 2 positions a fragment below one whose height changed only
    // when viewport layout reaches it, so the frames are compared once the
    // pass above has shown every part of both rows.
    let (frames_a, frames_b) = (fragment_frames(&view), fragment_frames(&fresh_view));
    if frames_a != frames_b {
        passed = false;
        let first = frames_a.iter().zip(&frames_b).position(|(x, y)| x != y);
        println!("  FAIL: fragment frames differ ({} vs {}, first at {first:?})", frames_a.len(), frames_b.len());
        if let Some(index) = first {
            for i in index.saturating_sub(2)..(index + 6).min(frames_a.len()).min(frames_b.len()) {
                println!("    {i:3} streamed {:?}\n        whole    {:?}", frames_a[i], frames_b[i]);
            }
        }
    } else {
        println!("  fragment frames: {} identical", frames_a.len());
    }

    streamed.scroll_to(0.0);
    let top = out.join("stack-top.png");
    std::fs::write(&top, streamed.capture()).expect("write PNG");
    written.insert(0, top);
    println!("  pixels: {identical}/{positions} scroll positions identical");
    for path in written {
        println!("  wrote {}", path.display());
    }
    passed
}

fn performance(stress: &str, kilobytes: usize, style_sheet: &Rc<StyleSheet>, mtm: MainThreadMarker) {
    let mut prefix = String::new();
    while prefix.len() < kilobytes * 1024 {
        prefix.push_str(stress);
        prefix.push_str("\n\n");
    }
    let mut messages: Vec<(bool, &str)> = OPENING.to_vec();
    messages.push((false, ""));
    let transcript = Transcript::new(style_sheet, &messages, mtm);
    let (view, storage) = transcript.last();
    let started = Instant::now();
    storage.beginEditing();
    storage.replaceCharactersInRange_withString(objc2_foundation::NSRange::new(0, storage.length()), &NSString::from_str(&prefix));
    storage.endEditing();
    view.update(MarkdownParser::parse(&prefix), &DirtySet::wholesale(), true);
    view.prepare_for_display();
    transcript.window.displayIfNeeded();
    println!("  whole {} KB message: update + first frame {:.1} ms", prefix.len() / 1024, ms(started.elapsed()));
    settle();
    transcript.follows_bottom.set(true);
    transcript.restack();
    // The host's first frame at the end of the message, which the stream
    // then extends.
    let started = Instant::now();
    view.prepare_for_display();
    transcript.window.displayIfNeeded();
    println!("  first frame at the end of the message: {:.1} ms", ms(started.elapsed()));
    view.set_streaming(true);

    let (mut update, mut layout, mut draw, mut total) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut slowest: Vec<(f64, usize, String, [f64; 3])> = Vec::new();
    for (index, (piece, fresh, dirty)) in parse_ahead(prefix.clone(), chunks(stress)).into_iter().enumerate() {
        let length = storage.length();
        let t0 = Instant::now();
        storage.beginEditing();
        storage.replaceCharactersInRange_withString(objc2_foundation::NSRange::new(length, 0), &NSString::from_str(&piece));
        storage.endEditing();
        view.update(fresh, &dirty, true);
        let t1 = Instant::now();
        view.prepare_for_display();
        let t2 = Instant::now();
        transcript.window.displayIfNeeded();
        let t3 = Instant::now();
        update.push(ms(t1 - t0));
        layout.push(ms(t2 - t1));
        draw.push(ms(t3 - t2));
        total.push(ms(t3 - t0));
        slowest.push((ms(t3 - t0), index, piece, [ms(t1 - t0), ms(t2 - t1), ms(t3 - t2)]));
        spin(0.0);
    }
    view.set_streaming(false);
    settle();
    slowest.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (time, index, piece, parts) in slowest.iter().take(5) {
        println!("  slow append #{index} {piece:?}: {time:.2} ms (update {:.2}, prepare {:.2}, draw {:.2})", parts[0], parts[1], parts[2]);
    }
    println!("  hosted, {} appends, message now {} UTF-16 units:", total.len(), view.parsed_document().length);
    report("update (edit, restack, scroll)", &mut update);
    report("prepare_for_display", &mut layout);
    report("window draw", &mut draw);
    report("total per append", &mut total);
}

/// Step 3 on Downright's document path, for comparison.
fn baseline(args: &Args, mtm: MainThreadMarker) -> bool {
    let stress = std::fs::read_to_string(&args.doc).expect("--doc");
    let mut prefix = String::new();
    while prefix.len() < args.kilobytes * 1024 {
        prefix.push_str(&stress);
        prefix.push_str("\n\n");
    }
    let appearance = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).expect("aqua");
    let style_sheet = Rc::new(StyleSheet::new(Theme::fallback(), &appearance, Some(true)));
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WINDOW_WIDTH, WINDOW_HEIGHT));
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
    window.setFrameOrigin(NSPoint::new(-30000.0, -30000.0));
    let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), frame);
    scroll.setHasVerticalScroller(true);
    let storage: Retained<NSTextStorage> =
        unsafe { msg_send![NSTextStorage::alloc(), initWithString: &*NSString::from_str(&prefix)] };
    let view = MarkdownTextView::new(frame, &storage, style_sheet, mtm);
    scroll.setDocumentView(Some(&view));
    window.setContentView(Some(&scroll));
    window.orderFrontRegardless();
    view.set_mode(RenderMode::Read);
    let started = Instant::now();
    view.update(MarkdownParser::parse(&prefix), &DirtySet::wholesale(), true);
    view.prepare_for_display();
    window.displayIfNeeded();
    println!("baseline: whole {} KB document {:.1} ms", prefix.len() / 1024, ms(started.elapsed()));
    spin(0.3);
    let (mut update, mut layout, mut draw, mut total) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (piece, fresh, dirty) in parse_ahead(prefix.clone(), chunks(&stress)) {
        let length = storage.length();
        let t0 = Instant::now();
        storage.beginEditing();
        storage.replaceCharactersInRange_withString(objc2_foundation::NSRange::new(length, 0), &NSString::from_str(&piece));
        storage.endEditing();
        view.update(fresh, &dirty, true);
        let t1 = Instant::now();
        view.prepare_for_display();
        let t2 = Instant::now();
        window.displayIfNeeded();
        let t3 = Instant::now();
        update.push(ms(t1 - t0));
        layout.push(ms(t2 - t1));
        draw.push(ms(t3 - t2));
        total.push(ms(t3 - t0));
        spin(0.0);
    }
    println!("baseline (document path, Read mode), {} appends:", total.len());
    report("update (edit included)", &mut update);
    report("prepare_for_display", &mut layout);
    report("window draw", &mut draw);
    report("total per append", &mut total);
    true
}
