import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `FloatingPanelSurface` scenes: an inspector host holding one labelled
/// section, in a floating surface configured with a resting and a sliver
/// frame and presented (or dismissed) without animation, inside a container
/// painted with the theme background.
@MainActor
final class FloatingPanelSurfaceScene: PanelScene {
    private var surface: FloatingPanelSurface?
    private var settledCount = 0

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let container = NSView(frame: NSRect(x: 0, y: 0, width: scenario.width, height: scenario.height))
        container.wantsLayer = true
        container.layer?.backgroundColor = styleSheet.background.cgColor
        let host = InspectorHostView(frame: .zero)
        host.styleSheet = styleSheet
        let content = NSTextField(labelWithString: scenario.string("label", "Floating content"))
        content.textColor = styleSheet.text
        host.setContent(content, section: .tasks)
        let surface = FloatingPanelSurface(styleSheet: styleSheet, content: host)
        surface.onFrameSpringSettled = { [weak self] in self?.settledCount += 1 }
        let width = CGFloat(scenario.double("panelWidth", 300))
        let height = CGFloat(scenario.double("panelHeight", 260))
        let resting = NSRect(x: scenario.width - width - 20, y: scenario.height - height - 20, width: width, height: height)
        let sliver = NSRect(x: resting.minX, y: resting.maxY - FloatingPanelSurface.Top.pourSliverHeight,
                            width: width, height: FloatingPanelSurface.Top.pourSliverHeight)
        container.addSubview(surface)
        surface.setRestingFrame(resting)
        surface.configureWindowFrames(resting: resting, sliver: sliver, contentHeight: height)
        switch scenario.string("presentation", "present") {
        case "dismiss":
            surface.presentFromSliver(animated: false)
            surface.dismissToSliver(animated: false)
        case "sliver":
            break
        default:
            surface.presentFromSliver(animated: false)
        }
        self.surface = surface
        return container
    }

    func model() -> JSON {
        guard let surface else { return .null }
        return .object([
            ("frame", PanelTree.rect(surface.frame)),
            ("usesGlass", .bool(surface.usesGlass)),
            ("isDismissing", .bool(surface.isDismissing)),
            ("visibleBody", PanelTree.rect(surface.visibleBodyBoundsForHitTesting)),
            ("preferredWidth", .double(Double(surface.preferredWidth))),
            ("fittedContentHeight", .double(Double(surface.fittedContentHeight))),
            ("contentLayoutHeight", .double(Double(surface.contentLayoutHeightForTesting))),
            ("rendersBody", .bool(surface.rendersBodyForTesting)),
            ("opaqueFallbackMounted", .bool(surface.opaqueFallbackIsMountedForTesting)),
            ("settledCount", .int(settledCount)),
        ])
    }
}
