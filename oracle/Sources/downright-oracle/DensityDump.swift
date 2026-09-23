import AppKit
import MarkdownCore
@testable import MarkdownRender

/// Hosts a `DensityGutterView` in a `MarkdownContainerView` the way
/// Downright's `DocumentWindowController` does: the gutter goes into the
/// leading (or trailing) accessory lane, and the controller's
/// `refreshDensityBands`, the gutter half of `updateBreadcrumbAndGutter` and
/// its `DensityGutterDelegate` methods are reproduced here. `upleft-oracle`
/// mirrors this class call for call.
@MainActor
final class DensityHost: DensityGutterDelegate {
    let container: MarkdownContainerView
    let gutter: DensityGutterView
    let text: String
    private(set) var document: ParsedDocument = MarkdownParser.parse("")
    /// Every fraction the gutter asked to scroll to, for `density-model`.
    private(set) var requestedFractions: [CGFloat] = []

    init(container: MarkdownContainerView, side: String, styleSheet: StyleSheet, text: String) {
        self.container = container
        self.text = text
        // `let densityGutterView = DensityGutterView()`.
        gutter = DensityGutterView()
        if side == "trailing" {
            container.trailingAccessory = gutter
        } else {
            container.leadingAccessory = gutter
        }
        gutter.delegate = self
        gutter.styleSheet = styleSheet
    }

    /// `DocumentWindowController.refreshDensityBands(metrics:)`, with the
    /// word count `sectionMetrics(for:)` caches.
    func refreshDensityBands(_ parsed: ParsedDocument) {
        document = parsed
        let metrics = Metrics.sectionMetrics(parsed)
        var wordCount = metrics.reduce(0) { $0 + $1.words }
        if wordCount == 0, parsed.length > 0 {
            wordCount = text.split(whereSeparator: { $0.isWhitespace }).count
        }
        let readMinutes = max(1, (wordCount + 199) / 200)
        gutter.metricsSummary = "\(wordCount) words · \(readMinutes) min read"
        gutter.bands = DensityGutterView.bands(for: parsed, changes: [], searchHits: [])
        let length = CGFloat(max(1, parsed.length))
        let current = visibleHeadingIndex(at: container.textView.topVisibleOffset)
        gutter.outlineEntries = parsed.headings.enumerated().map { index, heading in
            DensityOutlineEntry(
                title: heading.title,
                level: heading.level,
                fraction: CGFloat(heading.range.location) / length,
                isCurrent: index == current
            )
        }
        gutter.needsDisplay = true
    }

    /// The gutter half of `updateBreadcrumbAndGutter()`.
    func updateGutter() {
        let textView = container.textView
        let current = visibleHeadingIndex(at: textView.topVisibleOffset)
        let length = max(1, document.length)
        let top = CGFloat(textView.topVisibleOffset) / CGFloat(length)
        let visibleHeight = container.scrollView.contentView.bounds.height
        let documentHeight = max(1, container.scrollView.documentView?.bounds.height ?? 1)
        let span = min(1, visibleHeight / documentHeight)
        gutter.visibleRange = top...min(1, top + span)
        gutter.readProgress = max(gutter.readProgress, min(1, top + span))
        let previousCurrent = gutter.outlineEntries.firstIndex(where: \.isCurrent)
        if previousCurrent != current {
            gutter.outlineEntries = gutter.outlineEntries.enumerated().map { index, entry in
                var updated = entry
                updated.isCurrent = index == current
                return updated
            }
        }
    }

    func visibleHeadingIndex(at offset: Int) -> Int? {
        let headings = document.headings
        var low = 0
        var high = headings.count
        while low < high {
            let middle = (low + high) / 2
            if headings[middle].range.location <= offset {
                low = middle + 1
            } else {
                high = middle
            }
        }
        return low > 0 ? low - 1 : nil
    }

    func densityGutter(_ gutter: DensityGutterView, didRequestScrollToFraction fraction: CGFloat) {
        requestedFractions.append(fraction)
    }

