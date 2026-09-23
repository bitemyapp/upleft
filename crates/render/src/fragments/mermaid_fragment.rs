//! Port of `Fragments/MermaidFragment.swift`: Mermaid diagrams (§11.3),
//! rendered natively and themed from the active palette, held in the shared,
//! cost-bounded Mermaid cache — plus the cached door of
//! `MermaidRendererBridge.image(source:styleSheet:)`.
//!
//! The renderer itself is `upleft-mermaid`, which depends on this crate, so
//! the render layer reaches it through [`install_mermaid_renderer`]: the host
//! (the app, `upleft-oracle`, the benches) installs
//! `upleft_mermaid::downright::mermaid_renderer_bridge::install_fragment_renderer`
//! once at start-up. Without it every diagram draws as a failed object.
//!
//! Threading: as in Swift, a diagram is rendered synchronously the first
//! time layout asks for its fragment's height (and again, from the cache,
//! when it draws), on the main thread, so the first displayed frame already
//! contains it. `upleft-mermaid`'s uncached path is about 2.7× faster than
//! Swift's (see its PORTING.md).

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::any::Any;
use std::rc::Rc;
use std::sync::OnceLock;

use objc2::rc::Retained;
use objc2_app_kit::{NSImage, NSScreen, NSTextElement, NSTextLayoutFragment, NSTextRange};
use objc2_core_foundation::{CGFloat, CGPoint, CGSize};
use objc2_core_graphics::CGContext;

use crate::appkit_compat::{RectExt, rect};
use crate::engine::render_metrics;
use crate::fragments::bounded_image_cache::{MERMAID, MermaidCacheKey};
use crate::fragments::fragment_base::{
    DownrightFragment, FailedObject, FragmentBehavior, FragmentContext, StyleToken, draw_ns_image,
};
use crate::render_contracts::FragmentPayload;
use crate::swift_compat::{smax, trim_whitespaces_and_newlines};
use crate::theme::style_sheet::StyleSheet;

/// The uncached body of `MermaidRendererBridge.image(source:styleSheet:)`:
/// trimmed source and style sheet in, the ink-cropped image out.
pub type MermaidRenderer = fn(&str, &StyleSheet) -> Option<Retained<NSImage>>;

static RENDERER: OnceLock<MermaidRenderer> = OnceLock::new();

/// Installs the Mermaid renderer the fragments and the bridge call. The
/// first installation wins.
pub fn install_mermaid_renderer(renderer: MermaidRenderer) {
    let _ = RENDERER.set(renderer);
}

/// `MermaidRendererBridge.scale()`: `NSScreen.main?.backingScaleFactor ?? 2`.
fn scale() -> CGFloat {
    objc2::MainThreadMarker::new()
        .and_then(NSScreen::mainScreen)
        .map_or(2.0, |screen| screen.backingScaleFactor())
}

/// `MermaidRendererBridge.image(source:styleSheet:)`: the single cached
/// path — fragments and export both come through here.
pub fn mermaid_image(source: &str, style_sheet: &StyleSheet) -> Option<Retained<NSImage>> {
    let trimmed = trim_whitespaces_and_newlines(source);
    if trimmed.is_empty() {
        return None;
    }
    let scale = scale();
    let key = MermaidCacheKey {
        source: trimmed.to_owned(),
        style_token: StyleToken::of(style_sheet),
        scale: crate::swift_compat::int_truncating((scale * 2.0).round()),
    };
    let key_cost = key.source.len();
    MERMAID.image(&key, key_cost, || (RENDERER.get()?)(trimmed, style_sheet))
}

/// `MermaidFragment`'s hooks.
pub struct MermaidFragment;

/// `MermaidFragment(textElement:range:payload:context:)`.
pub fn make(
    text_element: &NSTextElement,
    range: Option<&NSTextRange>,
    payload: &FragmentPayload,
    context: &Rc<FragmentContext>,
) -> Retained<NSTextLayoutFragment> {
    Retained::into_super(DownrightFragment::new(
        c"MermaidFragment",
        text_element,
        range,
        payload,
        context,
        Box::new(MermaidFragment),
    ))
}

/// Same trust instrument as a missing image (§8.4).
fn failure(fragment: &DownrightFragment) -> FailedObject {
    FailedObject { label: "Diagram could not be rendered".to_owned(), source: fragment.payload().detail().to_owned() }
}

impl FragmentBehavior for MermaidFragment {
    fn suppresses_text(&self, _fragment: &DownrightFragment) -> bool {
        true
    }

    fn override_height(&self, fragment: &DownrightFragment) -> Option<CGFloat> {
        if !fragment.is_first_paragraph_of_block() {
            return Some(0.0);
        }
        let style = fragment.style_sheet()?;
        let grid = smax(1.0, style.baseline_grid);
        let size = rendered_size(fragment);
        if !(size.height > 0.0) {
            return Some(render_metrics::snap_up(
                fragment.failed_object_height(&failure(fragment), &style) + style.line_height * 0.5,
                grid,
            ));
        }
        // One line height of air, split above and below by the centring.
        Some(render_metrics::snap_up(size.height + style.line_height, grid))
    }

    fn draw_object(&self, fragment: &DownrightFragment, point: CGPoint, cg: &CGContext) {
        if !fragment.is_first_paragraph_of_block() {
            return;
        }
        let Some(style) = fragment.style_sheet() else { return };
        let Some(image) = rendered_image(fragment) else {
            let failure = failure(fragment);
            let height = fragment.failed_object_height(&failure, &style);
            fragment.draw_failed_object(&failure, rect(point.x, point.y, fragment.content_width(), height), &style, cg);
            return;
        };
        let size = rendered_size(fragment);
        let origin = CGPoint::new(
            point.x + smax(0.0, (fragment.content_width() - size.width) / 2.0),
            point.y + smax(0.0, (fragment.layoutFragmentFrame().height() - size.height) / 2.0),
        );
        draw_ns_image(&image, rect(origin.x, origin.y, size.width, size.height), cg, 0.0);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Point size, brought inside the measure when the diagram is wider.
fn rendered_size(fragment: &DownrightFragment) -> CGSize {
    let Some(image) = rendered_image(fragment) else { return CGSize::new(0.0, 0.0) };
    let natural = image.size();
    let content_width = fragment.content_width();
    if !(natural.width > content_width && natural.width > 0.0) {
        return natural;
    }
    let scale = content_width / natural.width;
    CGSize::new(content_width, (natural.height * scale).round())
}

fn rendered_image(fragment: &DownrightFragment) -> Option<Retained<NSImage>> {
    let style = fragment.style_sheet()?;
    mermaid_image(fragment.payload().detail(), &style)
}
