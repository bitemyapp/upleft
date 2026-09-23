import Foundation
import MarkdownCore
import MarkdownRender
@testable import DownrightApp

/// Swift side of the `formats` suite: Downright's persisted formats, driven by
/// a script (`corpus/formats/*.json`; the format is described in the doc
/// comment of `crates/conformance/src/dump/formats.rs`, the Rust side, which
/// must stay in step with this file).
///
/// Each script runs in a fresh sandbox under `NSTemporaryDirectory()`. Its
/// steps call the real stores (SnapshotStore, DocumentStateStore,
/// ChangeTracker, Preferences.Values, ReaderProfiles, ReviewSidecar and its
/// anchor resolver, DocumentTrust and TrustStore, PathResolver and
/// ExternalEditor, JumpHistory, ScrollAnchoring); each step's result is
/// dumped, then every file the sandbox holds. `Preferences.shared` is never
/// touched: it writes the user's global preferences, which no sandbox
/// isolates.
///
/// Files are dumped as:
/// - `hex`: snapshot objects (zlib bytes), and anything that is not UTF-8 or
///   starts with a byte-order mark;
/// - `lines`: everything else, split on "\n" — byte-exact, which is how
///   `preferences.json`, review sidecars and `trust.json` (sorted-key
///   encoders) are compared;
/// - `json`: files written by a `JSONEncoder` without `.sortedKeys` (snapshot
///   indexes, state files, `recents.json`, and any path a script lists in
///   `"parsed"`), whose key order changes from process to process: parsed
///   with `JSONSerialization` and rewritten with `[.prettyPrinted,
///   .sortedKeys]`. A file that does not parse falls back to `lines`.
///
/// Normalised, and nothing else:
/// - the sandbox root path, spelled `<root>`;
/// - document keys (SHA-256 of a document path, which contains the sandbox
///   root), spelled `<key:relative/path>` in file names and values;
/// - `UUID` strings, spelled `<uuid-N>` in order of first appearance
///   (Downright makes random ones for marks, reviews and custom profiles);
/// - timestamps read from the clock (`Date()`, which these APIs do not let a
///   caller inject): in step results, and in `json` files under the keys
///   `date`, `created`, `lastOpened`, `firstSeen`, a date within two days of
///   the run is `<now>`; fixed dates a script supplies are compared as written;
/// - `Set` members Swift writes in hash order: `foldedHeadings`,
///   `expandedCodeBlocks`, `collapsedCodeBlocks` are sorted in `json` files,
///   and the lines of each `"effects" : [ … ]` block in `trust.json` are
///   sorted (commas re-placed).
enum FormatsDump {
    static func run(input: URL, flags: [String]) throws -> JSON {
        let data = try Data(contentsOf: input)
        guard let script = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let steps = script["steps"] as? [[String: Any]]
        else { throw AppOracleError(description: "formats: a script is an object with \"steps\"") }
        let runner = try FormatsRunner(script: script)
        defer { runner.cleanup() }
        var results: [JSON] = []
        for (index, step) in steps.enumerated() {
            let op = step["op"] as? String ?? ""
            let value = try runner.perform(op, step)
            results.append(.object([("step", .int(index)), ("op", .string(op)), ("value", value)]))
        }
        runner.drain()
        let dump = JSON.object([("results", .array(results)), ("files", .array(runner.dumpFiles()))])
        return runner.normalize(dump)
    }
}

private func sync(_ body: @escaping @Sendable () async -> Void) {
    let semaphore = DispatchSemaphore(value: 0)
    Task.detached {
        await body()
        semaphore.signal()
    }
    semaphore.wait()
}

private func utf8Less(_ a: String, _ b: String) -> Bool {
    Array(a.utf8).lexicographicallyPrecedes(Array(b.utf8))
}

private let isoEncoder: JSONEncoder = {
    let encoder = JSONEncoder()
    encoder.dateEncodingStrategy = .iso8601
    return encoder
}()

private let isoDecoder: JSONDecoder = {
    let decoder = JSONDecoder()
    decoder.dateDecodingStrategy = .iso8601
    return decoder
}()

/// `JSONEncoder`'s `.iso8601` text for a date.
private func iso8601(_ date: Date) -> String {
    let text = String(data: try! isoEncoder.encode([date]), encoding: .utf8)!
    return String(text.dropFirst(2).dropLast(2))
}

/// `JSONDecoder`'s `.iso8601` reading of a string.
private func parseDate(_ text: String) -> Date? {
    guard let data = try? JSONEncoder().encode([text]) else { return nil }
    return (try? isoDecoder.decode([Date].self, from: data))?.first
}

final class FormatsRunner {
    let root: URL
    let rootPath: String
    let unresolvedRootPath: String
    let runStart = Date()
    let parsedPaths: [String]
    let hasSupportDirectory: Bool

    var snapshotStores: [String: SnapshotStore] = [:]
    var historyDirectories: [String: URL] = [:]
    var stateStores: [String: DocumentStateStore] = [:]
    var supportDirectories: [String: URL] = [:]
    var trackers: [String: ChangeTracker] = [:]
    var reviews: [String: ReviewItem] = [:]
    var trustStores: [String: TrustStore] = [:]
    var resolvers: [String: PathResolver] = [:]
    var histories: [String: JumpHistory] = [:]
    var mentionedPaths: Set<String> = []

    init(script: [String: Any]) throws {
        let base = URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent("upleft-formats-\(UUID().uuidString.lowercased())", isDirectory: true)
        try FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        unresolvedRootPath = base.path
        root = base.resolvingSymlinksInPath()
        rootPath = root.path
        parsedPaths = script["parsed"] as? [String] ?? []
        hasSupportDirectory = script["supportDirectory"] is String
        if let support = script["supportDirectory"] as? String {
            setenv("DOWNRIGHT_SUPPORT_DIRECTORY", url(support).path, 1)
        }
    }

    /// A script path: relative to the sandbox, or absolute as written.
    func url(_ relative: String) -> URL {
        if relative.hasPrefix("/") { return URL(fileURLWithPath: relative) }
        mentionedPaths.insert(relative)
        return relative.isEmpty ? root : root.appendingPathComponent(relative)
    }

    func cleanup() {
        func open(_ path: String) {
            var info = stat()
            guard lstat(path, &info) == 0, (info.st_mode & S_IFMT) == S_IFDIR else { return }
            _ = chmod(path, 0o755)
            for name in (try? FileManager.default.contentsOfDirectory(atPath: path)) ?? [] {
                let child = path + "/" + name
                var childInfo = stat()
                if lstat(child, &childInfo) == 0, (childInfo.st_mode & S_IFMT) == S_IFREG { _ = chmod(child, 0o644) }
                open(child)
            }
        }
        open(rootPath)
        try? FileManager.default.removeItem(atPath: rootPath)
    }

