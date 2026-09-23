import Foundation
import MarkdownCore

/// The canonical dump of `MarkdownParser.parse`. `crates/conformance` emits
/// the same shape from `upleft-core`; field names and order must match.
enum ParseDump {
    static func document(_ document: ParsedDocument) -> JSON {
        .object([
            ("length", .int(document.length)),
            ("lineStarts", .array(document.lineStarts.map { .int($0) })),
            ("frontMatter", document.frontMatter.map(frontMatter) ?? .null),
            ("root", block(document.root)),
            ("headings", .array(document.headings.map(heading))),
            ("tasks", .array(document.tasks.map(task))),
            ("pathTokens", .array(document.pathTokens.map(resolvableToken))),
            ("footnotes", .array(document.footnotes.keys.sorted().map { key in
                let block = document.footnotes[key]!
                return .object([
                    ("identifier", .string(key)),
                    ("range", .range(block.range)),
                    ("identity", identity(block.identity)),
                ])
            })),
            ("linkReferences", .array(document.linkReferences.keys.sorted().map { key in
                let reference = document.linkReferences[key]!
                return .object([
                    ("key", .string(key)),
                    ("identifier", .string(reference.identifier)),
                    ("destination", .string(reference.destination)),
                    ("title", .string(reference.title)),
                    ("range", .range(reference.range)),
                ])
            })),
        ])
    }

    static func identity(_ identity: BlockIdentity) -> JSON {
        .array([.int(identity.kind), .int(identity.ordinal)])
    }

    static func block(_ block: MDBlock) -> JSON {
        .object([
            ("content", content(block.content)),
            ("range", .range(block.range)),
            ("contentRange", .range(block.contentRange)),
            ("markerRange", .range(block.markerRange)),
            ("trailingMarkerRange", .range(block.trailingMarkerRange)),
            ("depth", .int(block.depth)),
            ("quoteDepth", .int(block.quoteDepth)),
            ("subtreeHash", .hex(block.subtreeHash)),
            ("identity", identity(block.identity)),
            ("safeHTML", block.safeHTML.map(safeHTML) ?? .null),
            ("inlines", .array(block.inlines.map(inline))),
            ("children", .array(block.children.map(self.block))),
        ])
    }

    static func content(_ content: BlockContent) -> JSON {
        switch content {
        case .document:
            return .object([("kind", .string("document"))])
        case .heading(let level):
            return .object([("kind", .string("heading")), ("level", .int(level))])
        case .paragraph:
            return .object([("kind", .string("paragraph"))])
        case .blockQuote:
            return .object([("kind", .string("blockQuote"))])
        case .callout(let kind, let title):
            return .object([
                ("kind", .string("callout")),
                ("calloutKind", .string(kind.rawValue)),
                ("title", .string(title)),
            ])
        case .list(let ordered, let start, let tight, let marker):
            return .object([
                ("kind", .string("list")),
                ("ordered", .bool(ordered)),
                ("start", .int(start)),
                ("tight", .bool(tight)),
                ("marker", .string(marker.rawValue)),
            ])
        case .listItem(let ordinal, let checkbox):
            return .object([
                ("kind", .string("listItem")),
                ("ordinal", .int(ordinal)),
                ("checkbox", checkbox.map { .object([
                    ("isChecked", .bool($0.isChecked)),
                    ("markRange", .range($0.markRange)),
                ]) } ?? .null),
            ])
        case .codeBlock(let language, let isFenced, let contentRange):
            return .object([
                ("kind", .string("codeBlock")),
                ("language", .string(language)),
                ("isFenced", .bool(isFenced)),
                ("codeRange", .range(contentRange)),
            ])
        case .mermaid(let sourceRange):
            return .object([("kind", .string("mermaid")), ("sourceRange", .range(sourceRange))])
        case .mathBlock(let latexRange):
            return .object([("kind", .string("mathBlock")), ("latexRange", .range(latexRange))])
        case .table(let data):
            return .object([
                ("kind", .string("table")),
                ("alignments", .array(data.alignments.map { .string($0.rawValue) })),
                ("delimiterRange", .range(data.delimiterRange)),
                ("rows", .array(data.rows.map { row in
                    .object([
                        ("range", .range(row.range)),
                        ("isHeader", .bool(row.isHeader)),
                        ("cells", .array(row.cells.map { cell in
                            .object([
                                ("range", .range(cell.range)),
                                ("contentRange", .range(cell.contentRange)),
                                ("inlines", .array(cell.inlines.map(inline))),
                            ])
                        })),
                    ])
                })),
            ])
        case .thematicBreak:
            return .object([("kind", .string("thematicBreak"))])
        case .htmlBlock:
            return .object([("kind", .string("htmlBlock"))])
        case .frontMatter(let value):
            return .object([("kind", .string("frontMatter")), ("frontMatter", frontMatter(value))])
        case .footnoteDefinition(let identifier):
            return .object([("kind", .string("footnoteDefinition")), ("identifier", .string(identifier))])
        }
    }

    static func frontMatter(_ value: FrontMatter) -> JSON {
        .object([
            ("range", .range(value.range)),
            ("bodyRange", .range(value.bodyRange)),
            ("fields", .array(value.fields.map { field in
                .object([
                    ("key", .string(field.key)),
                    ("value", .string(field.value)),
                    ("keyRange", .range(field.keyRange)),
                    ("valueRange", .range(field.valueRange)),
                ])
            })),
        ])
    }

