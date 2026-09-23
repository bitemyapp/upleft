import Foundation
@testable import DownrightApp

/// Swift side of the `workspace` suite: `WorkspaceIndex`, the link graph,
/// unlinked mentions and `WorkspaceSearch` over the committed sample
/// workspace.
///
///   downright-app-oracle workspace <queries.json> <out.json>
///
/// The input names the workspace root (relative to the query file), an
/// optional policy and the searches:
///
///     {"root": "../workspace",
///      "policy": {"markdownExtensions": [...], "ignoredDirectoryNames": [...],
///                 "ignoresHiddenDirectories": true, "maximumFiles": 10000,
///                 "maximumBytesPerFile": …, "maximumTotalBytes": …,
///                 "readConcurrency": 4},
///      "searches": [{"text": "…", "isRegex": false, "caseSensitive": false,
///                    "wholeWord": false, "limitPerFile": 100}, …]}
///
/// The index runs as the app runs it: `WorkspaceIndex(policy:)` with
/// Foundation's enumerator and reader, `reroot(to:)` on the main actor, and
/// the main run loop turning until the scan's snapshot is published. Every
/// `onUpdate` is recorded (revision, entry count). The process first moves to
/// `/`, a launched app's working directory, because the link graph resolves
/// stems through `URL(fileURLWithPath:)` of relative paths.
///
/// Paths in the dump are relative to the standardized root: entry ids, URLs,
/// graph keys and search file ids. Graph dictionaries are listed in snapshot
/// order (their keys are entry ids), so no dictionary order leaks in. A scan
/// with `maximumTotalBytes` and more than one reader thread skips a
/// scheduling-dependent file, so query files that set that budget also set
/// `readConcurrency` to 1.
///
/// `upleft-oracle workspace` (crates/conformance/src/dump/workspace.rs)
/// writes the same.
enum WorkspaceDump {
    static func policy(_ object: [String: Any]?) -> WorkspaceIndexPolicy {
        let defaults = WorkspaceIndexPolicy()
        guard let object else { return defaults }
        return WorkspaceIndexPolicy(
            markdownExtensions: (object["markdownExtensions"] as? [String]).map(Set.init) ?? defaults.markdownExtensions,
            ignoredDirectoryNames: (object["ignoredDirectoryNames"] as? [String]).map(Set.init) ?? defaults.ignoredDirectoryNames,
            ignoresHiddenDirectories: object["ignoresHiddenDirectories"] as? Bool ?? defaults.ignoresHiddenDirectories,
            maximumFiles: object["maximumFiles"] as? Int ?? defaults.maximumFiles,
            maximumBytesPerFile: (object["maximumBytesPerFile"] as? Int).map(Int64.init) ?? defaults.maximumBytesPerFile,
            maximumTotalBytes: (object["maximumTotalBytes"] as? Int).map(Int64.init) ?? defaults.maximumTotalBytes,
            readConcurrency: object["readConcurrency"] as? Int ?? defaults.readConcurrency
        )
    }

    static func query(_ object: [String: Any]) -> WorkspaceSearchQuery {
        WorkspaceSearchQuery(
            text: object["text"] as? String ?? "",
            isRegex: object["isRegex"] as? Bool ?? false,
            caseSensitive: object["caseSensitive"] as? Bool ?? false,
            wholeWord: object["wholeWord"] as? Bool ?? false
        )
    }

    /// Runs `WorkspaceIndex` on the main actor until it publishes a
    /// non-empty revision; returns every snapshot it published.
    @MainActor
    static func index(root: URL, policy: WorkspaceIndexPolicy) throws -> [WorkspaceIndexSnapshot] {
        let index = WorkspaceIndex(policy: policy)
        var updates: [WorkspaceIndexSnapshot] = []
        index.onUpdate = { updates.append($0) }
        index.reroot(to: root)
        let deadline = Date().addingTimeInterval(120)
        while updates.count < 2 && Date() < deadline {
            _ = RunLoop.main.run(mode: .default, before: Date().addingTimeInterval(0.01))
        }
        guard updates.count == 2 else { throw AppOracleError(description: "the index never published a scan") }
        return updates
    }

