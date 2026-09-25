//! Mermaid diagrams and display math rendered off the main thread, for
//! hosted views. An Upleft extension with no Swift counterpart; see
//! docs/EMBEDDING.md.
//!
//! Downright renders both synchronously, on the main thread, the first time
//! layout asks a fragment for its height. A hosted view's fragments ask here
//! instead ([`mermaid`], [`math`]):
//!
//! - a hit in the shared cache (the same `MERMAID` and `MATH` caches the
//!   synchronous path fills) is drawn at once;
//! - a miss schedules the render on a global queue, once per key however
//!   many fragments ask, and returns a placeholder size: the size this key
//!   rendered at before, or an estimate from its source;
//! - when the render lands, the image is stored in the shared cache and every
//!   view that asked lays out the blocks with that source again
//!   (`MarkdownTextView::object_render_landed`), which re-reports its height.
//!
//! A render that fails is remembered, so the fragment draws Downright's
//! failure card instead of asking again.
//!
//! # Thread safety
//!
//! What runs on the worker, and why it may:
//!
//! - **Mermaid**: parsing, ELK layout and drawing into a private
//!   `CGBitmapContext` (Core Graphics contexts are per-thread objects); text
//!   is measured and drawn with Core Text and with `NSAttributedString`
//!   drawing into an `NSGraphicsContext` made for that bitmap on the worker,
//!   which AppKit's threading guide allows ("AppKit is generally thread-safe
//!   when drawing with its graphics functions and classes, including
//!   NSBezierPath and NSString"). Two calls in the synchronous path are not
//!   safe off the main thread, and the worker path avoids both: reading
//!   `NSScreen.mainScreen` for the backing scale (the scale is read here, on
//!   the main thread, and passed in) and `NSFontManager.sharedFontManager`
//!   for italic system fonts (the worker resolves the italic through
//!   `NSFontDescriptor` symbolic traits, which yields the same font).
//!   Per-thread state in `upleft-mermaid` and `upleft-elk` is thread-local.
//! - **Display math**: parsing and typesetting are Core Text and plain Rust;
//!   the font tables sit behind locks. The formula becomes an `NSImage` with
//!   a drawing handler, padded by drawing it into a second `NSImage` with
//!   `lockFocus`. AppKit's guide allows exactly this: "one thread can create
//!   an NSImage object, draw to the image buffer, and pass it off to the main
//!   thread for drawing".
//! - **The style sheet** travels to the worker as a clone. Its colours were
//!   snapshotted to sRGB against one appearance when it was built, so nothing
//!   on the worker resolves a dynamic colour against the wrong appearance;
//!   its fonts, colours and appearance are immutable objects.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};

use dispatch2::{DispatchQoS, DispatchQueue, GlobalQueueIdentifier, MainThreadBound};
use objc2::rc::{Retained, Weak as ObjcWeak};
use objc2::{MainThreadMarker, Message};
use objc2_app_kit::{NSColor, NSImage, NSScreen};
use objc2_core_foundation::{CGFloat, CGSize};
use upleft_math::MathRenderer;
use upleft_math::downright::bounded_image_cache::MathRendererCacheKey;

use crate::fragments::bounded_image_cache::{CachedImage, MERMAID, MermaidCacheKey};
use crate::fragments::fragment_base::{DownrightFragment, StyleToken};
use crate::swift_compat::{int_truncating, trim_whitespaces_and_newlines};
use crate::theme::style_sheet::StyleSheet;
use crate::view::markdown_text_view::MarkdownTextView;

/// A Mermaid renderer that may run on any thread: trimmed source, style
/// sheet and backing scale in, the ink-cropped image out. The Mermaid crate
/// installs it with the synchronous renderer
/// (`upleft_mermaid::downright::mermaid_renderer_bridge::install_fragment_renderer`).
pub type AsyncMermaidRenderer = fn(&str, &StyleSheet, CGFloat) -> Option<Retained<NSImage>>;

static MERMAID_RENDERER: OnceLock<AsyncMermaidRenderer> = OnceLock::new();

/// Installs the thread-safe Mermaid renderer. The first installation wins.
pub fn install_async_mermaid_renderer(renderer: AsyncMermaidRenderer) {
    let _ = MERMAID_RENDERER.set(renderer);
}

/// Renders scheduled and not landed yet. For tests and harnesses that wait
/// for a view to settle.
pub fn pending_count() -> usize {
    PENDING.load(Ordering::SeqCst)
}

/// Where `view` draws placeholders now: the TextKit offsets of the
/// fragments waiting for a render. For a host that tells a placeholder in
/// sight from a finished view.
pub fn pending_offsets(view: &MarkdownTextView) -> Vec<isize> {
    let Some(mtm) = MainThreadMarker::new() else { return Vec::new() };
    let state = state();
    let mut offsets = Vec::new();
    for waiters in state.in_flight.values() {
        for (waiter, offset) in waiters {
            if waiter.get(mtm).load().is_some_and(|waiting| std::ptr::eq(&*waiting, view)) && !offsets.contains(offset) {
                offsets.push(*offset);
            }
        }
    }
    offsets
}

