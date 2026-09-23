import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `LocalAIPanelView` scenes: `state.availability` (a case name),
/// `state.running`, then a result from `DeterministicLocalAIProvider` for
/// `state.task` (a `LocalAITask` raw value) over `state.source` (or the
/// scenario's document) and `state.selection` (`[location, length]`), or an
/// explicit `nil` result with `state.clearResult`; then `state.pick` (task
/// menu indices, each selected and its action sent as a click does) and
/// `state.press` ("apply"/"close", the buttons' actions). The real model is
/// never called. `state.current` builds with `LocalAIPanelView()` and assigns
/// the sheet afterwards. Reduce Motion: nothing here animates.
@MainActor
final class LocalAIPanelViewScene: PanelScene {
    private var panel: LocalAIPanelView?
    private var delegate: Delegate?
    private var error: String?

    private final class Delegate: LocalAIPanelViewDelegate {
        var events: [String] = []
        func localAIPanel(_ panel: LocalAIPanelView, didRequest task: LocalAITask) { events.append("request \(task.rawValue)") }
        func localAIPanel(_ panel: LocalAIPanelView, didApply preview: LocalAIPreview) {
            events.append("apply \(preview.range.location) \(preview.range.length)")
        }
        func localAIPanelDidCancel(_ panel: LocalAIPanelView) { events.append("cancel") }
    }

    private final class Box: @unchecked Sendable {
        var outcome: Result<LocalAIResult, Error>?
    }

    /// `try await DeterministicLocalAIProvider().run(request)`, synchronously.
    static func runDeterministic(_ request: LocalAIRequest) -> Result<LocalAIResult, Error> {
        let semaphore = DispatchSemaphore(value: 0)
        let box = Box()
        Task.detached {
            do {
                box.outcome = .success(try await DeterministicLocalAIProvider().run(request))
            } catch {
                box.outcome = .failure(error)
            }
            semaphore.signal()
        }
        semaphore.wait()
        return box.outcome!
    }

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let panel = scenario.bool("current") ? LocalAIPanelView() : LocalAIPanelView(styleSheet: styleSheet)
        if scenario.bool("current") { panel.styleSheet = styleSheet }
        let delegate = Delegate()
        panel.delegate = delegate
        self.delegate = delegate
        switch scenario.string("availability") {
        case "available": panel.availability = .available
        case "frameworkUnavailable": panel.availability = .frameworkUnavailable
        case "systemUnavailable": panel.availability = .systemUnavailable
        default: break
        }
        if scenario.bool("running") { panel.isRunning = true }
        if let raw = scenario.string("task"), let task = LocalAITask(rawValue: raw) {
            let source = try scenario.string("source") ?? scenario.documentText()
            let selection = scenario.array("selection").compactMap { ($0 as? NSNumber)?.intValue }
            let request = LocalAIRequest(
                task: task,
                source: source,
                selection: selection.count == 2 ? NSRange(location: selection[0], length: selection[1]) : nil
            )
            switch Self.runDeterministic(request) {
            case .success(let result): panel.result = result
            case .failure(let failure): error = "\(failure)"
            }
        }
        if scenario.bool("clearResult") { panel.result = nil }
        let popup = panel.subviews.compactMap { $0 as? NSPopUpButton }.first
        for index in scenario.array("pick").compactMap({ ($0 as? NSNumber)?.intValue }) {
            guard let popup else { break }
            popup.selectItem(at: index)
            popup.sendAction(popup.action, to: popup.target)
        }
        let buttons = panel.subviews.compactMap { $0 as? NSButton }.filter { !($0 is NSPopUpButton) }
        for name in scenario.strings("press") {
            let title = name == "apply" ? "Apply Preview" : "Close"
            guard let button = buttons.first(where: { $0.title == title }) else { continue }
            button.sendAction(button.action, to: button.target)
        }
        self.panel = panel
        return panel
    }

    func model() -> JSON {
        guard let panel else { return .null }
        return .object([
            ("preferredWidth", .double(Double(panel.preferredWidth))),
            ("isRunning", .bool(panel.isRunning)),
            ("hasResult", .bool(panel.result != nil)),
            ("resultText", .string(panel.result?.text)),
            ("hasPreview", .bool(panel.result?.preview != nil)),
            ("error", .string(error)),
            ("events", .array((delegate?.events ?? []).map { .string($0) })),
            ("fittingSize", PanelTree.size(panel.fittingSize)),
        ])
    }
}
