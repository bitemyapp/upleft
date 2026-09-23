import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `ConflictBarView` scenes: the bar as built, optionally with a status.
@MainActor
final class ConflictBarViewScene: PanelScene {
    private var bar: ConflictBarView?

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let bar = scenario.bool("current") ? ConflictBarView() : ConflictBarView(styleSheet: styleSheet)
        if scenario.bool("current") { bar.styleSheet = styleSheet }
        if let status = scenario.string("status") { bar.setStatus(status) }
        if let message = scenario.string("message") { bar.message = message }
        self.bar = bar
        return bar
    }

    func model() -> JSON {
        guard let bar else { return .null }
        return .object([
            ("message", .string(bar.message)),
            ("fittedWidth", .double(Double(bar.fittedWidth))),
            ("intrinsicContentSize", PanelTree.size(bar.intrinsicContentSize)),
        ])
    }
}
