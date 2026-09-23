//! Port of Downright's `Tests/MarkdownRenderTests/MermaidOrientationProbeTests.swift`:
//! the bridge's bitmap must match the library's known-good (`MermaidLayer`)
//! render, read upright, and be cropped to its own ink.

mod common;

use objc2::rc::Retained;
use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSBitmapImageRep, NSImage};
use objc2_core_graphics::CGImage;
use upleft_mermaid::downright::mermaid_renderer_bridge as bridge;
use upleft_mermaid::views::mermaid_layer::MermaidLayer;
use upleft_mermaid::LayoutConfig;
use upleft_render::render_contracts::Theme;
use upleft_render::theme::style_sheet::StyleSheet;

fn sheet() -> StyleSheet {
    let appearance = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).unwrap();
    StyleSheet::new(Theme::fallback(), &appearance, None)
}

/// Raw pixels of an image, as `NSBitmapImageRep(data: image.tiffRepresentation)`.
struct Bitmap {
    data: Vec<u8>,
    width: usize,
    height: usize,
    bytes_per_row: usize,
    samples_per_pixel: usize,
}

fn bitmap_rep(image: &NSImage) -> Option<Bitmap> {
    let tiff = image.TIFFRepresentation()?;
    let rep: Retained<NSBitmapImageRep> = NSBitmapImageRep::imageRepWithData(&tiff)?;
    let width = rep.pixelsWide() as usize;
    let height = rep.pixelsHigh() as usize;
    let bytes_per_row = rep.bytesPerRow() as usize;
    let samples_per_pixel = rep.samplesPerPixel() as usize;
    let base = rep.bitmapData();
    if base.is_null() {
        return None;
    }
    let data = unsafe { std::slice::from_raw_parts(base, bytes_per_row * height) }.to_vec();
    Some(Bitmap { data, width, height, bytes_per_row, samples_per_pixel })
}

/// `inkBox(_:)`: half-open `(minX, minY, maxX, maxY, width, height)`.
fn ink_box(rep: &Bitmap) -> Option<(usize, usize, usize, usize, usize, usize)> {
    if rep.samples_per_pixel < 4 {
        if rep.width == 0 || rep.height == 0 {
            return None;
        }
        return Some((0, 0, rep.width, rep.height, rep.width, rep.height));
    }
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (rep.width as i64, rep.height as i64, -1i64, -1i64);
    for y in 0..rep.height {
        for x in 0..rep.width {
            if rep.data[y * rep.bytes_per_row + x * rep.samples_per_pixel + 3] > 8 {
                min_x = min_x.min(x as i64);
                max_x = max_x.max(x as i64);
                min_y = min_y.min(y as i64);
                max_y = max_y.max(y as i64);
            }
        }
    }
    if max_x < min_x || max_y < min_y {
        return None;
    }
    let (min_x, min_y, max_x, max_y) = (min_x as usize, min_y as usize, max_x as usize, max_y as usize);
    Some((min_x, min_y, max_x + 1, max_y + 1, max_x - min_x + 1, max_y - min_y + 1))
}

/// Where the widest bright row falls in the content's row span (top-down).
fn crossbar_fraction(rep: &Bitmap) -> f64 {
    let (width, height) = (rep.width, rep.height);
    if width == 0 || height == 0 {
        return 0.5;
    }
    let bright = |x: usize, y: usize| {
        let o = y * rep.bytes_per_row + x * rep.samples_per_pixel;
        rep.data[o] as i64 + rep.data[o + 1] as i64 + rep.data[o + 2] as i64 > 120
    };
    let (mut min_y, mut max_y) = (height as i64, -1i64);
    for y in (0..height).step_by(2) {
        for x in (0..width).step_by(2) {
            if bright(x, y) {
                min_y = min_y.min(y as i64);
                max_y = max_y.max(y as i64);
            }
        }
    }
    if max_y <= min_y {
        return 0.5;
    }
    let (mut widest, mut widest_y) = (0, min_y);
    let mut y = min_y;
    while y <= max_y {
        let count = (0..width).step_by(2).filter(|&x| bright(x, y as usize)).count();
        if count > widest {
            widest = count;
            widest_y = y;
        }
        y += 2;
    }
    (widest_y - min_y) as f64 / (max_y - min_y) as f64
}

const FLOW: &str = "flowchart LR\n    A[\"Start\"] --> B[\"Process\"]\n    B --> C{\"Decision\"}\n    C -->|yes| D[\"Done\"]";

