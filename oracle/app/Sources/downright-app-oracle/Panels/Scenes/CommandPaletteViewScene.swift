import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `CommandPaletteView` scenes.
///
/// The palette gets an in-memory recents store (`recents` seeds it) and,
/// when the scenario has a document, a `CurrentDocumentQuickOpenProvider`
/// over it. With `commands` (raw values) the scene passes its own model
/// (default key bindings); otherwise the palette builds its default model
/// (every command, `KeybindingStore.shared`'s bindings, which the sandboxed
/// home leaves at their defaults).
///
/// State, applied in this order:
///   current      build with `CommandPaletteView()` and assign the style sheet
///                (no document, the user-defaults store)
///   query        typed: the field's string, then `controlTextDidChange`
///   chip         click the filter chip at this index
///   select       select this table row, as a click does
///   doubleClick  the table's double action
///   cancel       `cancelOperation(nil)`
///   restyle      assign the style sheet again
///   keys         (windowed only) key codes sent through `NSApp.sendEvent`
///                after the window is shown
///
/// The palette has no animation of its own; Reduce Motion changes nothing.
@MainActor
final class CommandPaletteViewScene: PanelScene, CommandPaletteViewDelegate {
    final class MemoryRecentStore: CommandPaletteRecentStore {
        var values: [Command] = []
        func recentCommands() -> [Command] { values }
        func record(_ command: Command) {
            values.removeAll { $0 == command }
            values.insert(command, at: 0)
        }
    }

    private var palette: CommandPaletteView?
    private let store = MemoryRecentStore()
    private var chosen: [String] = []
    private var cancels = 0
    private var keyCodes: [UInt16] = []

    func commandPalette(_ palette: CommandPaletteView, didChoose result: QuickOpenResult) {
        chosen.append(result.id)
    }

    func commandPaletteDidCancel(_ palette: CommandPaletteView) {
        cancels += 1
    }

    private var searchField: NSSearchField? {
        palette?.subviews.compactMap { $0 as? NSSearchField }.first
    }

    private var tableView: NSTableView? {
        palette?.subviews.compactMap { ($0 as? NSScrollView)?.documentView as? NSTableView }.first
    }

    private var emptyState: PanelEmptyStateView? {
        palette?.subviews.compactMap { $0 as? PanelEmptyStateView }.first
    }

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        store.values = scenario.strings("recents").compactMap(Command.init(rawValue:))
        var providers: [any QuickOpenProvider] = []
        if scenario.documentPath != nil {
            providers.append(CurrentDocumentQuickOpenProvider(document: MarkdownParser.parse(try scenario.documentText())))
        }
        let palette: CommandPaletteView
        if scenario.bool("current") {
            palette = CommandPaletteView()
            palette.styleSheet = styleSheet
        } else {
            var model: CommandPaletteModel?
            let commands = scenario.strings("commands").compactMap(Command.init(rawValue:))
            if !commands.isEmpty {
                model = CommandPaletteModel(
                    commands: commands,
                    bindings: { KeybindingDefaults.table[$0] ?? [] },
                    recentCommands: store.recentCommands(),
                    providers: providers
                )
            }
            palette = CommandPaletteView(styleSheet: styleSheet, recentStore: store, model: model, providers: providers)
        }
        palette.delegate = self
        self.palette = palette
        keyCodes = scenario.array("keys").compactMap { ($0 as? NSNumber)?.uint16Value }

        if let query = scenario.string("query"), let field = searchField {
            field.stringValue = query
            palette.controlTextDidChange(Notification(name: NSControl.textDidChangeNotification, object: field))
        }
        if let chip = scenario.int("chip"),
           let stack = palette.subviews.compactMap({ $0 as? NSStackView }).first,
           stack.arrangedSubviews.indices.contains(chip),
           let button = stack.arrangedSubviews[chip] as? NSButton {
            button.performClick(nil)
        }
        if let row = scenario.int("select"), let table = tableView {
            table.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
        }
        if scenario.bool("doubleClick"), let table = tableView {
            palette.perform(NSSelectorFromString("doubleClick:"), with: table)
        }
        if scenario.bool("cancel") { palette.cancelOperation(nil) }
        if scenario.bool("restyle") { palette.styleSheet = styleSheet }
        return palette
    }

    func afterShow(window: NSWindow, scenario: PanelScenario) {
        for keyCode in keyCodes {
            guard let event = NSEvent.keyEvent(
                with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0,
                windowNumber: window.windowNumber, context: nil, characters: "",
                charactersIgnoringModifiers: "", isARepeat: false, keyCode: keyCode
            ) else { continue }
            NSApp.sendEvent(event)
        }
    }

    func model() -> JSON {
        guard let palette else { return .null }
        return .object([
            ("preferredWidth", .double(Double(palette.preferredWidth))),
            ("query", .string(searchField?.stringValue)),
            ("rows", .int(tableView?.numberOfRows ?? -1)),
            ("selectedRow", .int(tableView?.selectedRow ?? -1)),
            ("tableValue", .string(tableView?.accessibilityValue() as? String)),
            ("emptyTitle", .string(emptyState?.title)),
            ("emptySubtitle", .string(emptyState?.subtitle)),
            ("emptyHidden", .bool(emptyState?.isHidden ?? true)),
            ("chosen", .array(chosen.map { .string($0) })),
            ("cancels", .int(cancels)),
            ("recents", .array(store.values.map { .string($0.rawValue) })),
        ])
    }
}
