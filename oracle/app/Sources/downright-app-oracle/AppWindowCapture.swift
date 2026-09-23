import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

// `app-window <scenario.json> <out.png> [--layout out.json]`
// `bench-app-window <scenario.json> <out.json>`
//
// The window-level suite: builds one of the app's real windows — a document
// window over a corpus file, the start window, the first-run setup panel, or
// a Settings pane — the way the app builds it, and captures the window server's
// composite of the whole window (`WindowServerCapture`). Nothing appears on screen and
// nothing takes focus: the app is never activated, and the window is moved
// far outside every display before it is ordered in (see `OffScreenWindows`
// for why a titled window needs help to stay there). An off-screen window
// draws in its inactive appearance, on both sides.
//
// Every run starts from a fresh sandbox: HOME, CFFIXED_USER_HOME and
// DOWNRIGHT_SUPPORT_DIRECTORY point into a new temporary folder before
// anything reads them, the scenario's `preferences` object (if any) is written
// as the settings file, and this process's own defaults domain is cleared, so
// ThemeStore and Settings start from their defaults. The one write outside the
// sandbox is `Preferences.shared`'s Quick Look publication
// (`com.bitemyapp.upleft.quickLook.*` in the global domain), which Upleft
// itself writes on every launch.
//
// `upleft-oracle` (crates/conformance/src/dump/app_window.rs) mirrors every
// step of this file.

/// One scenario file under `corpus/app-window/`.
struct AppWindowScenario {
    /// "document", "start", "setup" or "preferences".
    var window: String
    /// Repository-relative path of the document (`document` windows).
    var document: String?
    var mode: RenderMode = .live
    /// Forces `NSApp.appearance`: "light" or "dark".
    var dark = false
    /// Content size applied after the controller builds its window.
    var size: NSSize?
    /// Written verbatim as `preferences.json`; absent means no settings file
    /// (a first run).
    var preferences: Data?
    /// Settings pane to select (`preferences` windows).
    var pane: String?
    /// `StartGuideOffer` for the start window: "unavailable", "secondary",
    /// "primary".
    var guide: String = "unavailable"
    /// Recent documents seeded into the sandbox before anything reads them
    /// (`AppWindowSandbox.seedRecents`): each `{"path": "folder/name.md",
    /// "heading": String?, "opened": ISO 8601 String, "words": Int?}`.
    var recents: [[String: Any]] = []
    /// Commands (`Command` raw values) performed on the document window once
    /// its first frame has settled, in order, each followed by a settle.
    var commands: [String] = []
    var settleTimeout: TimeInterval = 10

    init(url: URL) throws {
        let data = try Data(contentsOf: url)
        guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let window = object["window"] as? String
        else { throw AppOracleError(description: "scenario needs a \"window\"") }
        self.window = window
        document = object["document"] as? String
        if let mode = object["mode"] as? String {
            guard let parsed = RenderMode(rawValue: mode) else {
                throw AppOracleError(description: "unknown mode \(mode)")
            }
            self.mode = parsed
        }
        dark = (object["appearance"] as? String) == "dark"
        if let size = object["size"] as? [Double], size.count == 2 {
            self.size = NSSize(width: size[0], height: size[1])
        }
        if let preferences = object["preferences"] {
            self.preferences = try JSONSerialization.data(withJSONObject: preferences, options: [.prettyPrinted, .sortedKeys])
        }
        pane = object["pane"] as? String
        if let guide = object["guide"] as? String { self.guide = guide }
        recents = object["recents"] as? [[String: Any]] ?? []
        commands = object["commands"] as? [String] ?? []
        if let timeout = object["settleTimeout"] as? Double { settleTimeout = timeout }
    }
}

