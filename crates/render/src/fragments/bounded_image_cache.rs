//! Port of `Fragments/BoundedImageCache.swift`, the parts the render layer
//! owns: `MermaidCacheKey`, `MarkdownFragmentImageCaches.images` and
//! `.mermaid`, and `ImageRenderCache` (the background image loader).
//!
//! The generic `BoundedImageCache<Key>` and the math cache
//! (`MathRendererCacheKey`, `MarkdownFragmentImageCaches.math`) were ported
//! with the math renderer and live in `upleft_math::downright::bounded_image_cache`;
//! the caches here are instances of that same type.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::SystemTime;

use dispatch2::{DispatchQoS, DispatchQueue, GlobalQueueIdentifier, MainThreadBound};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AnyThread, MainThreadMarker, Message};
use objc2_app_kit::NSImage;
use objc2_core_foundation::{CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFURL, CGFloat, CGSize};
use objc2_foundation::{NSNumber, NSString, NSURL};
use objc2_image_io::{
    CGImageSource, kCGImagePropertyHasAlpha, kCGImagePropertyPixelHeight, kCGImagePropertyPixelWidth,
    kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailWithTransform,
    kCGImageSourceShouldCache, kCGImageSourceThumbnailMaxPixelSize,
};
pub use upleft_math::downright::bounded_image_cache::{BoundedImageCache, CachedImage};

/// `MermaidCacheKey`: identity of a rendered diagram. `scale` is the backing
/// pixel scale the diagram was rasterised at.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MermaidCacheKey {
    pub source: String,
    pub style_token: i64,
    pub scale: i64,
}

/// `MarkdownFragmentImageCaches.images`.
pub static IMAGES: LazyLock<ImageRenderCache> = LazyLock::new(ImageRenderCache::new);

/// `MarkdownFragmentImageCaches.mermaid`.
pub static MERMAID: LazyLock<BoundedImageCache<MermaidCacheKey>> =
    LazyLock::new(|| BoundedImageCache::new(48, 24 * 1024 * 1024));

/// `ImageRenderCache.Key`: the standardized file URL and the decode's pixel
/// ceiling. The URL is held as its absolute string (URL equality) plus its
/// path, which is what the loader stats and decodes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    url: String,
    path: String,
    max_pixel_dimension: isize,
}

/// `ImageRenderCache.Freshness`: disk identity of a cached decode.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Freshness {
    file_size: Option<i64>,
    modified: Option<SystemTime>,
}

/// A completion, which only ever runs on the main thread.
type Completion = MainThreadBound<Box<dyn FnOnce(Option<Retained<NSImage>>)>>;

#[derive(Default)]
struct LoaderState {
    freshness: HashMap<String, Option<Freshness>>,
    failed_loads: HashSet<Key>,
    in_flight: HashMap<Key, Vec<Completion>>,
}

struct Inner {
    cache: BoundedImageCache<Key>,
    state: Mutex<LoaderState>,
}

/// Loads only the pixels the image viewport needs, off the main thread. The
/// returned image keeps the source pixel dimensions as its point size.
///
/// The draw path may only call [`ImageRenderCache::cached_image`], a pure
/// in-memory lookup; a miss schedules [`ImageRenderCache::load_image_async`].
#[derive(Clone)]
pub struct ImageRenderCache {
    inner: Arc<Inner>,
}

impl Default for ImageRenderCache {
    fn default() -> Self {
        Self::new()
    }
}

/// `url.standardizedFileURL`, as the key's two strings.
fn key(url: &NSURL, max_pixel_dimension: isize) -> Key {
    let standardized = if url.isFileURL() { url.standardizedURL().unwrap_or_else(|| url.retain()) } else { url.retain() };
    Key {
        url: standardized.absoluteString().map(|string| string.to_string()).unwrap_or_default(),
        path: standardized.path().map(|path| upleft_swift_text::ns::foundation::to_string(&path)).unwrap_or_default(),
        max_pixel_dimension: max_pixel_dimension.max(1),
    }
}

impl ImageRenderCache {
    pub fn new() -> ImageRenderCache {
        ImageRenderCache {
            inner: Arc::new(Inner {
                cache: BoundedImageCache::new(96, 32 * 1024 * 1024),
                state: Mutex::new(LoaderState::default()),
            }),
        }
    }

    /// Pure cache lookup — never touches the filesystem.
    pub fn cached_image(&self, url: &NSURL, max_pixel_dimension: isize) -> Option<Retained<NSImage>> {
        self.inner.cache.cached(&key(url, max_pixel_dimension))
    }

