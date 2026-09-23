import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// `bench-panel <scenario.json> <out.json>`: how long a panel takes to build
/// and lay out (and, with `"draw": true`, draw into a bitmap), windowless, for
/// each entry of the scenario's `states` (merged over `state`, as
/// `panel-model` does). The scene's `prepare` (parsing the document, building
/// the model the panel is handed) is not timed. `runs` (default 20) samples
/// after `warmup` (default 3) per stage; prints `<panel> <stage>  p50 … ms  p95
/// … ms` lines as the other app benches do. Mirrored by
/// `crates/conformance/src/dump/panel/mod.rs` (`run_bench`).
enum PanelBench {
    @MainActor
    static func run(input: URL) throws -> JSON {
        let json = try readScenarioJSON(input)
        pinThemeSelection()
        OffScreenWindows.install()
        _ = NSApplication.shared
        let runs = (json["runs"] as? NSNumber)?.intValue ?? 20
        let warmup = (json["warmup"] as? NSNumber)?.intValue ?? 3
        let base = json["state"] as? [String: Any] ?? [:]
        let states = json["states"] as? [[String: Any]] ?? [[:]]
        var stages: [JSON] = []
        for entry in states {
            var merged = base
            for (key, value) in entry { merged[key] = value }
            let scenario = try PanelScenario(json: json, state: merged)
            let (styleSheet, appearance) = try panelStyleSheet(scenario)
            NSApp.appearance = appearance
            let draw = scenario.bool("draw")
            var samples: [Double] = []
            for index in 0..<(warmup + runs) {
                let scene = try PanelScenes.make(scenario.panel)
                try scene.prepare(scenario, styleSheet: styleSheet)
                let start = DispatchTime.now().uptimeNanoseconds
                let panel = try scene.build(scenario, styleSheet: styleSheet)
                panel.frame = NSRect(x: 0, y: 0, width: scenario.width, height: scenario.height)
                panel.layoutSubtreeIfNeeded()
                if draw, let rep = panel.bitmapImageRepForCachingDisplay(in: panel.bounds) {
                    panel.cacheDisplay(in: panel.bounds, to: rep)
                }
                let end = DispatchTime.now().uptimeNanoseconds
                if index >= warmup { samples.append(Double(end - start) / 1_000_000) }
                OffScreenWindows.verify(NSApp.windows.filter { $0.isVisible })
            }
            samples.sort()
            let p50 = samples[samples.count / 2]
            let p95 = samples[min(samples.count - 1, Int(Double(samples.count) * 0.95))]
            let name = "\(scenario.panel) \(entry["name"] as? String ?? "")"
            print("\(name)  p50 \(String(format: "%.3f", p50)) ms  p95 \(String(format: "%.3f", p95)) ms")
            stages.append(.object([
                ("name", .string(name)),
                ("p50", .double(p50)),
                ("p95", .double(p95)),
                ("samples", .array(samples.map { .double($0) })),
            ]))
        }
        return .object([("stages", .array(stages))])
    }
}
