import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `TaskProgressRing` scenes.
///
/// State, applied in this order:
///   current      build with `TaskProgressRing()` and assign the style sheet
///   fromDocument progress = (checked tasks, all tasks) of the document, as
///                `DocumentWindowController` computes it
///   steps        progress assignments in order, `[[done, total], ...]`
///   active       `isActive = true`
///   inactive     `isActive = false` (after `active`)
///   hover        `mouseEntered(with:)` with an enter event
///   exit         `mouseExited(with:)` after `hover`
///   press        `accessibilityPerformPress()`
///   restyle      assign the style sheet again
///
/// Reduce Motion (forced on by the harness) makes the arc, the count
/// crossfade, the completion check and pop, the hover plate and the release
/// moment immediate.
@MainActor
final class TaskProgressRingScene: PanelScene {
    private var ring: TaskProgressRing?
    private var activations = 0
    private var visibilityChanges: [Bool] = []
    private var pressResult: Bool?

    private static func enterExit(_ type: NSEvent.EventType) -> NSEvent? {
        NSEvent.enterExitEvent(
            with: type, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: 0,
            context: nil, eventNumber: 0, trackingNumber: 0, userData: nil
        )
    }

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let ring = scenario.bool("current") ? TaskProgressRing() : TaskProgressRing(styleSheet: styleSheet)
        if scenario.bool("current") { ring.styleSheet = styleSheet }
        ring.onActivate = { [weak self] in self?.activations += 1 }
        ring.onVisibilityChange = { [weak self] hidden in self?.visibilityChanges.append(hidden) }
        if scenario.bool("fromDocument") {
            let parsed = MarkdownParser.parse(try scenario.documentText())
            let completedTasks = parsed.tasks.reduce(into: 0) { count, task in
                if task.isChecked { count += 1 }
            }
            ring.progress = (done: completedTasks, total: parsed.tasks.count)
        }
        for step in scenario.array("steps") {
            guard let pair = step as? [Any], pair.count == 2 else { continue }
            ring.progress = (done: (pair[0] as? NSNumber)?.intValue ?? 0, total: (pair[1] as? NSNumber)?.intValue ?? 0)
        }
        if scenario.bool("active") { ring.isActive = true }
        if scenario.bool("inactive") { ring.isActive = false }
        if scenario.bool("hover"), let event = Self.enterExit(.mouseEntered) { ring.mouseEntered(with: event) }
        if scenario.bool("exit"), let event = Self.enterExit(.mouseExited) { ring.mouseExited(with: event) }
        if scenario.bool("press") { pressResult = ring.accessibilityPerformPress() }
        if scenario.bool("restyle") { ring.styleSheet = styleSheet }
        self.ring = ring
        return ring
    }

    func model() -> JSON {
        guard let ring else { return .null }
        return .object([
            ("progress", .array([.int(ring.progress.done), .int(ring.progress.total)])),
            ("countText", .string(ring.countTextForTesting)),
            ("isActive", .bool(ring.isActive)),
            ("accessibilityValueDescription", .string(ring.accessibilityValueDescription())),
            ("mouseDownCanMoveWindow", .bool(ring.mouseDownCanMoveWindow)),
            ("focusRingMaskBounds", PanelTree.rect(ring.focusRingMaskBounds)),
            ("activations", .int(activations)),
            ("visibilityChanges", .array(visibilityChanges.map { .bool($0) })),
            ("pressResult", pressResult.map { .bool($0) } ?? .null),
            ("morphSide", .double(Double(TaskProgressRing.morphSide))),
        ])
    }
}
