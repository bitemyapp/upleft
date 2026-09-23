import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `SearchResultsPanelView` scenes. State, applied in this order:
///
/// - `current`: build with `SearchResultsPanelView()` and assign the style
///   sheet afterwards;
/// - the search, as the window controller runs it: the sibling files are
///   `files` (repository-relative) or the `.md` files of `directory` sorted
///   by name; with a `query` (and `options`: `regex`, `caseSensitive`,
///   `wholeWord`) the panel gets `query`, `searchedFileCount`, `hits = []`
///   and `isSearching`, then — unless `isSearching` is set — the hits of a
///   synchronous `SiblingSearch` (`limitPerFile`, default 20) and
///   `isSearching = false`;
/// - `searchedFileCount` overrides the count; `select` selects a row and
///   `activate` runs the table's `onActivate`.
///
/// The spinner only reveals itself a second after a pass starts, and every
/// capture scene finishes its pass before then; Reduce Motion (forced on by
/// the harness) changes nothing here.
@MainActor
final class SearchResultsPanelViewScene: PanelScene {
    private var panel: SearchResultsPanelView?
    private let recorder = SearchResultsSceneRecorder()

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let panel = scenario.bool("current") ? SearchResultsPanelView() : SearchResultsPanelView(styleSheet: styleSheet)
        if scenario.bool("current") { panel.styleSheet = styleSheet }
        panel.delegate = recorder
        try Self.search(panel, scenario)
        let table = Self.table(in: panel)
        if let row = scenario.int("select") {
            table?.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
        }
        if scenario.bool("activate") {
            (table as? PanelTableView)?.onActivate?()
        }
        self.panel = panel
        return panel
    }

    /// The sibling files a scenario names.
    static func siblingURLs(_ scenario: PanelScenario) throws -> [URL] {
        let files = scenario.strings("files")
        if !files.isEmpty {
            return files.map { repositoryRoot.appendingPathComponent($0) }
        }
        guard let directory = scenario.string("directory") else { return [] }
        let folder = repositoryRoot.appendingPathComponent(directory)
        return try FileManager.default.contentsOfDirectory(atPath: folder.path)
            .filter { $0.hasSuffix(".md") }
            .sorted()
            .map { folder.appendingPathComponent($0) }
    }

    /// The window controller's sibling pass, run synchronously.
    /// `query` is the find bar's current query when the panel sits in the
    /// search inspector; otherwise it comes from `query` and `options`.
    static func search(_ panel: SearchResultsPanelView, _ scenario: PanelScenario, query given: FindQuery? = nil) throws {
        if let text = scenario.string("query") {
            let options = scenario.strings("options")
            var query = FindQuery()
            query.text = text
            query.isRegex = options.contains("regex")
            query.caseSensitive = options.contains("caseSensitive")
            query.wholeWord = options.contains("wholeWord")
            if let given { query = given }
            let urls = try siblingURLs(scenario)
            panel.query = query.text
            panel.searchedFileCount = urls.count
            panel.hits = []
            panel.isSearching = !query.isEmpty && FindEngine.isValid(query)
            if !scenario.bool("isSearching") {
                panel.hits = SiblingSearch.search(query, in: urls, limitPerFile: scenario.int("limitPerFile", 20))
                panel.isSearching = false
            }
        }
        if let count = scenario.int("searchedFileCount") { panel.searchedFileCount = count }
    }

    static func table(in view: NSView) -> NSTableView? {
        for subview in view.subviews {
            if let table = subview as? NSTableView { return table }
            if let found = table(in: subview) { return found }
        }
        return nil
    }

    static func emptyState(in view: NSView) -> PanelEmptyStateView? {
        view.subviews.compactMap { $0 as? PanelEmptyStateView }.first
    }

    static func hitJSON(_ hit: SiblingSearch.Hit) -> JSON {
        .array([
            .string(hit.displayName), .int(hit.lineNumber), JSON.range(hit.range), JSON.range(hit.contextRange),
            .string(hit.headingTitle),
        ])
    }

    func model() -> JSON {
        guard let panel else { return .null }
        let empty = Self.emptyState(in: panel)
        return .object([
            ("query", .string(panel.query)),
            ("searchedFileCount", .int(panel.searchedFileCount)),
            ("isSearching", .bool(panel.isSearching)),
            ("preferredWidth", .double(Double(panel.preferredWidth))),
            ("hitCount", .int(panel.hits.count)),
            ("hits", .array(panel.hits.prefix(40).map(Self.hitJSON))),
            ("emptyTitle", .string(empty?.title)),
            ("emptySubtitle", .string(empty?.subtitle)),
            ("accessibilityValue", .string(panel.accessibilityValue() as? String)),
            ("selected", .array(recorder.selected.map(Self.hitJSON))),
        ])
    }
}

@MainActor
final class SearchResultsSceneRecorder: SearchResultsDelegate {
    var selected: [SiblingSearch.Hit] = []
    func searchResults(_ view: SearchResultsPanelView, didSelect hit: SiblingSearch.Hit) { selected.append(hit) }
}
