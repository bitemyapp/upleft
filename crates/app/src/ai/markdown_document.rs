//! Port of `Sources/DownrightApp/AI/MarkdownDocument.swift`.
//!
//! The document: raw text, the tree over it, and everything that watches it.
//!
//! §3.1 is enforced here and nowhere else needs to worry about it — the
//! `NSTextStorage` holds the file's exact characters at all times, and every
//! mutation goes through [`MarkdownDocument::replace`], which is a plain text
//! replacement with plain text undo.  There is no document model to
//! re-serialise, so there is nothing that can normalise a file behind the
//! user's back.
//!
//! `MarkdownDocument` is an `NSObject` subclass (Objective-C name
//! `MarkdownDocument`) that is its storage's `NSTextStorageDelegate`, and
//! `MarkdownUndoManager` an `NSUndoManager` subclass (Objective-C name
//! `MarkdownUndoManager`), as in Swift. Both belong to the main thread.
//!
//! Swift's concurrency maps onto dispatch as follows:
//!
//! - `Task { @MainActor … }` and `await self?.method()` from background work
//!   are `DispatchQueue.main` blocks. `[weak self]` captures become the
//!   document's registry id, looked up on the main thread.
//! - `Task.detached(priority: .userInitiated)` is the global user-initiated
//!   queue.
//! - `enqueueParseControl`'s task chain is the parse coordinator's own FIFO
//!   actor queue (`markdown_parse_worker`).
//! - `DispatchWorkItem`s are `dispatch_block_create`d blocks, and
//!   `RunLoop.main.perform(inModes: [.common])` is
//!   `-[NSRunLoop performInModes:block:]`.
//!
//! Like the Swift, `open`, `save` and friends read and write the file
//! synchronously on the caller's (main) thread; the external-write path
//! reads and diffs off the main thread.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use block2::RcBlock;
use dispatch2::{DispatchQoS, DispatchQueue, GlobalQueueIdentifier};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSTextStorage, NSTextStorageDelegate, NSTextStorageEditActions};
use objc2_foundation::{
    NSArray, NSAttributedStringEnumerationOptions, NSInteger, NSNotification, NSNotificationCenter, NSOperationQueue,
    NSRange as FoundationRange, NSRunLoop, NSRunLoopCommonModes, NSString, NSStringCompareOptions, NSUndoManager,
};
use upleft_core::ast_diff::ASTDiff;
use upleft_core::contracts::{ChangeHunk, DirtySet, ParseOptions, TextEdit, TidyRule};
use upleft_core::document_io::{DocumentIO, DocumentIOError, DynError, write_data_atomically};
use upleft_core::model::{ByteFidelity, ParsedDocument};
use upleft_core::parser::MarkdownParser;
use upleft_core::restructure::Restructure;
use upleft_core::text_diff::TextDiff;
use upleft_core::tidy::TidyDocument;
use upleft_foundation::date::Date;
use upleft_foundation::file_manager;
use upleft_foundation::url::FileUrl;
use upleft_render::render_contracts::{FragmentPayload, attribute_keys};
use upleft_swift_text::NSRange;

use super::change_tracker::ChangeTracker;
use super::document_state_store::{DocumentState, DocumentStateStore, ScrollAnchor, ScrollAnchoring};
use super::file_watcher::{self, FileWatcher, WorkItem};
use super::markdown_parse_worker::{
    MarkdownParseCoordinator, MarkdownParseRequest, MarkdownParseResult, MarkdownParseRevision, MarkdownParseWorker,
};
use super::snapshot_store::{self, SnapshotKind, SnapshotStore, VersionRecord};
use crate::support::preferences::{self, Preferences};

// MARK: - Save intents and errors

/// How a save request is allowed to behave when the on-disk version is newer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SaveIntent {
    /// Refuses to overwrite an unacknowledged external change and surfaces the
    /// conflict instead of deciding silently.  Used by every implicit save
    /// path: occlusion autosave, quit, checkbox toggle, and the close alert.
    #[default]
    Normal,
    /// An explicit "keep my changes and write them over the file" decision,
    /// made by the user through the conflict bar.
    KeepMine,
    /// Recreates a path that is missing or replaces one that cannot currently
    /// be read. This intent is only issued from the explicit recovery sheet;
    /// autosave, close, quit, and task toggles must never choose it.
    RecreateFile,
}

/// Swift's untyped error, shared so a failure can be both thrown and handed
/// to `onSaveFailure`.
pub type SharedError = Arc<DynError>;

#[derive(Clone, Debug)]
pub enum SaveError {
    /// The save was refused because writing the buffer would clobber a newer
    /// on-disk version whose conflict had not been resolved.  The conflict has
    /// already been surfaced via `onExternalEvent`.
    BlockedByExternalConflict,
    /// The document disappeared after it was opened. A normal save must not
    /// silently bring it back.
    FileMissing(FileUrl),
    /// The path still exists but could not be read, so overwriting it would be
    /// an uninformed destructive action.
    FileUnreadable(FileUrl, SharedError),
}

impl SaveError {
    /// `errorDescription`.
    pub fn error_description(&self) -> String {
        match self {
            SaveError::BlockedByExternalConflict => "The file changed on disk. Review the conflict before saving.".into(),
            SaveError::FileMissing(_) => {
                "The original file is missing. Choose Save a Copy or explicitly recreate it.".into()
            }
            SaveError::FileUnreadable(_, error) => format!("The original file could not be read: {error}"),
        }
    }
}

/// Everything `save`, `open` and friends can throw.
#[derive(Clone, Debug)]
pub enum DocumentError {
    Save(SaveError),
    /// `CocoaError(.fileNoSuchFile)`: a save with no backing path.
    FileNoSuchFile,
    /// A read, encode or write failure from `DocumentIO`.
    Io(SharedError),
}

impl DocumentError {
    /// `error.localizedDescription`.
    pub fn localized_description(&self) -> String {
        match self {
            DocumentError::Save(error) => error.error_description(),
            DocumentError::FileNoSuchFile => "The file doesn\u{2019}t exist.".into(),
            DocumentError::Io(error) => error.to_string(),
        }
    }

    pub fn save_error(&self) -> Option<&SaveError> {
        match self {
            DocumentError::Save(error) => Some(error),
            _ => None,
        }
    }
}

impl std::fmt::Display for DocumentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.localized_description())
    }
}

impl std::error::Error for DocumentError {}

impl From<SaveError> for DocumentError {
    fn from(error: SaveError) -> Self {
        DocumentError::Save(error)
    }
}

fn io_error(error: impl Into<DynError>) -> DocumentError {
    DocumentError::Io(Arc::new(error.into()))
}

// MARK: - Undo manager

/// Instance state of `MarkdownUndoManager`.
pub struct MarkdownUndoManagerIvars {
    on_will_apply_undo_redo: RefCell<Option<Rc<dyn Fn()>>>,
    on_did_apply_undo_redo: RefCell<Option<Rc<dyn Fn()>>>,
    is_applying_undo_redo: Cell<bool>,
}

define_class!(
    /// Owns the command boundary AppKit otherwise keeps private.  Storage
    /// delegate callbacks arrive after the text system has already started
    /// moving the selection, which is too late to remember the reader's
    /// camera.
    // SAFETY:
    // - NSUndoManager may be subclassed; `undo` and `redo` keep their
    //   signatures and call super.
    // - Main-thread only, as the Swift class is `@MainActor`.
    // - The ivars need no custom Drop.
    #[unsafe(super(NSUndoManager, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MarkdownUndoManager"]
    #[ivars = MarkdownUndoManagerIvars]
    pub struct MarkdownUndoManager;

    impl MarkdownUndoManager {
        #[unsafe(method(undo))]
        fn undo_override(&self) {
            self.around_undo_redo(|| {
                // SAFETY: `-undo` on the superclass.
                let () = unsafe { msg_send![super(self), undo] };
            });
        }

        #[unsafe(method(redo))]
        fn redo_override(&self) {
            self.around_undo_redo(|| {
                // SAFETY: `-redo` on the superclass.
                let () = unsafe { msg_send![super(self), redo] };
            });
        }
    }
);

impl MarkdownUndoManager {
    pub fn new(mtm: MainThreadMarker) -> Retained<MarkdownUndoManager> {
        let this = Self::alloc(mtm).set_ivars(MarkdownUndoManagerIvars {
            on_will_apply_undo_redo: RefCell::new(None),
            on_did_apply_undo_redo: RefCell::new(None),
            is_applying_undo_redo: Cell::new(false),
        });
        // SAFETY: NSUndoManager's designated initialiser.
        unsafe { msg_send![super(this), init] }
    }

    pub fn set_on_will_apply_undo_redo(&self, callback: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_will_apply_undo_redo.borrow_mut() = callback;
    }

    pub fn set_on_did_apply_undo_redo(&self, callback: Option<Rc<dyn Fn()>>) {
        *self.ivars().on_did_apply_undo_redo.borrow_mut() = callback;
    }

    /// `private(set) var isApplyingUndoRedo`.
    pub fn is_applying_undo_redo(&self) -> bool {
        self.ivars().is_applying_undo_redo.get()
    }

    /// `onWillApplyUndoRedo?()`, then the flag around `super`, then, as
    /// Swift's `defer`, the flag cleared and `onDidApplyUndoRedo?()`.
    fn around_undo_redo(&self, body: impl FnOnce()) {
        let will = self.ivars().on_will_apply_undo_redo.borrow().clone();
        if let Some(will) = will {
            will();
        }
        self.ivars().is_applying_undo_redo.set(true);
        struct Deferred<'a>(&'a MarkdownUndoManager);
        impl Drop for Deferred<'_> {
            fn drop(&mut self) {
                self.0.ivars().is_applying_undo_redo.set(false);
                let did = self.0.ivars().on_did_apply_undo_redo.borrow().clone();
                if let Some(did) = did {
                    did();
                }
            }
        }
        let _deferred = Deferred(self);
        body();
    }
}

// MARK: - Document types

/// The save boundary's complete view of the backing path. Read failures are
/// data, not an absence of evidence: only `.unchanged` permits an implicit
/// write.
#[derive(Clone, Debug)]
pub enum DiskState {
    Unchanged { data: Vec<u8>, fidelity: ByteFidelity },
    Changed { text: String, hunks: Vec<ChangeHunk>, data: Vec<u8>, fidelity: ByteFidelity },
    Missing,
    Unreadable(SharedError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Neutral,
    Edited,
    Saving,
    Saved,
    ChangedOnDisk,
    Conflict,
    SaveFailed,
}

/// One explicit state vocabulary for native chrome and accessibility.
/// Provenance is intentionally a short action label rather than a second
/// state machine; it is useful in a tooltip without cluttering the toolbar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresentationState {
    pub phase: Phase,
    pub provenance: Option<String>,
    pub detail: Option<String>,
}

impl PresentationState {
    pub const NEUTRAL: PresentationState = PresentationState { phase: Phase::Neutral, provenance: None, detail: None };

    pub fn new(phase: Phase, provenance: Option<String>, detail: Option<String>) -> PresentationState {
        PresentationState { phase, provenance, detail }
    }
}

/// What an external write did while the buffer was dirty (§8.1).
#[derive(Clone, Debug)]
pub struct Conflict {
    pub incoming_text: String,
    pub hunks: Vec<ChangeHunk>,
    pub changed_block_count: isize,
    pub incoming_fidelity: Option<ByteFidelity>,
    pub incoming_byte_hash: Option<String>,
}

#[derive(Clone, Debug)]
pub enum ExternalEvent {
    /// Applied in place; scroll position preserved by the delegate.
    Applied { hunks: Vec<ChangeHunk> },
    /// Buffer was dirty — never clobber.  Ask the user.
    Conflict(Conflict),
    FileRemoved,
    FileRestored,
}

/// Why the previous text cannot be shown.  Worth distinguishing: one is a
/// normal consequence of the retention settings, the other is damage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unavailable {
    /// Dropped by the age or size cap.
    Pruned,
    /// The object is there but does not decompress or does not hash back to
    /// its own name.
    Corrupt,
}

/// What the app can say about changes made since the reader last reviewed
/// this document (§8.2).
///
/// The third case is the point: the bytes moved, the previous text has been
/// pruned, and the honest answer is "this changed since you last read it, but
/// I can no longer show you what" — not silence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnreadChanges {
    None,
    Marked { count: usize },
    PreviousVersionUnavailable { reason: Unavailable },
}

