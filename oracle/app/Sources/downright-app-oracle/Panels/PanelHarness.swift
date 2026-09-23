import AppKit
import ScreenCaptureKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

// The panel suites (`panel`, `panel-model`): Downright's own panel classes,
// built in a deterministic state from a small JSON scenario
// (`corpus/panels/*.json`, `corpus/panel-models/*.json`), hosted off-screen
// and never activated. `upleft-oracle` mirrors every call here
// (crates/conformance/src/dump/panel/mod.rs), so the harness is never a
// source of differences.
//
//   panel        <scenario.json> <out.png> [--layout <out.json>]
//   panel-model  <scenario.json> <out.json>
//
// A scenario:
//
//   {"panel": "DocumentHealthView", "theme": "Paper Light", "dark": false,
//    "width": 336, "height": 520, "document": "corpus/generated/docs/README.md",
//    "state": {...}}
//
// `panel-model` scenarios add `"states": [{"name": ..., ...}, ...]`; each
// entry is merged over `state` and built windowless.

enum PanelHarnessError: Error, CustomStringConvertible {
    case usage(String)
    case unknownPanel(String)
    case unknownTheme(String)
    case notPorted(String)

    var description: String {
        switch self {
        case .usage(let message): return message
        case .unknownPanel(let name): return "unknown panel \(name)"
        case .unknownTheme(let name): return "unknown theme \(name)"
        case .notPorted(let name): return "no scene for \(name) yet"
        }
    }
}

/// One scenario: the envelope every panel shares, plus the panel's own
/// `state` object.
struct PanelScenario {
    let panel: String
    let theme: String
    let dark: Bool
    let width: CGFloat
    let height: CGFloat
    /// Path from the repository root, if the panel is attached to a document.
    let documentPath: String?
    let state: [String: Any]

    init(json: [String: Any], state: [String: Any]? = nil) throws {
        guard let panel = json["panel"] as? String else { throw PanelHarnessError.usage("scenario has no panel") }
        self.panel = panel
        theme = json["theme"] as? String ?? "Paper Light"
        dark = json["dark"] as? Bool ?? false
        width = CGFloat((json["width"] as? NSNumber)?.doubleValue ?? 336)
        height = CGFloat((json["height"] as? NSNumber)?.doubleValue ?? 480)
        documentPath = json["document"] as? String
        self.state = state ?? (json["state"] as? [String: Any] ?? [:])
    }

    /// The attached document's text (read as the app reads a file: UTF-8).
    func documentText() throws -> String {
        guard let documentPath else { return "" }
        let url = repositoryRoot.appendingPathComponent(documentPath)
        return try String(contentsOf: url, encoding: .utf8)
    }

    func string(_ key: String) -> String? { state[key] as? String }
    func string(_ key: String, _ fallback: String) -> String { state[key] as? String ?? fallback }
    func int(_ key: String) -> Int? { (state[key] as? NSNumber)?.intValue }
    func int(_ key: String, _ fallback: Int) -> Int { (state[key] as? NSNumber)?.intValue ?? fallback }
    func double(_ key: String) -> Double? { (state[key] as? NSNumber)?.doubleValue }
    func double(_ key: String, _ fallback: Double) -> Double { (state[key] as? NSNumber)?.doubleValue ?? fallback }
    func bool(_ key: String) -> Bool { (state[key] as? NSNumber)?.boolValue ?? false }
    func array(_ key: String) -> [Any] { state[key] as? [Any] ?? [] }
    func object(_ key: String) -> [String: Any] { state[key] as? [String: Any] ?? [:] }
    func strings(_ key: String) -> [String] { array(key).compactMap { $0 as? String } }
}

/// What a panel scene provides. The harness owns the window, the settle loop
/// and the capture; the scene builds and configures the panel.
@MainActor
protocol PanelScene: AnyObject {
    /// Builds the panel and applies the scenario's state.
    func build(_ scenario: PanelScenario, styleSheet: StyleSheet) throws -> NSView
    /// Puts the panel in the harness window (default: the window's content
    /// view, at the scenario's size).
    func host(_ panel: NSView, in window: NSWindow, scenario: PanelScenario)
    /// A window the scene owns (a titled window is captured with
    /// ScreenCaptureKit; a borderless one with `cacheDisplay`). Nil: the
    /// harness window.
    func ownWindow(_ panel: NSView) -> NSWindow?
    /// Runs once, after the window is ordered in (off-screen).
    func afterShow(window: NSWindow, scenario: PanelScenario)
    /// Runs before every settle check.
    func beforeSettleCheck()
    /// Panel-specific derived values (text, geometry, counts) for the dump.
    func model() -> JSON
}

