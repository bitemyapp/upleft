import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `DocumentStatusBarView` scenes.
///
/// State, applied in this order:
///   cursor       `cursorPosition = (line, column)`, from `[line, column]`
///   caretOffset  `cursorPosition` from a UTF-16 offset into the document:
///                the line from `SourceLineIndex`, the column counted from
///                the line's first unit (1-based), as the window derives it
///   unsaved      `hasFileURL = false`
///   saved        `hasFileURL = true` again
///   clearCursor  `cursorPosition = nil`
///   invisible    `isVisible = false`
///   reshow       `isVisible = true`
///   restyle      assign the style sheet again
///
/// The bar has no animation; Reduce Motion changes nothing.
@MainActor
final class DocumentStatusBarViewScene: PanelScene {
    private var bar: DocumentStatusBarView?

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let bar = DocumentStatusBarView(styleSheet: styleSheet)
        let cursor = scenario.array("cursor").compactMap { ($0 as? NSNumber)?.intValue }
        if cursor.count == 2 { bar.cursorPosition = (line: cursor[0], column: cursor[1]) }
        if let offset = scenario.int("caretOffset") {
            let text = try scenario.documentText()
            let index = SourceLineIndex(text: text)
            let line = index.line(at: offset)
            let lineRange = (text as NSString).lineRange(for: NSRange(location: offset, length: 0))
            bar.cursorPosition = (line: line, column: offset - lineRange.location + 1)
        }
        if scenario.bool("unsaved") { bar.hasFileURL = false }
        if scenario.bool("saved") { bar.hasFileURL = true }
        if scenario.bool("clearCursor") { bar.cursorPosition = nil }
        if scenario.bool("invisible") { bar.isVisible = false }
        if scenario.bool("reshow") { bar.isVisible = true }
        if scenario.bool("restyle") { bar.styleSheet = styleSheet }
        self.bar = bar
        return bar
    }

    func model() -> JSON {
        guard let bar else { return .null }
        return .object([
            ("cursorPosition", bar.cursorPosition.map { .array([.int($0.line), .int($0.column)]) } ?? .null),
            ("hasFileURL", .bool(bar.hasFileURL)),
            ("isVisible", .bool(bar.isVisible)),
            ("intrinsicContentSize", PanelTree.size(bar.intrinsicContentSize)),
        ])
    }
}
