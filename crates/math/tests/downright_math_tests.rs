//! Downright's own math tests (Tests/MarkdownRenderTests): the renderer's
//! padding and `\mathop` handling (GeometryProbeTests), the bounded image
//! cache (BoundedImageCacheTests.limitsAreEnforced), and the font-bundle probe
//! (MathFontBundleTests) — the last adapted to Upleft's layout, where the
//! resolver looks for `mathFonts.bundle` itself rather than for SwiftPM's
//! `SwiftMath_SwiftMath.bundle` around it.

use std::path::PathBuf;

use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_app_kit::{NSColor, NSImage};
use objc2_foundation::NSSize;
use upleft_math::downright::bounded_image_cache::BoundedImageCache;
use upleft_math::downright::inline_math_display::inline_attachment_bounds;
use upleft_math::downright::math_font_bundle::MathFontBundle;
use upleft_math::downright::math_renderer::{MathRenderer, trimming_whitespaces_and_newlines};

fn text_color() -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(41.0 / 255.0, 37.0 / 255.0, 34.0 / 255.0, 1.0)
}

// MARK: - GeometryProbeTests

/// P0-2: block math renders through the shared renderer with 8pt of padding
/// on every edge, so a tall formula can never clip through its line box.
#[test]
fn block_math_gets_padding() {
    let color = text_color();
    let unpadded = MathRenderer::image("\\frac{a}{b}", true, 18.0, &color, 0.0)
        .expect("math renderer returned nil");
    let padded = MathRenderer::image("\\frac{a}{b}", true, 18.0, &color, 8.0)
        .expect("math renderer returned nil");
    let (unpadded, padded) = (unpadded.size(), padded.size());
    assert!(
        padded.width - unpadded.width >= 15.9,
        "block math should carry 8pt of air per side"
    );
    assert!(padded.height - unpadded.height >= 15.9);
}

#[test]
fn block_math_supports_operator_wrappers() {
    let formula = r"\mathop{\mathrm{read}}(source) \longrightarrow \mathop{\mathrm{parse}}(tree)";
    let image = MathRenderer::image(formula, true, 16.0, &NSColor::labelColor(), 0.0);
    assert!(
        image.is_some(),
        "common operator wrappers should not fall back to raw LaTeX"
    );
}

#[test]
fn math_op_wrapper_is_dropped_keeping_its_content() {
    assert_eq!(
        MathRenderer::swift_math_source(r"\mathop{\mathrm{d}}x"),
        r"\mathrm{d}x"
    );
    assert_eq!(
        MathRenderer::swift_math_source(r"\mathop{a{b}c}_x"),
        "a{b}c_x"
    );
    assert_eq!(MathRenderer::swift_math_source(r"\mathop{x"), r"\mathop{x");
    assert_eq!(MathRenderer::swift_math_source(r"\mathop x"), r"\mathop x");
    assert_eq!(
        MathRenderer::swift_math_source(r"\frac{a}{b}"),
        r"\frac{a}{b}"
    );
    assert_eq!(MathRenderer::swift_math_source(r"\mathop{\}}"), r"\}");
}

#[test]
fn empty_and_malformed_formulas_render_nil() {
    let color = text_color();
    assert!(MathRenderer::image("", false, 16.0, &color, 0.0).is_none());
    assert!(MathRenderer::image(" \n\t ", false, 16.0, &color, 0.0).is_none());
    assert!(MathRenderer::image("\\frac{", false, 16.0, &color, 0.0).is_none());
    assert!(MathRenderer::image("\\notacommand", true, 16.0, &color, 8.0).is_none());
    assert_eq!(trimming_whitespaces_and_newlines("  x \n"), "x");
}

#[test]
fn a_formula_is_typeset_once_per_source_and_style() {
    let color = text_color();
    let first = MathRenderer::image("x^{cache}", false, 16.1, &color, 0.0).unwrap();
    // Same quantised point size (16.1 and 16.05 both round to 16.0) and source.
    let second = MathRenderer::image(" x^{cache} ", false, 16.05, &color, 0.0).unwrap();
    assert!(std::ptr::eq(&*first, &*second));
    let display = MathRenderer::image("x^{cache}", true, 16.1, &color, 0.0).unwrap();
    assert!(!std::ptr::eq(&*first, &*display));
}

