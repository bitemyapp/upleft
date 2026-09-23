import Foundation
@testable import MarkdownCore

/// `core-text`: every MarkdownCore function that works on text alone — no
/// parse tree — applied to one input file. `crates/conformance/src/dump/
/// core_text.rs` emits the same shape from `upleft-core`.
///
/// The segmentation used to pick inputs (lines, paragraphs, quote lines,
/// fences, code spans, heading lines) is deliberately plain UTF-16 scanning,
/// identical on both sides, so that only MarkdownCore's own behaviour is
/// compared.
enum CoreTextDump {
    static func document(data: Data, url: URL) -> JSON {
        var pairs: [(String, JSON)] = []
        pairs.append(("byteCount", .int(data.count)))
        pairs.append(("readHead", .array([1, 2, 3, 4, 5, 7, 64, 4096].map { limit in
            .string(DocumentIO.readHead(contentsOf: url, limit: limit))
        })))
        let decoded: (text: String, fidelity: ByteFidelity)
        do {
            decoded = try DocumentIO.decodeSnapshot(data, sourceURL: url)
        } catch {
            pairs.append(("decode", .object([("error", .string(errorKind(error)))])))
            return .object(pairs)
        }
        let text = decoded.text
        pairs.append(("decode", .object([
            ("text", .string(text)),
            ("fidelity", fidelity(decoded.fidelity)),
        ])))
        do {
            let encoded = try DocumentIO.encodedData(text, fidelity: decoded.fidelity)
            pairs.append(("encode", .object([
                ("roundTrip", .bool(encoded == data)),
                ("sha256", .string(DocumentIO.contentHash(encoded))),
            ])))
        } catch {
            pairs.append(("encode", .object([("error", .string(errorKind(error)))])))
        }
        pairs.append(("contentHash", .string(DocumentIO.contentHash(text))))
        pairs.append(("dominantLineEnding", .string(lineEnding(DocumentIO.dominantLineEnding(text)))))
        pairs.append(contentsOf: analyses(text))
        return .object(pairs)
    }

