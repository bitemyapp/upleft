//! Port of `Sources/DownrightQL/PreviewViewController.swift`: the Quick Look
//! preview extension (§10).
//!
//! Not a reduced-fidelity fallback: it draws with the same `upleft-render`
//! view layer the app draws with, so it *cannot* drift from the app's
//! rendering. That is the entire reason §3.3 forbids a WebView: a
//! `WKWebView` carrying KaTeX and Mermaid.js would not reliably fit under the
//! extension's hard ~120MB ceiling, and an extension that exceeds it is
//! killed outright.
//!
//! `PreviewViewController` is a `define_class!` `NSViewController` subclass
//! whose Objective-C name is the Swift class's, and it conforms to
//! `QLPreviewingController` (declared here: there is no objc2 binding for
//! QuickLookUI). The extension's `NSExtensionPrincipalClass` names it.
//!
//! How Swift's concurrency maps (`preparePreviewOfFile(at:completionHandler:)`):
//!
//! * the main-actor `Task { [weak self] … }` is a block on the main queue,
//!   cancelled through a shared flag ([`PreviewTask`]);
//! * the detached, user-initiated load is a block on the user-initiated
//!   global queue. Its `Task.isCancelled` reads the same flag, which is what
//!   `withTaskCancellationHandler`'s `load.cancel()` amounts to;
//! * `await MainActor.run { … }` is the hop back to the main queue.
//!
//! One deliberate strictness (AGENTS.md: never block the main thread with
//! parsing, and be at least as strict as Swift where it is lax): Swift parses
//! the document on the main actor inside `present` and `presentTruncated`.
//! Upleft runs the same `MarkdownParser` calls, with the same inputs and in
//! the same order, on the load's background queue, and hands the parsed
//! documents to `present` on the main thread. The parser is pure, so nothing
//! observable changes except that Finder's main thread stays free.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use block2::{DynBlock, RcBlock};
use dispatch2::{DispatchQoS, DispatchQueue, GlobalQueueIdentifier, MainThreadBound};
use objc2::rc::{Allocated, Retained, Weak as ObjcWeak};
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, extern_protocol, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAppearanceCustomization, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSBeep, NSBezelStyle,
    NSButton, NSCellImagePosition, NSColor, NSControlSize, NSEvent, NSEventMask, NSEventModifierFlags, NSFont,
    NSFontWeightMedium, NSFontWeightRegular, NSImage, NSImageSymbolConfiguration, NSImageView, NSLayoutAttribute,
    NSLayoutConstraint, NSLayoutConstraintOrientation, NSLayoutPriorityDefaultLow, NSLineBreakMode, NSResponder,
    NSScrollView, NSScrollerStyle, NSStackView, NSTextField, NSTextStorage, NSTextView,
    NSUserInterfaceLayoutOrientation, NSView, NSViewBoundsDidChangeNotification, NSViewController,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectView, NSWorkspace,
};
use objc2_core_foundation::CGFloat;
use objc2_foundation::{
    NSArray, NSBundle, NSCocoaErrorDomain, NSCopying, NSEdgeInsets, NSError, NSLocale, NSNotification, NSNotificationCenter,
    NSNumber, NSNumberFormatter, NSNumberFormatterStyle, NSOperationQueue, NSRange, NSRunLoop, NSRunLoopCommonModes,
    NSSize, NSString, NSTimer, NSURL, NSURLFileSizeKey,
};
use upleft_core::metrics::Metrics;
use upleft_core::parser::MarkdownParser;
use upleft_core::structural_zoom::StructuralZoom;
use upleft_core::{DirtySet, ParseOptions, ParsedDocument};
use upleft_render::appkit_compat::{main_after, main_async, ns_string, rect};
use upleft_render::render_contracts::{RenderMode, Theme, ThemeAppearance};
use upleft_render::swift_compat::{smax, smin, string_eq};
use upleft_render::theme::preview_appearance::{PreviewAppearance, PreviewAppearanceStore};
use upleft_render::theme::style_sheet::StyleSheet;
use upleft_render::theme::theme_store::ThemeStore;
use upleft_render::view::density_gutter_view::{DensityGutterDelegate, DensityGutterView};
use upleft_render::view::density_outline_window::DensityOutlineEntry;
use upleft_render::view::markdown_container_view::MarkdownContainerView;
use upleft_render::view::markdown_text_view_delegate::ScrollPosition;
use upleft_swift_text as swift_text;

use crate::quick_look_loader::{QuickLookLoadedContent, QuickLookLoader};
use crate::quick_look_policy::QuickLookPolicy;

// QuickLookUI defines the `QLPreviewingController` protocol; linking it is
// what makes the protocol (and so the conformance below) exist at run time.
#[link(name = "QuickLookUI", kind = "framework")]
unsafe extern "C" {}

extern_protocol!(
    /// `QLPreviewingController` (QuickLookUI): the one method this
    /// controller implements.
    ///
    /// # Safety
    ///
    /// The method keeps QuickLookUI's signature.
    pub unsafe trait QLPreviewingController: NSObjectProtocol {
        #[optional]
        #[unsafe(method(preparePreviewOfFileAtURL:completionHandler:))]
        #[unsafe(method_family = none)]
        #[allow(non_snake_case)]
        fn preparePreviewOfFileAtURL_completionHandler(&self, url: &NSURL, handler: &DynBlock<dyn Fn(*mut NSError)>);
    }
);

/// `CocoaError.Code` values the controller reports.
const CODER_INVALID_VALUE: isize = 4866;
const USER_CANCELLED: isize = 3072;
const FILE_READ_CORRUPT_FILE: isize = 259;

/// `CocoaError(code)` as it bridges to `NSError`: the Cocoa domain, no user
/// info.
fn cocoa_error(code: isize) -> Retained<NSError> {
    // SAFETY: `NSCocoaErrorDomain` is an immutable Foundation global.
    unsafe { NSError::errorWithDomain_code_userInfo(NSCocoaErrorDomain, code, None) }
}

/// Calls a Quick Look completion handler with an optional error.
fn complete(handler: &RcBlock<dyn Fn(*mut NSError)>, error: Option<Retained<NSError>>) {
    handler.call((error.as_ref().map_or(std::ptr::null_mut(), |error| Retained::as_ptr(error) as *mut NSError),));
}

fn ns(text: &str) -> Retained<NSString> {
    NSString::from_str(text)
}

