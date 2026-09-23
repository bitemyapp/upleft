import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `VisualDebuggerView` scenes. Not written yet.
@MainActor
final class VisualDebuggerViewScene: PanelScene {
    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        throw PanelHarnessError.notPorted("VisualDebuggerView")
    }
}