    static func analyses(_ text: String) -> [(String, JSON)] {
        let ns = text as NSString
        let map = SourceMap(text)
        let whole = NSRange(location: 0, length: ns.length)
        let lines = (0..<map.lineCount).map { map.contentRange(ofLine: $0) }
        var out: [(String, JSON)] = []

        out.append(("length", .int(ns.length)))
        out.append(("fnvUTF8", .hex(FNV.hash(text))))
        out.append(("fnvUTF16", .hex(FNV.hash(ns, range: whole))))
        out.append(("sourceMap", .object([
            ("lineStarts", .array(map.lineStarts.map { .int($0) })),
            ("lineEnds", .array(map.lineEnds.map { .int($0) })),
            ("mayContainHTML", .bool(map.mayContainHTML)),
            ("offsets", .array((0..<min(map.lineCount + 1, 200)).map { line in
                .array([1, 2, 3, 5, 8, 13, 21, 34, 55, 89].map { .int(map.offset(line: line + 1, column: $0)) })
            })),
            ("lineContaining", .array(stride(from: 0, through: ns.length, by: max(1, ns.length / 97)).map {
                .int(map.line(containing: $0))
            })),
        ])))

        // SourceScanner: footnote and link reference definitions.
        let scan = SourceScanner(map: map)
        out.append(("sourceScanner", .object([
            ("footnoteDefinitions", .array(scan.footnoteDefinitions.map { definition in
                .object([
                    ("identifier", .string(definition.identifier)),
                    ("markerRange", .range(definition.markerRange)),
                    ("range", .range(definition.range)),
                ])
            })),
            ("linkReferences", .array(scan.linkReferences.keys.sorted().map { key in
                let reference = scan.linkReferences[key]!
                return .object([
                    ("key", .string(key)),
                    ("identifier", .string(reference.identifier)),
                    ("destination", .string(reference.destination)),
                    ("title", .string(reference.title)),
                    ("range", .range(reference.range)),
                ])
            })),
        ])))

        out.append(("frontMatter", FrontMatterScanner.scan(map).map(ParseDump.frontMatter) ?? .null))

        // Callouts: every line whose first non-blank unit is `>`.
        out.append(("callouts", .array(lines.enumerated().compactMap { index, range -> JSON? in
            guard firstNonBlank(ns, range).map({ ns.character(at: $0) == 0x3E }) == true else { return nil }
            let match = CalloutScanner.scan(map, quoteRange: range)
            return .object([
                ("line", .int(index)),
                ("match", match.map { match in
                    .object([
                        ("kind", .string(match.kind.rawValue)),
                        ("title", .string(match.title)),
                        ("markerRange", .range(match.markerRange)),
                    ])
                } ?? .null),
            ])
        })))

        // Math, wikilinks and path tokens over the whole text and per line.
        out.append(("math", .array(MathScanner.matches(in: ns, range: whole).map(math))))
        out.append(("mathPerLine", .array(lines.map { .array(MathScanner.matches(in: ns, range: $0).map(math)) })))
        out.append(("wikilinks", .array(WikilinkScanner.matches(in: ns, range: whole).map(wikilink))))
        out.append(("wikilinksPerLine", .array(lines.map { .array(WikilinkScanner.matches(in: ns, range: $0).map(wikilink)) })))
        out.append(("pathTokens", .array(PathTokenScanner.matches(in: ns, range: whole).map(pathMatch))))
        out.append(("pathTokensPerLine", .array(lines.map { .array(PathTokenScanner.matches(in: ns, range: $0).map(pathMatch)) })))
        out.append(("codeSpans", .array(codeSpans(ns, lines: lines).map { span in
            .object([
                ("range", .range(span)),
                ("pathToken", PathTokenScanner.codeSpanMatch(in: ns, range: span).map(pathMatch) ?? .null),
            ])
        })))

        // Paragraph chunks: SafeHTML, whole-block math.
        let paragraphs = paragraphRanges(ns, lines: lines)
        out.append(("safeHTML", SafeHTMLParser.parse(text).map(ParseDump.safeHTML) ?? .null))
        out.append(("paragraphs", .array(paragraphs.map { range in
            .object([
                ("range", .range(range)),
                ("safeHTML", SafeHTMLParser.parse(text, range: range).map(ParseDump.safeHTML) ?? .null),
                ("mathBlock", MathScanner.wholeBlock(in: ns, range: range).map(math) ?? .null),
            ])
        })))

        // Fences: kind of the info string and a guess from the body.
        out.append(("fences", .array(fences(ns, lines: lines).map { fence in
            let info = ns.substring(with: fence.info)
            let body = ns.substring(with: fence.body)
            return .object([
                ("line", .int(fence.line)),
                ("info", .string(info)),
                ("kind", .string(fenceKind(FenceLanguage.kind(for: info)))),
                ("guess", .string(FenceLanguage.guess(from: body))),
            ])
        })))

        // Slugs of heading-shaped lines.
        out.append(("slugs", .array(lines.compactMap { range -> JSON? in
            guard range.length > 0, ns.character(at: range.location) == 0x23 else { return nil }
            var start = range.location
            while start < range.upperBound, ns.character(at: start) == 0x23 { start += 1 }
            while start < range.upperBound, ns.character(at: start) == 0x20 { start += 1 }
            let title = ns.substring(with: NSRange(location: start, length: range.upperBound - start))
            return .array([.string(title), .string(Slug.make(title))])
        })))

        let metrics = Metrics.metrics(of: text)
        out.append(("metrics", .object([
            ("words", .int(metrics.words)),
            ("characters", .int(metrics.characters)),
            ("sentences", .int(metrics.sentences)),
            ("readMinutes", .double(metrics.readMinutes)),
        ])))

        // Diffs against deterministic mutations.
        let mutated = mutate(text)
        let edited = insertX(text)
        out.append(("diff", .object([
            ("mutatedLength", .int((mutated as NSString).length)),
            ("forward", hunks(TextDiff.hunks(old: text, new: mutated), newLength: (mutated as NSString).length)),
            ("backward", hunks(TextDiff.hunks(old: mutated, new: text), newLength: ns.length)),
            ("oneCharacter", hunks(TextDiff.hunks(old: text, new: edited), newLength: (edited as NSString).length)),
            ("identical", hunks(TextDiff.hunks(old: text, new: text), newLength: ns.length)),
            ("myers", myers(text, mutated, maxDistance: 4096)),
            ("myersCapped", myers(text, mutated, maxDistance: 8)),
        ])))
        return out
    }

