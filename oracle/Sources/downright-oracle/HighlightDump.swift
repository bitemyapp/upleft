import Foundation
@testable import MarkdownRender

/// `highlight`: every fenced code block in the document, found by a plain
/// line scan (not MarkdownCore), highlighted by
/// `BuiltinSyntaxHighlighter.shared`. The scan is defined on UTF-16 code
/// units so the Rust side (`crates/conformance/src/dump/highlight.rs`) picks
/// exactly the same blocks:
///
/// - lines end at U+000A (a trailing U+000D stays in the line);
/// - an opening fence is up to three spaces, then three or more of one of
///   `` ` `` or `~`; the info string is the rest of the line with spaces,
///   tabs and U+000D trimmed from both ends;
/// - the block closes at a line of up to three spaces, then at least as many
///   of the same character, then only spaces, tabs or U+000D; otherwise it
///   runs to the end of the document;
/// - the code is every unit from the first body line to the closing line;
/// - the language is the info string's first space- or tab-separated word.
enum HighlightDump {
    struct Fence {
        var line: Int
        var info: [UInt16]
        var code: [UInt16]
    }

    static func isBlank(_ unit: UInt16) -> Bool { unit == 0x20 || unit == 0x09 || unit == 0x0D }

    /// Returns (fence character, run length, index after the run) for a line.
    static func fenceRun(_ line: ArraySlice<UInt16>) -> (UInt16, Int, Int)? {
        var index = line.startIndex
        var spaces = 0
        while index < line.endIndex, line[index] == 0x20, spaces < 3 { index += 1; spaces += 1 }
        guard index < line.endIndex, line[index] == 0x60 || line[index] == 0x7E else { return nil }
        let marker = line[index]
        var length = 0
        while index < line.endIndex, line[index] == marker { index += 1; length += 1 }
        return length >= 3 ? (marker, length, index) : nil
    }

    static func fences(_ units: [UInt16]) -> [Fence] {
        var lines: [Range<Int>] = []
        var start = 0
        for (index, unit) in units.enumerated() where unit == 0x0A {
            lines.append(start..<(index + 1))
            start = index + 1
        }
        if start < units.count { lines.append(start..<units.count) }

        var result: [Fence] = []
        var lineIndex = 0
        while lineIndex < lines.count {
            let line = units[lines[lineIndex]]
            guard let (marker, length, after) = fenceRun(line) else { lineIndex += 1; continue }
            var info = Array(units[after..<line.endIndex])
            while let last = info.last, isBlank(last) || last == 0x0A { info.removeLast() }
            while let first = info.first, isBlank(first) { info.removeFirst() }
            let bodyStart = lines[lineIndex].upperBound
            var bodyEnd = units.count
            var closing = lines.count
            var probe = lineIndex + 1
            while probe < lines.count {
                let candidate = units[lines[probe]]
                if let (closeMarker, closeLength, closeAfter) = fenceRun(candidate),
                   closeMarker == marker, closeLength >= length,
                   units[closeAfter..<candidate.endIndex].allSatisfy({ isBlank($0) || $0 == 0x0A }) {
                    bodyEnd = lines[probe].lowerBound
                    closing = probe
                    break
                }
                probe += 1
            }
            result.append(Fence(line: lineIndex, info: info, code: Array(units[bodyStart..<max(bodyStart, bodyEnd)])))
            lineIndex = closing + 1
        }
        return result
    }

    static func language(_ info: [UInt16]) -> String? {
        let word = info.prefix { $0 != 0x20 && $0 != 0x09 }
        return word.isEmpty ? nil : String(utf16CodeUnits: Array(word), count: word.count)
    }

    static func runs(_ runs: [SyntaxRun]) -> JSON {
        .array(runs.map { .array([.int($0.range.location), .int($0.range.length), .string($0.token.rawValue)]) })
    }

