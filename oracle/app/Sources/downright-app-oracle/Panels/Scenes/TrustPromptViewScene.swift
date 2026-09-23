import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `TrustPromptView` scenes: a `TrustRequest` from `state.effect` (a
/// `TrustEffect` raw value), `state.displayName`, `state.canonicalPath`,
/// `state.externalURL` and `state.documentPath` (a file URL path), unless
/// `state.noRequest`; then `state.choose` (decision names, through
/// `chooseForTesting`). `state.current` builds with `TrustPromptView()` and
/// assigns the sheet afterwards. Reduce Motion: nothing here animates.
@MainActor
final class TrustPromptViewScene: PanelScene {
    private var prompt: TrustPromptView?
    private var request: TrustRequest?
    private var delegate: Delegate?

    private final class Delegate: TrustPromptViewDelegate {
        var decisions: [String] = []
        func trustPrompt(_ view: TrustPromptView, didChoose decision: TrustPromptDecision, request: TrustRequest) {
            decisions.append("\(decision) \(request.target.displayName)")
        }
    }

    static func decision(_ name: String) -> TrustPromptDecision? {
        switch name {
        case "allowOnce": return .allowOnce
        case "allowForFile": return .allowForFile
        case "allowForFolder": return .allowForFolder
        case "deny": return .deny
        case "revoke": return .revoke
        default: return nil
        }
    }

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let prompt = scenario.bool("current") ? TrustPromptView() : TrustPromptView(styleSheet: styleSheet)
        if scenario.bool("current") { prompt.styleSheet = styleSheet }
        let delegate = Delegate()
        prompt.delegate = delegate
        self.delegate = delegate
        if !scenario.bool("noRequest") {
            let effect = TrustEffect(rawValue: scenario.string("effect", "openExternalLink")) ?? .openExternalLink
            let request = TrustRequest(
                effect: effect,
                target: TrustTarget(
                    displayName: scenario.string("displayName", ""),
                    canonicalPath: scenario.string("canonicalPath"),
                    externalURL: scenario.string("externalURL")
                ),
                documentURL: scenario.string("documentPath").map { URL(fileURLWithPath: $0) }
            )
            prompt.request = request
            self.request = request
        }
        for name in scenario.strings("choose") {
            guard let decision = Self.decision(name) else { continue }
            prompt.chooseForTesting(decision)
        }
        self.prompt = prompt
        return prompt
    }

    func model() -> JSON {
        guard let prompt else { return .null }
        return .object([
            ("preferredWidth", .double(Double(prompt.preferredWidth))),
            ("fileGrantName", .string(request.flatMap { TrustPromptView.fileGrantName(for: $0) })),
            ("hasRequest", .bool(prompt.request != nil)),
            ("acceptsFirstResponder", .bool(prompt.acceptsFirstResponder)),
            ("decisions", .array((delegate?.decisions ?? []).map { .string($0) })),
            ("fittingSize", PanelTree.size(prompt.fittingSize)),
        ])
    }
}
