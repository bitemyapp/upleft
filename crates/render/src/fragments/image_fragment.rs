//! Port of `Fragments/ImageFragment.swift`: images (§11.3) with rounded
//! corners, a restrained shadow and alt text as a caption. Paths resolve
//! against the document's directory (§3.4); decoding happens on a
//! background queue and the fragment draws a placeholder until it lands.

// `!(a > b)` spells Swift's `guard a > b`, which is false for NaN; the
// negated comparisons are deliberate.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use std::any::Any;
use std::rc::Rc;

use objc2::rc::{Retained, Weak};
use objc2_app_kit::{
    NSColor, NSColorSpace, NSImage, NSMutableParagraphStyle, NSScreen, NSTextAlignment, NSTextElement,
    NSTextLayoutFragment, NSTextRange,
};
use objc2_core_foundation::{CGFloat, CGPoint, CGSize};
use objc2_core_graphics::{CGContext, CGPath};
use objc2_foundation::{NSString, NSURL};

use crate::appkit_compat::{RectExt, attribute_value, attributed_string, keys, rect};
use crate::engine::render_metrics;
use crate::fragments::bounded_image_cache::IMAGES;
use crate::fragments::fragment_base::{
    DownrightFragment, FailedObject, FragmentBehavior, FragmentContext, draw_ns_image, draw_text, fill_rect, mixed,
};
use crate::fragments::local_asset_policy::{LocalAssetPolicy, LocalAssetRequest, file_url};
use crate::render_contracts::{FragmentPayload, ThemeAppearance, attribute_keys};
use crate::swift_compat::{smax, smin};
use crate::theme::style_sheet::StyleSheet;
use crate::view::markdown_text_view::MarkdownTextView;

/// `ImageFragment.LoadResult`.
enum LoadResult {
    Loaded(Retained<NSImage>),
    /// Not in the cache yet; a background decode is in flight and will
    /// invalidate this fragment's layout when it lands.
    Loading,
    /// Declared by the Swift, never produced by `loadResult`.
    #[allow(dead_code)]
    Missing,
    Blocked,
}

/// `ImageFragment`'s hooks.
pub struct ImageFragment;

/// `ImageFragment(textElement:range:payload:context:)`.
pub fn make(
    text_element: &NSTextElement,
    range: Option<&NSTextRange>,
    payload: &FragmentPayload,
    context: &Rc<FragmentContext>,
) -> Retained<NSTextLayoutFragment> {
    Retained::into_super(DownrightFragment::new(
        c"ImageFragment",
        text_element,
        range,
        payload,
        context,
        Box::new(ImageFragment),
    ))
}

impl FragmentBehavior for ImageFragment {
    fn suppresses_text(&self, _fragment: &DownrightFragment) -> bool {
        true
    }

    fn override_height(&self, fragment: &DownrightFragment) -> Option<CGFloat> {
        if !fragment.is_first_paragraph_of_block() {
            return Some(0.0);
        }
        let style = fragment.style_sheet()?;
        let grid = smax(1.0, style.baseline_grid);
        let picture = display_size(fragment);
        let mut height = picture.height;
        if !caption(fragment).is_empty() {
            height += render_metrics::IMAGE_CAPTION_GAP + style.line_height;
        }
        Some(render_metrics::snap_up(height + style.line_height * 0.5, grid))
    }