extension PanelScene {
    func host(_ panel: NSView, in window: NSWindow, scenario: PanelScenario) {
        panel.frame = NSRect(x: 0, y: 0, width: scenario.width, height: scenario.height)
        window.contentView = panel
    }
    func ownWindow(_ panel: NSView) -> NSWindow? { nil }
    func afterShow(window: NSWindow, scenario: PanelScenario) {}
    func beforeSettleCheck() {}
    func model() -> JSON { .null }
}

/// The style sheet every scene draws with: the scenario's theme against its
/// appearance, Reduce Motion forced on as in the render harness.
@MainActor
func panelStyleSheet(_ scenario: PanelScenario) throws -> (StyleSheet, NSAppearance) {
    let appearance = NSAppearance(named: scenario.dark ? .darkAqua : .aqua)!
    guard let theme = ThemeStore.shared.themes.first(where: { $0.name == scenario.theme }) else {
        throw PanelHarnessError.unknownTheme(scenario.theme)
    }
    return (StyleSheet(theme: theme, appearance: appearance, reduceMotionOverride: true), appearance)
}

func readScenarioJSON(_ url: URL) throws -> [String: Any] {
    let data = try Data(contentsOf: url)
    guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
        throw PanelHarnessError.usage("scenario is not a JSON object")
    }
    return json
}

// MARK: - panel (windowed, off-screen)

@MainActor
final class PanelCaptureSession: NSObject, NSApplicationDelegate {
    let scenario: PanelScenario
    let outputPNG: URL
    let outputLayout: URL?
    private var scene: PanelScene!
    private var window: NSWindow!
    private var settleView: NSView!
    private var previousCapture: Data?
    private var stableCaptures = 0
    private var deadline = Date.distantFuture
    private var usesScreenCaptureKit = false

    init(scenario: PanelScenario, outputPNG: URL, outputLayout: URL?) {
        self.scenario = scenario
        self.outputPNG = outputPNG
        self.outputLayout = outputLayout
    }

