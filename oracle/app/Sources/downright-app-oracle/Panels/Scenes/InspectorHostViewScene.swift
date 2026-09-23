import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `InspectorHostView` scenes: the host with plain content views installed
/// for `state.sections` (tasks, history, context, search), then `select`,
/// `titles` (section → title) and `remove` applied in that order.
@MainActor
final class InspectorHostViewScene: PanelScene {
    private var host: InspectorHostView?
    private var selections: [String] = []

    static func section(_ name: String) -> InspectorSection? {
        switch name {
        case "tasks": return .tasks
        case "history": return .history
        case "context": return .context
        case "search": return .search
        default: return nil
        }
    }

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let host = InspectorHostView(frame: NSRect(x: 0, y: 0, width: scenario.width, height: scenario.height))
        host.styleSheet = styleSheet
        host.onSelectionChange = { [weak self] section in self?.selections.append(section?.title ?? "nil") }
        for name in scenario.strings("sections") {
            guard let section = Self.section(name) else { continue }
            let content = NSTextField(labelWithString: "\(section.title) content")
            content.textColor = styleSheet.text
            host.setContent(content, section: section)
        }
        let titles = scenario.object("titles")
        for name in titles.keys.sorted() {
            guard let section = Self.section(name), let title = titles[name] as? String else { continue }
            host.setTitle(title, for: section)
        }
        if let select = scenario.string("select"), let section = Self.section(select) { host.select(section) }
        for name in scenario.strings("remove") {
            guard let section = Self.section(name) else { continue }
            host.removeContent(section: section)
        }
        self.host = host
        return host
    }

    func model() -> JSON {
        guard let host else { return .null }
        return .object([
            ("selectedSection", .string(host.selectedSection?.title)),
            ("contentCount", .int(host.contentCount)),
            ("hasContent", .bool(host.hasContent)),
            ("floatingFittingHeight", .double(Double(host.floatingFittingHeight))),
            ("closeButtonFrame", PanelTree.rect(host.closeButtonForTesting.frame)),
            ("selections", .array(selections.map { .string($0) })),
        ])
    }
}