    func drain() {
        for store in snapshotStores.values {
            sync { await store.waitForPendingWrites() }
        }
    }

    // MARK: - Step arguments

    func string(_ step: [String: Any], _ key: String) -> String? { step[key] as? String }
    func int(_ step: [String: Any], _ key: String) -> Int? { (step[key] as? NSNumber)?.intValue }
    func double(_ step: [String: Any], _ key: String) -> Double? { (step[key] as? NSNumber)?.doubleValue }
    func bool(_ step: [String: Any], _ key: String) -> Bool? { (step[key] as? NSNumber)?.boolValue }

    func range(_ value: Any?) -> NSRange {
        let pair = (value as? [NSNumber])?.map(\.intValue) ?? [0, 0]
        return NSRange(location: pair[0], length: pair[1])
    }

    func require<T>(_ value: T?, _ what: String) throws -> T {
        guard let value else { throw AppOracleError(description: "formats: missing \(what)") }
        return value
    }

    func path(_ step: [String: Any], _ key: String) throws -> URL {
        url(try require(string(step, key), key))
    }

    func name(_ step: [String: Any], _ key: String = "store") -> String { string(step, key) ?? "main" }

    /// A script string with `<root>` spelled out as the sandbox path.
    func expand(_ text: String?) -> String {
        (text ?? "").replacingOccurrences(of: "<root>", with: rootPath)
    }

    /// `text` or, for texts a script cannot spell in JSON (`JSONSerialization`
    /// drops a string's leading U+FEFF), `textHex`.
    func textField(_ step: [String: Any], _ key: String) -> String {
        if let hex = string(step, key + "Hex") {
            let digits = Array(hex.utf8)
            var bytes: [UInt8] = []
            var index = 0
            while index + 1 < digits.count {
                bytes.append(UInt8(String(decoding: digits[index..<index + 2], as: UTF8.self), radix: 16) ?? 0)
                index += 2
            }
            return String(decoding: bytes, as: UTF8.self)
        }
        return string(step, key) ?? ""
    }

    func writeText(_ text: String, to target: URL) throws {
        try FileManager.default.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data(text.utf8).write(to: target)
    }

    // MARK: - Values

    func stamp(_ date: Date) -> JSON {
        abs(date.timeIntervalSince(runStart)) < 2 * 86_400 ? .string("<now>") : .string(iso8601(date))
    }

    func ranges(_ values: [NSRange]) -> JSON { .array(values.map { JSON.range($0) }) }

    func hex(_ data: Data) -> JSON {
        .object([("hex", .string(data.map { String(format: "%02x", $0) }.joined()))])
    }

    func lines(_ data: Data) -> JSON {
        guard !data.starts(with: [0xEF, 0xBB, 0xBF]), let text = String(data: data, encoding: .utf8) else { return hex(data) }
        return .object([("lines", .array(text.components(separatedBy: "\n").map { .string($0) }))])
    }

    func errorCode(_ error: Error) -> JSON {
        .object([("error", .int((error as NSError).code))])
    }

    func record(_ record: SnapshotStore.VersionRecord?) -> JSON {
        guard let record else { return .null }
        return .object([
            ("hash", .string(record.hash)),
            ("date", stamp(record.date)),
            ("byteCount", .int(record.byteCount)),
            ("kind", .string(record.kind.rawValue)),
        ])
    }

    func content(_ content: SnapshotStore.Content) -> JSON {
        switch content {
        case .text(let text): return .object([("text", .string(text))])
        case .missing: return .string("missing")
        case .corrupt: return .string("corrupt")
        }
    }

    func mark(_ mark: ChangeTracker.Mark?) -> JSON {
        guard let mark else { return .null }
        return .object([
            ("id", .string(mark.id.uuidString)),
            ("kind", .string(mark.kind.rawValue)),
            ("range", .range(mark.range)),
            ("wordRanges", ranges(mark.wordRanges)),
            ("deletedText", .string(mark.deletedText)),
            ("created", stamp(mark.created)),
            ("visited", .bool(mark.visited)),
            ("firstSeen", mark.firstSeen.map(stamp) ?? .null),
        ])
    }

    func persisted(_ mark: ChangeTracker.PersistedMark) -> JSON {
        .object([
            ("id", .string(mark.id.uuidString)),
            ("kind", .string(mark.kind)),
            ("range", .range(mark.range.range)),
            ("wordRanges", ranges(mark.wordRanges.map(\.range))),
            ("deletedText", .string(mark.deletedText)),
            ("created", stamp(mark.created)),
            ("visited", .bool(mark.visited)),
        ])
    }

    func anchor(_ anchor: ScrollAnchor) -> JSON {
        .object([
            ("headingSlug", .string(anchor.headingSlug)),
            ("headingIndex", .int(anchor.headingIndex)),
            ("fractionThroughSection", .double(anchor.fractionThroughSection)),
        ])
    }

    func scrollAnchor(_ value: Any?) -> ScrollAnchor {
        let spec = value as? [String: Any] ?? [:]
        return ScrollAnchor(
            headingSlug: spec["headingSlug"] as? String ?? "",
            headingIndex: (spec["headingIndex"] as? NSNumber)?.intValue ?? 0,
            fractionThroughSection: (spec["fractionThroughSection"] as? NSNumber)?.doubleValue ?? 0
        )
    }

    func state(_ state: DocumentState) -> JSON {
        .object([
            ("path", .string(state.path)),
            ("lastSeenHash", .string(state.lastSeenHash)),
            ("reviewBaselineHash", .string(state.reviewBaselineHash)),
            ("marks", .array(state.marks.map(persisted))),
            ("anchor", anchor(state.anchor)),
            ("mode", .string(state.mode.rawValue)),
            ("zoomLevel", .int(state.zoomLevel.rawValue)),
            ("foldedHeadings", .array(state.foldedHeadings.sorted(by: utf8Less).map { .string($0) })),
            ("expandedCodeBlocks", .array(state.expandedCodeBlocks.sorted().map { .int($0) })),
            ("collapsedCodeBlocks", .array(state.collapsedCodeBlocks.sorted().map { .int($0) })),
            ("lastOpened", stamp(state.lastOpened)),
            ("sidebarVisible", .bool(state.sidebarVisible)),
            ("selectionLocation", .int(state.selectionLocation)),
            ("selectionLength", .int(state.selectionLength)),
            ("splitViewEnabled", .bool(state.splitViewEnabled)),
        ])
    }

