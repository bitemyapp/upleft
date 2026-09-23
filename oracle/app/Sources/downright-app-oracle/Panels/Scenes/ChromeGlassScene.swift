import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `ChromeGlass` scenes: the material at `state.frame` inside a container
/// painted with the theme background, with a label in its content view.
@MainActor
final class ChromeGlassScene: PanelScene {
    private var glass: ChromeGlass?

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let container = NSView(frame: NSRect(x: 0, y: 0, width: scenario.width, height: scenario.height))
        container.wantsLayer = true
        container.layer?.backgroundColor = styleSheet.background.cgColor
        let tint: ChromeGlass.Tint
        switch scenario.string("tint", "panel") {
        case "band": tint = .band
        case "control": tint = .control
        default: tint = .panel
        }
        let glass = ChromeGlass(
            styleSheet: styleSheet,
            cornerRadius: CGFloat(scenario.double("cornerRadius", Double(PanelMetrics.surfaceRadius))),
            roundedCorners: scenario.string("corners", "all") == "bottomOnly" ? .bottomOnly : .all,
            tint: tint
        )
        if scenario.bool("showsFocus") { glass.showsFocus = true }
        if let opacity = scenario.double("shadowOpacity") { glass.shadowOpacity = Float(opacity) }
        let label = NSTextField(labelWithString: scenario.string("label", "Glass"))
        label.frame = NSRect(x: 12, y: 8, width: 160, height: 18)
        glass.contentView.addSubview(label)
        let frameValues = scenario.array("frame").compactMap { ($0 as? NSNumber)?.doubleValue }
        glass.frame = frameValues.count == 4
            ? NSRect(x: frameValues[0], y: frameValues[1], width: frameValues[2], height: frameValues[3])
            : container.bounds.insetBy(dx: 20, dy: 20)
        container.addSubview(glass)
        self.glass = glass
        return container
    }

    func model() -> JSON {
        guard let glass else { return .null }
        return .object([
            ("usesGlass", .bool(glass.usesGlass)),
            ("rendersOpaqueFallback", .bool(glass.rendersOpaqueFallbackForTesting)),
            ("isDarkBackground", .bool(ChromeGlass.isDarkBackground(glass.styleSheet.background))),
            ("glassTint", PanelTree.color(ChromeGlass.glassTint(glass.styleSheet, tint: glass.tint), in: glass)),
            ("opaqueFallback", PanelTree.color(ChromeGlass.opaqueFallbackColor(glass.styleSheet, tint: glass.tint), in: glass)),
        ])
    }
}