/// `Int.formatted()`: the integer format style in the current locale, which
/// is `NumberFormatter`'s decimal style (checked against Swift in en_US,
/// de_DE, fr_FR, es_ES, hi_IN, ar_EG and pl_PL).
fn formatted(value: isize) -> String {
    let formatter = NSNumberFormatter::new();
    formatter.setNumberStyle(NSNumberFormatterStyle::DecimalStyle);
    formatter.setLocale(Some(&NSLocale::autoupdatingCurrentLocale()));
    formatter
        .stringFromNumber(&NSNumber::new_isize(value))
        .map(|string| string.to_string())
        .unwrap_or_else(|| value.to_string())
}

/// `private enum LoadResult`, carrying the parsed documents `present` needs
/// (see the module notes on where the parse runs).
enum LoadResult {
    Full { text: String, document: Arc<ParsedDocument> },
    Prefix { prefix: String, document: Arc<ParsedDocument> },
    Failure,
}

/// The cancellation state of one `previewTask`. `cancel()` is
/// `previewTask?.cancel()`: it cancels the task and, through the
/// cancellation handler, its detached load.
#[derive(Clone)]
pub struct PreviewTask(Arc<AtomicBool>);

impl PreviewTask {
    fn new() -> PreviewTask {
        PreviewTask(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// `Task.isCancelled`.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// A Quick Look file URL on its way to the load queue. `URL` is `Sendable`
/// in Swift; `NSURL` is immutable and thread-safe.
struct SendUrl(Retained<NSURL>);

// SAFETY: NSURL is an immutable, thread-safe Foundation value class.
unsafe impl Send for SendUrl {}

pub struct PreviewViewControllerIvars {
    storage: Retained<NSTextStorage>,
    container: RefCell<Option<Retained<MarkdownContainerView>>>,
    density_gutter: RefCell<Option<Retained<DensityGutterView>>>,
    parsed_document: RefCell<Option<Arc<ParsedDocument>>>,
    fallback_text_view: RefCell<Option<Retained<NSTextView>>>,
    source_url: RefCell<Option<Retained<NSURL>>>,

    content_bottom_constraint: RefCell<Option<Retained<NSLayoutConstraint>>>,
    notice_bar: RefCell<Option<Retained<NSView>>>,
    scroll_observer: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
    key_monitor: RefCell<Option<Retained<AnyObject>>>,
    current_heading_index: Cell<Option<isize>>,

    /// Quick Look's host process already owns AppKit, TextKit, and extension
    /// infrastructure before our document is presented. Budget the preview's
    /// incremental footprint, not the host's unrelated baseline, or every
    /// normal preview falls back to raw source a moment after rendering.
    memory_baseline_bytes: Cell<isize>,

    memory_timer: RefCell<Option<Retained<NSTimer>>>,
    preview_task: RefCell<Option<PreviewTask>>,
    preview_generation: Cell<usize>,

    /// `gutter.delegate = self`: the density gutter holds its delegate
    /// weakly, so the controller owns the proxy that forwards to it.
    gutter_delegate: RefCell<Option<Rc<PreviewGutterDelegate>>>,
}

impl PreviewViewControllerIvars {
    fn new() -> PreviewViewControllerIvars {
        PreviewViewControllerIvars {
            storage: NSTextStorage::new(),
            container: RefCell::new(None),
            density_gutter: RefCell::new(None),
            parsed_document: RefCell::new(None),
            fallback_text_view: RefCell::new(None),
            source_url: RefCell::new(None),
            content_bottom_constraint: RefCell::new(None),
            notice_bar: RefCell::new(None),
            scroll_observer: RefCell::new(None),
            key_monitor: RefCell::new(None),
            current_heading_index: Cell::new(None),
            memory_baseline_bytes: Cell::new(0),
            memory_timer: RefCell::new(None),
            preview_task: RefCell::new(None),
            preview_generation: Cell::new(0),
            gutter_delegate: RefCell::new(None),
        }
    }
}

impl Drop for PreviewViewControllerIvars {
    /// `deinit`.
    fn drop(&mut self) {
        if let Some(task) = self.preview_task.get_mut().as_ref() {
            task.cancel();
        }
        if let Some(timer) = self.memory_timer.get_mut().as_ref() {
            timer.invalidate();
        }
        if let Some(observer) = self.scroll_observer.get_mut().take() {
            // SAFETY: the token `addObserverForName:object:queue:usingBlock:`
            // returned.
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(observer.as_ref()) };
        }
        if let Some(monitor) = self.key_monitor.get_mut().take() {
            // SAFETY: the token `addLocalMonitorForEventsMatchingMask:handler:`
            // returned.
            unsafe { NSEvent::removeMonitor(&monitor) };
        }
    }
}

define_class!(
    /// `final class PreviewViewController: NSViewController, QLPreviewingController`.
    // SAFETY: the designated initialiser sets the ivars before forwarding to
    // NSViewController's; overrides keep AppKit's signatures. Drop is on the
    // ivars only.
    #[unsafe(super(NSViewController, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "PreviewViewController"]
    #[ivars = PreviewViewControllerIvars]
    pub struct PreviewViewController;

    unsafe impl NSObjectProtocol for PreviewViewController {}

    impl PreviewViewController {
        /// NSViewController's designated initialiser. Quick Look creates the
        /// principal class from Objective-C (`init` forwards here), so the
        /// ivars are set in the override rather than in a Rust constructor.
        #[unsafe(method_id(initWithNibName:bundle:))]
        fn __init_with_nib_name(
            this: Allocated<Self>,
            nib_name: Option<&NSString>,
            bundle: Option<&NSBundle>,
        ) -> Option<Retained<Self>> {
            let this = this.set_ivars(PreviewViewControllerIvars::new());
            // SAFETY: NSViewController's designated initialiser.
            unsafe { msg_send![super(this), initWithNibName: nib_name, bundle: bundle] }
        }

        #[unsafe(method(loadView))]
        fn __load_view(&self) {
            self.load_view();
        }

        #[unsafe(method(viewDidLayout))]
        fn __view_did_layout(&self) {
            // SAFETY: the override calls through to NSViewController.
            let () = unsafe { msg_send![super(self), viewDidLayout] };
            self.update_density_gutter_visibility();
            self.update_density_gutter_state();
        }

        #[unsafe(method(openInApp:))]
        fn __open_in_app(&self, sender: &NSButton) {
            self.open_in_app(sender);
        }
    }

    unsafe impl QLPreviewingController for PreviewViewController {
        #[unsafe(method(preparePreviewOfFileAtURL:completionHandler:))]
        fn __prepare_preview_of_file(&self, url: &NSURL, handler: &DynBlock<dyn Fn(*mut NSError)>) {
            self.prepare_preview_of_file(url, handler.copy());
        }
    }
);

/// `PreviewViewController.urlKey`: its address is the association key.
static URL_KEY: u8 = 0;

impl PreviewViewController {
    /// `PreviewViewController()`: `init`, which NSViewController forwards to
    /// `initWithNibName:bundle:` with no nib.
    pub fn new(mtm: MainThreadMarker) -> Retained<PreviewViewController> {
        // SAFETY: NSViewController routes `init` to the designated
        // initialiser overridden above.
        unsafe { msg_send![Self::alloc(mtm), init] }
    }

    /// `previewAppearance`.
    fn preview_appearance(&self) -> PreviewAppearance {
        PreviewAppearanceStore::appearance()
    }

    /// `loadView()`.
    fn load_view(&self) {
        let mtm = MainThreadMarker::from(self);
        let view = NSView::initWithFrame(NSView::alloc(mtm), rect(0.0, 0.0, 720.0, 800.0));
        self.setView(&view);
        // nil preserves the host's native macOS appearance. An explicit user
        // choice is applied only to this preview surface, never globally.
        self.view().setAppearance(self.preview_appearance().ns_appearance().as_deref());
    }

    // MARK: - QLPreviewingController

    /// `preparePreviewOfFile(at:completionHandler:)`.
    pub fn prepare_preview_of_file(&self, url: &NSURL, handler: RcBlock<dyn Fn(*mut NSError)>) {
        let ivars = self.ivars();
        if let Some(task) = ivars.preview_task.borrow().as_ref() {
            task.cancel();
        }
        ivars.preview_generation.set(ivars.preview_generation.get().wrapping_add(1));
        let generation = ivars.preview_generation.get();

        let task = PreviewTask::new();
        let weak_self: ObjcWeak<PreviewViewController> = ObjcWeak::from(self);
        let url = url.copy();
        let token = task.clone();
        // `Task { [weak self] in … }` inherits the main actor: it starts on a
        // later turn of the main queue.
        main_async(move || Self::run_preview_task(weak_self, token, generation, url, handler));
        *ivars.preview_task.borrow_mut() = Some(task);
    }

    /// The body of the preview task, up to `await load.value`.
    fn run_preview_task(
        weak_self: ObjcWeak<PreviewViewController>,
        task: PreviewTask,
        generation: usize,
        url: Retained<NSURL>,
        handler: RcBlock<dyn Fn(*mut NSError)>,
    ) {
        let Some(this) = weak_self.load() else {
            complete(&handler, Some(cocoa_error(CODER_INVALID_VALUE)));
            return;
        };
        let mtm = MainThreadMarker::from(&*this);

        // Finder owns the main thread that presents this controller. File
        // coordination and decoding must not occupy it, especially for the
        // multi-megabyte prefix path. The load runs detached (off the main
        // actor), and cancellation is forwarded to it: a superseded or
        // dismissed preview stops paying for bytes nobody will present.
        let load_task = task.clone();
        let load_url = SendUrl(url.clone());
        let then = MainThreadBound::new(
            Box::new(move |load_result: LoadResult| this.finish_preview(&task, generation, &url, &handler, load_result))
                as Box<dyn FnOnce(LoadResult)>,
            mtm,
        );
        DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(DispatchQoS::UserInitiated)).exec_async(
            move || {
                let load_result = Self::load(&load_task, load_url);
                DispatchQueue::main().exec_async(move || {
                    let mtm = MainThreadMarker::new().expect("the main queue runs on the main thread");
                    (then.into_inner(mtm))(load_result);
                });
            },
        );
    }

    /// The detached load (`Task.detached(priority: .userInitiated)`), plus
    /// the parses `present` and `presentTruncated` run on its result.
    fn load(task: &PreviewTask, url: SendUrl) -> LoadResult {
        if task.is_cancelled() {
            return LoadResult::Failure;
        }
        let url = url.0;
        // `(try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0`.
        // SAFETY: `NSURLFileSizeKey` is an immutable Foundation global.
        let file_size_key = unsafe { NSURLFileSizeKey };
        let byte_count = url
            .resourceValuesForKeys_error(&NSArray::from_slice(&[file_size_key]))
            .ok()
            .and_then(|values| values.objectForKey(file_size_key))
            .and_then(|value| value.downcast::<NSNumber>().ok())
            .map_or(0, |number| number.integerValue());
        let Some(path) = url.path().map(|path| PathBuf::from(path.to_string())) else {
            return LoadResult::Failure;
        };
        match QuickLookLoader::load(&path, byte_count, None) {
            Some(QuickLookLoadedContent::Prefix(text)) => {
                if task.is_cancelled() {
                    return LoadResult::Failure;
                }
                let (prefix, document) = Self::parse_truncated(&text);
                LoadResult::Prefix { prefix, document }
            }
            Some(QuickLookLoadedContent::Full(text)) => {
                if task.is_cancelled() {
                    return LoadResult::Failure;
                }
                let document = MarkdownParser::parse(&text);
                LoadResult::Full { text, document }
            }
            None => LoadResult::Failure,
        }
    }

    /// The part of `presentTruncated(_:url:)` that runs before `present`:
    /// the structure-only parse, the block cutoff, the bounded prefix, and
    /// then `present`'s own parse of that prefix.
    fn parse_truncated(text: &str) -> (String, Arc<ParsedDocument>) {
        let document = MarkdownParser::parse_with(text, ParseOptions::STRUCTURE_ONLY);
        let cutoff = document
            .root
            .children
            .iter()
            .take(QuickLookPolicy::PREFIX_BLOCK_COUNT as usize)
            .last()
            .map(|block| block.range.upper_bound())
            .unwrap_or_else(|| text.encode_utf16().count() as isize);
        let prefix = Self::bounded_prefix(
            text,
            cutoff.min(QuickLookPolicy::PREFIX_RENDER_LIMIT_UTF16),
            QuickLookPolicy::PREFIX_RENDER_LIMIT_BYTES,
        );
        let document = MarkdownParser::parse(&prefix);
        (prefix, document)
    }

    /// After `await load.value`, back on the main actor.
    fn finish_preview(
        &self,
        task: &PreviewTask,
        generation: usize,
        url: &NSURL,
        handler: &RcBlock<dyn Fn(*mut NSError)>,
        load_result: LoadResult,
    ) {
        if task.is_cancelled() {
            complete(handler, Some(cocoa_error(USER_CANCELLED)));
            return;
        }

        // `await MainActor.run { … }`.
        if task.is_cancelled() || self.ivars().preview_generation.get() != generation {
            complete(handler, Some(cocoa_error(USER_CANCELLED)));
            return;
        }

        // Quick Look may reuse this controller for another file after the
        // user changes the setting in Upleft. Re-resolve the explicit
        // override for every preview instead of pinning the appearance from
        // the first file shown in this process.
        self.view().setAppearance(self.preview_appearance().ns_appearance().as_deref());
        self.reset_preview();
        match load_result {
            LoadResult::Prefix { prefix, document } => self.present_truncated(prefix, document, url),
            LoadResult::Full { text, document } => self.present(&text, document, url),
            LoadResult::Failure => {
                complete(handler, Some(cocoa_error(FILE_READ_CORRUPT_FILE)));
                return;
            }
        }
        self.start_memory_watch();
        if self.ivars().preview_generation.get() == generation {
            *self.ivars().preview_task.borrow_mut() = None;
        }
        complete(handler, None);
    }

    // MARK: - Rendering

    /// `present(_:url:)`, with `MarkdownParser.parse(text)` already done off
    /// the main thread.
    fn present(&self, text: &str, document: Arc<ParsedDocument>, url: &NSURL) {
        let mtm = MainThreadMarker::from(self);
        let ivars = self.ivars();
        let storage = &ivars.storage;
        storage.replaceCharactersInRange_withString(NSRange::new(0, storage.length()), &ns_string(text));
        *ivars.parsed_document.borrow_mut() = Some(document.clone());
        *ivars.source_url.borrow_mut() = Some(url.copy());

        let container = MarkdownContainerView::with_storage(storage, mtm);
        container.text_view().set_style_sheet(Rc::new(self.make_style_sheet()));
        container.text_view().set_mode(RenderMode::Read);
        container.text_view().update(document.clone(), &DirtySet::wholesale(), true);
        // Selectable and copyable — most Quick Look previews are dead
        // surfaces, and this one isn't (§10).
        container.text_view().setSelectable(true);

        let gutter = self.make_density_gutter(&document, container.text_view().style_sheet());

        self.install(&container);
        *ivars.container.borrow_mut() = Some(container);
        *ivars.density_gutter.borrow_mut() = Some(gutter);
        self.update_density_gutter_visibility();
        self.start_interaction_observation();
        let weak_self: ObjcWeak<PreviewViewController> = ObjcWeak::from(self);
        main_async(move || {
            if let Some(this) = weak_self.load() {
                this.update_density_gutter_state();
            }
        });
    }

    /// `presentTruncated(_:url:)`, with its parses already done off the main
    /// thread (`parse_truncated`).
    fn present_truncated(&self, prefix: String, document: Arc<ParsedDocument>, url: &NSURL) {
        self.present(&prefix, document, url);
        self.install_open_in_app_bar(url, &format!("Showing the first {} blocks", QuickLookPolicy::PREFIX_BLOCK_COUNT));
    }

    /// Keep the rendered prefix bounded in both Foundation's UTF-16
    /// coordinate space and its UTF-8 storage size. Walking character
    /// boundaries avoids returning a string with a split surrogate when a
    /// limit lands mid-scalar.
    ///
    /// `boundedPrefix(_:utf16Limit:byteLimit:)`: steps by `Character`
    /// (`text.index(after:)`), so a grapheme cluster is kept or dropped whole.
    pub fn bounded_prefix(text: &str, utf16_limit: isize, byte_limit: isize) -> String {
        if !(utf16_limit > 0 && byte_limit > 0) {
            return String::new();
        }
        let mut utf16_count: isize = 0;
        let mut byte_count: isize = 0;
        let mut end = 0;
        for character in swift_text::graphemes(text) {
            let character_utf16_count = character.encode_utf16().count() as isize;
            let character_byte_count = character.len() as isize;
            if !(utf16_count + character_utf16_count <= utf16_limit && byte_count + character_byte_count <= byte_limit) {
                break;
            }
            utf16_count += character_utf16_count;
            byte_count += character_byte_count;
            end += character.len();
        }
        text[..end].to_owned()
    }

    /// Plain text is the floor, not a failure: a preview that renders nothing
    /// is worse than a preview that renders the source (§10).
    fn fall_back_to_plain_text(&self) {
        let mtm = MainThreadMarker::from(self);
        let ivars = self.ivars();
        if ivars.fallback_text_view.borrow().is_some() {
            return;
        }
        if let Some(timer) = ivars.memory_timer.take() {
            timer.invalidate();
        }
        self.stop_interaction_observation();
        let container = ivars.container.borrow().clone();
        let style_sheet = match container {
            Some(container) => container.text_view().style_sheet(),
            None => Rc::new(self.make_style_sheet()),
        };
        for subview in self.view().subviews().iter() {
            subview.removeFromSuperview();
        }
        *ivars.container.borrow_mut() = None;
        *ivars.density_gutter.borrow_mut() = None;
        *ivars.parsed_document.borrow_mut() = None;
        *ivars.notice_bar.borrow_mut() = None;
        *ivars.content_bottom_constraint.borrow_mut() = None;
        ivars.current_heading_index.set(None);

        let text_view = NSTextView::new(mtm);
        text_view.setString(&ivars.storage.string());
        text_view.setEditable(false);
        text_view.setSelectable(true);
        // SAFETY: `NSFontWeightRegular` is an immutable AppKit global.
        text_view.setFont(Some(&NSFont::monospacedSystemFontOfSize_weight(13.0, unsafe { NSFontWeightRegular })));
        text_view.setTextColor(Some(&style_sheet.text));
        text_view.setBackgroundColor(&style_sheet.background);
        text_view.setTextContainerInset(NSSize::new(24.0, 20.0));

        let scroll = NSScrollView::new(mtm);
        scroll.setDocumentView(Some(&text_view));
        scroll.setHasVerticalScroller(true);
        scroll.setScrollerStyle(NSScrollerStyle::Overlay);
        scroll.setDrawsBackground(true);
        scroll.setBackgroundColor(&style_sheet.background);
        self.install(&scroll);
        *ivars.fallback_text_view.borrow_mut() = Some(text_view);
        let source_url = ivars.source_url.borrow().clone();
        if let Some(source_url) = source_url {
            self.install_open_in_app_bar(&source_url, "Plain text preview — open in Upleft for full rendering");
        }
    }

    /// Quick Look has its own bundle and defaults domain. Resolve the palette
    /// from the shared setting, falling back to the bundled System theme so a
    /// dark macOS desktop can never inherit the app's old Paper Light default.
    fn make_style_sheet(&self) -> StyleSheet {
        let mode = self.preview_appearance();
        let appearance = mode.ns_appearance().unwrap_or_else(|| self.view().effectiveAppearance());
        let preferred_name = PreviewAppearanceStore::theme_name(&appearance);
        let theme: Theme = preferred_name
            .and_then(|name| ThemeStore::shared().themes().into_iter().find(|theme| string_eq(&theme.name, &name)))
            .or_else(|| {
                ThemeStore::shared().themes().into_iter().find(|theme| match mode {
                    PreviewAppearance::Dark => theme.appearance == ThemeAppearance::Dark,
                    PreviewAppearance::Light => theme.appearance != ThemeAppearance::Dark,
                    PreviewAppearance::System => {
                        // SAFETY: AppKit exports the appearance names as
                        // immutable globals.
                        let (aqua, dark_aqua) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
                        let is_dark = appearance
                            .bestMatchFromAppearancesWithNames(&NSArray::from_slice(&[aqua, dark_aqua]))
                            .is_some_and(|best| &*best == dark_aqua);
                        if is_dark {
                            theme.appearance == ThemeAppearance::Dark
                        } else {
                            theme.appearance != ThemeAppearance::Dark
                        }
                    }
                })
            })
            .unwrap_or_else(Theme::fallback);
        StyleSheet::new(theme, &appearance, None)
    }

    /// Quick Look reuses one controller for multiple files. Release every
    /// prior surface and its backing render graph before installing the next
    /// document; replacing the text storage alone leaves old views, timers,
    /// constraints, and layout fragments reachable.
    fn reset_preview(&self) {
        let ivars = self.ivars();
        if let Some(timer) = ivars.memory_timer.take() {
            timer.invalidate();
        }
        self.stop_interaction_observation();
        for subview in self.view().subviews().iter() {
            subview.removeFromSuperview();
        }
        *ivars.container.borrow_mut() = None;
        *ivars.density_gutter.borrow_mut() = None;
        *ivars.parsed_document.borrow_mut() = None;
        *ivars.fallback_text_view.borrow_mut() = None;
        *ivars.source_url.borrow_mut() = None;
        *ivars.notice_bar.borrow_mut() = None;
        *ivars.content_bottom_constraint.borrow_mut() = None;
        ivars.current_heading_index.set(None);
        let storage = &ivars.storage;
        if storage.length() > 0 {
            storage.replaceCharactersInRange_withString(NSRange::new(0, storage.length()), &ns(""));
        }
    }

    /// `install(_:)`.
    fn install(&self, subview: &NSView) {
        let view = self.view();
        subview.setTranslatesAutoresizingMaskIntoConstraints(false);
        view.addSubview(subview);
        let bottom = subview.bottomAnchor().constraintEqualToAnchor(&view.bottomAnchor());
        *self.ivars().content_bottom_constraint.borrow_mut() = Some(bottom.clone());
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
            subview.leadingAnchor().constraintEqualToAnchor(&view.leadingAnchor()),
            subview.trailingAnchor().constraintEqualToAnchor(&view.trailingAnchor()),
            subview.topAnchor().constraintEqualToAnchor(&view.topAnchor()),
            bottom,
        ]));
    }

    /// `installOpenInAppBar(for:note:)`.
    fn install_open_in_app_bar(&self, url: &NSURL, note: &str) {
        let mtm = MainThreadMarker::from(self);
        let ivars = self.ivars();
        let previous_bar = ivars.notice_bar.borrow().clone();
        if let Some(previous_bar) = previous_bar {
            previous_bar.removeFromSuperview();
        }

        let icon = NSImageView::imageViewWithImage(
            &NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns("doc.text.magnifyingglass"), None)
                .unwrap_or_else(NSImage::new),
            mtm,
        );
        // SAFETY: `NSFontWeightMedium` is an immutable AppKit global.
        icon.setSymbolConfiguration(Some(&NSImageSymbolConfiguration::configurationWithPointSize_weight(
            13.0,
            unsafe { NSFontWeightMedium },
        )));
        icon.setContentTintColor(Some(&NSColor::secondaryLabelColor()));
        icon.setAccessibilityHidden(true);

        let label = NSTextField::labelWithString(&ns(note), mtm);
        // SAFETY: as above.
        label.setFont(Some(&NSFont::systemFontOfSize_weight(12.0, unsafe { NSFontWeightMedium })));
        label.setTextColor(Some(&NSColor::secondaryLabelColor()));
        label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
        label.setContentCompressionResistancePriority_forOrientation(
            NSLayoutPriorityDefaultLow,
            NSLayoutConstraintOrientation::Horizontal,
        );

        // SAFETY: `openInApp:` is this class's action method; a button does
        // not retain its target, and the controller owns the button's view
        // hierarchy.
        let button = unsafe {
            NSButton::buttonWithTitle_target_action(
                &ns("Open in Upleft"),
                Some(self as &AnyObject),
                Some(sel!(openInApp:)),
                mtm,
            )
        };
        // `.rounded`, which the SDK now spells `.push` (the same value).
        button.setBezelStyle(NSBezelStyle::Push);
        button.setControlSize(NSControlSize::Small);
        button.setImage(
            NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns("arrow.up.forward.app"), None).as_deref(),
        );
        button.setImagePosition(NSCellImagePosition::ImageLeading);
        // SAFETY: both are live Objective-C objects; the key is a static's
        // address.
        unsafe {
            objc2::ffi::objc_setAssociatedObject(
                &*button as *const NSButton as *mut AnyObject,
                &URL_KEY as *const u8 as *const c_void,
                url as *const NSURL as *mut AnyObject,
                objc2::ffi::OBJC_ASSOCIATION_RETAIN,
            );
        }

        let spacer = NSView::new(mtm);
        let bar = NSStackView::stackViewWithViews(
            &NSArray::from_slice(&[&*icon as &NSView, &*label as &NSView, &*spacer, &*button as &NSView]),
            mtm,
        );
        bar.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        bar.setAlignment(NSLayoutAttribute::CenterY);
        bar.setSpacing(8.0);
        bar.setEdgeInsets(NSEdgeInsets { top: 8.0, left: 14.0, bottom: 8.0, right: 14.0 });
        bar.setTranslatesAutoresizingMaskIntoConstraints(false);

        let background = NSVisualEffectView::new(mtm);
        background.setMaterial(NSVisualEffectMaterial::HeaderView);
        background.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
        background.setTranslatesAutoresizingMaskIntoConstraints(false);
        background.addSubview(&bar);
        *ivars.notice_bar.borrow_mut() = Some(Retained::into_super(background.clone()));

        let view = self.view();
        view.addSubview(&background);
        let content_bottom = ivars.content_bottom_constraint.borrow().clone();
        if let Some(content_bottom) = content_bottom {
            content_bottom.setActive(false);
        }
        let container = ivars.container.borrow().clone();
        let content_view: Option<Retained<NSView>> = match container {
            Some(container) => Some(Retained::into_super(container)),
            None => {
                let fallback_text_view = ivars.fallback_text_view.borrow().clone();
                fallback_text_view
                    .and_then(|text_view| text_view.enclosingScrollView())
                    .map(|scroll| Retained::into_super(scroll))
            }
        };
        if let Some(content_view) = content_view {
            let reserved_bottom = content_view.bottomAnchor().constraintEqualToAnchor(&background.topAnchor());
            reserved_bottom.setActive(true);
            *ivars.content_bottom_constraint.borrow_mut() = Some(reserved_bottom);
        }
        NSLayoutConstraint::activateConstraints(&NSArray::from_retained_slice(&[
            background.leadingAnchor().constraintEqualToAnchor(&view.leadingAnchor()),
            background.trailingAnchor().constraintEqualToAnchor(&view.trailingAnchor()),
            background.bottomAnchor().constraintEqualToAnchor(&view.bottomAnchor()),
            bar.leadingAnchor().constraintEqualToAnchor(&background.leadingAnchor()),
            bar.trailingAnchor().constraintEqualToAnchor(&background.trailingAnchor()),
            bar.topAnchor().constraintEqualToAnchor(&background.topAnchor()),
            bar.bottomAnchor().constraintEqualToAnchor(&background.bottomAnchor()),
        ]));
    }

    /// `makeDensityGutter(for:styleSheet:)`.
    fn make_density_gutter(&self, document: &ParsedDocument, style_sheet: Rc<StyleSheet>) -> Retained<DensityGutterView> {
        let mtm = MainThreadMarker::from(self);
        let gutter = DensityGutterView::new(style_sheet, mtm);
        let delegate = Rc::new(PreviewGutterDelegate { controller: ObjcWeak::from(self) });
        gutter.set_delegate(Some(Rc::downgrade(&delegate) as Weak<dyn DensityGutterDelegate>));
        *self.ivars().gutter_delegate.borrow_mut() = Some(delegate);
        // Finder's compact Quick Look panel leaves no usable side margin for
        // a 320pt card. Opt into the rail's compact, translucent overlay
        // fallback so hover still communicates the target section instead of
        // vanishing.
        gutter.set_allows_preview_content_overlap(true);
        gutter.set_bands(DensityGutterView::bands_for(document, &[], &[]));

        let word_count = Metrics::document_word_count(document);
        let read_minutes = 1isize.max((word_count as f64 / Metrics::WORDS_PER_MINUTE).ceil() as isize);
        gutter.set_metrics_summary(format!(
            "{} words · {} characters · {} min read",
            formatted(word_count),
            formatted(document.length),
            read_minutes
        ));

        let length = 1isize.max(document.length) as CGFloat;
        gutter.set_outline_entries(
            document
                .headings
                .iter()
                .map(|heading| {
                    DensityOutlineEntry::new(
                        heading.title.clone(),
                        heading.level,
                        heading.range.location as CGFloat / length,
                        false,
                    )
                })
                .collect(),
        );
        gutter
    }

    /// `updateDensityGutterVisibility()`.
    fn update_density_gutter_visibility(&self) {
        let ivars = self.ivars();
        let container = ivars.container.borrow().clone();
        let density_gutter = ivars.density_gutter.borrow().clone();
        let (Some(container), Some(density_gutter)) = (container, density_gutter) else { return };
        let should_show = self.view().bounds().size.width >= QuickLookPolicy::MINIMUM_DENSITY_GUTTER_WIDTH;
        let gutter_view: &NSView = &density_gutter;
        let is_accessory =
            container.leading_accessory().is_some_and(|accessory| std::ptr::eq(&*accessory, gutter_view));
        if should_show && !is_accessory {
            container.set_leading_accessory(Some(Retained::into_super(Retained::into_super(density_gutter))));
        } else if !should_show && is_accessory {
            container.set_leading_accessory(None);
        }
    }

    /// `startInteractionObservation()`.
    fn start_interaction_observation(&self) {
        self.stop_interaction_observation();
        let container = self.ivars().container.borrow().clone();
        let Some(container) = container else { return };
        let clip_view = container.scroll_view().contentView();
        clip_view.setPostsBoundsChangedNotifications(true);
        let weak_self: ObjcWeak<PreviewViewController> = ObjcWeak::from(self);
        let block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
            if let Some(this) = weak_self.load() {
                this.update_density_gutter_state();
            }
        });
        // SAFETY: the block only runs on the main queue.
        let observer = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(NSViewBoundsDidChangeNotification),
                Some(&clip_view),
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
        };
        *self.ivars().scroll_observer.borrow_mut() = Some(observer);

        let weak_self: ObjcWeak<PreviewViewController> = ObjcWeak::from(self);
        let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            let Some(this) = weak_self.load() else { return event.as_ptr() };
            // SAFETY: AppKit hands the monitor a live event.
            let event_ref = unsafe { event.as_ref() };
            let event_window = event_ref.window(MainThreadMarker::from(&*this));
            let view_window = this.view().window();
            let same_window = match (&event_window, &view_window) {
                (Some(a), Some(b)) => std::ptr::eq(&**a, &**b),
                (None, None) => true,
                _ => false,
            };
            if !same_window {
                return event.as_ptr();
            }
            let modifiers = event_ref.modifierFlags() & NSEventModifierFlags::DeviceIndependentFlagsMask;
            if !(modifiers & (NSEventModifierFlags::Command | NSEventModifierFlags::Control | NSEventModifierFlags::Option))
                .is_empty()
            {
                return event.as_ptr();
            }
            let characters =
                event_ref.charactersIgnoringModifiers().map(|characters| swift_text::lowercased(&characters.to_string()));
            match characters.as_deref() {
                Some("n") => {
                    this.jump_to_heading(true);
                    std::ptr::null_mut()
                }
                Some("p") => {
                    this.jump_to_heading(false);
                    std::ptr::null_mut()
                }
                _ => event.as_ptr(),
            }
        });
        // SAFETY: the block returns the event or nil, as a local monitor must.
        let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block) };
        *self.ivars().key_monitor.borrow_mut() = monitor;
    }

    /// `stopInteractionObservation()`.
    fn stop_interaction_observation(&self) {
        let ivars = self.ivars();
        if let Some(observer) = ivars.scroll_observer.take() {
            // SAFETY: the token `addObserverForName:object:queue:usingBlock:`
            // returned.
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(observer.as_ref()) };
        }
        if let Some(monitor) = ivars.key_monitor.take() {
            // SAFETY: the token `addLocalMonitorForEventsMatchingMask:handler:`
            // returned.
            unsafe { NSEvent::removeMonitor(&monitor) };
        }
    }

    /// `updateDensityGutterState()`.
    fn update_density_gutter_state(&self) {
        let ivars = self.ivars();
        let container = ivars.container.borrow().clone();
        let document = ivars.parsed_document.borrow().clone();
        let density_gutter = ivars.density_gutter.borrow().clone();
        let (Some(container), Some(document), Some(density_gutter)) = (container, document, density_gutter) else {
            return;
        };
        let length = 1isize.max(document.length) as CGFloat;
        let top = smin(1.0, container.text_view().top_visible_offset() as CGFloat / length);
        let visible_height = container.scroll_view().contentView().bounds().size.height;
        let document_height = smax(
            1.0,
            container.scroll_view().documentView().map_or(1.0, |document_view| document_view.bounds().size.height),
        );
        let span = smin(1.0, visible_height / document_height);
        density_gutter.set_visible_range((top, smin(1.0, top + span)));
        density_gutter.set_read_progress(smax(density_gutter.read_progress(), smin(1.0, top + span)));

        // `lastIndex(where:)` walks backwards, evaluating the predicate (and
        // with it `topVisibleOffset`) once per heading it passes.
        let current = document
            .headings
            .iter()
            .rposition(|heading| heading.range.location <= container.text_view().top_visible_offset())
            .map(|index| index as isize);
        if current == ivars.current_heading_index.get() {
            return;
        }
        ivars.current_heading_index.set(current);
        density_gutter.set_outline_entries(
            density_gutter
                .outline_entries()
                .into_iter()
                .enumerate()
                .map(|(index, entry)| {
                    let mut updated = entry;
                    updated.is_current = Some(index as isize) == current;
                    updated
                })
                .collect(),
        );
    }

    /// `jumpToHeading(forward:)`.
    fn jump_to_heading(&self, forward: bool) {
        let ivars = self.ivars();
        let container = ivars.container.borrow().clone();
        let document = ivars.parsed_document.borrow().clone();
        let (Some(container), Some(document)) = (container, document) else {
            NSBeep();
            return;
        };
        if document.headings.is_empty() {
            NSBeep();
            return;
        }
        let top = container.text_view().top_visible_offset();
        let heading = if forward {
            document.headings.iter().find(|heading| heading.range.location > top + 1)
        } else {
            document.headings.iter().rev().find(|heading| heading.range.location < top - 1)
        };
        let Some(heading) = heading else {
            NSBeep();
            return;
        };
        container.text_view().scroll_to_offset(heading.range.location, ScrollPosition::Top, true);
    }

    /// `openInApp(_:)`.
    fn open_in_app(&self, sender: &NSButton) {
        // SAFETY: the key is a static's address; the value, if any, is the
        // NSURL `installOpenInAppBar` associated.
        let value = unsafe {
            objc2::ffi::objc_getAssociatedObject(
                sender as *const NSButton as *const AnyObject,
                &URL_KEY as *const u8 as *const c_void,
            )
        };
        // SAFETY: a non-null associated object is a live Objective-C object.
        let Some(value) = (unsafe { (value as *const AnyObject).as_ref() }) else { return };
        let Some(url) = value.downcast_ref::<NSURL>() else { return };
        NSWorkspace::sharedWorkspace().openURL(url);
    }

    // MARK: - Memory discipline (§10, non-negotiable)

    /// `startMemoryWatch()`.
    fn start_memory_watch(&self) {
        let ivars = self.ivars();
        if let Some(timer) = ivars.memory_timer.take() {
            timer.invalidate();
        }
        let generation = ivars.preview_generation.get();
        // TextKit lays out lazily after `present` returns. Sampling
        // immediately treats that normal first viewport as a leak and
        // replaces a tiny file with raw source. Establish the steady-state
        // baseline after its first layout turn, then watch only subsequent
        // growth.
        let weak_self: ObjcWeak<PreviewViewController> = ObjcWeak::from(self);
        main_after(1.0, move || {
            let Some(this) = weak_self.load() else { return };
            if this.ivars().preview_generation.get() != generation || this.ivars().fallback_text_view.borrow().is_some()
            {
                return;
            }
            this.ivars().memory_baseline_bytes.set(Self::resident_bytes());
            let weak_timer_self: ObjcWeak<PreviewViewController> = ObjcWeak::from(&*this);
            let block = RcBlock::new(move |_timer: NonNull<NSTimer>| {
                let Some(this) = weak_timer_self.load() else { return };
                let resident = Self::resident_bytes();
                if !(this.preview_memory_bytes_now() > QuickLookPolicy::MEMORY_CEILING_BYTES
                    || resident > 95 * 1024 * 1024)
                {
                    return;
                }
                this.fall_back_to_plain_text();
            });
            // SAFETY: the timer is scheduled on the main run loop, so the
            // block only runs on the main thread.
            let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(0.4, true, &block) };
            // SAFETY: `NSRunLoopCommonModes` is an immutable Foundation global.
            unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
            *this.ivars().memory_timer.borrow_mut() = Some(timer);
        });
    }

    /// Bytes in use across malloc zones. `malloc_zone_statistics` is what
    /// §10 specifies; it is cheap enough to poll and, unlike `task_info`,
    /// reports what this process actually allocated rather than what the
    /// kernel has mapped for it.
    pub fn resident_bytes() -> isize {
        let mut zone_count: libc::c_uint = 0;
        let mut zones: *mut libc::vm_address_t = std::ptr::null_mut();
        let result = unsafe { malloc_get_all_zones(mach_task_self_, std::ptr::null(), &mut zones, &mut zone_count) };
        if result != libc::KERN_SUCCESS || zones.is_null() {
            return 0;
        }
        let mut total: isize = 0;
        for index in 0..zone_count as usize {
            let zone = unsafe { *zones.add(index) } as *mut libc::c_void;
            if zone.is_null() {
                continue;
            }
            let mut statistics = MallocStatistics::default();
            unsafe { malloc_zone_statistics(zone, &mut statistics) };
            total += statistics.size_in_use as isize;
        }
        total
    }

    /// `previewMemoryBytes` (the instance property).
    fn preview_memory_bytes_now(&self) -> isize {
        Self::preview_memory_bytes(Self::resident_bytes(), self.ivars().memory_baseline_bytes.get())
    }

    /// `previewMemoryBytes(current:baseline:)`: the preview's incremental
    /// footprint, never negative.
    pub fn preview_memory_bytes(current: isize, baseline: isize) -> isize {
        (current - baseline).max(0)
    }

    // MARK: - Test and conformance hooks (Swift reaches these with
    // `@testable import` or not at all)

    /// The installed container, if a document is presented.
    pub fn container_for_testing(&self) -> Option<Retained<MarkdownContainerView>> {
        self.ivars().container.borrow().clone()
    }

    /// The density gutter built for the presented document.
    pub fn density_gutter_for_testing(&self) -> Option<Retained<DensityGutterView>> {
        self.ivars().density_gutter.borrow().clone()
    }

    /// The plain-text fallback's text view, once the controller fell back.
    pub fn fallback_text_view_for_testing(&self) -> Option<Retained<NSTextView>> {
        self.ivars().fallback_text_view.borrow().clone()
    }

    /// The open-in-app bar, when one is installed.
    pub fn notice_bar_for_testing(&self) -> Option<Retained<NSView>> {
        self.ivars().notice_bar.borrow().clone()
    }

    /// The text storage the preview renders from.
    pub fn storage_for_testing(&self) -> Retained<NSTextStorage> {
        self.ivars().storage.clone()
    }

    /// `fallBackToPlainText()`, as the memory watch calls it.
    pub fn fall_back_to_plain_text_for_testing(&self) {
        self.fall_back_to_plain_text();
    }

    /// `previewGeneration &+= 1`: retires a pending memory watch the way a
    /// newer preview does (its first sample sees another generation). The
    /// conformance scene uses it so a capture never depends on the process's
    /// own malloc footprint.
    pub fn retire_memory_watch_for_testing(&self) {
        let generation = &self.ivars().preview_generation;
        generation.set(generation.get().wrapping_add(1));
    }

    /// `currentHeadingIndex`.
    pub fn current_heading_index_for_testing(&self) -> Option<isize> {
        self.ivars().current_heading_index.get()
    }

    /// Whether the memory watch's timer is scheduled.
    pub fn has_memory_timer_for_testing(&self) -> bool {
        self.ivars().memory_timer.borrow().is_some()
    }

    /// `jumpToHeading(forward:)`.
    pub fn jump_to_heading_for_testing(&self, forward: bool) {
        self.jump_to_heading(forward);
    }

    /// The gutter delegate the controller installs (`extension
    /// PreviewViewController: DensityGutterDelegate`).
    pub fn gutter_delegate_for_testing(&self) -> Option<Rc<dyn DensityGutterDelegate>> {
        self.ivars().gutter_delegate.borrow().clone().map(|delegate| delegate as Rc<dyn DensityGutterDelegate>)
    }
}

