//! Port of `Sources/DownrightApp/AI/MarkdownParseWorker.swift`.
//!
//! The Swift coordinator is an `actor`; here it is a serial dispatch queue
//! that plays the actor's executor. Each actor method is a block on that
//! queue, submitted asynchronously, so messages keep the order their sender
//! issued them (the document's `enqueueParseControl` chain). `await
//! worker.run(...)` suspends the actor without blocking it: the parse runs on
//! the global user-initiated queue (the priority of the document's detached
//! parse task) and its completion hops back onto the actor queue. `await
//! nextResult()` becomes [`MarkdownParseCoordinator::next_result`] with a
//! completion, and the actor's `wake` continuation becomes a stored closure
//! that re-enters the loop.

use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use dispatch2::{DispatchQoS, DispatchQueue, DispatchRetained, GlobalQueueIdentifier};
use upleft_core::ast_diff::ASTDiff;
use upleft_core::contracts::DirtySet;
use upleft_core::model::ParsedDocument;
use upleft_core::parser::MarkdownParser;

/// The revision attached to one immutable source snapshot.
///
/// A value type makes it difficult to accidentally compare a parse result from
/// one source edit with the revision of another edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct MarkdownParseRevision {
    pub raw_value: u64,
}

impl MarkdownParseRevision {
    pub const ZERO: MarkdownParseRevision = MarkdownParseRevision { raw_value: 0 };

    pub const fn new(raw_value: u64) -> MarkdownParseRevision {
        MarkdownParseRevision { raw_value }
    }

    pub fn advanced(self) -> MarkdownParseRevision {
        MarkdownParseRevision { raw_value: self.raw_value.wrapping_add(1) }
    }
}

#[derive(Clone, Debug)]
pub struct MarkdownParseResult {
    pub revision: MarkdownParseRevision,
    pub text: String,
    pub document: Arc<ParsedDocument>,
    pub dirty: DirtySet,
}

/// One immutable source snapshot waiting for the parse lane.
#[derive(Clone, Debug)]
pub struct MarkdownParseRequest {
    pub text: String,
    pub previous: Arc<ParsedDocument>,
    pub revision: MarkdownParseRevision,
}

/// The parse itself: `(text, previous, revision) -> result`. Swift's is
/// `async`; this one runs to completion on whatever background thread calls
/// it, which is where the Swift operation's synchronous body runs too.
pub type Operation =
    Arc<dyn Fn(String, Arc<ParsedDocument>, MarkdownParseRevision) -> MarkdownParseResult + Send + Sync>;

/// Pure parse work.  The app runs the worker in a detached user-initiated
/// task; tests can inject a deterministic closure without sleeping.
#[derive(Clone)]
pub struct MarkdownParseWorker {
    pub operation: Operation,
}

impl Default for MarkdownParseWorker {
    fn default() -> Self {
        MarkdownParseWorker::new(None)
    }
}

impl MarkdownParseWorker {
    /// `init(operation: Operation? = nil)`.
    pub fn new(operation: Option<Operation>) -> MarkdownParseWorker {
        MarkdownParseWorker { operation: operation.unwrap_or_else(|| Arc::new(Self::default_operation)) }
    }

    /// `MarkdownParseWorker { text, previous, revision in … }`.
    pub fn with_operation(
        operation: impl Fn(String, Arc<ParsedDocument>, MarkdownParseRevision) -> MarkdownParseResult + Send + Sync + 'static,
    ) -> MarkdownParseWorker {
        MarkdownParseWorker { operation: Arc::new(operation) }
    }

    pub fn run(&self, text: String, previous: Arc<ParsedDocument>, revision: MarkdownParseRevision) -> MarkdownParseResult {
        (self.operation)(text, previous, revision)
    }

