import AppKit
import MarkdownCore
@testable import MarkdownRender

/// `displaymap`: the source ⇄ TextKit coordinate maps Downright builds for a
/// document, and every conversion through them.
///
///  * `logical` — `DisplayMap(paragraphs:hidden:)` over the fully collapsed
///    marker set (`MarkerPolicy.hiddenRanges`, no caret).
///  * `carets` — at eight fixed carets: the caret-aware hidden set, the
///    revealed markers, and the logical map with that paragraph overridden
///    the way the view reveals a caret's markers.
///  * `hardWrap`, `inlineMath`, `footnotes` — the other producers'
///    output for the same document.
///  * `view` — what a real `MarkdownTextView` in `--mode` publishes after
///    `update(document:dirty: .wholesale)` (`currentDisplayMap`, the layout
///    map), with the caret reveal policy set to `.never` so the map does not
///    depend on where AppKit leaves the insertion point.
///
/// Per-offset conversions are run-length encoded as `[offset, value - offset]`
/// at every change of the difference, which is exact and keeps the dump
/// proportional to the number of substitutions rather than to the text.
enum DisplayMapDump {
    static func run(text: String, flags: Flags) throws -> JSON {
        let appearance = NSAppearance(named: flags.dark ? .darkAqua : .aqua)!
        guard let theme = ThemeStore.shared.themes.first(where: { $0.name == flags.theme }) else {
            throw OracleError.unknownTheme(flags.theme, ThemeStore.shared.themes.map(\.name))
        }
        let styleSheet = StyleSheet(theme: theme, appearance: appearance, reduceMotionOverride: true)
        let document = MarkdownParser.parse(text)
        let ns = text as NSString
        let policy = flags.mode.policy
        let paragraphs = ParagraphIndex(text: ns)

        let hidden = MarkerPolicy.hiddenRanges(document: document, policy: policy, caret: nil, selections: [])
        let logical = DisplayMap(paragraphs: paragraphs, hidden: hidden)

        var carets: [JSON] = []
        for index in 1...8 {
            let caret = ns.length * index / 8
            let selections = [NSRange(location: caret, length: 0)]
            let revealed = MarkerPolicy.revealedMarkerRanges(
                document: document, policy: policy, caret: caret, selections: selections)
            let caretHidden = MarkerPolicy.hiddenRanges(
                document: document, policy: policy, caret: caret, selections: selections)
            let overridden = logical.replacingParagraph(containing: caret, excluding: revealed)
            carets.append(.object([
                ("caret", .int(caret)),
                ("revealed", ranges(revealed)),
                ("hidden", ranges(caretHidden)),
                ("paragraphHidden", ranges(logical.hiddenRanges(inParagraphContaining: caret))),
                ("ending", .range(logical.substitutionEnding(at: caret)?.sourceRange)),
                ("starting", .range(logical.substitutionStarting(at: caret)?.sourceRange)),
                // The override changes one paragraph; convert across it and
                // one offset either side.
                ("overridden", conversions(overridden, around: paragraphs.paragraphRange(containing: caret))),
            ]))
        }

        let plan = HardWrapReflow.plan(document: document, text: ns, hiddenRanges: hidden, enabled: true)
        let footnotes = FootnoteReferenceDisplay.references(in: document)

        // The real view, as the app drives it for a freshly opened document.
        let storage = NSTextStorage(string: text)
        let view = MarkdownTextView(
            frame: NSRect(x: 0, y: 0, width: flags.width, height: flags.height),
            storage: storage,
            styleSheet: styleSheet
        )
        var configuration = view.configuration
        configuration.revealPolicy = .never
        view.configuration = configuration
        view.mode = flags.mode
        view.update(document: document, dirty: .wholesale)
        let layout = view.currentDisplayMap

        return .object([
            ("length", .int(ns.length)),
            ("paragraphs", .array(paragraphs.starts.map { .int($0) })),
            ("hidden", ranges(hidden)),
            ("logical", conversions(logical)),
            ("carets", .array(carets)),
            ("hardWrap", .object([
                ("ranges", ranges(plan.ranges)),
                ("substitutions", .array(plan.substitutions.map(substitution))),
            ])),
            ("inlineMath", ranges(InlineMathDisplay.ranges(in: document))),
            ("footnotes", .array(footnotes.map { reference in
                .object([("range", .range(reference.range)), ("identifier", .string(reference.identifier))])
            })),
            ("view", .object([
                ("substitutions", .array(layout.substitutions.map(substitution))),
                ("hidden", ranges(layout.hiddenRanges)),
                ("conversions", conversions(layout)),
            ])),
        ])
    }

