import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `FrontMatterEditorView` scenes, built as `showFrontMatterEditor` builds
/// the panel: `FrontMatterEditorView()` + `styleSheet` (`state.current`) or
/// `init(styleSheet:)`, the delegate, then `document` from `state.text` or
/// the scenario's document (twice with `state.redocument`). Then, in order:
///
/// - `scrollOriginY` → `setFieldScrollOriginYForTesting`;
/// - `edits`: `{"key", "value"?, "kind"?, "send": "value" | "kind"}` — the
///   row's value field and kind pop-up are set, and the chosen control's
///   action is sent (the row commits);
/// - `remove`: keys whose row's remove button action is sent;
/// - `add`: `{"key", "value", "kind"}` typed into the add form, then the Add
///   button's action is sent;
/// - `sourceMode`: the Source Focus button's action is sent;
/// - `prepare`: `prepareForPresentation(focus:)` (`""` means nil);
/// - `focusSelector`: `focusField()` (the selector InspectorHostView sends).
///
/// With `state.host` the recorder is the host (proposal applied, reparsed,
/// `document` reassigned). After the window is shown, `state.focus`
/// (`""` means nil) runs `prepareForPresentation(focus:)`, after
/// `state.scrollAfterShow` → `setFieldScrollOriginYForTesting`.
/// Reduce Motion: the panel does not animate.
@MainActor
final class FrontMatterEditorViewScene: PanelScene {
    private var editor: FrontMatterEditorView?
    private let recorder = FrontMatterSceneRecorder()

    static func value(_ value: FrontMatterValue) -> JSON {
        switch value {
        case .text(let text): return .object([("kind", .string("text")), ("value", .string(text))])
        case .boolean(let flag): return .object([("kind", .string("boolean")), ("value", .bool(flag))])
        case .number(let number): return .object([("kind", .string("number")), ("value", .double(number))])
        case .list(let items): return .object([("kind", .string("list")), ("value", .array(items.map { .string($0) }))])
        }
    }

    static func operation(_ operation: FrontMatterEditOperation) -> JSON {
        switch operation {
        case .set(let key, let value): return .object([("op", .string("set")), ("key", .string(key)), ("value", Self.value(value))])
        case .add(let key, let value): return .object([("op", .string("add")), ("key", .string(key)), ("value", Self.value(value))])
        case .remove(let key): return .object([("op", .string("remove")), ("key", .string(key))])
        }
    }

    static func send(_ control: NSControl) {
        guard let action = control.action else { return }
        NSApp.sendAction(action, to: control.target, from: control)
    }

    /// The row for `key` (`FrontMatterFieldRow` is private; its
    /// accessibility label names it).
    static func row(_ key: String, in editor: NSView) -> NSView? {
        TableEditorViewScene.descendants(of: editor, as: NSView.self)
            .first { $0.accessibilityLabel() == "Front matter field \(key)" }
    }

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let text = try scenario.string("text") ?? scenario.documentText()
        recorder.text = text
        recorder.host = scenario.bool("host")
        let editor: FrontMatterEditorView
        if scenario.bool("current") {
            editor = FrontMatterEditorView()
            editor.styleSheet = styleSheet
        } else {
            editor = FrontMatterEditorView(styleSheet: styleSheet)
        }
        editor.delegate = recorder
        editor.document = MarkdownParser.parse(text)
        if scenario.bool("redocument") { editor.document = MarkdownParser.parse(text) }

        if let y = scenario.double("scrollOriginY") { editor.setFieldScrollOriginYForTesting(CGFloat(y)) }
        for case let edit as [String: Any] in scenario.array("edits") {
            guard let key = edit["key"] as? String, let row = Self.row(key, in: editor) else { continue }
            let field = TableEditorViewScene.descendants(of: row, as: NSTextField.self)
                .first { $0.accessibilityLabel() == "Value for \(key)" }
            let popup = TableEditorViewScene.descendants(of: row, as: NSPopUpButton.self).first
            if let value = edit["value"] as? String { field?.stringValue = value }
            if let kind = (edit["kind"] as? NSNumber)?.intValue { popup?.selectItem(at: kind) }
            if edit["send"] as? String == "kind" {
                if let popup { Self.send(popup) }
            } else if let field {
                Self.send(field)
            }
        }
        for key in scenario.strings("remove") {
            guard let row = Self.row(key, in: editor),
                  let button = TableEditorViewScene.descendants(of: row, as: NSButton.self)
                    .first(where: { $0.accessibilityLabel() == "Remove field" })
            else { continue }
            Self.send(button)
        }
        let add = scenario.object("add")
        if !add.isEmpty {
            let fields = TableEditorViewScene.descendants(of: editor, as: NSTextField.self)
            fields.first { $0.placeholderString == "Field name" }?.stringValue = add["key"] as? String ?? ""
            fields.first { $0.placeholderString == "Value" }?.stringValue = add["value"] as? String ?? ""
            if let kind = (add["kind"] as? NSNumber)?.intValue {
                TableEditorViewScene.descendants(of: editor, as: NSPopUpButton.self)
                    .first { $0.accessibilityLabel() == "New field type" }?.selectItem(at: kind)
            }
            if let button = TableEditorViewScene.descendants(of: editor, as: NSButton.self)
                .first(where: { $0.title == "Add field" }) {
                Self.send(button)
            }
        }
        if scenario.bool("sourceMode"),
           let button = TableEditorViewScene.descendants(of: editor, as: NSButton.self)
            .first(where: { $0.title == "Open Source Focus" }) {
            Self.send(button)
        }
        if let prepare = scenario.string("prepare") {
            editor.prepareForPresentation(focus: prepare.isEmpty ? nil : prepare)
        }
        if scenario.bool("focusSelector") { editor.focusField() }
        self.editor = editor
        return editor
    }

    func afterShow(window: NSWindow, scenario: PanelScenario) {
        guard let editor else { return }
        if let y = scenario.double("scrollAfterShow") { editor.setFieldScrollOriginYForTesting(CGFloat(y)) }
        if let focus = scenario.string("focus") { editor.prepareForPresentation(focus: focus.isEmpty ? nil : focus) }
    }

    func model() -> JSON {
        guard let editor else { return .null }
        return .object([
            ("renderedFieldCount", .int(editor.renderedFieldCount)),
            ("showsSourceModePrompt", .bool(editor.showsSourceModePrompt)),
            ("fieldScrollOriginY", .double(Double(editor.fieldScrollOriginYForTesting))),
            ("focusedFieldKey", .string(editor.focusedFieldKeyForTesting)),
            ("preferredWidth", .double(Double(editor.preferredWidth))),
            ("requests", .array(recorder.requests.map(Self.operation))),
            ("sourceModeRequests", .int(recorder.sourceModeRequests)),
            ("text", .string(recorder.text)),
        ])
    }
}

@MainActor
private final class FrontMatterSceneRecorder: FrontMatterEditorDelegate {
    var text = ""
    var host = false
    var requests: [FrontMatterEditOperation] = []
    var sourceModeRequests = 0

    func frontMatterEditor(_ editor: FrontMatterEditorView, didRequest operation: FrontMatterEditOperation) {
        requests.append(operation)
        guard host else { return }
        let parsed = MarkdownParser.parse(text)
        let result = FrontMatterEditing.propose(parsed, operation: operation)
        guard let proposal = result.proposal, let next = proposal.applying(to: text) else {
            editor.document = parsed
            return
        }
        text = next
        editor.document = MarkdownParser.parse(text)
    }

    func frontMatterEditorWantsSourceMode(_ editor: FrontMatterEditorView) {
        sourceModeRequests += 1
    }
}
