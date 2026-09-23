import AppKit
import BeautifulMermaid
@testable import MarkdownRender

/// Mermaid as Downright draws it: beautiful-mermaid-swift driven through
/// `MermaidRendererBridge`.
///
///   downright-oracle mermaid-parse  <file.mmd> <out.json>
///   downright-oracle mermaid-layout <file.mmd> <out.json> [--theme NAME] [--dark]
///   downright-oracle mermaid        <file.mmd> <out.png>  [--theme NAME] [--dark]
///   downright-oracle mermaid-bench  <dir> <out.json>
///
/// The `.mmd` file is the fence body, verbatim; the bridge trims it.
///
/// - `mermaid-parse` dumps `MermaidParser.parse` of the trimmed source (or
///   the error it throws).
/// - `mermaid-layout` adds the `DiagramTheme` the bridge derives from the
///   style sheet and the `PositionedGraph` `GraphLayout(config: LayoutConfig())`
///   produces — every coordinate.
/// - `mermaid` writes the image `MermaidRendererBridge.image` returns (the
///   ink-cropped bitmap at the bridge's scale), or a 1×1 opaque magenta pixel
///   when it returns nil.
///
/// `upleft-oracle` writes the same shapes from
/// `crates/conformance/src/dump/mermaid.rs`.
enum MermaidDump {
    static func styleSheet(themeName: String, dark: Bool) throws -> StyleSheet {
        guard let theme = ThemeStore.shared.themes.first(where: { $0.name == themeName }) else {
            throw OracleError.unknownTheme(themeName, ThemeStore.shared.themes.map(\.name))
        }
        let appearance = NSAppearance(named: dark ? .darkAqua : .aqua)!
        return StyleSheet(theme: theme, appearance: appearance, reduceMotionOverride: true)
    }