    func recent(_ recent: RecentDocument) -> JSON {
        .object([
            ("path", .string(recent.path)),
            ("displayName", .string(recent.displayName)),
            ("firstHeading", .string(recent.firstHeading)),
            ("lastOpened", stamp(recent.lastOpened)),
            ("wordCount", .int(recent.wordCount)),
        ])
    }

    func typography(_ typography: TypographyConfig) -> JSON {
        .object([
            ("preset", .string(typography.preset.rawValue)),
            ("bodySize", .double(Double(typography.bodySize))),
            ("scaleRatio", .double(Double(typography.scaleRatio))),
            ("lineHeightMultiple", .double(Double(typography.lineHeightMultiple))),
            ("measureCharacters", .double(Double(typography.measureCharacters))),
            ("monoFamily", .string(typography.monoFamily)),
            ("monoSizeAdjust", .double(Double(typography.monoSizeAdjust))),
            ("monoLigatures", .bool(typography.monoLigatures)),
            ("opticalMargins", .bool(typography.opticalMargins)),
            ("mathScale", .double(Double(typography.mathScale))),
        ])
    }

    func profile(_ profile: ReaderProfile) -> JSON {
        .object([
            ("id", .string(profile.id)),
            ("name", .string(profile.name)),
            ("isBuiltIn", .bool(profile.isBuiltIn)),
            ("typographyScale", .string(profile.typographyScale.rawValue)),
            ("scaleValue", .double(Double(profile.typographyScale.value))),
            ("scaleTitle", .string(profile.typographyScale.title)),
            ("measureCharacters", .double(Double(profile.measureCharacters))),
            ("chromeDensity", .string(profile.chromeDensity.rawValue)),
            ("densityTitle", .string(profile.chromeDensity.title)),
            ("motionPreference", .string(profile.motionPreference.rawValue)),
            ("motionTitle", .string(profile.motionPreference.title)),
        ])
    }

    func reviewAnchor(_ anchor: ReviewAnchor?) -> JSON {
        guard let anchor else { return .null }
        return .object([
            ("range", .range(anchor.range)),
            ("selectedText", .string(anchor.selectedText)),
            ("beforeFingerprint", .string(anchor.beforeFingerprint)),
            ("afterFingerprint", .string(anchor.afterFingerprint)),
        ])
    }

    func review(_ item: ReviewItem?) -> JSON {
        guard let item else { return .null }
        return .object([
            ("id", .string(item.id.uuidString)),
            ("kind", .string(item.kind.rawValue)),
            ("title", .string(item.title)),
            ("anchor", reviewAnchor(item.anchor)),
            ("body", .string(item.body)),
            ("replacement", .string(item.replacement)),
            ("state", .string(item.state.rawValue)),
        ])
    }

    func grant(_ grant: TrustGrant) -> JSON {
        .object([
            ("scope", .string(grant.scope.rawValue)),
            ("canonicalPath", .string(grant.canonicalPath)),
            ("effects", .array(grant.effects.map(\.rawValue).sorted(by: utf8Less).map { .string($0) })),
            ("externalURL", .string(grant.externalURL)),
        ])
    }

    func entry(_ entry: JumpHistory.Entry?) -> JSON {
        guard let entry else { return .null }
        return .object([("url", .string(entry.url?.path)), ("offset", .int(entry.offset)), ("label", .string(entry.label))])
    }

    // MARK: - Steps