enum AppWindowSandbox {
    /// A fresh home and support folder for this process, installed in the
    /// environment before any store reads it.
    static func prepare(_ scenario: AppWindowScenario) throws -> URL {
        let root = URL(fileURLWithPath: NSTemporaryDirectory(), isDirectory: true)
            .appendingPathComponent("upleft-app-window-\(getpid())-\(UUID().uuidString)", isDirectory: true)
        let home = root.appendingPathComponent("home", isDirectory: true)
        let support = root.appendingPathComponent("support", isDirectory: true)
        try FileManager.default.createDirectory(at: home, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: support, withIntermediateDirectories: true)
        setenv("HOME", home.path, 1)
        setenv("CFFIXED_USER_HOME", home.path, 1)
        setenv("DOWNRIGHT_SUPPORT_DIRECTORY", support.path, 1)
        if let preferences = scenario.preferences {
            try preferences.write(to: support.appendingPathComponent("preferences.json"))
        }
        try seedRecents(scenario.recents, root: root, support: support)
        clearOwnDefaults()
        return root
    }

    /// Writes each recent's file under `<root>/recents/` (a one-line heading,
    /// so `recents(limit:)` finds it on disk) and `recents.json` in the
    /// support folder, in the scenario's order, as `DocumentStateStore`
    /// stores it: absolute path, the file name without its extension, the
    /// heading, the date and the word count.
    static func seedRecents(_ recents: [[String: Any]], root: URL, support: URL) throws {
        guard !recents.isEmpty else { return }
        let folder = root.appendingPathComponent("recents", isDirectory: true)
        var entries: [[String: Any]] = []
        for recent in recents {
            guard let relative = recent["path"] as? String, let opened = recent["opened"] as? String else {
                throw AppOracleError(description: "a recent needs \"path\" and \"opened\"")
            }
            let heading = recent["heading"] as? String ?? ""
            let file = folder.appendingPathComponent(relative)
            try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
            try Data("# \(heading)\n".utf8).write(to: file)
            entries.append([
                "path": file.path,
                "displayName": file.deletingPathExtension().lastPathComponent,
                "firstHeading": heading,
                "lastOpened": opened,
                "wordCount": recent["words"] as? Int ?? 0,
            ])
        }
        try JSONSerialization.data(withJSONObject: entries).write(to: support.appendingPathComponent("recents.json"))
    }

    /// This process's own defaults domain (never the user's app domain):
    /// ThemeStore's selection and Settings' remembered pane start unset.
    static func clearOwnDefaults() {
        let domain = Bundle.main.bundleIdentifier ?? ProcessInfo.processInfo.processName
        UserDefaults.standard.removePersistentDomain(forName: domain)
    }

    static func remove(_ root: URL) {
        clearOwnDefaults()
        try? FileManager.default.removeItem(at: root)
    }
}

/// What the app's launch does before any window is built:
/// `AppDelegate.applySelectedTheme`, with the system appearance taken from
/// the scenario rather than the machine. The application icon is the one
/// the bundle carries (`CFBundleIconFile` AppIcon): an oracle binary has no
/// bundle, and AppKit would otherwise show the icon of the folder it runs
/// from.
@MainActor
func applyScenarioAppearance(_ scenario: AppWindowScenario, repositoryRoot: URL) {
    NSApp.applicationIconImage = NSImage(contentsOf: repositoryRoot.appendingPathComponent("vendor/downright/Resources/AppIcon.icns"))
    let appearance = NSAppearance(named: scenario.dark ? .darkAqua : .aqua)!
    NSApp.appearance = appearance
    ThemeStore.shared.select(named: Preferences.shared.themeName(for: appearance))
}

/// AppKit pulls a titled window back onto a display when it is ordered in
/// (`-[NSWindow constrainFrameRect:toScreen:]`), which would show it on the
/// owner's screen; only borderless windows stay where they are put. The
/// harness replaces that one method with the identity for the whole process,
/// before any window exists, so every window stays at (-30000, -30000).
/// Downright never overrides it, and a window that is not on a screen is not
/// constrained anyway, so nothing drawn changes. `upleft-oracle` does the same.
enum OffScreenWindows {
    static func install() {
        let selector = #selector(NSWindow.constrainFrameRect(_:to:))
        guard let method = class_getInstanceMethod(NSWindow.self, selector) else { return }
        let identity: @convention(block) (NSWindow, NSRect, NSScreen?) -> NSRect = { _, rect, _ in rect }
        method_setImplementation(method, imp_implementationWithBlock(identity))
    }

