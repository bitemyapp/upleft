# upleft-app porting notes

## Swift shim (`swift-shim/`)

Upleft contains no Swift, with one sanctioned exception: this shim. FoundationModels and App Intents have no Objective-C or C interface. Their API is Swift structs, generics, property wrappers and macros, so objc2 cannot reach them. The shim holds only what cannot be written in Rust.

**What is in it**

- `LocalAIShim.swift` has four `@_cdecl` entry points for FoundationModels:
  - `upleft_local_ai_availability` returns `SystemLanguageModel.default.availability` in the terms LocalAI.swift distinguishes. The codes are: `0` available, `1` unavailable (the reason is ignored, as Downright ignores it), `2` compiled with the framework but running before macOS 26, `3` built without it (`#if canImport(FoundationModels)` false).
  - `upleft_local_ai_respond` runs `LanguageModelSession(model: .default, instructions:)` and then `respond(to:)` in a new `Task`. It reports the response text, the error's `String(describing:)`, a `CancellationError`, or "framework unavailable" through a completion callback.
  - `upleft_local_ai_cancel` and `upleft_local_ai_release` cancel and free the task handle.
  - Strings cross the boundary as UTF-8 pointer and length, so a document holding U+0000 reaches the model whole.
- `AppIntentsShim.swift` holds `OpenMarkdownIntent`, `OpenMarkdownIntentError` and `DownrightShortcuts`, copied from AppIntents.swift, titles and strings unchanged. The one change is the body of `perform()`. It hands the path to the C function pointer Rust registers (`upleft_app_intents_register_open_markdown`, done by `integrations::app_intents::register()`), awaits the outcome, and throws the same errors. With no handler registered it throws `.unavailable`, as Downright does when the registry has no open handler.

**What stays in Rust** (`ai::local_ai`, `integrations::app_intents`)

These follow LocalAI.swift line for line: prompt construction (including the nonce and the directive text), `input(for:)`, both availability reads, the cancellation checks before and after the model call, result shaping, `DeterministicLocalAIProvider`, `LocalAIEditValidator` and `LocalAILatestWinsController`. `perform()`'s body is also Rust: the path is normalised off the main thread, then opened on the main queue.

**Build** (`build.rs`)

- `swift build -c release --triple <arch>-apple-macosx14.0`, with the scratch directory under `OUT_DIR`. The build reruns when `swift-shim/Package.swift` or `Sources/` changes, and SwiftPM rebuilds incrementally.
- The package targets `.macOS(.v14)` with Swift 5 language mode, as in Downright's Package.swift.
- `cargo:rustc-link-lib=static=UpleftSwiftShim`, plus link search paths for the toolchain's `usr/lib/swift/macosx`, `/usr/lib/swift` (both from `swift -print-target-info`) and the SDK's `usr/lib/swift`.
- The Swift object's autolink entries (`LC_LINKER_OPTION`: `-lswiftCore`, `-lswift_Concurrency`, `-framework FoundationModels`, and so on) pull in the libraries. `rustc-link-search` and `rustc-link-lib` propagate to every dependent binary and test (`rustc-link-arg` would not), so `cargo test -p upleft-app`, `upleft-oracle` and `conform` all link.
- The Swift runtime comes from the OS (`/usr/lib/swift`), so no rpath is needed from macOS 14 on.
- Stamping: nothing to stamp. A static library has no `LC_BUILD_VERSION` that AppKit reads. Its object records `minos 14.0` (checked with `otool -l`), and each executable's build version still comes from the Rust link (docs/BUILD-VERSION.md).

**App Intents metadata.** Xcode runs `appintentsmetadataprocessor` to register intents with the system. Downright's SwiftPM bundle script does not, and neither does this build. The app-bundle port should extract `Metadata.appintents` from the linked binary if Shortcuts integration is wanted.

`integrations::app_intents::perform` calls `NativeIntegrationPolicy::normalized_path` and `IntegrationRegistry::shared(mtm).open` from `integrations::native_integration` directly, as the Swift does.

**Tests and conformance**

- `tests/local_ai_tests.rs` ports LocalAITests. `appleAdapterAvailabilityFailsClosed` returns early when the model is available, as in Swift.
- The `local-ai` suite never calls the real model. The Swift oracle builds DownrightApp with `-enable-private-imports`, and `LocalAIDump.swift` uses `@_private(sourceFile: "LocalAI.swift") import` to call the Apple adapter's private `input(for:)`, `prompt(task:input:)` and `result(for:input:output:)` directly, next to the deterministic provider, the validator and the controller.
- The shim's FFI path (a real `respond`, and cancellation mid-response) was checked once by hand on a Mac where the model is available. No committed test calls the model.

## Document layer (`ai::file_watcher`, `ai::markdown_parse_worker`, `ai::markdown_document`, `ai::sibling_scanner`)

**How Swift's concurrency maps**

