import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `SearchInspectorView` scenes. State, applied in this order:
///
/// - `current`: build with `SearchInspectorView()` and assign the style
///   sheet afterwards;
/// - the find bar's state, exactly as `FindBarViewScene.applyFindState`
///   applies it (`selectionScope`, `options`, `query` with the document's
///   find session, `status`, `valid`);
/// - `showsReplace` (through the inspector, which also grows the find
///   bar's slot) and `replacement`;
/// - `results`: a `SearchResultsPanelView` searched for the find bar's
///   current query over the scenario's sibling files
///   (`SearchResultsPanelViewScene.search`), handed to `setResults`
///   (`resultsTwice`: handed over again); `clearResults`: `setResults(nil)`.
///
/// Reduce Motion (forced on by the harness) makes the find bar's slot change
/// height without the glide.
@MainActor
final class SearchInspectorViewScene: PanelScene {
    private var inspector: SearchInspectorView?
    private var results: SearchResultsPanelView?
    private var session: FindSession?

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let inspector = scenario.bool("current") ? SearchInspectorView() : SearchInspectorView(styleSheet: styleSheet)
        if scenario.bool("current") { inspector.styleSheet = styleSheet }
        let bar = inspector.findBar
        session = try FindBarViewScene.applyFindState(bar, scenario)
        if scenario.bool("showsReplace") { inspector.showsReplace = true }
        if let replacement = scenario.string("replacement"),
           let field = FindBarViewScene.textField("Replace with", in: bar) {
            field.stringValue = replacement
        }
        if scenario.bool("results") {
            let results = SearchResultsPanelView(styleSheet: styleSheet)
            try SearchResultsPanelViewScene.search(results, scenario, query: bar.currentQuery)
            inspector.setResults(results)
            if scenario.bool("resultsTwice") { inspector.setResults(results) }
            self.results = results
        }
        if scenario.bool("clearResults") { inspector.setResults(nil) }
        self.inspector = inspector
        return inspector
    }

    func model() -> JSON {
        guard let inspector else { return .null }
        let bar = inspector.findBar
        var pairs: [(String, JSON)] = [
            ("showsReplace", .bool(inspector.showsReplace)),
            ("findBarShowsReplace", .bool(bar.showsReplace)),
            ("findBarFrame", PanelTree.rect(bar.frame)),
            ("findBarWantsLayer", .bool(bar.wantsLayer)),
            ("statusText", .string(bar.statusText)),
            ("isQueryValid", .bool(bar.isQueryValid)),
            ("queryText", .string(bar.currentQuery.text)),
            ("resultsAttached", .bool(results?.superview != nil)),
            ("resultsHitCount", .int(results?.hits.count ?? -1)),
        ]
        if let session {
            pairs.append(("matchCount", .int(session.count)))
        }
        return .object(pairs)
    }
}