    static func run(input: URL, flags: [String]) throws -> JSON {
        let data = try Data(contentsOf: input)
        guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let rootPath = object["root"] as? String
        else { throw AppOracleError(description: "\(input.path): expected {root, …}") }
        let root = input.deletingLastPathComponent().appendingPathComponent(rootPath).standardizedFileURL
        let searches = object["searches"] as? [[String: Any]] ?? []
        let policy = policy(object["policy"] as? [String: Any])
        guard FileManager.default.changeCurrentDirectoryPath("/") else {
            throw AppOracleError(description: "cannot move to /")
        }
        let updates = try MainActor.assumeIsolated { try index(root: root, policy: policy) }
        let snapshot = updates[1]

        let rootPrefix = snapshot.rootURL.path.hasSuffix("/") ? snapshot.rootURL.path : snapshot.rootURL.path + "/"
        func relative(_ path: String) -> JSON {
            guard path.utf8.starts(with: rootPrefix.utf8) else { return .string(path) }
            return .string(String(decoding: path.utf8.dropFirst(rootPrefix.utf8.count), as: UTF8.self))
        }
        func range(_ range: NSRange) -> JSON { .range(range) }

        let entries: [JSON] = snapshot.entries.map { entry in
            .object([
                ("id", relative(entry.id)),
                ("url", relative(entry.url.path)),
                ("relativePath", .string(entry.relativePath)),
                ("text", .string(entry.text)),
                ("byteCount", .int(Int(entry.byteCount))),
                ("headings", .array(entry.headings.map {
                    .object([("title", .string($0.title)), ("range", range($0.range)), ("level", .int($0.level))])
                })),
                ("frontMatter", .array(entry.frontMatter.map {
                    .object([("key", .string($0.key)), ("value", .string($0.value)), ("range", range($0.range))])
                })),
                ("links", .array(entry.links.map {
                    .object([
                        ("destination", .string($0.destination)), ("range", range($0.range)),
                        ("kind", .string($0.kind.rawValue)),
                    ])
                })),
            ])
        }

        func target(_ link: WorkspaceLinkTarget) -> JSON {
            .object([
                ("sourceFile", relative(link.sourceFile)), ("sourceRange", range(link.sourceRange)),
                ("destination", .string(link.destination)), ("targetFile", link.targetFile.map(relative) ?? .null),
            ])
        }
        func mentions(_ mentions: [String: [WorkspaceSearchMention]]) -> JSON {
            .array(snapshot.entries.compactMap { entry in
                mentions[entry.id].map { list in
                    .object([
                        ("target", relative(entry.id)),
                        ("mentions", .array(list.map {
                            .object([
                                ("fileID", relative($0.fileID)), ("range", range($0.range)),
                                ("target", .string($0.target)),
                            ])
                        })),
                    ])
                }
            })
        }

        let graph = WorkspaceLinkGraphBuilder.build(snapshot: snapshot, includeUnlinkedMentions: true)
        let graphJSON: JSON = .object([
            ("outgoing", .array(snapshot.entries.compactMap { entry in
                graph.outgoing[entry.id].map { .object([("source", relative(entry.id)), ("links", .array($0.map(target)))]) }
            })),
            ("backlinks", .array(snapshot.entries.compactMap { entry in
                graph.backlinks[entry.id].map { list in
                    .object([
                        ("target", relative(entry.id)),
                        ("links", .array(list.map {
                            .object([
                                ("sourceFile", relative($0.sourceFile)), ("sourceRange", range($0.sourceRange)),
                                ("targetFile", relative($0.targetFile)), ("destination", .string($0.destination)),
                            ])
                        })),
                        ("linksTo", .int(graph.linksTo(fileID: entry.id).count)),
                    ])
                }
            })),
            ("unresolved", .array(graph.unresolved.map(target))),
            ("unlinkedMentions", mentions(graph.unlinkedMentions)),
            ("unlinkedMentionsGivenGraph", mentions(WorkspaceSearch.unlinkedMentions(snapshot: snapshot, graph: graph))),
        ])

        let searchJSON: [JSON] = searches.map { object in
            let query = query(object)
            let results = WorkspaceSearch.search(query, in: snapshot, limitPerFile: object["limitPerFile"] as? Int ?? 100)
            return .object([
                ("isValid", .bool(WorkspaceSearch.isValid(query))),
                ("results", .array(results.map {
                    .object([
                        ("fileID", relative($0.fileID)), ("url", relative($0.url.path)),
                        ("relativePath", .string($0.relativePath)), ("range", range($0.range)),
                        ("contextRange", range($0.contextRange)), ("contextText", .string($0.contextText)),
                        ("line", .int($0.line)), ("heading", .string($0.heading)),
                    ])
                })),
            ])
        }

        return .object([
            ("updates", .array(updates.map {
                .object([
                    ("revision", .int($0.revision)), ("entries", .int($0.entries.count)),
                    ("skippedFiles", .int($0.skippedFiles)), ("root", relative($0.rootURL.path + "/")),
                ])
            })),
            ("skippedFiles", .int(snapshot.skippedFiles)),
            ("entries", .array(entries)),
            ("graph", graphJSON),
            ("searches", .array(searchJSON)),
        ])
    }
}
