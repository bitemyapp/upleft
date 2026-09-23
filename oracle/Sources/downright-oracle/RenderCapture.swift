import AppKit
import MarkdownCore
import MarkdownRender

/// What to render and how. The Rust oracle accepts the same flags.
struct RenderRequest {
    var input: URL
    var outputPNG: URL
    var outputLayout: URL?
    var mode: RenderMode = .live
    var themeName: String = "Paper Light"
    var dark = false
    var width: CGFloat = 1000
    var height: CGFloat = 1400
    /// Upper bound on waiting for the view to settle (images decode off the
    /// main thread; motion must finish).
    var settleTimeout: TimeInterval = 8
}

/// Renders a document through Downright's real `MarkdownContainerView` in an
/// on-screen borderless window of a running, activated application, then
/// captures it with `cacheDisplay`. TextKit 2 draws only inside a real display
/// cycle, so a headless capture would be blank (Downright's
/// `RenderSmokeTests` documents the same constraint).
final class RenderSession: NSObject, NSApplicationDelegate {
    let request: RenderRequest
    private var window: NSWindow!
    private var container: MarkdownContainerView!
    private var previousCapture: Data?
    private var stableCaptures = 0
    private var deadline = Date.distantFuture

    init(request: RenderRequest) {
        self.request = request
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        do {
            try start()
        } catch {
            FileHandle.standardError.write("render failed: \(error)\n".data(using: .utf8)!)
            exit(2)
        }
    }

    private func start() throws {
        let text = try String(contentsOf: request.input, encoding: .utf8)
        let appearance = NSAppearance(named: request.dark ? .darkAqua : .aqua)!
        guard let theme = ThemeStore.shared.themes.first(where: { $0.name == request.themeName }) else {
            throw OracleError.unknownTheme(request.themeName, ThemeStore.shared.themes.map(\.name))
        }
        let styleSheet = StyleSheet(theme: theme, appearance: appearance, reduceMotionOverride: true)

        let frame = NSRect(x: 0, y: 0, width: request.width, height: request.height)
        window = NSWindow(contentRect: frame, styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.appearance = appearance
        window.colorSpace = .sRGB

        let storage = NSTextStorage(string: text)
        container = MarkdownContainerView(storage: storage, styleSheet: styleSheet)
        container.frame = frame
        window.contentView = container
        // As in the app: the container is laid out in its window (which sets
        // the responsive measure and the text view's size limits) before the
        // first document update resizes the text view to its content.
        window.layoutIfNeeded()
        container.layoutSubtreeIfNeeded()
        container.textView.mode = request.mode
        container.textView.update(document: MarkdownParser.parse(text), dirty: .wholesale)

        NSApp.activate(ignoringOtherApps: true)
        window.orderFrontRegardless()
        prepareFirstFrame()
        deadline = Date().addingTimeInterval(request.settleTimeout)
        scheduleCapture()
    }

    /// The first-frame sequence of Downright's `DocumentWindowController`
    /// (`restoreInitialReadingPositionIfReady`), minus the reading-position
    /// restore: without `resizeToFitContent` the text view keeps its initial
    /// frame and clips the measure.
    private func prepareFirstFrame() {
        window.layoutIfNeeded()
        container.layoutSubtreeIfNeeded()
        container.textView.resizeToFitContent()
        container.textView.scroll(toOffset: 0, position: .top, animated: false)
        container.textView.prepareForDisplay()
        container.textView.displayIfNeeded()
    }

    private func scheduleCapture() {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { [weak self] in
            self?.captureIfSettled()
        }
    }

    /// Captures until three consecutive frames are byte-identical, so late
    /// image decodes and fragment caches have landed before the result is
    /// taken as the picture.
    private func captureIfSettled() {
        container.layoutSubtreeIfNeeded()
        if let layout = container.textView.textLayoutManager {
            layout.ensureLayout(for: layout.documentRange)
        }
        container.displayIfNeeded()
        guard let rep = container.bitmapImageRepForCachingDisplay(in: container.bounds) else {
            fail("no bitmap representation")
            return
        }
        container.cacheDisplay(in: container.bounds, to: rep)
        guard let png = rep.representation(using: .png, properties: [:]) else {
            fail("PNG encoding failed")
            return
        }
        if png == previousCapture {
            stableCaptures += 1
        } else {
            stableCaptures = 0
            previousCapture = png
        }
        if stableCaptures >= 2 || Date() > deadline {
            do {
                try png.write(to: request.outputPNG)
                if let layoutURL = request.outputLayout {
                    try LayoutDump.textView(container.textView, container: container, bitmap: rep)
                        .text.write(to: layoutURL, atomically: true, encoding: .utf8)
                }
            } catch {
                fail("write failed: \(error)")
                return
            }
            if Date() > deadline, stableCaptures < 2 {
                FileHandle.standardError.write("warning: render did not settle before the timeout\n".data(using: .utf8)!)
            }
            exit(0)
        }
        scheduleCapture()
    }

    private func fail(_ message: String) {
        FileHandle.standardError.write("render failed: \(message)\n".data(using: .utf8)!)
        exit(2)
    }
}

enum OracleError: Error, CustomStringConvertible {
    case unknownTheme(String, [String])
    case usage(String)