struct PendingExternalRestore {
    previous_text: String,
    selection: NSRange,
    anchor: ScrollAnchor,
}

/// Everything the background half of `absorbExternalWrite` prepared.
struct PreparedExternalWrite {
    /// Carried as in Swift; the main-actor half checks the generation it is
    /// handed alongside.
    #[allow(dead_code)]
    generation: u64,
    url: FileUrl,
    captured_text: String,
    baseline: String,
    incoming: String,
    fidelity: ByteFidelity,
    incoming_hash: String,
    incoming_byte_hash: String,
    baseline_hunks: Vec<ChangeHunk>,
    application_hunks: Vec<ChangeHunk>,
}

type Callback<F> = RefCell<Option<Rc<F>>>;

/// Upper bound on unconsumed suppression tokens. Each one holds a
/// whole-document string; the bound keeps a run of unmatched callbacks from
/// accumulating unbounded state and keeps the exact-match scan in
/// `handle_text_storage_edit` cheap.
const MAXIMUM_IGNORED_EXTERNAL_CALLBACK_TEXTS: usize = 8;
const EXTERNAL_WRITE_QUIET_PERIOD: f64 = 0.25;

/// Instance state of `MarkdownDocument` (Swift's stored properties).
pub struct MarkdownDocumentIvars {
    id: u64,
    storage: Retained<NSTextStorage>,
    url: RefCell<Option<FileUrl>>,
    fidelity: Cell<ByteFidelity>,
    parsed: RefCell<Arc<ParsedDocument>>,
    is_dirty: Cell<bool>,
    /// Hash of what is currently on disk, as far as we know.
    disk_hash: RefCell<String>,
    /// Raw-byte generation token for save compare-and-swap. `disk_hash` is a
    /// text/review identity and deliberately cannot distinguish BOM or CRLF.
    disk_byte_hash: RefCell<String>,
    /// Last text generation successfully read from or committed to disk.
    last_committed_text: RefCell<String>,
    /// An external write the buffer has not yet been reconciled with.
    pending_conflict: RefCell<Option<Conflict>>,
    presentation_state: RefCell<PresentationState>,
    /// The document as the reader last **finished reviewing** it.
    review_baseline_text: RefCell<String>,
    review_baseline_hash: RefCell<String>,
    unread_changes: Cell<UnreadChanges>,

    changes: ChangeTracker,
    state: RefCell<DocumentState>,
    undo_manager: Retained<MarkdownUndoManager>,

    on_reparse: Callback<dyn Fn(&Arc<ParsedDocument>, &DirtySet)>,
    on_will_apply_edits: Callback<dyn Fn(&[TextEdit])>,
    on_parse_activity: Callback<dyn Fn(bool)>,
    on_external_event: Callback<dyn Fn(&ExternalEvent)>,
    before_save_commit_for_testing: Callback<dyn Fn()>,
    on_dirty_changed: Callback<dyn Fn(bool)>,
    on_presentation_state_changed: Callback<dyn Fn(&PresentationState)>,
    on_will_apply_undo_redo: Callback<dyn Fn()>,
    on_save_failure: Callback<dyn Fn(&DocumentError)>,
    current_top_offset_provider: Callback<dyn Fn() -> isize>,
    restore_offset_handler: Callback<dyn Fn(isize)>,
    current_selection_provider: Callback<dyn Fn() -> NSRange>,
    restore_selection_handler: Callback<dyn Fn(NSRange)>,
    on_external_write_activity: Callback<dyn Fn(bool)>,
    on_file_renamed: Callback<dyn Fn(&FileUrl)>,

    watcher: RefCell<Option<FileWatcher>>,
    pending_external_write: RefCell<Option<WorkItem>>,
    external_preparation_generation: Cell<u64>,
    is_absorbing_burst: Cell<bool>,
    reparse_scheduled: Cell<bool>,
    is_applying_external_change: Cell<bool>,
    ignored_external_storage_callback_texts: RefCell<Vec<String>>,
    is_applying_batch: Cell<bool>,
    suppress_reparse: Cell<bool>,
    parse_coordinator: MarkdownParseCoordinator,
    snapshot_store: SnapshotStore,
    document_state_store: &'static DocumentStateStore,
    /// `Preferences.shared`, or an injected instance for tests.
    preferences: Option<&'static Preferences>,
    /// The detached parse loop's cancellation flag, once started.
    parse_task: RefCell<Option<Arc<AtomicBool>>>,
    revision: Cell<MarkdownParseRevision>,
    last_async_parse_revision: Cell<Option<MarkdownParseRevision>>,
    is_closed: Cell<bool>,
    preferences_observation: RefCell<Option<Retained<ProtocolObject<dyn NSObjectProtocol>>>>,
    saved_state_work_item: RefCell<Option<WorkItem>>,
    pending_external_restore: RefCell<Option<PendingExternalRestore>>,
    next_external_dirty_override: RefCell<Option<DirtySet>>,
    force_next_dirty_wholesale: Cell<bool>,
}

impl Drop for MarkdownDocumentIvars {
    /// `deinit`.
    fn drop(&mut self) {
        if let Some(observation) = self.preferences_observation.borrow_mut().take() {
            // SAFETY: an observer this document registered.
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(observation.as_ref()) };
        }
        if let Some(task) = self.parse_task.borrow_mut().take() {
            task.store(true, Ordering::SeqCst);
        }
        if let Some(item) = self.saved_state_work_item.borrow_mut().take() {
            item.cancel();
        }
        self.parse_coordinator.shutdown();
        unregister(self.id);
    }
}

define_class!(
    /// The document: raw text, the tree over it, and everything that
    /// watches it.
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - Main-thread only (`@MainActor` in Swift).
    // - Teardown lives in the ivars' Drop, which touches no Objective-C
    //   superclass state.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MarkdownDocument"]
    #[ivars = MarkdownDocumentIvars]
    pub struct MarkdownDocument;

    unsafe impl NSObjectProtocol for MarkdownDocument {}

    unsafe impl NSTextStorageDelegate for MarkdownDocument {
        #[unsafe(method(textStorage:didProcessEditing:range:changeInLength:))]
        fn text_storage_did_process_editing(
            &self,
            text_storage: &NSTextStorage,
            edited_mask: NSTextStorageEditActions,
            _edited_range: FoundationRange,
            _delta: NSInteger,
        ) {
            if !edited_mask.contains(NSTextStorageEditActions::EditedCharacters) {
                return;
            }
            // Capture the post-edit source while the delegate callback is still
            // describing this storage mutation. The task may run after another
            // edit, so reading `storage.string` in the task would lose the
            // transaction boundary and consume the wrong suppression token.
            let callback_text = ns_to_string(&text_storage.string());
            let id = self.ivars().id;
            on_main_with_document(id, move |document| document.handle_text_storage_edit(callback_text));
        }
    }
);

// MARK: - Registry (`[weak self]` across queues)

thread_local! {
    /// Live documents by id, main thread only. Blocks that hop back to the
    /// main queue carry the id, not the object, the way Swift's `[weak self]`
    /// captures carry a weak reference.
    static DOCUMENTS: RefCell<HashMap<u64, Weak<MarkdownDocument>>> = RefCell::new(HashMap::new());
}

static NEXT_DOCUMENT_ID: AtomicU64 = AtomicU64::new(1);

fn register(document: &MarkdownDocument) {
    DOCUMENTS.with(|documents| {
        documents.borrow_mut().insert(document.ivars().id, Weak::from(document));
    });
}

fn unregister(id: u64) {
    let _ = DOCUMENTS.try_with(|documents| {
        if let Ok(mut documents) = documents.try_borrow_mut() {
            documents.remove(&id);
        }
    });
}

fn document_for(id: u64) -> Option<Retained<MarkdownDocument>> {
    DOCUMENTS.with(|documents| documents.borrow().get(&id).and_then(Weak::load))
}

/// `Task { @MainActor [weak self] in … }`.
fn on_main_with_document(id: u64, body: impl FnOnce(&MarkdownDocument) + Send + 'static) {
    DispatchQueue::main().exec_async(move || {
        if let Some(document) = document_for(id) {
            body(&document);
        }
    });
}

fn user_initiated() -> dispatch2::DispatchRetained<DispatchQueue> {
    DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(DispatchQoS::UserInitiated))
}

// MARK: - Foundation helpers

fn ns_to_string(string: &NSString) -> String {
    upleft_swift_text::ns::foundation::to_string(string)
}

/// `text as NSString`, keeping a leading U+FEFF (`NSString::from_str` would
/// drop it).
fn ns(text: &str) -> Retained<NSString> {
    if text.is_ascii() {
        return NSString::from_str(text);
    }
    upleft_swift_text::ns::foundation::ns_from_utf16(&upleft_swift_text::ns::utf16(text))
}

fn foundation_range(range: NSRange) -> FoundationRange {
    FoundationRange::new(range.location as usize, range.length as usize)
}

fn utf16_length(text: &str) -> isize {
    upleft_swift_text::utf16_count(text)
}

/// Swift `String ==`.
fn same_text(a: &str, b: &str) -> bool {
    upleft_swift_text::str_eq(a, b)
}

impl MarkdownDocument {
    // MARK: - Init

    /// `MarkdownDocument()`: the shared stores and `Preferences.shared`.
    pub fn new(mtm: MainThreadMarker) -> Retained<MarkdownDocument> {
        Self::with_dependencies(
            mtm,
            MarkdownParseWorker::default(),
            SnapshotStore::shared().clone(),
            DocumentStateStore::shared(),
            None,
        )
    }

