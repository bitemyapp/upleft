import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `ActivityIndicatorView` scenes. The reveal waits a second, so only the
/// synchronous states are captured: idle, `begin()` (still hidden) and
/// `begin()` then `end()`.
@MainActor
final class ActivityIndicatorViewScene: PanelScene {
    private var indicator: ActivityIndicatorView?
    private var visibilityChanges: [Bool] = []

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let indicator = ActivityIndicatorView()
        indicator.onVisibilityChange = { [weak self] hidden in self?.visibilityChanges.append(hidden) }
        if scenario.bool("begin") { indicator.begin() }
        if scenario.bool("end") { indicator.end() }
        self.indicator = indicator
        let container = NSView(frame: NSRect(x: 0, y: 0, width: scenario.width, height: scenario.height))
        indicator.frame = NSRect(x: 4, y: 4, width: 18, height: 18)
        container.addSubview(indicator)
        return container
    }

    func model() -> JSON {
        guard let indicator else { return .null }
        return .object([
            ("hidden", .bool(indicator.isHidden)),
            ("intrinsicContentSize", PanelTree.size(indicator.intrinsicContentSize)),
            ("visibilityChanges", .array(visibilityChanges.map { .bool($0) })),
        ])
    }
}
