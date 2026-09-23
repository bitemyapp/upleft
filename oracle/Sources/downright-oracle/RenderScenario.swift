import AppKit
import MarkdownCore
@testable import MarkdownRender

/// `downright-oracle render-state <scenario.json> <out.png> [--layout out.json]`
///
/// A `render` capture of one document driven into a particular state through
/// the renderer's public surface: configuration, streamed or external edits
/// (reparse, `ASTDiff.dirtySet`, `update(document:dirty:)`, exactly as the app
/// commits an external change), search hits, change marks, speech highlight,
/// folding, structural zoom, source focus, collapsed code, selection and
/// scroll position. `crates/conformance/src/dump/render_state.rs` reads the
/// same file and makes the same calls in the same order.
///
/// Every range is UTF-16 and refers to the text *after* all edits. The
/// scenario files are written by `scripts/build-render-state-corpus.py`.
struct RenderScenario {
    struct Edit {
        var range: NSRange
        var text: String
    }

    var document: URL
    var mode: RenderMode = .live
    var theme = "Paper Light"
    var dark = false
    var width: CGFloat = 1000
    var height: CGFloat = 1400
    var reduceMotion = true
    var configuration: MarkdownRenderConfiguration?
    var initialText: String?
    var edits: [Edit] = []
    var foldedHeadings: [String] = []
    var zoom: ZoomLevel?
    var collapseCode: [(offset: Int, collapsed: Bool)] = []
    var focusSource: NSRange?
    var changeMarks: [MarkdownTextView.ChangeMark] = []
    var searchHits: [NSRange] = []
    var currentSearchHit: NSRange?
    var speechHighlight: NSRange?
    var selection: [NSRange] = []
    var scroll: (offset: Int, position: ScrollPosition)?

    static func load(_ url: URL) throws -> RenderScenario {
        let data = try Data(contentsOf: url)
        guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let document = json["document"] as? String else {
            throw OracleError.usage("\(url.path): a scenario needs a \"document\"")
        }
        func range(_ value: Any?) -> NSRange? {
            guard let pair = value as? [Int], pair.count == 2 else { return nil }
            return NSRange(location: pair[0], length: pair[1])
        }
        func ranges(_ value: Any?) -> [NSRange] {
            (value as? [Any] ?? []).compactMap(range)
        }
        var scenario = RenderScenario(document: URL(fileURLWithPath: document, relativeTo: url.deletingLastPathComponent()).standardizedFileURL)
        if let mode = (json["mode"] as? String).flatMap(RenderMode.init(rawValue:)) { scenario.mode = mode }
        if let theme = json["theme"] as? String { scenario.theme = theme }
        if let dark = json["dark"] as? Bool { scenario.dark = dark }
        if let width = json["width"] as? Double { scenario.width = CGFloat(width) }
        if let height = json["height"] as? Double { scenario.height = CGFloat(height) }
        if let reduceMotion = json["reduceMotion"] as? Bool { scenario.reduceMotion = reduceMotion }
        if let configuration = json["configuration"] as? [String: Any] {
            scenario.configuration = MarkdownRenderConfiguration(
                showInvisibles: configuration["showInvisibles"] as? Bool ?? false,
                revealPolicy: (configuration["revealPolicy"] as? String).flatMap(MarkdownRevealPolicy.init(rawValue:)) ?? .primaryCaret,
                typographicSubstitution: configuration["typographicSubstitution"] as? Bool ?? false,
                typewriterScrolling: configuration["typewriterScrolling"] as? Bool ?? false,
                reflowHardWrappedParagraphs: configuration["reflowHardWrappedParagraphs"] as? Bool ?? true,
                codeCollapseThreshold: configuration["codeCollapseThreshold"] as? Int ?? RenderMetrics.codeCollapseLineCount
            )
        }
        scenario.initialText = json["initialText"] as? String
        scenario.edits = (json["edits"] as? [[String: Any]] ?? []).compactMap { edit in
            guard let location = edit["location"] as? Int, let length = edit["length"] as? Int,
                  let text = edit["text"] as? String else { return nil }
            return Edit(range: NSRange(location: location, length: length), text: text)
        }
        scenario.foldedHeadings = json["foldedHeadings"] as? [String] ?? []
        scenario.zoom = (json["zoom"] as? Int).flatMap(ZoomLevel.init(rawValue:))
        scenario.collapseCode = (json["collapseCode"] as? [[String: Any]] ?? []).compactMap { entry in
            guard let offset = entry["offset"] as? Int else { return nil }
            return (offset, entry["collapsed"] as? Bool ?? true)
        }
        scenario.focusSource = range(json["focusSource"])
        scenario.changeMarks = (json["changeMarks"] as? [[String: Any]] ?? []).compactMap { mark in
            guard let kind = (mark["kind"] as? String).flatMap(ChangeKind.init(rawValue:)),
                  let markRange = range(mark["range"]) else { return nil }
            return MarkdownTextView.ChangeMark(
                kind: kind, range: markRange, words: ranges(mark["words"]),
                visited: mark["visited"] as? Bool ?? false,
                deletedText: mark["deletedText"] as? String ?? ""
            )
        }
        scenario.searchHits = ranges(json["searchHits"])
        scenario.currentSearchHit = range(json["currentSearchHit"])
        scenario.speechHighlight = range(json["speechHighlight"])
        scenario.selection = ranges(json["selection"])
        if let scroll = json["scroll"] as? [String: Any], let offset = scroll["offset"] as? Int {
            scenario.scroll = (offset, scroll["position"] as? String == "center" ? .center : .top)
        }
        return scenario
    }

    /// The capture parameters, from the scenario rather than from flags.
    func request(output: URL, layout: URL?, headless: Bool, captureFromScreen: Bool) -> RenderRequest {
        RenderRequest(
            input: document, outputPNG: output, outputLayout: layout,
            mode: mode, themeName: theme, dark: dark, width: width, height: height,
            captureFromScreen: captureFromScreen, headless: headless
        )
    }

    /// Everything after the first frame, in a fixed order.
    func apply(to textView: MarkdownTextView) {
        guard let storage = textView.textStorage else { return }
        var document = textView.parsedDocument
        for edit in edits {
            storage.beginEditing()
            storage.replaceCharacters(in: edit.range, with: edit.text)
            storage.endEditing()
            let fresh = MarkdownParser.parse(storage.string)
            let dirty = ASTDiff.dirtySet(old: document, new: fresh)
            textView.update(document: fresh, dirty: dirty)
            document = fresh
            // One frame per edit, as a live stream would get.
            textView.prepareForDisplay()
        }
        if !foldedHeadings.isEmpty { textView.foldedHeadingSlugs = Set(foldedHeadings) }
        if let zoom { textView.zoomLevel = zoom }
        for entry in collapseCode { textView.setCodeBlockCollapsed(entry.collapsed, at: entry.offset) }
        if let focusSource { textView.focusSource(in: focusSource) }
        if !changeMarks.isEmpty { textView.changeMarks = changeMarks }
        if !searchHits.isEmpty { textView.searchHits = searchHits }
        if let currentSearchHit { textView.currentSearchHit = currentSearchHit }
        if let speechHighlight { textView.speechHighlight = speechHighlight }
        if !selection.isEmpty { textView.setSourceSelectedRanges(selection) }
        if let scroll { textView.scroll(toOffset: scroll.offset, position: scroll.position, animated: false) }
        textView.prepareForDisplay()
        textView.displayIfNeeded()
    }
}