    func perform(_ op: String, _ step: [String: Any]) throws -> JSON {
        switch op {
        // Files
        case "write":
            let target = try path(step, "path")
            try FileManager.default.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
            if let hex = string(step, "hex") {
                let digits = Array(hex.utf8)
                var bytes = Data()
                var index = 0
                while index + 1 < digits.count {
                    bytes.append(UInt8(String(decoding: digits[index..<index + 2], as: UTF8.self), radix: 16)!)
                    index += 2
                }
                try bytes.write(to: target)
            } else {
                try Data(expand(string(step, "text")).utf8).write(to: target)
            }
            return .null
        case "mkdir":
            try FileManager.default.createDirectory(at: try path(step, "path"), withIntermediateDirectories: true)
            return .null
        case "symlink":
            try FileManager.default.createSymbolicLink(atPath: try path(step, "path").path, withDestinationPath: try path(step, "target").path)
            return .null
        case "remove":
            try? FileManager.default.removeItem(at: try path(step, "path"))
            return .null
        case "chmod":
            _ = chmod(try path(step, "path").path, mode_t(try require(int(step, "mode"), "mode")))
            return .null
        case "touch":
            let date = try require(parseDate(try require(string(step, "date"), "date")), "date")
            try FileManager.default.setAttributes([.modificationDate: date], ofItemAtPath: try path(step, "path").path)
            return .null
        case "truncate":
            let target = try path(step, "path")
            let data = try Data(contentsOf: target)
            try data.prefix(int(step, "length") ?? data.count / 2).write(to: target)
            return .null
        case "exists":
            return .bool(FileManager.default.fileExists(atPath: try path(step, "path").path))

        // SnapshotStore
        case "snapshot.open":
            let history = try path(step, "history")
            snapshotStores[name(step)] = SnapshotStore(historyDirectory: history)
            historyDirectories[name(step)] = history
            return .null
        case "snapshot.writeIndex":
            let history = try require(historyDirectories[name(step)], "snapshot store")
            let file = history.appendingPathComponent("index", isDirectory: true)
                .appendingPathComponent(SnapshotStore.documentKey(for: try path(step, "doc")) + ".json")
            try writeText(expand(string(step, "text")), to: file)
            return .null
        case "snapshot.touchObject":
            let history = try require(historyDirectories[name(step)], "snapshot store")
            let hash = try require(string(step, "hash"), "hash")
            let file = history.appendingPathComponent("objects", isDirectory: true)
                .appendingPathComponent(String(hash.prefix(2)), isDirectory: true).appendingPathComponent(hash)
            let date = try require(parseDate(string(step, "date") ?? ""), "date")
            try FileManager.default.setAttributes([.modificationDate: date], ofItemAtPath: file.path)
            return .null
        case "snapshot.limits":
            let store = try snapshot(step)
            if let age = double(step, "maximumAge") { store.maximumAge = age }
            if let bytes = int(step, "maximumBytes") { store.maximumBytes = bytes }
            if let perDocument = int(step, "maximumBytesPerDocument") { store.maximumBytesPerDocument = perDocument }
            return .object([
                ("maximumAge", .double(store.maximumAge)),
                ("maximumBytes", .int(store.maximumBytes)),
                ("maximumBytesPerDocument", .int(store.maximumBytesPerDocument)),
            ])
        case "snapshot.record":
            let store = try snapshot(step)
            let kind = try require(SnapshotStore.SnapshotKind(rawValue: string(step, "kind") ?? "external"), "kind")
            return record(store.record(textField(step, "text"), for: try path(step, "doc"), kind: kind))
        case "snapshot.wait":
            let store = try snapshot(step)
            sync { await store.waitForPendingWrites() }
            return .null
        case "snapshot.prune":
            let store = try snapshot(step)
            sync { await store.pruneOneGenerationForTesting() }
            return .null
        case "snapshot.versions":
            return .array(try snapshot(step).versions(for: try path(step, "doc")).map { record($0) })
        case "snapshot.content":
            let store = try snapshot(step)
            let hash: String
            if string(step, "text") != nil || string(step, "textHex") != nil {
                hash = SnapshotStore.hash(textField(step, "text"))
            } else if let explicit = string(step, "hash") {
                hash = explicit
            } else {
                let versions = store.versions(for: try path(step, "doc"))
                let index = int(step, "version") ?? 0
                guard index < versions.count else { return .null }
                hash = versions[index].hash
            }
            return .object([("content", content(store.content(forHash: hash))), ("text", .string(store.text(forHash: hash)))])
        case "snapshot.forget":
            try snapshot(step).forget(try path(step, "doc"))
            return .null
        case "snapshot.totalBytes":
            return .int(try snapshot(step).totalBytes())
        case "snapshot.inventory":
            let inventory = try snapshot(step).objectInventory().sorted { utf8Less($0.hash, $1.hash) }
            return .array(inventory.map { .object([("hash", .string($0.hash)), ("size", .int($0.size))]) })
        case "snapshot.hash":
            return .string(SnapshotStore.hash(string(step, "text") ?? ""))
        case "snapshot.documentKey":
            return .string(SnapshotStore.documentKey(for: try path(step, "doc")))

        // DocumentStateStore
        case "state.open":
            let support = try path(step, "support")
            stateStores[name(step)] = DocumentStateStore(supportDirectory: support)
            supportDirectories[name(step)] = support
            return .null
        case "state.writeFile":
            let support = try require(supportDirectories[name(step)], "state store")
            let file = support.appendingPathComponent("state", isDirectory: true)
                .appendingPathComponent(SnapshotStore.documentKey(for: try path(step, "doc")) + ".json")
            try writeText(expand(string(step, "text")), to: file)
            return .null
        case "state.get":
            return state(try stateStore(step).state(for: try path(step, "doc")))
        case "state.save":
            let store = try stateStore(step)
            let document = try path(step, "doc")
            var value = (string(step, "base") ?? "current") == "new" ? DocumentState(path: document.path) : store.state(for: document)
            try apply(step["set"] as? [String: Any] ?? [:], to: &value)
            store.save(value, for: document)
            return state(value)
        case "state.decode":
            do {
                return state(try JSONDecoder.snapshotDecoder.decode(DocumentState.self, from: Data(expand(string(step, "json")).utf8)))
            } catch {
                return errorCode(error)
            }
        case "state.recents":
            return .array(try stateStore(step).recents(limit: int(step, "limit") ?? 30).map(recent))
        case "state.noteOpened":
            try stateStore(step).noteOpened(try path(step, "doc"), document: MarkdownParser.parse(string(step, "text") ?? ""))
            return .null
        case "state.removeRecent":
            try stateStore(step).removeRecent(path: try path(step, "path").path)
            return .null
        case "state.clearRecents":
            try stateStore(step).clearRecents()
            return .null
        case "state.canonicalPath":
            return .string(DocumentStateStore.canonicalPath(try path(step, "path").path))

        // ScrollAnchoring
        case "anchor.make":
            return anchor(ScrollAnchoring.anchor(for: int(step, "offset") ?? 0, in: MarkdownParser.parse(string(step, "text") ?? "")))
        case "anchor.offset":
            return .int(ScrollAnchoring.offset(for: scrollAnchor(step["anchor"]), in: MarkdownParser.parse(string(step, "text") ?? "")))

        // ChangeTracker
        case "tracker.new":
            let tracker = ChangeTracker()
            if let lifetime = double(step, "lifetime") { tracker.lifetime = lifetime }
            if let dwell = double(step, "dwell") { tracker.dwell = dwell }
            trackers[name(step, "tracker")] = tracker
            return .null
        case "tracker.apply":
            let tracker = try changeTracker(step)
            let replacing = bool(step, "replacing") ?? true
            let old = string(step, "old") ?? ""
            let new = string(step, "new") ?? ""
            if let hunks = step["hunks"] as? [[String: Any]] {
                let parsed = try hunks.map { spec -> ChangeHunk in
                    ChangeHunk(
                        kind: try require(ChangeKind(rawValue: spec["kind"] as? String ?? ""), "kind"),
                        newRange: range(spec["new"]),
                        oldRange: range(spec["old"]),
                        wordRanges: (spec["words"] as? [Any] ?? []).map { range($0) }
                    )
                }
                tracker.apply(hunks: parsed, newText: new, oldText: old, replacingExisting: replacing)
            } else {
                tracker.apply(hunks: TextDiff.hunks(old: old, new: new), newText: new, oldText: old, replacingExisting: replacing)
            }
            return trackerSummary(tracker)
        case "tracker.marks":
            return trackerSummary(try changeTracker(step))
        case "tracker.adjust":
            let tracker = try changeTracker(step)
            tracker.adjust(forEditIn: range(step["range"]), delta: int(step, "delta") ?? 0)
            return trackerSummary(tracker)
        case "tracker.next":
            return mark(try changeTracker(step).next(after: int(step, "offset") ?? 0))
        case "tracker.previous":
            return mark(try changeTracker(step).previous(before: int(step, "offset") ?? 0))
        case "tracker.markAt":
            return mark(try changeTracker(step).mark(at: int(step, "offset") ?? 0))
        case "tracker.visit":
            let tracker = try changeTracker(step)
            let index = int(step, "index") ?? 0
            if index < tracker.marks.count { tracker.markVisited(tracker.marks[index].id) }
            return trackerSummary(tracker)
        case "tracker.noteVisible":
            let tracker = try changeTracker(step)
            tracker.noteVisibleRange(range(step["range"]), now: try require(parseDate(string(step, "now") ?? ""), "now"))
            return trackerSummary(tracker)
        case "tracker.restore":
            let tracker = try changeTracker(step)
            let now = try require(parseDate(string(step, "now") ?? ""), "now")
            tracker.restore(try persistedMarks(step), textLength: int(step, "textLength") ?? 0, now: now)
            return trackerSummary(tracker)
        case "tracker.merge":
            let tracker = try changeTracker(step)
            tracker.merge(persisted: try persistedMarks(step))
            return trackerSummary(tracker)
        case "tracker.dropExpired":
            let tracker = try changeTracker(step)
            let dropped = tracker.dropExpiredMarks(now: try require(parseDate(string(step, "now") ?? ""), "now"))
            return .object([("dropped", .bool(dropped)), ("tracker", trackerSummary(tracker))])
        case "tracker.clear":
            let tracker = try changeTracker(step)
            var reviewed = false
            tracker.onReviewed = { reviewed = true }
            tracker.clear()
            tracker.onReviewed = nil
            return .object([("reviewed", .bool(reviewed)), ("tracker", trackerSummary(tracker))])
        case "tracker.reset":
            let tracker = try changeTracker(step)
            tracker.reset()
            return trackerSummary(tracker)
        case "tracker.persisted":
            return .array(try changeTracker(step).persistedMarks.map(persisted))
        case "tracker.ranges":
            let tracker = try changeTracker(step)
            return .object([ChangeKind.inserted, .deleted, .modified].map { ($0.rawValue, ranges(tracker.ranges(of: $0))) })

        // Preferences.Values
        case "prefs.defaults":
            return try preferences(Preferences.Values(), step)
        case "prefs.decode":
            let values: Preferences.Values
            do {
                values = try JSONDecoder().decode(Preferences.Values.self, from: Data(expand(string(step, "json")).utf8))
            } catch {
                return errorCode(error)
            }
            return try preferences(values, step)

        // ReaderProfiles
        case "profiles.builtIns":
            return .array(ReaderProfile.builtIns.map(profile))
        case "profiles.make":
            return profile(try readerProfile(step))
        case "profiles.custom":
            if let id = string(step, "id") { return profile(ReaderProfile.custom(id: id, name: string(step, "name") ?? "")) }
            return profile(ReaderProfile.custom(name: string(step, "name") ?? ""))
        case "profiles.save":
            let specs = step["profiles"] as? [[String: Any]] ?? []
            JSONReaderProfileStore(url: try path(step, "path")).saveCustomProfiles(try specs.map { try readerProfile($0) })
            return .null
        case "profiles.load":
            return .array(JSONReaderProfileStore(url: try path(step, "path")).loadCustomProfiles().map(profile))

        // Review sidecars
        case "review.anchor":
            return reviewAnchor(ReviewAnchorResolver.makeAnchor(
                in: string(step, "text") ?? "", range: range(step["range"]), contextLength: int(step, "contextLength") ?? 48
            ))
        case "review.resolve":
            let resolution = ReviewAnchorResolver.resolve(
                try anchorSpec(step["anchor"]), in: string(step, "text") ?? "", contextLength: int(step, "contextLength") ?? 48
            )
            return .object([("status", .string(resolution.status.rawValue)), ("range", .range(resolution.range))])
        case "review.fingerprint":
            return .string(ReviewAnchorResolver.fingerprint(string(step, "text") ?? ""))
        case "review.make":
            let item = ReviewSidecarEngine.makeReview(
                kind: try require(ReviewKind(rawValue: string(step, "kind") ?? ""), "kind"),
                in: string(step, "text") ?? "",
                range: range(step["range"]),
                body: string(step, "body") ?? "",
                replacement: string(step, "replacement")
            )
            if let item, let key = string(step, "name") { reviews[key] = item }
            return review(item)
        case "review.item":
            let kind = try require(ReviewKind(rawValue: string(step, "kind") ?? ""), "kind")
            let reviewState = try require(ReviewState(rawValue: string(step, "state") ?? "open"), "state")
            let anchor = try anchorSpec(step["anchor"])
            let item: ReviewItem
            if let id = string(step, "id") {
                item = ReviewItem(
                    id: try require(UUID(uuidString: id), "id"), kind: kind, anchor: anchor,
                    body: string(step, "body") ?? "", replacement: string(step, "replacement"), state: reviewState
                )
            } else {
                item = ReviewItem(kind: kind, anchor: anchor, body: string(step, "body") ?? "", replacement: string(step, "replacement"), state: reviewState)
            }
            reviews[try require(string(step, "name"), "name")] = item
            return review(item)
        case "review.setState":
            let key = try require(string(step, "review"), "review")
            var item = try require(reviews[key], "review \(key)")
            item.state = try require(ReviewState(rawValue: string(step, "state") ?? ""), "state")
            reviews[key] = item
            return review(item)
        case "review.apply":
            let item = try require(reviews[try require(string(step, "review"), "review")], "review")
            switch ReviewSidecarEngine.applySuggestion(item, to: string(step, "text") ?? "") {
            case .applied(let edit):
                return .object([("applied", .object([
                    ("range", .range(edit.range)), ("replacement", .string(edit.replacement)), ("summary", .string(edit.summary)),
                ]))])
            case .stale(let status):
                return .object([("stale", .string(status.rawValue))])
            }
        case "review.save":
            let names = step["reviews"] as? [String] ?? []
            let sidecar = ReviewSidecar(version: int(step, "version") ?? 1, reviews: try names.map { try require(reviews[$0], "review \($0)") })
            do {
                try LocalReviewSidecarStore().save(sidecar, for: try path(step, "doc"))
                return .string("saved")
            } catch {
                return errorCode(error)
            }
        case "review.load":
            do {
                let sidecar = try LocalReviewSidecarStore().load(for: try path(step, "doc"))
                return .object([("version", .int(sidecar.version)), ("reviews", .array(sidecar.reviews.map { review($0) }))])
            } catch {
                return errorCode(error)
            }
        case "review.sidecarURL":
            return .string(LocalReviewSidecarStore.sidecarURL(for: try path(step, "doc")).path)
        case "review.critic":
            return ranges(ReviewSidecarEngine.criticMarkupRanges(in: string(step, "text") ?? ""))

        // Trust
        case "trust.open":
            let key = name(step)
            if key == "shared" {
                guard hasSupportDirectory else { throw AppOracleError(description: "formats: TrustStore.shared needs \"supportDirectory\"") }
                trustStores[key] = TrustStore.shared
            } else {
                let grants = try (step["grants"] as? [[String: Any]] ?? []).map { try grantSpec($0) }
                trustStores[key] = TrustStore(persistence: InMemoryTrustStorePersistence(grants))
            }
            return .array(try trustStore(step).grants().map(grant))
        case "trust.grant":
            let store = try trustStore(step)
            let effects = try (step["effects"] as? [String] ?? []).map { try require(TrustEffect(rawValue: $0), "effect") }
            let granted = store.grant(
                scope: try require(TrustScope(rawValue: string(step, "scope") ?? ""), "scope"),
                path: try path(step, "path"),
                effects: Set(effects),
                externalURL: string(step, "externalURL")
            )
            return .object([("granted", .bool(granted)), ("grants", .array(store.grants().map(grant)))])
        case "trust.revoke":
            let store = try trustStore(step)
            let revoked = store.revoke(scope: try require(TrustScope(rawValue: string(step, "scope") ?? ""), "scope"), path: try path(step, "path"))
            return .object([("revoked", .bool(revoked)), ("grants", .array(store.grants().map(grant)))])
        case "trust.state":
            return .string(try trustStore(step).state(for: string(step, "doc").map(url)).rawValue)
        case "trust.decide":
            let request = TrustRequest(
                effect: try require(TrustEffect(rawValue: string(step, "effect") ?? ""), "effect"),
                target: TrustTarget(
                    displayName: string(step, "displayName") ?? "",
                    canonicalPath: string(step, "canonicalPath").map { url($0).path },
                    externalURL: string(step, "externalURL")
                ),
                documentURL: string(step, "doc").map(url)
            )
            let trustState = try require(DocumentTrustState(rawValue: string(step, "state") ?? "standard"), "state")
            let policy: DocumentTrust
            if let grants = step["grants"] as? [[String: Any]] {
                policy = DocumentTrust(state: trustState, grants: try grants.map { try grantSpec($0) })
            } else {
                policy = try trustStore(step).policy(state: trustState)
            }
            return .object([("documentPath", .string(request.documentPath)), ("decision", .string(policy.decision(for: request).rawValue))])
        case "trust.canonical":
            return .string(DocumentTrust.canonicalFilePath(try path(step, "path"))?.path)
        case "trust.isWithin":
            return .bool(DocumentTrust.isWithin(try path(step, "child"), try path(step, "root")))
        case "trust.folderScope":
            return .string(DocumentTrust.folderScope(for: try path(step, "path"), isDirectory: bool(step, "isDirectory") ?? false).path)
        case "trust.decodeGrants":
            do {
                return .array(try JSONDecoder().decode([TrustGrant].self, from: Data(expand(string(step, "json")).utf8)).map(grant))
            } catch {
                return errorCode(error)
            }
        case "trust.titles":
            return .object([
                ("states", .array(DocumentTrustState.allCases.map { .array([.string($0.rawValue), .string($0.title)]) })),
                ("effects", .array(TrustEffect.allCases.map { .array([.string($0.rawValue), .string($0.title)]) })),
                ("scopes", .array(TrustScope.allCases.map { .string($0.rawValue) })),
            ])

        // PathResolver and ExternalEditor
        case "resolver.open":
            resolvers[name(step, "resolver")] = PathResolver(documentURL: string(step, "doc").map(url))
            return .null
        case "resolver.resolve":
            let resolver = try require(resolvers[name(step, "resolver")], "resolver")
            let resolution = resolver.resolve(PathToken(rawPath: expand(string(step, "raw")), line: int(step, "line")))
            return .object([
                ("url", .string(resolution.url?.path)),
                ("urlIsDirectory", resolution.url.map { .bool($0.hasDirectoryPath) } ?? .null),
                ("exists", .bool(resolution.exists)),
                ("isDirectory", .bool(resolution.isDirectory)),
                ("line", .int(resolution.line)),
            ])
        case "resolver.invalidate":
            try require(resolvers[name(step, "resolver")], "resolver").invalidate()
            return .null
        case "resolver.gitRoot":
            return .string(PathResolver.findGitRoot(from: try path(step, "path"))?.path)
        case "editor.url":
            let editor = try require(ExternalEditor(rawValue: string(step, "editor") ?? ""), "editor")
            return .string(editor.url(for: try path(step, "path"), line: int(step, "line"))?.absoluteString)
        case "editor.all":
            return .array(ExternalEditor.allCases.map {
                .object([("raw", .string($0.rawValue)), ("title", .string($0.title)), ("bundleIdentifier", .string($0.bundleIdentifier))])
            })

        // JumpHistory
        case "jump.record":
            let history = jumpHistory(step)
            history.record(from: try entrySpec(step["from"]), to: try require(try entrySpec(step["to"]), "to"))
            return jumpState(history)
        case "jump.back":
            let history = jumpHistory(step)
            let result = history.goBack()
            return .object([("entry", entry(result)), ("state", jumpState(history))])
        case "jump.forward":
            let history = jumpHistory(step)
            let result = history.goForward()
            return .object([("entry", entry(result)), ("state", jumpState(history))])
        case "jump.clear":
            let history = jumpHistory(step)
            history.clear()
            return jumpState(history)

        default:
            throw AppOracleError(description: "formats: unknown op \(op)")
        }
    }

