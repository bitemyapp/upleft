import Foundation
@testable import DownrightApp
import MarkdownCore

/// Swift side of the `find` suite: `FindEngine`, `FindSession` and
/// `SiblingSearch` over fixed query sets.
///
///   downright-app-oracle find <queries.json> <out.json>
///
/// The input names its documents (paths relative to the query file) and its
/// queries:
///
///     {"documents": ["../generated/agent/agent-5000.md", …],
///      "queries": [{"text": "…", "isRegex": false, "caseSensitive": false,
///                   "wholeWord": false, "scope": [location, length],
///                   "template": "$1", "caret": 0}, …],
///      "siblings": true}
///
/// Every document is read with `DocumentIO.read`, as the app reads it. For
/// each document and query the dump holds `isValid`, every match range,
/// `replaceAllEdits` and `replacement(for:)` of the first three matches (when
/// the query has a template), and a `FindSession` walk: `update` at the
/// caret, `statusText`, `currentMatch`, two steps forward and one back, a
/// `replacementEdit` and a `clear`. With `"siblings": true`, each query also
/// runs `SiblingSearch.search` over all the documents; hits name their
/// document by index. Ranges are UTF-16 `[location, length]`.
///
/// `upleft-oracle find` (crates/conformance/src/dump/find.rs) writes the same.
enum FindDump {
    static func query(_ object: [String: Any]) -> FindQuery {
        var query = FindQuery()
        query.text = object["text"] as? String ?? ""
        query.isRegex = object["isRegex"] as? Bool ?? false
        query.caseSensitive = object["caseSensitive"] as? Bool ?? false
        query.wholeWord = object["wholeWord"] as? Bool ?? false
        if let scope = object["scope"] as? [Int], scope.count == 2 {
            query.scope = NSRange(location: scope[0], length: scope[1])
        }
        return query
    }

    static func edit(_ edit: TextEdit?) -> JSON {
        guard let edit else { return .null }
        return .object([("range", .range(edit.range)), ("replacement", .string(edit.replacement))])
    }

    static func run(input: URL, flags: [String]) throws -> JSON {
        let data = try Data(contentsOf: input)
        guard let root = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let documentPaths = root["documents"] as? [String],
              let queryObjects = root["queries"] as? [[String: Any]]
        else { throw AppOracleError(description: "\(input.path): expected {documents, queries}") }
        let directory = input.deletingLastPathComponent()
        let urls = documentPaths.map { directory.appendingPathComponent($0).standardizedFileURL }
        let texts = try urls.map { try DocumentIO.read(contentsOf: $0).text }

        var documents: [JSON] = []
        for (documentIndex, text) in texts.enumerated() {
            var results: [JSON] = []
            for object in queryObjects {
                let query = query(object)
                let template = object["template"] as? String
                let caret = object["caret"] as? Int ?? 0
                let matches = FindEngine.matches(in: text, query: query)
                var fields: [(String, JSON)] = [
                    ("isValid", .bool(FindEngine.isValid(query))),
                    ("matches", .array(matches.map { JSON.range($0) })),
                ]
                if let template {
                    fields.append(("replaceAll", .array(
                        FindEngine.replaceAllEdits(in: text, query: query, template: template).map { edit($0) }
                    )))
                    fields.append(("replacement", .array(matches.prefix(3).map {
                        .string(FindEngine.replacement(for: $0, in: text, query: query, template: template))
                    })))
                }
                let session = FindSession()
                session.update(query: query, in: text, caret: caret)
                var walk: [JSON] = [.object([
                    ("status", .string(session.statusText)),
                    ("index", .int(session.currentIndex)),
                    ("current", .range(session.currentMatch)),
                    ("count", .int(session.count)),
                ])]
                for forward in [true, true, false] {
                    let next = session.advance(forward: forward)
                    walk.append(.object([
                        ("advanced", .range(next)),
                        ("status", .string(session.statusText)),
                        ("index", .int(session.currentIndex)),
                    ]))
                }
                let replacement = session.replacementEdit(in: text, template: template ?? "X", caret: caret)
                session.clear()
                fields.append(("session", .object([
                    ("walk", .array(walk)),
                    ("replacementEdit", edit(replacement)),
                    ("clearedStatus", .string(session.statusText)),
                    ("clearedCount", .int(session.count)),
                ])))
                results.append(.object(fields))
            }
            documents.append(.object([("document", .int(documentIndex)), ("queries", .array(results))]))
        }

        var output: [(String, JSON)] = [
            ("documents", .array(documentPaths.map { JSON.string($0) })),
            ("results", .array(documents)),
        ]
        if root["siblings"] as? Bool == true {
            output.append(("siblings", .array(queryObjects.map { object in
                .array(SiblingSearch.search(query(object), in: urls).map { hit in
                    .object([
                        ("document", .int(urls.firstIndex(of: hit.url))),
                        ("displayName", .string(hit.displayName)),
                        ("range", .range(hit.range)),
                        ("contextRange", .range(hit.contextRange)),
                        ("contextText", .string(hit.contextText)),
                        ("headingTitle", .string(hit.headingTitle)),
                        ("lineNumber", .int(hit.lineNumber)),
                    ])
                })
            })))
        }
        return .object(output)
    }
}