    func densityGutter(
        _ gutter: DensityGutterView, previewAtFraction fraction: CGFloat
    ) -> (title: String, snippet: String, context: String)? {
        let offset = Int(fraction * CGFloat(document.length))
        guard let index = document.headings.lastIndex(where: { $0.range.location <= offset }) else {
            return ("Document start", "", gutter.metricsSummary)
        }
        let heading = document.headings[index]
        let sectionPosition = "Section \(index + 1) of \(document.headings.count)"
        let context = heading.wordCount > 0
            ? "\(sectionPosition) · \(heading.wordCount) words"
            : sectionPosition
        let textView = container.textView
        if textView.mode == .source || textView.sourceFocus == .document {
            let snippetLength = min(160, max(0, document.length - offset))
            return (
                heading.title,
                document.substring(NSRange(location: offset, length: snippetLength)),
                context
            )
        }
        return (
            heading.title,
            StructuralZoom.sectionPreview(document, headingIndex: index) ?? "Section overview",
            context
        )
    }
}

/// `density-hover`: a `MarkdownScene` with the density gutter, driven into
/// a hover state through the gutter's own event methods (the passive-hover
/// path: `mouseEntered` then `mouseMoved` at a mark), or into its outline
/// through `presentOutlineForKeyboard()`. The capture holds the main window
/// with every visible child window (the preview card or the outline panel)
/// stacked beneath it.
final class DensityHoverScene: CaptureScene {
    let markdown: MarkdownScene
    /// `0.25`, `0.5`, `0.75` (a mark at that share of the stack) or `outline`.
    let action: String
    private var window: NSWindow?
    private var readyAt = Date.distantFuture

    /// Time for the hover dwell and for any flashed overlay scroller to fade.
    static let driveDelay: TimeInterval = 0.3
    static let settleAfterDrive: TimeInterval = 1.5

    init(markdown: MarkdownScene, action: String) {
        self.markdown = markdown
        self.action = action
    }

    func build(in window: NSWindow, request: RenderRequest) throws -> NSView {
        self.window = window
        return try markdown.build(in: window, request: request)
    }

    func afterShow(window: NSWindow) {
        markdown.afterShow(window: window)
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.driveDelay) { [self] in
            drive()
            readyAt = Date().addingTimeInterval(Self.settleAfterDrive)
        }
    }

    private func drive() {
        MainActor.assumeIsolated { driveOnMain() }
    }

    @MainActor
    private func driveOnMain() {
        guard let gutter = markdown.densityHost?.gutter, let window else { return }
        if action == "outline" {
            gutter.presentOutlineForKeyboard()
            return
        }
        let share = CGFloat(Double(action) ?? 0.5)
        let positions = gutter.markPositionsForTesting
        guard !positions.isEmpty else { return }
        let index = Int((CGFloat(positions.count - 1) * share).rounded())
        let local = NSPoint(x: gutter.bounds.midX, y: positions[index])
        let location = gutter.convert(local, to: nil)
        guard let event = NSEvent.mouseEvent(
            with: .mouseMoved, location: location, modifierFlags: [], timestamp: 0,
            windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: 0, pressure: 0
        ) else { return }
        gutter.mouseEntered(with: event)
        gutter.mouseMoved(with: event)
    }

    func beforeSettleCheck() {
        markdown.beforeSettleCheck()
    }

    var isReady: Bool { Date() >= readyAt }

    func extraWindows() -> [NSWindow] {
        (window?.childWindows ?? []).filter(\.isVisible)
    }

    func writeExtras(bitmap: NSBitmapImageRep, request: RenderRequest) throws {}
}

/// `density-model`: the gutter's model — bands, thinning, pips, the stack
/// geometry, hover hysteresis, the mark layers it draws in each state, its
/// hit testing, the spring integration, and the outline's rows — for one
/// document, without a window.
@MainActor
enum DensityModelDump {
    static let heights: [CGFloat] = [100, 300, 700, 1400]

    /// Deterministic review overlays so the pip paths run: a change on every
    /// fourth top-level block, a search hit on every fifth.
    static func overlays(_ document: ParsedDocument) -> (changes: [(ChangeKind, NSRange)], hits: [NSRange]) {
        let kinds: [ChangeKind] = [.inserted, .modified, .deleted]
        var changes: [(ChangeKind, NSRange)] = []
        var hits: [NSRange] = []
        for (index, block) in document.root.children.enumerated() {
            if index % 4 == 1 { changes.append((kinds[(index / 4) % 3], block.range)) }
            if index % 5 == 2 {
                hits.append(NSRange(location: block.range.location, length: min(3, block.range.length)))
            }
        }
        return (changes, hits)
    }

