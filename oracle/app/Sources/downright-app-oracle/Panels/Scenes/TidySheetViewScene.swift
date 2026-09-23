import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `TidySheetView` scenes. Not written yet.
@MainActor
final class TidySheetViewScene: PanelScene {
    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        throw PanelHarnessError.notPorted("TidySheetView")
    }
}