    /// Refuses to go on if a window touches any display.
    static func verify(_ windows: [NSWindow]) {
        for window in windows where NSScreen.screens.contains(where: { $0.frame.intersects(window.frame) }) {
            for window in NSApp.windows { window.orderOut(nil) }
            FileHandle.standardError.write("app-window failed: a window reached a display at \(window.frame)\n".data(using: .utf8)!)
            exit(2)
        }
    }
}

/// One built window and what drives it.
@MainActor
final class AppWindowScene {
    let scenario: AppWindowScenario
    private(set) var window: NSWindow!
    private var documentController: DocumentWindowController?
    private var retained: NSWindowController?

    init(scenario: AppWindowScenario) {
        self.scenario = scenario
    }

    func build(repositoryRoot: URL) throws {
        switch scenario.window {
        case "document":
            guard let path = scenario.document else { throw AppOracleError(description: "document scenario needs \"document\"") }
            let url = repositoryRoot.appendingPathComponent(path)
            let controller = DocumentWindowController()
            if let size = scenario.size { controller.window?.setContentSize(size) }
            try controller.open(url, mode: scenario.mode)
            documentController = controller
            window = controller.window
            show()
            controller.applyCommandLineOpen(line: nil, review: false)
        case "start":
            let guide: StartGuideOffer = switch scenario.guide {
            case "primary": .primary
            case "secondary": .secondary
            default: .unavailable
            }
            let recents = DocumentStateStore.shared.recents(limit: StartWindowController.recentDisplayLimit)
            let controller = StartWindowController(recents: recents, guide: guide)
            retained = controller
            window = controller.window
            show()
        case "setup":
            guard let controller = SetupWindowController.makeIfNeeded() else {
                throw AppOracleError(description: "SetupWindowController.makeIfNeeded() returned nil on this machine")
            }
            retained = controller
            window = controller.window
            show()
        case "preferences":
            let controller = PreferencesWindowController()
            if let name = scenario.pane {
                guard let pane = SettingsPane(rawValue: name) else { throw AppOracleError(description: "unknown pane \(name)") }
                controller.select(pane)
            }
            retained = controller
            window = controller.window
            show()
        case "probe":
            // Nothing of Downright's: a stock titled window, to prove the two
            // harnesses' launch, off-screen placement and capture agree
            // before any ported window is judged by them.
            let probe = NSWindow(
                contentRect: NSRect(x: 0, y: 0, width: 480, height: 320),
                styleMask: [.titled, .closable, .miniaturizable, .resizable],
                backing: .buffered, defer: false
            )
            probe.isReleasedWhenClosed = false
            probe.title = "Probe"
            let label = NSTextField(labelWithString: "The quick brown fox jumps over the lazy dog.")
            label.frame = NSRect(x: 24, y: 140, width: 432, height: 24)
            label.font = .systemFont(ofSize: 17)
            let button = NSButton(title: "Continue", target: nil, action: nil)
            button.bezelStyle = .push
            button.frame = NSRect(x: 360, y: 20, width: 100, height: 32)
            probe.contentView?.addSubview(label)
            probe.contentView?.addSubview(button)
            window = probe
            show()
        default:
            throw AppOracleError(description: "unknown window \(scenario.window)")
        }
    }

    /// `showWindow(nil)` / `makeKeyAndOrderFront(nil)`, minus the key status
    /// and on no screen.
    private func show() {
        window.setFrameOrigin(NSPoint(x: -30000, y: -30000))
        window.orderFrontRegardless()
        OffScreenWindows.verify([window])
    }

    /// Commands still to perform; each runs once the previous state settled.
    private var pendingCommands: [String]?

    /// Performs the next command, or returns false when none is left.
    func performNextCommand() throws -> Bool {
        if pendingCommands == nil { pendingCommands = scenario.commands }
        guard var commands = pendingCommands, !commands.isEmpty else { return false }
        let name = commands.removeFirst()
        pendingCommands = commands
        guard let controller = documentController else {
            throw AppOracleError(description: "commands need a document window")
        }
        guard let command = Command(rawValue: name) else { throw AppOracleError(description: "unknown command \(name)") }
        _ = controller.perform(command)
        return true
    }