    /// Decodes `url` off the main thread, records it in the cache, then calls
    /// `completion` on the main thread with the result (`None` when the file
    /// is missing or unreadable). Concurrent requests for the same image
    /// share one decode. Must be called on the main thread.
    pub fn load_image_async(
        &self,
        url: &NSURL,
        max_pixel_dimension: isize,
        completion: impl FnOnce(Option<Retained<NSImage>>) + 'static,
    ) {
        let mtm = MainThreadMarker::new().expect("load_image_async is called on the main thread");
        let key = key(url, max_pixel_dimension);
        let completion: Completion = MainThreadBound::new(Box::new(completion), mtm);
        {
            let mut state = self.inner.state.lock().unwrap();
            if let Some(callbacks) = state.in_flight.get_mut(&key) {
                callbacks.push(completion);
                return;
            }
            state.in_flight.insert(key.clone(), vec![completion]);
        }

        let inner = self.inner.clone();
        let queue =
            DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(DispatchQoS::UserInitiated));
        queue.exec_async(move || {
            let image = Self::decode_if_needed(&inner, &key).map(CachedImage);
            let callbacks = inner.state.lock().unwrap().in_flight.remove(&key).unwrap_or_default();
            DispatchQueue::main().exec_async(move || {
                let mtm = MainThreadMarker::new().expect("the main queue runs on the main thread");
                for callback in callbacks {
                    (callback.into_inner(mtm))(image.as_ref().map(|image| image.0.clone()));
                }
            });
        });
    }

    /// Runs on the background queue. Re-decodes when the file is not cached
    /// yet or changed on disk since it was cached.
    fn decode_if_needed(inner: &Inner, key: &Key) -> Option<Retained<NSImage>> {
        let current = freshness(&key.path);
        let cached = inner.cache.cached(key);
        let (changed, is_failed) = {
            let mut state = inner.state.lock().unwrap();
            let changed = state.freshness.get(&key.url) != Some(&Some(current.clone()));
            if changed {
                state.freshness.insert(key.url.clone(), Some(current));
                state.failed_loads.remove(key);
            }
            (changed, state.failed_loads.contains(key))
        };
        if is_failed && !changed {
            return None;
        }
        if let Some(cached) = cached
            && !changed
        {
            return Some(cached);
        }
        let Some(image) = downsampled_image(&key.path, key.max_pixel_dimension) else {
            inner.state.lock().unwrap().failed_loads.insert(key.clone());
            return None;
        };
        inner.state.lock().unwrap().failed_loads.remove(key);
        inner.cache.store(image.clone(), key, key.path.len());
        Some(image)
    }

    pub fn remove_all(&self) {
        self.inner.cache.remove_all();
    }

    pub fn count_for_testing(&self) -> usize {
        self.inner.cache.count_for_testing()
    }
}

/// `url.resourceValues(forKeys: [.fileSizeKey, .contentModificationDateKey])`.
fn freshness(path: &str) -> Freshness {
    match std::fs::metadata(path) {
        Ok(metadata) => Freshness {
            file_size: metadata.is_file().then_some(metadata.len() as i64),
            modified: metadata.modified().ok(),
        },
        Err(_) => Freshness { file_size: None, modified: None },
    }
}

/// `ImageRenderCache.downsampledImage(at:maxPixelDimension:)`.
fn downsampled_image(path: &str, max_pixel_dimension: isize) -> Option<Retained<NSImage>> {
    objc2::rc::autoreleasepool(|_| {
        let url = NSURL::fileURLWithPath(&NSString::from_str(path));
        // SAFETY: NSURL is toll-free bridged to CFURL.
        let cf_url: &CFURL = unsafe { &*(Retained::as_ptr(&url) as *const CFURL) };
        // SAFETY: a null options dictionary is allowed.
        let source: CFRetained<CGImageSource> = unsafe { CGImageSource::with_url(cf_url, None) }?;
        // SAFETY: index 0 with no options.
        let properties: Option<CFRetained<CFDictionary>> = unsafe { source.properties_at_index(0, None) };
        let number = |key: &CFString| -> Option<CGFloat> {
            let properties = properties.as_ref()?;
            // SAFETY: the dictionary's keys and values are CF objects.
            let value = unsafe { properties.value(key as *const CFString as *const _) };
            if value.is_null() {
                return None;
            }
            // SAFETY: a non-null CF object from the property dictionary.
            let object: &AnyObject = unsafe { &*(value as *const AnyObject) };
            object.downcast_ref::<NSNumber>().map(|number| number.doubleValue())
        };
        // SAFETY: ImageIO exports these keys as immutable globals.
        let (width_key, height_key, alpha_key) =
            unsafe { (kCGImagePropertyPixelWidth, kCGImagePropertyPixelHeight, kCGImagePropertyHasAlpha) };
        let source_width = number(width_key);
        let source_height = number(height_key);
        let declared_alpha: Option<bool> = properties.as_ref().and_then(|properties| {
            // SAFETY: as above.
            let value = unsafe { properties.value(alpha_key as *const CFString as *const _) };
            if value.is_null() {
                return None;
            }
            let object: &AnyObject = unsafe { &*(value as *const AnyObject) };
            // `as? Bool` bridges only an NSNumber holding 0 or 1.
            object.downcast_ref::<NSNumber>().and_then(|number| match number.integerValue() {
                0 if number.doubleValue() == 0.0 => Some(false),
                1 if number.doubleValue() == 1.0 => Some(true),
                _ => None,
            })
        });
        // SAFETY: ImageIO exports these keys as immutable globals.
        let options = unsafe {
            let keys: [&CFString; 4] = [
                kCGImageSourceCreateThumbnailFromImageAlways,
                kCGImageSourceCreateThumbnailWithTransform,
                kCGImageSourceThumbnailMaxPixelSize,
                kCGImageSourceShouldCache,
            ];
            let dimension = CFNumber::new_isize(max_pixel_dimension);
            let values: [&objc2_core_foundation::CFType; 4] = [
                CFBoolean::new(true),
                CFBoolean::new(true),
                &dimension,
                CFBoolean::new(false),
            ];
            CFDictionary::from_slices(&keys, &values)
        };
        // SAFETY: the options dictionary holds the documented key and value types.
        let cg_image = unsafe { source.thumbnail_at_index(0, Some(options.as_opaque())) }?;
        let size = CGSize::new(
            source_width.unwrap_or(objc2_core_graphics::CGImage::width(Some(&cg_image)) as CGFloat),
            source_height.unwrap_or(objc2_core_graphics::CGImage::height(Some(&cg_image)) as CGFloat),
        );
        let image = NSImage::initWithCGImage_size(NSImage::alloc(), &cg_image, size);
        // The source is the authority on transparency.
        if let Some(declared) = declared_alpha
            && let Some(first) = image.representations().firstObject()
        {
            first.setAlpha(declared);
        }
        Some(image)
    })
}
