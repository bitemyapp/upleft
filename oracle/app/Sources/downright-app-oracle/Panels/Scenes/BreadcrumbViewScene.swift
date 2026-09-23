import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `BreadcrumbView` scenes.
///
/// State, applied in this order:
///   current      build with `BreadcrumbView()` and assign the style sheet
///   heading      trail = the ancestor chain of the document's heading at
///                this index (negative: from the end), built as
///                `DocumentWindowController.refreshBreadcrumb` builds it
///   trail        an explicit trail, `[[index, title, level], ...]`
///   zoom         `zoomLevel` by raw value
///   present      `showCurrentSection()`
///   next         a second heading index (a section change)
///   hide         `hideCurrentSection()`
///   clear        trail = []
///
/// Reduce Motion (forced on by the harness) makes the fades and the
/// section-change crossfade immediate.
@MainActor
final class BreadcrumbViewScene: PanelScene {
    private var crumb: BreadcrumbView?
    private var headings: [HeadingNode] = []

    private func trail(heading raw: Int) -> [(index: Int, title: String, level: Int)] {
        let position = raw < 0 ? headings.count + raw : raw
        guard headings.indices.contains(position) else { return [] }
        var index = position
        var trail: [(index: Int, title: String, level: Int)] = []
        while true {
            let heading = headings[index]
            trail.insert((index, heading.title, heading.level), at: 0)
            guard let parent = heading.parentIndex else { break }
            index = parent
        }
        return trail
    }

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        headings = MarkdownParser.parse(try scenario.documentText()).headings
        let crumb = scenario.bool("current") ? BreadcrumbView() : BreadcrumbView(styleSheet: styleSheet)
        if scenario.bool("current") { crumb.styleSheet = styleSheet }
        if let heading = scenario.int("heading") { crumb.trail = trail(heading: heading) }
        let explicit = scenario.array("trail").compactMap { $0 as? [Any] }
        if !explicit.isEmpty {
            crumb.trail = explicit.map { entry in
                ((entry[0] as? NSNumber)?.intValue ?? 0,
                 entry[1] as? String ?? "",
                 (entry[2] as? NSNumber)?.intValue ?? 1)
            }
        }
        if let zoom = scenario.int("zoom"), let level = ZoomLevel(rawValue: zoom) { crumb.zoomLevel = level }
        if scenario.bool("present") { crumb.showCurrentSection() }
        if let next = scenario.int("next") { crumb.trail = trail(heading: next) }
        if scenario.bool("hide") { crumb.hideCurrentSection() }
        if scenario.bool("clear") { crumb.trail = [] }
        self.crumb = crumb
        return crumb
    }

    func model() -> JSON {
        guard let crumb else { return .null }
        let menu = crumb.makePathMenu()
        return .object([
            ("presented", .bool(crumb.isPresentedForTesting)),
            ("currentTitleOrigin", .double(Double(crumb.currentTitleOrigin))),
            ("trail", .array(crumb.trail.map { .array([.int($0.index), .string($0.title), .int($0.level)]) })),
            ("zoomLevel", .int(crumb.zoomLevel.rawValue)),
            ("pathMenu", .array(menu.items.map {
                .array([.string($0.title), .int($0.indentationLevel), .int($0.state.rawValue)])
            })),
            ("sameTrailSelf", .bool(BreadcrumbView.sameTrail(crumb.trail, crumb.trail))),
            ("intrinsicContentSize", PanelTree.size(crumb.intrinsicContentSize)),
        ])
    }
}
