import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `TaskPanelView` scenes: the panel as `DocumentWindowController` builds it
/// (`TaskPanelView()`, then `styleSheet`, `tasks`, `headings` and `reload()`),
/// with the scenario document's parsed tasks (none without a document), a
/// recording delegate, then `state.actions` applied in order. Each action is
/// `[name, argument?]`:
///
///   expandPile n / collapsePile n   setCompletedPileExpandedForTesting(_:section:)
///   select row / deselect           the task table's selection
///   key code                        the task table's keyDown(with:) (49 space,
///                                   36 return, 123 left, 124 right)
///   commandN                        the panel's performKeyEquivalent(with:) ⌘N
///   undo title / dismissUndo        presentUndoForTesting / dismissUndoForTesting
///   beginNewTask                    beginNewTaskForCommand()
///   commit text                     commitNewTaskForTesting(_:)
///   cancel                          cancelOperation(nil)
///   scrollRow row                   the task table's scrollRowToVisible(_:)
///   reload                          reload()
///
/// Hosting the panel in a window can reset its sheet to `StyleSheet.current`
/// (`viewDidChangeEffectiveAppearance`), so `afterShow` assigns the scenario's
/// sheet again, as the window controller does on a theme change.
///
/// Reduce Motion (forced on by the harness): every row rebuild is a plain
/// `reloadData`, the empty state, the quick-add field and the undo pill appear
/// without fades, completion moments and row glows do nothing, and the
/// section bar places its segments without a transaction animation, so the
/// scene is still once laid out. The undo pill's four-second dismiss timer is
/// wall-clock; undo scenes use small documents that settle well inside it.
@MainActor
final class TaskPanelViewScene: PanelScene {
    private final class Recorder: TaskPanelDelegate {
        var calls: [String] = []

        func taskPanel(_ panel: TaskPanelView, didToggleTaskAt markOffset: Int) {
            calls.append("toggle \(markOffset)")
        }

        func taskPanel(_ panel: TaskPanelView, didSelectTaskAt contentOffset: Int) {
            calls.append("select \(contentOffset)")
        }

        func taskPanel(_ panel: TaskPanelView, didRequestNewTask text: String, headingIndex: Int?) {
            calls.append("new \(text) \(headingIndex.map { String($0) } ?? "nil")")
        }

        func taskPanel(_ panel: TaskPanelView, didMoveTask taskIndex: Int, before targetIndex: Int?) {
            calls.append("move \(taskIndex) \(targetIndex.map { String($0) } ?? "nil")")
        }
    }

    private var panel: TaskPanelView?
    private var styleSheet: StyleSheet?
    private let recorder = Recorder()
    private var contentSizeChanges = 0

    static func keyEvent(_ code: UInt16, command: Bool = false, characters: String = "") -> NSEvent {
        NSEvent.keyEvent(
            with: .keyDown,
            location: .zero,
            modifierFlags: command ? [.command] : [],
            timestamp: 0,
            windowNumber: 0,
            context: nil,
            characters: characters,
            charactersIgnoringModifiers: characters,
            isARepeat: false,
            keyCode: code
        )!
    }

