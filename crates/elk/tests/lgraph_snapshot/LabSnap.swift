import Foundation

// Snapshot dumper for differential tests of single processors.
// ELKLAB_SNAP=Name1,Name2 ELKLAB_SNAPDIR=dir: before and after every run of a
// listed processor, writes the processed LGraph (and everything reachable from
// it) as JSON to dir/<seq>-<Name>-before.json / -after.json.

private let _snapNames: Set<String> = {
    guard let s = ProcessInfo.processInfo.environment["ELKLAB_SNAP"] else { return [] }
    return Set(s.split(separator: ",").map(String.init))
}()
private let _snapDir = ProcessInfo.processInfo.environment["ELKLAB_SNAPDIR"] ?? "/tmp"
private var _snapSeq = 0
private var _snapPending: [String] = []

package func _labSnapBefore(_ g: LGraph, _ fullName: String) {
    let name = _labShort(fullName)
    guard _snapNames.contains(name) || _snapNames.contains("*") else { return }
    _snapSeq += 1
    let base = String(format: "%04d-%@", _snapSeq, name)
    _snapPending.append(base)
    try! _LabSnapWriter(g).json().write(toFile: "\(_snapDir)/\(base)-before.json", atomically: true, encoding: .utf8)
}

package func _labSnapAfter(_ g: LGraph, _ fullName: String) {
    let name = _labShort(fullName)
    guard _snapNames.contains(name) || _snapNames.contains("*") else { return }
    let base = _snapPending.removeLast()
    try! _LabSnapWriter(g).json().write(toFile: "\(_snapDir)/\(base)-after.json", atomically: true, encoding: .utf8)
}

private func d(_ v: Double) -> String { "\"\(v)\"" }
private func esc(_ s: String) -> String {
    var r = "\""
    for c in s.unicodeScalars {
        switch c {
        case "\"": r += "\\\""
        case "\\": r += "\\\\"
        case "\n": r += "\\n"
        case "\r": r += "\\r"
        case "\t": r += "\\t"
        default:
            if c.value < 0x20 { r += String(format: "\\u%04x", c.value) } else { r.unicodeScalars.append(c) }
        }
    }
    return r + "\""
}

private final class _LabSnapWriter {
    let g: LGraph
    var graphs: [ObjectIdentifier: Int] = [:]; var graphList: [LGraph] = []
    var layers: [ObjectIdentifier: Int] = [:]; var layerList: [Layer] = []
    var nodes: [ObjectIdentifier: Int] = [:]; var nodeList: [LNode] = []
    var ports: [ObjectIdentifier: Int] = [:]; var portList: [LPort] = []
    var edges: [ObjectIdentifier: Int] = [:]; var edgeList: [LEdge] = []
    var labels: [ObjectIdentifier: Int] = [:]; var labelList: [LLabel] = []
    var objs: [ObjectIdentifier: Int] = [:]
    var queue: [(Int, Int)] = []  // (kind, index): 0 node 1 port 2 edge 3 label 4 layer
    var portByPosition: [ObjectIdentifier: Int] = [:]

    init(_ g: LGraph) {
        self.g = g
        _ = graph(g)
        for l in g.layers { _ = layer(l) }
        for n in g.layerlessNodes { _ = node(n) }
        visitProps(g.getAllProperties())
        var qi = 0
        while qi < queue.count {
            let (k, i) = queue[qi]; qi += 1
            switch k {
            case 0:
                let n = nodeList[i]
                if let gg = n.graph { _ = graph(gg) }
                if let l = n.layer { _ = layer(l) }
                for p in n.ports { _ = port(p) }
                for l in n.labels { _ = label(l) }
                if let ng = n.nestedGraph { _ = graph(ng) }
                visitProps(n.getAllProperties())
            case 1:
                let p = portList[i]
                if let o = p.owner { _ = node(o) }
                for l in p.labels { _ = label(l) }
                for e in p.incomingEdges { _ = edge(e) }
                for e in p.outgoingEdges { _ = edge(e) }
                visitProps(p.getAllProperties())
            case 2:
                let e = edgeList[i]
                if let s = e.source { _ = port(s) }
                if let t = e.target { _ = port(t) }
                for l in e.labels { _ = label(l) }
                visitProps(e.getAllProperties())
            case 3:
                visitProps(labelList[i].getAllProperties())
            default:
                let l = layerList[i]
                _ = graph(l.owner)
                for n in l.nodes { _ = node(n) }
                visitProps(l.getAllProperties())
            }
        }
        for (i, p) in portList.enumerated() where portByPosition[ObjectIdentifier(p.position)] == nil {
            portByPosition[ObjectIdentifier(p.position)] = i
        }
    }

