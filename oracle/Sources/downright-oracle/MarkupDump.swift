import Foundation
import Markdown

/// The canonical dump of the swift-markdown tree Downright's MarkdownCore
/// walks: `Document(parsing: text, options: [.disableSmartOpts])` over the
/// whole file. `crates/conformance/src/dump/markup.rs` emits the same shape
/// from `upleft-markup`; field names and order must match.
///
/// Every node carries its kind (the Swift type, `Table.Head` style for the
/// nested table types), its `range` as `[startLine, startColumn, endLine,
/// endColumn]` (1-based line, 1-based UTF-8 column) or null, the properties
/// its type exposes, `plainText` when the type has one, its `indexInParent`,
/// and its children.
enum MarkupDump {
    static func document(_ text: String) -> JSON {
        markup(Document(parsing: text, options: [.disableSmartOpts]))
    }

    static func range(_ range: SourceRange?) -> JSON {
        guard let range else { return .null }
        return .array([
            .int(range.lowerBound.line), .int(range.lowerBound.column),
            .int(range.upperBound.line), .int(range.upperBound.column),
        ])
    }

    static func markup(_ markup: Markup) -> JSON {
        var pairs: [(String, JSON)] = [
            ("kind", .string(kind(markup))),
            ("range", range(markup.range)),
        ]
        pairs += properties(markup)
        if let convertible = markup as? PlainTextConvertibleMarkup {
            pairs.append(("plainText", .string(convertible.plainText)))
        }
        pairs.append(("indexInParent", .int(markup.indexInParent)))
        pairs.append(("children", .array(markup.children.map(self.markup))))
        return .object(pairs)
    }

    static func kind(_ markup: Markup) -> String {
        switch markup {
        case is Document: return "Document"
        case is BlockQuote: return "BlockQuote"
        case is CodeBlock: return "CodeBlock"
        case is CustomBlock: return "CustomBlock"
        case is Heading: return "Heading"
        case is ThematicBreak: return "ThematicBreak"
        case is HTMLBlock: return "HTMLBlock"
        case is ListItem: return "ListItem"
        case is OrderedList: return "OrderedList"
        case is UnorderedList: return "UnorderedList"
        case is Paragraph: return "Paragraph"
        case is InlineCode: return "InlineCode"
        case is CustomInline: return "CustomInline"
        case is Emphasis: return "Emphasis"
        case is Image: return "Image"
        case is InlineHTML: return "InlineHTML"
        case is LineBreak: return "LineBreak"
        case is Link: return "Link"
        case is SoftBreak: return "SoftBreak"
        case is Strong: return "Strong"
        case is Text: return "Text"
        case is SymbolLink: return "SymbolLink"
        case is InlineAttributes: return "InlineAttributes"
        case is Strikethrough: return "Strikethrough"
        case is Table: return "Table"
        case is Table.Head: return "Table.Head"
        case is Table.Body: return "Table.Body"
        case is Table.Row: return "Table.Row"
        case is Table.Cell: return "Table.Cell"
        default: return "unexpected:\(type(of: markup))"
        }
    }

    static func properties(_ markup: Markup) -> [(String, JSON)] {
        switch markup {
        case let code as CodeBlock:
            return [("code", .string(code.code)), ("language", .string(code.language))]
        case let html as HTMLBlock:
            return [("rawHTML", .string(html.rawHTML))]
        case let heading as Heading:
            return [("level", .int(heading.level))]
        case let item as ListItem:
            let checkbox: JSON
            switch item.checkbox {
            case .checked: checkbox = .string("checked")
            case .unchecked: checkbox = .string("unchecked")
            case nil: checkbox = .null
            }
            return [("checkbox", checkbox)]
        case let list as OrderedList:
            return [("startIndex", .int(Int(list.startIndex)))]
        case let table as Table:
            return [
                ("columnAlignments", .array(table.columnAlignments.map { alignment -> JSON in
                    switch alignment {
                    case .left: return .string("left")
                    case .center: return .string("center")
                    case .right: return .string("right")
                    case nil: return .null
                    }
                })),
                ("maxColumnCount", .int(table.maxColumnCount)),
            ]
        case let cell as Table.Cell:
            return [("colspan", .int(Int(cell.colspan))), ("rowspan", .int(Int(cell.rowspan)))]
        case let link as Link:
            return [
                ("destination", .string(link.destination)),
                ("title", .string(link.title)),
                ("isAutolink", .bool(link.isAutolink)),
            ]
        case let image as Image:
            return [("source", .string(image.source)), ("title", .string(image.title))]
        case let code as InlineCode:
            return [("code", .string(code.code))]
        case let html as InlineHTML:
            return [("rawHTML", .string(html.rawHTML))]
        case let text as Text:
            return [("string", .string(text.string))]
        case let custom as CustomInline:
            return [("text", .string(custom.text))]
        case let link as SymbolLink:
            return [("destination", .string(link.destination))]
        case let attributes as InlineAttributes:
            return [("attributes", .string(attributes.attributes))]
        default:
            return []
        }
    }
}

/// `downright-oracle markup-bench <file.md> <out.json>`: times swift-markdown's
/// `Document(parsing:options: [.disableSmartOpts])` over the file the way
/// drbench's "cmark alone" case does (one warm-up, then nearest-rank
/// percentiles), so `upleft-markup`'s `parse` bench has a Swift number to beat.
enum MarkupBench {
    static let runs = 200

    static func run(_ input: URL, output: String) throws {
        var text = try String(contentsOf: input, encoding: .utf8)
        // drbench parses a string it built itself, which is native UTF-8; a
        // file read can come back bridged, so make it native before timing.
        text.makeContiguousUTF8()
        _ = Document(parsing: text, options: [.disableSmartOpts])
        var samples: [Double] = []
        samples.reserveCapacity(runs)
        for _ in 0..<runs {
            let start = DispatchTime.now().uptimeNanoseconds
            _ = Document(parsing: text, options: [.disableSmartOpts])
            samples.append(Double(DispatchTime.now().uptimeNanoseconds - start) / 1_000_000)
        }
        samples.sort()
        func percentile(_ p: Double) -> Double {
            let rank = Int((p * Double(samples.count)).rounded(.up))
            return samples[min(samples.count - 1, max(0, rank - 1))]
        }
        let json = JSON.object([
            ("runs", .int(runs)),
            ("p50Ms", .double(percentile(0.50))),
            ("p95Ms", .double(percentile(0.95))),
            ("minMs", .double(samples[0])),
        ])
        try json.text.write(toFile: output, atomically: true, encoding: .utf8)
        print(String(format: "swift-markdown Document(parsing:)  p50 %.3f ms  p95 %.3f ms  min %.3f ms  (n=%d)",
                     percentile(0.50), percentile(0.95), samples[0], runs))
    }
}
