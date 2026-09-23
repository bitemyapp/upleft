import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `DocumentStatusBarView` scenes. Not written yet.
@MainActor
final class DocumentStatusBarViewScene: PanelScene {
    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        throw PanelHarnessError.notPorted("DocumentStatusBarView")
    }
}