/// `extension PreviewViewController: DensityGutterDelegate`, on a proxy the
/// controller owns (the gutter keeps its delegate weakly).
pub struct PreviewGutterDelegate {
    controller: ObjcWeak<PreviewViewController>,
}

impl DensityGutterDelegate for PreviewGutterDelegate {
    fn density_gutter_did_request_scroll_to_fraction(&self, gutter: &DensityGutterView, fraction: CGFloat) {
        let Some(controller) = self.controller.load() else { return };
        let document = controller.ivars().parsed_document.borrow().clone();
        let Some(document) = document else { return };
        let offset = document.length.min(0isize.max((fraction * document.length as CGFloat) as isize));
        let container = controller.ivars().container.borrow().clone();
        let Some(container) = container else { return };
        let text_view = container.text_view();
        // QL can deliver the first click before TextKit has completed its
        // lazy layout. Force the target fragment, then use the same spring as
        // the app for normal clicks. If an interpolated rail fraction lands
        // in a collapsed/empty fragment, settle on the nearest heading
        // instead of silently leaving the page where it was.
        text_view.layoutSubtreeIfNeeded();
        if text_view.rect_for_offset(offset).is_some() {
            text_view.scroll_to_offset(offset, ScrollPosition::Top, !gutter.is_scrubbing());
            return;
        }
        // `min(by:)`: the first element no later element beats.
        let mut fallback: Option<&upleft_core::HeadingNode> = None;
        for heading in &document.headings {
            match fallback {
                None => fallback = Some(heading),
                Some(best) => {
                    if (heading.range.location - offset).abs() < (best.range.location - offset).abs() {
                        fallback = Some(heading);
                    }
                }
            }
        }
        let Some(fallback) = fallback else { return };
        text_view.scroll_to_offset(fallback.range.location, ScrollPosition::Top, !gutter.is_scrubbing());
    }

