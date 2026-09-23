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
            pairs.append(("parsed", graph(try MermaidParser.parse(source))))
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
            pairs.append(("parsed", graph(parsed)))
            do {
                let positioned = try GraphLayout(config: LayoutConfig()).layout(parsed)
                pairs.append(("positioned", positionedGraph(positioned)))
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

    // MARK: - Errors

    /// The thrown error as its enum case and payload strings. Several of the
    /// library's error types are private, so this reads them through `Mirror`.
    static func errorJSON(_ error: Error) -> JSON {
        let mirror = Mirror(reflecting: error)
        var name = String(describing: error)
        var values: [JSON] = []
        if let child = mirror.children.first, let label = child.label {
            name = label
            collectStrings(child.value, into: &values)
        } else if let paren = name.firstIndex(of: "(") {
            name = String(name[..<paren])
        }
        return .object([("case", .string(name)), ("values", .array(values))])
    }

    static func collectStrings(_ value: Any, into out: inout [JSON]) {
        if let string = value as? String {
            out.append(.string(string))
            return
        }
        for child in Mirror(reflecting: value).children {
            collectStrings(child.value, into: &out)
        }
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

    // MARK: - Parsed diagrams

    static func strings(_ values: [String]) -> JSON {
        .array(values.map { .string($0) })
    }

    static func dictionary(_ dict: [String: String]?) -> JSON {
        guard let dict else { return .null }
        return .object(dict.keys.sorted().map { ($0, .string(dict[$0]!)) })
    }

    static func graph(_ graph: MermaidGraph) -> JSON {
        var pairs: [(String, JSON)] = [("type", .string(graph.type.rawValue))]
        switch graph.typedPayload {
        case .flowchart(let model), .stateDiagram(let model):
            pairs.append(("model", flowModel(model)))
        case .sequenceDiagram(let diagram):
            pairs.append(("model", sequence(diagram)))
        case .classDiagram(let diagram):
            pairs.append(("model", classDiagram(diagram)))
        case .erDiagram(let diagram):
            pairs.append(("model", erDiagram(diagram)))
        case .xyChart(let chart):
            pairs.append(("model", xyChart(chart)))
        case nil:
            pairs.append(("model", .null))
        }
        return .object(pairs)
    }

    static func flowModel(_ model: ParsedGraphModel) -> JSON {
        .object([
            ("direction", .string(model.direction.rawValue)),
            ("nodes", .array(model.nodesInOrder.map { entry in
                .object([
                    ("id", .string(entry.id)),
                    ("nodeId", .string(entry.node.id)),
                    ("label", .string(entry.node.label)),
                    ("shape", .string(entry.node.shape.rawValue)),
                ])
            })),
            ("edges", .array(model.edges.map { edge in
                .object([
                    ("source", .string(edge.source)),
                    ("target", .string(edge.target)),
                    ("label", .string(edge.label)),
                    ("style", .string(edge.style.rawValue)),
                    ("hasArrowStart", .bool(edge.hasArrowStart)),
                    ("hasArrowEnd", .bool(edge.hasArrowEnd)),
                    ("inlineStyle", dictionary(edge.inlineStyle)),
                ])
            })),
            ("subgraphs", .array(model.subgraphs.map(subgraph))),
            ("classDefs", .object(model.classDefs.keys.sorted().map { ($0, dictionary(model.classDefs[$0])) })),
            ("classAssignments", dictionary(model.classAssignments)),
            ("nodeStyles", .object(model.nodeStyles.keys.sorted().map { ($0, dictionary(model.nodeStyles[$0])) })),
            ("linkStyles", .array(model.linkStyles.keys.sorted().map { key in
                .array([.int(key), dictionary(model.linkStyles[key])])
            })),
        ])
    }

    static func subgraph(_ sub: original_src_types.MermaidSubgraph) -> JSON {
        .object([
            ("id", .string(sub.id)),
            ("label", .string(sub.label)),
            ("nodeIds", strings(sub.nodeIds)),
            ("direction", .string(sub.direction?.rawValue)),
            ("children", .array(sub.children.map(subgraph))),
        ])
    }

    static func sequence(_ d: SequenceDiagram) -> JSON {
        .object([
            ("actors", .array(d.actors.map { a in
                .object([("id", .string(a.id)), ("label", .string(a.label)), ("type", .string(a.type))])
            })),
            ("messages", .array(d.messages.map { m in
                .object([
                    ("from", .string(m.from)),
                    ("to", .string(m.to)),
                    ("label", .string(m.label)),
                    ("lineStyle", .string(m.lineStyle)),
                    ("arrowHead", .string(m.arrowHead)),
                    ("activate", .bool(m.activate)),
                    ("deactivate", .bool(m.deactivate)),
                ])
            })),
            ("blocks", .array(d.blocks.map { b in
                .object([
                    ("type", .string(b.type)),
                    ("label", .string(b.label)),
                    ("startIndex", .int(b.startIndex)),
                    ("endIndex", .int(b.endIndex)),
                    ("dividers", .array(b.dividers.map { .object([("index", .int($0.index)), ("label", .string($0.label))]) })),
                ])
            })),
            ("notes", .array(d.notes.map { n in
                .object([
                    ("actorIds", strings(n.actorIds)),
                    ("text", .string(n.text)),
                    ("position", .string(n.position)),
                    ("afterIndex", .int(n.afterIndex)),
                ])
            })),
        ])
    }

    static func member(_ m: ClassMember) -> JSON {
        .object([
            ("visibility", .string(m.visibility)),
            ("name", .string(m.name)),
            ("type", .string(m.type)),
            ("isStatic", .bool(m.isStatic)),
            ("isAbstract", .bool(m.isAbstract)),
            ("isMethod", .bool(m.isMethod)),
            ("params", .string(m.params)),
        ])
    }

    static func classDiagram(_ d: ClassDiagram) -> JSON {
        .object([
            ("classes", .array(d.classes.map { c in
                .object([
                    ("id", .string(c.id)),
                    ("label", .string(c.label)),
                    ("attributes", .array(c.attributes.map(member))),
                    ("methods", .array(c.methods.map(member))),
                    ("annotation", .string(c.annotation)),
                ])
            })),
            ("relationships", .array(d.relationships.map { r in
                .object([
                    ("from", .string(r.from)),
                    ("to", .string(r.to)),
                    ("type", .string(r.type)),
                    ("markerAt", .string(r.markerAt)),
                    ("label", .string(r.label)),
                    ("fromCardinality", .string(r.fromCardinality)),
                    ("toCardinality", .string(r.toCardinality)),
                ])
            })),
            ("namespaces", .array(d.namespaces.map { n in
                .object([("name", .string(n.name)), ("classIds", strings(n.classIds))])
            })),
        ])
    }

    static func attribute(_ a: ErAttribute) -> JSON {
        .object([
            ("type", .string(a.type)),
            ("name", .string(a.name)),
            ("keys", strings(a.keys)),
            ("comment", .string(a.comment)),
        ])
    }

    static func erDiagram(_ d: ErDiagram) -> JSON {
        .object([
            ("entities", .array(d.entities.map { e in
                .object([
                    ("id", .string(e.id)),
                    ("label", .string(e.label)),
                    ("attributes", .array(e.attributes.map(attribute))),
                ])
            })),
            ("relationships", .array(d.relationships.map { r in
                .object([
                    ("entity1", .string(r.entity1)),
                    ("entity2", .string(r.entity2)),
                    ("cardinality1", .string(r.cardinality1)),
                    ("cardinality2", .string(r.cardinality2)),
                    ("label", .string(r.label)),
                    ("identifying", .bool(r.identifying)),
                ])
            })),
        ])
    }

    static func axis(_ a: XYAxis) -> JSON {
        .object([
            ("title", .string(a.title)),
            ("categories", a.categories.map(strings) ?? .null),
            ("range", a.range.map { .array([.double($0.min), .double($0.max)]) } ?? .null),
        ])
    }

    static func xyChart(_ c: XYChart) -> JSON {
        .object([
            ("title", .string(c.title)),
            ("horizontal", .bool(c.horizontal)),
            ("xAxis", axis(c.xAxis)),
            ("yAxis", axis(c.yAxis)),
            ("series", .array(c.series.map { s in
                .object([("type", .string(s.type.rawValue)), ("data", .array(s.data.map { .double($0) }))])
            })),
        ])
    }

    // MARK: - Positioned diagrams

    static func point(_ x: Double, _ y: Double) -> JSON {
        .array([.double(x), .double(y)])
    }

    static func positionedGraph(_ g: PositionedGraph) -> JSON {
        var pairs: [(String, JSON)] = [
            ("type", .string(g.diagram.type.rawValue)),
            ("width", .double(g.width)),
            ("height", .double(g.height)),
        ]
        switch g.content {
        case .flowchart(let nodes, let edges, let groups):
            pairs.append(("kind", .string("flowchart")))
            pairs.append(contentsOf: flowContent(nodes, edges, groups))
        case .stateDiagram(let nodes, let edges, let groups):
            pairs.append(("kind", .string("stateDiagram")))
            pairs.append(contentsOf: flowContent(nodes, edges, groups))
        case .sequenceDiagram(let actors, let messages, let blocks, let lifelines, let activations, let notes):
            pairs.append(("kind", .string("sequenceDiagram")))
            pairs.append(("actors", .array(actors.map { a in
                .object([
                    ("id", .string(a.id)), ("label", .string(a.label)), ("type", .string(a.type)),
                    ("x", .double(a.x)), ("y", .double(a.y)), ("width", .double(a.width)), ("height", .double(a.height)),
                ])
            })))
            pairs.append(("messages", .array(messages.map { m in
                .object([
                    ("from", .string(m.from)), ("to", .string(m.to)), ("label", .string(m.label)),
                    ("lineStyle", .string(m.lineStyle)), ("arrowHead", .string(m.arrowHead)),
                    ("x1", .double(m.x1)), ("x2", .double(m.x2)), ("y", .double(m.y)), ("isSelf", .bool(m.isSelf)),
                ])
            })))
            pairs.append(("blocks", .array(blocks.map { b in
                .object([
                    ("type", .string(b.type)), ("label", .string(b.label)),
                    ("x", .double(b.x)), ("y", .double(b.y)), ("width", .double(b.width)), ("height", .double(b.height)),
                    ("dividers", .array(b.dividers.map { .object([("y", .double($0.y)), ("label", .string($0.label))]) })),
                ])
            })))
            pairs.append(("lifelines", .array(lifelines.map { l in
                .object([("actorId", .string(l.actorId)), ("x", .double(l.x)), ("topY", .double(l.topY)), ("bottomY", .double(l.bottomY))])
            })))
            // Activations still open at the end are appended in Dictionary
            // order, which Swift randomises per process: compare them sorted.
            let sortedActivations = activations.sorted { a, b in
                if a.topY != b.topY { return a.topY < b.topY }
                if a.x != b.x { return a.x < b.x }
                return a.bottomY < b.bottomY
            }
            pairs.append(("activations", .array(sortedActivations.map { a in
                .object([
                    ("actorId", .string(a.actorId)), ("x", .double(a.x)), ("topY", .double(a.topY)),
                    ("bottomY", .double(a.bottomY)), ("width", .double(a.width)),
                ])
            })))
            pairs.append(("notes", .array(notes.map { n in
                .object([
                    ("text", .string(n.text)), ("x", .double(n.x)), ("y", .double(n.y)),
                    ("width", .double(n.width)), ("height", .double(n.height)),
                    ("position", .string(n.position)), ("actors", strings(n.actors)),
                ])
            })))
        case .classDiagram(let classes, let relationships):
            pairs.append(("kind", .string("classDiagram")))
            pairs.append(("classes", .array(classes.map { c in
                .object([
                    ("id", .string(c.id)), ("label", .string(c.label)), ("annotation", .string(c.annotation)),
                    ("attributes", .array(c.attributes.map(member))), ("methods", .array(c.methods.map(member))),
                    ("x", .double(c.x)), ("y", .double(c.y)), ("width", .double(c.width)), ("height", .double(c.height)),
                    ("headerHeight", .double(c.headerHeight)), ("attrHeight", .double(c.attrHeight)),
                    ("methodHeight", .double(c.methodHeight)),
                ])
            })))
            pairs.append(("relationships", .array(relationships.map { r in
                .object([
                    ("from", .string(r.from)), ("to", .string(r.to)), ("type", .string(r.type)),
                    ("markerAt", .string(r.markerAt)), ("label", .string(r.label)),
                    ("fromCardinality", .string(r.fromCardinality)), ("toCardinality", .string(r.toCardinality)),
                    ("points", .array(r.points.map { point($0.x, $0.y) })),
                    ("labelPosition", r.labelPosition.map { point($0.x, $0.y) } ?? .null),
                ])
            })))
        case .erDiagram(let entities, let relationships):
            pairs.append(("kind", .string("erDiagram")))
            pairs.append(("entities", .array(entities.map { e in
                .object([
                    ("id", .string(e.id)), ("label", .string(e.label)),
                    ("attributes", .array(e.attributes.map(attribute))),
                    ("x", .double(e.x)), ("y", .double(e.y)), ("width", .double(e.width)), ("height", .double(e.height)),
                    ("headerHeight", .double(e.headerHeight)), ("rowHeight", .double(e.rowHeight)),
                ])
            })))
            pairs.append(("relationships", .array(relationships.map { r in
                .object([
                    ("entity1", .string(r.entity1)), ("entity2", .string(r.entity2)),
                    ("cardinality1", .string(r.cardinality1)), ("cardinality2", .string(r.cardinality2)),
                    ("label", .string(r.label)), ("identifying", .bool(r.identifying)),
                    ("points", .array(r.points.map { point($0.x, $0.y) })),
                ])
            })))
        case .xyChart(let chart):
            pairs.append(("kind", .string("xyChart")))
            pairs.append(("chart", positionedChart(chart)))
        }
        return .object(pairs)
    }

    static func flowContent(_ nodes: [PositionedNode], _ edges: [PositionedEdge], _ groups: [PositionedGroup]) -> [(String, JSON)] {
        [
            ("nodes", .array(nodes.map { n in
                .object([
                    ("id", .string(n.id)), ("label", .string(n.label)), ("shape", .string(n.shape)),
                    ("x", .double(n.x)), ("y", .double(n.y)), ("width", .double(n.width)), ("height", .double(n.height)),
                    ("inlineStyle", dictionary(n.inlineStyle)),
                ])
            })),
            ("edges", .array(edges.map { e in
                .object([
                    ("source", .string(e.source)), ("target", .string(e.target)), ("label", .string(e.label)),
                    ("style", .string(e.style)), ("hasArrowStart", .bool(e.hasArrowStart)), ("hasArrowEnd", .bool(e.hasArrowEnd)),
                    ("points", .array(e.points.map { point($0.x, $0.y) })),
                    ("labelPosition", e.labelPosition.map { point($0.x, $0.y) } ?? .null),
                    ("inlineStyle", dictionary(e.inlineStyle)),
                ])
            })),
            ("groups", .array(groups.map(group))),
        ]
    }

    static func group(_ g: PositionedGroup) -> JSON {
        .object([
            ("id", .string(g.id)), ("label", .string(g.label)),
            ("x", .double(g.x)), ("y", .double(g.y)), ("width", .double(g.width)), ("height", .double(g.height)),
            ("headerHeight", .double(g.headerHeight)),
            ("children", .array(g.children.map(group))),
        ])
    }

    static func tick(_ t: XYAxisTick) -> JSON {
        .object([
            ("label", .string(t.label)), ("x", .double(t.x)), ("y", .double(t.y)),
            ("tx", .double(t.tx)), ("ty", .double(t.ty)), ("labelX", .double(t.labelX)), ("labelY", .double(t.labelY)),
            ("textAnchor", .string(t.textAnchor)),
        ])
    }

    static func positionedAxis(_ a: PositionedXYAxis) -> JSON {
        .object([
            ("title", a.title.map { t in
                .object([
                    ("text", .string(t.text)), ("x", .double(t.x)), ("y", .double(t.y)),
                    ("rotate", t.rotate.map { .double($0) } ?? .null),
                ])
            } ?? .null),
            ("ticks", .array(a.ticks.map(tick))),
            ("line", .array([.double(a.line.x1), .double(a.line.y1), .double(a.line.x2), .double(a.line.y2)])),
        ])
    }

    static func positionedChart(_ c: PositionedXYChart) -> JSON {
        .object([
            ("width", .double(c.width)),
            ("height", .double(c.height)),
            ("horizontal", .bool(c.horizontal)),
            ("title", c.title.map { .object([("text", .string($0.text)), ("x", .double($0.x)), ("y", .double($0.y))]) } ?? .null),
            ("xAxis", positionedAxis(c.xAxis)),
            ("yAxis", positionedAxis(c.yAxis)),
            ("plotArea", .array([.double(c.plotArea.x), .double(c.plotArea.y), .double(c.plotArea.width), .double(c.plotArea.height)])),
            ("bars", .array(c.bars.map { b in
                .object([
                    ("x", .double(b.x)), ("y", .double(b.y)), ("width", .double(b.width)), ("height", .double(b.height)),
                    ("value", .double(b.value)), ("label", .string(b.label)),
                    ("seriesIndex", .int(b.seriesIndex)), ("colorIndex", .int(b.colorIndex)),
                ])
            })),
            ("lines", .array(c.lines.map { l in
                .object([
                    ("points", .array(l.points.map { p in
                        .object([("x", .double(p.x)), ("y", .double(p.y)), ("value", .double(p.value)), ("label", .string(p.label))])
                    })),
                    ("seriesIndex", .int(l.seriesIndex)),
                    ("colorIndex", .int(l.colorIndex)),
                ])
            })),
            ("gridLines", .array(c.gridLines.map { .array([.double($0.x1), .double($0.y1), .double($0.x2), .double($0.y2)]) })),
            ("legend", .array(c.legend.map { i in
                .object([
                    ("label", .string(i.label)), ("x", .double(i.x)), ("y", .double(i.y)), ("type", .string(i.type.rawValue)),
                    ("seriesIndex", .int(i.seriesIndex)), ("colorIndex", .int(i.colorIndex)),
                ])
            })),
        ])
    }
}

/// `downright-oracle mermaid-bench <dir> <out.json>`: prepare and render every
/// `.mmd` under `dir` the way `MermaidRendererBridge` does, uncached, and
/// report the best-of-N wall time per stage.
enum MermaidBench {
    static func run(_ directory: URL, output: String) throws {
        let sheet = try MermaidDump.styleSheet(themeName: "Paper Light", dark: false)
        let theme = MermaidRendererBridge.theme(from: sheet)
        let scale = NSScreen.main?.backingScaleFactor ?? 2
        let files = try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)
            .filter { $0.pathExtension == "mmd" }
            .sorted { $0.path < $1.path }
        let sources = try files.map { try String(contentsOf: $0, encoding: .utf8).trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
        let rounds = Int(ProcessInfo.processInfo.environment["MERMAID_BENCH_ROUNDS"] ?? "") ?? 5

        func once() -> (prepare: Double, render: Double) {
            var prepareTime = 0.0
            var renderTime = 0.0
            for source in sources {
                let start = DispatchTime.now().uptimeNanoseconds
                let renderer = MermaidImageRenderer(theme: theme, config: LayoutConfig())
                let prepared = try? renderer.prepare(from: source)
                let middle = DispatchTime.now().uptimeNanoseconds
                if let prepared { _ = draw(prepared, scale: scale) }
                let end = DispatchTime.now().uptimeNanoseconds
                prepareTime += Double(middle - start) / 1e6
                renderTime += Double(end - middle) / 1e6
            }
            return (prepareTime, renderTime)
        }

        _ = once()
        var best = (prepare: Double.infinity, render: Double.infinity)
        for _ in 0..<rounds {
            let r = once()
            best.prepare = min(best.prepare, r.prepare)
            best.render = min(best.render, r.render)
        }
        try write(.object([
            ("diagrams", .int(sources.count)),
            ("rounds", .int(rounds)),
            ("prepareMs", .double(best.prepare)),
            ("renderMs", .double(best.render)),
            ("totalMs", .double(best.prepare + best.render)),
        ]), to: output)
    }

    /// `MermaidRendererBridge.render` up to the crop (the crop is a scan of
    /// the alpha channel and is included).
    static func draw(_ prepared: PreparedDiagram, scale: CGFloat) -> CGImage? {
        let bounds = prepared.bounds
        guard bounds.width > 0, bounds.height > 0 else { return nil }
        let padded = bounds.insetBy(dx: -32, dy: -32)
        let pixelWidth = Int((padded.width * scale).rounded())
        let pixelHeight = Int((padded.height * scale).rounded())
        guard pixelWidth > 0, pixelHeight > 0,
              let ctx = CGContext(
                  data: nil, width: pixelWidth, height: pixelHeight,
                  bitsPerComponent: 8, bytesPerRow: 0,
                  space: CGColorSpaceCreateDeviceRGB(),
                  bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue | CGBitmapInfo.byteOrder32Big.rawValue
              ) else { return nil }
        ctx.translateBy(x: 0, y: CGFloat(pixelHeight))
        ctx.scaleBy(x: 1, y: -1)
        ctx.scaleBy(x: scale, y: scale)
        ctx.translateBy(x: -padded.minX, y: -padded.minY)
        prepared.render(ctx, bounds)
        guard let image = ctx.makeImage(), let base = ctx.data else { return nil }
        let pixels = base.assumingMemoryBound(to: UInt8.self)
        var minX = pixelWidth, maxX = -1
        for y in 0..<pixelHeight {
            let row = pixels + y * ctx.bytesPerRow
            for x in 0..<pixelWidth where row[x * 4 + 3] > 8 {
                minX = min(minX, x); maxX = max(maxX, x)
            }
        }
        _ = maxX
        return image
    }
}