#[test]
fn mermaid_bridge_matches_known_good_path() {
    let sheet = sheet();
    let layer = MermaidLayer { source: FLOW.into(), theme: bridge::theme(&sheet), layout_config: LayoutConfig::default() };
    let known_good = common::with_elk(FLOW, 1, || layer.render_image(2.0)).expect("known-good render");
    let ours = common::with_elk(FLOW, 1, || bridge::image(FLOW, &sheet)).expect("bridge render").ns_image();

    let a = bitmap_rep(&ours).unwrap();
    let b = bitmap_rep(&known_good).unwrap();
    let ink_a = ink_box(&a).expect("bridge render is not blank");
    let ink_b = ink_box(&b).expect("known-good render is not blank");
    let x_scale = ink_b.4 as f64 / ink_a.4 as f64;
    let y_scale = ink_b.5 as f64 / ink_a.5 as f64;
    let (mut differing, mut total) = (0, 0);
    for y in (ink_a.1..ink_a.3).step_by(2) {
        for x in (ink_a.0..ink_a.2).step_by(2) {
            let ox = (ink_b.0 + ((x - ink_a.0) as f64 * x_scale) as usize).min(b.width - 1);
            let oy = (ink_b.1 + ((y - ink_a.1) as f64 * y_scale) as usize).min(b.height - 1);
            let o = y * a.bytes_per_row + x * a.samples_per_pixel;
            let p = oy * b.bytes_per_row + ox * b.samples_per_pixel;
            let d = (a.data[o] as i64 - b.data[p] as i64).abs()
                + (a.data[o + 1] as i64 - b.data[p + 1] as i64).abs()
                + (a.data[o + 2] as i64 - b.data[p + 2] as i64).abs();
            if d > 60 {
                differing += 1;
            }
            total += 1;
        }
    }
    let fraction = differing as f64 / total.max(1) as f64;
    assert!(fraction < 0.05, "bridge no longer matches known-good render path ({fraction})");
}

/// A diagram's image must be the diagram: every edge of the bitmap carries ink.
#[test]
fn mermaid_bridge_trims_to_its_own_ink() {
    let sheet = sheet();
    let source = "sequenceDiagram\n    Agent->>Disk: write temp file\n    Agent->>Disk: rename() over target\n    Disk->>Upleft: FSEvents on parent directory";
    let image = bridge::image(source, &sheet).expect("diagram renders");
    let rep = bitmap_rep(&image.ns_image()).unwrap();
    let ink = ink_box(&rep).expect("diagram has ink");
    assert_eq!(ink.0, 0, "blank columns on the left edge");
    assert_eq!(ink.1, 0, "blank rows on the top edge");
    assert_eq!(ink.2, rep.width, "blank columns on the right edge");
    assert_eq!(ink.3, rep.height, "blank rows on the bottom edge");
    assert_eq!(CGImage::width(Some(&image.cg_image)), rep.width);
}

/// The bridge bitmap reads upright: a "T" crossbar sits in the upper half.
#[test]
fn mermaid_bridge_bitmap_is_upright() {
    let sheet = sheet();
    let image = common::with_elk(FLOW, 1, || bridge::image(FLOW, &sheet)).expect("bridge produced a bitmap");
    let rep = bitmap_rep(&image.ns_image()).unwrap();
    assert!(crossbar_fraction(&rep) <= 0.5, "bridge bitmap has upside-down labels");
}

/// The fragment layer's door, `MermaidRendererBridge.image(source:styleSheet:)`
/// with `MarkdownFragmentImageCaches.mermaid` in front of it: once the
/// renderer is installed it hands back the bridge's own bitmap, and a second
/// call is served from the cache.
#[test]
fn mermaid_fragment_door_is_the_cached_bridge() {
    use upleft_render::fragments::mermaid_fragment::mermaid_image;
    bridge::install_fragment_renderer();
    let sheet = sheet();
    let ours = common::with_elk(FLOW, 1, || mermaid_image(FLOW, &sheet)).expect("cached door renders");
    let direct = common::with_elk(FLOW, 1, || bridge::image(FLOW, &sheet)).expect("bridge render").ns_image();
    let a = bitmap_rep(&ours).unwrap();
    let b = bitmap_rep(&direct).unwrap();
    assert_eq!((a.width, a.height), (b.width, b.height));
    assert!(a.data == b.data, "the cached door drew a different bitmap");
    assert_eq!(ours.size().width, direct.size().width);
    // A hit returns the retained image itself, without rendering again.
    let again = mermaid_image(FLOW, &sheet).expect("cache hit");
    assert!(std::ptr::eq(&*again, &*ours));
    // Whitespace around the source is the same diagram.
    let padded = mermaid_image(&format!("\n  {FLOW}\n\n"), &sheet).expect("cache hit");
    assert!(std::ptr::eq(&*padded, &*ours));
    assert!(mermaid_image("  \n", &sheet).is_none());
}
