//! `BoundedImageCache.swift` (the part the math renderer uses): a small,
//! cost-bounded LRU cache of rendered images, and the math cache's key.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{LazyLock, Mutex};

use objc2::rc::Retained;
use objc2_app_kit::NSImage;
use objc2_core_foundation::CGFloat;

/// An image the cache hands between threads, as the Swift cache does.
#[derive(Clone)]
pub struct CachedImage(pub Retained<NSImage>);

// SAFETY: the cached images are immutable once created; Downright's cache
// shares them across threads the same way.
unsafe impl Send for CachedImage {}
unsafe impl Sync for CachedImage {}

struct Entry {
    image: Option<CachedImage>,
    cost: usize,
    stamp: u64,
}

struct State<K> {
    entries: HashMap<K, Entry>,
    total_cost: usize,
    stamp: u64,
}

/// Entries are evicted by least-recent use until both the count and the
/// estimated decoded-pixel budgets hold. A value too large for the budget is
/// returned to the caller but never retained.
pub struct BoundedImageCache<K: Hash + Eq + Clone> {
    count_limit: usize,
    total_cost_limit: usize,
    state: Mutex<State<K>>,
}

impl<K: Hash + Eq + Clone> BoundedImageCache<K> {
    pub fn new(count_limit: usize, total_cost_limit: usize) -> Self {
        BoundedImageCache {
            count_limit: count_limit.max(1),
            total_cost_limit: total_cost_limit.max(1),
            state: Mutex::new(State {
                entries: HashMap::new(),
                total_cost: 0,
                stamp: 0,
            }),
        }
    }

    pub fn image(
        &self,
        key: &K,
        key_cost: usize,
        create: impl FnOnce() -> Option<Retained<NSImage>>,
    ) -> Option<Retained<NSImage>> {
        if let Some(hit) = self.touch(key) {
            return hit;
        }

        let image = create();
        // A nil render (malformed formula, unparseable diagram) must not be
        // cached: it costs nothing to reproduce and would otherwise pin the
        // failure in memory while the document scrolls past it.
        let image = image?;
        let cost = Self::cost(&image).saturating_add(key_cost.max(1));
        if cost > self.total_cost_limit {
            return Some(image);
        }

        let mut state = self.state.lock().unwrap();
        state.stamp = state.stamp.wrapping_add(1);
        let stamp = state.stamp;
        if let Some(replaced) = state.entries.insert(
            key.clone(),
            Entry {
                image: Some(CachedImage(image.clone())),
                cost,
                stamp,
            },
        ) {
            state.total_cost -= replaced.cost;
        }
        state.total_cost += cost;
        self.trim_locked(&mut state);
        Some(image)
    }

    /// A hit refreshes the entry's stamp; a miss leaves the clock alone.
    fn touch(&self, key: &K) -> Option<Option<Retained<NSImage>>> {
        let mut state = self.state.lock().unwrap();
        if !state.entries.contains_key(key) {
            return None;
        }
        state.stamp = state.stamp.wrapping_add(1);
        let stamp = state.stamp;
        let hit = state.entries.get_mut(key).unwrap();
        hit.stamp = stamp;
        Some(hit.image.as_ref().map(|image| image.0.clone()))
    }

    /// Pure lookup that never runs a render.
    pub fn cached(&self, key: &K) -> Option<Retained<NSImage>> {
        self.touch(key).flatten()
    }

    /// Records a value produced elsewhere, with the same admission rules.
    pub fn store(&self, image: Retained<NSImage>, key: &K, key_cost: usize) {
        let cost = Self::cost(&image).saturating_add(key_cost.max(1));
        if cost > self.total_cost_limit {
            return;
        }
        let mut state = self.state.lock().unwrap();
        state.stamp = state.stamp.wrapping_add(1);
        let stamp = state.stamp;
        if let Some(replaced) = state.entries.insert(
            key.clone(),
            Entry {
                image: Some(CachedImage(image)),
                cost,
                stamp,
            },
        ) {
            state.total_cost -= replaced.cost;
        }
        state.total_cost += cost;
        self.trim_locked(&mut state);
    }

    pub fn remove_all(&self) {
        let mut state = self.state.lock().unwrap();
        state.entries.clear();
        state.total_cost = 0;
    }

    pub fn count_for_testing(&self) -> usize {
        self.state.lock().unwrap().entries.len()
    }

    fn trim_locked(&self, state: &mut State<K>) {
        while state.entries.len() > self.count_limit || state.total_cost > self.total_cost_limit {
            let Some(victim) = state
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.stamp)
                .map(|(key, _)| key.clone())
            else {
                return;
            };
            let entry = state.entries.remove(&victim).unwrap();
            state.total_cost -= entry.cost;
        }
    }

    fn cost(image: &NSImage) -> usize {
        let pixels = image.representations().iter().fold(0usize, |largest, rep| {
            largest.max(safe_product(rep.pixelsWide(), rep.pixelsHigh()))
        });
        safe_product(pixels.max(1) as isize, 4)
    }
}

fn safe_product(lhs: isize, rhs: isize) -> usize {
    if lhs <= 0 || rhs <= 0 {
        return 1;
    }
    lhs.checked_mul(rhs)
        .map_or(usize::MAX, |product| product as usize)
}

/// Identity of a typeset formula in the shared math cache. `padding` is part
/// of the identity: a padded block formula is a different bitmap.
#[derive(Clone, Debug)]
pub struct MathRendererCacheKey {
    pub source: String,
    pub display: bool,
    pub point_size: CGFloat,
    pub color_token: String,
    pub padding: CGFloat,
}

impl PartialEq for MathRendererCacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
            && self.display == other.display
            && self.point_size == other.point_size
            && self.color_token == other.color_token
            && self.padding == other.padding
    }
}

impl Eq for MathRendererCacheKey {}

/// Swift's `Double` hashing treats `-0.0` as `0.0`.
fn hash_double<H: Hasher>(value: CGFloat, state: &mut H) {
    let value = if value == 0.0 { 0.0 } else { value };
    value.to_bits().hash(state);
}

impl Hash for MathRendererCacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.source.hash(state);
        self.display.hash(state);
        hash_double(self.point_size, state);
        self.color_token.hash(state);
        hash_double(self.padding, state);
    }
}

/// `MarkdownFragmentImageCaches.math`.
pub static MATH: LazyLock<BoundedImageCache<MathRendererCacheKey>> =
    LazyLock::new(|| BoundedImageCache::new(128, 16 * 1024 * 1024));
