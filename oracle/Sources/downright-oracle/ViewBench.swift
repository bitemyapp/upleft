import AppKit
import MarkdownCore
import MarkdownRender

/// `bench-view <file.md> <out.json> [--mode M] [--theme NAME] [--dark] [--width W] [--height H]`
///
/// Times the render scene's view work for one document: `MarkdownScene`'s
/// `update(document:dirty: .wholesale)`, the app's first-frame sequence
/// (`afterShow`), and one settle pass (`beforeSettleCheck` plus a display).
/// The document is parsed once, outside the timings. Every run builds a fresh
/// window, storage and container, exactly as the render command does; the
/// first run is a warm-up. `VIEW_BENCH_RUNS` sets the run count (default 10).
/// `upleft-oracle bench-view` runs the same sequence.
final class ViewBench: NSObject, NSApplicationDelegate {
    let request: RenderRequest
    let output: String

    init(request: RenderRequest, output: String) {
        self.request = request
        self.output = output
    }

    static func run(request: RenderRequest, output: String) -> Never {
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        let bench = ViewBench(request: request, output: output)
        app.delegate = bench
        app.run()
        exit(0)
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        do {
            try bench()
            exit(0)
        } catch {
            FileHandle.standardError.write("bench-view failed: \(error)\n".data(using: .utf8)!)
            exit(2)
        }
    }

    private static func now() -> UInt64 { DispatchTime.now().uptimeNanoseconds }

    private func bench() throws {
        let text = try String(contentsOf: request.input, encoding: .utf8)
        let document = MarkdownParser.parse(text)
        let appearance = NSAppearance(named: request.dark ? .darkAqua : .aqua)!
        guard let theme = ThemeStore.shared.themes.first(where: { $0.name == request.themeName }) else {
            throw OracleError.unknownTheme(request.themeName, ThemeStore.shared.themes.map(\.name))
        }
        let styleSheet = StyleSheet(theme: theme, appearance: appearance, reduceMotionOverride: true)
        let runs = Int(ProcessInfo.processInfo.environment["VIEW_BENCH_RUNS"] ?? "") ?? 10
        // Headless, as every capture is by default: never activated, the
        // window placed outside every screen.

        var update: [Double] = [], firstFrame: [Double] = [], settle: [Double] = [], total: [Double] = []
        var fragments = 0
        for index in 0...runs {
            let frame = NSRect(x: 0, y: 0, width: request.width, height: request.height)
            let window = NSWindow(contentRect: frame, styleMask: [.borderless], backing: .buffered, defer: false)
            window.setFrameOrigin(NSPoint(x: -30000, y: -30000))
            window.isReleasedWhenClosed = false
            window.appearance = appearance
            window.colorSpace = .sRGB
            let storage = NSTextStorage(string: text)
            let container = MarkdownContainerView(storage: storage, styleSheet: styleSheet)
            container.frame = frame
            window.contentView = container
            window.layoutIfNeeded()
            container.layoutSubtreeIfNeeded()
            container.textView.mode = request.mode

            let t0 = Self.now()
            container.textView.update(document: document, dirty: .wholesale)
            let t1 = Self.now()
            window.orderFrontRegardless()
            let t2 = Self.now()
            window.layoutIfNeeded()
            container.layoutSubtreeIfNeeded()
            container.textView.resizeToFitContent()
            container.textView.scroll(toOffset: 0, position: .top, animated: false)
            container.textView.prepareForDisplay()
            container.textView.displayIfNeeded()
            let t3 = Self.now()
            container.layoutSubtreeIfNeeded()
            if let layout = container.textView.textLayoutManager {
                layout.ensureLayout(for: layout.documentRange)
            }
            container.displayIfNeeded()
            let t4 = Self.now()

            if index > 0 {
                update.append(Double(t1 - t0) / 1e6)
                firstFrame.append(Double(t3 - t2) / 1e6)
                settle.append(Double(t4 - t3) / 1e6)
                total.append(Double((t1 - t0) + (t4 - t2)) / 1e6)
            }
            if let layout = container.textView.textLayoutManager {
                fragments = 0
                layout.enumerateTextLayoutFragments(from: layout.documentRange.location, options: []) { _ in
                    fragments += 1
                    return true
                }
            }
            window.orderOut(nil)
            window.contentView = nil
            window.close()
        }

        func stats(_ label: String, _ values: [Double]) -> (String, JSON) {
            let sorted = values.sorted()
            let p50 = MathBench.percentile(sorted, 0.50), p95 = MathBench.percentile(sorted, 0.95)
            print(String(format: "  %-44@  p50 %8.3f ms   p95 %8.3f ms   max %8.3f ms (n=%d)",
                         label as NSString, p50, p95, sorted.last!, sorted.count))
            return (label, .object([
                ("p50", .double(p50)), ("p95", .double(p95)), ("max", .double(sorted.last!)), ("runs", .int(values.count)),
            ]))
        }
        print("bench-view: \(request.input.lastPathComponent), \(runs) runs")
        try write(.object([
            stats("update(document:) wholesale", update),
            stats("first frame (afterShow)", firstFrame),
            stats("settle pass", settle),
            stats("update to settled", total),
            ("fragments", .int(fragments)),
        ]), to: output)
    }
}