/// What a fragment draws.
pub enum ObjectImage {
    Ready(Retained<NSImage>),
    /// Rendering on a worker; draw a placeholder this size.
    Pending(CGSize),
    Failed,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Mermaid(MermaidCacheKey),
    Math(MathRendererCacheKey),
}

/// A style sheet on its way to a worker.
struct SendStyleSheet(StyleSheet);
// SAFETY: see the module documentation: every object a `StyleSheet` holds is
// immutable once built, and its colours are resolved sRGB snapshots.
unsafe impl Send for SendStyleSheet {}

struct SendColor(Retained<NSColor>);
// SAFETY: a resolved colour is immutable.
unsafe impl Send for SendColor {}

/// A view waiting for a render, and where in it the fragment that asked
/// starts (its TextKit offset), for `pending_offsets`.
type Waiter = (MainThreadBound<ObjcWeak<MarkdownTextView>>, isize);

/// Bound on the remembered sizes and failures; both are only hints.
const MEMORY: usize = 1024;

/// Landed images kept outside the shared caches, which may evict an image
/// (or refuse one over their cost budget) before the views that asked for it
/// have drawn it; without this a refused image would render forever.
const RECENT: usize = 16;

#[derive(Default)]
struct State {
    in_flight: HashMap<Key, Vec<Waiter>>,
    failed: HashSet<Key>,
    sizes: HashMap<Key, CGSize>,
    recent: Vec<(Key, CachedImage)>,
}

/// While a key renders, its fragments stay placeholders even if the worker
/// has already filled the shared cache: a fragment's height may only change
/// when the landing, on the main thread, lays it out again. Otherwise the
/// fragments below it would keep a tiling made for the old height.
fn is_in_flight(key: &Key) -> bool {
    state().in_flight.contains_key(key)
}

fn recent(key: &Key) -> Option<Retained<NSImage>> {
    state().recent.iter().find(|(recent, _)| recent == key).map(|(_, image)| image.0.clone())
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(Mutex::default);
static PENDING: AtomicUsize = AtomicUsize::new(0);

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|poison| poison.into_inner())
}

thread_local! {
    /// The main screen's backing scale, until the screens change.
    static MAIN_SCREEN_SCALE: std::cell::Cell<Option<CGFloat>> = const { std::cell::Cell::new(None) };
    /// The observer that forgets it then.
    static SCREEN_OBSERVER: std::cell::OnceCell<Retained<objc2::runtime::ProtocolObject<dyn objc2_foundation::NSObjectProtocol>>> =
        const { std::cell::OnceCell::new() };
}

/// `NSScreen.main?.backingScaleFactor ?? 2`, read on the main thread.
///
/// Every layout of a hosted Mermaid fragment asks for the scale, and
/// `NSScreen.mainScreen` asks the window server each time (about 0.1 ms, most
/// of a long transcript's layout). The value is kept until AppKit reports
/// that the screens changed.
fn main_screen_scale(mtm: MainThreadMarker) -> CGFloat {
    if let Some(scale) = MAIN_SCREEN_SCALE.get() {
        return scale;
    }
    SCREEN_OBSERVER.with(|observer| {
        observer.get_or_init(|| {
            let block = block2::RcBlock::new(|_note: std::ptr::NonNull<objc2_foundation::NSNotification>| {
                MAIN_SCREEN_SCALE.set(None);
            });
            // SAFETY: the name is an AppKit static; the block only clears a
            // main-thread cell, and the main queue delivers it.
            unsafe {
                objc2_foundation::NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                    Some(objc2_app_kit::NSApplicationDidChangeScreenParametersNotification),
                    None,
                    Some(&objc2_foundation::NSOperationQueue::mainQueue()),
                    &block,
                )
            }
        });
    });
    let scale = NSScreen::mainScreen(mtm).map_or(2.0, |screen| screen.backingScaleFactor());
    MAIN_SCREEN_SCALE.set(Some(scale));
    scale
}

