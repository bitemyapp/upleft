import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `TaskSectionBarView` scenes: the bar in a plain container on the task
/// panel's rail (18pt leading and trailing insets, `state.top` points from the
/// top, default 20), given the segments of the scenario document's worklist,
/// laid out, then `state.actions` applied in order. Each action is
/// `[name, argument?]`:
///
///   hover x / press x / release x   a mouseMoved / mouseDown / mouseUp event at
///                                   fraction x of the bar's width, on its midline
///   exit                            mouseExited(with:)
///   key code                        keyDown(with:) (123 left, 124 right, 36 return)
///   clear                           segments = []
///
/// Reduce Motion (forced on by the harness): segments are placed in a
/// transaction with actions disabled (no glide, no cascade start times), so a
/// hover swell or a press dip lands at once and the scene is still.
@MainActor
final class TaskSectionBarViewScene: PanelScene {
    private var bar: TaskSectionBarView?
    private var selections: [Int] = []

    static func mouseEvent(_ type: NSEvent.EventType, at point: NSPoint) -> NSEvent {
        NSEvent.mouseEvent(
            with: type,
            location: point,
            modifierFlags: [],
            timestamp: 0,
            windowNumber: 0,
            context: nil,
            eventNumber: 0,
            clickCount: 1,
            pressure: 0
        )!
    }

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let document = MarkdownParser.parse(try scenario.documentText())
        let container = NSView(frame: NSRect(x: 0, y: 0, width: scenario.width, height: scenario.height))
        let bar = TaskSectionBarView(styleSheet: styleSheet)
        bar.onSelectSegment = { [weak self] index in self?.selections.append(index) }
        container.addSubview(bar)
        NSLayoutConstraint.activate([
            bar.leadingAnchor.constraint(equalTo: container.leadingAnchor, constant: TaskRowMetrics.contentInset),
            bar.trailingAnchor.constraint(equalTo: container.trailingAnchor, constant: -TaskRowMetrics.contentInset),
            bar.topAnchor.constraint(equalTo: container.topAnchor, constant: CGFloat(scenario.double("top", 20))),
        ])
        bar.segments = TaskWorklist(tasks: document.tasks, headings: document.headings).segments
        container.layoutSubtreeIfNeeded()
        for case let action as [Any] in scenario.array("actions") {
            guard let name = action.first as? String else { continue }
            let value = action.count > 1 ? (action[1] as? NSNumber)?.doubleValue ?? 0 : 0
            let point = bar.convert(NSPoint(x: bar.bounds.width * CGFloat(value), y: bar.bounds.midY), to: nil)
            switch name {
            case "hover": bar.mouseMoved(with: Self.mouseEvent(.mouseMoved, at: point))
            case "press": bar.mouseDown(with: Self.mouseEvent(.leftMouseDown, at: point))
            case "release": bar.mouseUp(with: Self.mouseEvent(.leftMouseUp, at: point))
            case "exit": bar.mouseExited(with: Self.mouseEvent(.mouseMoved, at: point))
            case "key": bar.keyDown(with: TaskPanelViewScene.keyEvent(UInt16(value)))
            case "clear": bar.segments = []
            default: break
            }
        }
        self.bar = bar
        return container
    }

    func model() -> JSON {
        guard let bar else { return .null }
        return .object([
            ("segmentCount", .int(bar.segments.count)),
            ("accessibilityValue", .string(bar.accessibilityValue() as? String)),
            ("customActions", .array((bar.accessibilityCustomActions() ?? []).map { .string($0.name) })),
            ("intrinsicContentSize", PanelTree.size(bar.intrinsicContentSize)),
            ("acceptsFirstResponder", .bool(bar.acceptsFirstResponder)),
            ("frame", PanelTree.rect(bar.frame)),
            ("selections", .array(selections.map { .int($0) })),
        ])
    }
}
