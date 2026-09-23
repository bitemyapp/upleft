import BeautifulMermaid
import Foundation

/// The diagram-model half of `MermaidDump`: the parsed `MermaidGraph` and
/// the `PositionedGraph` as JSON. It imports only BeautifulMermaid, so the
/// ELK capture tool (crates/mermaid/tools/elk-capture) compiles it too.
/// `crates/conformance/src/dump/mermaid.rs` writes the same shapes.
enum MermaidModelDump {
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
