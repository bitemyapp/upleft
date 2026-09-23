import AppKit
import ScreenCaptureKit
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
    /// Capture the composited window (default) rather than `cacheDisplay`.
    var captureFromScreen = true
}

/// What a capture shows. `CaptureSession` owns the window, the settle loop
/// and the capture; a scene owns the content. `upleft-oracle` mirrors both.
protocol CaptureScene: AnyObject {
    /// Builds the content in `window` and returns the view whose cached
    /// display decides when the scene has settled.
    func build(in window: NSWindow, request: RenderRequest) throws -> NSView
    /// Runs once, after the window is ordered on screen.
    func afterShow(window: NSWindow)
    /// Runs before every settle check.
    func beforeSettleCheck()
    /// Writes any extra outputs once the scene has settled.
    func writeExtras(bitmap: NSBitmapImageRep, request: RenderRequest) throws
    /// False while the scene is still driving itself into the state to
    /// capture; the session only counts stable captures once it is ready.
    var isReady: Bool { get }
    /// Further windows (child windows, panels) captured below the main
    /// window, top to bottom, in the same PNG.
    func extraWindows() -> [NSWindow]
}

extension CaptureScene {
    var isReady: Bool { true }
    func extraWindows() -> [NSWindow] { [] }
}

/// Shows a scene in an on-screen borderless window of a running, activated
/// application, waits until three consecutive `cacheDisplay` captures are
/// byte-identical (late image decodes and fragment caches have landed), then
/// captures the composited window with ScreenCaptureKit. TextKit 2 draws only
/// inside a real display cycle, so a headless capture would be blank
/// (Downright's `RenderSmokeTests` documents the same constraint).
final class CaptureSession: NSObject, NSApplicationDelegate {
    let request: RenderRequest
    let scene: CaptureScene
    private var window: NSWindow!
    private var settleView: NSView!
    private var previousCapture: Data?
    private var stableCaptures = 0
    private var deadline = Date.distantFuture

    init(request: RenderRequest, scene: CaptureScene) {
        self.request = request
        self.scene = scene
    }

    static func run(request: RenderRequest, scene: CaptureScene) -> Never {
        acquireWindowCaptureLock()
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        let session = CaptureSession(request: request, scene: scene)
        app.delegate = session
        app.run()
        exit(0)
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        do {
            try start()
        } catch {
            fail("\(error)")
        }
    }

