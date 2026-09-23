import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `VersionTimelineView` scenes: a history built from `state.versions`
/// (`{"at": seconds after state.base, "kind": "external"|"local"|"baseline",
/// "hash": …, "bytes": …}`), then `state.selectedIndex` and `state.steps`
/// (arrow-key steps, as `accessibilityPerformIncrement`/`Decrement`)
/// applied in that order. `state.current` builds with `VersionTimelineView()`
/// and assigns the sheet afterwards, as hosts do.
///
/// Dates are fixed (`state.base` is a reference-date offset), so the stamp is
/// stable; the relative part of the caption compares with now, so scenarios
/// keep their histories years in the past. Reduce Motion: the timeline has no
/// animation.
@MainActor
final class VersionTimelineViewScene: PanelScene {
    private var timeline: VersionTimelineView?
    private var scrubs: [String] = []

    static func versions(_ scenario: PanelScenario) -> [SnapshotStore.VersionRecord] {
        let base = scenario.double("base", 650_280_600)
        return scenario.array("versions").enumerated().compactMap { index, value in
            guard let object = value as? [String: Any] else { return nil }
            let at = (object["at"] as? NSNumber)?.doubleValue ?? 0
            let kind = SnapshotStore.SnapshotKind(rawValue: object["kind"] as? String ?? "local") ?? .local
            return SnapshotStore.VersionRecord(
                hash: object["hash"] as? String ?? "hash-\(index)",
                date: Date(timeIntervalSinceReferenceDate: base + at),
                byteCount: (object["bytes"] as? NSNumber)?.intValue ?? 100,
                kind: kind
            )
        }
    }

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let timeline = scenario.bool("current") ? VersionTimelineView() : VersionTimelineView(styleSheet: styleSheet)
        if scenario.bool("current") { timeline.styleSheet = styleSheet }
        let delegate = Delegate(scene: self)
        timeline.delegate = delegate
        self.delegate = delegate
        timeline.versions = Self.versions(scenario)
        if let index = scenario.int("selectedIndex") { timeline.selectedIndex = index }
        for step in scenario.array("steps").compactMap({ ($0 as? NSNumber)?.intValue }) {
            _ = step > 0 ? timeline.accessibilityPerformIncrement() : timeline.accessibilityPerformDecrement()
        }
        self.timeline = timeline
        return timeline
    }

    private var delegate: Delegate?

    private final class Delegate: VersionTimelineDelegate {
        weak var scene: VersionTimelineViewScene?
        init(scene: VersionTimelineViewScene) { self.scene = scene }
        func versionTimeline(_ view: VersionTimelineView, didScrubTo record: SnapshotStore.VersionRecord) {
            scene?.scrubs.append(record.hash)
        }
        func versionTimeline(_ view: VersionTimelineView, didRequestRestore record: SnapshotStore.VersionRecord) {}
    }

    func model() -> JSON {
        guard let timeline else { return .null }
        return .object([
            ("selectedIndex", .int(timeline.selectedIndex)),
            ("selectedHash", .string(timeline.selectedRecord?.hash)),
            ("versionCount", .int(timeline.versions.count)),
            ("intrinsicContentSize", PanelTree.size(timeline.intrinsicContentSize)),
            ("isFlipped", .bool(timeline.isFlipped)),
            ("acceptsFirstResponder", .bool(timeline.acceptsFirstResponder)),
            ("focusRingMaskBounds", PanelTree.rect(timeline.focusRingMaskBounds)),
            ("scrubs", .array(scrubs.map { .string($0) })),
        ])
    }
}
