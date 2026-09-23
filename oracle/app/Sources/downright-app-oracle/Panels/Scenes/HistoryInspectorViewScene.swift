import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `HistoryInspectorView` scenes: `state.versions` as in
/// `VersionTimelineViewScene`, then `state.steps` on the embedded timeline
/// (its delegate, the inspector, updates the caption). `state.current`
/// builds with `HistoryInspectorView()` and assigns the sheet afterwards.
///
/// Histories stay years in the past: the caption's relative date compares
/// with now. Reduce Motion: nothing here animates.
@MainActor
final class HistoryInspectorViewScene: PanelScene {
    private var inspector: HistoryInspectorView?
    private var timeline: VersionTimelineView?

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let inspector = scenario.bool("current") ? HistoryInspectorView() : HistoryInspectorView(styleSheet: styleSheet)
        if scenario.bool("current") { inspector.styleSheet = styleSheet }
        inspector.versions = VersionTimelineViewScene.versions(scenario)
        let timeline = inspector.subviews.compactMap { $0 as? VersionTimelineView }.first
        for step in scenario.array("steps").compactMap({ ($0 as? NSNumber)?.intValue }) {
            _ = step > 0 ? timeline?.accessibilityPerformIncrement() : timeline?.accessibilityPerformDecrement()
        }
        self.inspector = inspector
        self.timeline = timeline
        return inspector
    }

    func model() -> JSON {
        guard let inspector else { return .null }
        return .object([
            ("versionCount", .int(inspector.versions.count)),
            ("selectedIndex", .int(timeline?.selectedIndex)),
            ("selectedHash", .string(timeline?.selectedRecord?.hash)),
            ("fittingSize", PanelTree.size(inspector.fittingSize)),
        ])
    }
}
