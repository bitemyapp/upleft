import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `ChangeSummaryBarView` scenes. State, applied in this order:
///
/// - `current`: build with `ChangeSummaryBarView()` and assign the style
///   sheet afterwards;
/// - `changeCount` (with `message`, default "Updated on disk"): the
///   count-only `configure(message:changeCount:)`;
/// - otherwise `marks` (`[kind, location, length]` triples, kind a
///   `ChangeKind` raw value) summarised against the document's UTF-16 length
///   (or `documentLength` without a document), then
///   `configure(message:summary:)` with the optional `message`;
/// - `press`: accessibility labels of the bar's (hidden) buttons, each sent
///   `performClick(nil)`.
///
/// The toast is hosted at its intrinsic size, centred over a themed
/// background. Reduce Motion (forced on by the harness) changes nothing
/// here: the bar never animates itself.
@MainActor
final class ChangeSummaryBarViewScene: PanelScene {
    private var bar: ChangeSummaryBarView?
    private var summary: ChangeSummaryBarView.Summary?
    private let recorder = ChangeSummarySceneRecorder()

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let bar = scenario.bool("current") ? ChangeSummaryBarView() : ChangeSummaryBarView(styleSheet: styleSheet)
        if scenario.bool("current") { bar.styleSheet = styleSheet }
        bar.delegate = recorder
        if let count = scenario.int("changeCount") {
            bar.configure(message: scenario.string("message", "Updated on disk"), changeCount: count)
        } else if scenario.state["marks"] != nil {
            let length = scenario.documentPath != nil
                ? try scenario.documentText().utf16.count
                : scenario.int("documentLength", 0)
            let marks = scenario.array("marks").compactMap { entry -> ChangeTracker.Mark? in
                guard let parts = entry as? [Any], parts.count == 3,
                      let kind = (parts[0] as? String).flatMap(ChangeKind.init(rawValue:)),
                      let location = (parts[1] as? NSNumber)?.intValue,
                      let size = (parts[2] as? NSNumber)?.intValue
                else { return nil }
                return ChangeTracker.Mark(kind: kind, range: NSRange(location: location, length: size))
            }
            let summary = ChangeSummaryBarView.Summary(marks: marks, documentLength: length)
            bar.configure(message: scenario.string("message"), summary: summary)
            self.summary = summary
        }
        for label in scenario.strings("press") {
            Self.buttons(in: bar).first { $0.accessibilityLabel() == label }?.performClick(nil)
        }
        self.bar = bar
        return bar
    }

    func host(_ panel: NSView, in window: NSWindow, scenario: PanelScenario) {
        let container = NSView(frame: NSRect(x: 0, y: 0, width: scenario.width, height: scenario.height))
        container.wantsLayer = true
        if let bar { container.layer?.backgroundColor = bar.styleSheet.background.cgColor }
        window.contentView = container
        let size = panel.intrinsicContentSize
        panel.frame = NSRect(
            x: ((scenario.width - size.width) / 2).rounded(),
            y: ((scenario.height - size.height) / 2).rounded(),
            width: size.width,
            height: size.height
        )
        container.addSubview(panel)
    }

    static func buttons(in view: NSView) -> [NSButton] {
        view.subviews.flatMap { subview -> [NSButton] in
            (subview as? NSButton).map { [$0] } ?? buttons(in: subview)
        }
    }

    func model() -> JSON {
        guard let bar else { return .null }
        var pairs: [(String, JSON)] = [
            ("message", .string(bar.message)),
            ("positionStatus", .string(bar.positionStatusForTesting)),
            ("intrinsicContentSize", PanelTree.size(bar.intrinsicContentSize)),
            ("fittedWidth", .double(Double(bar.fittedWidth))),
            ("acceptsFirstResponder", .bool(bar.acceptsFirstResponder)),
            ("buttons", .array(Self.buttons(in: bar).map { button in
                .array([.string(button.accessibilityLabel()), .bool(button.isHidden), .bool(button.isEnabled),
                        .string(button.toolTip)])
            })),
            ("events", .array(recorder.events.map { .string($0) })),
        ]
        if let summary {
            pairs.append(("summary", .object([
                ("added", .int(summary.added)),
                ("rewritten", .int(summary.rewritten)),
                ("removed", .int(summary.removed)),
                ("total", .int(summary.total)),
                ("headline", .string(summary.headline)),
                ("accessibilityDescription", .string(summary.accessibilityDescription)),
                ("distributionDescription", .string(summary.distributionDescription)),
                ("positions", .array(summary.positions.map {
                    .array([.double($0.fraction), .string($0.kind.rawValue)])
                })),
            ])))
        }
        return .object(pairs)
    }
}

@MainActor
final class ChangeSummarySceneRecorder: ChangeSummaryBarDelegate {
    var events: [String] = []
    func changeSummaryBar(_ bar: ChangeSummaryBarView, didRequestJump forward: Bool) {
        events.append(forward ? "next" : "previous")
    }
    func changeSummaryBarDidRequestMarkReviewed(_ bar: ChangeSummaryBarView) { events.append("reviewed") }
    func changeSummaryBarDidRequestDismiss(_ bar: ChangeSummaryBarView) { events.append("dismiss") }
}