    /// Language names that exercise trimming, case folding and the alias table.
    static let languageProbes: [String] = LanguageCatalog.canonicalNames
        + LanguageCatalog.aliases.keys.sorted()
        + ["SH", " Rust ", "\tPython3\n", "C++", "OBJECTIVE-C", "m\u{212A}d", "\u{200B}py\u{200B}", "",
           "   ", "unknown", "ts ", "İ", "ΣΑΣ", "rust,ignore", "{.python}", "json5", "Diff", "HTML"]

    static func document(_ text: String) -> JSON {
        let units = Array(text.utf16)
        let highlighter = BuiltinSyntaxHighlighter.shared
        let cache = SyntaxRunCache(capacity: 4)
        return .object([
            ("fences", .array(fences(units).map { fence in
                let info = String(utf16CodeUnits: fence.info, count: fence.info.count)
                let code = String(utf16CodeUnits: fence.code, count: fence.code.count)
                let language = language(fence.info)
                let direct = highlighter.highlight(code, language: language)
                // The cache must hand back exactly the direct result, twice.
                let cachedFirst = cache.runs(for: code, language: language, highlighter: highlighter)
                let cachedSecond = cache.runs(for: code, language: language, highlighter: highlighter)
                return .object([
                    ("line", .int(fence.line)),
                    ("info", .string(info)),
                    ("language", .string(language)),
                    ("canonical", .string(language.flatMap(BuiltinSyntaxHighlighter.canonicalLanguage))),
                    ("canonicalInfo", .string(BuiltinSyntaxHighlighter.canonicalLanguage(info))),
                    ("supports", .bool(language.map(highlighter.supports(language:)) ?? false)),
                    ("codeLength", .int(fence.code.count)),
                    ("runs", runs(direct)),
                    ("cacheAgrees", .bool(cachedFirst == direct && cachedSecond == direct)),
                    // The info string itself as the language, which is what a
                    // caller that forgets to split the info string passes.
                    ("infoRuns", runs(highlighter.highlight(code, language: info))),
                ])
            })),
            ("languages", .array(languageProbes.map { probe in
                .array([.string(probe), .string(BuiltinSyntaxHighlighter.canonicalLanguage(probe))])
            })),
            ("supported", .array(BuiltinSyntaxHighlighter.supportedLanguages.map { .string($0) })),
        ])
    }
}

/// `vscode-theme`: the input file through `VSCodeThemeImporter` (as
/// `ThemeStore.importVSCodeTheme` calls it) and through the theme decoder
/// (`JSONDecoder().decode(Theme.self, …)`, as `ThemeStore` loads user themes).
enum VSCodeThemeDump {
    static func dump(_ data: Data, url: URL) -> JSON {
        let fallbackName = url.deletingPathExtension().lastPathComponent
        let stripped = JSONCSanitizer.strip(data)
        let imported: JSON
        do {
            let theme = try VSCodeThemeImporter.theme(from: data, fallbackName: fallbackName)
            imported = .object([
                ("theme", ThemeDump.theme(theme)),
                ("validation", ThemeDump.validation(theme)),
                ("export", .string(exportText(theme))),
                ("slug", .string(ThemeStore.slug(theme.name))),
            ])
        } catch {
            imported = .object([("error", .string(String(describing: error)))])
        }
        let decoded: JSON
        if let theme = try? JSONDecoder().decode(Theme.self, from: data) {
            decoded = .object([("theme", ThemeDump.theme(theme)), ("export", .string(exportText(theme)))])
        } else {
            decoded = .null
        }
        return .object([
            ("fallbackName", .string(fallbackName)),
            ("stripped", .string(String(decoding: stripped, as: UTF8.self))),
            ("imported", imported),
            ("decoded", decoded),
        ])
    }

    /// `ThemeStore.export(_:to:)`'s encoder, without the file.
    static func exportText(_ theme: Theme) -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        return String(decoding: try! encoder.encode(theme), as: UTF8.self)
    }
}
