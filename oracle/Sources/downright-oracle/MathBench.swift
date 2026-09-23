import AppKit
@testable import MarkdownRender
@testable import SwiftMath

/// `bench-math <dir> <out.json>`: SwiftMath's stages over every `.tex` under
/// `dir`, with drbench's `measure` harness (one warm-up, N runs, nearest-rank
/// p50/p95). `upleft-oracle bench-math` runs the same stages over the same
/// inputs. Each run covers the whole corpus:
///
/// - parse:   `MTMathListBuilder.build(fromString:)`
/// - typeset: parse + `MTTypesetter.createLineForMathList`
/// - render:  `MTMathImage.asImage()` + rasterising the image for a 2x
///            context — MathRenderer's work for a cache miss, then the draw
enum MathBench {
    struct Formula {
        let source: String
        let display: Bool
        let fontSize: CGFloat
    }

    static func formulas(in directory: URL) throws -> [Formula] {
        let (base, _) = try MathDump.parameters()
        var files: [URL] = []
        let enumerator = FileManager.default.enumerator(at: directory, includingPropertiesForKeys: nil)
        while let url = enumerator?.nextObject() as? URL {
            if url.pathExtension == "tex" { files.append(url) }
        }
        files.sort { $0.path < $1.path }
        return try files.compactMap { url in
            let input = try MathDump.read(url)
            let trimmed = input.latex.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty else { return nil }
            let pointSize = input.display ? base * 1.12 : base
            return Formula(
                source: MathDump.swiftMathSource(from: trimmed),
                display: input.display,
                fontSize: (pointSize * 4).rounded() / 4)
        }
    }

    static func run(_ directory: URL, to output: String) throws {
        let formulas = try formulas(in: directory)
        let color = try MathDump.parameters().color
        let runs = Int(ProcessInfo.processInfo.environment["MATH_BENCH_RUNS"] ?? "") ?? 10
        var fonts: [CGFloat: MTFont] = [:]
        for formula in formulas where fonts[formula.fontSize] == nil {
            fonts[formula.fontSize] = MTFontManager.fontManager.defaultFont!.copy(withSize: formula.fontSize)
        }
        var sink = 0
        print("bench-math: \(formulas.count) formulas, \(runs) runs")
        var results: [(String, JSON)] = []
        results.append(measure("parse", runs: runs) {
            for formula in formulas {
                sink &+= MTMathListBuilder.build(fromString: formula.source)?.atoms.count ?? 0
            }
        })
        results.append(measure("parse + typeset", runs: runs) {
            for formula in formulas {
                guard let list = MTMathListBuilder.build(fromString: formula.source) else { continue }
                let display = MTTypesetter.createLineForMathList(
                    list, font: fonts[formula.fontSize]!, style: formula.display ? .display : .text)
                sink &+= display?.subDisplays.count ?? 0
            }
        })
        results.append(measure("parse + typeset + render", runs: runs) {
            for formula in formulas {
                let renderer = MTMathImage(
                    latex: formula.source, fontSize: formula.fontSize, textColor: color,
                    labelMode: formula.display ? .display : .text,
                    textAlignment: formula.display ? .center : .left)
                let (_, image) = renderer.asImage()
                if let image, let bitmap = rasterize(image) { sink &+= bitmap.width }
            }
        })
        results.append(("formulas", .int(formulas.count)))
        results.append(("sink", .int(sink & 1)))
        try write(.object(results), to: output)
    }
}

extension MathBench {
    /// `MathDump.rasterize` without the PNG encoding: the bitmap `drawNSImage` draws.
    static func rasterize(_ image: NSImage) -> CGImage? {
        let scale: CGFloat = 2
        let width = max(1, Int((image.size.width * scale).rounded(.up)))
        let height = max(1, Int((image.size.height * scale).rounded(.up)))
        guard let context = CGContext(
            data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
        else { return nil }
        context.scaleBy(x: scale, y: scale)
        var proposed = CGRect(origin: .zero, size: image.size)
        let drawingContext = NSGraphicsContext(cgContext: context, flipped: true)
        return image.cgImage(forProposedRect: &proposed, context: drawingContext, hints: nil)
    }

    /// drbench's `measure`: one warm-up, then `runs` timed runs.
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
}