    static func run(text: String, flags: Flags) throws -> JSON {
        _ = NSApplication.shared
        let document = MarkdownParser.parse(text)
        let (changes, hits) = overlays(document)
        let bands = DensityGutterView.bands(for: document, changes: changes, searchHits: hits)
        let plain = DensityGutterView.bands(for: document, changes: [], searchHits: [])
        let appearance = NSAppearance(named: flags.dark ? .darkAqua : .aqua)!
        guard let theme = ThemeStore.shared.themes.first(where: { $0.name == flags.theme }) else {
            throw OracleError.unknownTheme(flags.theme, ThemeStore.shared.themes.map(\.name))
        }
        let calm = StyleSheet(theme: theme, appearance: appearance, reduceMotionOverride: true)
        let lively = StyleSheet(theme: theme, appearance: appearance, reduceMotionOverride: false)
        return .object([
            ("length", .int(document.length)),
            ("bands", .array(bands.map(band))),
            ("plainBands", .int(plain.count)),
            ("tracks", .array(heights.map { track(height: $0, bands: bands) })),
            ("views", .array([
                view(height: 700, bands: bands, styleSheet: calm),
                view(height: 300, bands: bands, styleSheet: calm),
            ])),
            ("springs", springs(height: 700, bands: plain, styleSheet: lively)),
            ("outline", outline(document, styleSheet: calm)),
        ])
    }

    static func kind(_ kind: DensityBand.Kind) -> JSON {
        switch kind {
        case .heading(let level): return .string("heading\(level)")
        case .codeBlock: return .string("codeBlock")
        case .table: return .string("table")
        case .math: return .string("math")
        case .taskList: return .string("taskList")
        case .change(let change): return .string("change.\(change.rawValue)")
        case .searchHit: return .string("searchHit")
        case .image: return .string("image")
        case .callout: return .string("callout")
        }
    }

    static func band(_ band: DensityBand) -> JSON {
        .array([kind(band.kind), .double(band.startFraction), .double(band.endFraction)])
    }

    static func pip(_ pip: DensityGutterView.Pip) -> JSON {
        .array([.string(pip.change?.rawValue ?? ""), .bool(pip.searchHit)])
    }

    static func selection(_ selection: DensityGutterView.Selection) -> JSON {
        .object([
            ("marks", .array(selection.marks.map(band))),
            ("pips", .array(selection.pips.map(pip))),
        ])
    }

    static func doubles(_ values: [CGFloat]) -> JSON {
        .array(values.map { .double($0) })
    }

    static func track(height: CGFloat, bands: [DensityBand]) -> JSON {
        let track = DensityGutterView.trackRange(height: height)
        let trackHeight = track.bottom - track.top
        let capacity = DensityGutterView.stackCapacity(track: trackHeight)
        let selected = DensityGutterView.selection(for: bands, capacity: capacity)
        let plain = DensityGutterView.selection(for: bands, capacity: capacity, includeOverlays: false)
        let pitch = DensityGutterView.markPitch(track: trackHeight, count: selected.marks.count)
        let positions = DensityGutterView.centeredBandYPositions(
            height: height, count: selected.marks.count, markGap: pitch
        )
        var compressed: [JSON] = []
        for pointer in [height * 0.25, height * 0.5, height * 0.75] + Array(positions.prefix(3)) {
            compressed.append(doubles(DensityGutterView.centeredBandYPositions(
                height: height, count: selected.marks.count, markGap: pitch, pointerY: pointer
            )))
        }
        let current: [JSON] = [(0.0, 0.1), (0.3, 0.5), (0.9, 1.0)].map { range in
            DensityGutterView.currentHeadingFraction(in: selected.marks, at: range.0...range.1)
                .map { JSON.double($0) } ?? .null
        }
        let slop = DensityGutterView.dismissalSlop(for: positions)
        var sweep: [JSON] = []
        var previous: Int?
        var y: CGFloat = 0
        while y <= height {
            let next = DensityGutterView.nextHoveredBandIndex(
                at: y, positions: positions, currentIndex: previous,
                activationSlop: DensityGutterView.hoverActivationSlop, dismissalSlop: slop
            )
            sweep.append(.int(next ?? -1))
            previous = next
            y += 7
        }
        return .object([
            ("height", .double(height)),
            ("track", doubles([track.top, track.bottom])),
            ("capacity", .int(capacity)),
            ("selection", selection(selected)),
            ("plain", selection(plain)),
            ("pitch", .double(pitch)),
            ("positions", doubles(positions)),
            ("compressed", .array(compressed)),
            ("current", .array(current)),
            ("dismissalSlop", .double(slop)),
            ("sweep", .array(sweep)),
            ("influence", doubles(stride(from: 0.0, through: 40.0, by: 5.0).map {
                DensityGutterView.proximityInfluence(distance: CGFloat($0))
            })),
        ])
    }

