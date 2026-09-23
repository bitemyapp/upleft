import AppKit
import MarkdownCore
import MarkdownRender
// `PreviewViewController`'s state (`container`, `densityGutter`, `storage`,
// `previewGeneration`, …) is `private`. Package.swift compiles DownrightQL
// with `-enable-private-imports` (symbol visibility only, like
// `-enable-testing`) so the scene can read it and retire the memory watch.
@_private(sourceFile: "PreviewViewController.swift") import DownrightQL

/// `PreviewViewController` scenes (the `quicklook-preview` suite): the Quick
/// Look preview extension's view controller, driven the way Quick Look
/// drives it, then hosted in the harness's off-screen borderless window.
///
/// The scenario's `document` is previewed with
/// `preparePreviewOfFile(at:completionHandler:)`; the harness pumps the main
/// run loop until the handler runs (the load is detached). The preview
/// resolves its own theme and appearance (`PreviewAppearanceStore`, then
/// the view's effective appearance, which is the scenario's `NSApp`
/// appearance); the scenario's `theme` is not used. State:
///
///   repeat      preview the document's text repeated N times, written (UTF-8)
///               to a temporary file, e.g. to cross the large-file threshold
///   then        a second document previewed by the same controller (reuse)
///   fallback    `fallBackToPlainText()` after the preview, as the memory
///               watch would
///
/// Before every settle check the scene lays the whole document out (see
/// `beforeSettleCheck`).
///
/// The memory watch samples this process's malloc zones a second after the
/// preview and falls back to plain text above its ceiling, which would make
/// the capture depend on the oracle's own footprint and on timing. The scene
/// retires it the way a newer preview would: `previewGeneration &+= 1`
/// before its first sample (Rust: `retire_memory_watch_for_testing`).
@MainActor
final class PreviewViewControllerScene: PanelScene {
    private var controller: PreviewViewController?
    private var outcomes: [JSON] = []

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        guard let documentPath = scenario.documentPath else {
            throw PanelHarnessError.usage("a PreviewViewController scenario needs a document")
        }
        let controller = PreviewViewController()
        self.controller = controller
        if let count = scenario.int("repeat") {
            let text = String(repeating: try scenario.documentText(), count: count)
            let temporary = FileManager.default.temporaryDirectory
                .appendingPathComponent("upleft-quicklook-preview-\(ProcessInfo.processInfo.processIdentifier).md")
            try text.write(to: temporary, atomically: true, encoding: .utf8)
            defer { try? FileManager.default.removeItem(at: temporary) }
            try preview(temporary, with: controller)
        } else {
            try preview(repositoryRoot.appendingPathComponent(documentPath), with: controller)
        }
        if let then = scenario.string("then") {
            try preview(repositoryRoot.appendingPathComponent(then), with: controller)
        }
        if scenario.bool("fallback") { controller.fallBackToPlainText() }
        controller.previewGeneration &+= 1
        return controller.view
    }

    /// As the render harness does: lay the whole document out and size the
    /// text view to it, so its height no longer depends on which layout pass
    /// happened to run last (the preview is built before the window hosts it,
    /// and the two resize paths race in Swift as in Rust). Then let the
    /// controller's own `viewDidLayout` refresh the gutter's visibility and
    /// state. The container is laid out afresh first: its geometry from
    /// before the window hosted it (not backing-aligned) otherwise survives
    /// or not depending on which later pass marks it dirty.
    func beforeSettleCheck() {
        guard let controller, let container = controller.container else { return }
        container.needsLayout = true
        container.layoutSubtreeIfNeeded()
        if let layout = container.textView.textLayoutManager {
            layout.ensureLayout(for: layout.documentRange)
        }
        container.textView.resizeToFitContent()
        controller.view.needsLayout = true
        controller.view.layoutSubtreeIfNeeded()
    }

    private func preview(_ url: URL, with controller: PreviewViewController) throws {
        var outcome: JSON?
        controller.preparePreviewOfFile(at: url) { error in
            if let error = error as NSError? {
                outcome = .object([("domain", .string(error.domain)), ("code", .int(error.code))])
            } else {
                outcome = .null
            }
        }
        let deadline = Date().addingTimeInterval(30)
        while outcome == nil && Date() < deadline {
            _ = RunLoop.current.run(mode: .default, before: Date().addingTimeInterval(0.005))
        }
        guard let outcome else { throw PanelHarnessError.usage("the preview's completion handler never ran") }
        outcomes.append(outcome)
    }

    func model() -> JSON {
        guard let controller else { return .null }
        let gutter = controller.densityGutter
        let gutterShown: Bool = {
            guard let container = controller.container, let gutter else { return false }
            return container.leadingAccessory === gutter
        }()
        return .object([
            ("outcomes", .array(outcomes)),
            ("storageLength", .int(controller.storage.length)),
            ("container", .bool(controller.container != nil)),
            ("fallback", .bool(controller.fallbackTextView != nil)),
            ("noticeBar", .bool(controller.noticeBar != nil)),
            ("gutterShown", .bool(gutterShown)),
            ("currentHeadingIndex", controller.currentHeadingIndex.map { .int($0) } ?? .null),
            ("metricsSummary", gutter.map { .string($0.metricsSummary) } ?? .null),
            ("visibleRange", gutter.map { .array([.double(Double($0.visibleRange.lowerBound)), .double(Double($0.visibleRange.upperBound))]) } ?? .null),
            ("readProgress", gutter.map { .double(Double($0.readProgress)) } ?? .null),
            ("outline", gutter.map { gutter in
                .array(gutter.outlineEntries.map { entry in
                    .array([.string(entry.title), .int(entry.level), .double(Double(entry.fraction)), .bool(entry.isCurrent)])
                })
            } ?? .null),
        ])
    }
}