    // MARK: Segmentation (mirrored exactly in Rust)

    static func isBlank(_ unit: unichar) -> Bool { unit == 0x20 || unit == 0x09 }

    static func firstNonBlank(_ ns: NSString, _ range: NSRange) -> Int? {
        var index = range.location
        while index < range.upperBound, isBlank(ns.character(at: index)) { index += 1 }
        return index < range.upperBound ? index : nil
    }

    /// Maximal runs of lines that are not all spaces and tabs.
    static func paragraphRanges(_ ns: NSString, lines: [NSRange]) -> [NSRange] {
        var out: [NSRange] = []
        var start: Int?
        var end = 0
        for range in lines {
            if firstNonBlank(ns, range) == nil {
                if let s = start { out.append(NSRange(location: s, length: end - s)); start = nil }
            } else {
                if start == nil { start = range.location }
                end = range.upperBound
            }
        }
        if let s = start { out.append(NSRange(location: s, length: end - s)) }
        return out
    }

    /// Backtick pairs on one line: each `` ` `` opens, the next `` ` `` on the
    /// same line closes, and the content between them is the span.
    static func codeSpans(_ ns: NSString, lines: [NSRange]) -> [NSRange] {
        var out: [NSRange] = []
        for range in lines {
            var index = range.location
            var open: Int?
            while index < range.upperBound {
                if ns.character(at: index) == 0x60 {
                    if let o = open {
                        out.append(NSRange(location: o + 1, length: index - o - 1))
                        open = nil
                    } else {
                        open = index
                    }
                }
                index += 1
            }
        }
        return out
    }

    struct Fence {
        var line: Int
        var info: NSRange
        var body: NSRange
    }

    /// A line whose first non-blank units are three or more backticks or
    /// tildes opens a fence; the next line starting (after blanks) with at
    /// least as many of the same unit closes it.
    static func fences(_ ns: NSString, lines: [NSRange]) -> [Fence] {
        func run(_ range: NSRange) -> (unit: unichar, start: Int, length: Int)? {
            guard let first = firstNonBlank(ns, range) else { return nil }
            let unit = ns.character(at: first)
            guard unit == 0x60 || unit == 0x7E else { return nil }
            var index = first
            while index < range.upperBound, ns.character(at: index) == unit { index += 1 }
            return index - first >= 3 ? (unit, first, index - first) : nil
        }
        var out: [Fence] = []
        var line = 0
        while line < lines.count {
            guard let open = run(lines[line]) else { line += 1; continue }
            let infoStart = open.start + open.length
            let info = NSRange(location: infoStart, length: lines[line].upperBound - infoStart)
            let bodyStart = line + 1 < lines.count ? lines[line + 1].location : lines[line].upperBound
            var closing = line + 1
            while closing < lines.count {
                if let close = run(lines[closing]), close.unit == open.unit, close.length >= open.length { break }
                closing += 1
            }
            let bodyEnd = closing < lines.count ? lines[closing].location : ns.length
            out.append(Fence(line: line, info: info, body: NSRange(location: bodyStart, length: max(0, bodyEnd - bodyStart))))
            line = closing + 1
        }
        return out
    }

    /// Deletes every 7th line and inserts a marker line before every 11th,
    /// splitting on LF code units.
    static func mutate(_ text: String) -> String {
        let units = Array(text.utf16)
        var lines: [ArraySlice<UInt16>] = []
        var start = 0
        for (index, unit) in units.enumerated() where unit == 0x0A {
            lines.append(units[start...index])
            start = index + 1
        }
        if start < units.count { lines.append(units[start...]) }
        var out: [UInt16] = []
        for (index, line) in lines.enumerated() {
            if index % 11 == 10 { out.append(contentsOf: "<<inserted \(index)>>\n".utf16) }
            if index % 7 == 6 { continue }
            out.append(contentsOf: line)
        }
        return String(decoding: out, as: UTF16.self)
    }

