import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `PanelChrome.swift`'s controls, one per scenario (`state.kind`), placed in
/// a plain container at `state.frame` (default: the whole container).
///
/// kinds: messageBar, segmented, progress, checkbox, emptyState, groupRow,
/// backdrop, symbolButton, textButton, toggle, selectionRow, lineIndex,
/// relativeTime, metrics.
@MainActor
final class PanelChromeScene: PanelScene {
    private var values: [(String, JSON)] = []

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let container = NSView(frame: NSRect(x: 0, y: 0, width: scenario.width, height: scenario.height))
        let frameValues = scenario.array("frame").compactMap { ($0 as? NSNumber)?.doubleValue }
        let frame = frameValues.count == 4
            ? NSRect(x: frameValues[0], y: frameValues[1], width: frameValues[2], height: frameValues[3])
            : container.bounds
        let kind = scenario.string("kind", "")
        var view: NSView
        switch kind {
        case "messageBar":
            let bar = MessageBarView(styleSheet: styleSheet, stripeColor: styleSheet.accent)
            bar.message = scenario.string("message", "")
            for title in scenario.strings("actions") { bar.addAction(title) {} }
            for symbol in scenario.strings("symbols") { bar.addSymbolAction(symbol, label: symbol) {} }
            if let status = scenario.string("status") { bar.setStatus(status) }
            if scenario.bool("reviewLayout") { bar.useReviewBarLayout() }
            values.append(("fittedWidth", .double(Double(bar.fittedWidth))))
            view = bar
        case "segmented":
            let control = PanelSegmentedControl(
                items: scenario.strings("items"),
                selectedIndex: scenario.int("selectedIndex", 0),
                styleSheet: styleSheet
            )
            for index in scenario.array("disabled").compactMap({ ($0 as? NSNumber)?.intValue }) {
                control.setEnabled(false, forSegment: index)
            }
            if let select = scenario.int("select") { control.setSelectedIndex(select, animated: false) }
            values.append(("selectedIndex", .int(control.selectedIndex)))
            values.append(("intrinsicContentSize", PanelTree.size(control.intrinsicContentSize)))
            view = control
        case "progress":
            let bar = PanelProgressBar(styleSheet: styleSheet)
            bar.fraction = CGFloat(scenario.double("fraction", 0))
            values.append(("fraction", .double(Double(bar.fraction))))
            view = bar
        case "checkbox":
            let side = CGFloat(scenario.double("side", Double(PanelCheckbox.Geometry.panelSide)))
            let box = PanelCheckbox(side: side)
            box.setStyleSheet(styleSheet)
            switch scenario.string("checkState", "off") {
            case "on": box.setState(.on, animated: false)
            case "mixed": box.setState(.mixed, animated: false)
            default: break
            }
            if scenario.bool("toggle") { box.performToggle() }
            values.append(("state", .string("\(box.state)")))
            values.append(("hitBounds", PanelTree.rect(box.hitBounds)))
            view = box
        case "emptyState":
            let list = NSView(frame: container.bounds)
            container.addSubview(list)
            let empty = PanelEmptyStateView()
            empty.configure(
                symbol: scenario.string("symbol", "checkmark.seal"),
                title: scenario.string("title", ""),
                subtitle: scenario.string("subtitle", ""),
                styleSheet: styleSheet
            )
            empty.install(in: container, over: list, verticalBias: CGFloat(scenario.double("bias", 1)))
            empty.isHidden = false
            values.append(("title", .string(empty.title)))
            values.append(("subtitle", .string(empty.subtitle)))
            return container
        case "groupRow":
            let row = PanelGroupRowView(identifier: NSUserInterfaceItemIdentifier("group"))
            row.configure(text: scenario.string("text", ""), color: styleSheet.textSecondary)
            view = row
        case "backdrop":
            let backdrop = PanelBackdrop(styleSheet: styleSheet)
            if scenario.bool("usesSurfaceFill") { backdrop.usesSurfaceFill = true }
            if let veil = scenario.double("veilAlpha") { backdrop.veilAlpha = CGFloat(veil) }
            if scenario.bool("opaqueAccent") { backdrop.opaqueSurfaceColor = styleSheet.accent }
            if scenario.bool("blendsWithinWindow") { backdrop.blendsWithinWindow = true }
            view = backdrop
        case "symbolButton":
            let button = PanelButton.symbol(
                scenario.string("symbol", "xmark"),
                label: scenario.string("label", "Close"),
                action: ButtonAction {},
                pointSize: CGFloat(scenario.double("pointSize", 13))
            )
            if scenario.bool("tinted") { button.contentTintColor = styleSheet.textSecondary }
            if scenario.bool("disabled") { button.isEnabled = false }
            button.translatesAutoresizingMaskIntoConstraints = true
            values.append(("intrinsicContentSize", PanelTree.size(button.intrinsicContentSize)))
            view = button
        case "textButton":
            let button = PanelButton.text(scenario.string("title", ""), action: ButtonAction {},
                                          isDefault: scenario.bool("isDefault"))
            button.translatesAutoresizingMaskIntoConstraints = true
            values.append(("intrinsicContentSize", PanelTree.size(button.intrinsicContentSize)))
            view = button
        case "toggle":
            let button = PanelButton.toggle(scenario.string("title", ""), label: scenario.string("label", ""),
                                            action: ButtonAction {})
            if scenario.bool("on") { button.state = .on }
            button.translatesAutoresizingMaskIntoConstraints = true
            view = button
        case "selectionRow":
            let row = PanelSelectionRowView()
            row.styleSheet = styleSheet
            row.isSelected = scenario.bool("selected")
            view = row
        case "lineIndex":
            let index = SourceLineIndex(text: try scenario.documentText())
            var captions: [JSON] = []
            for pair in scenario.array("ranges") {
                guard let numbers = pair as? [NSNumber], numbers.count == 2 else { continue }
                let range = NSRange(location: numbers[0].intValue, length: numbers[1].intValue)
                captions.append(.array([.int(index.line(at: range.location)), .string(index.caption(for: range))]))
            }
            values.append(("captions", .array(captions)))
            view = NSView()
        case "relativeTime":
            let now = Date(timeIntervalSinceReferenceDate: scenario.double("now", 800_000_000))
            var strings: [JSON] = []
            for offset in scenario.array("offsets").compactMap({ ($0 as? NSNumber)?.doubleValue }) {
                let date = now.addingTimeInterval(-offset)
                strings.append(.array([.string(RelativeTime.short(date, now: now)),
                                       .string(RelativeTime.long(date, now: now)),
                                       .string(RelativeTime.stamp(date))]))
            }
            values.append(("strings", .array(strings)))
            view = NSView()
        case "metrics":
            var rows: [JSON] = []
            for height in scenario.array("heights").compactMap({ ($0 as? NSNumber)?.doubleValue }) {
                let h = CGFloat(height)
                rows.append(.array([.double(Double(PanelMetrics.capsuleRadius(forHeight: h))),
                                    .double(Double(PanelMetrics.controlRadius(forHeight: h))),
                                    PanelTree.rect(PanelMetrics.rowSurface(in: NSRect(x: 0, y: 0, width: 100, height: h)))]))
            }
            values.append(("radii", .array(rows)))
            let path = PanelMetrics.continuousRoundedPath(
                rect: NSRect(x: 2, y: 3, width: 120, height: 40), radius: CGFloat(scenario.double("radius", 12)))
            values.append(("pathBounds", PanelTree.rect(path.boundingBoxOfPath)))
            view = NSView()
        default:
            throw PanelHarnessError.usage("unknown PanelChrome kind \(kind)")
        }
        view.frame = frame
        container.addSubview(view)
        return container
    }

    func model() -> JSON { .object(values) }
}
