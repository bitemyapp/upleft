// Group B test tooling (not part of the port): the elk-swift instrumentation
// that produced the dumps in this directory, for `tests/crossmin_differential.rs`.
//
// To regenerate or extend the dumps: copy the elk-swift lab
// (`crates/elk/tools/elklab.sh` builds it into target/elklab) to a scratch
// directory, put this file next to
// `Sources/ElkSwift/ELK/org/eclipse/elk/alg/layered/p3order/org_eclipse_elk_alg_layered_p3order_LayerSweepCrossingMinimizer.swift`,
// and add these three lines right after `progressMonitor.begin(...)` at the
// top of `LayerSweepCrossingMinimizer.process`:
//
//     let _cmFile = CMDump.nextFile()
//     CMDump.write(layeredGraph, _cmFile, "pre", "\(crossMinType)")
//     defer { CMDump.write(layeredGraph, _cmFile, "post", "\(crossMinType)") }
//
// then `swift build -c release` and run `ELKLAB_CMDUMP_DIR=<dir> lab <graph.json>`.
// Every processor run writes cm-NNNN-pre.json / cm-NNNN-post.json. Point
// UPLEFT_ELK_CMDUMP_DIR at a tree of such directories to check them all.
// The checked-in dumps had properties of untyped ("Other") values stripped.
// Group B instrumentation: dumps the layered graph(s) a LayerSweepCrossingMinimizer
// sees before and after it runs, for differential testing of the Rust port.
import Foundation