    func graph(_ x: LGraph) -> Int {
        if let i = graphs[ObjectIdentifier(x)] { return i }
        graphs[ObjectIdentifier(x)] = graphList.count; graphList.append(x); return graphList.count - 1
    }
    func layer(_ x: Layer) -> Int {
        if let i = layers[ObjectIdentifier(x)] { return i }
        layers[ObjectIdentifier(x)] = layerList.count; layerList.append(x); queue.append((4, layerList.count - 1)); return layerList.count - 1
    }
    func node(_ x: LNode) -> Int {
        if let i = nodes[ObjectIdentifier(x)] { return i }
        nodes[ObjectIdentifier(x)] = nodeList.count; nodeList.append(x); queue.append((0, nodeList.count - 1)); return nodeList.count - 1
    }
    func port(_ x: LPort) -> Int {
        if let i = ports[ObjectIdentifier(x)] { return i }
        ports[ObjectIdentifier(x)] = portList.count; portList.append(x); queue.append((1, portList.count - 1)); return portList.count - 1
    }
    func edge(_ x: LEdge) -> Int {
        if let i = edges[ObjectIdentifier(x)] { return i }
        edges[ObjectIdentifier(x)] = edgeList.count; edgeList.append(x); queue.append((2, edgeList.count - 1)); return edgeList.count - 1
    }
    func label(_ x: LLabel) -> Int {
        if let i = labels[ObjectIdentifier(x)] { return i }
        labels[ObjectIdentifier(x)] = labelList.count; labelList.append(x); queue.append((3, labelList.count - 1)); return labelList.count - 1
    }
    func obj(_ x: AnyObject) -> Int {
        if let i = objs[ObjectIdentifier(x)] { return i }
        objs[ObjectIdentifier(x)] = objs.count; return objs.count - 1
    }

    func unwrap(_ v: Any) -> Any? {
        let m = Mirror(reflecting: v)
        if m.displayStyle == .optional {
            guard let c = m.children.first else { return nil }
            return unwrap(c.value)
        }
        return v
    }

    func visitValue(_ v0: Any) {
        guard let v = unwrap(v0) else { return }
        switch v {
        case let x as LNode: _ = node(x)
        case let x as LPort: _ = port(x)
        case let x as LEdge: _ = edge(x)
        case let x as LLabel: _ = label(x)
        case let x as Layer: _ = layer(x)
        case let x as LGraph: _ = graph(x)
        case let xs as [LNode]: for x in xs { _ = node(x) }
        case let xs as [LPort]: for x in xs { _ = port(x) }
        case let xs as [LEdge]: for x in xs { _ = edge(x) }
        case let xs as [LLabel]: for x in xs { _ = label(x) }
        default: break
        }
    }

    func visitProps(_ props: [String: Any]) {
        for k in props.keys.sorted() { visitValue(props[k]!) }
    }

    func vec(_ v: KVector) -> String { "[\(d(v.x)),\(d(v.y))]" }
    func spacing(_ s: Spacing) -> String { "[\(d(s.top)),\(d(s.right)),\(d(s.bottom)),\(d(s.left))]" }

