import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `RenderTargetsView` scenes. Not written yet.
@MainActor
final class RenderTargetsViewScene: PanelScene {
    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        throw PanelHarnessError.notPorted("RenderTargetsView")
    }
}