#[test]
fn inline_attachment_is_centred_on_the_x_height() {
    let bounds = inline_attachment_bounds(NSSize::new(20.0, 14.0), 8.0);
    assert_eq!((bounds.origin.x, bounds.origin.y), (0.0, -3.0));
    assert_eq!((bounds.size.width, bounds.size.height), (20.0, 14.0));
}

// MARK: - BoundedImageCacheTests

fn make_image() -> Retained<NSImage> {
    NSImage::initWithSize(NSImage::alloc(), NSSize::new(2.0, 2.0))
}

#[test]
fn limits_are_enforced() {
    let cache = BoundedImageCache::<String>::new(2, 8 * 1024);
    let mut builds = 0;

    for key in ["one", "two"] {
        let _ = cache.image(&key.to_owned(), 1, || {
            builds += 1;
            Some(make_image())
        });
    }
    assert_eq!(cache.count_for_testing(), 2);

    // Refresh one entry, then force an eviction. The least-recently-used
    // value must leave first.
    let _ = cache.image(&"one".to_owned(), 1, || {
        builds += 1;
        Some(make_image())
    });
    let _ = cache.image(&"three".to_owned(), 1, || {
        builds += 1;
        Some(make_image())
    });
    assert_eq!(cache.count_for_testing(), 2);
    // "two" was evicted; rebuilding it increments the create counter.
    let _ = cache.image(&"two".to_owned(), 1, || {
        builds += 1;
        Some(make_image())
    });
    assert_eq!(builds, 4);

    // Nil renders are never retained.
    let before_nil = cache.count_for_testing();
    let _ = cache.image(&"missing".to_owned(), 1, || {
        builds += 1;
        None
    });
    assert_eq!(cache.count_for_testing(), before_nil);
    assert_eq!(builds, 5);
}

// MARK: - MathFontBundleTests (Upleft layout)

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> TemporaryDirectory {
        // Tests run in parallel threads of one process, and the clock is too
        // coarse to tell them apart, so a counter makes each name unique.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let unique = format!(
            "upleft-mathfonts-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let root = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&root).unwrap();
        TemporaryDirectory(root)
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Materialises a complete `mathFonts.bundle` under `root`.
fn make_bundle(root: &std::path::Path) -> PathBuf {
    let fonts = root.join("mathFonts.bundle");
    std::fs::create_dir_all(&fonts).unwrap();
    std::fs::write(fonts.join("latinmodern-math.otf"), b"").unwrap();
    fonts
}

#[test]
fn the_probe_agrees_with_the_resolver() {
    let root = TemporaryDirectory::new();
    let fonts = make_bundle(&root.0);
    assert!(MathFontBundle::probe(std::slice::from_ref(&fonts)));
    assert!(MathFontBundle::resolver_would_find(&[fonts]));
}

#[test]
fn a_root_without_the_font_is_declined_by_both() {
    let root = TemporaryDirectory::new();
    let fonts = root.0.join("mathFonts.bundle");
    assert!(!MathFontBundle::probe(std::slice::from_ref(&fonts)));
    assert!(!MathFontBundle::resolver_would_find(&[fonts]));
}

/// A bundle whose `.otf` was lost in an incomplete copy must not count.
#[test]
fn a_bundle_missing_the_font_file_is_declined_by_both() {
    let root = TemporaryDirectory::new();
    let fonts = make_bundle(&root.0);
    std::fs::remove_file(fonts.join("latinmodern-math.otf")).unwrap();
    assert!(!MathFontBundle::probe(std::slice::from_ref(&fonts)));
    assert!(!MathFontBundle::resolver_would_find(&[fonts]));
}

/// The process has to be able to render math at all, or every math test is
/// passing vacuously.
#[test]
fn the_running_process_can_reach_the_math_fonts() {
    assert!(MathFontBundle::is_available());
}