    /// `init(parseWorker:snapshotStore:documentStateStore:)`. `preferences`
    /// replaces `Preferences.shared` where the Swift reads it (tests pass a
    /// `Preferences::for_testing` instance so they never touch the user's
    /// settings).
    pub fn with_dependencies(
        mtm: MainThreadMarker,
        parse_worker: MarkdownParseWorker,
        snapshot_store: SnapshotStore,
        document_state_store: &'static DocumentStateStore,
        preferences: Option<&'static Preferences>,
    ) -> Retained<MarkdownDocument> {
        let id = NEXT_DOCUMENT_ID.fetch_add(1, Ordering::Relaxed);
        let this = Self::alloc(mtm).set_ivars(MarkdownDocumentIvars {
            id,
            storage: NSTextStorage::new(),
            url: RefCell::new(None),
            fidelity: Cell::new(ByteFidelity::DEFAULT),
            parsed: RefCell::new(ParsedDocument::empty()),
            is_dirty: Cell::new(false),
            disk_hash: RefCell::new(String::new()),
            disk_byte_hash: RefCell::new(String::new()),
            last_committed_text: RefCell::new(String::new()),
            pending_conflict: RefCell::new(None),
            presentation_state: RefCell::new(PresentationState::NEUTRAL),
            review_baseline_text: RefCell::new(String::new()),
            review_baseline_hash: RefCell::new(String::new()),
            unread_changes: Cell::new(UnreadChanges::None),
            changes: ChangeTracker::new(),
            state: RefCell::new(DocumentState::new("")),
            undo_manager: MarkdownUndoManager::new(mtm),
            on_reparse: RefCell::new(None),
            on_will_apply_edits: RefCell::new(None),
            on_parse_activity: RefCell::new(None),
            on_external_event: RefCell::new(None),
            before_save_commit_for_testing: RefCell::new(None),
            on_dirty_changed: RefCell::new(None),
            on_presentation_state_changed: RefCell::new(None),
            on_will_apply_undo_redo: RefCell::new(None),
            on_save_failure: RefCell::new(None),
            current_top_offset_provider: RefCell::new(None),
            restore_offset_handler: RefCell::new(None),
            current_selection_provider: RefCell::new(None),
            restore_selection_handler: RefCell::new(None),
            on_external_write_activity: RefCell::new(None),
            on_file_renamed: RefCell::new(None),
            watcher: RefCell::new(None),
            pending_external_write: RefCell::new(None),
            external_preparation_generation: Cell::new(0),
            is_absorbing_burst: Cell::new(false),
            reparse_scheduled: Cell::new(false),
            is_applying_external_change: Cell::new(false),
            ignored_external_storage_callback_texts: RefCell::new(Vec::new()),
            is_applying_batch: Cell::new(false),
            suppress_reparse: Cell::new(false),
            parse_coordinator: MarkdownParseCoordinator::new(parse_worker),
            snapshot_store,
            document_state_store,
            preferences,
            parse_task: RefCell::new(None),
            revision: Cell::new(MarkdownParseRevision::ZERO),
            last_async_parse_revision: Cell::new(None),
            is_closed: Cell::new(false),
            preferences_observation: RefCell::new(None),
            saved_state_work_item: RefCell::new(None),
            pending_external_restore: RefCell::new(None),
            next_external_dirty_override: RefCell::new(None),
            force_next_dirty_wholesale: Cell::new(false),
        });
        // SAFETY: NSObject's designated initialiser.
        let this: Retained<MarkdownDocument> = unsafe { msg_send![super(this), init] };
        register(&this);

        this.ivars().parse_coordinator.set_on_busy_change(move |busy| {
            on_main_with_document(id, move |document| {
                let callback = document.ivars().on_parse_activity.borrow().clone();
                if let Some(callback) = callback {
                    callback(busy);
                }
            });
        });
        this.storage().setDelegate(Some(ProtocolObject::from_ref(&*this)));
        this.undo_manager().setGroupsByEvent(false);
        // External rewrites can register whole-document undo payloads for as
        // long as a window remains open. Bound the stack like a conventional
        // editor so a busy file cannot grow memory without limit.
        this.undo_manager().setLevelsOfUndo(200);
        this.undo_manager().set_on_did_apply_undo_redo(Some(Rc::new(move || {
            if let Some(document) = document_for(id) {
                document.finish_undo_redo();
            }
        })));
        // Clearing marks *is* finishing a review, wherever the call comes from,
        // so the baseline moves with it and the next agent write is measured
        // from what the user just signed off on.
        this.ivars().changes.set_on_reviewed(Some(Box::new(move || {
            if let Some(document) = document_for(id) {
                let text = document.text();
                document.advance_review_baseline(&text);
            }
        })));
        let block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
            if let Some(document) = document_for(id) {
                document.preferences_did_change();
            }
        });
        // SAFETY: the block only touches the document on the main queue,
        // which is where the main operation queue runs it.
        let observation = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(&NSString::from_str(preferences::DID_CHANGE)),
                None,
                Some(&NSOperationQueue::mainQueue()),
                &block,
            )
        };
        *this.ivars().preferences_observation.borrow_mut() = Some(observation);
        this
    }

    fn preferences(&self) -> &'static Preferences {
        self.ivars().preferences.unwrap_or_else(Preferences::shared)
    }

    // MARK: - Accessors

    pub fn storage(&self) -> &NSTextStorage {
        &self.ivars().storage
    }

    pub fn undo_manager(&self) -> &MarkdownUndoManager {
        &self.ivars().undo_manager
    }

    pub fn changes(&self) -> &ChangeTracker {
        &self.ivars().changes
    }

    pub fn url(&self) -> Option<FileUrl> {
        self.ivars().url.borrow().clone()
    }

    pub fn fidelity(&self) -> ByteFidelity {
        self.ivars().fidelity.get()
    }

    pub fn parsed(&self) -> Arc<ParsedDocument> {
        self.ivars().parsed.borrow().clone()
    }

    pub fn is_dirty(&self) -> bool {
        self.ivars().is_dirty.get()
    }

    pub fn disk_hash(&self) -> String {
        self.ivars().disk_hash.borrow().clone()
    }

    pub fn pending_conflict(&self) -> Option<Conflict> {
        self.ivars().pending_conflict.borrow().clone()
    }

    pub fn presentation_state(&self) -> PresentationState {
        self.ivars().presentation_state.borrow().clone()
    }

    pub fn review_baseline_text(&self) -> String {
        self.ivars().review_baseline_text.borrow().clone()
    }

    pub fn review_baseline_hash(&self) -> String {
        self.ivars().review_baseline_hash.borrow().clone()
    }

    pub fn unread_changes(&self) -> UnreadChanges {
        self.ivars().unread_changes.get()
    }

    pub fn state(&self) -> DocumentState {
        self.ivars().state.borrow().clone()
    }

    pub fn set_state(&self, state: DocumentState) {
        *self.ivars().state.borrow_mut() = state;
    }

    pub fn revision(&self) -> MarkdownParseRevision {
        self.ivars().revision.get()
    }

    pub fn last_async_parse_revision(&self) -> Option<MarkdownParseRevision> {
        self.ivars().last_async_parse_revision.get()
    }

    /// `var text: String { storage.string }`.
    pub fn text(&self) -> String {
        ns_to_string(&self.storage().string())
    }

    fn storage_length(&self) -> isize {
        self.storage().length() as isize
    }

    pub fn display_name(&self) -> String {
        self.url().map_or_else(|| "Untitled".to_owned(), |url| url.deleting_path_extension().last_path_component())
    }

    // MARK: - Callbacks

    pub fn set_on_reparse(&self, callback: Option<impl Fn(&Arc<ParsedDocument>, &DirtySet) + 'static>) {
        *self.ivars().on_reparse.borrow_mut() = callback.map(|callback| Rc::new(callback) as Rc<_>);
    }

    pub fn set_on_will_apply_edits(&self, callback: Option<impl Fn(&[TextEdit]) + 'static>) {
        *self.ivars().on_will_apply_edits.borrow_mut() = callback.map(|callback| Rc::new(callback) as Rc<_>);
    }

    pub fn set_on_parse_activity(&self, callback: Option<impl Fn(bool) + 'static>) {
        *self.ivars().on_parse_activity.borrow_mut() = callback.map(|callback| Rc::new(callback) as Rc<_>);
    }

    pub fn set_on_external_event(&self, callback: Option<impl Fn(&ExternalEvent) + 'static>) {
        *self.ivars().on_external_event.borrow_mut() = callback.map(|callback| Rc::new(callback) as Rc<_>);
    }

    /// Deterministic filesystem-race seam. Production never assigns it.
    pub fn set_before_save_commit_for_testing(&self, callback: Option<impl Fn() + 'static>) {
        *self.ivars().before_save_commit_for_testing.borrow_mut() = callback.map(|callback| Rc::new(callback) as Rc<_>);
    }

    pub fn set_on_dirty_changed(&self, callback: Option<impl Fn(bool) + 'static>) {
        *self.ivars().on_dirty_changed.borrow_mut() = callback.map(|callback| Rc::new(callback) as Rc<_>);
    }

    pub fn set_on_presentation_state_changed(&self, callback: Option<impl Fn(&PresentationState) + 'static>) {
        *self.ivars().on_presentation_state_changed.borrow_mut() = callback.map(|callback| Rc::new(callback) as Rc<_>);
    }

    /// `onWillApplyUndoRedo`, whose `didSet` hands it to the undo manager.
    pub fn set_on_will_apply_undo_redo(&self, callback: Option<impl Fn() + 'static>) {
        let callback: Option<Rc<dyn Fn()>> = callback.map(|callback| Rc::new(callback) as Rc<_>);
        *self.ivars().on_will_apply_undo_redo.borrow_mut() = callback.clone();
        self.undo_manager().set_on_will_apply_undo_redo(callback);
    }

    pub fn set_on_save_failure(&self, callback: Option<impl Fn(&DocumentError) + 'static>) {
        *self.ivars().on_save_failure.borrow_mut() = callback.map(|callback| Rc::new(callback) as Rc<_>);
    }

    pub fn set_current_top_offset_provider(&self, callback: Option<impl Fn() -> isize + 'static>) {
        *self.ivars().current_top_offset_provider.borrow_mut() = callback.map(|callback| Rc::new(callback) as Rc<_>);
    }

    pub fn set_restore_offset_handler(&self, callback: Option<impl Fn(isize) + 'static>) {
        *self.ivars().restore_offset_handler.borrow_mut() = callback.map(|callback| Rc::new(callback) as Rc<_>);
    }

    pub fn set_current_selection_provider(&self, callback: Option<impl Fn() -> NSRange + 'static>) {
        *self.ivars().current_selection_provider.borrow_mut() = callback.map(|callback| Rc::new(callback) as Rc<_>);
    }

    pub fn set_restore_selection_handler(&self, callback: Option<impl Fn(NSRange) + 'static>) {
        *self.ivars().restore_selection_handler.borrow_mut() = callback.map(|callback| Rc::new(callback) as Rc<_>);
    }

    pub fn set_on_external_write_activity(&self, callback: Option<impl Fn(bool) + 'static>) {
        *self.ivars().on_external_write_activity.borrow_mut() = callback.map(|callback| Rc::new(callback) as Rc<_>);
    }

    pub fn set_on_file_renamed(&self, callback: Option<impl Fn(&FileUrl) + 'static>) {
        *self.ivars().on_file_renamed.borrow_mut() = callback.map(|callback| Rc::new(callback) as Rc<_>);
    }

    fn fire_on_reparse(&self, document: &Arc<ParsedDocument>, dirty: &DirtySet) {
        let callback = self.ivars().on_reparse.borrow().clone();
        if let Some(callback) = callback {
            callback(document, dirty);
        }
    }

    fn fire_on_external_event(&self, event: ExternalEvent) {
        let callback = self.ivars().on_external_event.borrow().clone();
        if let Some(callback) = callback {
            callback(&event);
        }
    }

    fn fire_on_external_write_activity(&self, active: bool) {
        let callback = self.ivars().on_external_write_activity.borrow().clone();
        if let Some(callback) = callback {
            callback(active);
        }
    }

    fn fire_on_save_failure(&self, error: &DocumentError) {
        let callback = self.ivars().on_save_failure.borrow().clone();
        if let Some(callback) = callback {
            callback(error);
        }
    }

    fn current_top_offset(&self) -> Option<isize> {
        let provider = self.ivars().current_top_offset_provider.borrow().clone();
        provider.map(|provider| provider())
    }

    // MARK: - Opening and saving

    pub fn open(&self, file_url: &FileUrl) -> Result<(), DocumentError> {
        // Canonicalise once, up front.  A `.atomic` write renames a new file
        // over the destination, which would *replace a symlink with a regular
        // file* while the link's target kept its stale content.  Resolving here
        // makes the identity, the watcher, and the write path agree, so saving
        // a file opened through a symlink edits the target it points at rather
        // than clobbering the link itself (§8.1).
        let canonical = file_url.resolving_symlinks_in_path();
        // Read before touching any state: a file that vanishes between the
        // link-follow and the read must leave the document exactly as the
        // caller's `close()` left it, not half-torn-down.
        let (text, fidelity, data) =
            DocumentIO::read_snapshot(std::path::Path::new(&canonical.path())).map_err(io_error)?;

        let ivars = self.ivars();
        ivars.is_closed.set(false);
        ivars.parse_coordinator.resume();
        self.cancel_parse_work();
        self.cancel_pending_external_write();
        // In-place hops reuse this document; change marks from the previous
        // file must not decorate the next one.  `reset`, not `clear`: dropping
        // a document is not the user reviewing it.
        ivars.changes.reset();
        *ivars.url.borrow_mut() = Some(canonical.clone());
        ivars.fidelity.set(fidelity);
        *ivars.state.borrow_mut() = ivars.document_state_store.state(&canonical);

        ivars.suppress_reparse.set(true);
        self.storage().beginEditing();
        self.storage().replaceCharactersInRange_withString(
            FoundationRange::new(0, self.storage().length()),
            &ns(&text),
        );
        self.storage().endEditing();
        ivars.suppress_reparse.set(false);

        *ivars.disk_hash.borrow_mut() = DocumentIO::content_hash(&text);
        *ivars.disk_byte_hash.borrow_mut() = DocumentIO::content_hash_data(&data);
        *ivars.last_committed_text.borrow_mut() = text.clone();
        ivars.is_dirty.set(false);
        ivars.last_async_parse_revision.set(None);
        self.publish_presentation_state(PresentationState::NEUTRAL);

        // Structure-only first paint for outline/state. The controller paints
        // decorations after applying zoom/folds; full decoration converges on
        // the async parse lane.
        let structure = MarkdownParser::parse_with(&text, ParseOptions::STRUCTURE_ONLY);
        *ivars.parsed.borrow_mut() = structure.clone();

        ivars.snapshot_store.record(&text, &canonical, SnapshotKind::Baseline);
        ivars.document_state_store.note_opened(&canonical, &structure);

        self.restore_review_state(&text);

        self.start_watching(&canonical);
        ivars.force_next_dirty_wholesale.set(true);
        // First full paint must not depend on the serial typing lane's
        // resume/discard control messages for a document that may have just
        // been reused in place. The immutable revision gate makes this direct
        // parse safe, and subsequent edits return to the coalescing lane.
        self.start_priority_async_reparse();
        Ok(())
    }

    // MARK: - Review baseline (§8.1, §8.2)

    /// Rebuilds the unread-changes picture on open.
    ///
    /// Three independent questions, answered in order:
    ///
    /// 1. **Did the bytes move since the reader last reviewed?**  A hash
    ///    comparison, made without touching the object store, so a pruned
    ///    object can never be mistaken for an unchanged file.
    /// 2. **Can the previous text still be shown?**  If not, say so
    ///    (`previous_version_unavailable`) instead of showing nothing at all.
    /// 3. **What did the reader already work through?**  Persisted marks carry
    ///    the visited flags back, so reopening a document does not silently
    ///    count twelve unreviewed changes as read.
    fn restore_review_state(&self, current_text: &str) {
        let ivars = self.ivars();
        let stored_baseline = {
            let state = ivars.state.borrow();
            if state.review_baseline_hash.is_empty() {
                state.last_seen_hash.clone()
            } else {
                state.review_baseline_hash.clone()
            }
        };

        let disk_hash = ivars.disk_hash.borrow().clone();
        if stored_baseline.is_empty() || same_text(&stored_baseline, &disk_hash) {
            // Nothing outstanding: the file is exactly as the reader left it.
            self.adopt_baseline(current_text, &disk_hash);
            ivars.changes.reset();
            ivars.unread_changes.set(UnreadChanges::None);
            return;
        }

        let stored = ivars.snapshot_store.content_for_hash(&stored_baseline);
        let snapshot_store::Content::Text(previous) = stored else {
            // The file moved and the old text is gone.  Keep the baseline hash
            // so a later write is still measured from the right place, and let
            // the owner say what happened.
            *ivars.review_baseline_text.borrow_mut() = String::new();
            *ivars.review_baseline_hash.borrow_mut() = stored_baseline;
            ivars.changes.reset();
            let reason = if stored == snapshot_store::Content::Corrupt { Unavailable::Corrupt } else { Unavailable::Pruned };
            ivars.unread_changes.set(UnreadChanges::PreviousVersionUnavailable { reason });
            return;
        };

        self.adopt_baseline(&previous, &stored_baseline);
        let hunks = TextDiff::hunks(&previous, current_text);
        if hunks.is_empty() {
            ivars.changes.reset();
            ivars.unread_changes.set(UnreadChanges::None);
            return;
        }
        ivars.changes.apply(&hunks, current_text, &previous, true);
        // Re-anchor the persisted set over the freshly computed one so review
        // progress survives the close/reopen: same kind, same range, same mark.
        let marks = ivars.state.borrow().marks.clone();
        ivars.changes.merge(&marks);
        ivars.unread_changes.set(UnreadChanges::Marked { count: ivars.changes.count() });
    }

    fn adopt_baseline(&self, text: &str, hash: &str) {
        *self.ivars().review_baseline_text.borrow_mut() = text.to_owned();
        *self.ivars().review_baseline_hash.borrow_mut() = hash.to_owned();
    }

    /// Moves the review baseline forward.  **Only user actions call this**:
    /// finishing a review, keeping their own version in a conflict, restoring
    /// a historical version, or opening a document that has nothing
    /// outstanding.  An incoming write must never advance it — that is the bug
    /// this whole mechanism exists to prevent.
    pub fn advance_review_baseline(&self, text: &str) {
        self.adopt_baseline(text, &SnapshotStore::hash(text));
        let ivars = self.ivars();
        ivars.unread_changes.set(UnreadChanges::None);
        let mut state = ivars.state.borrow_mut();
        state.review_baseline_hash = ivars.review_baseline_hash.borrow().clone();
        state.marks = Vec::new();
    }

    /// The explicit "I have read these" action.  Equivalent to
    /// `changes.clear()` — which routes here through `on_reviewed` — but named
    /// so a call site reads as intent rather than as cleanup.
    pub fn mark_changes_reviewed(&self) {
        self.ivars().changes.clear();
        if !self.is_dirty() && self.ivars().pending_conflict.borrow().is_none() {
            self.publish_presentation_state(PresentationState::NEUTRAL);
        }
    }

    /// Adopts text with no backing file — used by `Compare` windows and by the
    /// version timeline's preview pane.
    pub fn adopt(&self, text: &str, display_url: Option<&FileUrl>) {
        let ivars = self.ivars();
        ivars.is_closed.set(false);
        ivars.parse_coordinator.resume();
        self.cancel_parse_work();
        *ivars.url.borrow_mut() = display_url.cloned();
        ivars.suppress_reparse.set(true);
        self.storage().beginEditing();
        self.storage().replaceCharactersInRange_withString(FoundationRange::new(0, self.storage().length()), &ns(text));
        self.storage().endEditing();
        ivars.suppress_reparse.set(false);
        self.reparse_synchronously(true, true);
        ivars.is_dirty.set(false);
        self.publish_presentation_state(PresentationState::NEUTRAL);
    }

    pub fn save(&self, intent: SaveIntent) -> Result<(), DocumentError> {
        let ivars = self.ivars();
        // A closed document has no owner left to consent to a write.
        // Stragglers (queued autosave work, occlusion events during teardown)
        // must neither touch the file nor raise a failure alert for a window
        // that is gone.
        if ivars.is_closed.get() {
            return Ok(());
        }
        let Some(url) = self.url() else {
            let error = DocumentError::FileNoSuchFile;
            self.publish_save_failure(&error);
            return Err(error);
        };
        let path = std::path::PathBuf::from(url.path());
        let text = self.text();

        // §8.1: a save that would overwrite a newer-on-disk version must not
        // happen silently.  Every implicit save path (occlusion autosave, quit,
        // checkbox toggle, close alert) funnels through here, and none of them
        // may make the keep-mine call for the user.  Surface any unresolved
        // conflict — or one detected right now — and refuse to write.
        let mut expected_disk_data: Option<Vec<u8>> = None;
        let mut requires_missing_path = false;
        if intent != SaveIntent::RecreateFile {
            if intent == SaveIntent::Normal {
                let pending = ivars.pending_conflict.borrow().clone();
                if let Some(conflict) = pending {
                    let incoming = conflict.incoming_text.clone();
                    return Err(self.present_blocking_conflict(conflict, &incoming, None).into());
                }
            }
            match self.inspect_disk_state() {
                DiskState::Unchanged { data, fidelity } => {
                    expected_disk_data = Some(data);
                    ivars.fidelity.set(fidelity);
                }
                DiskState::Changed { text: incoming, hunks, data, fidelity } => {
                    if intent == SaveIntent::Normal {
                        let conflict = Conflict {
                            incoming_text: incoming.clone(),
                            changed_block_count: hunks.len() as isize,
                            hunks,
                            incoming_fidelity: Some(fidelity),
                            incoming_byte_hash: Some(DocumentIO::content_hash_data(&data)),
                        };
                        let incoming_hash = SnapshotStore::hash(&incoming);
                        return Err(self.present_blocking_conflict(conflict, &incoming, Some(incoming_hash)).into());
                    }
                    // Keep Mine is explicit about content, not byte formatting.
                    // Preserve the latest readable encoding/BOM/line-ending facts.
                    expected_disk_data = Some(data);
                    ivars.fidelity.set(fidelity);
                }
                DiskState::Missing => {
                    let error = DocumentError::Save(SaveError::FileMissing(url));
                    self.publish_save_failure(&error);
                    return Err(error);
                }
                DiskState::Unreadable(underlying) => {
                    let error = DocumentError::Save(SaveError::FileUnreadable(url, underlying));
                    self.publish_save_failure(&error);
                    return Err(error);
                }
            }
        } else {
            // The recovery choice authorizes creation only while the path is
            // still missing/unreadable. If a readable generation has appeared
            // since the sheet was shown, it is external state and wins.
            match self.inspect_disk_state() {
                DiskState::Missing => requires_missing_path = true,
                DiskState::Unreadable(_) => {}
                DiskState::Unchanged { data, fidelity } => {
                    expected_disk_data = Some(data);
                    ivars.fidelity.set(fidelity);
                }
                DiskState::Changed { text: incoming, hunks, data, fidelity } => {
                    let conflict = Conflict {
                        incoming_text: incoming.clone(),
                        changed_block_count: hunks.len() as isize,
                        hunks,
                        incoming_fidelity: Some(fidelity),
                        incoming_byte_hash: Some(DocumentIO::content_hash_data(&data)),
                    };
                    let incoming_hash = SnapshotStore::hash(&incoming);
                    return Err(self.present_blocking_conflict(conflict, &incoming, Some(incoming_hash)).into());
                }
            }
        }

        let provenance = self.presentation_state().provenance;
        self.publish_presentation_state(PresentationState::new(Phase::Saving, provenance, None));
        let encoded = match DocumentIO::encoded_data(&text, ivars.fidelity.get()) {
            Ok(encoded) => encoded,
            Err(error) => {
                let error = io_error(error);
                self.publish_save_failure(&error);
                return Err(error);
            }
        };
        let before_commit = ivars.before_save_commit_for_testing.borrow().clone();
        if let Some(before_commit) = before_commit {
            before_commit();
        }
        if let Some(watcher) = ivars.watcher.borrow().as_ref() {
            watcher.suppress_own_write(file_watcher::DEFAULT_SUPPRESSION_INTERVAL);
        }
        let written: Result<(), DynError> = if let Some(expected) = &expected_disk_data {
            DocumentIO::replace_existing_atomically(&encoded, &path, expected)
        } else if requires_missing_path {
            DocumentIO::create_atomically(&encoded, &path)
        } else {
            // Only the explicit Recreate File recovery action may create or
            // replace without an inspected existing generation.
            write_data_atomically(&encoded, &path).map_err(Into::into)
        };
        if let Err(error) = written {
            if let Some(watcher) = ivars.watcher.borrow().as_ref() {
                watcher.cancel_own_write_suppression();
            }
            if let Some(DocumentIOError::TargetChanged { displaced, .. }) = error.downcast_ref::<DocumentIOError>() {
                for data in displaced {
                    if let Ok((decoded, _)) = DocumentIO::decode_snapshot(data, &path) {
                        ivars.snapshot_store.record(&decoded, &url, SnapshotKind::External);
                    }
                }
            }
            match self.inspect_disk_state() {
                DiskState::Changed { text: incoming, hunks, data, fidelity } => {
                    let conflict = Conflict {
                        incoming_text: incoming.clone(),
                        changed_block_count: hunks.len() as isize,
                        hunks,
                        incoming_fidelity: Some(fidelity),
                        incoming_byte_hash: Some(DocumentIO::content_hash_data(&data)),
                    };
                    let incoming_hash = SnapshotStore::hash(&incoming);
                    return Err(self.present_blocking_conflict(conflict, &incoming, Some(incoming_hash)).into());
                }
                DiskState::Missing => {
                    let missing = DocumentError::Save(SaveError::FileMissing(url));
                    self.publish_save_failure(&missing);
                    return Err(missing);
                }
                DiskState::Unreadable(underlying) => {
                    let unreadable = DocumentError::Save(SaveError::FileUnreadable(url, underlying));
                    self.publish_save_failure(&unreadable);
                    return Err(unreadable);
                }
                DiskState::Unchanged { .. } => {}
            }
            let error = io_error(error);
            self.publish_save_failure(&error);
            return Err(error);
        }
        if let Some(watcher) = ivars.watcher.borrow().as_ref() {
            watcher.acknowledge_own_write(Some(&encoded));
        }

        *ivars.disk_hash.borrow_mut() = DocumentIO::content_hash(&text);
        *ivars.disk_byte_hash.borrow_mut() = DocumentIO::content_hash_data(&encoded);
        *ivars.last_committed_text.borrow_mut() = text.clone();
        *ivars.pending_conflict.borrow_mut() = None;
        ivars.snapshot_store.record(&text, &url, SnapshotKind::Local);
        self.set_dirty(false);
        self.persist_state();
        self.publish_saved_state();
        Ok(())
    }

    /// Re-reads the path at the write boundary. Missing and unreadable are
    /// distinct fail-closed states, never collapsed into "no change".
    pub fn inspect_disk_state(&self) -> DiskState {
        let Some(url) = self.url() else { return DiskState::Missing };
        let ivars = self.ivars();
        let (incoming, fidelity, data) = match DocumentIO::read_snapshot(std::path::Path::new(&url.path())) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                if !file_manager::file_exists(&url.path()) {
                    return DiskState::Missing;
                }
                return DiskState::Unreadable(Arc::new(error));
            }
        };
        let incoming_hash = SnapshotStore::hash(&incoming);
        let incoming_byte_hash = DocumentIO::content_hash_data(&data);
        if same_text(&incoming_byte_hash, &ivars.disk_byte_hash.borrow()) {
            return DiskState::Unchanged { data, fidelity };
        }
        // The decoded generation is still the one we opened; only its byte
        // representation changed externally. Adopt those facts even when the
        // buffer has local edits, so the edit is saved using the latest BOM,
        // encoding, and line endings rather than silently reverting them.
        if same_text(&incoming_hash, &ivars.disk_hash.borrow()) {
            *ivars.disk_byte_hash.borrow_mut() = incoming_byte_hash;
            ivars.fidelity.set(fidelity);
            return DiskState::Unchanged { data, fidelity };
        }
        // If the disk already holds the buffer's bytes, writing is a no-op and
        // there is nothing being clobbered.  A trailing-newline-only difference
        // is likewise not a content change: it is an artifact `DocumentIO` will
        // reconcile on this very write, so it must not surface as a conflict or
        // block an autosave.
        let buffer = self.text();
        if same_text(&incoming_hash, &SnapshotStore::hash(&buffer)) {
            *ivars.disk_hash.borrow_mut() = incoming_hash;
            *ivars.disk_byte_hash.borrow_mut() = incoming_byte_hash;
            ivars.fidelity.set(fidelity);
            return DiskState::Unchanged { data, fidelity };
        }
        DiskState::Changed { hunks: TextDiff::hunks(&buffer, &incoming), text: incoming, data, fidelity }
    }

    /// Records the external snapshot, marks the conflict pending, publishes
    /// the conflict event, and returns the blocking error that cancels the
    /// save.  The event (conflict bar) is the only surface — no
    /// `on_save_failure` alert, because a refused save here is a deliberate,
    /// visible outcome.
    fn present_blocking_conflict(&self, conflict: Conflict, incoming: &str, incoming_hash: Option<String>) -> SaveError {
        let ivars = self.ivars();
        *ivars.pending_conflict.borrow_mut() = Some(conflict.clone());
        if let Some(incoming_hash) = incoming_hash {
            *ivars.disk_hash.borrow_mut() = incoming_hash;
        }
        if let Some(url) = self.url() {
            ivars.snapshot_store.record(incoming, &url, SnapshotKind::External);
        }
        let provenance = self.presentation_state().provenance;
        self.publish_presentation_state(PresentationState::new(
            Phase::Conflict,
            provenance,
            Some("Changed on disk".into()),
        ));
        self.fire_on_external_event(ExternalEvent::Conflict(conflict));
        SaveError::BlockedByExternalConflict
    }

    pub fn save_if_needed(&self, intent: SaveIntent) -> Result<(), DocumentError> {
        // Same lifetime rule as save(): after close() the document is inert.
        // Reporting success here keeps straggler implicit saves silent instead
        // of surfacing errors for a window that no longer exists.
        if self.ivars().is_closed.get() {
            return Ok(());
        }
        if !self.is_dirty() {
            return Ok(());
        }
        if self.url().is_none() {
            let error = DocumentError::FileNoSuchFile;
            self.fire_on_save_failure(&error);
            return Err(error);
        }
        self.save(intent)
    }

    /// Explicit recovery action. No implicit path may call this.
    pub fn recreate_missing_file(&self) -> Result<(), DocumentError> {
        self.save(SaveIntent::RecreateFile)
    }

    /// Explicitly abandons unsaved local changes after a failed save. The
    /// in-memory text remains visible until the window closes, but it is no
    /// longer eligible for autosave and the missing path is never resurrected.
    pub fn discard_unsaved_changes(&self) {
        let ivars = self.ivars();
        let snapshot = self.url().and_then(|url| DocumentIO::read_snapshot(std::path::Path::new(&url.path())).ok());
        let replacement = if let Some((text, fidelity, data)) = snapshot {
            ivars.fidelity.set(fidelity);
            *ivars.disk_hash.borrow_mut() = DocumentIO::content_hash(&text);
            *ivars.disk_byte_hash.borrow_mut() = DocumentIO::content_hash_data(&data);
            *ivars.last_committed_text.borrow_mut() = text.clone();
            text
        } else {
            ivars.last_committed_text.borrow().clone()
        };
        ivars.suppress_reparse.set(true);
        self.storage().beginEditing();
        self.storage()
            .replaceCharactersInRange_withString(FoundationRange::new(0, self.storage().length()), &ns(&replacement));
        self.storage().endEditing();
        ivars.suppress_reparse.set(false);
        self.undo_manager().removeAllActions();
        self.reparse_synchronously(true, true);
        *ivars.pending_conflict.borrow_mut() = None;
        self.set_dirty(false);
        self.publish_presentation_state(PresentationState::NEUTRAL);
    }

    pub fn close(&self) {
        let ivars = self.ivars();
        ivars.is_closed.set(true);
        if let Some(item) = ivars.saved_state_work_item.borrow().as_ref() {
            item.cancel();
        }
        *ivars.pending_external_restore.borrow_mut() = None;
        self.cancel_parse_work();
        self.cancel_pending_external_write();
        // Persist *before* discarding: closing a window is not a review, and
        // the twelve marks the reader had not looked at yet must come back.
        self.persist_state();
        // Discard change marks owned by a torn-down document so they cannot
        // leak across to the next file opened in the same window.
        ivars.changes.reset();
        // Undo registrations target `self`, so every entry retains the whole
        // document (storage, parsed tree, watcher reference). A closed
        // document can never serve another undo, and without this the stack
        // keeps it — and everything it holds — un-deallocable.
        self.undo_manager().removeAllActions();
        ivars.parse_coordinator.suspend();
        if let Some(watcher) = ivars.watcher.borrow_mut().take() {
            watcher.stop();
        }
    }

    fn persist_state(&self) {
        let Some(url) = self.url() else { return };
        let ivars = self.ivars();
        let mut state = ivars.state.borrow().clone();
        // `last_seen_hash` is disk bookkeeping and moves with every absorbed
        // write.  `review_baseline_hash` is the reader's place in the review
        // and moves only when they say so — the two must never be conflated.
        state.last_seen_hash = ivars.disk_hash.borrow().clone();
        state.review_baseline_hash = ivars.review_baseline_hash.borrow().clone();
        state.marks = ivars.changes.persisted_marks();
        if let Some(top) = self.current_top_offset() {
            state.anchor = ScrollAnchoring::anchor(top, &self.parsed());
        }
        state.last_opened = Date::now();
        *ivars.state.borrow_mut() = state.clone();
        ivars.document_state_store.save(&state, &url);
    }

    /// Restores the reader's place from persisted state (§8.2).
    pub fn restored_offset(&self) -> isize {
        ScrollAnchoring::offset(&self.ivars().state.borrow().anchor, &self.parsed())
    }

    /// Keep reference-valued fragment metadata aligned with the storage while
    /// the parser works on its immutable snapshot.  Mirrors the projection in
    /// MarkdownTextView's editing funnel so document-level mutations
    /// (commands, undo/redo, external absorption) cannot leave payloads
    /// pointing at pre-edit offsets.
    fn project_fragment_payloads(&self, range: NSRange, inserted_length: isize) {
        let storage = self.storage();
        let length = storage.length() as isize;
        let start = 0.max(range.location.min(length));
        let seen: RefCell<HashSet<*const AnyObject>> = RefCell::new(HashSet::new());
        let block = RcBlock::new(|value: *mut AnyObject, _range: FoundationRange, _stop: NonNull<Bool>| {
            // SAFETY: the attribute value, valid for the enumeration.
            let Some(value) = (unsafe { value.as_ref() }) else { return };
            let Some(payload) = value.downcast_ref::<FragmentPayload>() else { return };
            if !seen.borrow_mut().insert(value as *const AnyObject) {
                return;
            }
            payload.project_source_ranges(range, inserted_length);
        });
        storage.enumerateAttribute_inRange_options_usingBlock(
            attribute_keys::dr_fragment(),
            FoundationRange::new(start as usize, (length - start) as usize),
            NSAttributedStringEnumerationOptions::empty(),
            &block,
        );
    }

    // MARK: - Editing
    //
    // Every mutation funnels through here so undo, dirty tracking, and change
    // mark adjustment are impossible to forget at a call site.

    /// `replace(_:with:actionName:)`.
    pub fn replace(&self, range: NSRange, replacement: &str, action_name: Option<&str>) -> bool {
        let ivars = self.ivars();
        if !(range.location >= 0 && range.upper_bound() <= self.storage_length()) {
            return false;
        }
        let previous = ns_to_string(&self.storage().string().substringWithRange(foundation_range(range)));
        if same_text(&previous, replacement) {
            return false;
        }

        if !ivars.is_applying_batch.get() && !self.undo_manager().is_applying_undo_redo() {
            let callback = ivars.on_will_apply_edits.borrow().clone();
            if let Some(callback) = callback {
                callback(&[TextEdit::new(range, replacement, action_name.unwrap_or("Edit"), None)]);
            }
        }

        let replacement_length = utf16_length(replacement);
        let new_range = NSRange::new(range.location, replacement_length);
        self.undo_manager().beginUndoGrouping();
        let action = action_name.map(str::to_owned);
        self.register_undo(move |document| {
            document.replace(new_range, &previous, action.as_deref());
        });
        if let Some(action_name) = action_name {
            self.undo_manager().setActionName(&NSString::from_str(action_name));
        }
        self.undo_manager().endUndoGrouping();

        // Attribute runs move with the storage edit, but the fragment payload
        // objects riding on them are reference values whose ranges do not.
        // Project them across this edit exactly as MarkdownTextView does for
        // its own editing funnel; otherwise every unchanged block below an
        // insertion keeps pointing at pre-edit offsets until some later edit
        // happens to redecorate it.
        self.project_fragment_payloads(range, replacement_length);
        self.storage().beginEditing();
        self.storage().replaceCharactersInRange_withString(foundation_range(range), &ns(replacement));
        self.storage().endEditing();

        let delta = replacement_length - range.length;
        ivars.changes.adjust(range, delta);
        if !ivars.is_applying_external_change.get() {
            let provenance =
                if self.undo_manager().is_applying_undo_redo() { "Undo" } else { action_name.unwrap_or("Edit") };
            self.note_mutation(provenance);
            self.set_dirty(true);
        }
        true
    }

    /// `undoManager.registerUndo(withTarget: self) { doc in … }`.
    fn register_undo(&self, handler: impl Fn(&MarkdownDocument) + 'static) {
        let block = RcBlock::new(move |target: NonNull<AnyObject>| {
            // SAFETY: the target registered below is this document.
            let document = unsafe { target.cast::<MarkdownDocument>().as_ref() };
            handler(document);
        });
        // SAFETY: the handler runs on the main thread (undo is main-only) with
        // the target it was registered with.
        unsafe { self.undo_manager().registerUndoWithTarget_handler(self.as_ref(), &block) };
    }

    /// Applies structural-command edits as one explicit transaction.
    ///
    /// When `tidy_rules` is given, the transaction follows the documented
    /// sequence — apply edit, reparse, plan tidy rules, apply tidy edit — all
    /// inside the *same* undo group, so ⌘Z undoes the command and its
    /// automatic repair together (§6.4: indenting renumbers ordered lists).
    pub fn apply(&self, edits: &[TextEdit], action_name: &str, tidy_rules: Option<&[TidyRule]>) {
        if edits.is_empty() {
            return;
        }
        let ivars = self.ivars();
        // Back to front so earlier offsets stay valid, matching
        // `[TextEdit].applied(to:)`.
        let mut ordered = edits.to_vec();
        ordered.sort_by(|a, b| b.range.location.cmp(&a.range.location));
        let mut accepted: Vec<TextEdit> = Vec::new();
        let mut last_start = isize::MAX;
        for edit in ordered {
            if edit.range.upper_bound() > last_start {
                continue;
            }
            last_start = edit.range.location;
            accepted.push(edit);
        }
        if accepted.is_empty() {
            return;
        }

        self.note_mutation(action_name);
        let callback = ivars.on_will_apply_edits.borrow().clone();
        if let Some(callback) = callback {
            callback(&accepted);
        }
        ivars.is_applying_batch.set(true);
        struct EndBatch<'a>(&'a MarkdownDocument);
        impl Drop for EndBatch<'_> {
            fn drop(&mut self) {
                self.0.ivars().is_applying_batch.set(false);
            }
        }
        let _end_batch = EndBatch(self);
        self.undo_manager().beginUndoGrouping();
        for edit in &accepted {
            self.replace(edit.range, &edit.replacement, None);
        }
        if let Some(tidy_rules) = tidy_rules
            && !tidy_rules.is_empty()
        {
            // Reparse silently mid-batch: the plan must see the tree as the
            // landed edits shape it, and the trailing `reparse_now()` below
            // remains the single notification point.
            self.reparse_synchronously(false, false);
            let mut tidy_last_start = isize::MAX;
            let mut plan = TidyDocument::plan_with(&self.parsed(), tidy_rules);
            plan.sort_by(|a, b| b.range.location.cmp(&a.range.location));
            for edit in plan {
                if !(edit.range.upper_bound() <= tidy_last_start && edit.range.upper_bound() <= self.storage_length()) {
                    continue;
                }
                tidy_last_start = edit.range.location;
                self.replace(edit.range, &edit.replacement, None);
            }
        }
        self.undo_manager().setActionName(&NSString::from_str(action_name));
        self.undo_manager().endUndoGrouping();
        // Structural commands are explicit transactions. Converge their tree
        // before returning so the renderer never spends an event-loop turn in
        // raw Markdown after the command has already completed.
        self.reparse_now(false);
    }

    /// Toggling a checkbox writes the file immediately (§7.1, §8.5).
    pub fn toggle_task(&self, offset: isize) {
        self.ensure_parsed_current();
        let Some(edit) = Restructure::toggle_task(&self.parsed(), offset) else { return };
        self.apply(&[edit], "Toggle Task", None);
        if self.url().is_some() {
            let _ = self.save_if_needed(SaveIntent::Normal);
        }
    }

    fn set_dirty(&self, value: bool) {
        let ivars = self.ivars();
        if ivars.is_dirty.get() == value {
            return;
        }
        ivars.is_dirty.set(value);
        let callback = ivars.on_dirty_changed.borrow().clone();
        if let Some(callback) = callback {
            callback(value);
        }
        let current = self.presentation_state();
        if value && current.phase != Phase::Conflict {
            self.publish_presentation_state(PresentationState::new(
                Phase::Edited,
                Some(current.provenance.unwrap_or_else(|| "Edit".into())),
                None,
            ));
        }
    }

    /// Records the human-scale action that most recently changed source.
    /// This is deliberately callable from view/controller seams (Paste,
    /// Replace, panel actions) while parser and layout callbacks have no
    /// access to it.
    pub fn note_mutation(&self, provenance: &str) {
        let value = upleft_swift_text::trim_whitespaces_and_newlines(provenance);
        if value.is_empty() {
            return;
        }
        let current = self.presentation_state();
        let phase = if self.is_dirty() { Phase::Edited } else { current.phase };
        self.publish_presentation_state(PresentationState::new(phase, Some(value.to_owned()), current.detail));
    }

    fn publish_presentation_state(&self, state: PresentationState) {
        let ivars = self.ivars();
        if let Some(item) = ivars.saved_state_work_item.borrow_mut().take() {
            item.cancel();
        }
        if state == *ivars.presentation_state.borrow() {
            return;
        }
        *ivars.presentation_state.borrow_mut() = state.clone();
        let callback = ivars.on_presentation_state_changed.borrow().clone();
        if let Some(callback) = callback {
            callback(&state);
        }
    }

    fn publish_save_failure(&self, error: &DocumentError) {
        let provenance = self.presentation_state().provenance;
        self.publish_presentation_state(PresentationState::new(
            Phase::SaveFailed,
            provenance,
            Some(error.localized_description()),
        ));
        self.fire_on_save_failure(error);
    }

    fn publish_saved_state(&self) {
        let provenance = self.presentation_state().provenance;
        self.publish_presentation_state(PresentationState::new(Phase::Saved, provenance, None));
        let id = self.ivars().id;
        let item = WorkItem::new(move || {
            // The work item runs on the main queue.
            if let Some(document) = document_for(id)
                && !document.is_dirty()
                && document.ivars().pending_conflict.borrow().is_none()
            {
                document.publish_presentation_state(PresentationState::NEUTRAL);
            }
        });
        item.schedule_after(DispatchQueue::main(), 1.25);
        *self.ivars().saved_state_work_item.borrow_mut() = Some(item);
    }

    // MARK: - Parsing (§3.5)

    /// Coalesced to the end of the runloop turn: a burst of keystrokes or a
    /// multi-edit command reparses once, not once per character.
    fn schedule_reparse(&self) {
        let ivars = self.ivars();
        if ivars.reparse_scheduled.get() || ivars.suppress_reparse.get() {
            return;
        }
        ivars.reparse_scheduled.set(true);
        let id = ivars.id;
        let block = RcBlock::new(move || {
            if let Some(document) = document_for(id) {
                document.flush_scheduled_reparse();
            }
        });
        // SAFETY: `NSRunLoopCommonModes` is a constant; the block runs on the
        // main run loop.
        unsafe {
            let modes = NSArray::from_slice(&[NSRunLoopCommonModes]);
            NSRunLoop::mainRunLoop().performInModes_block(&modes, &block);
        }
    }

    /// Starts the coalesced worker snapshot.  Kept as a small seam so tests
    /// can flush scheduled work without depending on wall-clock run-loop time.
    pub fn flush_scheduled_reparse(&self) {
        if !self.ivars().reparse_scheduled.get() {
            return;
        }
        self.ivars().reparse_scheduled.set(false);
        self.start_async_reparse();
    }

    pub fn reparse_now(&self, wholesale: bool) {
        self.ivars().reparse_scheduled.set(false);
        self.reparse_synchronously(true, wholesale);
    }

    fn finish_undo_redo(&self) {
        if same_text(&self.parsed().text, &self.text()) {
            return;
        }
        // A grouped undo/redo can invoke several inverse closures. Keep the
        // renderer suspended for the whole transaction, then publish exactly
        // one parsed/display-map repair after AppKit has finished mutating the
        // shared storage.
        self.reparse_now(false);
    }

    /// Forces convergence before an operation that reads the tree.  Commands
    /// that rewrite source must never plan edits from an older async parse.
    pub fn ensure_parsed_current(&self) {
        if same_text(&self.parsed().text, &self.text()) {
            return;
        }
        self.reparse_now(false);
    }

    fn reparse_synchronously(&self, notifying: bool, wholesale: bool) {
        self.cancel_parse_work();
        let previous = self.parsed();
        let fresh = MarkdownParser::parse(&self.text());
        *self.ivars().parsed.borrow_mut() = fresh.clone();
        if !notifying {
            return;
        }
        let dirty = if wholesale { DirtySet::wholesale() } else { ASTDiff::dirty_set(Some(&previous), &fresh) };
        self.fire_on_reparse(&fresh, &dirty);
    }

    fn start_async_reparse(&self) {
        let ivars = self.ivars();
        if ivars.suppress_reparse.get() || ivars.is_closed.get() {
            return;
        }
        let text = self.text();
        let previous = self.parsed();
        let parse_revision = ivars.revision.get();
        ivars.parse_coordinator.submit(MarkdownParseRequest { text, previous, revision: parse_revision });

        if ivars.parse_task.borrow().is_some() {
            return;
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        *ivars.parse_task.borrow_mut() = Some(cancelled.clone());
        parse_loop(ivars.parse_coordinator.clone(), ivars.id, cancelled);
    }

    /// External replacement must not wait behind an obsolete, non-cancellable
    /// initial parse. Run this one snapshot concurrently and rely on the same
    /// immutable revision/text checks used by the serial typing lane.
    fn start_priority_async_reparse(&self) {
        let ivars = self.ivars();
        if ivars.suppress_reparse.get() || ivars.is_closed.get() {
            return;
        }
        let request = MarkdownParseRequest { text: self.text(), previous: self.parsed(), revision: ivars.revision.get() };
        let id = ivars.id;
        ivars.parse_coordinator.run_immediately(request, move |result| {
            on_main_with_document(id, move |document| document.apply_async_parse(result));
        });
    }

    fn apply_async_parse(&self, result: MarkdownParseResult) {
        let ivars = self.ivars();
        if ivars.is_closed.get()
            || result.revision != ivars.revision.get()
            || !same_text(&result.text, &self.text())
            || !same_text(&result.document.text, &result.text)
        {
            return;
        }
        *ivars.parsed.borrow_mut() = result.document.clone();
        ivars.last_async_parse_revision.set(Some(result.revision));
        let dirty = ivars.next_external_dirty_override.borrow_mut().take().unwrap_or_else(|| {
            if ivars.force_next_dirty_wholesale.get() { DirtySet::wholesale() } else { result.dirty.clone() }
        });
        ivars.force_next_dirty_wholesale.set(false);
        self.fire_on_reparse(&result.document, &dirty);
        let restore = ivars.pending_external_restore.borrow_mut().take();
        if let Some(restore) = restore {
            let restored = ScrollAnchoring::offset(&restore.anchor, &result.document);
            let handler = ivars.restore_offset_handler.borrow().clone();
            if let Some(handler) = handler {
                handler(restored);
            }
            self.restore_selection(&restore.previous_text, restore.selection, restored);
        }
    }

    fn cancel_parse_work(&self) {
        let ivars = self.ivars();
        ivars.reparse_scheduled.set(false);
        ivars.revision.set(ivars.revision.get().advanced());
        ivars.parse_coordinator.discard_pending();
    }

    fn invalidate_parse_work_for_edit(&self) {
        let ivars = self.ivars();
        ivars.revision.set(ivars.revision.get().advanced());
    }

    // MARK: - External changes (§8.1)

    fn start_watching(&self, file_url: &FileUrl) {
        let ivars = self.ivars();
        if !self.preferences().values().watch_files {
            if let Some(watcher) = ivars.watcher.borrow_mut().take() {
                watcher.stop();
            }
            return;
        }
        if let Some(watcher) = ivars.watcher.borrow().as_ref() {
            watcher.stop();
        }
        let id = ivars.id;
        let watcher = FileWatcher::new(file_url, false, None, move |event| {
            // FileWatcher delivers on the main queue.
            if let Some(document) = document_for(id) {
                document.handle_watch_event(event);
            }
        });
        // The previous watcher, already stopped, drops here.
        *ivars.watcher.borrow_mut() = Some(watcher);
    }

    fn preferences_did_change(&self) {
        let Some(url) = self.url() else { return };
        let ivars = self.ivars();
        if ivars.is_closed.get() || !self.preferences().values().watch_files {
            if let Some(watcher) = ivars.watcher.borrow_mut().take() {
                watcher.stop();
            }
            return;
        }
        self.start_watching(&url);
    }

    fn cancel_pending_external_write(&self) {
        if let Some(item) = self.ivars().pending_external_write.borrow_mut().take() {
            item.cancel();
        }
        self.end_burst_if_needed();
    }

    fn end_burst_if_needed(&self) {
        let ivars = self.ivars();
        if !ivars.is_absorbing_burst.get() {
            return;
        }
        ivars.is_absorbing_burst.set(false);
        self.fire_on_external_write_activity(false);
    }

    pub fn handle_watch_event(&self, event: file_watcher::Event) {
        // `FileWatcher` dispatches to the main queue; a block already in
        // flight when `close()` ran cannot be retracted, and `stop()` only
        // cancels work that has not started.  A late event on a closed
        // document would schedule a fresh absorb and fire external-change UI
        // on a window that is going away — the same guard
        // `start_async_reparse` and `preferences_did_change` already apply.
        if self.ivars().is_closed.get() {
            return;
        }
        match event {
            file_watcher::Event::Removed => {
                let provenance = self.presentation_state().provenance;
                self.publish_presentation_state(PresentationState::new(
                    Phase::ChangedOnDisk,
                    provenance,
                    Some("File missing".into()),
                ));
                self.fire_on_external_event(ExternalEvent::FileRemoved);
            }
            file_watcher::Event::Restored => {
                self.fire_on_external_event(ExternalEvent::FileRestored);
                self.handle_external_write();
            }
            file_watcher::Event::Changed => self.handle_external_write(),
            file_watcher::Event::Renamed(new_url) => self.adopt_renamed_file(&new_url),
        }
    }

    /// The file was renamed under us and the watcher re-attached.  Move the
    /// document's identity with it so reading position, history, and the
    /// window's own idea of what it is showing all keep pointing at one file.
    fn adopt_renamed_file(&self, new_url: &FileUrl) {
        let ivars = self.ivars();
        if self.url().is_none() {
            return;
        }
        let canonical = new_url.resolving_symlinks_in_path();
        if Some(&canonical) == self.url().as_ref() {
            return;
        }
        *ivars.url.borrow_mut() = Some(canonical.clone());
        ivars.state.borrow_mut().path = canonical.path();
        let state = ivars.state.borrow().clone();
        ivars.document_state_store.save(&state, &canonical);
        // Seed the new key's history with what we are holding, so the timeline
        // does not start empty at the new name.
        ivars.snapshot_store.record(&self.text(), &canonical, SnapshotKind::Baseline);
        let callback = ivars.on_file_renamed.borrow().clone();
        if let Some(callback) = callback {
            callback(&canonical);
        }
    }

    /// Trailing quiet-period debounce.
    ///
    /// `FileWatcher` already coalesces at 300 ms, but an agent that writes a
    /// file five times over three seconds clears that window between writes,
    /// so each one arrived as its own full-buffer replace, synchronous reparse
    /// and scroll restore.  Hold off until nothing has landed for 250 ms and
    /// absorb once.  Nothing is buffered: the flush re-reads the file, so it
    /// always applies the newest bytes rather than a stale copy.
    pub fn handle_external_write(&self) {
        let ivars = self.ivars();
        ivars.external_preparation_generation.set(ivars.external_preparation_generation.get().wrapping_add(1));
        if let Some(item) = ivars.pending_external_write.borrow().as_ref() {
            item.cancel();
        }
        if !ivars.is_absorbing_burst.get() {
            ivars.is_absorbing_burst.set(true);
            self.fire_on_external_write_activity(true);
        }
        let id = ivars.id;
        let item = WorkItem::new(move || {
            if let Some(document) = document_for(id) {
                document.absorb_external_write();
            }
        });
        item.schedule_after(DispatchQueue::main(), EXTERNAL_WRITE_QUIET_PERIOD);
        *ivars.pending_external_write.borrow_mut() = Some(item);
    }

    /// Applies any external write still waiting on the quiet period.  Tests
    /// and lifecycle code use this instead of guessing at wall-clock timing.
    pub fn flush_pending_external_write(&self) {
        let pending = self.ivars().pending_external_write.borrow_mut().take();
        let Some(item) = pending else { return };
        item.cancel();
        self.absorb_external_write();
    }

    fn absorb_external_write(&self) {
        let ivars = self.ivars();
        *ivars.pending_external_write.borrow_mut() = None;
        let Some(url) = self.url() else {
            self.end_burst_if_needed();
            return;
        };
        let generation = ivars.external_preparation_generation.get();
        let captured_text = self.text();
        let baseline = self.effective_baseline_text(&captured_text);
        let snapshot_store = ivars.snapshot_store.clone();
        let id = ivars.id;

        // File I/O, history reservation, and the two Myers diffs are pure or
        // internally synchronized. Keeping them off the main actor is what
        // lets a reader continue scrolling while a large agent rewrite lands.
        user_initiated().exec_async(move || {
            let Ok((incoming, fresh_fidelity, data)) = DocumentIO::read_snapshot(std::path::Path::new(&url.path()))
            else {
                on_main_with_document(id, move |document| document.finish_external_preparation(generation, None));
                return;
            };
            let incoming_hash = SnapshotStore::hash(&incoming);
            let incoming_byte_hash = DocumentIO::content_hash_data(&data);
            let current_hash = SnapshotStore::hash(&captured_text);
            let unchanged = same_text(&incoming_hash, &current_hash);
            if !unchanged {
                snapshot_store.record(&incoming, &url, SnapshotKind::External);
            }
            let baseline_hunks = if unchanged { Vec::new() } else { TextDiff::hunks(&baseline, &incoming) };
            let application_hunks = if unchanged { Vec::new() } else { TextDiff::hunks(&captured_text, &incoming) };
            let prepared = PreparedExternalWrite {
                generation,
                url,
                captured_text,
                baseline,
                incoming,
                fidelity: fresh_fidelity,
                incoming_hash,
                incoming_byte_hash,
                baseline_hunks,
                application_hunks,
            };
            on_main_with_document(id, move |document| {
                document.finish_external_preparation(generation, Some(prepared));
            });
        });
    }

    fn finish_external_preparation(&self, generation: u64, prepared: Option<PreparedExternalWrite>) {
        let ivars = self.ivars();
        if ivars.is_closed.get() {
            return;
        }
        if generation != ivars.external_preparation_generation.get() {
            return;
        }
        struct EndBurst<'a>(&'a MarkdownDocument);
        impl Drop for EndBurst<'_> {
            fn drop(&mut self) {
                self.0.end_burst_if_needed();
            }
        }
        let _end_burst = EndBurst(self);
        let Some(prepared) = prepared else { return };
        if self.url().as_ref() != Some(&prepared.url) {
            return;
        }

        // A local edit that landed while the background diff ran wins. The
        // captured clean snapshot is no longer safe to apply, so compute the
        // conflict against the current buffer on a fresh background turn.
        let current = self.text();
        if !same_text(&current, &prepared.captured_text) {
            ivars.external_preparation_generation.set(ivars.external_preparation_generation.get().wrapping_add(1));
            let next_generation = ivars.external_preparation_generation.get();
            ivars.is_absorbing_burst.set(true);
            let id = ivars.id;
            user_initiated().exec_async(move || {
                let hunks = TextDiff::hunks(&current, &prepared.incoming);
                on_main_with_document(id, move |document| {
                    document.finish_prepared_conflict(
                        next_generation,
                        prepared.incoming,
                        prepared.incoming_hash,
                        prepared.fidelity,
                        prepared.incoming_byte_hash,
                        hunks,
                    );
                });
            });
            return;
        }

        if same_text(&prepared.incoming_hash, &SnapshotStore::hash(&prepared.captured_text)) {
            *ivars.disk_hash.borrow_mut() = prepared.incoming_hash;
            *ivars.disk_byte_hash.borrow_mut() = prepared.incoming_byte_hash;
            ivars.fidelity.set(prepared.fidelity);
            if !self.is_dirty() && ivars.pending_conflict.borrow().is_none() {
                self.publish_presentation_state(PresentationState::NEUTRAL);
            }
            return;
        }
        if self.is_dirty() && final_newline_difference_only(&prepared.captured_text, &prepared.incoming) {
            *ivars.disk_hash.borrow_mut() = prepared.incoming_hash;
            *ivars.disk_byte_hash.borrow_mut() = prepared.incoming_byte_hash;
            ivars.fidelity.set(prepared.fidelity);
            return;
        }

        *ivars.disk_hash.borrow_mut() = prepared.incoming_hash.clone();
        *ivars.disk_byte_hash.borrow_mut() = prepared.incoming_byte_hash.clone();
        if self.is_dirty() {
            self.present_prepared_conflict(
                prepared.incoming,
                prepared.application_hunks,
                prepared.fidelity,
                prepared.incoming_byte_hash,
            );
            return;
        }

        *ivars.pending_conflict.borrow_mut() = None;
        ivars.fidelity.set(prepared.fidelity);
        *ivars.last_committed_text.borrow_mut() = prepared.incoming.clone();
        self.apply_external_text_with(
            &prepared.incoming,
            &prepared.baseline_hunks,
            Some(&prepared.baseline),
            Some(prepared.application_hunks.clone()),
        );
        let changes = &ivars.changes;
        ivars.unread_changes.set(if changes.is_empty() {
            UnreadChanges::None
        } else {
            UnreadChanges::Marked { count: changes.count() }
        });
        let hunks = prepared.baseline_hunks;
        let detail = if hunks.is_empty() {
            None
        } else {
            Some(format!("{} changed block{}", hunks.len(), if hunks.len() == 1 { "" } else { "s" }))
        };
        self.publish_presentation_state(PresentationState::new(Phase::ChangedOnDisk, None, detail));
        self.fire_on_external_event(ExternalEvent::Applied { hunks });
    }

    fn finish_prepared_conflict(
        &self,
        generation: u64,
        incoming: String,
        incoming_hash: String,
        incoming_fidelity: ByteFidelity,
        incoming_byte_hash: String,
        hunks: Vec<ChangeHunk>,
    ) {
        let ivars = self.ivars();
        if ivars.is_closed.get() || generation != ivars.external_preparation_generation.get() {
            return;
        }
        *ivars.disk_hash.borrow_mut() = incoming_hash;
        self.present_prepared_conflict(incoming, hunks, incoming_fidelity, incoming_byte_hash);
        self.end_burst_if_needed();
    }

    fn present_prepared_conflict(
        &self,
        incoming: String,
        hunks: Vec<ChangeHunk>,
        fidelity: ByteFidelity,
        byte_hash: String,
    ) {
        let conflict = Conflict {
            incoming_text: incoming,
            changed_block_count: hunks.len() as isize,
            hunks,
            incoming_fidelity: Some(fidelity),
            incoming_byte_hash: Some(byte_hash),
        };
        *self.ivars().pending_conflict.borrow_mut() = Some(conflict.clone());
        let provenance = self.presentation_state().provenance;
        self.publish_presentation_state(PresentationState::new(
            Phase::Conflict,
            provenance,
            Some("Changed on disk".into()),
        ));
        self.fire_on_external_event(ExternalEvent::Conflict(conflict));
    }

    /// The text to diff an incoming write against.  Falls back to the buffer
    /// when the baseline text is unavailable (pruned history), which degrades
    /// to the old behaviour rather than to no marks at all.
    fn effective_baseline_text(&self, fallback: &str) -> String {
        let baseline = self.ivars().review_baseline_text.borrow();
        if baseline.is_empty() { fallback.to_owned() } else { baseline.clone() }
    }

    /// `applyExternalText(_:hunks:)`.
    pub fn apply_external_text(&self, incoming: &str, hunks: &[ChangeHunk]) {
        self.apply_external_text_with(incoming, hunks, None, None);
    }

    /// Replaces the buffer in place, holding the reader's position by
    /// anchoring to the nearest unchanged heading rather than to a byte
    /// offset (§8.1), and putting the selection back afterwards.
    pub fn apply_external_text_with(
        &self,
        incoming: &str,
        hunks: &[ChangeHunk],
        baseline: Option<&str>,
        prepared_application_hunks: Option<Vec<ChangeHunk>>,
    ) {
        let ivars = self.ivars();
        // Adopting the on-disk version resolves any outstanding conflict.
        *ivars.pending_conflict.borrow_mut() = None;
        let top_offset = self.current_top_offset().unwrap_or(0);
        let anchor = ScrollAnchoring::anchor(top_offset, &self.parsed());
        let previous_text = self.text();
        let selection_provider = ivars.current_selection_provider.borrow().clone();
        let previous_selection = selection_provider.map_or(NSRange::new(0, 0), |provider| provider());

        // External replacement is a new immutable source generation just like
        // a local character edit. Without advancing here, an initial/open
        // parse still in flight can share the same revision and cause the
        // coordinator to reject this newer snapshot as a duplicate.
        self.invalidate_parse_work_for_edit();
        let application_hunks = prepared_application_hunks.unwrap_or_else(|| {
            if same_text(baseline.unwrap_or(&previous_text), &previous_text) {
                hunks.to_vec()
            } else {
                TextDiff::hunks(&previous_text, incoming)
            }
        });
        self.adopt_external_buffer(incoming, Some(&application_hunks));
        self.set_dirty(false);

        ivars.changes.apply(hunks, incoming, baseline.unwrap_or(&previous_text), true);
        *ivars.pending_external_restore.borrow_mut() =
            Some(PendingExternalRestore { previous_text, selection: previous_selection, anchor });
        let incoming_length = utf16_length(incoming);
        *ivars.next_external_dirty_override.borrow_mut() = Some(DirtySet::new(
            application_hunks.iter().map(|hunk| TextDiff::anchor_range(hunk, incoming_length)).collect(),
            false,
        ));
        self.start_priority_async_reparse();
    }

    /// Puts the buffer's whole contents behind an external write, with an
    /// undo that says what it is doing.
    ///
    /// ⌘Z reverting an agent's rewrite is the fastest possible answer to "no,
    /// put it back" and must keep working.  What it must *not* do is quietly
    /// leave a dirty buffer while `disk_hash` still holds the agent's content.
    /// So the undo restores the text *and* surfaces the disagreement it just
    /// created, through the same conflict bar that any other
    /// buffer-versus-disk disagreement uses.
    fn adopt_external_buffer(&self, incoming: &str, hunks: Option<&[ChangeHunk]>) {
        let ivars = self.ivars();
        let previous = self.text();
        let whole = FoundationRange::new(0, self.storage().length());

        self.undo_manager().beginUndoGrouping();
        self.register_undo(move |document| {
            // Undo groups unwind last-in-first-out, so after newer local edits
            // have unwound the buffer contains this generation's incoming
            // text. Reading it here avoids retaining a second whole-document
            // copy in every external-change undo group.
            let incoming = document.text();
            document.revert_external_buffer(&previous, &incoming);
        });
        self.undo_manager().setActionName(&NSString::from_str("External Change"));
        self.undo_manager().endUndoGrouping();

        ivars.is_applying_external_change.set(true);
        // Bounded: a token whose callback never arrives (a racing local edit
        // changed the post-edit text before delivery) must not retain a
        // whole-document copy forever, and the oldest entries are precisely the
        // ones whose transactions are already over.
        {
            let mut tokens = ivars.ignored_external_storage_callback_texts.borrow_mut();
            tokens.push(incoming.to_owned());
            if tokens.len() > MAXIMUM_IGNORED_EXTERNAL_CALLBACK_TEXTS {
                tokens.remove(0);
            }
        }
        let storage = self.storage();
        storage.beginEditing();
        match hunks {
            Some(hunks) if !hunks.is_empty() => {
                let incoming_text = ns(incoming);
                let incoming_length = incoming_text.length() as isize;
                let mut ordered = hunks.to_vec();
                ordered.sort_by(|a, b| b.old_range.location.cmp(&a.old_range.location));
                for hunk in ordered {
                    if !(hunk.old_range.location >= 0
                        && hunk.old_range.upper_bound() <= storage.length() as isize
                        && hunk.new_range.location >= 0
                        && hunk.new_range.upper_bound() <= incoming_length)
                    {
                        continue;
                    }
                    let replacement = incoming_text.substringWithRange(foundation_range(hunk.new_range));
                    // Same reference-payload rule as replace(): hunk edits
                    // shift the runs below them without moving payload ranges.
                    // (`substring(with:).utf16.count` of the bridged string.)
                    let replacement_length = utf16_length(&ns_to_string(&replacement));
                    self.project_fragment_payloads(hunk.old_range, replacement_length);
                    storage.replaceCharactersInRange_withString(foundation_range(hunk.old_range), &replacement);
                }
                // Diff hunks are an optimisation, never a source of truth. If a
                // capped/fallback diff cannot express the exact transformation,
                // repair to the byte-faithful incoming text before publishing.
                if !same_text(&self.text(), incoming) {
                    storage.replaceCharactersInRange_withString(FoundationRange::new(0, storage.length()), &incoming_text);
                }
            }
            _ => storage.replaceCharactersInRange_withString(whole, &ns(incoming)),
        }
        storage.endEditing();
        ivars.is_applying_external_change.set(false);
    }

    /// Undo of an external absorb.  The buffer goes back to what the reader
    /// had; the file on disk still holds the agent's version, so that is an
    /// unresolved conflict and is shown as one.
    fn revert_external_buffer(&self, previous: &str, incoming: &str) {
        let ivars = self.ivars();
        self.undo_manager().beginUndoGrouping();
        let redo_incoming = incoming.to_owned();
        self.register_undo(move |document| {
            document.adopt_external_buffer(&redo_incoming, None);
            document.set_dirty(false);
            *document.ivars().pending_conflict.borrow_mut() = None;
            document.reparse_now(false);
        });
        self.undo_manager().setActionName(&NSString::from_str("External Change"));
        self.undo_manager().endUndoGrouping();

        self.storage().beginEditing();
        self.storage().replaceCharactersInRange_withString(FoundationRange::new(0, self.storage().length()), &ns(previous));
        self.storage().endEditing();
        self.reparse_now(false);
        ivars.changes.reset();
        self.set_dirty(true);

        let hunks = TextDiff::hunks(previous, incoming);
        let conflict = Conflict {
            incoming_text: incoming.to_owned(),
            changed_block_count: hunks.len() as isize,
            hunks,
            incoming_fidelity: Some(ivars.fidelity.get()),
            incoming_byte_hash: Some(ivars.disk_byte_hash.borrow().clone()),
        };
        *ivars.pending_conflict.borrow_mut() = Some(conflict.clone());
        ivars.unread_changes.set(UnreadChanges::None);
        self.fire_on_external_event(ExternalEvent::Conflict(conflict));
    }

    /// Restores the reader's selection after the buffer was replaced.
    ///
    /// Matched on the selected *text* rather than on byte offsets, for the
    /// same reason the scroll position is anchored to a heading: an agent
    /// inserting two paragraphs above you must not move what you had
    /// selected.  A selection whose text is gone collapses to a caret at the
    /// restored reading position rather than jumping to the top of the file.
    fn restore_selection(&self, previous_text: &str, selection: NSRange, offset: isize) {
        let handler = self.ivars().restore_selection_handler.borrow().clone();
        let Some(handler) = handler else { return };
        let previous = ns(previous_text);
        let storage_length = self.storage_length();
        if !(selection.length > 0 && selection.upper_bound() <= previous.length() as isize && selection.length <= 4096) {
            handler(NSRange::new(offset.min(storage_length), 0));
            return;
        }
        let needle = previous.substringWithRange(foundation_range(selection));
        let haystack = self.storage().string();
        let haystack_length = haystack.length() as isize;
        let start = offset.min(haystack_length);
        let forward = haystack.rangeOfString_options_range(
            &needle,
            NSStringCompareOptions::LiteralSearch,
            FoundationRange::new(start as usize, (haystack_length - start) as usize),
        );
        let found = if forward.location != objc2_foundation::NSNotFound as usize {
            forward
        } else {
            haystack.rangeOfString_options_range(
                &needle,
                NSStringCompareOptions::LiteralSearch | NSStringCompareOptions::BackwardsSearch,
                FoundationRange::new(0, haystack_length as usize),
            )
        };
        handler(if found.location != objc2_foundation::NSNotFound as usize {
            NSRange::new(found.location as isize, found.length as isize)
        } else {
            NSRange::new(start, 0)
        });
    }

    /// Conflict resolution: take the version on disk, dropping local edits.
    pub fn resolve_conflict_taking_theirs(&self, conflict: &Conflict) {
        let ivars = self.ivars();
        // The accepted generation is also the recovery baseline. A subsequent
        // deletion and Discard must not revive the text from before the
        // conflict.
        *ivars.last_committed_text.borrow_mut() = conflict.incoming_text.clone();
        // Text and byte facts travel together even if the path changes again
        // while the user is reviewing the conflict.
        if let Some(incoming_fidelity) = conflict.incoming_fidelity {
            ivars.fidelity.set(incoming_fidelity);
        }
        *ivars.disk_hash.borrow_mut() = DocumentIO::content_hash(&conflict.incoming_text);
        if let Some(incoming_byte_hash) = &conflict.incoming_byte_hash {
            *ivars.disk_byte_hash.borrow_mut() = incoming_byte_hash.clone();
        }
        self.apply_external_text(&conflict.incoming_text, &conflict.hunks);
        self.publish_presentation_state(PresentationState::NEUTRAL);
    }

    /// Conflict resolution: keep the buffer and write it over the file.
    pub fn resolve_conflict_keeping_mine(&self) -> Result<(), DocumentError> {
        let result = self.save_if_needed(SaveIntent::KeepMine);
        if result.is_ok() {
            *self.ivars().pending_conflict.borrow_mut() = None;
            self.ivars().changes.clear();
        }
        result
    }

    // MARK: - Time travel (§8.3)

    pub fn versions(&self) -> Vec<VersionRecord> {
        let Some(url) = self.url() else { return Vec::new() };
        self.ivars().snapshot_store.versions(&url)
    }

    /// Whether a historical version can still be shown, so the timeline can
    /// distinguish "pruned" from "damaged" instead of showing an empty pane.
    pub fn content(&self, version: &VersionRecord) -> snapshot_store::Content {
        self.ivars().snapshot_store.content(version)
    }

    /// Restores a historical version into the buffer as a normal, undoable
    /// edit.  Choosing a version is a review decision, so the baseline moves
    /// with it: the next agent write is measured against what the user just
    /// chose.
    pub fn restore(&self, version: &VersionRecord) -> bool {
        let snapshot_store::Content::Text(text) = self.ivars().snapshot_store.content(version) else {
            return false;
        };
        let previous = self.text();
        let hunks = TextDiff::hunks(&previous, &text);
        self.replace(NSRange::new(0, self.storage_length()), &text, Some("Restore Version"));
        self.reparse_now(false);
        self.advance_review_baseline(&text);
        let changes = &self.ivars().changes;
        changes.apply(&hunks, &text, &previous, true);
        self.ivars().unread_changes.set(if changes.is_empty() {
            UnreadChanges::None
        } else {
            UnreadChanges::Marked { count: changes.count() }
        });
        true
    }

    // MARK: - NSTextStorageDelegate

    fn handle_text_storage_edit(&self, callback_text: String) {
        let ivars = self.ivars();
        if ivars.suppress_reparse.get() {
            return;
        }
        {
            let mut tokens = ivars.ignored_external_storage_callback_texts.borrow_mut();
            if let Some(index) = tokens.iter().position(|token| same_text(token, &callback_text)) {
                tokens.remove(index);
                return;
            }
        }
        // An explicit transaction may have already published its synchronous
        // parse before this delegate hop runs. Do not schedule a second parse
        // merely because NSTextStorage delivered the callback later.
        if same_text(&self.parsed().text, &self.text()) {
            return;
        }
        // `NSTextStorageDelegate` is delivered on a later main-actor turn. The
        // external transaction already submitted this exact snapshot; consume
        // only its callback above. A subsequent user edit must take the normal
        // path, invalidate the external parse, and own the camera.
        *ivars.pending_external_restore.borrow_mut() = None;
        *ivars.next_external_dirty_override.borrow_mut() = None;
        self.invalidate_parse_work_for_edit();
        if !ivars.is_applying_external_change.get() {
            self.set_dirty(true);
        }
        let undo_manager = self.undo_manager();
        if undo_manager.is_applying_undo_redo() || ivars.is_applying_batch.get() {
            // The enclosing command/undo transaction publishes one coherent
            // parse after all storage edits have landed.
            return;
        }
        if undo_manager.isUndoing() || undo_manager.isRedoing() {
            self.reparse_synchronously(true, false);
            return;
        }
        self.schedule_reparse();
    }
}