package final class CMDump {
    static var counter = 0
    static var oids: [ObjectIdentifier: Int] = [:]
    static var keep: [AnyObject] = []
    static func oid(_ o: AnyObject) -> Int {
        let k = ObjectIdentifier(o)
        if let v = oids[k] { return v }
        let v = oids.count
        oids[k] = v
        keep.append(o) // keep alive so identifiers are never reused
        return v
    }
    var graphs: [LGraph] = []
    var graphIdx: [ObjectIdentifier: Int] = [:]
    var nodes: [LNode] = []
    var nodeIdx: [ObjectIdentifier: Int] = [:]
    var ports: [LPort] = []
    var portIdx: [ObjectIdentifier: Int] = [:]
    var edges: [LEdge] = []
    var edgeIdx: [ObjectIdentifier: Int] = [:]
    var randoms: [Random] = []
    var randomIdx: [ObjectIdentifier: Int] = [:]

    static var dir: String? { ProcessInfo.processInfo.environment["ELKLAB_CMDUMP_DIR"] }

    package static func nextFile() -> Int { counter += 1; return counter }

    func addGraph(_ g: LGraph) {
        let k = ObjectIdentifier(g)
        if graphIdx[k] != nil { return }
        graphIdx[k] = graphs.count
        graphs.append(g)
    }
    func addNode(_ n: LNode) {
        let k = ObjectIdentifier(n)
        if nodeIdx[k] != nil { return }
        nodeIdx[k] = nodes.count
        nodes.append(n)
    }
    func addPort(_ p: LPort) {
        let k = ObjectIdentifier(p)
        if portIdx[k] != nil { return }
        portIdx[k] = ports.count
        ports.append(p)
    }
    func addEdge(_ e: LEdge) {
        let k = ObjectIdentifier(e)
        if edgeIdx[k] != nil { return }
        edgeIdx[k] = edges.count
        edges.append(e)
    }

    init(_ root: LGraph) {
        var queue = [root]
        while !queue.isEmpty {
            let g = queue.removeFirst()
            if graphIdx[ObjectIdentifier(g)] != nil { continue }
            addGraph(g)
            for l in g.layers { for n in l.nodes { addNode(n); if let ng = n.nestedGraph { queue.append(ng) } } }
            for n in g.layerlessNodes { addNode(n); if let ng = n.nestedGraph { queue.append(ng) } }
        }
        // Close over ports, edges and referenced elements.
        var changed = true
        while changed {
            changed = false
            let (gc, nc, pc, ec) = (graphs.count, nodes.count, ports.count, edges.count)
            for n in nodes { for p in n.ports { addPort(p) }; if let ng = n.nestedGraph { addGraph(ng) }; if let g = n.graph { addGraph(g) } }
            for p in ports { for e in p.incomingEdges + p.outgoingEdges { addEdge(e) }; if let o = p.owner { addNode(o) } }
            for e in edges { if let s = e.source { addPort(s) }; if let t = e.target { addPort(t) } }
            for g in graphs { for l in g.layers { for n in l.nodes { addNode(n) } }; if let pn = g.parentNode { addNode(pn) } }
            for h in (graphs as [MapPropertyHolder]) + (nodes as [MapPropertyHolder]) + (ports as [MapPropertyHolder]) {
                for (_, v) in h.propertyMap ?? [:] { refs(v) }
            }
            if gc != graphs.count || nc != nodes.count || pc != ports.count || ec != edges.count { changed = true }
        }
    }

    func refs(_ v: Any) {
        switch v {
        case let n as LNode: addNode(n)
        case let p as LPort: addPort(p)
        case let ns as [LNode]: ns.forEach(addNode)
        case let ps as [LPort]: ps.forEach(addPort)
        default: break
        }
    }

    func value(_ v: Any) -> [String: Any] {
        switch v {
        case let b as Bool: return ["t": "Bool", "v": b]
        case let i as Int: return ["t": "Int", "v": i]
        case let d as Double: return ["t": "Double", "v": d.isFinite ? d : 0, "bits": String(d.bitPattern)]
        case let s as String: return ["t": "Str", "v": s]
        case let n as LNode: return ["t": "LNode", "v": CMDump.oid(n)]
        case let p as LPort: return ["t": "LPort", "v": CMDump.oid(p)]
        case let ns as [LNode]: return ["t": "LNodes", "v": ns.map { CMDump.oid($0) }]
        case let ps as [LPort]: return ["t": "LPorts", "v": ps.map { CMDump.oid($0) }]
        case let x as PortConstraints: return ["t": "PortConstraints", "v": "\(x)"]
        case let x as HierarchyHandling: return ["t": "HierarchyHandling", "v": "\(x)"]
        case let x as OrderingStrategy: return ["t": "OrderingStrategy", "v": "\(x)"]
        case let x as GroupOrderStrategy: return ["t": "GroupOrderStrategy", "v": "\(x)"]
        case let x as LayerConstraint: return ["t": "LayerConstraint", "v": "\(x)"]
        case let x as PortSide: return ["t": "PortSide", "v": x.rawValue]
        case let x as Set<GraphProperties>: return ["t": "GraphProperties", "v": x.map { "\($0)" }.sorted()]
        case let r as Random:
            let k = ObjectIdentifier(r)
            if randomIdx[k] == nil { randomIdx[k] = randoms.count; randoms.append(r) }
            return ["t": "Random", "v": CMDump.oid(r), "seed": String(r.seed)]
        default: return ["t": "Other", "v": String(describing: type(of: v))]
        }
    }

    func props(_ h: MapPropertyHolder) -> [String: Any] {
        var out: [String: Any] = [:]
        for (k, v) in h.propertyMap ?? [:] { out[k] = value(v) }
        return out
    }

    func json() -> [String: Any] {
        var gs: [[String: Any]] = []
        for g in graphs {
            gs.append([
                "oid": CMDump.oid(g),
                "id": g.id,
                "parent": g.parentNode.map { CMDump.oid($0) } as Any? ?? NSNull(),
                "layers": g.layers.map { l in ["oid": CMDump.oid(l), "id": l.id, "nodes": l.nodes.map { CMDump.oid($0) }] as [String: Any] },
                "layerless": g.layerlessNodes.map { CMDump.oid($0) },
                "props": props(g),
            ])
        }
        var ns: [[String: Any]] = []
        for n in nodes {
            var idx: [String: Any] = [:]
            if let psi = n.portSideIndices { for (s, r) in psi { idx[s.rawValue] = [r.0, r.1] } }
            ns.append([
                "oid": CMDump.oid(n),
                "id": n.id,
                "type": n.type.rawValue,
                "graph": n.graph.map { CMDump.oid($0) } as Any? ?? NSNull(),
                "layerOf": n.layer.map { CMDump.oid($0) } as Any? ?? NSNull(),
                "nested": n.nestedGraph.map { CMDump.oid($0) } as Any? ?? NSNull(),
                "ports": n.ports.map { CMDump.oid($0) },
                "cached": n.portSidesCached,
                "indices": n.portSideIndices == nil ? NSNull() : idx,
                "label": n.labels.first?.text ?? "",
                "props": props(n),
            ])
        }
        var ps: [[String: Any]] = []
        for p in ports {
            ps.append([
                "oid": CMDump.oid(p),
                "id": p.id,
                "side": p.side.rawValue,
                "owner": p.owner.map { CMDump.oid($0) } as Any? ?? NSNull(),
                "anchor": [String(p.anchor.x.bitPattern), String(p.anchor.y.bitPattern)],
                "size": [String(p.size.x.bitPattern), String(p.size.y.bitPattern)],
                "pos": [String(p.position.x.bitPattern), String(p.position.y.bitPattern)],
                "explicit": p.explicitlySuppliedPortAnchor,
                "in": p.incomingEdges.map { CMDump.oid($0) },
                "out": p.outgoingEdges.map { CMDump.oid($0) },
                "props": props(p),
            ])
        }
        var es: [[String: Any]] = []
        for e in edges {
            es.append([
                "oid": CMDump.oid(e),
                "id": e.id,
                "source": e.source.map { CMDump.oid($0) } as Any? ?? NSNull(),
                "target": e.target.map { CMDump.oid($0) } as Any? ?? NSNull(),
            ])
        }
        return ["graphs": gs, "nodes": ns, "ports": ps, "edges": es, "randoms": randoms.map { ["oid": CMDump.oid($0), "seed": String($0.seed)] as [String: Any] }]
    }

    package static func write(_ root: LGraph, _ file: Int, _ phase: String, _ type: String) {
        guard let dir = dir else { return }
        let d = CMDump(root)
        var j = d.json()
        j["type"] = type
        j["phase"] = phase
        let data = try! JSONSerialization.data(withJSONObject: j, options: [.sortedKeys])
        let path = "\(dir)/cm-\(String(format: "%04d", file))-\(phase).json"
        try! data.write(to: URL(fileURLWithPath: path))
    }
}