    /// What the bridge hands the library: the source trimmed of whitespace and
    /// newlines.
    static func source(_ url: URL) throws -> String {
        try String(contentsOf: url, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    // MARK: - Commands

    static func parse(_ url: URL, to output: String) throws {
        let source = try source(url)
        var pairs: [(String, JSON)] = [("empty", .bool(source.isEmpty))]
        do {
            pairs.append(("parsed", MermaidModelDump.graph(try MermaidParser.parse(source))))
        } catch {
            pairs.append(("error", errorJSON(error)))
        }
        try write(.object(pairs), to: output)
    }

    static func layout(_ url: URL, to output: String, themeName: String, dark: Bool) throws {
        let source = try source(url)
        let sheet = try styleSheet(themeName: themeName, dark: dark)
        var pairs: [(String, JSON)] = [
            ("empty", .bool(source.isEmpty)),
            ("theme", theme(MermaidRendererBridge.theme(from: sheet))),
        ]
        do {
            let parsed = try MermaidParser.parse(source)
            pairs.append(("parsed", MermaidModelDump.graph(parsed)))
            do {
                let positioned = try GraphLayout(config: LayoutConfig()).layout(parsed)
                pairs.append(("positioned", MermaidModelDump.positionedGraph(positioned)))
                let bounds = CGRect(x: 0, y: 0, width: max(1, positioned.width), height: max(1, positioned.height))
                pairs.append(("bounds", .array([.double(bounds.minX), .double(bounds.minY), .double(bounds.width), .double(bounds.height)])))
            } catch {
                pairs.append(("layoutError", errorJSON(error)))
            }
        } catch {
            pairs.append(("error", errorJSON(error)))
        }
        try write(.object(pairs), to: output)
    }

    static func image(_ url: URL, to output: String, themeName: String, dark: Bool) throws {
        let source = try String(contentsOf: url, encoding: .utf8)
        let sheet = try styleSheet(themeName: themeName, dark: dark)
        let png: Data
        if let image = MermaidRendererBridge.image(source: source, styleSheet: sheet) {
            png = try encode(image)
        } else {
            png = try sentinel()
        }
        try png.write(to: URL(fileURLWithPath: output))
    }

    /// The bitmap behind the `NSImage` the bridge returns — the cropped
    /// `CGImage`, at its own pixel size.
    static func encode(_ image: NSImage) throws -> Data {
        guard let rep = image.representations.first,
              let cgImage = rep.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
            throw OracleError.usage("the bridge's image has no CGImage")
        }
        guard cgImage.width == rep.pixelsWide, cgImage.height == rep.pixelsHigh else {
            throw OracleError.usage("the bridge's image was resampled")
        }
        return try png(cgImage)
    }

    static func png(_ image: CGImage) throws -> Data {
        guard let data = NSBitmapImageRep(cgImage: image).representation(using: .png, properties: [:]) else {
            throw OracleError.usage("PNG encoding failed")
        }
        return data
    }

    /// A 1×1 opaque magenta pixel: "the bridge returned nil". A real render is
    /// never 1×1 transparent-free magenta at this size.
    static func sentinel() throws -> Data {
        let context = CGContext(
            data: nil, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 0,
            space: CGColorSpace(name: CGColorSpace.sRGB)!,
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        context.setFillColor(CGColor(srgbRed: 1, green: 0, blue: 1, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: 1, height: 1))
        return try png(context.makeImage()!)
    }

    static func errorJSON(_ error: Error) -> JSON {
        MermaidModelDump.errorJSON(error)
    }

    // MARK: - Theme

    static func color(_ color: NSColor?) -> JSON {
        color.map { AttributeDump.colorJSON($0) } ?? .null
    }

    static func theme(_ theme: DiagramTheme) -> JSON {
        .object([
            ("background", color(theme.background)),
            ("foreground", color(theme.foreground)),
            ("line", color(theme.line)),
            ("accent", color(theme.accent)),
            ("muted", color(theme.muted)),
            ("surface", color(theme.surface)),
            ("border", color(theme.border)),
            ("font", AttributeDump.fontJSON(theme.font)),
            ("lineWidth", .double(theme.lineWidth)),
            ("cornerRadius", .double(theme.cornerRadius)),
            ("transparent", .bool(theme.transparent)),
            ("effectiveLine", color(theme.effectiveLine())),
            ("effectiveAccent", color(theme.effectiveAccent())),
            ("effectiveMuted", color(theme.effectiveMuted())),
            ("effectiveSurface", color(theme.effectiveSurface())),
            ("effectiveBorder", color(theme.effectiveBorder())),
            ("effectiveTextSecondary", color(theme.effectiveTextSecondary())),
            ("effectiveTextFaint", color(theme.effectiveTextFaint())),
            ("effectiveArrow", color(theme.effectiveArrow())),
            ("effectiveInnerStroke", color(theme.effectiveInnerStroke())),
            ("subgraphHeaderColor", color(theme.subgraphHeaderColor())),
            ("keyBadgeColor", color(theme.keyBadgeColor())),
        ])
    }

}

/// `downright-oracle mermaid-bench <dir> <out.json>`: for every `.mmd`
/// under `dir`, the best-of-N wall time of
///
/// - `prepareMs`: `MermaidImageRenderer.prepare(from:)` (parse + layout), and
/// - `bridgeMs`: `MermaidRendererBridge.image(source:styleSheet:)` with its
///   cache emptied first — the whole path a fragment takes on a cache miss
///   (trim, prepare, draw, ink crop, `NSImage`).
enum MermaidBench {
    static func run(_ directory: URL, output: String) throws {
        let sheet = try MermaidDump.styleSheet(themeName: "Paper Light", dark: false)
        let theme = MermaidRendererBridge.theme(from: sheet)
        let files = try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)
            .filter { $0.pathExtension == "mmd" }
            .sorted { $0.path < $1.path }
        let sources = try files.map { try String(contentsOf: $0, encoding: .utf8) }
            .filter { !$0.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
        let rounds = Int(ProcessInfo.processInfo.environment["MERMAID_BENCH_ROUNDS"] ?? "") ?? 5

        func once() -> (prepare: Double, bridge: Double, images: Int) {
            var prepareTime = 0.0
            var bridgeTime = 0.0
            var images = 0
            for source in sources {
                let trimmed = source.trimmingCharacters(in: .whitespacesAndNewlines)
                var start = DispatchTime.now().uptimeNanoseconds
                let renderer = MermaidImageRenderer(theme: theme, config: LayoutConfig())
                _ = try? renderer.prepare(from: trimmed)
                var end = DispatchTime.now().uptimeNanoseconds
                prepareTime += Double(end - start) / 1e6

                MarkdownFragmentImageCaches.mermaid.removeAll()
                start = DispatchTime.now().uptimeNanoseconds
                autoreleasepool {
                    if MermaidRendererBridge.image(source: source, styleSheet: sheet) != nil { images += 1 }
                }
                end = DispatchTime.now().uptimeNanoseconds
                bridgeTime += Double(end - start) / 1e6
            }
            return (prepareTime, bridgeTime, images)
        }

        _ = once()
        var best = (prepare: Double.infinity, bridge: Double.infinity, images: 0)
        for _ in 0..<rounds {
            let r = once()
            best.prepare = min(best.prepare, r.prepare)
            best.bridge = min(best.bridge, r.bridge)
            best.images = r.images
        }
        try write(.object([
            ("diagrams", .int(sources.count)),
            ("images", .int(best.images)),
            ("rounds", .int(rounds)),
            ("prepareMs", .double(best.prepare)),
            ("bridgeMs", .double(best.bridge)),
        ]), to: output)
    }
}
