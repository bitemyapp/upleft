import Foundation

#if canImport(FoundationModels)
import FoundationModels
#endif

// C entry points for Apple's on-device model, used by Upleft's port of
// Downright's `AppleOnDeviceAIProvider` (crates/app/src/ai/local_ai.rs).
//
// Only the two framework calls live here. Prompt construction, input
// handling, cancellation checks and result shaping stay in Rust, in the
// order LocalAI.swift performs them. The `#if canImport` and `#available`
// guards are LocalAI.swift's own, so a build or a system without the
// framework reports exactly what Downright reports.

/// `SystemLanguageModel.default.availability`, as `AppleOnDeviceAIProvider`
/// distinguishes it:
///
/// - `0`: `.available`
/// - `1`: `.unavailable(_)` (the reason is not used by Downright)
/// - `2`: FoundationModels compiled in, but the system is older than macOS 26
/// - `3`: FoundationModels could not be imported at build time
@_cdecl("upleft_local_ai_availability")
public func upleftLocalAIAvailability() -> Int32 {
    #if canImport(FoundationModels)
    if #available(macOS 26.0, *) {
        switch SystemLanguageModel.default.availability {
        case .available: return 0
        case .unavailable: return 1
        }
    }
    return 2
    #else
    return 3
    #endif
}

/// Strings cross the boundary as UTF-8 pointer and byte count, never as C
/// strings, so a document holding U+0000 reaches the model whole.
private func string(_ bytes: UnsafePointer<UInt8>?, _ count: Int) -> String {
    guard let bytes, count > 0 else { return "" }
    return String(decoding: UnsafeBufferPointer(start: bytes, count: count), as: UTF8.self)
}

private func withUTF8(_ text: String, _ body: (UnsafePointer<UInt8>?, Int) -> Void) {
    var text = text
    text.withUTF8 { buffer in body(buffer.baseAddress, buffer.count) }
}

/// Completion for `upleft_local_ai_respond`, called exactly once, on a Swift
/// concurrency thread (or before `upleft_local_ai_respond` returns, for
/// status `3`). `status`:
///
/// - `0`: `text` is `response.content`
/// - `1`: `session.respond` threw; `text` is `String(describing: error)`
/// - `2`: it threw `CancellationError`; `text` is null
/// - `3`: FoundationModels is unavailable (not compiled in, or older than
///   macOS 26): LocalAI.swift's `throw LocalAIError.unavailable(.frameworkUnavailable)`
///
/// `text` (UTF-8, `count` bytes) is valid only for the duration of the call.
public typealias UpleftLocalAICompletion = @convention(c) (
    _ context: UnsafeMutableRawPointer?,
    _ status: Int32,
    _ text: UnsafePointer<UInt8>?,
    _ count: Int
) -> Void

/// The running `Task`, so Rust can cancel it the way cancelling Downright's
/// controller task cancels the `session.respond` it is awaiting.
private final class LocalAIRun {
    var task: Task<Void, Never>?
}

/// Runs `LanguageModelSession(model: .default, instructions:)` and
/// `session.respond(to: prompt)` in a new `Task`, and reports through
/// `completion`. Returns a retained handle for `upleft_local_ai_cancel` that
/// the caller must pass to `upleft_local_ai_release` once.
@_cdecl("upleft_local_ai_respond")
public func upleftLocalAIRespond(
    _ instructions: UnsafePointer<UInt8>?,
    _ instructionsCount: Int,
    _ prompt: UnsafePointer<UInt8>?,
    _ promptCount: Int,
    _ context: UnsafeMutableRawPointer?,
    _ completion: UpleftLocalAICompletion
) -> UnsafeMutableRawPointer {
    let run = LocalAIRun()
    let handle = Unmanaged.passRetained(run).toOpaque()
    #if canImport(FoundationModels)
    if #available(macOS 26.0, *) {
        let instructions = string(instructions, instructionsCount)
        let prompt = string(prompt, promptCount)
        run.task = Task {
            do {
                let session = LanguageModelSession(
                    model: .default,
                    instructions: instructions
                )
                let response = try await session.respond(to: prompt)
                withUTF8(response.content) { completion(context, 0, $0, $1) }
            } catch is CancellationError {
                completion(context, 2, nil, 0)
            } catch {
                withUTF8(String(describing: error)) { completion(context, 1, $0, $1) }
            }
        }
        return handle
    }
    #endif
    completion(context, 3, nil, 0)
    return handle
}

/// `Task.cancel()` on the run behind `handle`.
@_cdecl("upleft_local_ai_cancel")
public func upleftLocalAICancel(_ handle: UnsafeMutableRawPointer) {
    Unmanaged<LocalAIRun>.fromOpaque(handle).takeUnretainedValue().task?.cancel()
}

/// Balances the retain `upleft_local_ai_respond` returned.
@_cdecl("upleft_local_ai_release")
public func upleftLocalAIRelease(_ handle: UnsafeMutableRawPointer) {
    Unmanaged<LocalAIRun>.fromOpaque(handle).release()
}