    /// Every window the capture shows, the document window first.
    var capturedWindows: [NSWindow] {
        [window] + (window.childWindows ?? []).filter(\.isVisible)
    }
}

/// Geometry of every view in the captured windows, to localise a pixel
/// difference to the view that drew it.
@MainActor
enum AppWindowGeometry {
    static func className(_ object: AnyObject) -> String {
        var name = String(describing: type(of: object))
        if name.hasPrefix("NSKVONotifying_") { name.removeFirst("NSKVONotifying_".count) }
        return name
    }

    static func rect(_ rect: NSRect) -> JSON {
        .array([.double(rect.origin.x), .double(rect.origin.y), .double(rect.size.width), .double(rect.size.height)])
    }

    /// An identifier AppKit makes from an object's address
    /// (`NSTabViewControllerToolbarUIProvider(0x…)`) differs per run.
    static func withoutAddress(_ identifier: String) -> String {
        guard let range = identifier.range(of: "(0x") else { return identifier }
        return String(identifier[..<range.lowerBound]) + "(0x…)"
    }

    static func view(_ view: NSView) -> JSON {
        var pairs: [(String, JSON)] = [
            ("class", .string(className(view))),
            ("frame", rect(view.frame)),
            ("bounds", rect(view.bounds)),
            ("hidden", .bool(view.isHidden)),
            ("alpha", .double(Double(view.alphaValue))),
        ]
        if let field = view as? NSTextField {
            pairs.append(("text", .string(field.stringValue)))
        } else if let button = view as? NSButton {
            pairs.append(("title", .string(button.title)))
            pairs.append(("state", .int(button.state.rawValue)))
        }
        pairs.append(("subviews", .array(view.subviews.map { Self.view($0) })))
        return .object(pairs)
    }

    static func window(_ window: NSWindow) -> JSON {
        var pairs: [(String, JSON)] = [
            ("class", .string(className(window))),
            ("frame", rect(window.frame)),
            ("contentLayoutRect", rect(window.contentLayoutRect)),
            ("title", .string(window.title)),
            ("subtitle", .string(window.subtitle)),
            ("styleMask", .int(Int(window.styleMask.rawValue))),
            ("appearance", .string(window.effectiveAppearance.name.rawValue)),
        ]
        if let toolbar = window.toolbar {
            pairs.append(("toolbar", .object([
                ("identifier", .string(withoutAddress(toolbar.identifier))),
                ("items", .array(toolbar.items.map { item in
                    .object([
                        ("identifier", .string(item.itemIdentifier.rawValue)),
                        ("viewClass", item.view.map { .string(className($0)) } ?? .null),
                    ])
                })),
            ])))
        }
        let root = window.contentView?.superview ?? window.contentView
        pairs.append(("views", root.map { view($0) } ?? .null))
        return .object(pairs)
    }
}

/// Owns the run: sandbox, launch, build, settle, capture.
@MainActor
final class AppWindowSession: NSObject, NSApplicationDelegate {
    let scenario: AppWindowScenario
    let output: URL
    let layout: URL?
    let repositoryRoot: URL
    let sandbox: URL
    private var scene: AppWindowScene!
    private var previousCapture: Data?
    private var stableCaptures = 0
    private var deadline = Date.distantFuture

    init(scenario: AppWindowScenario, output: URL, layout: URL?, repositoryRoot: URL, sandbox: URL) {
        self.scenario = scenario
        self.output = output
        self.layout = layout
        self.repositoryRoot = repositoryRoot
        self.sandbox = sandbox
    }

    static func run(input: URL, output: String, flags: [String], repositoryRoot: URL) throws -> Never {
        let scenario = try AppWindowScenario(url: input)
        var layout: URL?
        if let index = flags.firstIndex(of: "--layout"), index + 1 < flags.count {
            layout = URL(fileURLWithPath: flags[index + 1])
        }
        acquireAppWindowCaptureLock()
        OffScreenWindows.install()
        let sandbox = try AppWindowSandbox.prepare(scenario)
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        let session = AppWindowSession(
            scenario: scenario, output: URL(fileURLWithPath: output), layout: layout,
            repositoryRoot: repositoryRoot, sandbox: sandbox
        )
        app.delegate = session
        app.run()
        exit(0)
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        do {
            applyScenarioAppearance(scenario, repositoryRoot: repositoryRoot)
            scene = AppWindowScene(scenario: scenario)
            try scene.build(repositoryRoot: repositoryRoot)
            restartSettling()
            scheduleCheck()
        } catch {
            fail("\(error)")
        }
    }

