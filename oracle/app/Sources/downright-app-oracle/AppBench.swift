import AppKit
@testable import DownrightApp
import MarkdownCore
import MarkdownRender

/// Timings for the export, workspace and find ports, with drbench's
/// `measure` harness (one warm-up, N runs, nearest-rank p50/p95), as
/// `CoreTextBench` in the core oracle does. `upleft-oracle` runs the same
/// stages over the same inputs (crates/conformance/src/dump/app_bench.rs);
/// `scripts/app-bench-compare.py` runs both and compares the p50s.
///
///   downright-app-oracle bench-export    <file.md> <out.json>   HTMLExporter.html()
///   downright-app-oracle bench-workspace <folder>  <out.json>   WorkspaceIndex scan, graph, search
///   downright-app-oracle bench-find      <file.md> <out.json>   FindEngine, FindSession
enum AppBench {
    static func percentile(_ ascending: [Double], _ p: Double) -> Double {
        let rank = Int((p * Double(ascending.count)).rounded(.up))
        return ascending[min(ascending.count - 1, max(0, rank - 1))]
    }

    /// Each run drains its own autorelease pool inside the timed region, so
    /// the run pays for freeing its temporaries (bitmaps, TIFF and PNG data)
    /// as an app does when the event that exported ends; without it the
    /// oracle's never-drained top-level pool would hide that cost.
    static func measure(_ label: String, runs: Int, _ body: () -> Void) -> (String, JSON) {
        autoreleasepool { body() }
        var samples: [Double] = []
        samples.reserveCapacity(runs)
        for _ in 0..<runs {
            let start = DispatchTime.now().uptimeNanoseconds
            autoreleasepool { body() }
            samples.append(Double(DispatchTime.now().uptimeNanoseconds - start) / 1_000_000)
        }
        samples.sort()
        let p50 = percentile(samples, 0.50), p95 = percentile(samples, 0.95)
        print(String(format: "  %-52@  p50 %8.3f ms   p95 %8.3f ms   max %8.3f ms (n=%d)",
                     label as NSString, p50, p95, samples.last!, samples.count))
        return (label, .object([
            ("p50", .double(p50)), ("p95", .double(p95)), ("max", .double(samples.last!)), ("runs", .int(runs)),
        ]))
    }

    static func styleSheet() -> StyleSheet {
        let theme = ThemeStore.shared.themes.first { $0.name == "Paper Light" } ?? .fallback
        return StyleSheet(theme: theme, appearance: NSAppearance(named: .aqua)!, reduceMotionOverride: true)
    }

    /// The window controller's exporter over `input`, parsed once.
    static func export(input: URL) throws -> JSON {
        let url = input.standardizedFileURL
        let text = try DocumentIO.read(contentsOf: url).text
        let name = url.lastPathComponent
        let sheet = styleSheet()
        let exporter = { (document: ParsedDocument) in
            HTMLExporter(
                document: document, theme: sheet.theme, title: url.deletingPathExtension().lastPathComponent,
                baseDirectory: url.deletingLastPathComponent(),
                imageProvider: NativeFragmentImageProvider(styleSheet: sheet)
            )
        }
        let document = MarkdownParser.parse(text)
        var sink = 0
        var results: [(String, JSON)] = []
        results.append(measure("HTMLExporter.html, \(name)", runs: 25) {
            sink &+= exporter(document).html().utf8.count
        })
        results.append(measure("parse + HTMLExporter.html, \(name)", runs: 10) {
            sink &+= exporter(MarkdownParser.parse(text)).html().utf8.count
        })
        results.append(measure("HTMLExporter.html forPrint, \(name)", runs: 25) {
            var paper = exporter(document)
            paper.forPrint = true
            sink &+= paper.html().utf8.count
        })
        if sink == 42 { print("") }
        return .object(results)
    }

    /// `WorkspaceIndex` over `folder` as the app runs it (start, the scan on
    /// the worker pool, the main-actor publish), then the graph and search
    /// over the snapshot.
    @MainActor
    static func workspace(folder: URL) throws -> JSON {
        let root = folder.standardizedFileURL
        guard FileManager.default.changeCurrentDirectoryPath("/") else {
            throw AppOracleError(description: "cannot move to /")
        }
        let name = root.lastPathComponent
        var snapshot = WorkspaceIndexSnapshot.empty
        func scan() {
            let index = WorkspaceIndex(policy: WorkspaceIndexPolicy())
            var published: WorkspaceIndexSnapshot?
            index.onUpdate = { published = $0 }
            index.start(rootURL: root)
            while published == nil {
                _ = RunLoop.main.run(mode: .default, before: Date().addingTimeInterval(0.001))
            }
            snapshot = published!
        }
        var sink = 0
        var results: [(String, JSON)] = []
        results.append(measure("WorkspaceIndex scan, \(name)", runs: 10) { scan(); sink &+= snapshot.entries.count })
        results.append(measure("WorkspaceLinkGraphBuilder.build, \(name)", runs: 25) {
            sink &+= WorkspaceLinkGraphBuilder.build(snapshot: snapshot).outgoing.count
        })
        results.append(measure("WorkspaceSearch.search \"release\", \(name)", runs: 10) {
            sink &+= WorkspaceSearch.search(WorkspaceSearchQuery(text: "release"), in: snapshot).count
        })
        results.append(measure("WorkspaceSearch.search regex whole word, \(name)", runs: 10) {
            sink &+= WorkspaceSearch.search(
                WorkspaceSearchQuery(text: "(?:link|task)s?", isRegex: true, wholeWord: true), in: snapshot
            ).count
        })
        if sink == 42 { print("") }
        return .object(results)
    }

    static func find(input: URL) throws -> JSON {
        let text = try DocumentIO.read(contentsOf: input.standardizedFileURL).text
        let name = input.lastPathComponent
        var sink = 0
        var results: [(String, JSON)] = []
        let literal = FindQuery(text: "section")
        results.append(measure("FindEngine.matches \"section\", \(name)", runs: 25) {
            sink &+= FindEngine.matches(in: text, query: literal).count
        })
        let wholeWord = FindQuery(text: "Section", caseSensitive: true, wholeWord: true)
        results.append(measure("FindEngine.matches case, whole word, \(name)", runs: 25) {
            sink &+= FindEngine.matches(in: text, query: wholeWord).count
        })
        let regex = FindQuery(text: "(?m)^## (.*)$", isRegex: true)
        results.append(measure("FindEngine.matches regex, \(name)", runs: 25) {
            sink &+= FindEngine.matches(in: text, query: regex).count
        })
        let capture = FindQuery(text: #"Section (\d+)"#, isRegex: true)
        results.append(measure("FindEngine.replaceAllEdits regex, \(name)", runs: 10) {
            sink &+= FindEngine.replaceAllEdits(in: text, query: capture, template: "Part $1").count
        })
        let task = FindQuery(text: "task")
        results.append(measure("FindSession.update + advance, \(name)", runs: 25) {
            let session = FindSession()
            session.update(query: task, in: text, caret: text.utf16.count / 2)
            sink &+= session.advance(forward: true)?.location ?? 0
        })
        if sink == 42 { print("") }
        return .object(results)
    }
}
