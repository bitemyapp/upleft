import Foundation
import ElkSwift

func native(_ value: Any) -> Any {
    switch value {
    case let number as NSNumber:
        if CFGetTypeID(number) == CFBooleanGetTypeID() { return number.boolValue }
        return number.doubleValue
    case let string as String: return string
    case let array as [Any]: return array.map(native)
    case let object as [String: Any]:
        var out: [String: Any] = [:]
        for (k, v) in object { out[k] = native(v) }
        return out
    default: return value
    }
}
func pt(_ v: Any?) -> String { let d = v as! [String: Any]; return "(\(d["x"]!),\(d["y"]!))" }
func dump(_ n: [String: Any]) -> String {
    var s = "\(n["id"] ?? "") \(n["x"] ?? "") \(n["y"] ?? "") \(n["width"] ?? "") \(n["height"] ?? "")\n"
    for c in (n["children"] as? [[String: Any]]) ?? [] { s += dump(c) }
    for e in (n["edges"] as? [[String: Any]]) ?? [] {
        for sec in (e["sections"] as? [[String: Any]]) ?? [] {
            s += "\(e["id"] ?? "") \(pt(sec["startPoint"])) \(pt(sec["endPoint"])) \(((sec["bendPoints"] as? [Any]) ?? []).map(pt))\n"
        }
        for l in (e["labels"] as? [[String: Any]]) ?? [] { s += "  label \(l["x"] ?? "") \(l["y"] ?? "")\n" }
    }
    return s
}
let args = CommandLine.arguments

func _sideChar(_ s: PortSide) -> String {
    switch s { case .NORTH: return "N"; case .EAST: return "E"; case .SOUTH: return "S"; case .WEST: return "W"; default: return "U" }
}
func _nm(_ n: LNode?) -> String {
    guard let n else { return "nil" }
    if let l = n.labels.first { return l.text }
    if n.type == .longEdge, let e = n.getProperty(InternalProperties.ORIGIN) as? LEdge {
        return "d\(e.getProperty(InternalProperties.MODEL_ORDER) as? Int ?? -1)"
    }
    return "?\(n.type.rawValue)"
}
func _pn(_ p: LPort?) -> String {
    guard let p else { return "nil" }
    return (p.labels.first?.text ?? "q") + _sideChar(p.side)
}
func _dump(_ g: LGraph, _ title: String) -> String {
    var s = "== \(title) cyc=\(g.getProperty(InternalProperties.CYCLIC) as? Bool ?? false)\n"
    s += "LL:" + g.layerlessNodes.map { " " + _nm($0) }.joined() + "\n"
    var all: [LNode] = []
    for (i, layer) in g.layers.enumerated() {
        s += "L\(i):"
        for node in layer.nodes {
            s += " " + _nm(node) + "[" + node.ports.map { _pn($0) }.joined(separator: ",") + "]"
            all.append(node)
        }
        s += "\n"
    }
    all.append(contentsOf: g.layerlessNodes)
    for node in all {
        for port in node.ports {
            for e in port.outgoingEdges {
                let r = (e.getProperty(InternalProperties.REVERSED) as? Bool ?? false) ? "R" : ""
                s += "e\(e.getProperty(InternalProperties.MODEL_ORDER) as? Int ?? -1)\(r):\(_nm(e.source?.node)).\(_pn(e.source))->\(_nm(e.target?.node)).\(_pn(e.target))\n"
            }
        }
    }
    return s
}