    static func inline(_ span: InlineSpan) -> JSON {
        .object([
            ("kind", inlineKind(span.kind)),
            ("range", .range(span.range)),
            ("contentRange", .range(span.contentRange)),
            ("leadingMarkerRange", .range(span.leadingMarkerRange)),
            ("trailingMarkerRange", .range(span.trailingMarkerRange)),
            ("children", .array(span.children.map(inline))),
        ])
    }

    static func pathToken(_ token: PathToken) -> JSON {
        .object([
            ("rawPath", .string(token.rawPath)),
            ("line", .int(token.line)),
            ("column", .int(token.column)),
        ])
    }

    static func inlineKind(_ kind: InlineKind) -> JSON {
        switch kind {
        case .text: return .object([("kind", .string("text"))])
        case .emphasis: return .object([("kind", .string("emphasis"))])
        case .strong: return .object([("kind", .string("strong"))])
        case .strikethrough: return .object([("kind", .string("strikethrough"))])
        case .inlineCode: return .object([("kind", .string("inlineCode"))])
        case .link(let destination, let title):
            return .object([
                ("kind", .string("link")),
                ("destination", .string(destination)),
                ("title", .string(title)),
            ])
        case .autolink(let destination):
            return .object([("kind", .string("autolink")), ("destination", .string(destination))])
        case .wikilink(let target, let label):
            return .object([
                ("kind", .string("wikilink")),
                ("target", .string(target)),
                ("label", .string(label)),
            ])
        case .image(let source, let alt):
            return .object([
                ("kind", .string("image")),
                ("source", .string(source)),
                ("alt", .string(alt)),
            ])
        case .inlineMath(let latexRange):
            return .object([("kind", .string("inlineMath")), ("latexRange", .range(latexRange))])
        case .pathToken(let token):
            return .object([("kind", .string("pathToken")), ("token", pathToken(token))])
        case .footnoteReference(let identifier):
            return .object([("kind", .string("footnoteReference")), ("identifier", .string(identifier))])
        case .softBreak: return .object([("kind", .string("softBreak"))])
        case .lineBreak: return .object([("kind", .string("lineBreak"))])
        case .inlineHTML: return .object([("kind", .string("inlineHTML"))])
        }
    }

    static func safeHTML(_ document: SafeHTMLDocument) -> JSON {
        .object([
            ("range", .range(document.range)),
            ("isSafe", .bool(document.isSafe)),
            ("annotations", .array(document.annotations.map { annotation in
                .object([
                    ("kind", safeHTMLKind(annotation.kind)),
                    ("range", .range(annotation.range)),
                    ("contentRange", .range(annotation.contentRange)),
                    ("tagRanges", .array(annotation.tagRanges.map { JSON.range($0) })),
                ])
            })),
        ])
    }

    static func safeHTMLKind(_ kind: SafeHTMLKind) -> JSON {
        switch kind {
        case .paragraph(let align):
            return .object([("kind", .string("paragraph")), ("align", .string(align?.rawValue))])
        case .heading(let level):
            return .object([("kind", .string("heading")), ("level", .int(level))])
        case .strong: return .object([("kind", .string("strong"))])
        case .emphasis: return .object([("kind", .string("emphasis"))])
        case .link(let destination, let title):
            return .object([
                ("kind", .string("link")),
                ("destination", .string(destination)),
                ("title", .string(title)),
            ])
        case .image(let source, let alt):
            return .object([("kind", .string("image")), ("source", .string(source)), ("alt", .string(alt))])
        case .inert: return .object([("kind", .string("inert"))])
        case .lineBreak: return .object([("kind", .string("lineBreak"))])
        case .details(let open): return .object([("kind", .string("details")), ("open", .bool(open))])
        case .detailsClosing: return .object([("kind", .string("detailsClosing"))])
        case .summary: return .object([("kind", .string("summary"))])
        case .table: return .object([("kind", .string("table"))])
        case .tableRow: return .object([("kind", .string("tableRow"))])
        case .tableCell(let header, let align):
            return .object([
                ("kind", .string("tableCell")),
                ("header", .bool(header)),
                ("align", .string(align?.rawValue)),
            ])
        }
    }

    static func heading(_ heading: HeadingNode) -> JSON {
        .object([
            ("level", .int(heading.level)),
            ("title", .string(heading.title)),
            ("range", .range(heading.range)),
            ("contentRange", .range(heading.contentRange)),
            ("sectionRange", .range(heading.sectionRange)),
            ("parentIndex", .int(heading.parentIndex)),
            ("childIndices", .array(heading.childIndices.map { .int($0) })),
            ("slug", .string(heading.slug)),
            ("wordCount", .int(heading.wordCount)),
        ])
    }

    static func task(_ task: TaskItem) -> JSON {
        .object([
            ("isChecked", .bool(task.isChecked)),
            ("markRange", .range(task.markRange)),
            ("contentRange", .range(task.contentRange)),
            ("text", .string(task.text)),
            ("headingIndex", .int(task.headingIndex)),
            ("indentLevel", .int(task.indentLevel)),
        ])
    }

    static func resolvableToken(_ token: ResolvableToken) -> JSON {
        .object([
            ("token", pathToken(token.token)),
            ("range", .range(token.range)),
            ("fromCodeSpan", .bool(token.fromCodeSpan)),
        ])
    }
}