    // MARK: - Step helpers

    func snapshot(_ step: [String: Any]) throws -> SnapshotStore {
        try require(snapshotStores[name(step)], "snapshot store \(name(step))")
    }

    func stateStore(_ step: [String: Any]) throws -> DocumentStateStore {
        try require(stateStores[name(step)], "state store \(name(step))")
    }

    func changeTracker(_ step: [String: Any]) throws -> ChangeTracker {
        try require(trackers[name(step, "tracker")], "tracker")
    }

    func trustStore(_ step: [String: Any]) throws -> TrustStore {
        try require(trustStores[name(step)], "trust store \(name(step))")
    }

    func jumpHistory(_ step: [String: Any]) -> JumpHistory {
        let key = name(step, "history")
        if let history = histories[key] { return history }
        let history = JumpHistory()
        histories[key] = history
        return history
    }

    func jumpState(_ history: JumpHistory) -> JSON {
        .object([
            ("entries", .array(history.entries.map { entry($0) })),
            ("canGoBack", .bool(history.canGoBack)),
            ("canGoForward", .bool(history.canGoForward)),
            ("current", entry(history.current)),
        ])
    }

    func entrySpec(_ value: Any?) throws -> JumpHistory.Entry? {
        guard let spec = value as? [String: Any] else { return nil }
        return JumpHistory.Entry(
            url: (spec["url"] as? String).map(url),
            offset: (spec["offset"] as? NSNumber)?.intValue ?? 0,
            label: spec["label"] as? String ?? ""
        )
    }