    func value(_ key: String, _ v0: Any, _ owner: LNode?) -> String {
        guard let v = unwrap(v0) else { return "{\"t\":\"nil\"}" }
        switch v {
        case let x as Bool: return "{\"t\":\"b\",\"v\":\(x)}"
        case let x as Int: return "{\"t\":\"i\",\"v\":\(x)}"
        case let x as Double: return "{\"t\":\"d\",\"v\":\(d(x))}"
        case let x as String: return "{\"t\":\"s\",\"v\":\(esc(x))}"
        case let x as KVector:
            var alias = ""
            if let p = portByPosition[ObjectIdentifier(x)] { alias = ",\"aliasPort\":\(p)" }
            return "{\"t\":\"kv\",\"o\":\(obj(x)),\"v\":\(vec(x))\(alias)}"
        case let x as KVectorChain:
            return "{\"t\":\"kvc\",\"o\":\(obj(x)),\"v\":[\(x.elements.map(vec).joined(separator: ","))]}"
        case let x as ElkMargin: return "{\"t\":\"margin\",\"o\":\(obj(x)),\"v\":\(spacing(x))}"
        case let x as ElkPadding: return "{\"t\":\"padding\",\"o\":\(obj(x)),\"v\":\(spacing(x))}"
        case let x as LNode: return "{\"t\":\"node\",\"v\":\(node(x))}"
        case let x as LPort: return "{\"t\":\"port\",\"v\":\(port(x))}"
        case let x as LEdge: return "{\"t\":\"edge\",\"v\":\(edge(x))}"
        case let x as LLabel: return "{\"t\":\"label\",\"v\":\(label(x))}"
        case let x as Layer: return "{\"t\":\"layer\",\"v\":\(layer(x))}"
        case let x as LGraph: return "{\"t\":\"graph\",\"v\":\(graph(x))}"
        case let xs as [LNode]: return "{\"t\":\"nodes\",\"v\":[\(xs.map { String(node($0)) }.joined(separator: ","))]}"
        case let xs as [LPort]: return "{\"t\":\"ports\",\"v\":[\(xs.map { String(port($0)) }.joined(separator: ","))]}"
        case let xs as [LEdge]: return "{\"t\":\"edges\",\"v\":[\(xs.map { String(edge($0)) }.joined(separator: ","))]}"
        case let xs as [LLabel]: return "{\"t\":\"labels\",\"v\":[\(xs.map { String(label($0)) }.joined(separator: ","))]}"
        case let xs as Set<org_eclipse_elk_alg_layered_options_GraphProperties>:
            return "{\"t\":\"gprops\",\"v\":[\(xs.map { esc("\($0)") }.sorted().joined(separator: ","))]}"
        default:
            let typeName = String(describing: type(of: v))
            let m = Mirror(reflecting: v)
            if m.displayStyle == .enum {
                return "{\"t\":\"enum\",\"type\":\(esc(typeName)),\"v\":\(esc("\(v)"))}"
            }
            if let rr = v as? any RawRepresentable, let raw = rr.rawValue as? Int {
                return "{\"t\":\"raw\",\"type\":\(esc(typeName)),\"v\":\(raw)}"
            }
            if m.displayStyle == .class {
                return "{\"t\":\"obj\",\"type\":\(esc(typeName)),\"o\":\(obj(v as AnyObject))}"
            }
            return "{\"t\":\"opaque\",\"type\":\(esc(typeName))}"
        }
    }

    func props(_ p: [String: Any], _ owner: LNode? = nil) -> String {
        "{" + p.keys.sorted().map { "\(esc($0)):\(value($0, p[$0]!, owner))" }.joined(separator: ",") + "}"
    }

    func ref<T: AnyObject>(_ x: T?, _ map: [ObjectIdentifier: Int]) -> String {
        guard let x else { return "null" }
        return map[ObjectIdentifier(x)].map(String.init) ?? "null"
    }

