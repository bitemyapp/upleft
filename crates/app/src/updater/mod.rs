//! `Sources/DownrightApp/Updater/`.
//!
//! Every Swift type here is `@MainActor`. The ports are single-threaded
//! (`Rc`, `Cell`, `RefCell`) and live on the main thread; [`main_actor`] holds
//! the two main-queue hops Swift makes (`DispatchQueue.main.async` /
//! `Task { @MainActor in … }`, and `DispatchQueue.main.asyncAfter`), which
//! carry those non-`Send` objects to a later turn of the same thread.

pub mod downright_update_driver;
pub mod release_watch;
pub mod sparkle;
pub mod update_coordinator;
pub mod update_engine;
pub mod update_metadata;
pub mod update_state_machine;

/// Main-queue hops for the updater's main-actor objects.
pub mod main_actor {
    use std::cell::Cell;
    use std::mem::ManuallyDrop;
    use std::rc::Rc;
    use std::time::Duration;

    use dispatch2::{DispatchQueue, DispatchTime};

    /// Whether the caller is on the main thread (`pthread_main_np`).
    pub fn is_main_thread() -> bool {
        // SAFETY: `pthread_main_np` has no preconditions.
        unsafe { libc::pthread_main_np() != 0 }
    }

    /// A value created on the main thread and only ever opened there. The
    /// main queue runs its blocks on the main thread, so a block that carries
    /// one out and back is sound; the checks make any other use a panic
    /// rather than a data race. Dropped off the main thread (a block that was
    /// never run, discarded by another queue), the value is leaked rather
    /// than destroyed on the wrong thread.
    pub(crate) struct MainOnly<T>(ManuallyDrop<T>);

    // SAFETY: the value is only reachable through `into_inner`, which checks
    // that it runs on the main thread, and `Drop` never runs `T`'s destructor
    // elsewhere.
    unsafe impl<T> Send for MainOnly<T> {}

    impl<T> MainOnly<T> {
        pub(crate) fn new(value: T) -> MainOnly<T> {
            assert!(is_main_thread(), "an updater main-actor object was used off the main thread");
            MainOnly(ManuallyDrop::new(value))
        }

        /// The value, on the main thread (a block that the main queue may run
        /// more than once, such as a notification observer).
        pub(crate) fn get(&self) -> &T {
            assert!(is_main_thread(), "an updater main-actor value was opened off the main thread");
            &self.0
        }

        pub(crate) fn into_inner(mut self) -> T {
            assert!(is_main_thread(), "an updater main-actor value was opened off the main thread");
            // SAFETY: taken once; `self` is forgotten so `Drop` does not see it.
            let value = unsafe { ManuallyDrop::take(&mut self.0) };
            std::mem::forget(self);
            value
        }
    }

    impl<T> Drop for MainOnly<T> {
        fn drop(&mut self) {
            if is_main_thread() {
                // SAFETY: dropped once, on the thread that owns the value.
                unsafe { ManuallyDrop::drop(&mut self.0) };
            }
        }
    }

    /// `DispatchQueue.main.async { … }` and `Task { @MainActor in … }` from
    /// the main actor: the work runs on a later turn of the main queue, in
    /// FIFO order with every other main-queue block.
    pub fn async_main(work: impl FnOnce() + 'static) {
        let work = MainOnly::new(work);
        DispatchQueue::main().exec_async(move || (work.into_inner())());
    }

    /// A `DispatchWorkItem`: cancelling it before it runs makes it a no-op.
    #[derive(Clone)]
    pub struct WorkItem {
        cancelled: Rc<Cell<bool>>,
    }

    impl WorkItem {
        /// `DispatchWorkItem.cancel()`.
        pub fn cancel(&self) {
            self.cancelled.set(true);
        }

        pub fn is_cancelled(&self) -> bool {
            self.cancelled.get()
        }
    }

    /// `DispatchQueue.main.asyncAfter(deadline: .now() + interval, execute: work)`
    /// with a fresh `DispatchWorkItem`.
    pub fn async_after(interval: f64, work: impl FnOnce() + 'static) -> WorkItem {
        let item = WorkItem { cancelled: Rc::new(Cell::new(false)) };
        let flag = item.cancelled.clone();
        let payload = MainOnly::new(move || {
            if !flag.get() {
                work();
            }
        });
        // Swift's `DispatchTime + Double` truncates `seconds * NSEC_PER_SEC`
        // to whole nanoseconds.
        let nanoseconds = (interval * 1_000_000_000.0) as i64;
        let when = DispatchTime::try_from(Duration::from_nanos(nanoseconds.max(0) as u64)).unwrap_or(DispatchTime::NOW);
        let _ = DispatchQueue::main().after(when, move || (payload.into_inner())());
        item
    }
}