    func trackerSummary(_ tracker: ChangeTracker) -> JSON {
        .object([
            ("count", .int(tracker.count)),
            ("unread", .int(tracker.unreadCount)),
            ("decorated", .int(tracker.decoratedMarks.count)),
            ("marks", .array(tracker.marks.map { mark($0) })),
        ])
    }

    func persistedMarks(_ step: [String: Any]) throws -> [ChangeTracker.PersistedMark] {
        if let source = string(step, "from") {
            return try require(trackers[source], "tracker \(source)").persistedMarks
        }
        if let storeName = string(step, "fromState") {
            return try require(stateStores[storeName], "state store").state(for: try path(step, "doc")).marks
        }
        return try (step["marks"] as? [[String: Any]] ?? []).map { spec in
            ChangeTracker.PersistedMark(
                id: try require(UUID(uuidString: spec["id"] as? String ?? ""), "mark id"),
                kind: spec["kind"] as? String ?? "",
                range: ChangeTracker.PersistedRange(range(spec["range"])),
                wordRanges: (spec["wordRanges"] as? [Any] ?? []).map { ChangeTracker.PersistedRange(range($0)) },
                deletedText: spec["deletedText"] as? String ?? "",
                created: try require(parseDate(spec["created"] as? String ?? ""), "created"),
                visited: (spec["visited"] as? NSNumber)?.boolValue ?? false
            )
        }
    }