    private func restartSettling() {
        previousCapture = nil
        stableCaptures = 0
        deadline = Date().addingTimeInterval(scenario.settleTimeout)
    }

    private func scheduleCheck() {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { [weak self] in
            MainActor.assumeIsolated { self?.checkSettled() }
        }
    }

    /// Three byte-identical window-server captures in a row, 150 ms apart,
    /// mean the window has settled.
    private func checkSettled() {
        OffScreenWindows.verify(scene.capturedWindows)
        let png: Data
        do {
            png = try WindowServerCapture.png(of: scene.capturedWindows)
        } catch {
            fail("window capture failed: \(error)")
        }
        if png == previousCapture {
            stableCaptures += 1
        } else {
            stableCaptures = 0
            previousCapture = png
        }
        guard stableCaptures >= 2 || Date() > deadline else {
            scheduleCheck()
            return
        }
        if stableCaptures < 2 {
            FileHandle.standardError.write("warning: window did not settle before the timeout\n".data(using: .utf8)!)
        }
        do {
            if try scene.performNextCommand() {
                restartSettling()
                scheduleCheck()
                return
            }
            if let layout {
                let windows = scene.capturedWindows.map { AppWindowGeometry.window($0) }
                try JSON.array(windows).text.write(to: layout, atomically: true, encoding: .utf8)
            }
            try png.write(to: output)
        } catch {
            fail("\(error)")
        }
        AppWindowSandbox.remove(sandbox)
        exit(0)
    }

    private func fail(_ message: String) -> Never {
        FileHandle.standardError.write("app-window failed: \(message)\n".data(using: .utf8)!)
        AppWindowSandbox.remove(sandbox)
        exit(2)
    }
}

/// The captures one above the other, left-aligned, in the first capture's
/// colour space (as `stackImages` in the core oracle).
func stackAppWindowImages(_ images: [CGImage]) throws -> CGImage {
    let width = images.map(\.width).max() ?? 0
    let height = images.map(\.height).reduce(0, +)
    guard let space = images.first?.colorSpace,
          let context = CGContext(
            data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0, space: space,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
          ) else {
        throw AppOracleError(description: "cannot make the stacking context")
    }
    var top = height
    for image in images {
        top -= image.height
        context.draw(image, in: CGRect(x: 0, y: top, width: image.width, height: image.height))
    }
    guard let stacked = context.makeImage() else { throw AppOracleError(description: "stacking failed") }
    return stacked
}

/// The window server's own composite of a window — title bar, materials,
/// glass and layers exactly as they would reach a display — for a window
/// that is on no display. `CGWindowListCreateImage` (looked up at run time:
/// the macOS 15 SDK marks it obsoleted, the system still provides it) is the
/// one API that can: ScreenCaptureKit refuses such a window
/// (SCStreamErrorDomain -3811, titled or not), and `cacheDisplay` draws the
/// views but not the glass and backdrop layers the window server composites.
/// Child windows are stacked beneath the main window.
enum WindowServerCapture {
    typealias CreateImage = @convention(c) (CGRect, UInt32, UInt32, UInt32) -> Unmanaged<CGImage>?

    static let createImage: CreateImage? = {
        guard let handle = dlopen("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics", RTLD_NOW),
              let symbol = dlsym(handle, "CGWindowListCreateImage")
        else { return nil }
        return unsafeBitCast(symbol, to: CreateImage.self)
    }()

