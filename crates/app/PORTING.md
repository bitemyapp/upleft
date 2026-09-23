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

**Wiring still owed** (for the app-core merge): `integrations::app_intents::install_native_integration` takes `NativeIntegrationPolicy.normalizedPath` and `IntegrationRegistry.shared.open` from `integrations::native_integration`, which the palette branch ports. Until they are installed, `perform` answers `.unavailable`.

**Tests and conformance**

- `tests/local_ai_tests.rs` ports LocalAITests. `appleAdapterAvailabilityFailsClosed` returns early when the model is available, as in Swift.
- The `local-ai` suite never calls the real model. The Swift oracle builds DownrightApp with `-enable-private-imports`, and `LocalAIDump.swift` uses `@_private(sourceFile: "LocalAI.swift") import` to call the Apple adapter's private `input(for:)`, `prompt(task:input:)` and `result(for:input:output:)` directly, next to the deterministic provider, the validator and the controller.
- The shim's FFI path (a real `respond`, and cancellation mid-response) was checked once by hand on a Mac where the model is available. No committed test calls the model.

## Threading notes (document layer)

- `ai::file_watcher` keeps FileWatcher.swift's mechanics:
  - a serial utility-QoS queue, identified by a queue-specific key;
  - an FSEvents stream on that queue;
  - a dispatch timer source for the poll;
  - `dispatch_block_create`d work items;
  - main-queue delivery.

  `new`, `retarget` and `acknowledge_own_write` read and hash the file synchronously, as Swift's `init`, `retarget(to:)` and `acknowledgeOwnWrite(contents:)` do. `FileWatcherTests` depend on that init-time baseline. It is the one place where the port blocks its caller on I/O, and only because the Swift does.
- `ai::markdown_parse_worker` turns the `MarkdownParseCoordinator` actor into a serial dispatch queue. Actor methods become blocks on it, sent asynchronously in the caller's order. `nextResult()` becomes a completion, and the parked `wake` continuation becomes a stored closure. Parses run on the global user-initiated queue.
- Tests whose Swift originals run on the main actor, or whose code delivers to the main queue, are `harness = false` binaries (`tests/main_thread/mod.rs`). They run on the main thread and pump the main run loop while they wait.
