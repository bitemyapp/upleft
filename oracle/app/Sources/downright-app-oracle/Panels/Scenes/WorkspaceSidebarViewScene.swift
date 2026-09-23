import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `WorkspaceSidebarView` scenes over the sample workspace
/// (`state.workspace`, default `corpus/workspace`), indexed once per process
/// with `WorkspaceIndex` exactly as the `workspace` suite does
/// (`WorkspaceDump.index`). The process first moves to `/`, a launched app's
/// working directory: the sidebar groups files by `URL(fileURLWithPath:)` of
/// relative paths and the link graph resolves stems the same way.
///
/// The scene feeds the panel as the window controller does: `isScanning`
/// true, then (unless `state.stillScanning`) `isScanning` false, `entries`
/// (the first `state.entryLimit` if given), `searchResults = []`,
/// `selectedFileID` and `backlinks` for `state.currentFile` (a path relative
/// to the workspace). `state.error` instead ends the scan with that message.
/// Then `state.tab`, `state.search` (typed with `setSearchTextForTesting`;
/// the results of `WorkspaceSearch.search` are delivered unless
/// `state.searchPending`), `state.select` (a row, selected as a click would)
/// and `state.activate` (the table's action). `state.current` builds with
/// `WorkspaceSidebarView()` and assigns the sheet afterwards.
///
/// Reduce Motion: the tab strip's thumb moves without animation. A busy
/// state starts the activity indicator's one-second reveal, so captured
/// scenarios are never busy.
@MainActor
final class WorkspaceSidebarViewScene: PanelScene {
    private var sidebar: WorkspaceSidebarView?
    private var delegate: Delegate?

    private static var snapshots: [String: WorkspaceIndexSnapshot] = [:]

    private final class Delegate: WorkspaceSidebarViewDelegate {
        var events: [String] = []
        func workspaceSidebar(_ view: WorkspaceSidebarView, didSelect url: URL, range: NSRange?, inNewWindow: Bool) {
            let rangeText = range.map { "\($0.location) \($0.length)" } ?? "nil"
            events.append("select \(url.lastPathComponent) \(rangeText) \(inNewWindow)")
        }
        func workspaceSidebar(_ view: WorkspaceSidebarView, didSearch query: WorkspaceSearchQuery) {
            events.append("search \(query.text)")
        }
    }

    static func tab(_ name: String) -> WorkspaceSidebarView.WorkspaceSidebarTab? {
        switch name {
        case "files": return .files
        case "search": return .search
        case "backlinks": return .backlinks
        default: return nil
        }
    }

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let root = repositoryRoot.appendingPathComponent(scenario.string("workspace", "corpus/workspace")).standardizedFileURL
        guard FileManager.default.changeCurrentDirectoryPath("/") else {
            throw PanelHarnessError.usage("cannot move to /")
        }
        let snapshot: WorkspaceIndexSnapshot
        if let cached = Self.snapshots[root.path] {
            snapshot = cached
        } else {
            snapshot = try WorkspaceDump.index(root: root, policy: WorkspaceIndexPolicy())[1]
            Self.snapshots[root.path] = snapshot
        }

        let sidebar = scenario.bool("current") ? WorkspaceSidebarView() : WorkspaceSidebarView(styleSheet: styleSheet)
        if scenario.bool("current") { sidebar.styleSheet = styleSheet }
        let delegate = Delegate()
        sidebar.delegate = delegate
        self.delegate = delegate
        if let error = scenario.string("error") {
            sidebar.isScanning = false
            sidebar.errorMessage = error
        } else {
            sidebar.isScanning = true
            if !scenario.bool("stillScanning") {
                sidebar.isScanning = false
                let graph = WorkspaceLinkGraphBuilder.build(snapshot: snapshot)
                if let limit = scenario.int("entryLimit") {
                    sidebar.entries = Array(snapshot.entries.prefix(limit))
                } else {
                    sidebar.entries = snapshot.entries
                }
                sidebar.searchResults = []
                let currentID = scenario.string("currentFile").map { root.appendingPathComponent($0).standardizedFileURL.path }
                sidebar.selectedFileID = currentID
                sidebar.backlinks = currentID.map { graph.linksTo(fileID: $0) } ?? []
            }
        }
        if let name = scenario.string("tab"), let tab = Self.tab(name) { sidebar.selectedTab = tab }
        if let text = scenario.string("search") {
            sidebar.setSearchTextForTesting(text)
            if !scenario.bool("searchPending") {
                sidebar.searchResults = WorkspaceSearch.search(WorkspaceSearchQuery(text: text), in: snapshot)
            }
        }
        let table = sidebar.subviews.compactMap { ($0 as? NSScrollView)?.documentView as? NSTableView }.first
        if let row = scenario.int("select"), let table {
            table.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
        }
        if scenario.bool("activate"), let table {
            table.sendAction(table.action, to: table.target)
        }
        self.sidebar = sidebar
        return sidebar
    }

    func model() -> JSON {
        guard let sidebar else { return .null }
        return .object([
            ("preferredWidth", .double(Double(sidebar.preferredWidth))),
            ("selectedTab", .string(sidebar.selectedTab.rawValue)),
            ("entryCount", .int(sidebar.entries.count)),
            ("searchResultCount", .int(sidebar.searchResults.count)),
            ("backlinkCount", .int(sidebar.backlinks.count)),
            ("isScanning", .bool(sidebar.isScanning)),
            ("events", .array((delegate?.events ?? []).map { .string($0) })),
            ("fittingSize", PanelTree.size(sidebar.fittingSize)),
        ])
    }
}