    fn draw_object(&self, fragment: &DownrightFragment, point: CGPoint, cg: &CGContext) {
        if !fragment.is_first_paragraph_of_block() {
            return;
        }
        let Some(style) = fragment.style_sheet() else { return };
        let picture = display_size(fragment);
        // Images sit in the reading column, so they centre on it.
        let origin = CGPoint::new(point.x + smax(0.0, (fragment.prose_content_width() - picture.width) / 2.0), point.y);
        let target = rect(origin.x, origin.y, picture.width, picture.height);

        let context = Some(cg);
        match load_result(fragment) {
            LoadResult::Loaded(image) => {
                CGContext::save_g_state(context);
                let shadow = NSColor::blackColor().colorWithAlphaComponent(if style.increase_contrast { 0.0 } else { 0.18 });
                CGContext::set_shadow_with_color(
                    context,
                    CGSize::new(0.0, 2.0),
                    render_metrics::IMAGE_SHADOW_RADIUS,
                    Some(&shadow.CGColor()),
                );
                fill_rect(cg, target, &matte(&image, &style), render_metrics::IMAGE_CORNER_RADIUS);
                CGContext::restore_g_state(context);
                draw_ns_image(&image, target, cg, render_metrics::IMAGE_CORNER_RADIUS);
                if style.theme.appearance == ThemeAppearance::Dark {
                    CGContext::set_stroke_color_with_color(context, Some(&style.text.colorWithAlphaComponent(0.08).CGColor()));
                    CGContext::set_line_width(context, 1.0);
                    CGContext::stroke_rect(context, target.inset_by(0.5, 0.5));
                }
            }
            LoadResult::Loading => {
                // A quiet placeholder while the background decode runs.
                CGContext::save_g_state(context);
                CGContext::set_fill_color_with_color(
                    context,
                    Some(&style.code_background.colorWithAlphaComponent(0.55).CGColor()),
                );
                // SAFETY: a null transform is allowed.
                let path = unsafe {
                    CGPath::with_rounded_rect(
                        target,
                        render_metrics::IMAGE_CORNER_RADIUS,
                        render_metrics::IMAGE_CORNER_RADIUS,
                        std::ptr::null(),
                    )
                };
                CGContext::add_path(context, Some(&path));
                CGContext::fill_path(context);
                CGContext::restore_g_state(context);
            }
            result => {
                // §8.4's trust instrument applied to images.
                let failure = match result {
                    LoadResult::Blocked => blocked(fragment),
                    _ => missing(fragment),
                };
                fragment.draw_failed_object(&failure, target, &style, cg);
            }
        }

        let caption = caption(fragment);
        if caption.is_empty() {
            return;
        }
        let paragraph = NSMutableParagraphStyle::new();
        paragraph.setAlignment(NSTextAlignment::Center);
        let font = style.body_font().fontWithSize(style.body_font().pointSize() * 0.86);
        let text = attributed_string(
            &caption,
            &[
                (keys::font(), &font),
                (keys::foreground_color(), &style.text_secondary),
                (keys::paragraph_style(), &paragraph),
            ],
        );
        draw_text(
            cg,
            &text,
            rect(
                point.x,
                target.max_y() + render_metrics::IMAGE_CAPTION_GAP,
                fragment.prose_content_width(),
                style.line_height,
            ),
            true,
        );
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Alt text, which the engine parked on `drReference`.
fn caption(fragment: &DownrightFragment) -> String {
    let Some(storage) = fragment.context().and_then(|context| context.storage()) else { return String::new() };
    let location = fragment.payload().source_range().location;
    if !(location < storage.length() as isize) {
        return String::new();
    }
    attribute_value(&storage, attribute_keys::dr_reference(), location as usize)
        .and_then(|value| value.downcast::<NSString>().ok())
        .map(|value| upleft_swift_text::ns::foundation::to_string(&value))
        .unwrap_or_default()
}

/// The placeholder for a file that is not there.
fn missing(fragment: &DownrightFragment) -> FailedObject {
    FailedObject { label: compact_failure_label(fragment, "Missing image"), source: String::new() }
}

fn blocked(fragment: &DownrightFragment) -> FailedObject {
    FailedObject { label: compact_failure_label(fragment, "Blocked image"), source: String::new() }
}

/// A missing image is usually one short path: keep it in one compact row.
fn compact_failure_label(fragment: &DownrightFragment, label: &str) -> String {
    let detail = fragment.payload().detail();
    if detail.is_empty() { label.to_owned() } else { format!("{label}  ·  {detail}") }
}

/// What the image is composited over: the page, or for transparent artwork
/// in a dark theme a plate near the lighter pole.
fn matte(image: &NSImage, style: &StyleSheet) -> Retained<NSColor> {
    let has_alpha = image.representations().firstObject().is_some_and(|rep| rep.hasAlpha());
    if !has_alpha {
        return style.background.clone();
    }
    let page = &style.background;
    let ink = &style.text;
    if !(brightness(ink) > brightness(page)) {
        return page.clone();
    }
    mixed(ink, page, 0.06)
}

fn brightness(color: &NSColor) -> CGFloat {
    color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()).unwrap_or_else(|| objc2::Message::retain(color)).brightnessComponent()
}

fn display_size(fragment: &DownrightFragment) -> CGSize {
    let loaded = match load_result(fragment) {
        LoadResult::Loaded(image) if image.size().width > 0.0 => Some(image),
        _ => None,
    };
    let Some(image) = loaded else {
        let Some(style) = fragment.style_sheet() else {
            return CGSize::new(fragment.prose_content_width(), 64.0);
        };
        return CGSize::new(fragment.prose_content_width(), fragment.failed_object_height(&missing(fragment), &style));
    };
    let natural = image.size();
    let viewport_cap = viewport_height_cap(fragment);
    let width_scale = smin(1.0, fragment.prose_content_width() / natural.width);
    let height_scale = smin(1.0, viewport_cap / natural.height);
    let scale = smin(width_scale, height_scale);
    CGSize::new((natural.width * scale).round(), (natural.height * scale).round())
}

fn load_result(fragment: &DownrightFragment) -> LoadResult {
    let Some(request) = resolved_request(fragment) else { return LoadResult::Blocked };
    let authorizer = fragment.context().and_then(|context| context.local_asset_authorizer.borrow().clone());
    if !LocalAssetPolicy::allows(&request, authorizer.as_ref()) {
        return LoadResult::Blocked;
    }
    let dimension = target_pixel_dimension(fragment);
    let Some(image) = IMAGES.cached_image(&request.url, dimension) else {
        // Never read the file here: schedule a background decode and draw a
        // placeholder until it lands.
        schedule_async_load(fragment, &request.url, dimension);
        return LoadResult::Loading;
    };
    LoadResult::Loaded(image)
}

/// Asks the background loader for this image; when it arrives the fragment's
/// layout is invalidated.
fn schedule_async_load(fragment: &DownrightFragment, url: &NSURL, dimension: isize) {
    let view: Weak<MarkdownTextView> = match fragment.context().and_then(|context| context.text_view()) {
        Some(view) => Weak::from_retained(&view),
        None => Weak::default(),
    };
    let payload: Retained<FragmentPayload> = objc2::Message::retain(fragment.payload());
    IMAGES.load_image_async(url, dimension, move |image| {
        // Only invalidate when an image loaded, to avoid layout loops.
        if image.is_none() {
            return;
        }
        let Some(view) = view.load() else { return };
        // The payload reference, not a copied range: an edit in flight
        // projects its ranges in place.
        view.invalidate_fragments(Some(payload.source_range()));
    });
}

fn viewport_height_cap(fragment: &DownrightFragment) -> CGFloat {
    let height = fragment
        .context()
        .and_then(|context| context.text_view())
        .and_then(|view| view.enclosingScrollView())
        .map_or(800.0, |scroll| scroll.contentSize().height);
    smax(120.0, height * 0.70)
}

fn target_pixel_dimension(fragment: &DownrightFragment) -> isize {
    let scale = fragment
        .context()
        .and_then(|context| context.text_view())
        .and_then(|view| view.window())
        .map(|window| window.backingScaleFactor())
        .or_else(|| {
            objc2::MainThreadMarker::new().and_then(|mtm| NSScreen::mainScreen(mtm)).map(|screen| screen.backingScaleFactor())
        })
        .unwrap_or(2.0);
    // A hard per-image ceiling keeps one pathological asset from defeating
    // the cache's budget.
    let points = smax(fragment.content_width(), viewport_height_cap(fragment));
    let pixels = crate::swift_compat::int_truncating((points * scale).ceil()) as isize;
    pixels.max(1).min(2048)
}

fn resolved_request(fragment: &DownrightFragment) -> Option<LocalAssetRequest> {
    let document = fragment.context().and_then(|context| context.document_url.borrow().clone()).map(|path| file_url(&path));
    LocalAssetPolicy::request(fragment.payload().detail(), document.as_deref())
}