    static func run(input: URL, output: String, flags: [String]) throws -> Never {
        let scenario = try PanelScenario(json: readScenarioJSON(input))
        var layout: URL?
        var index = 0
        while index < flags.count {
            if flags[index] == "--layout", index + 1 < flags.count {
                layout = URL(fileURLWithPath: flags[index + 1])
                index += 2
            } else {
                throw PanelHarnessError.usage("unknown flag \(flags[index])")
            }
        }
        acquirePanelCaptureLock()
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        let session = MainActor.assumeIsolated {
            PanelCaptureSession(scenario: scenario, outputPNG: URL(fileURLWithPath: output), outputLayout: layout)
        }
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
        let (styleSheet, appearance) = try panelStyleSheet(scenario)
        NSApp.appearance = appearance
        scene = try PanelScenes.make(scenario.panel)
        let panel = try scene.build(scenario, styleSheet: styleSheet)
        if let own = scene.ownWindow(panel) {
            window = own
            usesScreenCaptureKit = own.styleMask.contains(.titled)
        } else {
            let frame = NSRect(x: 0, y: 0, width: scenario.width, height: scenario.height)
            window = NSWindow(contentRect: frame, styleMask: [.borderless], backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            window.appearance = appearance
            window.colorSpace = .sRGB
            scene.host(panel, in: window, scenario: scenario)
        }
        window.setFrameOrigin(NSPoint(x: -30000, y: -30000))
        window.orderFrontRegardless()
        window.layoutIfNeeded()
        scene.afterShow(window: window, scenario: scenario)
        settleView = window.contentView
        deadline = Date().addingTimeInterval(8)
        scheduleCheck()
    }

    private func scheduleCheck() {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { [weak self] in
            MainActor.assumeIsolated { self?.checkSettled() }
        }
    }

    private func checkSettled() {
        scene.beforeSettleCheck()
        window.layoutIfNeeded()
        settleView.displayIfNeeded()
        guard let rep = settleView.bitmapImageRepForCachingDisplay(in: settleView.bounds) else {
            fail("no bitmap representation")
        }
        settleView.cacheDisplay(in: settleView.bounds, to: rep)
        guard let png = rep.representation(using: .png, properties: [:]) else { fail("PNG encoding failed") }
        if png == previousCapture {
            stableCaptures += 1
        } else {
            stableCaptures = 0
            previousCapture = png
        }
        if stableCaptures < 2 && Date() < deadline {
            scheduleCheck()
            return
        }
        if stableCaptures < 2 {
            FileHandle.standardError.write("warning: panel did not settle before the timeout\n".data(using: .utf8)!)
        }
        do {
            if let outputLayout {
                let layout = JSON.object([
                    ("window", PanelTree.window(window)),
                    ("tree", PanelTree.dump(settleView)),
                    ("model", scene.model()),
                ])
                try layout.text.write(to: outputLayout, atomically: true, encoding: .utf8)
            }
            if !usesScreenCaptureKit {
                try png.write(to: outputPNG)
                exit(0)
            }
        } catch {
            fail("\(error)")
        }
        let windowNumber = CGWindowID(window.windowNumber)
        let output = outputPNG
        Task {
            do {
                let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: false)
                guard let scWindow = content.windows.first(where: { $0.windowID == windowNumber }) else {
                    throw PanelHarnessError.usage("ScreenCaptureKit does not list the panel window")
                }
                let filter = SCContentFilter(desktopIndependentWindow: scWindow)
                let configuration = SCStreamConfiguration()
                let scale = CGFloat(filter.pointPixelScale)
                configuration.width = Int(filter.contentRect.width * scale)
                configuration.height = Int(filter.contentRect.height * scale)
                configuration.showsCursor = false
                configuration.ignoreShadowsSingleWindow = true
                configuration.captureResolution = .best
                let image = try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: configuration)
                let rep = NSBitmapImageRep(cgImage: image)
                guard let data = rep.representation(using: .png, properties: [:]) else {
                    throw PanelHarnessError.usage("PNG encoding of the window capture failed")
                }
                try data.write(to: output)
                exit(0)
            } catch {
                FileHandle.standardError.write("panel failed: window capture failed: \(error)\n".data(using: .utf8)!)
                exit(2)
            }
        }
    }

    private func fail(_ message: String) -> Never {
        FileHandle.standardError.write("panel failed: \(message)\n".data(using: .utf8)!)
        exit(2)
    }
}

/// One window capture at a time on this machine (see `acquireWindowCaptureLock`
/// in the core oracle, which takes the same lock). Held until exit.
func acquirePanelCaptureLock() {
    let fd = open("/tmp/upleft-window-capture.lock", O_CREAT | O_RDWR, 0o644)
    guard fd >= 0, flock(fd, LOCK_EX) == 0 else {
        FileHandle.standardError.write("panel failed: cannot take /tmp/upleft-window-capture.lock\n".data(using: .utf8)!)
        exit(2)
    }
}

// MARK: - panel-model (windowless)

enum PanelModelDump {
    @MainActor
    static func run(input: URL, flags: [String]) throws -> JSON {
        let json = try readScenarioJSON(input)
        _ = NSApplication.shared
        let base = json["state"] as? [String: Any] ?? [:]
        let states = json["states"] as? [[String: Any]] ?? [[:]]
        var results: [JSON] = []
        for entry in states {
            var merged = base
            for (key, value) in entry { merged[key] = value }
            let scenario = try PanelScenario(json: json, state: merged)
            let (styleSheet, appearance) = try panelStyleSheet(scenario)
            NSApp.appearance = appearance
            let scene = try PanelScenes.make(scenario.panel)
            let panel = try scene.build(scenario, styleSheet: styleSheet)
            panel.frame = NSRect(x: 0, y: 0, width: scenario.width, height: scenario.height)
            panel.layoutSubtreeIfNeeded()
            results.append(.object([
                ("name", .string(entry["name"] as? String ?? "")),
                ("fittingSize", PanelTree.size(panel.fittingSize)),
                ("tree", PanelTree.dump(panel)),
                ("model", scene.model()),
            ]))
        }
        return .object([("panel", .string(json["panel"] as? String ?? "")), ("states", .array(results))])
    }
}
