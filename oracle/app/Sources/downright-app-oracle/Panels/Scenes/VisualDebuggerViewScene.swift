import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `VisualDebuggerView` scenes: a `VisualDebuggerModel` over the scenario's
/// document (parsed with `MarkdownParser.parse`) at `state.selection`
/// (`[location, length]`), `state.mode` (a `RenderMode` raw value, default
/// live) and `state.style` (the style facts), unless `state.noModel` keeps the
/// view's empty-document model. Copying is left to the tests (it writes a
/// pasteboard). `state.current` builds with `VisualDebuggerView()` and
/// assigns the sheet afterwards. Reduce Motion: nothing here animates.
@MainActor
final class VisualDebuggerViewScene: PanelScene {
    private var view: VisualDebuggerView?

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let view = scenario.bool("current") ? VisualDebuggerView() : VisualDebuggerView(styleSheet: styleSheet)
        if scenario.bool("current") { view.styleSheet = styleSheet }
        if !scenario.bool("noModel") {
            let document = MarkdownParser.parse(try scenario.documentText())
            let selection = scenario.array("selection").compactMap { ($0 as? NSNumber)?.intValue }
            let style = scenario.object("style")
            view.model = VisualDebuggerModel(input: VisualDebuggerInput(
                document: document,
                selection: selection.count == 2
                    ? NSRange(location: selection[0], length: selection[1])
                    : NSRange(location: 0, length: 0),
                mode: RenderMode(rawValue: scenario.string("mode", "live")) ?? .live,
                style: VisualDebuggerStyleFacts(
                    fontFamily: style["fontFamily"] as? String ?? "",
                    pointSize: (style["pointSize"] as? NSNumber)?.doubleValue ?? 0,
                    foregroundColor: style["foregroundColor"] as? String ?? "",
                    paragraphAlignment: style["paragraphAlignment"] as? String ?? "",
                    lineHeight: (style["lineHeight"] as? NSNumber)?.doubleValue ?? 0,
                    lineSpacing: (style["lineSpacing"] as? NSNumber)?.doubleValue ?? 0,
                    attributes: (style["attributes"] as? [Any] ?? []).compactMap { $0 as? String }
                )
            ))
        }
        self.view = view
        return view
    }

    func model() -> JSON {
        guard let view else { return .null }
        return .object([
            ("preferredWidth", .double(Double(view.preferredWidth))),
            ("line", .int(view.model.line)),
            ("column", .int(view.model.column)),
            ("summary", .string(view.summaryTextForTesting())),
            ("acceptsFirstResponder", .bool(view.acceptsFirstResponder)),
            ("fittingSize", PanelTree.size(view.fittingSize)),
        ])
    }
}
