import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `LocalAIPanelView` scenes. Not written yet.
@MainActor
final class LocalAIPanelViewScene: PanelScene {
    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        throw PanelHarnessError.notPorted("LocalAIPanelView")
    }
}
