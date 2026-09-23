import AppKit
import Foundation
import MarkdownCore
// `AppleOnDeviceAIProvider.input(for:)`, `prompt(task:input:)` and
// `result(for:input:output:)` are `private`. Package.swift compiles
// DownrightApp with `-enable-private-imports` (symbol visibility only, like
// `-enable-testing`) so the suite can call them without ever reaching
// `LanguageModelSession`.
@_private(sourceFile: "LocalAI.swift") import DownrightApp

/// Swift side of the `local-ai` suite. The real model is never called.
///
///   downright-app-oracle local-ai <case.json> <out.json>
///
/// The case holds
///
/// - `requests`: `{task, source, selection?: [location, length], output?}`.
///   Each runs through `DeterministicLocalAIProvider.run` (its result, and
///   `LocalAIEditValidator.edit` of its preview against the source), and
///   through the Apple adapter's private steps: `input(for:)`, then
///   `prompt(task:input:)` for every task and `result(for:input:output:)`
///   with `output` as the model's reply (default `""`), and the validator on
///   that preview.
/// - `previews`: `{range, originalSource, proposedSource, current}` for
///   `LocalAIPreview.isNoOp` and `LocalAIEditValidator.edit(for:in:)`.
/// - `controller`: lists of requests submitted back to back to one
///   `LocalAILatestWinsController` over the deterministic provider, on the
///   main actor; the dump lists what `onResult` delivered.
///
/// Normalised: the prompt's nonce (`UUID().uuidString.prefix(8)`, random) is
/// replaced by `NONCE` in both markers, after checking its shape.
enum LocalAIDump {
    static func run(input: URL, flags: [String]) throws -> JSON {
        let data = try Data(contentsOf: input)
        guard let root = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw AppOracleError(description: "local-ai: the case is not a JSON object")
        }
        var out: [(String, JSON)] = []
        out.append(("tasks", .array(LocalAITask.allCases.map {
            .object([("rawValue", .string($0.rawValue)), ("title", .string($0.title))])
        })))

        let requests = try (root["requests"] as? [[String: Any]] ?? []).map(request(from:))
        out.append(("requests", .array(requests.map { dumpRequest($0.request, output: $0.output) })))

        let previews = root["previews"] as? [[String: Any]] ?? []
        out.append(("previews", .array(try previews.map { entry in
            guard let range = rangeValue(entry["range"]),
                  let original = entry["originalSource"] as? String,
                  let proposed = entry["proposedSource"] as? String,
                  let current = entry["current"] as? String
            else { throw AppOracleError(description: "local-ai: malformed preview \(entry)") }
            let preview = LocalAIPreview(range: range, originalSource: original, proposedSource: proposed)
            return .object([
                ("isNoOp", .bool(preview.isNoOp)),
                ("edit", edit(LocalAIEditValidator.edit(for: preview, in: current))),
            ])
        })))

