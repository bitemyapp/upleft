import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `DocumentLensView` scenes, applied in this order:
///
///   target    built-in render target id, assigned to `renderTargetProfile`
///             before the model is built (as `documentLens(_:didSelectRenderTarget:)`)
///   (always)  unless `empty`: the model built as
///             `DocumentWindowController.configureDocumentLens` builds it —
///             health, asset references and diagnostics (document folder as
///             workspace root, file-system probe), the target's report,
///             `changes` ([{kind, location, length}]) — then `sourceText`,
///             then `model`
///   tab       `DocumentLensTab` raw value, assigned to `selectedTab`
///   select    `selectItemForTesting(at:)`
///   restyle   assign the style sheet again
///
/// Reduce Motion (forced on by the harness) does not reach this panel.
@MainActor
final class DocumentLensViewScene: PanelScene {
    private var view: DocumentLensView?
    private var selections: [JSON] = []

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let text = try scenario.documentText()
        let view = DocumentLensView(styleSheet: styleSheet)
        if let target = scenario.string("target"), let profile = BuiltInRenderTarget(rawValue: target)?.profile {
            view.renderTargetProfile = profile
        }
        if !scenario.bool("empty") {
            let parsed = MarkdownParser.parse(text)
            let health = DocumentHealth.analyze(parsed)
            let documentURL = DiagnosticsSceneSupport.documentURL(scenario)
            let context = AssetResolutionContext(
                documentURL: documentURL,
                workspaceRoot: documentURL?.deletingLastPathComponent()
            )
            let references = AssetDoctor.references(in: parsed, context: context)
            let assets = AssetDoctor.diagnose(parsed, context: context, probe: DiagnosticsSceneSupport.probe())
            let report = MarkdownCompatibility.diagnose(parsed, for: view.renderTargetProfile)
            var changes: [DocumentLensChange] = []
            for (index, value) in scenario.array("changes").enumerated() {
                guard let object = value as? [String: Any],
                      let kind = ChangeKind(rawValue: object["kind"] as? String ?? ""),
                      let location = (object["location"] as? NSNumber)?.intValue,
                      let length = (object["length"] as? NSNumber)?.intValue else { continue }
                changes.append(DocumentLensChange(
                    id: "\(index):\(location)", kind: kind, range: NSRange(location: location, length: length)
                ))
            }
            view.sourceText = text
            view.model = DocumentLensModel(input: DocumentLensInput(
                document: parsed, health: health, assetReferences: references, assets: assets,
                renderTarget: report, changes: changes
            ))
        }
        if let tab = scenario.string("tab"), let selected = DocumentLensTab(rawValue: tab) {
            view.selectedTab = selected
        }
        let delegate = RecordingLensSceneDelegate()
        view.delegate = delegate
        if let select = scenario.int("select") { view.selectItemForTesting(at: select) }
        selections = delegate.selections
        view.delegate = nil
        if scenario.bool("restyle") { view.styleSheet = styleSheet }
        self.view = view
        return view
    }

    func model() -> JSON {
        guard let view else { return .null }
        return .object([
            ("preferredWidth", .double(Double(view.preferredWidth))),
            ("selectedTab", .string(view.selectedTab.rawValue)),
            ("renderTargetProfile", .string(view.renderTargetProfile.id)),
            ("sectionCounts", .array(DocumentLensTab.allCases.map { .int(view.model.section($0).count) })),
            ("selections", .array(selections)),
            ("rows", DiagnosticsSceneSupport.tableRows(view)),
        ])
    }
}

@MainActor
private final class RecordingLensSceneDelegate: DocumentLensViewDelegate {
    var selections: [JSON] = []

    func documentLens(_ view: DocumentLensView, didSelect range: NSRange, item: DocumentLensItem) {
        selections.append(.object([("range", .range(range)), ("id", .string(item.id))]))
    }

    func documentLens(_ view: DocumentLensView, didSelectRenderTarget profile: RenderTargetProfile) {}
}