    private func start() throws {
        let appearance = NSAppearance(named: request.dark ? .darkAqua : .aqua)!
        let frame = NSRect(x: 0, y: 0, width: request.width, height: request.height)
        window = NSWindow(contentRect: frame, styleMask: [.borderless], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.appearance = appearance
        window.colorSpace = .sRGB
        settleView = try scene.build(in: window, request: request)

        NSApp.activate(ignoringOtherApps: true)
        window.orderFrontRegardless()
        scene.afterShow(window: window)
        deadline = Date().addingTimeInterval(request.settleTimeout)
        scheduleCheck()
    }

    private func scheduleCheck() {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { [weak self] in
            self?.checkSettled()
        }
    }

    private func checkSettled() {
        scene.beforeSettleCheck()
        settleView.displayIfNeeded()
        guard let rep = settleView.bitmapImageRepForCachingDisplay(in: settleView.bounds) else {
            fail("no bitmap representation")
            return
        }
        settleView.cacheDisplay(in: settleView.bounds, to: rep)
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
        if !scene.isReady { stableCaptures = 0 }
        guard stableCaptures >= 2 || Date() > deadline else {
            scheduleCheck()
            return
        }
        if stableCaptures < 2 {
            FileHandle.standardError.write("warning: render did not settle before the timeout\n".data(using: .utf8)!)
        }
        do {
            try scene.writeExtras(bitmap: rep, request: request)
            if !request.captureFromScreen {
                try viewCapture(main: rep, png: png).write(to: request.outputPNG)
                exit(0)
            }
        } catch {
            fail("write failed: \(error)")
            return
        }
        Task { @MainActor in
            do {
                try await self.captureWindowFromScreen(to: self.request.outputPNG)
                exit(0)
            } catch {
                self.fail("window capture failed: \(error)")
            }
        }
    }

    /// The window as the compositor shows it. `cacheDisplay` redraws views
    /// into a bitmap clipped to each view's bounds, but on screen TextKit 2
    /// fragments are layers, so only a window capture records what a reader
    /// actually sees. ScreenCaptureKit captures this process's own window;
    /// the `screencapture` tool proved unreliable (it hangs intermittently).
    private func captureWindowFromScreen(to url: URL) async throws {
        let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
        var image = try await captureImage(of: window, in: content)
        let extras = scene.extraWindows()
        if !extras.isEmpty {
            var images = [image]
            for extra in extras { images.append(try await captureImage(of: extra, in: content)) }
            image = try stackImages(images)
        }
        let rep = NSBitmapImageRep(cgImage: image)
        guard let png = rep.representation(using: .png, properties: [:]) else {
            throw OracleError.usage("PNG encoding of the window capture failed")
        }
        try png.write(to: url)
    }

    /// `--capture view`: the settle view's cached display, with each extra
    /// window's content view cached and stacked beneath it.
    private func viewCapture(main: NSBitmapImageRep, png: Data) throws -> Data {
        let extras = scene.extraWindows()
        guard !extras.isEmpty, let first = main.cgImage else { return png }
        var images = [first]
        for extra in extras {
            guard let view = extra.contentView,
                  let rep = view.bitmapImageRepForCachingDisplay(in: view.bounds) else {
                throw OracleError.usage("no bitmap representation for an extra window")
            }
            view.cacheDisplay(in: view.bounds, to: rep)
            guard let image = rep.cgImage else { throw OracleError.usage("no image for an extra window") }
            images.append(image)
        }
        guard let data = NSBitmapImageRep(cgImage: try stackImages(images)).representation(using: .png, properties: [:]) else {
            throw OracleError.usage("PNG encoding of the stacked capture failed")
        }
        return data
    }

    private func captureImage(of window: NSWindow, in content: SCShareableContent) async throws -> CGImage {
        guard let scWindow = content.windows.first(where: { $0.windowID == CGWindowID(window.windowNumber) }) else {
            throw OracleError.usage("ScreenCaptureKit does not list the render window")
        }
        let filter = SCContentFilter(desktopIndependentWindow: scWindow)
        let configuration = SCStreamConfiguration()
        configuration.width = Int(filter.contentRect.width * CGFloat(filter.pointPixelScale))
        configuration.height = Int(filter.contentRect.height * CGFloat(filter.pointPixelScale))
        configuration.showsCursor = false
        configuration.ignoreShadowsSingleWindow = true
        configuration.captureResolution = .best
        return try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: configuration)
    }

    /// The captures one above the other, left-aligned, in the first
    /// capture's colour space (so no pixel is colour-converted).
    private func stackImages(_ images: [CGImage]) throws -> CGImage {
        let width = images.map(\.width).max() ?? 0
        let height = images.map(\.height).reduce(0, +)
        guard let space = images.first?.colorSpace,
              let context = CGContext(
                data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0, space: space,
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
              ) else {
            throw OracleError.usage("cannot make the stacking context")
        }
        var top = height
        for image in images {
            top -= image.height
            context.draw(image, in: CGRect(x: 0, y: top, width: image.width, height: image.height))
        }
        guard let stacked = context.makeImage() else { throw OracleError.usage("stacking failed") }
        return stacked
    }

    private func fail(_ message: String) -> Never {
        FileHandle.standardError.write("render failed: \(message)\n".data(using: .utf8)!)
        exit(2)
    }
}

/// One window capture at a time on this machine. Two captures on screen at
/// once compete for activation and window order, so every capture (from any
/// worktree, oracle or agent) takes this lock for its whole process lifetime.
/// `upleft-oracle` takes the same lock.
func acquireWindowCaptureLock() {
    let fd = open("/tmp/upleft-window-capture.lock", O_CREAT | O_RDWR, 0o644)
    guard fd >= 0, flock(fd, LOCK_EX) == 0 else {
        FileHandle.standardError.write("render failed: cannot take /tmp/upleft-window-capture.lock\n".data(using: .utf8)!)
        exit(2)
    }
    // Deliberately never closed: the lock is released when the process exits.
}

