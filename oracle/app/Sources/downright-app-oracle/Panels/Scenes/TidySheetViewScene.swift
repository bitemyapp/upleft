import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `TidySheetView` scenes, built as `presentTidySheet` builds the sheet:
/// `TidySheetView()` + `styleSheet` (`state.current`) or
/// `init(styleSheet:)`, then `proposals` from `TidyDocument.plan` over
/// `state.text` or the scenario's document (`state.rules`, raw values,
/// restricts the rules; `state.limit` keeps the first edits), the delegate,
/// and `reload()`. Then, with the panel laid out at the scenario size:
///
/// - `toggles`: table rows whose checkbox is pressed
///   (`accessibilityPerformPress`);
/// - `expand`: table rows whose expand button's action is sent;
/// - `buttons`: footer buttons (by title, or title prefix) whose action is
///   sent.
///
/// `state.displayTexts` are passed through `TidySheetView.displayText` for
/// the model. Reduce Motion: the checkboxes change state without animating.
@MainActor
final class TidySheetViewScene: PanelScene {
    private var sheet: TidySheetView?
    private let recorder = TidySheetSceneRecorder()
    private var displayTexts: [String] = []

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let text = try scenario.string("text") ?? scenario.documentText()
        let parsed = MarkdownParser.parse(text)
        let names = scenario.strings("rules")
        var edits = names.isEmpty
            ? TidyDocument.plan(parsed)
            : TidyDocument.plan(parsed, rules: Set(names.compactMap { TidyRule(rawValue: $0) }))
        if let limit = scenario.int("limit") { edits = Array(edits.prefix(limit)) }

        let sheet: TidySheetView
        if scenario.bool("current") {
            sheet = TidySheetView()
            sheet.styleSheet = styleSheet
        } else {
            sheet = TidySheetView(styleSheet: styleSheet)
        }
        sheet.proposals = edits.map { edit in
            (edit: edit,
             before: (text as NSString).substring(with: edit.range),
             after: edit.replacement)
        }
        sheet.delegate = recorder
        sheet.reload()

        let toggles = scenario.array("toggles").compactMap { ($0 as? NSNumber)?.intValue }
        let expand = scenario.array("expand").compactMap { ($0 as? NSNumber)?.intValue }
        let buttons = scenario.strings("buttons")
        if !toggles.isEmpty || !expand.isEmpty || !buttons.isEmpty {
            sheet.frame = NSRect(x: 0, y: 0, width: scenario.width, height: scenario.height)
            sheet.layoutSubtreeIfNeeded()
        }
        let table = TableEditorViewScene.descendants(of: sheet, as: NSTableView.self).first
        for row in toggles {
            guard let table, row >= 0, row < table.numberOfRows,
                  let view = table.view(atColumn: 0, row: row, makeIfNecessary: true),
                  let checkbox = TableEditorViewScene.descendants(of: view, as: PanelCheckbox.self).first
            else { continue }
            _ = checkbox.accessibilityPerformPress()
        }
        for row in expand {
            guard let table, row >= 0, row < table.numberOfRows,
                  let view = table.view(atColumn: 0, row: row, makeIfNecessary: true),
                  let button = TableEditorViewScene.descendants(of: view, as: NSButton.self).first,
                  let action = button.action
            else { continue }
            NSApp.sendAction(action, to: button.target, from: button)
        }
        for title in buttons {
            guard let button = TableEditorViewScene.descendants(of: sheet, as: NSButton.self)
                .first(where: { $0.title == title || $0.title.hasPrefix(title) }),
                  let action = button.action
            else { continue }
            NSApp.sendAction(action, to: button.target, from: button)
        }
        displayTexts = scenario.strings("displayTexts")
        self.sheet = sheet
        return sheet
    }

    func model() -> JSON {
        guard let sheet else { return .null }
        let table = TableEditorViewScene.descendants(of: sheet, as: NSTableView.self).first
        return .object([
            ("tableRows", .int(table?.numberOfRows ?? -1)),
            ("proposals", .array(sheet.proposals.map { proposal in
                .object([
                    ("summary", .string(proposal.edit.summary)),
                    ("rule", .string(proposal.edit.rule?.rawValue)),
                    ("range", .range(proposal.edit.range)),
                    ("before", .string(TidySheetView.displayText(proposal.before))),
                    ("after", .string(TidySheetView.displayText(proposal.after))),
                ])
            })),
            ("applied", .array(recorder.applied.map { edits in .array(edits.map { .string($0.summary) }) })),
            ("cancelled", .int(recorder.cancelled)),
            ("displayTexts", .array(displayTexts.map { .string(TidySheetView.displayText($0)) })),
        ])
    }
}

@MainActor
private final class TidySheetSceneRecorder: TidySheetDelegate {
    var applied: [[TextEdit]] = []
    var cancelled = 0

    func tidySheet(_ sheet: TidySheetView, didApply edits: [TextEdit]) { applied.append(edits) }
    func tidySheetDidCancel(_ sheet: TidySheetView) { cancelled += 1 }
}
