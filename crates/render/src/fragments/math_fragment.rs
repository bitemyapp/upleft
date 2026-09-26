//! Port of `Fragments/MathFragment.swift`: block math (§11.3), typeset by
//! SwiftMath (`upleft-math`) and sized optically against body text, held in
//! the shared, cost-bounded math cache.
//!
//! Threading: as in Swift, the formula is typeset synchronously the first
//! time layout asks for this fragment's height, on the main thread, so the
//! first displayed frame already contains it. `upleft-math` typesets in
//! roughly half Swift's time (see its PORTING.md).

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::any::Any;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2_app_kit::{NSImage, NSTextElement, NSTextLayoutFragment, NSTextRange};
use objc2_core_foundation::{CGFloat, CGPoint};
use objc2_core_graphics::CGContext;
use upleft_math::MathRenderer;

use crate::appkit_compat::{RectExt, rect};
use crate::fragments::async_objects::{self, ObjectImage};
use crate::fragments::mermaid_fragment::draw_placeholder;
use crate::theme::style_sheet::StyleSheet;
use crate::engine::render_metrics;
use crate::fragments::fragment_base::{DownrightFragment, FailedObject, FragmentBehavior, FragmentContext, draw_ns_image};
use crate::render_contracts::FragmentPayload;
use crate::swift_compat::smax;

/// The payload detail a display formula is typeset from: its LaTeX range as
/// written (`MathRenderer::image` trims it).
pub fn block_latex(document: &upleft_core::ParsedDocument, latex_range: upleft_core::NSRange) -> String {
    document.substring(latex_range)
}

/// `MathFragment`'s hooks.
pub struct MathFragment;

/// `MathFragment(textElement:range:payload:context:)`.
pub fn make(
    text_element: &NSTextElement,
    range: Option<&NSTextRange>,
    payload: &FragmentPayload,
    context: &Rc<FragmentContext>,
) -> Retained<NSTextLayoutFragment> {
    Retained::into_super(DownrightFragment::new(
        c"MathFragment",
        text_element,
        range,
        payload,
        context,
        Box::new(MathFragment),
    ))
}

/// A formula that will not typeset names the failure and keeps its source.
fn failure(fragment: &DownrightFragment) -> FailedObject {
    FailedObject { label: "Formula could not be typeset".to_owned(), source: fragment.payload().detail().to_owned() }
}

impl FragmentBehavior for MathFragment {
    fn suppresses_text(&self, _fragment: &DownrightFragment) -> bool {
        true
    }

    fn override_height(&self, fragment: &DownrightFragment) -> Option<CGFloat> {
        if !fragment.is_first_paragraph_of_block() {
            return Some(0.0);
        }
        let style = fragment.style_sheet()?;
        let grid = smax(1.0, style.baseline_grid);
        if let Some(object) = hosted_object(fragment, &style) {
            let height = match object {
                ObjectImage::Ready(image) => image.size().height,
                ObjectImage::Pending(size) => size.height,
                ObjectImage::Failed => {
                    return Some(render_metrics::snap_up(
                        fragment.failed_object_height(&failure(fragment), &style) + style.line_height * 0.5,
                        grid,
                    ));
                }
            };
            return Some(render_metrics::snap_up(height + style.line_height * 0.7, grid));
        }
        let Some(image) = rendered_image(fragment) else {
            return Some(render_metrics::snap_up(
                fragment.failed_object_height(&failure(fragment), &style) + style.line_height * 0.5,
                grid,
            ));
        };
        Some(render_metrics::snap_up(image.size().height + style.line_height * 0.7, grid))
    }

    fn draw_object(&self, fragment: &DownrightFragment, point: CGPoint, cg: &CGContext) {
        if !fragment.is_first_paragraph_of_block() {
            return;
        }
        let Some(style) = fragment.style_sheet() else { return };
        if let Some(object) = hosted_object(fragment, &style) {
            match object {
                ObjectImage::Ready(image) => {
                    let frame = fragment.layoutFragmentFrame();
                    let size = image.size();
                    let origin = CGPoint::new(
                        point.x + smax(0.0, (fragment.content_width() - size.width) / 2.0),
                        point.y + smax(0.0, (frame.height() - size.height) / 2.0),
                    );
                    draw_image_at(&image, origin, cg);
                }
                ObjectImage::Pending(size) => draw_placeholder(fragment, point, size, &style, cg),
                ObjectImage::Failed => {
                    let failure = failure(fragment);
                    let height = fragment.failed_object_height(&failure, &style);
                    fragment.draw_failed_object(&failure, rect(point.x, point.y, fragment.content_width(), height), &style, cg);
                }
            }
            return;
        }
        let Some(image) = rendered_image(fragment) else {
            let failure = failure(fragment);
            let height = fragment.failed_object_height(&failure, &style);
            fragment.draw_failed_object(&failure, rect(point.x, point.y, fragment.content_width(), height), &style, cg);
            return;
        };
        let frame = fragment.layoutFragmentFrame();
        let size = image.size();
        let origin = CGPoint::new(
            point.x + smax(0.0, (fragment.content_width() - size.width) / 2.0),
            point.y + smax(0.0, (frame.height() - size.height) / 2.0),
        );
        draw_image_at(&image, origin, cg);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A hosted view's formula, from `async_objects`; `None` elsewhere.
fn hosted_object(fragment: &DownrightFragment, style: &StyleSheet) -> Option<ObjectImage> {
    let context = fragment.context()?;
    if !context.renders_objects_async.get() {
        return None;
    }
    Some(async_objects::math(fragment, fragment.payload().detail(), style.math_point_size * 1.12, &style.text))
}

fn rendered_image(fragment: &DownrightFragment) -> Option<Retained<NSImage>> {
    let style = fragment.style_sheet()?;
    let point_size = style.math_point_size * 1.12;
    // The padded bitmap keeps ≥ 8pt of air on every edge (§11.3).
    MathRenderer::image(fragment.payload().detail(), true, point_size, &style.text, 8.0)
}

/// `DownrightFragment.draw(image:at:in:)`.
pub fn draw_image_at(image: &NSImage, origin: CGPoint, cg: &CGContext) {
    let size = image.size();
    draw_ns_image(image, rect(origin.x, origin.y, size.width, size.height), cg, 0.0);
}
