//! Port of the `ImageRenderCache` cases of `BoundedImageCacheTests.swift`:
//! the draw path's lookup is a pure miss, the background loader fills the
//! cache, and concurrent requests share one decode while both complete.
//! (`limitsAreEnforced`, the generic cache's case, is in `upleft-math`.)

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSColor, NSImage};
use objc2_foundation::{NSDictionary, NSSize, NSURL};
use upleft_render::fragments::bounded_image_cache::ImageRenderCache;

use crate::support::*;
use crate::{Test, expect};

pub const TESTS: &[Test] = &[
    ("image_cache_async_load_populates_cache", async_load_populates_cache),
    ("image_cache_concurrent_loads_both_complete", concurrent_loads_both_complete),
];

/// Writes a tiny real PNG and returns its URL.
fn make_test_image() -> (Retained<NSURL>, std::path::PathBuf) {
    use objc2::AnyThread;
    let uuid = objc2_foundation::NSUUID::UUID().UUIDString().to_string();
    let path = std::path::PathBuf::from(objc2_foundation::NSTemporaryDirectory().to_string())
        .join(format!("downright-image-cache-{uuid}.png"));
    let image = NSImage::initWithSize(NSImage::alloc(), NSSize::new(4.0, 4.0));
    #[allow(deprecated)]
    image.lockFocus();
    NSColor::whiteColor().setFill();
    upleft_render::appkit_compat::rect_fill(rect(0.0, 0.0, 4.0, 4.0));
    #[allow(deprecated)]
    image.unlockFocus();
    let tiff = image.TIFFRepresentation().expect("tiff");
    let rep = NSBitmapImageRep::imageRepWithData(&tiff).expect("rep");
    let png = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }.expect("png");
    std::fs::write(&path, png.to_vec()).unwrap();
    (upleft_render::fragments::local_asset_policy::file_url(path.to_str().unwrap()), path)
}

fn async_load_populates_cache(_mtm: MainThreadMarker) {
    let (url, path) = make_test_image();
    let cache = ImageRenderCache::new();
    // The draw path's lookup must be a pure miss — no decode, no I/O.
    expect!(cache.cached_image(&url, 16).is_none());
    let loaded: Rc<RefCell<Option<Option<Retained<NSImage>>>>> = Rc::new(RefCell::new(None));
    let sink = loaded.clone();
    cache.load_image_async(&url, 16, move |image| *sink.borrow_mut() = Some(image));
    pump_main_queue(|| loaded.borrow().is_some(), Duration::from_secs(5));
    let _ = std::fs::remove_file(&path);
    expect!(loaded.borrow().as_ref().is_some_and(Option::is_some), "the loader returned nothing");
    // The decode is now cached under the same identity the draw path uses.
    expect!(cache.cached_image(&url, 16).is_some());
}

fn concurrent_loads_both_complete(_mtm: MainThreadMarker) {
    let (url, path) = make_test_image();
    let cache = ImageRenderCache::new();
    let results: Rc<RefCell<Vec<Option<Retained<NSImage>>>>> = Rc::new(RefCell::new(Vec::new()));
    for _ in 0..2 {
        let sink = results.clone();
        cache.load_image_async(&url, 16, move |image| sink.borrow_mut().push(image));
    }
    pump_main_queue(|| results.borrow().len() == 2, Duration::from_secs(5));
    let _ = std::fs::remove_file(&path);
    expect!(results.borrow().len() == 2);
    expect!(results.borrow().iter().all(Option::is_some));
}