/// The diagram a hosted Mermaid fragment draws.
pub fn mermaid(fragment: &DownrightFragment, style: &StyleSheet) -> ObjectImage {
    let Some(mtm) = MainThreadMarker::new() else { return ObjectImage::Failed };
    let trimmed = trim_whitespaces_and_newlines(fragment.payload().detail());
    if trimmed.is_empty() {
        return ObjectImage::Failed;
    }
    let Some(renderer) = MERMAID_RENDERER.get().copied() else { return ObjectImage::Failed };
    let scale = main_screen_scale(mtm);
    let cache_key = MermaidCacheKey {
        source: trimmed.to_owned(),
        style_token: fragment.context().map_or_else(|| StyleToken::of(style), |context| context.style_token()),
        scale: int_truncating((scale * 2.0).round()),
    };
    let key = Key::Mermaid(cache_key.clone());
    let in_flight = is_in_flight(&key);
    if !in_flight && let Some(image) = MERMAID.cached(&cache_key).or_else(|| recent(&key)) {
        return ObjectImage::Ready(image);
    }
    let estimate = || {
        let lines = trimmed.lines().count() as CGFloat;
        CGSize::new(fragment.content_width(), (48.0 + lines * 30.0).clamp(120.0, 480.0))
    };
    let source = trimmed.to_owned();
    let style = SendStyleSheet(style.clone());
    schedule(fragment, key, estimate, true, source.clone(), move || {
        let style = style;
        let image = renderer(&source, &style.0, scale)?;
        MERMAID.store(image.clone(), &cache_key, cache_key.source.len());
        Some(image)
    })
}

/// The formula a hosted display-math fragment draws: the same image
/// `MathFragment` asks `MathRenderer::image` for.
pub fn math(fragment: &DownrightFragment, latex: &str, point_size: CGFloat, color: &NSColor) -> ObjectImage {
    let Some(cache_key) = MathRenderer::cache_key(latex, true, point_size, color, 8.0) else {
        return ObjectImage::Failed;
    };
    let in_flight = is_in_flight(&Key::Math(cache_key.clone()));
    if !in_flight && let Some(image) = MathRenderer::cached_image(&cache_key) {
        return ObjectImage::Ready(image);
    }
    let rows = 1.0 + cache_key.source.matches("\\\\").count() as CGFloat;
    let estimate = || CGSize::new(fragment.content_width(), 16.0 + rows * point_size * 1.6);
    let source = cache_key.source.clone();
    let key = Key::Math(cache_key);
    if !in_flight && let Some(image) = recent(&key) {
        return ObjectImage::Ready(image);
    }
    let latex = latex.to_owned();
    let color = SendColor(color.retain());
    schedule(fragment, key, estimate, false, source, move || {
        let color = color;
        MathRenderer::image(&latex, true, point_size, &color.0, 8.0)
    })
}

/// Registers `fragment`'s view as waiting for `key` and starts the render
/// unless one is already running.
fn schedule(
    fragment: &DownrightFragment,
    key: Key,
    estimate: impl FnOnce() -> CGSize,
    is_mermaid: bool,
    trimmed_source: String,
    render: impl FnOnce() -> Option<Retained<NSImage>> + Send + 'static,
) -> ObjectImage {
    let Some(mtm) = MainThreadMarker::new() else { return ObjectImage::Failed };
    let view = fragment.context().and_then(|context| context.text_view());
    let offset = view.as_deref().map_or(-1, |view| view.text_kit_offset_of(fragment));
    let waiter: Waiter = (MainThreadBound::new(view.as_deref().map(ObjcWeak::from).unwrap_or_default(), mtm), offset);
    let size = {
        let mut state = state();
        if state.failed.contains(&key) {
            return ObjectImage::Failed;
        }
        let size = state.sizes.get(&key).copied();
        if let Some(waiters) = state.in_flight.get_mut(&key) {
            waiters.push(waiter);
            return ObjectImage::Pending(size.unwrap_or_else(estimate));
        }
        state.in_flight.insert(key.clone(), vec![waiter]);
        size
    };
    PENDING.fetch_add(1, Ordering::SeqCst);
    let queue = DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(DispatchQoS::UserInitiated));
    queue.exec_async(move || {
        let image = objc2::rc::autoreleasepool(|_| render().map(CachedImage));
        let size = image.as_ref().map(|image| image.0.size());
        DispatchQueue::main().exec_async(move || {
            let mtm = MainThreadMarker::new().expect("the main queue runs on the main thread");
            let waiters = {
                let mut state = state();
                match (size, &image) {
                    (Some(size), Some(image)) => {
                        if state.sizes.len() >= MEMORY {
                            state.sizes.clear();
                        }
                        state.sizes.insert(key.clone(), size);
                        if state.recent.len() >= RECENT {
                            state.recent.remove(0);
                        }
                        state.recent.push((key.clone(), image.clone()));
                    }
                    _ => {
                        if state.failed.len() >= MEMORY {
                            state.failed.clear();
                        }
                        state.failed.insert(key.clone());
                    }
                }
                state.in_flight.remove(&key).unwrap_or_default()
            };
            drop(image);
            let mut notified: Vec<Retained<MarkdownTextView>> = Vec::new();
            for (waiter, _) in waiters {
                let Some(view) = waiter.into_inner(mtm).load() else { continue };
                if notified.iter().any(|seen| std::ptr::eq(&**seen, &*view)) {
                    continue;
                }
                notified.push(view);
            }
            PENDING.fetch_sub(1, Ordering::SeqCst);
            for view in notified {
                view.object_render_landed(is_mermaid, &trimmed_source);
            }
        });
    });
    ObjectImage::Pending(size.unwrap_or_else(estimate))
}