    func apply(_ fields: [String: Any], to state: inout DocumentState) throws {
        for (key, value) in fields.sorted(by: { utf8Less($0.key, $1.key) }) {
            switch key {
            case "lastSeenHash": state.lastSeenHash = value as? String ?? ""
            case "reviewBaselineHash": state.reviewBaselineHash = value as? String ?? ""
            case "anchor": state.anchor = scrollAnchor(value)
            case "mode": state.mode = try require(RenderMode(rawValue: value as? String ?? ""), "mode")
            case "zoomLevel": state.zoomLevel = try require(ZoomLevel(rawValue: (value as? NSNumber)?.intValue ?? 0), "zoomLevel")
            case "foldedHeadings": state.foldedHeadings = Set(value as? [String] ?? [])
            case "expandedCodeBlocks": state.expandedCodeBlocks = Set((value as? [NSNumber] ?? []).map(\.intValue))
            case "collapsedCodeBlocks": state.collapsedCodeBlocks = Set((value as? [NSNumber] ?? []).map(\.intValue))
            case "lastOpened": state.lastOpened = try require(parseDate(value as? String ?? ""), "lastOpened")
            case "sidebarVisible": state.sidebarVisible = (value as? NSNumber)?.boolValue ?? false
            case "selectionLocation": state.selectionLocation = (value as? NSNumber)?.intValue ?? 0
            case "selectionLength": state.selectionLength = (value as? NSNumber)?.intValue ?? 0
            case "splitViewEnabled": state.splitViewEnabled = (value as? NSNumber)?.boolValue ?? false
            case "marksFromTracker": state.marks = try require(trackers[value as? String ?? ""], "tracker").persistedMarks
            default: throw AppOracleError(description: "formats: unknown state field \(key)")
            }
        }
    }

    func preferences(_ initial: Preferences.Values, _ step: [String: Any]) throws -> JSON {
        var values = initial
        if let theme = step["selectTheme"] as? [String: Any] {
            values.selectTheme(named: theme["name"] as? String ?? "", for: (theme["slot"] as? String) == "dark" ? .dark : .light)
        }
        // `persist()`'s encoder.
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        let data = try encoder.encode(values)
        if let write = string(step, "write") {
            let target = url(write)
            try FileManager.default.createDirectory(at: target.deletingLastPathComponent(), withIntermediateDirectories: true)
            try data.write(to: target, options: .atomic)
        }
        // `effectiveTypography` and `largeFileThresholdBytes`, on these values.
        var effective = values.typography
        effective.bodySize = max(10, min(28, effective.bodySize + values.textSizeAdjustment))
        let roundTrip = try JSONDecoder().decode(Preferences.Values.self, from: data)
        return .object([
            ("encoded", lines(data)),
            ("effectiveTypography", typography(effective)),
            ("largeFileThresholdBytes", .int(values.largeFileThresholdMegabytes * 1024 * 1024)),
            ("roundTripEqual", .bool(roundTrip == values)),
        ])
    }

    func readerProfile(_ step: [String: Any]) throws -> ReaderProfile {
        ReaderProfile(
            id: step["id"] as? String ?? "",
            name: step["name"] as? String ?? "",
            isBuiltIn: (step["isBuiltIn"] as? NSNumber)?.boolValue ?? false,
            typographyScale: try require(ReaderTypographyScale(rawValue: step["typographyScale"] as? String ?? "standard"), "typographyScale"),
            measureCharacters: CGFloat((step["measureCharacters"] as? NSNumber)?.doubleValue ?? 70),
            chromeDensity: try require(ReaderChromeDensity(rawValue: step["chromeDensity"] as? String ?? "comfortable"), "chromeDensity"),
            motionPreference: try require(ReaderMotionPreference(rawValue: step["motionPreference"] as? String ?? "follow-system"), "motionPreference")
        )
    }

    func anchorSpec(_ value: Any?) throws -> ReviewAnchor {
        let spec = try require(value as? [String: Any], "anchor")
        if let text = spec["text"] as? String {
            return try require(ReviewAnchorResolver.makeAnchor(in: text, range: range(spec["range"])), "anchor")
        }
        return ReviewAnchor(
            range: range(spec["range"]),
            selectedText: spec["selectedText"] as? String ?? "",
            beforeFingerprint: spec["beforeFingerprint"] as? String ?? "",
            afterFingerprint: spec["afterFingerprint"] as? String ?? ""
        )
    }

    func grantSpec(_ spec: [String: Any]) throws -> TrustGrant {
        TrustGrant(
            scope: try require(TrustScope(rawValue: spec["scope"] as? String ?? ""), "scope"),
            canonicalPath: url(spec["path"] as? String ?? "").path,
            effects: Set(try (spec["effects"] as? [String] ?? []).map { try require(TrustEffect(rawValue: $0), "effect") }),
            externalURL: spec["externalURL"] as? String
        )
    }

    // MARK: - Files

    func dumpFiles() -> [JSON] {
        var relatives: [String] = []
        func walk(_ relative: String) {
            let directory = relative.isEmpty ? rootPath : rootPath + "/" + relative
            for name in (try? FileManager.default.contentsOfDirectory(atPath: directory)) ?? [] {
                let child = relative.isEmpty ? name : relative + "/" + name
                relatives.append(child)
                var info = stat()
                if lstat(rootPath + "/" + child, &info) == 0, (info.st_mode & S_IFMT) == S_IFDIR { walk(child) }
            }
        }
        walk("")
        // Sorted as the dump will spell them: a document key depends on the
        // sandbox path, so the raw names sort differently from run to run.
        let labels = keyLabels()
        func spelled(_ relative: String) -> String {
            labels.reduce(relative) { $0.replacingOccurrences(of: $1.0, with: $1.1) }
        }
        var out: [JSON] = []
        for relative in relatives.sorted(by: { utf8Less(spelled($0), spelled($1)) }) {
            let full = rootPath + "/" + relative
            var info = stat()
            guard lstat(full, &info) == 0 else { continue }
            switch info.st_mode & S_IFMT {
            case S_IFDIR:
                out.append(.object([("path", .string(relative)), ("kind", .string("directory"))]))
            case S_IFLNK:
                let destination = (try? FileManager.default.destinationOfSymbolicLink(atPath: full)) ?? ""
                out.append(.object([("path", .string(relative)), ("kind", .string("symlink")), ("target", .string(destination))]))
            default:
                let data = FileManager.default.contents(atPath: full) ?? Data()
                out.append(.object([("path", .string(relative)), ("kind", .string("file")), ("content", fileContent(relative, data))]))
            }
        }
        return out
    }