    fn default_operation(text: String, previous: Arc<ParsedDocument>, revision: MarkdownParseRevision) -> MarkdownParseResult {
        let document = MarkdownParser::parse(&text);
        // An empty previous tree means first paint / open — never try to
        // reconcile block-by-block against nothing.
        let dirty =
            if previous.length == 0 { DirtySet::wholesale() } else { ASTDiff::dirty_set(Some(&previous), &document) };
        MarkdownParseResult { revision, text, document, dirty }
    }
}

/// Receives `nextResult()`'s value: `Some(result)`, or `None` once the
/// owning document shut down. Called on the coordinator's queue.
pub type NextResult = Box<dyn FnOnce(Option<MarkdownParseResult>) + Send>;

/// Serial, latest-wins parse coordinator.
///
/// cmark does not provide a cancellation point for a parse already in flight.
/// A cancelled task therefore cannot be used as a concurrency limit: a fast
/// typing burst would start one cmark parse per keystroke.  This coordinator
/// keeps one parse in flight and one pending snapshot.  A newer snapshot
/// replaces the pending snapshot before it starts.  Old results are still
/// checked by the document revision gate when they return.
#[derive(Clone)]
pub struct MarkdownParseCoordinator {
    inner: Arc<Inner>,
}

struct Inner {
    worker: MarkdownParseWorker,
    /// The actor's serial executor.
    queue: DispatchRetained<DispatchQueue>,
    /// Actor-isolated state; only touched from `queue`, one lock per block.
    state: Mutex<State>,
    /// Published on transitions of the busy state — a snapshot queued or a
    /// parse running.  Fired from the actor queue; the owner hops to the main
    /// thread.  Written once by the owning document before any snapshot can
    /// be submitted.
    on_busy_change: OnceLock<Box<dyn Fn(bool) + Send + Sync>>,
}

#[derive(Default)]
struct State {
    pending: Option<MarkdownParseRequest>,
    /// The `nextResult()` continuation parked on an empty lane.
    wake: Option<NextResult>,
    is_suspended: bool,
    is_shutdown: bool,
    in_flight: bool,
    last_published_busy: bool,
}

impl MarkdownParseCoordinator {
    pub fn new(worker: MarkdownParseWorker) -> MarkdownParseCoordinator {
        MarkdownParseCoordinator {
            inner: Arc::new(Inner {
                worker,
                queue: DispatchQueue::new("com.bitemyapp.upleft.parse-coordinator", None),
                state: Mutex::new(State::default()),
                on_busy_change: OnceLock::new(),
            }),
        }
    }

    /// `onBusyChange = …`. Only the first assignment takes effect, as the
    /// Swift contract (written exactly once) allows.
    pub fn set_on_busy_change(&self, handler: impl Fn(bool) + Send + Sync + 'static) {
        let _ = self.inner.on_busy_change.set(Box::new(handler));
    }

    pub fn submit(&self, request: MarkdownParseRequest) {
        self.inner.message(move |inner, state| {
            if state.is_suspended || state.is_shutdown {
                return;
            }
            if let Some(pending) = &state.pending
                && !(pending.revision < request.revision)
            {
                return;
            }
            state.pending = Some(request);
            inner.publish_busy(state);
            inner.wake(state);
        });
    }

    /// Runs a correctness-critical snapshot without waiting behind an obsolete
    /// non-cancellable cmark parse. It overlaps the stale request; the
    /// document revision gate still decides which result may commit.
    ///
    /// A coordinator that is suspended or shut down must not spend a parse on
    /// this request. The returned placeholder carries the request's own text,
    /// so the document's `document.text == text` gate rejects it — same
    /// contract as a result that raced a newer edit.
    pub fn run_immediately(
        &self,
        request: MarkdownParseRequest,
        completion: impl FnOnce(MarkdownParseResult) + Send + 'static,
    ) {
        self.inner.message(move |inner, state| {
            if state.is_suspended || state.is_shutdown {
                return completion(MarkdownParseResult {
                    revision: request.revision,
                    text: request.text,
                    document: ParsedDocument::empty(),
                    dirty: DirtySet::new(Vec::new(), false),
                });
            }
            let worker = inner.worker.clone();
            parse_lane().exec_async(move || {
                completion(worker.run(request.text, request.previous, request.revision));
            });
        });
    }

