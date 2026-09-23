import Foundation
@testable import MarkdownCore

/// `bench-core-text`: drbench's `measure` harness (one warm-up, N runs,
/// nearest-rank p50/p95) over the MarkdownCore stages that need no parse tree.
/// `upleft-oracle bench-core-text` runs the same stages over the same input.
/// Feed it `corpus/generated/agent/agent-5000.md`, which is drbench's
/// `agentDocument(lines: 5_000)` byte for byte.
enum CoreTextBench {
    static func percentile(_ ascending: [Double], _ p: Double) -> Double {
        let rank = Int((p * Double(ascending.count)).rounded(.up))
        return ascending[min(ascending.count - 1, max(0, rank - 1))]
    }

    static func measure(_ label: String, runs: Int, _ body: () -> Void) -> (String, JSON) {
        body()
        var samples: [Double] = []
        samples.reserveCapacity(runs)
        for _ in 0..<runs {
            let start = DispatchTime.now().uptimeNanoseconds
            body()
            samples.append(Double(DispatchTime.now().uptimeNanoseconds - start) / 1_000_000)
        }
        samples.sort()
        let p50 = percentile(samples, 0.50), p95 = percentile(samples, 0.95)
        print(String(format: "  %-44@  p50 %8.3f ms   p95 %8.3f ms   max %8.3f ms (n=%d)",
                     label as NSString, p50, p95, samples.last!, samples.count))
        return (label, .object([
            ("p50", .double(p50)), ("p95", .double(p95)), ("max", .double(samples.last!)), ("runs", .int(runs)),
        ]))
    }

    static func run(text: String) -> JSON {
        var edited = text
        edited.insert("x", at: edited.index(edited.startIndex, offsetBy: edited.count / 2))
        let ns = text as NSString
        let whole = NSRange(location: 0, length: ns.length)
        let data = Data(text.utf8)
        let map = SourceMap(text)
        let lines = (0..<map.lineCount).map { map.contentRange(ofLine: $0) }
        let quoteLines = lines.filter { range in
            CoreTextDump.firstNonBlank(ns, range).map { ns.character(at: $0) == 0x3E } == true
        }
        let paragraphs = CoreTextDump.paragraphRanges(ns, lines: lines)
        var sink = 0

        var results: [(String, JSON)] = []
        results.append(measure("TextDiff.hunks, external rewrite", runs: 10) {
            sink &+= TextDiff.hunks(old: text, new: edited).count
        })
        results.append(measure("TextDiff.hunks, identical", runs: 25) {
            sink &+= TextDiff.hunks(old: text, new: text).count
        })
        results.append(measure("Metrics.metrics(of:) whole text", runs: 10) {
            sink &+= Metrics.metrics(of: text).words
        })
        results.append(measure("Metrics.wordCount", runs: 25) {
            sink &+= Metrics.wordCount(text)
        })
        results.append(measure("SourceMap", runs: 25) {
            sink &+= SourceMap(text).lineCount
        })
        results.append(measure("SourceScanner", runs: 25) {
            sink &+= SourceScanner(map: map).footnoteDefinitions.count
        })
        results.append(measure("FrontMatterScanner.scan", runs: 25) {
            sink &+= FrontMatterScanner.scan(map)?.fields.count ?? 0
        })
        results.append(measure("MathScanner.matches, whole text", runs: 25) {
            sink &+= MathScanner.matches(in: ns, range: whole).count
        })
        results.append(measure("WikilinkScanner.matches, whole text", runs: 25) {
            sink &+= WikilinkScanner.matches(in: ns, range: whole).count
        })
        results.append(measure("PathTokenScanner.matches, whole text", runs: 25) {
            sink &+= PathTokenScanner.matches(in: ns, range: whole).count
        })
        results.append(measure("PathTokenScanner.matches, per line", runs: 25) {
            for range in lines { sink &+= PathTokenScanner.matches(in: ns, range: range).count }
        })
        results.append(measure("CalloutScanner.scan, quote lines", runs: 25) {
            for range in quoteLines { sink &+= CalloutScanner.scan(map, quoteRange: range) == nil ? 0 : 1 }
        })
        results.append(measure("SafeHTMLParser.parse, paragraphs", runs: 25) {
            for range in paragraphs { sink &+= SafeHTMLParser.parse(ns, range: range) == nil ? 0 : 1 }
        })
        results.append(measure("FNV.hash, UTF-16 whole text", runs: 25) {
            sink &+= Int(truncatingIfNeeded: FNV.hash(ns, range: whole))
        })
        results.append(measure("DocumentIO.decodeSnapshot", runs: 25) {
            sink &+= (try? DocumentIO.decodeSnapshot(data, sourceURL: URL(fileURLWithPath: "/dev/null")))?.text.utf8.count ?? 0
        })
        if sink == 42 { print("") }
        return .object(results)
    }
}