    /// The panel's task table: the document view of its scroll view.
    static func table(in panel: NSView) -> PanelTableView? {
        for view in panel.subviews {
            if let scroll = view as? NSScrollView, let table = scroll.documentView as? PanelTableView {
                return table
            }
        }
        return nil
    }

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let document = MarkdownParser.parse(try scenario.documentText())
        let panel = TaskPanelView()
        panel.delegate = recorder
        panel.styleSheet = styleSheet
        panel.onContentSizeChange = { [weak self] in self?.contentSizeChanges += 1 }
        panel.tasks = document.tasks
        panel.headings = document.headings
        panel.reload()
        let table = Self.table(in: panel)
        for case let action as [Any] in scenario.array("actions") {
            guard let name = action.first as? String else { continue }
            let number = action.count > 1 ? (action[1] as? NSNumber)?.intValue ?? 0 : 0
            let text = action.count > 1 ? action[1] as? String ?? "" : ""
            switch name {
            case "expandPile": panel.setCompletedPileExpandedForTesting(true, section: number)
            case "collapsePile": panel.setCompletedPileExpandedForTesting(false, section: number)
            case "select": table?.selectRowIndexes(IndexSet(integer: number), byExtendingSelection: false)
            case "deselect": table?.deselectAll(nil)
            case "key": table?.keyDown(with: Self.keyEvent(UInt16(number)))
            case "commandN": _ = panel.performKeyEquivalent(with: Self.keyEvent(45, command: true, characters: "n"))
            case "undo": panel.presentUndoForTesting(title: text)
            case "dismissUndo": panel.dismissUndoForTesting()
            case "beginNewTask": panel.beginNewTaskForCommand()
            case "commit": panel.commitNewTaskForTesting(text)
            case "cancel": panel.cancelOperation(nil)
            case "scrollRow": table?.scrollRowToVisible(number)
            case "reload": panel.reload()
            default: break
            }
        }
        self.panel = panel
        self.styleSheet = styleSheet
        Self.benchmark(document, scenario: scenario, styleSheet: styleSheet)
        return panel
    }

    /// `UPLEFT_PANEL_BENCH=N`: build, size, lay out and measure a fresh panel
    /// N times (parsing excluded) and report the times on stderr. Off by
    /// default; the dump is unaffected.
    static func benchmark(_ document: ParsedDocument, scenario: PanelScenario, styleSheet: StyleSheet) {
        guard let value = ProcessInfo.processInfo.environment["UPLEFT_PANEL_BENCH"], let runs = Int(value), runs > 0
        else { return }
        var times: [Double] = []
        for _ in 0..<runs {
            let start = DispatchTime.now().uptimeNanoseconds
            let panel = TaskPanelView()
            panel.styleSheet = styleSheet
            panel.tasks = document.tasks
            panel.headings = document.headings
            panel.reload()
            panel.frame = NSRect(x: 0, y: 0, width: scenario.width, height: scenario.height)
            panel.layoutSubtreeIfNeeded()
            _ = panel.fittedContentHeight
            times.append(Double(DispatchTime.now().uptimeNanoseconds - start) / 1e6)
        }
        times.sort()
        let line = String(format: "bench TaskPanelView swift: %d tasks, min %.2f ms, median %.2f ms\n",
                          document.tasks.count, times[0], times[times.count / 2])
        FileHandle.standardError.write(line.data(using: .utf8)!)
    }

    func afterShow(window: NSWindow, scenario: PanelScenario) {
        guard let panel, let styleSheet else { return }
        panel.styleSheet = styleSheet
    }

    func model() -> JSON {
        guard let panel else { return .null }
        let progress = panel.progress
        return .object([
            ("theme", .string(panel.styleSheet.theme.name)),
            ("statusLine", .string(panel.statusLineForTesting)),
            ("caption", .string(panel.captionForTesting)),
            ("progress", .array([.int(progress.done), .int(progress.total)])),
            ("preferredWidth", .double(Double(panel.preferredWidth))),
            ("rowCount", .int(panel.rowCountForTesting)),
            ("visibleTaskCount", .int(panel.visibleTaskCountForTesting)),
            ("pileRowCount", .int(panel.pileRowCountForTesting)),
            ("quickAddEditing", .bool(panel.quickAddEditingForTesting)),
            ("measuredListHeight", .double(Double(panel.measuredListHeightForTesting))),
            ("contentDocumentHeight", .double(Double(panel.contentDocumentHeightForTesting))),
            ("contentViewportHeight", .double(Double(panel.contentViewportHeightForTesting))),
            ("undoBottomInset", .double(Double(panel.undoBottomInsetForTesting))),
            ("undoRequiredBottomInset", .double(Double(panel.undoRequiredBottomInsetForTesting))),
            ("undoPillFrame", PanelTree.rect(panel.undoPillFrameForTesting)),
            ("lastRowFrame", panel.lastRowFrameForTesting.map { PanelTree.rect($0) } ?? .null),
            ("emptyAddButtonTitle", .string(panel.emptyAddButtonForTesting.title)),
            ("accessibilityValue", .string(panel.accessibilityValue() as? String)),
            ("delegateCalls", .array(recorder.calls.map { .string($0) })),
            ("contentSizeChanges", .int(contentSizeChanges)),
            ("fittedContentHeight", .double(Double(panel.fittedContentHeight))),
        ])
    }
}
