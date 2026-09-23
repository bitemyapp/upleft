import AppKit
import MarkdownCore
import MarkdownRender
// The controller's panel view, the header's labels, the content kind and
// `UpdatePanelContent.diagnostics` are `private`; the package builds
// DownrightApp with `-enable-private-imports`.
@_private(sourceFile: "UpdateWindowController.swift") import DownrightApp

/// `UpdateWindowController` scenes: the titled update window in each state
/// of a coordinator over a `FakeUpdateEngine` (started, UI suppressed, so
/// nothing but this window is ever built).
///
/// State: `steps` (driver callbacks and engine settings before the
/// controller is built, see `UpdateScenes`), `stepsAfter` (the same after
/// it is built, reaching the panel through `stateDidChange` — a download's
/// progress patch, for one), and `showDetails` (the failure view's
/// "Technical Details" toggle, as its click sends `toggleDetail`).
///
/// The window is the scene's own (`ownWindow`): titled, so the harness has
/// already made `constrainFrameRect:toScreen:` the identity; it moves the
/// window to (-30000, -30000) before ordering it in, and `showWindow` is
/// never called. The header shows the application icon, which the scene
/// sets to the bundle's `AppIcon.icns` (`UpdateScenes.useBundleIcon`). The
/// panel draws from its own style sheet (the current
/// theme, which the scene selects, against the window's appearance, with
/// the system Reduce Motion); nothing in it animates except progress
/// indicators, which `beforeSettleCheck` stops where they stand so the
/// capture settles.
@MainActor
final class UpdateWindowControllerScene: PanelScene {
    private var controller: UpdateWindowController?
    private var coordinator: UpdateCoordinator?
    private var engine: FakeUpdateEngine?

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        UpdateScenes.selectTheme(scenario.theme)
        UpdateScenes.useBundleIcon()
        let engine = FakeUpdateEngine()
        let coordinator = UpdateCoordinator(engine: engine)
        coordinator.suppressUIForTesting = true
        try? engine.start()
        self.engine = engine
        self.coordinator = coordinator
        UpdateScenes.run(scenario.array("steps"), on: coordinator, engine: engine)
        let controller = UpdateWindowController(coordinator: coordinator)
        self.controller = controller
        UpdateScenes.run(scenario.array("stepsAfter"), on: coordinator, engine: engine)
        if scenario.bool("showDetails"), let failure = Self.failureView(in: controller.window!.contentView!) {
            failure.toggleDetail()
        }
        return controller.window!.contentView!
    }

    private static func failureView(in view: NSView) -> UpdateFailureView? {
        if let failure = view as? UpdateFailureView { return failure }
        for subview in view.subviews {
            if let failure = failureView(in: subview) { return failure }
        }
        return nil
    }

    func ownWindow(_ panel: NSView) -> NSWindow? { controller?.window }

    func beforeSettleCheck() {
        if let content = controller?.window?.contentView { UpdateScenes.stopIndicators(in: content) }
    }

    func model() -> JSON {
        guard let controller, let coordinator, let panelView = controller.panelView else { return .null }
        let footer = panelView.footer
        let buttons: [JSON] = footer.buttons.map {
            .object([("title", .string($0.buttonTitle)), ("primary", .bool($0.isPrimary))])
        }
        let content = panelView.contentContainer.subviews.first
        var diagnostics: JSON = .null
        if case .failed(let failure, _) = coordinator.phase {
            diagnostics = .string(UpdatePanelContent.diagnostics(coordinator: coordinator, failure: failure))
        }
        return .object([
            ("windowFrame", PanelTree.rect(controller.window!.frame)),
            ("title", .string(panelView.header.titleLabel.stringValue)),
            ("versions", .string(panelView.header.versionsLabel.stringValue)),
            ("lastKind", .string(panelView.lastKind.map { "\($0)" })),
            ("content", .string(content.map { PanelTree.className($0) })),
            ("buttons", .array(buttons)),
            ("leading", .int(footer.leadingStack.arrangedSubviews.count)),
            ("trailing", .int(footer.trailingStack.arrangedSubviews.count)),
            ("pill", UpdateScenes.json(coordinator.pillModel)),
            ("diagnostics", diagnostics),
        ])
    }
}