    static func image(of window: NSWindow) throws -> CGImage {
        guard let createImage else { throw AppOracleError(description: "CGWindowListCreateImage is unavailable") }
        // kCGWindowListOptionIncludingWindow; kCGWindowImageBoundsIgnoreFraming
        // | kCGWindowImageBestResolution (no shadow, backing resolution).
        guard let image = createImage(.null, 1 << 3, UInt32(window.windowNumber), (1 << 0) | (1 << 3))?.takeRetainedValue() else {
            throw AppOracleError(description: "the window server returned no image for window \(window.windowNumber)")
        }
        return image
    }

    static func png(of windows: [NSWindow]) throws -> Data {
        let images = try windows.map { try image(of: $0) }
        let image = images.count == 1 ? images[0] : try stackAppWindowImages(images)
        guard let png = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
            throw AppOracleError(description: "PNG encoding of the window capture failed")
        }
        return png
    }
}

/// The machine-wide window-capture lock (`acquireWindowCaptureLock` in the
/// core oracle; `upleft-oracle` takes the same one). Held until exit.
func acquireAppWindowCaptureLock() {
    let fd = open("/tmp/upleft-window-capture.lock", O_CREAT | O_RDWR, 0o644)
    guard fd >= 0, flock(fd, LOCK_EX) == 0 else {
        FileHandle.standardError.write("app-window failed: cannot take /tmp/upleft-window-capture.lock\n".data(using: .utf8)!)
        exit(2)
    }
}

// MARK: - Timings

/// `bench-app-window <scenario.json> <out.json>`: document open to the first
/// displayed frame, and a Live → Source → Live mode switch to its next frame,
/// over the scenario's document. Windows are off-screen, as in `app-window`.
@MainActor
enum AppWindowBench {
    static let warmup = 3
    static let runs = 15

    static func run(input: URL, output: String, repositoryRoot: URL) throws -> Never {
        let scenario = try AppWindowScenario(url: input)
        acquireAppWindowCaptureLock()
        OffScreenWindows.install()
        let sandbox = try AppWindowSandbox.prepare(scenario)
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        applyScenarioAppearance(scenario, repositoryRoot: repositoryRoot)
        guard let path = scenario.document else { throw AppOracleError(description: "bench needs a document") }
        let url = repositoryRoot.appendingPathComponent(path)

        var open: [Double] = []
        var toSource: [Double] = []
        var toLive: [Double] = []
        for iteration in 0..<(warmup + runs) {
            let start = DispatchTime.now().uptimeNanoseconds
            let controller = DocumentWindowController()
            if let size = scenario.size { controller.window?.setContentSize(size) }
            try controller.open(url, mode: .live)
            let window = controller.window!
            window.setFrameOrigin(NSPoint(x: -30000, y: -30000))
            window.orderFrontRegardless()
            OffScreenWindows.verify([window])
            // The first frame is the one the deferred restore paints: it makes
            // the document's text view first responder.
            while window.firstResponder !== controller.primaryContainer.textView {
                RunLoop.main.run(mode: .default, before: Date(timeIntervalSinceNow: 0.001))
            }
            window.displayIfNeeded()
            let opened = DispatchTime.now().uptimeNanoseconds

            controller.applyMode(.source)
            window.displayIfNeeded()
            let sourced = DispatchTime.now().uptimeNanoseconds
            controller.applyMode(.live)
            window.displayIfNeeded()
            let lived = DispatchTime.now().uptimeNanoseconds

            if iteration >= warmup {
                open.append(Double(opened - start) / 1e6)
                toSource.append(Double(sourced - opened) / 1e6)
                toLive.append(Double(lived - sourced) / 1e6)
            }
            controller.close()
            RunLoop.main.run(mode: .default, before: Date(timeIntervalSinceNow: 0.05))
        }
        AppWindowSandbox.remove(sandbox)
        func stage(_ name: String, _ samples: [Double]) -> JSON {
            let sorted = samples.sorted()
            return .object([
                ("stage", .string(name)),
                ("p50", .double(sorted[sorted.count / 2])),
                ("min", .double(sorted[0])),
                ("runs", .int(sorted.count)),
            ])
        }
        let json = JSON.array([
            stage("open to first frame", open),
            stage("mode switch Live to Source", toSource),
            stage("mode switch Source to Live", toLive),
        ])
        try json.text.write(toFile: output, atomically: true, encoding: .utf8)
        exit(0)
    }
}