    /// drbench's one-character edit: an `x` at the middle Character.
    static func insertX(_ text: String) -> String {
        var edited = text
        edited.insert("x", at: edited.index(edited.startIndex, offsetBy: edited.count / 2))
        return edited
    }

    // MARK: Values

    static func errorKind(_ error: Error) -> String {
        switch error {
        case DocumentIOError.undecodable: return "undecodable"
        case DocumentIOError.unencodable(let encoding): return "unencodable:\(encoding.rawValue)"
        default: return "other"
        }
    }

    static func fidelity(_ value: ByteFidelity) -> JSON {
        .object([
            ("encoding", .string(value.encoding.rawValue)),
            ("hasBOM", .bool(value.hasBOM)),
            ("lineEnding", .string(lineEnding(value.lineEnding))),
            ("hasTrailingNewline", .bool(value.hasTrailingNewline)),
        ])
    }

    static func lineEnding(_ value: LineEnding) -> String {
        switch value {
        case .lf: return "lf"
        case .crlf: return "crlf"
        case .cr: return "cr"
        }
    }

    static func math(_ match: MathMatch) -> JSON {
        .object([
            ("range", .range(match.range)),
            ("contentRange", .range(match.contentRange)),
            ("isDisplay", .bool(match.isDisplay)),
        ])
    }

    static func wikilink(_ match: WikilinkMatch) -> JSON {
        .object([
            ("range", .range(match.range)),
            ("targetRange", .range(match.targetRange)),
            ("target", .string(match.target)),
            ("label", .string(match.label)),
        ])
    }

    static func pathMatch(_ match: PathTokenScanner.Match) -> JSON {
        .object([("range", .range(match.range)), ("token", ParseDump.pathToken(match.token))])
    }

    static func fenceKind(_ kind: FenceLanguage.Kind) -> String {
        switch kind {
        case .mermaid: return "mermaid"
        case .math: return "math"
        case .code: return "code"
        }
    }

    static func hunks(_ hunks: [ChangeHunk], newLength: Int) -> JSON {
        .array(hunks.map { hunk in
            .object([
                ("kind", .string(hunk.kind.rawValue)),
                ("newRange", .range(hunk.newRange)),
                ("oldRange", .range(hunk.oldRange)),
                ("wordRanges", .array(hunk.wordRanges.map { JSON.range($0) })),
                ("anchor", .range(TextDiff.anchorRange(for: hunk, inNewTextOfLength: newLength))),
            ])
        })
    }

    /// Myers over line hashes, run-length encoded as
    /// `[kind, firstOldIndex, firstNewIndex, count]` (`-1` for an absent side).
    static func myers(_ old: String, _ new: String, maxDistance: Int) -> JSON {
        let oldNS = old as NSString, newNS = new as NSString
        let oldLines = TextDiff.lines(of: oldNS), newLines = TextDiff.lines(of: newNS)
        guard let script = Myers.diff(
            oldLines.map { FNV.hash(oldNS, range: $0) },
            newLines.map { FNV.hash(newNS, range: $0) },
            maxDistance: maxDistance
        ) else { return .null }
        var runs: [(String, Int, Int, Int)] = []
        for step in script {
            let (kind, o, n): (String, Int, Int)
            switch step {
            case .equal(let oldIndex, let newIndex): (kind, o, n) = ("equal", oldIndex, newIndex)
            case .delete(let oldIndex): (kind, o, n) = ("delete", oldIndex, -1)
            case .insert(let newIndex): (kind, o, n) = ("insert", -1, newIndex)
            }
            if let last = runs.last, last.0 == kind,
               o == (last.1 < 0 ? -1 : last.1 + last.3), n == (last.2 < 0 ? -1 : last.2 + last.3) {
                runs[runs.count - 1].3 += 1
            } else {
                runs.append((kind, o, n, 1))
            }
        }
        return .array(runs.map { .array([.string($0.0), .int($0.1), .int($0.2), .int($0.3)]) })
    }
}