/// The detached parse task: `while let result = await
/// coordinator.nextResult() { guard !Task.isCancelled; await
/// self?.applyAsyncParse(result) }`.
fn parse_loop(coordinator: MarkdownParseCoordinator, id: u64, cancelled: Arc<AtomicBool>) {
    let next = coordinator.clone();
    coordinator.next_result(move |result| {
        let Some(result) = result else { return };
        if cancelled.load(Ordering::SeqCst) {
            return;
        }
        DispatchQueue::main().exec_async(move || {
            if let Some(document) = document_for(id) {
                document.apply_async_parse(result);
            }
            parse_loop(next, id, cancelled);
        });
    });
}

/// True when `a` and `b` differ only by the presence of a single trailing
/// newline (LF, CRLF or lone CR are all counted).  Used to treat the final
/// newline as byte-fidelity when reconciling a dirty buffer, so an external
/// absorb never reverts the user's trailing-newline edit. Clean buffers
/// still adopt the exact incoming text.
pub fn final_newline_difference_only(a: &str, b: &str) -> bool {
    fn stripped_newline(s: &str) -> &str {
        // CRLF is one Swift Character, even though it has two UTF-16 units.
        if upleft_swift_text::has_suffix(s, "\r\n") {
            return upleft_swift_text::drop_last(s, 1);
        }
        if upleft_swift_text::has_suffix(s, "\n") || upleft_swift_text::has_suffix(s, "\r") {
            return upleft_swift_text::drop_last(s, 1);
        }
        s
    }
    same_text(stripped_newline(a), stripped_newline(b))
}