    var description: String {
        switch self {
        case .unknownTheme(let name, let known): return "unknown theme \(name); known: \(known.joined(separator: ", "))"
        case .usage(let message): return message
        }
    }
}

/// Geometry of the laid-out document, to localise a pixel mismatch to the
/// fragment and line that produced it.
enum LayoutDump {
    static func textView(_ textView: MarkdownTextView, container: MarkdownContainerView, bitmap: NSBitmapImageRep) -> JSON {
        var fragments: [JSON] = []
        if let layout = textView.textLayoutManager, let content = layout.textContentManager {
            let documentStart = content.documentRange.location
            layout.enumerateTextLayoutFragments(from: documentStart, options: [.ensuresLayout]) { fragment in
                let range = fragment.rangeInElement
                let start = content.offset(from: documentStart, to: range.location)
                let end = content.offset(from: documentStart, to: range.endLocation)
                let lines = fragment.textLineFragments.map { line in
                    JSON.object([
                        ("characterRange", .range(line.characterRange)),
                        ("typographicBounds", AttributeDump.rect(line.typographicBounds)),
                        ("glyphOrigin", .array([.double(line.glyphOrigin.x), .double(line.glyphOrigin.y)])),
                    ])
                }
                fragments.append(.object([
                    ("class", .string(String(describing: type(of: fragment)))),
                    ("range", .array([.int(start), .int(end - start)])),
                    ("frame", AttributeDump.rect(fragment.layoutFragmentFrame)),
                    ("renderingSurfaceBounds", AttributeDump.rect(fragment.renderingSurfaceBounds)),
                    ("lines", .array(lines)),
                ]))
                return true
            }
        }
        return .object([
            ("bitmap", .object([
                ("pixelsWide", .int(bitmap.pixelsWide)),
                ("pixelsHigh", .int(bitmap.pixelsHigh)),
                ("bitsPerPixel", .int(bitmap.bitsPerPixel)),
                ("colorSpace", .string(bitmap.colorSpace.localizedName ?? "")),
            ])),
            ("containerFrame", AttributeDump.rect(container.frame)),
            ("scrollViewFrame", AttributeDump.rect(container.scrollView.frame)),
            ("textViewFrame", AttributeDump.rect(textView.frame)),
            ("textContainerSize", .array([
                .double(Double(textView.textContainer?.size.width ?? 0)),
                .double(Double(textView.textContainer?.size.height ?? 0)),
            ])),
            ("fragments", .array(fragments)),
        ])
    }
}
