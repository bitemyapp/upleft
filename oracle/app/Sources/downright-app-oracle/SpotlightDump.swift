import CoreServices
import DownrightSpotlightMetadata
import Foundation

/// Swift side of the `spotlight` suite: the Spotlight metadata Downright
/// derives from one corpus document.
///
/// * `accepts`: `SpotlightMetadataImporter.accepts(url)`.
/// * `metadata`: `SpotlightMetadataImporter.metadata(at:)` (a bounded head
///   read through `DocumentIO.readHead`) — title, text content (as lines),
///   keywords, content type and the `attributes` dictionary — or the
///   `CocoaError` code it throws.
/// * `importer`: what the C-callable `DownrightSpotlightPopulateMetadata`
///   bridge (the mdimporter's entry point) returns and writes into the
///   dictionary it is handed.
/// * `contentTypes`: `contentType(for:)` and `accepts(_:)` for this document's
///   name under every Markdown extension and a few others.
///
/// Nothing is read from the clock or from file dates.
enum SpotlightDump {
    static func run(input: URL, flags: [String]) throws -> JSON {
        var members: [(String, JSON)] = [("accepts", .bool(SpotlightMetadataImporter.accepts(input)))]
        do {
            let metadata = try SpotlightMetadataImporter.metadata(at: input)
            members.append(("metadata", dump(metadata)))
        } catch {
            members.append(("error", .int((error as NSError).code)))
        }

        let dictionary = NSMutableDictionary()
        let returned = downrightSpotlightPopulateMetadata(dictionary as CFMutableDictionary, "net.daringfireball.markdown" as CFString, input.path as CFString)
        members.append(("importer", .object([
            ("returned", .bool(returned)),
            ("attributes", attributes(dictionary as NSDictionary as? [String: Any] ?? [:])),
        ])))

        let base = input.deletingPathExtension().lastPathComponent
        let extensions = ["md", "MD", "markdown", "mdown", "mkd", "mdx", "mdc", "qmd", "rmd", "Rmd", "txt", ""]
        members.append(("contentTypes", .array(extensions.map { ext in
            let url = URL(fileURLWithPath: "/tmp/upleft-spotlight/\(base)" + (ext.isEmpty ? "" : ".\(ext)"))
            return .object([
                ("extension", .string(ext)),
                ("contentType", .string(SpotlightMetadataImporter.contentType(for: url))),
                ("accepts", .bool(SpotlightMetadataImporter.accepts(url))),
            ])
        })))
        return .object(members)
    }

    static func dump(_ metadata: SpotlightMetadata) -> JSON {
        .object([
            ("title", .string(metadata.title)),
            ("textContent", lines(metadata.textContent)),
            ("keywords", .array(metadata.keywords.map { .string($0) })),
            ("contentType", .string(metadata.contentType)),
            ("attributes", attributes(metadata.attributes)),
        ])
    }

    /// Members sorted by key; strings, string arrays, and the text content
    /// as lines.
    static func attributes(_ values: [String: Any]) -> JSON {
        .object(values.keys.sorted { Array($0.utf8).lexicographicallyPrecedes(Array($1.utf8)) }.map { key in
            let value = values[key]!
            switch value {
            case let text as String:
                return (key, key == "kMDItemTextContent" ? lines(text) : .string(text))
            case let array as [String]:
                return (key, .array(array.map { .string($0) }))
            default:
                return (key, .string("\(type(of: value))"))
            }
        })
    }

    static func lines(_ text: String) -> JSON {
        .array(Array(text.utf8).split(separator: 0x0A, omittingEmptySubsequences: false).map { .string(String(decoding: $0, as: UTF8.self)) })
    }
}
