import Foundation

// Downright's App Intents (Sources/DownrightApp/Integrations/AppIntents.swift),
// copied declaration for declaration. The one change is inside `perform()`:
// Downright calls `NativeIntegrationPolicy.normalizedPath` and then, on the
// main actor, `IntegrationRegistry.shared.open`. Here both run in Rust
// (`integrations::app_intents::perform`), reached through the C function
// pointer Rust registers with `upleft_app_intents_register_open_markdown`.
// Rust answers with the outcome and this file throws the same errors.

/// Called by Rust when the outcome of an open is known: `0` opened,
/// `1` `OpenMarkdownIntentError.unsupportedFile`, `2` `.unavailable`.
public typealias UpleftOpenMarkdownCompletion = @convention(c) (
    _ context: UnsafeMutableRawPointer?,
    _ status: Int32
) -> Void

/// Rust's `perform()` body: `path` (UTF-8, `count` bytes) is valid for the
/// duration of the call; `completion(context, status)` must be called exactly
/// once, from any thread.
public typealias UpleftOpenMarkdownHandler = @convention(c) (
    _ path: UnsafePointer<UInt8>?,
    _ count: Int,
    _ context: UnsafeMutableRawPointer?,
    _ completion: UpleftOpenMarkdownCompletion
) -> Void

private let openMarkdownLock = NSLock()
nonisolated(unsafe) private var openMarkdownHandler: UpleftOpenMarkdownHandler?

/// Installs (or, with `nil`, removes) Rust's `perform()` body.
@_cdecl("upleft_app_intents_register_open_markdown")
public func upleftAppIntentsRegisterOpenMarkdown(_ handler: UpleftOpenMarkdownHandler?) {
    openMarkdownLock.lock()
    openMarkdownHandler = handler
    openMarkdownLock.unlock()
}

/// Whether this build carries the App Intents declarations
/// (`#if canImport(AppIntents)` in AppIntents.swift).
@_cdecl("upleft_app_intents_available")
public func upleftAppIntentsAvailable() -> Bool {
    #if canImport(AppIntents)
    return true
    #else
    return false
    #endif
}

private final class OpenMarkdownContinuation {
    let continuation: CheckedContinuation<Int32, Never>
    init(_ continuation: CheckedContinuation<Int32, Never>) { self.continuation = continuation }
}

private func registeredOpenMarkdownHandler() -> UpleftOpenMarkdownHandler? {
    openMarkdownLock.lock()
    defer { openMarkdownLock.unlock() }
    return openMarkdownHandler
}

/// Hands `path` to Rust and waits for its outcome. With no handler registered
/// the registry has no open handler, which Downright reports as `.unavailable`.
func upleftOpenMarkdown(path: String) async -> Int32 {
    guard let handler = registeredOpenMarkdownHandler() else { return 2 }
    return await withCheckedContinuation { continuation in
        let box = Unmanaged.passRetained(OpenMarkdownContinuation(continuation)).toOpaque()
        var path = path
        path.withUTF8 { buffer in
            handler(buffer.baseAddress, buffer.count, box) { context, status in
                guard let context else { return }
                Unmanaged<OpenMarkdownContinuation>.fromOpaque(context)
                    .takeRetainedValue()
                    .continuation.resume(returning: status)
            }
        }
    }
}

#if canImport(AppIntents)
import AppIntents

/// Opens one user-selected Markdown file in the running app.  App Intents are
/// deliberately thin: all file policy and routing lives in the registry.
@available(macOS 14.0, *)
public struct OpenMarkdownIntent: AppIntent {
    public static let title: LocalizedStringResource = "Open Markdown in Downright"
    public static let description = IntentDescription("Open a Markdown document in Downright.")
    public static let openAppWhenRun = true

    @Parameter(title: "File path")
    public var path: String

    public init() {}

    public init(path: String) {
        self.path = path
    }

    public func perform() async throws -> some IntentResult {
        switch await upleftOpenMarkdown(path: path) {
        case 0: return .result()
        case 1: throw OpenMarkdownIntentError.unsupportedFile
        default: throw OpenMarkdownIntentError.unavailable
        }
    }
}

@available(macOS 14.0, *)
public enum OpenMarkdownIntentError: LocalizedError {
    case unsupportedFile
    case unavailable

    public var errorDescription: String? {
        switch self {
        case .unsupportedFile: "Choose a Markdown file."
        case .unavailable: "Downright could not open that file."
        }
    }
}

@available(macOS 14.0, *)
public struct DownrightShortcuts: AppShortcutsProvider {
    public static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: OpenMarkdownIntent(),
            phrases: ["Open a file in \(.applicationName)"],
            shortTitle: "Open Markdown",
            systemImageName: "doc.text"
        )
    }
}
#endif
