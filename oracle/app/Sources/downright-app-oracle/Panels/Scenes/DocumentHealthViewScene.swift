import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `DocumentHealthView` scenes: the panel configured as
/// `DocumentWindowController.configureDocumentHealth` does (source text, then
/// `DocumentHealth.analyze` of the parsed document), then `state`:
///
///   limit     keep the first N findings
///   select    `selectFindingForTesting(at:)`
///   ignore    `ignoreSelectionForTesting()`
///   apply     `applySafeFixesForTesting()` (no delegate: the status line only)
///   reset     `resetIgnoredFindings()`
///   restyle   assign the style sheet again (the `didSet` path)
///
/// Reduce Motion (forced on by the harness) does not reach this panel: it
/// has no animation of its own.
@MainActor
final class DocumentHealthViewScene: PanelScene {
    private var view: DocumentHealthView?

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let text = try scenario.documentText()
        let view = DocumentHealthView(styleSheet: styleSheet)
        view.sourceText = text
        var findings = DocumentHealth.analyze(MarkdownParser.parse(text))
        if let limit = scenario.int("limit") { findings = Array(findings.prefix(limit)) }
        view.diagnostics = findings
        if let select = scenario.int("select") { view.selectFindingForTesting(at: select) }
        if scenario.bool("ignore") { view.ignoreSelectionForTesting() }
        if scenario.bool("apply") { view.applySafeFixesForTesting() }
        if scenario.bool("reset") { view.resetIgnoredFindings() }
        if scenario.bool("restyle") { view.styleSheet = styleSheet }
        self.view = view
        return view
    }

    func model() -> JSON {
        guard let view else { return .null }
        return .object([
            ("preferredWidth", .double(Double(view.preferredWidth))),
            ("findings", .array(view.diagnostics.map { .string($0.id) })),
            ("rows", DiagnosticsSceneSupport.tableRows(view)),
        ])
    }
}

/// Helpers the diagnostics-group scenes share (DocumentHealthView,
/// RenderTargetsView, AssetDoctorView, DocumentLensView); mirrored in
/// `crates/conformance/src/dump/panel/scenes/document_health_view.rs`.
@MainActor
enum DiagnosticsSceneSupport {
    static let rowLimit = 60

    /// Every row the panel's data source reports, asked of a fresh table as
    /// the Swift tests ask: group flag, height, and the row view's class,
    /// accessibility label and tooltip.
    static func tableRows(_ source: NSTableViewDataSource & NSTableViewDelegate) -> JSON {
        let table = NSTableView()
        let count = source.numberOfRows?(in: table) ?? 0
        var rows: [JSON] = []
        for row in 0..<min(count, rowLimit) {
            let cell = source.tableView?(table, viewFor: nil, row: row)
            rows.append(.object([
                ("group", .bool(source.tableView?(table, isGroupRow: row) ?? false)),
                ("selectable", .bool(source.tableView?(table, shouldSelectRow: row) ?? true)),
                ("height", .double(Double(source.tableView?(table, heightOfRow: row) ?? -1))),
                ("class", .string(cell.map { PanelTree.className($0) })),
                ("axLabel", .string(cell?.accessibilityLabel())),
                ("toolTip", .string(cell?.toolTip)),
            ]))
        }
        return .object([("count", .int(count)), ("rows", .array(rows))])
    }

    /// The scenario document's URL (`repositoryRoot` + `document`).
    static func documentURL(_ scenario: PanelScenario) -> URL? {
        scenario.documentPath.map { repositoryRoot.appendingPathComponent($0) }
    }

    /// A file-system probe: existence, directory flag, size and extension,
    /// read the same way on both sides.
    static func probe() -> AssetProbe {
        AssetProbe { url in
            var isDirectory: ObjCBool = false
            guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory) else { return nil }
            let size = (try? FileManager.default.attributesOfItem(atPath: url.path))?[.size] as? NSNumber
            return AssetMetadata(
                exists: true,
                isDirectory: isDirectory.boolValue,
                byteSize: size.map { $0.int64Value },
                fileExtension: url.pathExtension
            )
        }
    }

    /// Repository-relative spelling of a path under the repository root, so
    /// dumps do not depend on where the checkout lives.
    static func relative(_ path: String) -> String {
        let root = repositoryRoot.path
        if path.hasPrefix(root + "/") { return String(path.dropFirst(root.count + 1)) }
        return path
    }
}