    static func cgColor(_ color: CGColor?) -> JSON {
        guard let color else { return .null }
        return .object([
            ("space", .string((color.colorSpace?.name as String?) ?? "")),
            ("components", .array((color.components ?? []).map { .double(Double($0)) })),
        ])
    }

    static func layers(_ view: NSView) -> JSON {
        .array((view.layer?.sublayers ?? []).map { layer in
            .object([
                ("frame", AttributeDump.rect(layer.frame)),
                ("cornerRadius", .double(layer.cornerRadius)),
                ("hidden", .bool(layer.isHidden)),
                ("background", cgColor(layer.backgroundColor)),
                ("shadowOpacity", .double(Double(layer.shadowOpacity))),
                ("shadowRadius", .double(layer.shadowRadius)),
                ("shadowColor", cgColor(layer.shadowColor)),
            ])
        })
    }

    static func event(_ type: NSEvent.EventType, at y: CGFloat, in gutter: DensityGutterView) -> NSEvent {
        let location = gutter.convert(NSPoint(x: gutter.bounds.midX, y: y), to: nil)
        return NSEvent.mouseEvent(
            with: type, location: location, modifierFlags: [], timestamp: 0,
            windowNumber: 0, context: nil, eventNumber: 0, clickCount: 1, pressure: 1
        )!
    }

    final class Recorder: DensityGutterDelegate {
        var fractions: [CGFloat] = []
        func densityGutter(_ gutter: DensityGutterView, didRequestScrollToFraction fraction: CGFloat) {
            fractions.append(fraction)
        }
        func densityGutter(
            _ gutter: DensityGutterView, previewAtFraction fraction: CGFloat
        ) -> (title: String, snippet: String, context: String)? { nil }
    }

    static func gutter(height: CGFloat, bands: [DensityBand], styleSheet: StyleSheet, recorder: Recorder) -> DensityGutterView {
        let gutter = DensityGutterView(styleSheet: styleSheet)
        gutter.performHapticFeedback = {}
        gutter.delegate = recorder
        gutter.frame = NSRect(x: 0, y: 0, width: DensityGutterView.width, height: height)
        gutter.bands = bands
        gutter.layoutSubtreeIfNeeded()
        return gutter
    }

    static func view(height: CGFloat, bands: [DensityBand], styleSheet: StyleSheet) -> JSON {
        let recorder = Recorder()
        let gutter = gutter(height: height, bands: bands, styleSheet: styleSheet, recorder: recorder)
        var states: [JSON] = []
        func record(_ name: String) {
            states.append(.object([("state", .string(name)), ("layers", layers(gutter))]))
        }
        record("rest")
        gutter.visibleRange = 0...0.2
        gutter.readProgress = 0.2
        record("top")
        gutter.showsOverlayPips = true
        record("pips")
        let positions = gutter.markPositionsForTesting
        var targets: [CGFloat] = [height / 2]
        if let first = positions.first, let last = positions.last {
            targets += [first, positions[positions.count / 2], last, first + 5]
        }
        for (index, y) in targets.enumerated() {
            gutter.driveHoverForTesting(toY: y)
            record("hover\(index)")
        }
        gutter.visibleRange = 0.5...0.7
        gutter.readProgress = 0.7
        record("scrolled")

        var hits: [JSON] = []
        var samples: [CGFloat] = [0, 10, height / 4, height / 2, height * 3 / 4, height - 10]
        for y in positions.prefix(4) { samples += [y - 3, y + 3] }
        for y in samples {
            gutter.mouseDown(with: event(.leftMouseDown, at: y, in: gutter))
            gutter.mouseUp(with: event(.leftMouseUp, at: y, in: gutter))
            let click = recorder.fractions.last
            gutter.mouseDown(with: event(.leftMouseDown, at: y, in: gutter))
            gutter.mouseDragged(with: event(.leftMouseDragged, at: y + 12, in: gutter))
            gutter.mouseUp(with: event(.leftMouseUp, at: y + 12, in: gutter))
            let drag = recorder.fractions.last
            hits.append(.array([.double(y), click.map { .double($0) } ?? .null, drag.map { .double($0) } ?? .null]))
        }
        gutter.mouseExited(with: event(.mouseMoved, at: -40, in: gutter))
        record("exited")
        return .object([
            ("height", .double(height)),
            ("positions", doubles(positions)),
            ("states", .array(states)),
            ("hits", .array(hits)),
            ("requested", doubles(recorder.fractions)),
            ("valueDescription", .string(gutter.accessibilityValueDescription() ?? "")),
        ])
    }