    fn density_gutter_preview_at_fraction(
        &self,
        gutter: &DensityGutterView,
        fraction: CGFloat,
    ) -> Option<(String, String, String)> {
        let controller = self.controller.load()?;
        let document = controller.ivars().parsed_document.borrow().clone()?;
        let offset = document.length.min((fraction * document.length as CGFloat) as isize);
        let Some(index) = document.headings.iter().rposition(|heading| heading.range.location <= offset) else {
            return Some(("Document start".to_owned(), String::new(), gutter.metrics_summary()));
        };
        let heading = &document.headings[index];
        let section_position = format!("Section {} of {}", index + 1, document.headings.len());
        let context = if heading.word_count > 0 {
            format!("{section_position} · {} words", heading.word_count)
        } else {
            section_position
        };
        Some((
            heading.title.clone(),
            StructuralZoom::section_preview(&document, index as isize).unwrap_or_else(|| "Section overview".to_owned()),
            context,
        ))
    }
}

/// `malloc_statistics_t`.
#[repr(C)]
#[derive(Default)]
struct MallocStatistics {
    blocks_in_use: libc::c_uint,
    size_in_use: libc::size_t,
    max_size_in_use: libc::size_t,
    size_allocated: libc::size_t,
}

unsafe extern "C" {
    static mach_task_self_: libc::mach_port_t;
    fn malloc_get_all_zones(
        task: libc::mach_port_t,
        reader: *const libc::c_void,
        addresses: *mut *mut libc::vm_address_t,
        count: *mut libc::c_uint,
    ) -> libc::kern_return_t;
    fn malloc_zone_statistics(zone: *mut libc::c_void, statistics: *mut MallocStatistics);
}