    func isParsed(_ relative: String) -> Bool {
        if parsedPaths.contains(relative) { return true }
        let components = relative.components(separatedBy: "/")
        if components.last == "recents.json" { return true }
        guard relative.hasSuffix(".json"), components.count >= 2 else { return false }
        let parent = components[components.count - 2]
        if parent == "state" { return true }
        return parent == "index" && components.count >= 3 && components[components.count - 3] == "history"
    }

    func fileContent(_ relative: String, _ data: Data) -> JSON {
        let components = relative.components(separatedBy: "/")
        if let objects = components.firstIndex(of: "objects"), objects > 0, components[objects - 1] == "history" {
            return hex(data)
        }
        if isParsed(relative), let object = try? JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed]),
           let text = try? JSONSerialization.data(withJSONObject: normalizeParsed(object, key: nil), options: [.prettyPrinted, .sortedKeys, .fragmentsAllowed, .withoutEscapingSlashes]),
           let string = String(data: text, encoding: .utf8) {
            return .object([("json", .array(string.components(separatedBy: "\n").map { .string($0) }))])
        }
        if components.last == "trust.json", !data.starts(with: [0xEF, 0xBB, 0xBF]), let text = String(data: data, encoding: .utf8) {
            return .object([("lines", .array(sortEffectBlocks(text.components(separatedBy: "\n")).map { .string($0) }))])
        }
        return lines(data)
    }

    static let timestampKeys: Set<String> = ["date", "created", "lastOpened", "firstSeen"]
    static let setKeys: Set<String> = ["foldedHeadings", "expandedCodeBlocks", "collapsedCodeBlocks"]

    func normalizeParsed(_ value: Any, key: String?) -> Any {
        if let dictionary = value as? [String: Any] {
            var out: [String: Any] = [:]
            for (member, element) in dictionary { out[member] = normalizeParsed(element, key: member) }
            return out
        }
        if let array = value as? [Any] {
            let elements = array.map { normalizeParsed($0, key: nil) }
            guard let key, Self.setKeys.contains(key) else { return elements }
            return elements.sorted { a, b in
                switch (a, b) {
                case (let x as String, let y as String): return utf8Less(x, y)
                case (let x as NSNumber, let y as NSNumber): return x.doubleValue < y.doubleValue
                default: return false
                }
            }
        }
        if let key, Self.timestampKeys.contains(key), let text = value as? String, let date = parseDate(text),
           abs(date.timeIntervalSince(runStart)) < 2 * 86_400 {
            return "<now>"
        }
        return value
    }

    /// Sorts the element lines of each `"effects" : [` block, re-placing the
    /// commas: Swift writes a `Set` in per-process hash order.
    func sortEffectBlocks(_ lines: [String]) -> [String] {
        var out: [String] = []
        var index = 0
        while index < lines.count {
            let line = lines[index]
            out.append(line)
            index += 1
            guard line.hasSuffix("\"effects\" : [") else { continue }
            var block: [String] = []
            while index < lines.count, !lines[index].trimmingCharacters(in: .whitespaces).hasPrefix("]") {
                block.append(lines[index].hasSuffix(",") ? String(lines[index].dropLast()) : lines[index])
                index += 1
            }
            block.sort(by: utf8Less)
            for (position, element) in block.enumerated() {
                out.append(position + 1 < block.count ? element + "," : element)
            }
        }
        return out
    }

    // MARK: - Normalisation

    /// Each document key the script can have produced, with its label.
    func keyLabels() -> [(String, String)] {
        mentionedPaths.sorted(by: utf8Less).map { (SnapshotStore.documentKey(for: url($0)), "<key:\($0)>") }
    }

    func normalize(_ dump: JSON) -> JSON {
        let keys = keyLabels()
        // The root as paths spell it, and as `JSONEncoder` escapes it.
        let rootSpellings = [rootPath, unresolvedRootPath].flatMap { [$0, $0.replacingOccurrences(of: "/", with: "\\/")] }
        var uuids: [String: String] = [:]
        func rewrite(_ text: String) -> String {
            var text = text
            for (key, label) in keys where text.contains(key) {
                text = text.replacingOccurrences(of: key, with: label)
            }
            for spelling in rootSpellings where text.contains(spelling) {
                text = text.replacingOccurrences(of: spelling, with: "<root>")
            }
            return replaceUUIDs(text, &uuids)
        }
        func walk(_ value: JSON) -> JSON {
            switch value {
            case .string(let text): return .string(rewrite(text))
            case .array(let values): return .array(values.map(walk))
            case .object(let pairs): return .object(pairs.map { ($0.0, walk($0.1)) })
            default: return value
            }
        }
        return walk(dump)
    }

    /// Replaces each upper-case `UUID.uuidString` with `<uuid-N>`, numbered by
    /// first appearance.
    func replaceUUIDs(_ text: String, _ table: inout [String: String]) -> String {
        let bytes = Array(text.utf8)
        guard bytes.count >= 36 else { return text }
        func isHex(_ byte: UInt8) -> Bool { (byte >= 0x30 && byte <= 0x39) || (byte >= 0x41 && byte <= 0x46) }
        var out: [UInt8] = []
        var index = 0
        while index < bytes.count {
            if index + 36 <= bytes.count {
                var matches = true
                for offset in 0..<36 {
                    let byte = bytes[index + offset]
                    if offset == 8 || offset == 13 || offset == 18 || offset == 23 {
                        if byte != 0x2D { matches = false; break }
                    } else if !isHex(byte) {
                        matches = false; break
                    }
                }
                if matches {
                    let uuid = String(decoding: bytes[index..<index + 36], as: UTF8.self)
                    let label = table[uuid] ?? "<uuid-\(table.count + 1)>"
                    table[uuid] = label
                    out.append(contentsOf: Array(label.utf8))
                    index += 36
                    continue
                }
            }
            out.append(bytes[index])
            index += 1
        }
        return String(decoding: out, as: UTF8.self)
    }
}