    /// Reduce Motion off: pointer events retarget the springs, and the
    /// driver's tick is stepped by hand (there is no window, so no display
    /// link), a frame at a time.
    static func springs(height: CGFloat, bands: [DensityBand], styleSheet: StyleSheet) -> JSON {
        let recorder = Recorder()
        let gutter = gutter(height: height, bands: bands, styleSheet: styleSheet, recorder: recorder)
        let positions = gutter.markPositionsForTesting
        guard let first = positions.first else { return .null }
        let target = positions[positions.count / 2]
        var frames: [JSON] = []
        gutter.mouseEntered(with: event(.mouseMoved, at: first, in: gutter))
        gutter.mouseMoved(with: event(.mouseMoved, at: target, in: gutter))
        for frame in 1...6 {
            let moving = gutter.springTick(dt: 1.0 / 120.0)
            gutter.springApply()
            if frame % 2 == 0 {
                frames.append(.object([("moving", .bool(moving)), ("layers", layers(gutter))]))
            }
        }
        var ticks = 0
        while gutter.springTick(dt: 1.0 / 120.0), ticks < 600 { ticks += 1 }
        gutter.springApply()
        frames.append(.object([("settledAfter", .int(ticks)), ("layers", layers(gutter))]))
        gutter.mouseExited(with: event(.mouseMoved, at: -40, in: gutter))
        for _ in 1...3 { _ = gutter.springTick(dt: 1.0 / 60.0) }
        gutter.springApply()
        frames.append(.object([("exiting", .bool(true)), ("layers", layers(gutter))]))
        return .array(frames)
    }

    static func outline(_ document: ParsedDocument, styleSheet: StyleSheet) -> JSON {
        let length = CGFloat(max(1, document.length))
        var low = 0
        var high = document.headings.count
        while low < high {
            let middle = (low + high) / 2
            if document.headings[middle].range.location <= 0 { low = middle + 1 } else { high = middle }
        }
        let current = low > 0 ? low - 1 : nil
        let entries = document.headings.enumerated().map { index, heading in
            DensityOutlineEntry(
                title: heading.title,
                level: heading.level,
                fraction: CGFloat(heading.range.location) / length,
                isCurrent: index == current
            )
        }
        let window = DensityOutlineWindow(styleSheet: styleSheet)
        window.entries = entries
        let table = NSTableView()
        var rows: [JSON] = []
        for row in 0..<min(entries.count, 40) {
            guard let cell = window.tableView(table, viewFor: nil, row: row) else {
                rows.append(.null)
                continue
            }
            let label = cell.subviews.compactMap { $0 as? NSTextField }.first
            let leading = cell.constraints.first {
                ($0.firstItem as? NSView) === label && $0.firstAttribute == .leading
            }
            let title: String = label?.stringValue ?? ""
            let font: JSON = label?.font.map(AttributeDump.fontJSON) ?? .null
            let color: JSON = label?.textColor.map(AttributeDump.colorJSON) ?? .null
            let lineBreakMode: Int = Int(label?.lineBreakMode.rawValue ?? 0)
            let leadingConstant: JSON = leading.map { JSON.double(Double($0.constant)) } ?? .null
            let cornerRadius: Double = Double(cell.layer?.cornerRadius ?? -1)
            let background: JSON = cgColor(cell.layer?.backgroundColor)
            let pairs: [(String, JSON)] = [
                ("title", .string(title)),
                ("font", font),
                ("color", color),
                ("lineBreakMode", .int(lineBreakMode)),
                ("leading", leadingConstant),
                ("cornerRadius", .double(cornerRadius)),
                ("background", background),
            ]
            rows.append(.object(pairs))
        }
        return .object([
            ("entries", .array(entries.map { entry in
                .array([.string(entry.title), .int(entry.level), .double(entry.fraction), .bool(entry.isCurrent)])
            })),
            ("rows", .int(window.numberOfRows(in: table))),
            ("cells", .array(rows)),
        ])
    }
}
