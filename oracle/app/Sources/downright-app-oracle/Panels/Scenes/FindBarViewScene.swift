import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `FindBarView` scenes. State, applied in this order:
///
/// - `presentation`: `"bar"` (default) or `"inspector"`; `current`: build with
///   `FindBarView()` and assign the style sheet afterwards (bar only);
/// - `selectionScope`: `[location, length]`;
/// - `options`: option-menu titles to toggle, sent through the options menu
///   as a click would (`Regular Expression`, `Match Case`, `Whole Word`,
///   `In Selection`);
/// - `query`: typed into the field (`setQueryText`, which notifies);
/// - with a `document` and a query: a `FindSession` runs the bar's current
///   query over the document from `caret` (default 0), advances `advance`
///   times, and the bar gets its status and validity — what the window
///   controller's `runFind` does;
/// - `status` and `valid` override those; `showsReplace`; `replacement`
///   (typed into the replace field).
///
/// The bar presentation is hosted as the document stack hosts it: a pill
/// over a themed background, `inset` points from each side, vertically
/// centred. Reduce Motion (forced on by the harness) makes the replace row,
/// the count and the warning glyph change without animating.
@MainActor
final class FindBarViewScene: PanelScene {
    private var bar: FindBarView?
    private var session: FindSession?
    private let recorder = FindBarSceneRecorder()
    private var inset: CGFloat = 20

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let inspector = scenario.string("presentation") == "inspector"
        let bar: FindBarView
        if scenario.bool("current") {
            bar = FindBarView()
            bar.styleSheet = styleSheet
        } else {
            bar = FindBarView(styleSheet: styleSheet, presentation: inspector ? .inspector : .bar)
        }
        bar.delegate = recorder
        session = try Self.applyFindState(bar, scenario)
        if scenario.bool("showsReplace") { bar.showsReplace = true }
        if let replacement = scenario.string("replacement"),
           let field = Self.textField("Replace with", in: bar) {
            field.stringValue = replacement
        }
        inset = CGFloat(scenario.double("inset", 20))
        self.bar = bar
        return bar
    }

    /// `selectionScope`, `options`, `query` (with the document's find
    /// session), `status` and `valid`, in that order.
    static func applyFindState(_ bar: FindBarView, _ scenario: PanelScenario) throws -> FindSession? {
        var session: FindSession?
        let scope = scenario.array("selectionScope").compactMap { ($0 as? NSNumber)?.intValue }
        if scope.count == 2 {
            bar.selectionScope = NSRange(location: scope[0], length: scope[1])
        }
        let options = scenario.strings("options")
        if !options.isEmpty {
            let menu = bar.makeOptionsMenuForTesting()
            for title in options {
                guard let item = menu.item(withTitle: title), let action = item.action else { continue }
                _ = NSApp.sendAction(action, to: item.target, from: item)
            }
        }
        if let query = scenario.string("query") {
            bar.setQueryText(query)
            if scenario.documentPath != nil {
                let text = try scenario.documentText()
                let current = bar.currentQuery
                let found = FindSession()
                found.update(query: current, in: text, caret: scenario.int("caret", 0))
                for _ in 0..<scenario.int("advance", 0) {
                    _ = found.advance(forward: true)
                }
                bar.statusText = found.statusText
                bar.isQueryValid = FindEngine.isValid(current)
                session = found
            }
        }
        if let status = scenario.string("status") { bar.statusText = status }
        if scenario.state["valid"] != nil { bar.isQueryValid = scenario.bool("valid") }
        return session
    }

    func host(_ panel: NSView, in window: NSWindow, scenario: PanelScenario) {
        guard let bar, scenario.string("presentation") != "inspector" else {
            panel.frame = NSRect(x: 0, y: 0, width: scenario.width, height: scenario.height)
            window.contentView = panel
            return
        }
        let container = NSView(frame: NSRect(x: 0, y: 0, width: scenario.width, height: scenario.height))
        container.wantsLayer = true
        container.layer?.backgroundColor = bar.styleSheet.background.cgColor
        window.contentView = container
        let height = bar.intrinsicContentSize.height
        bar.frame = NSRect(
            x: inset,
            y: ((scenario.height - height) / 2).rounded(),
            width: scenario.width - inset * 2,
            height: height
        )
        container.addSubview(bar)
    }

    static func textField(_ label: String, in root: NSView) -> NSTextField? {
        for view in root.subviews {
            if let field = view as? NSTextField, field.accessibilityLabel() == label { return field }
            if let found = textField(label, in: view) { return found }
        }
        return nil
    }

    func model() -> JSON {
        guard let bar else { return .null }
        let query = bar.currentQuery
        let menu = bar.makeOptionsMenuForTesting()
        var pairs: [(String, JSON)] = [
            ("statusText", .string(bar.statusText)),
            ("isQueryValid", .bool(bar.isQueryValid)),
            ("showsReplace", .bool(bar.showsReplace)),
            ("query", .object([
                ("text", .string(query.text)),
                ("isRegex", .bool(query.isRegex)),
                ("caseSensitive", .bool(query.caseSensitive)),
                ("wholeWord", .bool(query.wholeWord)),
                ("scope", JSON.range(query.scope)),
            ])),
            ("selectionScope", JSON.range(bar.selectionScope)),
            ("intrinsicContentSize", PanelTree.size(bar.intrinsicContentSize)),
            ("dividerCount", .int(bar.dividerCountForTesting)),
            ("hasCloseButton", .bool(bar.hasCloseButtonForTesting)),
            ("searchFieldIsBezeled", .bool(bar.searchFieldIsBezeledForTesting)),
            ("leadingGlyphIsAccessible", .bool(bar.leadingGlyphIsAccessibleForTesting)),
            ("findRowFrame", PanelTree.rect(bar.findRowFrameForTesting)),
            ("replaceRowFrame", PanelTree.rect(bar.replaceRowFrameForTesting)),
            ("replaceRowAlpha", .double(Double(bar.replaceRowAlphaForTesting))),
            ("replaceRowIsHidden", .bool(bar.replaceRowIsHiddenForTesting)),
            ("usesDenseReplaceMaterial", .bool(bar.usesDenseReplaceMaterialForTesting)),
            ("options", .array(menu.items.map { item in
                .array([.string(item.title), .int(item.state.rawValue), .bool(item.isEnabled),
                        .bool(item.isSeparatorItem)])
            })),
            ("emittedQueries", .array(recorder.queries.map { .string($0.text) })),
        ]
        if let session {
            let matches = session.matches
            pairs.append(("session", .object([
                ("count", .int(session.count)),
                ("currentIndex", JSON.int(session.currentIndex)),
                ("currentMatch", JSON.range(session.currentMatch)),
                ("first", .array(matches.prefix(8).map { JSON.range($0) })),
                ("last", .array(matches.suffix(8).map { JSON.range($0) })),
            ])))
        }
        return .object(pairs)
    }
}

@MainActor
final class FindBarSceneRecorder: FindBarDelegate {
    var queries: [FindQuery] = []
    func findBar(_ bar: FindBarView, didChange query: FindQuery) { queries.append(query) }
    func findBar(_ bar: FindBarView, didRequestAdvance forward: Bool) {}
    func findBar(_ bar: FindBarView, didRequestReplace replacement: String, all: Bool) {}
    func findBarDidRequestClose(_ bar: FindBarView) {}
}
