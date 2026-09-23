import AppKit
import MarkdownCore
@testable import MarkdownRender

/// `incremental`: decorate a document wholesale, then apply a fixed sequence
/// of source edits, each followed by the app's commit path — reparse,
/// `ASTDiff.dirtySet(old:new:)`, `DecorationEngine.decorate(_:document:dirty:)`
/// on the same engine and storage. Dumps each step's dirty set, decorated
/// bounds and counters, and the storage's attribute runs after the last edit.
///
/// Edits are positioned in UTF-16 units and never split a surrogate pair, so
/// `upleft-oracle` can reproduce them exactly.
enum IncrementalDump {
    struct Edit {
        var label: String
        var range: NSRange
        var replacement: String
    }

    static let stepCount = 8

    static func run(text: String, flags: Flags) throws -> JSON {
        let appearance = NSAppearance(named: flags.dark ? .darkAqua : .aqua)!
        guard let theme = ThemeStore.shared.themes.first(where: { $0.name == flags.theme }) else {
            throw OracleError.unknownTheme(flags.theme, ThemeStore.shared.themes.map(\.name))
        }
        let styleSheet = StyleSheet(theme: theme, appearance: appearance, reduceMotionOverride: true)
        let engine = DecorationEngine(styleSheet: styleSheet)
        engine.policy = flags.mode.policy
        let storage = NSTextStorage(string: text)
        var document = MarkdownParser.parse(text)
        let initial = engine.decorate(storage, document: document, dirty: .wholesale)

        var steps: [JSON] = []
        for step in 0..<stepCount {
            let edit = self.edit(step, in: storage.string as NSString)
            storage.replaceCharacters(in: edit.range, with: edit.replacement)
            let fresh = MarkdownParser.parse(storage.string)
            let dirty = ASTDiff.dirtySet(old: document, new: fresh)
            let bounds = engine.decoratedBounds(for: dirty, in: fresh, length: storage.length)
            let result = engine.decorate(storage, document: fresh, dirty: dirty)
            steps.append(.object([
                ("label", .string(edit.label)),
                ("edit", .range(edit.range)),
                ("replacement", .string(edit.replacement)),
                ("dirty", .object([
                    ("isWholesale", .bool(dirty.isWholesale)),
                    ("ranges", .array(dirty.ranges.map { .range($0) })),
                ])),
                ("bounds", .array(bounds.map { .range($0) })),
                ("result", resultJSON(result)),
            ]))
            document = fresh
        }
        return .object([
            ("initial", resultJSON(initial)),
            ("steps", .array(steps)),
            ("storage", AttributeDump.storage(storage)),
        ])
    }

    static func resultJSON(_ result: DecorationResult) -> JSON {
        .object([
            ("attributeRanges", .int(result.attributeRanges)),
            ("fragmentCount", .int(result.fragmentCount)),
        ])
    }

    /// `offset` clamped into the text and moved off the low half of a
    /// surrogate pair.
    static func snap(_ offset: Int, in text: NSString) -> Int {
        var p = max(0, min(offset, text.length))
        while p > 0, p < text.length, (0xDC00...0xDFFF).contains(text.character(at: p)) { p -= 1 }
        return p
    }

    static func isASCIILetter(_ c: unichar) -> Bool {
        (0x41...0x5A).contains(c) || (0x61...0x7A).contains(c)
    }

    static func isBreak(_ c: unichar) -> Bool { c == 0x0A || c == 0x0D }

    static func lineStart(_ offset: Int, in text: NSString) -> Int {
        var s = offset
        while s > 0, !isBreak(text.character(at: s - 1)) { s -= 1 }
        return s
    }

    static func edit(_ step: Int, in text: NSString) -> Edit {
        let n = text.length
        switch step {
        case 0:
            return Edit(label: "insert x at 1/3", range: NSRange(location: snap(n / 3, in: text), length: 0), replacement: "x")
        case 1:
            let p = snap(2 * n / 3, in: text)
            guard p < n else { return Edit(label: "delete at 2/3", range: NSRange(location: p, length: 0), replacement: "") }
            let c = text.character(at: p)
            let length = (0xD800...0xDBFF).contains(c) && p + 1 < n ? 2 : 1
            return Edit(label: "delete at 2/3", range: NSRange(location: p, length: length), replacement: "")
        case 2:
            return Edit(label: "newline at 1/2", range: NSRange(location: snap(n / 2, in: text), length: 0), replacement: "\n")
        case 3:
            let start = snap(n / 4, in: text)
            var i = start
            while i < n {
                if isASCIILetter(text.character(at: i)) {
                    var j = i
                    while j < n, isASCIILetter(text.character(at: j)) { j += 1 }
                    if j - i >= 3 {
                        return Edit(label: "replace word", range: NSRange(location: i, length: j - i), replacement: "renamed")
                    }
                    i = j
                } else {
                    i += 1
                }
            }
            return Edit(label: "replace word", range: NSRange(location: start, length: 0), replacement: "renamed")
        case 4:
            let s = lineStart(snap(3 * n / 4, in: text), in: text)
            return Edit(label: "heading at 3/4", range: NSRange(location: s, length: 0), replacement: "# ")
        case 5:
            let s = lineStart(snap(n / 5, in: text), in: text)
            var e = s
            while e < n, !isBreak(text.character(at: e)) { e += 1 }
            if e < n {
                e += text.character(at: e) == 0x0D && e + 1 < n && text.character(at: e + 1) == 0x0A ? 2 : 1
            }
            return Edit(label: "delete line at 1/5", range: NSRange(location: s, length: e - s), replacement: "")
        case 6:
            return Edit(label: "emphasis at 3/5", range: NSRange(location: snap(3 * n / 5, in: text), length: 0), replacement: "**")
        default:
            return Edit(label: "list at 4/5", range: NSRange(location: snap(4 * n / 5, in: text), length: 0), replacement: "\n\n- item\n")
        }
    }
}