    /// Drops a snapshot that has not started running.  A synchronous reparse
    /// has already produced a newer tree, so keeping this request would only
    /// spend parse time on a result that the document revision gate rejects.
    pub fn discard_pending(&self) {
        self.inner.message(|inner, state| {
            state.pending = None;
            inner.publish_busy(state);
        });
    }

    pub fn suspend(&self) {
        self.inner.message(|inner, state| {
            state.is_suspended = true;
            state.pending = None;
            inner.publish_busy(state);
            inner.wake(state);
        });
    }

    pub fn resume(&self) {
        self.inner.message(|inner, state| {
            if state.is_shutdown {
                return;
            }
            state.is_suspended = false;
            inner.wake(state);
        });
    }

    pub fn shutdown(&self) {
        self.inner.message(|inner, state| {
            state.is_shutdown = true;
            state.pending = None;
            state.in_flight = false;
            inner.publish_busy(state);
            inner.wake(state);
        });
    }

    /// `nextResult() async -> MarkdownParseResult?`: waits for and runs the
    /// next snapshot, then calls `completion` on the coordinator's queue.
    /// The caller owns the long-lived loop, calling this again once it has
    /// handled a result. `None` means that the owning document shut down.
    pub fn next_result(&self, completion: impl FnOnce(Option<MarkdownParseResult>) + Send + 'static) {
        let completion: NextResult = Box::new(completion);
        self.inner.message(move |inner, state| inner.next_result_step(state, completion));
    }
}

/// The global concurrent queue at the parse task's priority
/// (`Task.detached(priority: .userInitiated)`).
fn parse_lane() -> DispatchRetained<DispatchQueue> {
    DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(DispatchQoS::UserInitiated))
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Sends one actor message: `body` runs on the actor queue, after every
    /// message sent before it.
    fn message(self: &Arc<Self>, body: impl FnOnce(&Arc<Inner>, &mut State) + Send + 'static) {
        let inner = self.clone();
        self.queue.exec_async(move || {
            let mut state = inner.lock();
            body(&inner, &mut state);
        });
    }

    /// `true` while a snapshot is queued or a parse is in flight.
    fn is_busy(state: &State) -> bool {
        state.pending.is_some() || state.in_flight
    }

    fn publish_busy(&self, state: &mut State) {
        let busy = Self::is_busy(state);
        if busy == state.last_published_busy {
            return;
        }
        state.last_published_busy = busy;
        if let Some(handler) = self.on_busy_change.get() {
            handler(busy);
        }
    }

    /// `wake?.resume(); wake = nil`: the parked loop continues after the
    /// current actor message.
    fn wake(self: &Arc<Self>, state: &mut State) {
        if let Some(continuation) = state.wake.take() {
            let inner = self.clone();
            self.queue.exec_async(move || {
                let mut state = inner.lock();
                inner.next_result_step(&mut state, continuation);
            });
        }
    }

    /// One pass of `nextResult()`'s `while true` loop.
    fn next_result_step(self: &Arc<Self>, state: &mut State, completion: NextResult) {
        if let Some(request) = state.pending.take() {
            state.in_flight = true;
            self.publish_busy(state);
            let inner = self.clone();
            parse_lane().exec_async(move || {
                let result = inner.worker.run(request.text, request.previous, request.revision);
                let resumed = inner.clone();
                inner.queue.exec_async(move || {
                    let mut state = resumed.lock();
                    state.in_flight = false;
                    resumed.publish_busy(&mut state);
                    drop(state);
                    completion(Some(result));
                });
            });
            return;
        }
        if state.is_shutdown {
            return completion(None);
        }
        state.wake = Some(completion);
    }
}
