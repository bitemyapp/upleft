import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `DocumentQuickLook` scenes (`panel-model` only: the file draws nothing).
/// Each of `state.targets` (`{kind, value?, location?, length?, heading?,
/// line?}`) becomes a `ContextTarget` and goes through
/// `QuickLookRequest.resolve` as `DocumentWindowController.presentQuickLook`
/// calls it: the scenario document's URL, a real `PathResolver` for that
/// document, and `FileManager` existence. The panel is a plain empty view.
/// Paths in the dump are repository-relative.
@MainActor
final class DocumentQuickLookScene: PanelScene {
    private var results: [JSON] = []

    static func target(_ value: Any) -> ContextTarget? {
        guard let object = value as? [String: Any], let kind = object["kind"] as? String else { return nil }
        let text = object["value"] as? String ?? ""
        let range = NSRange(
            location: (object["location"] as? NSNumber)?.intValue ?? 0,
            length: (object["length"] as? NSNumber)?.intValue ?? 0
        )
        let targetKind: ContextTarget.Kind
        switch kind {
        case "image": targetKind = .image(text)
        case "pathToken":
            targetKind = .pathToken(PathToken(rawPath: text, line: (object["line"] as? NSNumber)?.intValue))
        case "link": targetKind = .link(text)
        case "heading": targetKind = .heading((object["heading"] as? NSNumber)?.intValue ?? 0)
        case "codeBlock": targetKind = .codeBlock(range)
        case "table": targetKind = .table(range)
        case "selection": targetKind = .selection
        case "plain": targetKind = .plain
        default: return nil
        }
        return ContextTarget(kind: targetKind, sourceRange: range)
    }

    static func describe(_ request: QuickLookRequest?) -> JSON {
        switch request {
        case .none: return .null
        case .lightbox(let source)?:
            return .object([("request", .string("lightbox")), ("source", .string(source))])
        case .panel(let url)?:
            return .object([
                ("request", .string("panel")),
                ("path", .string(DiagnosticsSceneSupport.relative(url.path))),
                ("directory", .bool(url.hasDirectoryPath)),
            ])
        }
    }

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let documentURL = DiagnosticsSceneSupport.documentURL(scenario)
        let resolver = PathResolver(documentURL: documentURL)
        results = scenario.array("targets").map { value in
            guard let target = Self.target(value) else { return .string("invalid target") }
            return Self.describe(QuickLookRequest.resolve(
                target,
                documentURL: documentURL,
                pathResolution: { resolver.resolve($0) },
                fileExists: { FileManager.default.fileExists(atPath: $0.path) }
            ))
        }
        return NSView(frame: NSRect(x: 0, y: 0, width: scenario.width, height: scenario.height))
    }

    func model() -> JSON {
        .object([("results", .array(results))])
    }
}