        let sequences = root["controller"] as? [[[String: Any]]] ?? []
        out.append(("controller", .array(try sequences.map { sequence in
            try controller(sequence.map(request(from:)).map(\.request))
        })))
        return .object(out)
    }

    private static func request(from entry: [String: Any]) throws -> (request: LocalAIRequest, output: String) {
        guard let raw = entry["task"] as? String, let task = LocalAITask(rawValue: raw),
              let source = entry["source"] as? String
        else { throw AppOracleError(description: "local-ai: malformed request \(entry)") }
        return (
            LocalAIRequest(task: task, source: source, selection: rangeValue(entry["selection"])),
            entry["output"] as? String ?? ""
        )
    }

    private static func rangeValue(_ value: Any?) -> NSRange? {
        guard let pair = value as? [Int], pair.count == 2 else { return nil }
        return NSRange(location: pair[0], length: pair[1])
    }

    private static func dumpRequest(_ request: LocalAIRequest, output: String) -> JSON {
        var fields: [(String, JSON)] = []
        let deterministic = runDeterministic(request)
        fields.append(("deterministic", outcome(deterministic)))
        if case .success(let value) = deterministic, let preview = value.preview {
            fields.append(("deterministicEdit", edit(LocalAIEditValidator.edit(for: preview, in: request.source))))
        } else {
            fields.append(("deterministicEdit", .null))
        }

        var apple: [(String, JSON)] = []
        do {
            let input = try AppleOnDeviceAIProvider.input(for: request)
            apple.append(("input", lines(input)))
            apple.append(("prompts", .array(LocalAITask.allCases.map { task in
                normalizedPrompt(AppleOnDeviceAIProvider.prompt(task: task, input: input))
            })))
            let shaped = AppleOnDeviceAIProvider.result(for: request, input: input, output: output)
            apple.append(("result", result(shaped)))
            apple.append(("edit", edit(shaped.preview.flatMap {
                LocalAIEditValidator.edit(for: $0, in: request.source)
            })))
        } catch {
            apple.append(("error", errorName(error)))
        }
        fields.append(("apple", .object(apple)))
        return .object(fields)
    }

    private final class Box: @unchecked Sendable {
        var outcome: Result<LocalAIResult, Error>?
    }

    /// `try await provider.run(request)` from this synchronous dump.
    private static func runDeterministic(_ request: LocalAIRequest) -> Result<LocalAIResult, Error> {
        let semaphore = DispatchSemaphore(value: 0)
        let box = Box()
        Task.detached {
            do {
                box.outcome = .success(try await DeterministicLocalAIProvider().run(request))
            } catch {
                box.outcome = .failure(error)
            }
            semaphore.signal()
        }
        semaphore.wait()
        return box.outcome!
    }

    private static func controller(_ requests: [LocalAIRequest]) throws -> JSON {
        try MainActor.assumeIsolated {
            let controller = LocalAILatestWinsController(provider: DeterministicLocalAIProvider())
            var delivered: [JSON] = []
            for request in requests {
                controller.submit(request) { delivered.append(outcome($0)) }
            }
            guard !requests.isEmpty else { return .array([]) }
            let deadline = Date().addingTimeInterval(5)
            while delivered.isEmpty, Date() < deadline {
                RunLoop.main.run(until: Date().addingTimeInterval(0.005))
            }
            guard !delivered.isEmpty else {
                throw AppOracleError(description: "local-ai: the controller delivered nothing")
            }
            // Grace: a superseded request would deliver now if it were going to.
            let grace = Date().addingTimeInterval(0.1)
            while Date() < grace {
                RunLoop.main.run(until: Date().addingTimeInterval(0.005))
            }
            return .array(delivered)
        }
    }

    private static func outcome(_ value: Result<LocalAIResult, Error>) -> JSON {
        switch value {
        case .success(let value): return .object([("result", result(value))])
        case .failure(let error): return .object([("error", errorName(error))])
        }
    }

    private static func result(_ value: LocalAIResult) -> JSON {
        .object([
            ("task", .string(value.task.rawValue)),
            ("text", lines(value.text)),
            ("preview", value.preview.map { preview in
                .object([
                    ("range", .range(preview.range)),
                    ("originalSource", lines(preview.originalSource)),
                    ("proposedSource", lines(preview.proposedSource)),
                    ("isNoOp", .bool(preview.isNoOp)),
                ])
            } ?? .null),
        ])
    }

    private static func edit(_ value: TextEdit?) -> JSON {
        guard let value else { return .null }
        return .object([
            ("range", .range(value.range)),
            ("replacement", lines(value.replacement)),
            ("summary", .string(value.summary)),
        ])
    }

    private static func errorName(_ error: Error) -> JSON {
        if let error = error as? LocalAIError {
            switch error {
            case .emptyInput: return .string("emptyInput")
            case .cancelled: return .string("cancelled")
            case .unavailable(let availability):
                switch availability {
                case .available: return .string("unavailable(available)")
                case .frameworkUnavailable: return .string("unavailable(frameworkUnavailable)")
                case .systemUnavailable: return .string("unavailable(systemUnavailable)")
                }
            }
        }
        if error is CancellationError { return .string("cancellation") }
        return .string("other")
    }

    /// Text as lines split on each U+000A code unit (literally, so `\r\n`
    /// keeps its `\r`), so a difference names the line.
    private static func lines(_ text: String) -> JSON {
        var parts: [JSON] = []
        var current: [UInt16] = []
        for unit in text.utf16 {
            if unit == 0x0A {
                parts.append(.string(String(utf16CodeUnits: current, count: current.count)))
                current.removeAll()
            } else {
                current.append(unit)
            }
        }
        parts.append(.string(String(utf16CodeUnits: current, count: current.count)))
        return .array(parts)
    }

    /// The prompt as lines, with the nonce in both markers replaced by `NONCE`.
    private static func normalizedPrompt(_ prompt: String) -> JSON {
        let lead = "=====BEGIN DOCUMENT ["
        guard let start = prompt.range(of: lead, options: .literal)?.upperBound,
              let close = prompt[start...].firstIndex(of: "]")
        else { return .object([("nonce", .string("missing"))]) }
        let nonce = String(prompt[start..<close])
        let shape = nonce.utf8.count == 8 && nonce.utf8.allSatisfy {
            ($0 >= 0x30 && $0 <= 0x39) || ($0 >= 0x41 && $0 <= 0x46)
        }
        let normalized = prompt
            .replacingOccurrences(
                of: "=====BEGIN DOCUMENT [\(nonce)]=====", with: "=====BEGIN DOCUMENT [NONCE]=====", options: .literal
            )
            .replacingOccurrences(
                of: "=====END DOCUMENT [\(nonce)]=====", with: "=====END DOCUMENT [NONCE]=====", options: .literal
            )
        return .object([
            ("nonce", .string(shape ? "8 upper-case hex digits" : "unexpected: \(nonce)")),
            ("lines", lines(normalized)),
        ])
    }
}