    static func ranges(_ ranges: [NSRange]) -> JSON {
        .array(ranges.map { .range($0) })
    }

    static func substitution(_ sub: DisplaySubstitution) -> JSON {
        .object([
            ("sourceRange", .range(sub.sourceRange)),
            ("displayLength", .int(sub.displayLength)),
            ("isHidden", .bool(sub.isHidden)),
            ("isHardWrapReflow", .bool(sub.isHardWrapReflow)),
            ("preservesSourceOffsets", .bool(sub.preservesSourceOffsets)),
            ("replacement", sub.replacement.map { replacement in
                .object([
                    ("string", .string(replacement.string)),
                    // A hidden run's layout filler copies the storage's own
                    // attributes, which the decorate suite already compares;
                    // its keys are enough to show where they were read.
                    ("attributes", sub.isHidden ? attributeKeys(replacement) : AttributeDump.storage(replacement)),
                ])
            } ?? .null),
        ])
    }

    /// Each attribute run's range and sorted keys.
    static func attributeKeys(_ string: NSAttributedString) -> JSON {
        var runs: [JSON] = []
        string.enumerateAttributes(in: NSRange(location: 0, length: string.length), options: []) { attributes, range, _ in
            runs.append(.object([
                ("range", .range(range)),
                ("keys", .array(attributes.keys.map(\.rawValue).sorted().map { .string($0) })),
            ]))
        }
        return .array(runs)
    }

    /// `[offset, f(offset) - offset]` wherever the difference changes, over
    /// `offsets`.
    static func runs(_ offsets: Range<Int>, _ f: (Int) -> Int) -> JSON {
        var out: [JSON] = []
        var last: Int?
        for offset in offsets {
            let delta = f(offset) - offset
            if delta != last {
                out.append(.array([.int(offset), .int(delta)]))
                last = delta
            }
        }
        return .array(out)
    }

    /// `[offset, value]` wherever a Boolean changes, over `offsets`.
    static func flags(_ offsets: Range<Int>, _ f: (Int) -> Bool) -> JSON {
        var out: [JSON] = []
        var last: Bool?
        for offset in offsets {
            let value = f(offset)
            if value != last {
                out.append(.array([.int(offset), .bool(value)]))
                last = value
            }
        }
        return .array(out)
    }

    /// Every conversion at every offset from 0 to one past the end (the
    /// clamp), or only around `paragraph` when one is given.
    static func conversions(_ map: DisplayMap, around paragraph: NSRange? = nil) -> JSON {
        let length = map.paragraphs.length
        let offsets: Range<Int>
        let paragraphIndices: Range<Int>
        if let paragraph {
            offsets = max(0, paragraph.location - 1)..<min(length + 2, paragraph.upperBound + 2)
            let index = map.paragraphs.index(containing: paragraph.location)
            paragraphIndices = index..<(index + 1)
        } else {
            offsets = 0..<(length + 2)
            paragraphIndices = map.paragraphs.starts.indices
        }
        return .object([
            ("isIdentity", .bool(map.isIdentity)),
            ("textKit", runs(offsets) { map.textKitOffset(forSource: $0) }),
            ("source", runs(offsets) { map.sourceOffset(forTextKit: $0) }),
            ("upper", runs(offsets) { map.sourceUpperBound(forTextKit: $0) }),
            ("canonical", flags(offsets) { map.isCanonical($0) }),
            ("textKitEnds", .array(paragraphIndices.map { .int(map.textKitEnd(ofParagraphAt: $0)) })),
        ])
    }
}
