import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `AssetDoctorView` scenes: the panel configured as
/// `DocumentWindowController.configureAssetDoctor` does — `AssetDoctor.diagnose`
/// of the parsed scenario document, with the document's folder as the
/// workspace root and a file-system probe (`DiagnosticsSceneSupport.probe`).
///
///   maximumBytes  the context's size limit (default 10 MB)
///   empty         assign no diagnostics (the "every image resolves" state)
///   restyle       assign the style sheet again
///
/// The rows' `reduceMotion` is read from the style sheet (the harness forces
/// it on) and stored; the panel never animates.
@MainActor
final class AssetDoctorViewScene: PanelScene {
    private var view: AssetDoctorView?

    static func diagnostics(_ scenario: PanelScenario, text: String) -> [AssetDiagnostic] {
        let documentURL = DiagnosticsSceneSupport.documentURL(scenario)
        let context: AssetResolutionContext
        if let maximumBytes = scenario.int("maximumBytes") {
            context = AssetResolutionContext(
                documentURL: documentURL,
                workspaceRoot: documentURL?.deletingLastPathComponent(),
                maximumBytes: Int64(maximumBytes)
            )
        } else {
            context = AssetResolutionContext(
                documentURL: documentURL,
                workspaceRoot: documentURL?.deletingLastPathComponent()
            )
        }
        return AssetDoctor.diagnose(MarkdownParser.parse(text), context: context, probe: DiagnosticsSceneSupport.probe())
    }

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let text = try scenario.documentText()
        let view = AssetDoctorView(styleSheet: styleSheet)
        if !scenario.bool("empty") {
            view.diagnostics = Self.diagnostics(scenario, text: text)
        }
        if scenario.bool("restyle") { view.styleSheet = styleSheet }
        self.view = view
        return view
    }

    func model() -> JSON {
        guard let view else { return .null }
        return .object([
            ("preferredWidth", .double(Double(view.preferredWidth))),
            ("findings", .array(view.diagnostics.map { .string($0.id) })),
            ("lines", .array(view.diagnostics.map { .int($0.reference.line) })),
            ("rows", DiagnosticsSceneSupport.tableRows(view)),
        ])
    }
}
