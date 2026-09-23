import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `TableEditorView` scenes. The document is `state.text` (inline) or the
/// scenario's document; the editor is built for `state.tableIndex` (default
/// 0), or with `TableEditorView(document:)` + `styleSheet` when
/// `state.current` is set. Then, in this order:
///
/// - `select`: `[row, column]` → `select(row:column:)`;
/// - `operations`: each `{"op": …}` → `apply(operation:)`. With
///   `state.host`, the recorder is the host: it applies each proposal to the
///   text, reparses and calls `update(document:)`, as
///   `DocumentWindowController` does;
/// - `actions`: Objective-C action selectors (`addRow:`, `deleteRow:`,
///   `cellClicked:`, `requestSource:`, `cancel:`, `finish:`, …) performed
///   with a nil sender;
/// - `alignment`: an index selected in the alignment pop-up, whose action is
///   then sent from it;
/// - `update`: replacement text → `update(document:)`;
/// - `reload`: `reload()`.
///
/// The recorder claims the finish/cancel callbacks (recording them), so no
/// window is ever closed. Reduce Motion: the panel does not animate.
@MainActor
final class TableEditorViewScene: PanelScene {
    private var editor: TableEditorView?
    private let recorder = TableEditorSceneRecorder()

    static func operation(_ object: [String: Any]) -> TableEditOperation? {
        func int(_ key: String) -> Int { (object[key] as? NSNumber)?.intValue ?? 0 }
        func string(_ key: String) -> String { object[key] as? String ?? "" }
        func strings(_ key: String) -> [String] { (object[key] as? [Any] ?? []).compactMap { $0 as? String } }
        switch object["op"] as? String {
        case "setCell": return .setCell(row: int("row"), column: int("column"), text: string("text"))
        case "setAlignment":
            let alignment: TableAlignment
            switch string("alignment") {
            case "left": alignment = .left
            case "center": alignment = .center
            case "right": alignment = .right
            default: alignment = .none
            }
            return .setAlignment(column: int("column"), alignment: alignment)
        case "insertRow": return .insertRow(index: int("index"), cells: strings("cells"))
        case "deleteRow": return .deleteRow(index: int("index"))
        case "moveRow": return .moveRow(from: int("from"), to: int("to"))
        case "insertColumn": return .insertColumn(index: int("index"), header: string("header"), cells: strings("cells"))
        case "deleteColumn": return .deleteColumn(index: int("index"))
        case "moveColumn": return .moveColumn(from: int("from"), to: int("to"))
        default: return nil
        }
    }

