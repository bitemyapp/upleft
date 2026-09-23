import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `RenderTargetsView` scenes, applied in this order:
///
///   profiles  the profile list: built-in ids (`"commonMark"`) or custom
///             `{"name": …, "capabilities": ["rawHTML", …]}`
///   (always)  `sourceText`, then `document` (as
///             `DocumentWindowController.configureRenderTargets` does)
///   profile   index into the profile list, assigned to `selectedProfile`
///             (what the target popup does)
///   select    `selectFindingForTesting(at:)`
///   apply     `applySafeFixesForTesting()` (no delegate: the status line)
///   restyle   assign the style sheet again
///
/// Reduce Motion (forced on by the harness) does not reach this panel.
@MainActor
final class RenderTargetsViewScene: PanelScene {
    private var view: RenderTargetsView?

    static func profile(_ value: Any) -> RenderTargetProfile? {
        if let id = value as? String {
            return BuiltInRenderTarget(rawValue: id)?.profile
        }
        guard let object = value as? [String: Any], let name = object["name"] as? String else { return nil }
        var capabilities: MarkdownCapabilities = []
        for raw in object["capabilities"] as? [String] ?? [] {
            guard let capability = MarkdownCapability(rawValue: raw) else { continue }
            capabilities.insert(MarkdownCapabilities(capability))
        }
        return RenderTargetProfile.custom(name: name, capabilities: capabilities)
    }

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let text = try scenario.documentText()
        let view = RenderTargetsView(styleSheet: styleSheet)
        let profiles = scenario.array("profiles").compactMap(Self.profile)
        if !profiles.isEmpty { view.profiles = profiles }
        view.sourceText = text
        view.document = MarkdownParser.parse(text)
        if let index = scenario.int("profile"), index >= 0, index < view.profiles.count {
            view.selectedProfile = view.profiles[index]
        }
        if let select = scenario.int("select") { view.selectFindingForTesting(at: select) }
        if scenario.bool("apply") { view.applySafeFixesForTesting() }
        if scenario.bool("restyle") { view.styleSheet = styleSheet }
        self.view = view
        return view
    }

    func model() -> JSON {
        guard let view else { return .null }
        return .object([
            ("preferredWidth", .double(Double(view.preferredWidth))),
            ("profiles", .array(view.profiles.map { .string($0.id) })),
            ("selectedProfile", .string(view.selectedProfile.id)),
            ("reportProfile", .string(view.report.profile.id)),
            ("findings", .array(view.report.diagnostics.map { .string($0.id) })),
            ("rows", DiagnosticsSceneSupport.tableRows(view)),
        ])
    }
}