func _mocDump(_ graph: LGraph) -> String {
    var s = "== moc\n"
    for strategy in [OrderingStrategy.NODES_AND_EDGES, OrderingStrategy.PREFER_EDGES] {
        let layers = graph.layers.map { $0.nodes }
        var prevIdx = -1
        for layer in layers {
            let previous = prevIdx == -1 ? layers[0] : layers[prevIdx]
            let comp = org_eclipse_elk_alg_layered_intermediate_preserveorder_ModelOrderNodeComparator(graph, previous, strategy, .EQUAL, .ONLY_WITHIN_GROUP, false)
            var line = ""
            if layer.count > 1 {
                for i in 0..<(layer.count - 1) {
                    for j in (i + 1)..<layer.count {
                        line += "\(comp.compare(layer[i], layer[j])) "
                    }
                }
                for i in stride(from: layer.count - 1, to: 0, by: -1) {
                    line += "\(comp.compare(layer[i], layer[i - 1])) "
                }
            }
            s += line + "\n"
            prevIdx += 1
        }
    }
    return s
}
if args.count > 1 && args[1] == "comp" {
    // comp <seed> <n> <e> <mode>
    let rnd = Random(seed: Int(args[2])!)
    let n = Int(args[3])!, e = Int(args[4])!, mode = Int(args[5])!
    let graph = LGraph()
    graph.setProperty(LayeredOptions.SPACING_COMPONENT_COMPONENT, value: Double(rnd.nextInt(30)) + 0.5)
    if rnd.nextInt(2) == 0 { graph.setProperty(LayeredOptions.ASPECT_RATIO, value: Double(rnd.nextInt(20)) / 7.0 + 0.3) }
    if mode >= 1 { graph.setProperty(InternalProperties.GRAPH_PROPERTIES, value: Set<GraphProperties>([.EXTERNAL_PORTS])) }
    if mode == 2 || mode == 4 { graph.setProperty(LayeredOptions.CONSIDER_MODEL_ORDER_COMPONENTS, value: ComponentOrderingStrategy.MODEL_ORDER) }
    if mode == 3 { graph.setProperty(LayeredOptions.CONSIDER_MODEL_ORDER_COMPONENTS, value: ComponentOrderingStrategy.GROUP_MODEL_ORDER) }
    if rnd.nextInt(2) == 0 { graph.setProperty(LayeredOptions.DIRECTION, value: Direction.DOWN) } else { graph.setProperty(LayeredOptions.DIRECTION, value: Direction.RIGHT) }
    graph.padding.top = 3.5; graph.padding.left = 1.25
    var nodes: [LNode] = []
    for i in 0..<n {
        let node = LNode(graph)
        graph.layerlessNodes.append(node)
        node.labels.append(LLabel("n\(i)"))
        if rnd.nextInt(6) != 0 { node.setProperty(InternalProperties.MODEL_ORDER, value: (i * 5) % 17) }
        if mode >= 1 && rnd.nextInt(4) == 0 {
            node.type = .externalPort
            let sides: [PortSide] = [.NORTH, .EAST, .SOUTH, .WEST]
            node.setProperty(InternalProperties.EXT_PORT_SIDE, value: sides[rnd.nextInt(4)])
        }
        nodes.append(node)
    }
    var pc = 0
    for _ in 0..<e {
        let s = rnd.nextInt(n), t = rnd.nextInt(n)
        if s == t { continue }
        let sp = LPort(); sp.setSide(.EAST); sp.setNode(nodes[s]); sp.labels.append(LLabel("p\(pc)")); pc += 1
        let tp = LPort(); tp.setSide(.WEST); tp.setNode(nodes[t]); tp.labels.append(LLabel("p\(pc)")); pc += 1
        let edge = LEdge(); edge.setSource(sp); edge.setTarget(tp)
        if rnd.nextInt(3) == 0 { let l = LLabel("l"); edge.labels.append(l) }
    }
    let cp = ComponentsProcessor()
    let comps = cp.split(graph)
    var out = "comps \(comps.count)\n"
    for c in comps {
        let sides = (c.getProperty(InternalProperties.EXT_PORT_CONNECTIONS) as? Set<PortSide> ?? []).map { _sideChar($0) }.sorted().joined()
        out += "C[\(sides)]:" + c.layerlessNodes.map { " " + _nm($0) }.joined() + "\n"
        c.size.x = Double(rnd.nextInt(200)) / 3.0
        c.size.y = Double(rnd.nextInt(150)) / 7.0
        c.offset.x = Double(rnd.nextInt(10)) / 3.0
        c.offset.y = Double(rnd.nextInt(10)) / 9.0
        for node in c.layerlessNodes {
            node.position.x = Double(rnd.nextInt(100)) / 3.0
            node.position.y = Double(rnd.nextInt(100)) / 7.0
            for port in node.ports {
                for edge in port.outgoingEdges {
                    edge.bendPoints.add(KVector(Double(rnd.nextInt(50)) / 3.0, 1.5))
                    if rnd.nextInt(3) == 0 {
                        let jp = KVectorChain()
                        jp.add(KVector(1.0 / 3.0, 2.0 / 3.0))
                        edge.setProperty(LayeredOptions.JUNCTION_POINTS, value: jp)
                    }
                    for l in edge.labels { l.position.x = 0.1; l.position.y = 0.2 }
                }
            }
        }
    }
    cp.combine(comps, target: graph)
    out += "size \(graph.size.x) \(graph.size.y) offset \(graph.offset.x) \(graph.offset.y) pad \(graph.padding.top) \(graph.padding.left)\n"
    for node in graph.layerlessNodes {
        out += "\(_nm(node)) \(node.position.x) \(node.position.y) g=\(node.getGraph() === graph)\n"
        for port in node.ports {
            for edge in port.outgoingEdges {
                out += "  bp" + edge.bendPoints.map { " \($0.x),\($0.y)" }.joined()
                if let jp = edge.getProperty(LayeredOptions.JUNCTION_POINTS) as? KVectorChain { out += " jp" + jp.map { " \($0.x),\($0.y)" }.joined() }
                out += edge.labels.map { " l \($0.position.x),\($0.position.y)" }.joined()
                out += "\n"
            }
        }
    }
    print(out, terminator: "")
    exit(0)
}
if args.count > 1 && args[1] == "chain" {
    // chain <seed> <n> <e>
    let rnd = Random(seed: Int(args[2])!)
    let n = Int(args[3])!, e = Int(args[4])!
    let graph = LGraph()
    graph.setProperty(InternalProperties.RANDOM, value: Random(seed: 7))
    graph.setProperty(LayeredOptions.CONSIDER_MODEL_ORDER_STRATEGY, value: OrderingStrategy.NODES_AND_EDGES)
    graph.setProperty(InternalProperties.MAX_MODEL_ORDER_NODES, value: n)
    graph.setProperty(LayeredOptions.HIGH_DEGREE_NODES_THRESHOLD, value: 4)
    var nodes: [LNode] = []
    for i in 0..<n {
        let node = LNode(graph)
        graph.layerlessNodes.append(node)
        node.labels.append(LLabel("n\(i)"))
        if rnd.nextInt(5) != 0 { node.setProperty(InternalProperties.MODEL_ORDER, value: i) }
        switch rnd.nextInt(14) {
        case 0: node.setProperty(LayeredOptions.LAYERING_LAYER_CONSTRAINT, value: LayerConstraint.FIRST)
        case 1: node.setProperty(LayeredOptions.LAYERING_LAYER_CONSTRAINT, value: LayerConstraint.LAST)
        case 2: node.setProperty(LayeredOptions.LAYERING_LAYER_CONSTRAINT, value: LayerConstraint.FIRST_SEPARATE)
        case 3: node.setProperty(LayeredOptions.LAYERING_LAYER_CONSTRAINT, value: LayerConstraint.LAST_SEPARATE)
        default: break
        }
        switch rnd.nextInt(8) {
        case 0: node.setProperty(InternalProperties.IN_LAYER_CONSTRAINT, value: InLayerConstraint.TOP)
        case 1: node.setProperty(InternalProperties.IN_LAYER_CONSTRAINT, value: InLayerConstraint.BOTTOM)
        default: break
        }
        switch rnd.nextInt(4) {
        case 0: node.setProperty(LayeredOptions.PORT_CONSTRAINTS, value: PortConstraints.FIXED_SIDE)
        case 1: node.setProperty(LayeredOptions.PORT_CONSTRAINTS, value: PortConstraints.FIXED_ORDER)
        default: break
        }
        nodes.append(node)
    }
    var portCounter = 0
    func newPort(_ node: LNode, _ side: PortSide) -> LPort {
        let port = LPort()
        port.setSide(side)
        port.setNode(node)
        port.labels.append(LLabel("p\(portCounter)"))
        port.setProperty(LayeredOptions.PORT_INDEX, value: (portCounter * 7) % 11)
        if rnd.nextInt(2) == 0 { port.setProperty(InternalProperties.MODEL_ORDER, value: portCounter) }
        portCounter += 1
        return port
    }
    for k in 0..<e {
        let s = rnd.nextInt(n), t = rnd.nextInt(n)
        if s == t { continue }
        var sp: LPort? = nil
        if rnd.nextInt(3) == 0 { sp = nodes[s].ports.first { $0.side == .EAST } }
        let sourcePort = sp ?? newPort(nodes[s], .EAST)
        var tp: LPort? = nil
        if rnd.nextInt(3) == 0 { tp = nodes[t].ports.first { $0.side == .WEST } }
        let targetPort = tp ?? newPort(nodes[t], .WEST)
        let edge = LEdge()
        edge.setSource(sourcePort)
        edge.setTarget(targetPort)
        edge.setProperty(InternalProperties.MODEL_ORDER, value: k)
        if rnd.nextInt(6) == 0 {
            edge.setProperty(LayeredOptions.PRIORITY_DIRECTION, value: 2)
            edge.setProperty(LayeredOptions.PRIORITY_SHORTNESS, value: 3)
        }
    }
    var out = _dump(graph, "input")
    let steps: [(String, (LGraph, IElkProgressMonitor) -> Void)] = [
        ("EdgeAndLayerConstraintEdgeReverser", { EdgeAndLayerConstraintEdgeReverser().process($0, $1) }),
        ("GreedyCycleBreaker", { GreedyCycleBreaker().process($0, $1) }),
        ("LayerConstraintPreprocessor", { LayerConstraintPreprocessor().process($0, $1) }),
        ("NetworkSimplexLayerer", { NetworkSimplexLayerer().process($0, $1) }),
        ("LayerConstraintPostprocessor", { LayerConstraintPostprocessor().process($0, $1) }),
        ("HighDegreeNodeLayeringProcessor", { HighDegreeNodeLayeringProcessor().process($0, $1) }),
        ("LongEdgeSplitter", { LongEdgeSplitter().process($0, $1) }),
        ("SortByInputModelProcessor", { SortByInputModelProcessor().process($0, $1) }),
        ("InLayerConstraintProcessor", { InLayerConstraintProcessor().process($0, $1) }),
        ("PortListSorter", { PortListSorter().process($0, $1) }),
        ("ReversedEdgeRestorer", { ReversedEdgeRestorer().process($0, $1) }),
    ]
    for (name, step) in steps {
        step(graph, BasicProgressMonitor())
        out += _dump(graph, name)
        if name == "SortByInputModelProcessor" { out += _mocDump(graph) }
    }
    print(out, terminator: "")
    exit(0)
}
if args.count > 1 && args[1] == "ns" {
    // ns <seed> <n> <e> <balance 0/1> <iterLimit>
    let random = Random(seed: Int(args[2])!)
    let n = Int(args[3])!, e = Int(args[4])!
    let graph = NGraph()
    for i in 0..<n { _ = NNode.of().id(i).create(graph) }
    for _ in 0..<e {
        let src = random.nextInt(n)
        var tgt = random.nextInt(n)
        while src == tgt { tgt = random.nextInt(n) }
        _ = NEdge.of().delta(random.nextInt(3)).weight(Double(random.nextInt(4))).source(graph.nodes[src]).target(graph.nodes[tgt]).create()
    }
    for i in 0..<(n / 2) {
        _ = NEdge.of().delta(random.nextInt(3)).weight(Double(random.nextInt(4))).source(graph.nodes[i]).target(graph.nodes[i + 1]).create()
    }
    for node in graph.nodes {
        for edge in Array(node.getOutgoingEdges()) {
            if let s = edge.source, let t = edge.target, s.id > t.id { _ = edge.reverse() }
        }
    }
    let ns = NetworkSimplex.forGraph(graph).withIterationLimit(Int(args[6])!).withBalancing(args[5] == "1")
    ns.execute()
    print(graph.nodes.map { "\($0.id):\($0.layer)" }.joined(separator: " "))
    exit(0)
}
let data = try! Data(contentsOf: URL(fileURLWithPath: args[1]))
let graph = native(try! JSONSerialization.jsonObject(with: data)) as! [String: Any]
let result = try! ELK().layout(graph: graph)
print(dump(result), terminator: "")