    static func descendants<T: NSView>(of view: NSView, as type: T.Type) -> [T] {
        var found: [T] = []
        for child in view.subviews {
            if let match = child as? T { found.append(match) }
            found.append(contentsOf: descendants(of: child, as: type))
        }
        return found
    }

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let text = try scenario.string("text") ?? scenario.documentText()
        recorder.text = text
        recorder.host = scenario.bool("host")
        let document = MarkdownParser.parse(text)
        let editor: TableEditorView
        if scenario.bool("current") {
            editor = TableEditorView(document: document)
            editor.styleSheet = styleSheet
        } else {
            editor = TableEditorView(document: document, tableIndex: scenario.int("tableIndex", 0), styleSheet: styleSheet)
        }
        editor.delegate = recorder
        let select = scenario.array("select").compactMap { ($0 as? NSNumber)?.intValue }
        if select.count == 2 { editor.select(row: select[0], column: select[1]) }
        for case let object as [String: Any] in scenario.array("operations") {
            guard let operation = Self.operation(object) else { continue }
            editor.apply(operation: operation)
        }
        for name in scenario.strings("actions") {
            _ = editor.perform(NSSelectorFromString(name), with: nil)
        }
        if let index = scenario.int("alignment"),
           let popup = Self.descendants(of: editor, as: NSPopUpButton.self).first,
           let action = popup.action {
            popup.selectItem(at: index)
            NSApp.sendAction(action, to: popup.target, from: popup)
        }
        if let update = scenario.string("update") {
            recorder.text = update
            editor.update(document: MarkdownParser.parse(update))
        }
        if scenario.bool("reload") { editor.reload() }
        cellInteractions(editor, scenario)
        self.editor = editor
        return editor
    }

    /// With the editor laid out at the scenario size: `beginEditing`
    /// (`[row, column]`) and `endEditing` (`{"row", "column", "text"}`) post
    /// the control's text notifications for a cell (its delegate, the
    /// editor, observes them); `keys` (`{"row", "column", "keyCode",
    /// "shift"}`) send a Tab (48) or Return (36, 76) key-down to a cell.
    private func cellInteractions(_ editor: TableEditorView, _ scenario: PanelScenario) {
        let begin = scenario.array("beginEditing").compactMap { $0 as? [Any] }
        let end = scenario.array("endEditing").compactMap { $0 as? [String: Any] }
        let keys = scenario.array("keys").compactMap { $0 as? [String: Any] }
        guard !begin.isEmpty || !end.isEmpty || !keys.isEmpty else { return }
        editor.frame = NSRect(x: 0, y: 0, width: scenario.width, height: scenario.height)
        editor.layoutSubtreeIfNeeded()
        guard let table = Self.descendants(of: editor, as: NSTableView.self).first else { return }
        func cell(_ row: Int, _ column: Int) -> NSTextField? {
            guard row >= 0, row < table.numberOfRows, column >= 0, column < table.numberOfColumns else { return nil }
            return table.view(atColumn: column, row: row, makeIfNecessary: true) as? NSTextField
        }
        func int(_ value: Any?) -> Int { (value as? NSNumber)?.intValue ?? 0 }
        for pair in begin where pair.count == 2 {
            guard let field = cell(int(pair[0]), int(pair[1])) else { continue }
            NotificationCenter.default.post(name: NSControl.textDidBeginEditingNotification, object: field)
        }
        for edit in end {
            guard let field = cell(int(edit["row"]), int(edit["column"])) else { continue }
            field.stringValue = edit["text"] as? String ?? ""
            NotificationCenter.default.post(name: NSControl.textDidEndEditingNotification, object: field)
        }
        for key in keys {
            guard let field = cell(int(key["row"]), int(key["column"])) else { continue }
            let code = UInt16(int(key["keyCode"]))
            let characters = code == 48 ? "\t" : "\r"
            guard let event = NSEvent.keyEvent(
                with: .keyDown, location: .zero,
                modifierFlags: (key["shift"] as? Bool ?? false) ? [.shift] : [],
                timestamp: 0, windowNumber: 0, context: nil,
                characters: characters, charactersIgnoringModifiers: characters,
                isARepeat: false, keyCode: code
            ) else { continue }
            field.keyDown(with: event)
        }
    }

    func model() -> JSON {
        guard let editor else { return .null }
        let table = Self.descendants(of: editor, as: NSTableView.self).first
        let columns: [JSON] = (table?.tableColumns ?? []).map { column in
            .object([
                ("identifier", .string(column.identifier.rawValue)),
                ("title", .string(column.title)),
                ("width", .double(Double(column.width))),
                ("minWidth", .double(Double(column.minWidth))),
            ])
        }
        return .object([
            ("rowCount", .int(editor.rowCountForTesting)),
            ("columnCount", .int(editor.columnCountForTesting)),
            ("sourceRange", .range(editor.sourceRangeForTesting)),
            ("appliedEditCount", .int(editor.appliedEditCount)),
            ("tableIndex", .int(editor.tableIndex)),
            ("preferredWidth", .double(Double(editor.preferredWidth))),
            ("selectedRow", .int(table?.selectedRow ?? -2)),
            ("columns", .array(columns)),
            ("proposals", .array(recorder.proposals.map { proposal in
                .object([
                    ("summary", .string(proposal.summary)),
                    ("range", .range(proposal.range)),
                    ("replacement", .string(proposal.replacement)),
                    ("expected", .string(proposal.expected)),
                ])
            })),
            ("sourceRequests", .array(recorder.sourceRequests.map { .range($0) })),
            ("finished", .int(recorder.finished)),
            ("cancelled", .int(recorder.cancelled)),
            ("text", .string(recorder.text)),
        ])
    }
}

@MainActor
private final class TableEditorSceneRecorder: TableEditorDelegate {
    var text = ""
    var host = false
    var proposals: [TableEditProposal] = []
    var sourceRequests: [NSRange] = []
    var finished = 0
    var cancelled = 0

    func tableEditor(_ editor: TableEditorView, didApply proposal: TableEditProposal) {
        proposals.append(proposal)
        guard host, let next = proposal.applying(to: text) else { return }
        text = next
        editor.update(document: MarkdownParser.parse(text))
    }

    func tableEditor(_ editor: TableEditorView, didRequestSource range: NSRange) {
        sourceRequests.append(range)
    }

    func tableEditorDidFinish(_ editor: TableEditorView) { finished += 1 }
    func tableEditorDidCancel(_ editor: TableEditorView) { cancelled += 1 }
}
