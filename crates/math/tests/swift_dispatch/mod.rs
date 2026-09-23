//! Support shared by the ports of SwiftMath's concurrency tests
//! (`MTFontV2Tests`, `MTFontMathTableV2Tests`, `MathFontTests`,
//! `MathImageTests`, `ConcurrencyThreadsafeTests`): Swift's
//! `Int.random(in:)`, `CGFloat.random(in:)` and `randomElement()`, and a
//! concurrent `DispatchQueue` + `DispatchGroup` built on std threads.

#![allow(dead_code)]

use std::collections::VecDeque;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::sync::{Mutex, mpsc};

use upleft_math::math_bundle::math_font::MathFont;

/// A fresh random `u64` (each `RandomState` has its own random SipHash keys).
pub fn random_u64() -> u64 {
    RandomState::new().hash_one(0u64)
}

/// `Int.random(in: lo ... hi)`.
pub fn random_int(lo: i64, hi: i64) -> i64 {
    let span = (hi - lo + 1) as u64;
    lo + (random_u64() % span) as i64
}

/// `CGFloat.random(in: lo ... hi)`.
pub fn random_cgfloat(lo: f64, hi: f64) -> f64 {
    let unit = (random_u64() >> 11) as f64 / (1u64 << 53) as f64;
    lo + unit * (hi - lo)
}

/// `items.randomElement()!`.
pub fn random_element<T: Copy>(items: &[T]) -> T {
    items[(random_u64() % items.len() as u64) as usize]
}

/// `MathFont.allCases.randomElement()!`.
pub fn random_math_font() -> MathFont {
    random_element(&MathFont::ALL_CASES)
}

/// A `DispatchWorkItem` and the block its `notify(queue: .main)` runs.
pub struct WorkItem<'a> {
    work: Box<dyn FnOnce() + Send + 'a>,
    notify: Box<dyn FnOnce() + 'a>,
}

impl<'a> WorkItem<'a> {
    pub fn new(work: impl FnOnce() + Send + 'a, notify: impl FnOnce() + 'a) -> Self {
        WorkItem {
            work: Box::new(work),
            notify: Box::new(notify),
        }
    }
}

/// `queue.async(group: group, execute: workitem)` on a concurrent queue for
/// every item, then `group.wait()`.
///
/// The work runs on a pool of worker threads, one per available core. Each
/// item's `notify` block runs on the calling thread (the tests' `.main`) as
/// soon as its work has finished, while the remaining work continues. Returns
/// once every work item and notify block has run; a panic in a work item
/// (a failed assertion) fails the caller.
///
/// In the Swift tests the main-queue `notify` blocks cannot run while
/// `group.wait()` blocks the main thread; running them here, as the work
/// completes, is what the tests' final `testCount == totalCases` assertions
/// intend.
pub fn dispatch_group_wait(items: Vec<WorkItem<'_>>) {
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    let (works, mut notifies): (Vec<_>, Vec<_>) = items
        .into_iter()
        .map(|item| (item.work, Some(item.notify)))
        .unzip();
    let queue = Mutex::new(works.into_iter().enumerate().collect::<VecDeque<_>>());
    let (done_tx, done_rx) = mpsc::channel::<usize>();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let done_tx = done_tx.clone();
            let queue = &queue;
            scope.spawn(move || {
                loop {
                    let next = queue.lock().unwrap().pop_front();
                    let Some((index, work)) = next else { break };
                    work();
                    done_tx.send(index).unwrap();
                }
            });
        }
        drop(done_tx);
        for index in done_rx {
            let notify = notifies[index].take().expect("one notify per work item");
            notify();
        }
    });
}