/// Downright's real `MarkdownContainerView`, set up the way the app's
/// `DocumentWindowController` sets up a document window.
final class MarkdownScene: CaptureScene {
    private var container: MarkdownContainerView!
    /// `--density leading|trailing` (see `DensityHost`).
    let density: String?
    private(set) var densityHost: DensityHost?

    init(density: String? = nil) {
        self.density = density
    }

    func build(in window: NSWindow, request: RenderRequest) throws -> NSView {
        let text = try String(contentsOf: request.input, encoding: .utf8)
        let appearance = NSAppearance(named: request.dark ? .darkAqua : .aqua)!
        guard let theme = ThemeStore.shared.themes.first(where: { $0.name == request.themeName }) else {
            throw OracleError.unknownTheme(request.themeName, ThemeStore.shared.themes.map(\.name))
        }
        let styleSheet = StyleSheet(theme: theme, appearance: appearance, reduceMotionOverride: true)
        let storage = NSTextStorage(string: text)
        container = MarkdownContainerView(storage: storage, styleSheet: styleSheet)
        if let density {
            let container = container!
            densityHost = MainActor.assumeIsolated {
                DensityHost(container: container, side: density, styleSheet: styleSheet, text: text)
            }
        }
        container.frame = NSRect(x: 0, y: 0, width: request.width, height: request.height)
        window.contentView = container
        // As in the app: the container is laid out in its window (which sets
        // the responsive measure and the text view's size limits) before the
        // first document update resizes the text view to its content.
        window.layoutIfNeeded()
        container.layoutSubtreeIfNeeded()
        container.textView.mode = request.mode
        let document = MarkdownParser.parse(text)
        container.textView.update(document: document, dirty: .wholesale)
        if let densityHost { MainActor.assumeIsolated { densityHost.refreshDensityBands(document) } }
        return container
    }

    /// The first-frame sequence of Downright's `DocumentWindowController`
    /// (`restoreInitialReadingPositionIfReady`), minus the reading-position
    /// restore.
    func afterShow(window: NSWindow) {
        window.layoutIfNeeded()
        container.layoutSubtreeIfNeeded()
        container.textView.resizeToFitContent()
        container.textView.scroll(toOffset: 0, position: .top, animated: false)
        container.textView.prepareForDisplay()
        container.textView.displayIfNeeded()
        if let densityHost { MainActor.assumeIsolated { densityHost.updateGutter() } }
    }

    func beforeSettleCheck() {
        container.layoutSubtreeIfNeeded()
        if let layout = container.textView.textLayoutManager {
            layout.ensureLayout(for: layout.documentRange)
        }
    }

    func writeExtras(bitmap: NSBitmapImageRep, request: RenderRequest) throws {
        guard let layoutURL = request.outputLayout else { return }
        try LayoutDump.textView(container.textView, container: container, bitmap: bitmap)
            .text.write(to: layoutURL, atomically: true, encoding: .utf8)
    }
}

/// A stock TextKit 2 text view showing the input verbatim. Nothing here is
/// Downright's: the `probe` suite exists to prove the two oracles' window,
/// settle and capture machinery are pixel-identical before any port is
/// judged by it.
final class ProbeScene: CaptureScene {
    private var scrollView: NSScrollView!

    func build(in window: NSWindow, request: RenderRequest) throws -> NSView {
        let text = try String(contentsOf: request.input, encoding: .utf8)
        scrollView = NSTextView.scrollableTextView()
        scrollView.frame = NSRect(x: 0, y: 0, width: request.width, height: request.height)
        let textView = scrollView.documentView as! NSTextView
        textView.font = NSFont.systemFont(ofSize: 15)
        textView.textContainerInset = NSSize(width: 24, height: 24)
        textView.string = text
        window.contentView = scrollView
        return scrollView
    }

    func afterShow(window: NSWindow) {
        window.layoutIfNeeded()
    }

    func beforeSettleCheck() {}

    func writeExtras(bitmap: NSBitmapImageRep, request: RenderRequest) throws {}
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