- `ai::file_watcher` keeps FileWatcher.swift's mechanics:
  - a serial utility-QoS queue (`com.ezzy.downright.filewatcher`), identified by a queue-specific key;
  - an FSEvents stream delivered on that queue;
  - a dispatch timer source for the 1.5 s poll;
  - `dispatch_block_create`d work items for the 0.30 s coalescing and the 0.35 s removal re-probe;
  - main-queue delivery.
- `ai::markdown_parse_worker` turns the `MarkdownParseCoordinator` actor into a serial dispatch queue:
  - actor methods become blocks on it, sent asynchronously in the caller's order;
  - that order is also what `MarkdownDocument.enqueueParseControl`'s task chain guarantees;
  - `nextResult()` becomes a completion, and the parked `wake` continuation becomes a stored closure;
  - parses run on the global user-initiated queue.
- `ai::markdown_document`: `MarkdownDocument` is a `define_class!` `NSObject` subclass named `MarkdownDocument`. It is its storage's `NSTextStorageDelegate`, and `MarkdownUndoManager` is an `NSUndoManager` subclass named `MarkdownUndoManager`. How the Swift constructs map:
  - `Task { @MainActor [weak self] … }` and `await self?.…` become main-queue blocks. They carry a document id that a main-thread registry resolves, so no Objective-C object crosses threads.
  - `Task.detached(priority: .userInitiated)` becomes the global user-initiated queue.
  - `DispatchWorkItem` becomes a `dispatch_block_create`d block.
  - `RunLoop.main.perform(inModes: [.common])` becomes `-[NSRunLoop performInModes:block:]`.
  - `registerUndo(withTarget:handler:)` becomes `-registerUndoWithTarget:handler:`.
  - The storage delegate captures the post-edit text and hops to the main queue, as in Swift.
- `ai::sibling_scanner`: background scans run on a serial utility queue (`com.ezzy.downright.sibling-scan`) and land on the main queue by id.
- Seams (Swift reads the singletons directly):
  - `MarkdownDocument::with_dependencies` takes an optional `Preferences` in place of `Preferences.shared`.
  - `SiblingScanner::with_document_state_store` takes a store in place of `DocumentStateStore.shared`.

  Production uses `MarkdownDocument::new` and `SiblingScanner::new`, which read the shared instances as Swift does.

**Main-thread I/O (candidates for the UI port to call off-main).** The model layer does its I/O on the same threads as Swift, per the coordinator's decision, so these sites block their caller (the main thread in the app) on I/O:

- `FileWatcher::new`, `retarget`: `resolvingSymlinksInPath`, plus a read and SHA-256 of the file.
- `FileWatcher::acknowledge_own_write`, `suppress_own_write`, `cancel_own_write_suppression`, `stop`: synchronous hops onto the watcher queue. `acknowledge_own_write` also reads and hashes the file there.
- `MarkdownDocument::open`:
  - `resolvingSymlinksInPath` and the full file read;
  - `DocumentStateStore.state` (reads the state file);
  - `noteOpened` (reads and rewrites `recents.json`);
  - `SnapshotStore.content(forHash:)` in the review-state restore (reads and decompresses an object);
  - the structure-only parse;
  - the `FileWatcher` above.
- `MarkdownDocument::save`, `save_if_needed`, `recreate_missing_file`, `resolve_conflict_keeping_mine`, `toggle_task` (which saves):
  - `inspect_disk_state` (a full read and hashes);
  - encoding;
  - the atomic write with `fsync`;
  - the watcher acknowledgement;
  - `DocumentStateStore.save`.
- `MarkdownDocument::inspect_disk_state` and `discard_unsaved_changes`: a full read.
- `MarkdownDocument::close`: `DocumentStateStore.save` (a write).
- The renamed-file handling from a watcher event: `DocumentStateStore.save`.
- `MarkdownDocument::versions`, `content`, `restore`: snapshot index and object reads.
- `MarkdownDocument::adopt`, `apply`, `reparse_now`, `ensure_parsed_current`, and undo/redo: a synchronous `MarkdownParser.parse` of the whole buffer (by design in Swift: explicit transactions converge before returning).
- `SiblingScanner::new` and `scan(true, _)`: a directory listing, plus a `DocumentStateStore` state read per sibling. With `compute_changes`, it also reads and hashes each sibling up to 2 MB.

The external-write path (`handle_external_write` → absorb) reads and diffs off the main thread, as in Swift.

**Tests**

- Test binaries whose Swift originals run on the main actor, or whose code delivers to the main queue, are `harness = false` (`tests/main_thread/mod.rs`). They run on the main thread and pump the main run loop wherever the Swift test awaits.
- The document tests never touch the real home (`tests/document_support/mod.rs`). They point Downright's own `DOWNRIGHT_SUPPORT_DIRECTORY` override at a temporary folder before any store is created, so `SnapshotStore.shared` and `DocumentStateStore.shared` keep their process-wide semantics inside the sandbox. Documents get a `Preferences::for_testing` instance, because loading `Preferences.shared` publishes the Quick Look appearance to the real user defaults.
