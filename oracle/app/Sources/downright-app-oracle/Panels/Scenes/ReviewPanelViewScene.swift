import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `ReviewPanelView` scenes: reviews anchored in the scenario's document
/// with `ReviewSidecarEngine.makeReview` (`state.reviews`: `{"kind", "range":
/// [location, length], "body", "replacement", "state"}`), the panel's source
/// text the document after `state.edits` (`[location, length, replacement]`,
/// applied in order with `NSString` replacement) unless `state.noSource`,
/// set before the reviews as the window controller does. Then
/// `state.select` (a row, selected as a click would) and `state.press`
/// ("apply", "reject", "resolve": the buttons' actions). `state.current`
/// builds with `ReviewPanelView()` and assigns the sheet afterwards.
/// Reduce Motion: nothing here animates.
@MainActor
final class ReviewPanelViewScene: PanelScene {
    private var panel: ReviewPanelView?
    private var delegate: Delegate?
    private var statuses: [String] = []

    private final class Delegate: ReviewPanelViewDelegate {
        var events: [String] = []
        func reviewPanel(_ panel: ReviewPanelView, didSelect review: ReviewItem) {
            events.append("select \(review.title) \(review.anchor.range.location)")
        }
        func reviewPanel(_ panel: ReviewPanelView, didApply review: ReviewItem) {
            events.append("apply \(review.title) \(review.anchor.range.location)")
        }
        func reviewPanel(_ panel: ReviewPanelView, didReject review: ReviewItem) {
            events.append("reject \(review.title) \(review.anchor.range.location)")
        }
        func reviewPanel(_ panel: ReviewPanelView, didResolve review: ReviewItem) {
            events.append("resolve \(review.title) \(review.anchor.range.location)")
        }
    }

    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView {
        let text = try scenario.documentText()
        var source = text
        for edit in scenario.array("edits") {
            let values = edit as? [Any] ?? []
            guard values.count == 3, let location = (values[0] as? NSNumber)?.intValue,
                  let length = (values[1] as? NSNumber)?.intValue, let replacement = values[2] as? String else { continue }
            source = (source as NSString).replacingCharacters(in: NSRange(location: location, length: length), with: replacement)
        }
        let reviews: [ReviewItem] = scenario.array("reviews").compactMap { value in
            guard let object = value as? [String: Any] else { return nil }
            let range = (object["range"] as? [Any] ?? []).compactMap { ($0 as? NSNumber)?.intValue }
            guard range.count == 2, let kind = ReviewKind(rawValue: object["kind"] as? String ?? "comment"),
                  var review = ReviewSidecarEngine.makeReview(
                      kind: kind, in: text, range: NSRange(location: range[0], length: range[1]),
                      body: object["body"] as? String ?? "", replacement: object["replacement"] as? String
                  ) else { return nil }
            review.state = ReviewState(rawValue: object["state"] as? String ?? "open") ?? .open
            return review
        }
        let panel = scenario.bool("current") ? ReviewPanelView() : ReviewPanelView(styleSheet: styleSheet)
        if scenario.bool("current") { panel.styleSheet = styleSheet }
        let delegate = Delegate()
        panel.delegate = delegate
        self.delegate = delegate
        if !scenario.bool("noSource") { panel.sourceText = source }
        panel.reviews = reviews
        statuses = reviews.map { ReviewAnchorResolver.resolve($0.anchor, in: panel.sourceText).status.rawValue }
        let table = panel.subviews.compactMap { ($0 as? NSScrollView)?.documentView as? NSTableView }.first
        if let row = scenario.int("select"), let table {
            table.selectRowIndexes(IndexSet(integer: row), byExtendingSelection: false)
        }
        let buttons = panel.subviews.compactMap { $0 as? NSStackView }.flatMap { $0.arrangedSubviews.compactMap { $0 as? NSButton } }
        for name in scenario.strings("press") {
            let title = name.prefix(1).uppercased() + name.dropFirst()
            guard let button = buttons.first(where: { $0.title == title }) else { continue }
            button.sendAction(button.action, to: button.target)
        }
        self.panel = panel
        return panel
    }

    func model() -> JSON {
        guard let panel else { return .null }
        return .object([
            ("preferredWidth", .double(Double(panel.preferredWidth))),
            ("reviewCount", .int(panel.reviews.count)),
            ("statuses", .array(statuses.map { .string($0) })),
            ("events", .array((delegate?.events ?? []).map { .string($0) })),
            ("fittingSize", PanelTree.size(panel.fittingSize)),
        ])
    }
}