    func json() -> String {
        var out = "{\n\"graphs\":[\n"
        out += graphList.enumerated().map { (i, x) in
            let full = i == 0
            var s = "{\"id\":\(x.id),\"size\":\(vec(x.size)),\"padding\":\(spacing(x.padding)),\"offset\":\(vec(x.offset)),\"parentNode\":\(ref(x.parentNode, nodes))"
            if full {
                s += ",\"layers\":[\(x.layers.map { String(layers[ObjectIdentifier($0)]!) }.joined(separator: ","))]"
                s += ",\"layerless\":[\(x.layerlessNodes.map { String(nodes[ObjectIdentifier($0)]!) }.joined(separator: ","))]"
                s += ",\"props\":\(props(x.getAllProperties()))"
            }
            return s + "}"
        }.joined(separator: ",\n")
        out += "\n],\n\"layers\":[\n"
        out += layerList.map { x in
            "{\"id\":\(x.id),\"owner\":\(graphs[ObjectIdentifier(x.owner)]!),\"size\":\(vec(x.size)),\"nodes\":[\(x.nodes.map { String(nodes[ObjectIdentifier($0)]!) }.joined(separator: ","))],\"props\":\(props(x.getAllProperties()))}"
        }.joined(separator: ",\n")
        out += "\n],\n\"nodes\":[\n"
        out += nodeList.map { x in
            var s = "{\"id\":\(x.id),\"type\":\(esc(x.type.rawValue)),\"pos\":\(vec(x.position)),\"size\":\(vec(x.size))"
            s += ",\"margin\":\(spacing(x.margin)),\"padding\":\(spacing(x.padding))"
            s += ",\"graph\":\(ref(x.graph, graphs)),\"layer\":\(ref(x.layer, layers)),\"nested\":\(ref(x.nestedGraph, graphs))"
            s += ",\"ports\":[\(x.ports.map { String(ports[ObjectIdentifier($0)]!) }.joined(separator: ","))]"
            s += ",\"labels\":[\(x.labels.map { String(labels[ObjectIdentifier($0)]!) }.joined(separator: ","))]"
            s += ",\"props\":\(props(x.getAllProperties(), x))}"
            return s
        }.joined(separator: ",\n")
        out += "\n],\n\"ports\":[\n"
        out += portList.map { x in
            var s = "{\"id\":\(x.id),\"side\":\(esc("\(x.side)")),\"pos\":\(vec(x.position)),\"size\":\(vec(x.size)),\"anchor\":\(vec(x.anchor))"
            s += ",\"explicitAnchor\":\(x.explicitlySuppliedPortAnchor),\"ext\":\(x.connectedToExternalNodes),\"margin\":\(spacing(x.margin))"
            s += ",\"owner\":\(ref(x.owner, nodes))"
            s += ",\"labels\":[\(x.labels.map { String(labels[ObjectIdentifier($0)]!) }.joined(separator: ","))]"
            s += ",\"in\":[\(x.incomingEdges.map { String(edges[ObjectIdentifier($0)]!) }.joined(separator: ","))]"
            s += ",\"out\":[\(x.outgoingEdges.map { String(edges[ObjectIdentifier($0)]!) }.joined(separator: ","))]"
            s += ",\"props\":\(props(x.getAllProperties()))}"
            return s
        }.joined(separator: ",\n")
        out += "\n],\n\"edges\":[\n"
        out += edgeList.map { x in
            var s = "{\"id\":\(x.id),\"source\":\(ref(x.source, ports)),\"target\":\(ref(x.target, ports))"
            s += ",\"bends\":[\(x.bendPoints.elements.map(vec).joined(separator: ","))]"
            s += ",\"labels\":[\(x.labels.map { String(labels[ObjectIdentifier($0)]!) }.joined(separator: ","))]"
            s += ",\"props\":\(props(x.getAllProperties()))}"
            return s
        }.joined(separator: ",\n")
        out += "\n],\n\"labels\":[\n"
        out += labelList.map { x in
            "{\"id\":\(x.id),\"text\":\(esc(x.text)),\"pos\":\(vec(x.position)),\"size\":\(vec(x.size)),\"props\":\(props(x.getAllProperties()))}"
        }.joined(separator: ",\n")
        out += "\n]\n}\n"
        return out
    }
}
