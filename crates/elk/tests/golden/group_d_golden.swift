// Generator for tests/golden/group_d.txt (upleft-elk group D golden tests).
//
// Build: copy the elk-swift lab (tools/elklab.sh output, target/elklab) to a
// scratch directory, add `.executableTarget(name: "golden", dependencies:
// ["ElkSwift"])` to its Package.swift, put this file at Sources/golden/main.swift,
// add `package init() {}` to CompoundGraphPreprocessor (its implicit init is
// internal), then `swift build -c release --product golden` and run
// `.build/release/golden > tests/golden/group_d.txt`.

import Foundation
import ElkSwift

// Golden data for upleft-elk group D unit tests. Every scenario builds a
// small layered graph by hand, runs processors, and prints a canonical dump
// that the Rust tests reproduce byte for byte.

func d(_ x: Double) -> String { "\(x)" }
func v(_ k: KVector) -> String { "\(d(k.x)),\(d(k.y))" }
func sp(_ s: Spacing) -> String { "\(d(s.top)),\(d(s.right)),\(d(s.bottom)),\(d(s.left))" }
func alignName(_ a: Alignment?) -> String {
    guard let a = a else { return "-" }
    switch a {
    case .automatic: return "AUTOMATIC"
    case .left: return "LEFT"
    case .right: return "RIGHT"
    case .top: return "TOP"
    case .bottom: return "BOTTOM"
    case .center: return "CENTER"
    }
}
func opt<T>(_ x: T?) -> String { x.map { "\($0)" } ?? "-" }

func allNodes(_ g: LGraph) -> [LNode] {
    var nodes = g.layerlessNodes
    for l in g.layers { nodes.append(contentsOf: l.nodes) }
    return nodes
}

func portRef(_ p: LPort?, _ nodes: [LNode]) -> String {
    guard let p = p, let o = p.owner, let ni = nodes.firstIndex(where: { $0 === o }),
          let pi = o.ports.firstIndex(where: { $0 === p }) else { return "?" }
    return "N\(ni)P\(pi)"
}

func dump(_ g: LGraph) -> String {
    var s = ""
    var nlp = "-"
    if let p = g.getProperty(LayeredOptions.NODE_LABELS_PADDING) as? ElkPadding { nlp = sp(p) }
    s += "graph size=\(v(g.size)) offset=\(v(g.offset)) padding=\(sp(g.padding)) nlp=\(nlp) elss=\(opt(g.getProperty(LayeredOptions.EDGE_LABELS_SIDE_SELECTION) as? EdgeLabelSideSelection))\n"
    let nodes = allNodes(g)
    for (i, n) in nodes.enumerated() {
        let layer = n.layer.map { l in g.layers.firstIndex(where: { $0 === l }).map { "\($0)" } ?? "x" } ?? "-"
        let pos = (n.getProperty(LayeredOptions.POSITION) as? KVector).map(v) ?? "-"
        let min = (n.getProperty(LayeredOptions.NODE_SIZE_MINIMUM) as? KVector).map(v) ?? "-"
        let nlplace = (n.getProperty(LayeredOptions.NODE_LABELS_PLACEMENT) as? NodeLabelPlacement).map { "\($0.rawValue)" } ?? "-"
        s += "N\(i) \(n.type.rawValue) pos=\(v(n.position)) size=\(v(n.size)) margin=\(sp(n.margin)) padding=\(sp(n.padding)) layer=\(layer)"
        s += " align=\(alignName(n.getProperty(LayeredOptions.ALIGNMENT) as? Alignment)) nlplace=\(nlplace) position=\(pos) min=\(min)"
        s += " lc=\(opt(n.getProperty(LayeredOptions.LAYERING_LAYER_CONSTRAINT) as? LayerConstraint)) ilc=\(opt(n.getProperty(InternalProperties.IN_LAYER_CONSTRAINT) as? InLayerConstraint)) eps=\(opt(n.getProperty(InternalProperties.EXT_PORT_SIDE) as? PortSide))"
        s += " pc=\(opt(n.getProperty(LayeredOptions.PORT_CONSTRAINTS) as? PortConstraints))\n"
        for (j, p) in n.ports.enumerated() {
            s += " P\(j) side=\(p.side) pos=\(v(p.position)) anchor=\(v(p.anchor)) size=\(v(p.size)) idx=\(opt(p.getProperty(LayeredOptions.PORT_INDEX) as? Int))\n"
            for l in p.labels { s += "  PL pos=\(v(l.position)) size=\(v(l.size))\n" }
            for e in p.outgoingEdges {
                let jps = (e.getProperty(LayeredOptions.JUNCTION_POINTS) as? KVectorChain).map { "[" + $0.elements.map(v).joined(separator: ";") + "]" } ?? "-"
                let toff = (e.getProperty(InternalProperties.TARGET_OFFSET) as? KVector).map(v) ?? "-"
                s += "  E -> \(portRef(e.target, nodes)) bends=[\(e.bendPoints.elements.map(v).joined(separator: ";"))] jps=\(jps) toff=\(toff)\n"
                for l in e.labels {
                    s += "   EL pos=\(v(l.position)) size=\(v(l.size)) inline=\(opt(l.getProperty(LayeredOptions.EDGE_LABELS_INLINE) as? Bool))\n"
                }
            }
        }
        for l in n.labels {
            let lp = (l.getProperty(LayeredOptions.NODE_LABELS_PLACEMENT) as? NodeLabelPlacement).map { "\($0.rawValue)" } ?? "-"
            s += " NL pos=\(v(l.position)) size=\(v(l.size)) nlplace=\(lp)\n"
        }
        if let nested = n.nestedGraph {
            s += " nested {\n" + dump(nested) + " }\n"
        }
    }
    return s
}

func node(_ g: LGraph, _ x: Double, _ y: Double, _ w: Double, _ h: Double) -> LNode {
    let n = LNode(g)
    n.position.x = x; n.position.y = y
    n.size.x = w; n.size.y = h
    g.layerlessNodes.append(n)
    return n
}

func port(_ n: LNode, _ side: PortSide, _ x: Double, _ y: Double, _ w: Double, _ h: Double) -> LPort {
    let p = LPort()
    p.setNode(n)
    p.size.x = w; p.size.y = h
    p.setSide(side)
    p.position.x = x; p.position.y = y
    return p
}

func edge(_ s: LPort, _ t: LPort) -> LEdge {
    let e = LEdge()
    e.setSource(s)
    e.setTarget(t)
    return e
}

func label(_ w: Double, _ h: Double, _ x: Double = 0, _ y: Double = 0) -> LLabel {
    let l = LLabel("l")
    l.size.x = w; l.size.y = h
    l.position.x = x; l.position.y = y
    return l
}

setvbuf(stdout, nil, _IONBF, 0)
let monitor = BasicProgressMonitor()

// MARK: - GraphTransformer

func transformerGraph(_ dir: Direction, _ congruency: DirectionCongruency?) -> LGraph {
    let g = LGraph()
    g.setProperty(LayeredOptions.DIRECTION, dir)
    if let c = congruency { g.setProperty(LayeredOptions.DIRECTION_CONGRUENCY, c) }
    g.size.x = 300; g.size.y = 200
    g.offset.x = 3; g.offset.y = 7
    g.padding.top = 1; g.padding.right = 2; g.padding.bottom = 3; g.padding.left = 4
    g.setProperty(LayeredOptions.NODE_LABELS_PADDING, ElkPadding(5, 6, 7, 8))
    g.setProperty(LayeredOptions.EDGE_LABELS_SIDE_SELECTION, EdgeLabelSideSelection.SMART_UP)

    let a = node(g, 10, 20, 40, 30)
    a.margin.top = 1; a.margin.right = 2; a.margin.bottom = 3; a.margin.left = 4
    a.padding.top = 5; a.padding.right = 6; a.padding.bottom = 7; a.padding.left = 8
    a.setProperty(LayeredOptions.ALIGNMENT, Alignment.left)
    a.setProperty(LayeredOptions.NODE_LABELS_PLACEMENT, NodeLabelPlacement([.inside, .hLeft, .vTop]))
    a.setProperty(LayeredOptions.POSITION, KVector(11, 22))
    a.setProperty(LayeredOptions.NODE_SIZE_MINIMUM, KVector(15, 25))
    let al = label(12, 6, 1, 2)
    al.setProperty(LayeredOptions.NODE_LABELS_PLACEMENT, NodeLabelPlacement([.outside, .hRight, .vBottom, .hPriority]))
    a.labels.append(al)

    let b = node(g, 120, 50, 30, 60)
    b.setProperty(LayeredOptions.ALIGNMENT, Alignment.bottom)
    let c = node(g, 200, 5, 20, 20)
    c.setProperty(LayeredOptions.ALIGNMENT, Alignment.top)

    let ext = node(g, 0, 90, 10, 10)
    ext.type = .externalPort
    ext.setProperty(InternalProperties.EXT_PORT_SIDE, PortSide.WEST)
    ext.setProperty(LayeredOptions.LAYERING_LAYER_CONSTRAINT, LayerConstraint.FIRST_SEPARATE)

    let ap = port(a, .EAST, 40, 12, 4, 6)
    ap.setProperty(LayeredOptions.PORT_INDEX, 3)
    ap.labels.append(label(5, 3, 2, -4))
    let an = port(a, .NORTH, 10, -2, 6, 2)
    an.explicitlySuppliedPortAnchor = true
    an.anchor.x = 1.5; an.anchor.y = 0.25
    let bw = port(b, .WEST, -4, 20, 4, 8)
    let bs = port(b, .SOUTH, 13, 60, 4, 4)
    let cw = port(c, .WEST, -2, 8, 2, 4)
    let ep = port(ext, .EAST, 10, 5, 0, 0)

    let e1 = edge(ap, bw)
    e1.bendPoints.add(KVector(80, 35))
    e1.bendPoints.add(KVector(80, 74))
    e1.setProperty(LayeredOptions.JUNCTION_POINTS, KVectorChain(KVector(80, 50)))
    let el = label(14, 8, 60, 40)
    el.setProperty(LayeredOptions.EDGE_LABELS_INLINE, true)
    e1.labels.append(el)
    let e2 = edge(bs, cw)
    e2.bendPoints.add(KVector(135, 150))
    e2.bendPoints.add(KVector(170, 150))
    e2.bendPoints.add(KVector(170, 15))
    _ = edge(ep, an)
    _ = edge(an, cw)
    return g
}

func testTransformer() {
    let dirs: [Direction] = [.RIGHT, .LEFT, .DOWN, .UP, .UNDEFINED]
    for congruency in [DirectionCongruency?.none, .READING_DIRECTION, .ROTATION] {
        for dir in dirs {
            for mode in [Mode.TO_INTERNAL_LTR, Mode.TO_INPUT_DIRECTION] {
                let g = transformerGraph(dir, congruency)
                GraphTransformer(mode).process(g, monitor)
                print("== transformer \(opt(congruency)) \(dir) \(mode)")
                print(dump(g), terminator: "")
            }
            // round trip, with a size change in between
            let g = transformerGraph(dir, congruency)
            GraphTransformer(.TO_INPUT_DIRECTION).process(g, monitor)
            g.size.x += 17; g.size.y += 9
            GraphTransformer(.TO_INTERNAL_LTR).process(g, monitor)
            print("== transformer roundtrip \(opt(congruency)) \(dir)")
            print(dump(g), terminator: "")
        }
    }
    // zero-sized graph: offsets come from the nodes
    for dir in [Direction.LEFT, .UP] {
        let g = transformerGraph(dir, nil)
        g.size.x = 0; g.size.y = 0
        GraphTransformer(.TO_INPUT_DIRECTION).process(g, monitor)
        print("== transformer zero-size \(dir)")
        print(dump(g), terminator: "")
    }
}


// MARK: - Self loops

func placePorts(_ n: LNode) {
    for (i, p) in n.ports.enumerated() {
        let k = Double(i)
        switch p.side {
        case .NORTH: p.position.x = 5 + 3 * k; p.position.y = 0
        case .SOUTH: p.position.x = 5 + 3 * k; p.position.y = n.size.y
        case .EAST: p.position.x = n.size.x; p.position.y = 2 + 2 * k
        case .WEST: p.position.x = 0; p.position.y = 2 + 2 * k
        default: break
        }
    }
}

func selfLoopGraph(_ fixedSides: Bool, _ dir: Direction) -> (LGraph, LNode) {
    let g = LGraph()
    g.setProperty(LayeredOptions.DIRECTION, dir)
    let n = node(g, 0, 0, 60, 40)
    let m = node(g, 200, 0, 20, 20)
    func sd(_ s: PortSide) -> PortSide { fixedSides ? s : .UNDEFINED }
    let pE = port(n, .EAST, 60, 20, 0, 0)
    let mW = port(m, .WEST, 0, 10, 0, 0)
    _ = edge(pE, mW)
    let p1 = port(n, sd(.NORTH), 0, 0, 0, 0)
    let p2 = port(n, sd(.NORTH), 0, 0, 0, 0)
    let p3 = port(n, sd(.EAST), 0, 0, 0, 0)
    let p4 = port(n, sd(.SOUTH), 0, 0, 0, 0)
    let p5 = port(n, sd(.WEST), 0, 0, 0, 0)
    let p6 = port(n, sd(.EAST), 0, 0, 0, 0)
    let p7 = port(n, sd(.NORTH), 0, 0, 0, 0)
    let p8 = port(n, sd(.EAST), 0, 0, 0, 0)
    let p9 = port(n, sd(.SOUTH), 0, 0, 0, 0)
    let p10 = port(n, sd(.NORTH), 0, 0, 0, 0)
    let p11 = port(n, sd(.EAST), 0, 0, 0, 0)
    let p12 = port(n, sd(.SOUTH), 0, 0, 0, 0)
    let p13 = port(n, sd(.WEST), 0, 0, 0, 0)
    let p14 = port(n, sd(.SOUTH), 0, 0, 0, 0)
    let p16 = port(n, sd(.WEST), 0, 0, 0, 0)
    _ = edge(p1, p2)
    _ = edge(p3, p4)
    let e3 = edge(p5, p6)
    e3.labels.append(label(20, 8))
    _ = edge(p7, p8)
    let e4 = edge(p8, p9)
    let l4 = label(16, 6)
    l4.setProperty(LayeredOptions.EDGE_LABELS_INLINE, true)
    e4.labels.append(l4)
    _ = edge(p10, p11)
    _ = edge(p11, p12)
    let e5 = edge(p12, p13)
    e5.labels.append(label(10, 10))
    e5.labels.append(label(4, 3))
    let e6 = edge(p14, p14)
    e6.labels.append(label(12, 4))
    _ = edge(pE, p16)
    return (g, n)
}

func testSelfLoops() {
    let cases: [(String, String, SelfLoopDistributionStrategy?, SelfLoopOrderingStrategy?, Direction, EdgeRouting?)] = [
        ("free", "FREE", .NORTH, .STACKED, .RIGHT, nil),
        ("free", "FREE", .NORTH_SOUTH, .SEQUENCED, .RIGHT, .ORTHOGONAL),
        ("free", "UNDEFINED", .EQUALLY, .REVERSE_STACKED, .RIGHT, nil),
        ("free", "FREE", .EQUALLY, .STACKED, .DOWN, .POLYLINE),
        ("free", "FREE", nil, .SEQUENCED, .UP, nil),
        ("fixedSide", "FIXED_SIDE", nil, .STACKED, .RIGHT, nil),
        ("fixedSide", "FIXED_SIDE", nil, .SEQUENCED, .DOWN, nil),
        ("fixedSide", "FIXED_SIDE", nil, .REVERSE_STACKED, .LEFT, .POLYLINE),
        ("fixedOrder", "FIXED_ORDER", nil, .STACKED, .RIGHT, nil),
    ]
    for (kind, opc, dist, ordering, dir, routing) in cases {
        let (g, n) = selfLoopGraph(kind != "free", dir)
        if kind == "fixedOrder" { n.setProperty(LayeredOptions.PORT_CONSTRAINTS, PortConstraints.FIXED_ORDER) }
        if let dist = dist { n.setProperty(LayeredOptions.EDGE_ROUTING_SELF_LOOP_DISTRIBUTION, dist) }
        if let ordering = ordering { n.setProperty(LayeredOptions.EDGE_ROUTING_SELF_LOOP_ORDERING, ordering) }
        if let routing = routing { g.setProperty(LayeredOptions.EDGE_ROUTING, routing) }
        let pc: PortConstraints
        switch opc {
        case "FREE": pc = .FREE
        case "FIXED_SIDE": pc = .FIXED_SIDE
        case "FIXED_ORDER": pc = .FIXED_ORDER
        default: pc = .UNDEFINED
        }
        n.setProperty(InternalProperties.ORIGINAL_PORT_CONSTRAINTS, pc)
        let header = "selfloops \(kind) \(opc) \(opt(dist)) \(opt(ordering)) \(dir) \(opt(routing))"

        SelfLoopPreProcessor().process(g, monitor)
        print("== \(header) pre")
        print(dump(g), terminator: "")

        let layer = Layer(g)
        g.layers.append(layer)
        for x in g.layerlessNodes { x.setLayer(layer) }
        g.layerlessNodes.removeAll()

        SelfLoopPortRestorer().process(g, monitor)
        placePorts(n)
        print("== \(header) restored")
        print(dump(g), terminator: "")

        n.position.x = 100; n.position.y = 50
        SelfLoopRouter().process(g, monitor)
        SelfLoopPostProcessor().process(g, monitor)
        print("== \(header) routed")
        print(dump(g), terminator: "")
    }
}


// MARK: - Label dummy switcher

func chainNode(_ g: LGraph, _ layer: Layer, _ type: NodeType, _ w: Double, _ h: Double) -> LNode {
    let n = LNode(g)
    n.type = type
    n.size.x = w; n.size.y = h
    n.setLayer(layer)
    let i = LPort(); i.setNode(n); i.setSide(.WEST)
    let o = LPort(); o.setNode(n); o.setSide(.EAST)
    return n
}

func chain(_ g: LGraph, _ nodes: [LNode], _ reversed: Bool) {
    for k in 0..<(nodes.count - 1) {
        let e = LEdge()
        e.setSource(nodes[k].ports[1])
        e.setTarget(nodes[k + 1].ports[0])
        if reversed { e.setProperty(InternalProperties.REVERSED, true) }
    }
}

func switcherGraph(_ strategy: CenterEdgeLabelPlacementStrategy?, _ override: CenterEdgeLabelPlacementStrategy?, _ trivial: Bool) -> (LGraph, [LLabel]) {
    let g = LGraph()
    g.setProperty(LayeredOptions.DIRECTION, Direction.RIGHT)
    if let s = strategy { g.setProperty(LayeredOptions.EDGE_LABELS_CENTER_LABEL_PLACEMENT_STRATEGY, s) }
    var layers = [Layer]()
    let widths: [Double] = [30, 50, 10, 25, 70, 20, 30]
    for k in 0..<7 {
        let l = Layer(g)
        l.size.x = widths[k] + 5
        g.layers.append(l)
        layers.append(l)
    }
    // filler nodes that give the layers their widths
    for k in 0..<7 { _ = chainNode(g, layers[k], .normal, widths[k], 10) }
    var labels = [LLabel]()
    func labelDummy(_ layer: Layer, _ w: Double) -> LNode {
        let n = chainNode(g, layer, .label, w, 12)
        let l = label(w, 12)
        if let o = override { l.setProperty(LayeredOptions.EDGE_LABELS_CENTER_LABEL_PLACEMENT_STRATEGY, o) }
        n.setProperty(InternalProperties.REPRESENTED_LABELS, [l])
        labels.append(l)
        return n
    }
    // chain 1: A d d L d d B
    let c1 = [chainNode(g, layers[0], .normal, 30, 20), chainNode(g, layers[1], .longEdge, 0, 0), chainNode(g, layers[2], .longEdge, 0, 0),
              labelDummy(layers[3], 40), chainNode(g, layers[4], .longEdge, 0, 0), chainNode(g, layers[5], .longEdge, 0, 0),
              chainNode(g, layers[6], .normal, 30, 20)]
    chain(g, c1, false)
    // chain 2: A L B (trivial)
    if trivial {
        let c2 = [chainNode(g, layers[2], .normal, 10, 10), labelDummy(layers[3], 8), chainNode(g, layers[4], .normal, 10, 10)]
        chain(g, c2, false)
    }
    // chain 3 (reversed): A d L d B
    let c3 = [chainNode(g, layers[1], .normal, 10, 10), chainNode(g, layers[2], .longEdge, 0, 0), labelDummy(layers[3], 90),
              chainNode(g, layers[4], .longEdge, 0, 0), chainNode(g, layers[5], .normal, 10, 10)]
    chain(g, c3, true)
    // chain 4: A L d d d B
    let c4 = [chainNode(g, layers[0], .normal, 10, 10), labelDummy(layers[1], 60), chainNode(g, layers[2], .longEdge, 0, 0),
              chainNode(g, layers[3], .longEdge, 0, 0), chainNode(g, layers[4], .longEdge, 0, 0), chainNode(g, layers[5], .normal, 10, 10)]
    chain(g, c4, false)
    return (g, labels)
}

func switcherDump(_ g: LGraph, _ labels: [LLabel]) -> String {
    var s = ""
    for (li, l) in g.layers.enumerated() {
        s += "L\(li) id=\(l.id):"
        for n in l.nodes {
            let lebld = (n.getProperty(InternalProperties.LONG_EDGE_BEFORE_LABEL_DUMMY) as? Bool).map { $0 ? "b" : "n" } ?? ""
            let pred = n.getIncomingEdges().first?.source?.node
            let predRef: String
            if let p = pred, let pl = p.layer, let pli = g.layers.firstIndex(where: { $0 === pl }), let pi = pl.nodes.firstIndex(where: { $0 === p }) {
                predRef = "\(pli).\(pi)"
            } else { predRef = "-" }
            s += " \(n.type.rawValue.prefix(2))\(lebld)<\(predRef) \(alignName(n.getProperty(LayeredOptions.ALIGNMENT) as? Alignment))"
        }
        s += "\n"
    }
    s += "labels:" + labels.map { " \(opt($0.getProperty(org_eclipse_elk_alg_layered_intermediate_LabelDummySwitcher.INCLUDE_LABEL) as? Bool))" }.joined() + "\n"
    return s
}

func testSwitcher() {
    let strategies: [CenterEdgeLabelPlacementStrategy?] = [nil, .MEDIAN_LAYER, .TAIL_LAYER, .HEAD_LAYER, .SPACE_EFFICIENT_LAYER, .WIDEST_LAYER, .CENTER_LAYER]
    for strategy in strategies {
        for override in [CenterEdgeLabelPlacementStrategy?.none, .HEAD_LAYER, .CENTER_LAYER, .WIDEST_LAYER] {
            // WIDEST_LAYER traps (`(l + 1)...r` with l == r) on a label dummy
            // between two normal nodes, so that chain is left out for it.
            let widest = strategy == .WIDEST_LAYER || override == .WIDEST_LAYER
            let (g, labels) = switcherGraph(strategy, override, !widest)
            LabelDummySwitcher().process(g, monitor)
            print("== switcher \(opt(strategy)) \(opt(override))")
            print(switcherDump(g, labels), terminator: "")
        }
    }
}

// MARK: - Hierarchical node resizing

func testResizer() {
    for (parentDir, childDir) in [(Direction.RIGHT, Direction.RIGHT), (.DOWN, .RIGHT), (.RIGHT, .UP), (.UP, .DOWN)] {
        for variant in 0..<3 {
            let g = LGraph()
            g.setProperty(LayeredOptions.DIRECTION, parentDir)
            g.setProperty(InternalProperties.GRAPH_PROPERTIES, Set<GraphProperties>([.HYPEREDGES]))
            let p = node(g, 5, 6, 10, 10)
            let pp = port(p, .EAST, 10, 5, 2, 3)
            let pl = label(4, 4, 11, 12)
            p.labels.append(pl)
            let c = LGraph()
            c.parentNode = p
            p.nestedGraph = c
            c.setProperty(LayeredOptions.DIRECTION, childDir)
            c.size.x = 80; c.size.y = 40
            c.padding.top = 2; c.padding.right = 3; c.padding.bottom = 4; c.padding.left = 5
            c.offset.x = 1; c.offset.y = 2
            let layer = Layer(c)
            c.layers.append(layer)
            let inner = LNode(c)
            inner.position.x = 10; inner.position.y = 12
            inner.size.x = 20; inner.size.y = 10
            inner.setLayer(layer)
            let ext = LNode(c)
            ext.type = .externalPort
            ext.size.x = 4; ext.size.y = 6
            ext.position.x = 90; ext.position.y = 17
            ext.setProperty(InternalProperties.ORIGIN, pp)
            ext.setProperty(InternalProperties.EXT_PORT_SIDE, PortSide.EAST)
            ext.setProperty(LayeredOptions.PORT_BORDER_OFFSET, 1.5)
            ext.setLayer(layer)
            if variant >= 1 {
                c.setProperty(InternalProperties.GRAPH_PROPERTIES, Set<GraphProperties>([.EXTERNAL_PORTS]))
            }
            if variant == 2 {
                c.setProperty(LayeredOptions.NODE_SIZE_CONSTRAINTS, SizeConstraint([.minimumSize]))
                c.setProperty(LayeredOptions.NODE_SIZE_OPTIONS, SizeOptions([.defaultMinimumSize]))
                c.setProperty(LayeredOptions.NODE_SIZE_MINIMUM, KVector(120, 0))
                c.setProperty(LayeredOptions.CONTENT_ALIGNMENT, ContentAlignment([.hCenter, .vBottom]))
            }
            HierarchicalNodeResizingProcessor().process(c, monitor)
            print("== resizer \(parentDir) \(childDir) \(variant)")
            let gp = (g.getProperty(InternalProperties.GRAPH_PROPERTIES) as? Set<GraphProperties>).map { $0.map { "\($0)" }.sorted().joined(separator: ",") } ?? "-"
            let cmin = (c.getProperty(LayeredOptions.NODE_SIZE_MINIMUM) as? KVector).map(v) ?? "-"
            print("parent props=\(gp) layers=\(c.layers.count) inner.layer=\(inner.layer == nil ? "nil" : "set") cmin=\(cmin)")
            print(dump(g), terminator: "")
        }
    }
}


// MARK: - Compound graphs

// Like `dump`, but lists every port's outgoing edges sorted by their text
// (the postprocessor re-adds original edges in dictionary order).
func sortedDump(_ g: LGraph) -> String {
    var out = [String]()
    var block = [String]()
    func flush() { out.append(contentsOf: block.sorted()); block = [] }
    for line in dump(g).split(separator: "\n", omittingEmptySubsequences: false) {
        if line.hasPrefix("  E ") || line.hasPrefix("   EL ") {
            if line.hasPrefix("  E ") { block.append(String(line)) } else { block[block.count - 1] += "\n" + line }
        } else {
            flush()
            out.append(String(line))
        }
    }
    flush()
    return out.joined(separator: "\n")
}

func graphName(_ g: LGraph, _ graphs: [(String, LGraph)]) -> String {
    graphs.first(where: { $0.1 === g })?.0 ?? "?"
}

func compoundScenario(_ merge: Bool, _ fixedSide: Bool, _ insideLoops: Bool, _ dir: Direction) {
    let r = LGraph()
    r.setProperty(LayeredOptions.DIRECTION, dir)
    let x = node(r, 300, 10, 20, 20)
    let y = node(r, 300, 80, 20, 20)
    let p = node(r, 50, 0, 100, 100)
    let q = node(r, 50, 150, 60, 40)
    if fixedSide { p.setProperty(LayeredOptions.PORT_CONSTRAINTS, PortConstraints.FIXED_SIDE) }
    let c = LGraph(); c.parentNode = p; p.nestedGraph = c
    let dd = LGraph(); dd.parentNode = q; q.nestedGraph = dd
    c.setProperty(LayeredOptions.DIRECTION, dir)
    dd.setProperty(LayeredOptions.DIRECTION, dir)
    if merge { c.setProperty(LayeredOptions.MERGE_HIERARCHY_EDGES, true) }
    c.padding.top = 3; c.padding.left = 4; c.offset.x = 2; c.offset.y = 1
    dd.padding.top = 5; dd.padding.left = 6
    let c1 = node(c, 10, 10, 20, 20)
    let c2 = node(c, 10, 50, 20, 20)
    let d1 = node(dd, 5, 5, 20, 20)
    let xw = port(x, .WEST, 0, 5, 0, 0)
    let xe = port(x, .EAST, 20, 5, 0, 0)
    let yw = port(y, .WEST, 0, 5, 0, 0)
    let pe = port(p, .EAST, 100, 50, 2, 2)
    let pw = port(p, .WEST, 0, 50, 2, 2)
    let c1p1 = port(c1, .EAST, 20, 5, 0, 0)
    let c1p2 = port(c1, .SOUTH, 10, 20, 0, 0)
    let c1p3 = port(c1, .WEST, 0, 5, 0, 0)
    let c1p4 = port(c1, .EAST, 20, 15, 0, 0)
    let c2p1 = port(c2, .WEST, 0, 5, 0, 0)
    let c2p2 = port(c2, .NORTH, 10, 0, 0, 0)
    let d1p1 = port(d1, .NORTH, 10, 0, 0, 0)
    func lab(_ e: LEdge, _ w: Double, _ placement: EdgeLabelPlacement) {
        let l = label(w, 5)
        l.setProperty(LayeredOptions.EDGE_LABELS_PLACEMENT, placement)
        e.labels.append(l)
    }
    let e1 = edge(c1p1, xw); lab(e1, 10, .center); lab(e1, 4, .head); lab(e1, 3, .tail)
    let e2 = edge(xe, c2p1); lab(e2, 11, .center)
    e2.setProperty(LayeredOptions.EDGE_THICKNESS, 2.0)
    let e3 = edge(c1p2, d1p1); lab(e3, 12, .center)
    let e4 = edge(c2p2, c1p3)
    let e5 = edge(c1p4, pe); lab(e5, 13, .center)
    let e6 = edge(c1p1, yw); lab(e6, 14, .center)
    e6.setProperty(LayeredOptions.EDGE_THICKNESS, 3.0)
    var origEdges = [e1, e2, e3, e4, e5, e6]
    if insideLoops {
        p.setProperty(LayeredOptions.INSIDE_SELF_LOOPS_ACTIVATE, true)
        let e7 = edge(pe, pw)
        e7.setProperty(LayeredOptions.INSIDE_SELF_LOOPS_YO, true)
        lab(e7, 15, .center)
        origEdges.append(e7)
    }
    let graphs = [("R", r), ("C", c), ("D", dd)]
    let header = "compound merge=\(merge) fixedSide=\(fixedSide) insideLoops=\(insideLoops) \(dir)"

    CompoundGraphPreprocessor().process(r, monitor)
    print("== \(header) pre")
    print(sortedDump(r))
    let map = r.getProperty(InternalProperties.CROSS_HIERARCHY_MAP) as? [LEdge: [CrossHierarchyEdge]] ?? [:]
    for (i, e) in origEdges.enumerated() {
        let segs = (map[e] ?? []).map { che -> String in
            let nodes = allNodes(che.graph)
            return "\(graphName(che.graph, graphs)):\(portRef(che.newEdge.source, nodes))->\(portRef(che.newEdge.target, nodes)):\(che.type)"
        }
        let src = e.source == nil ? "nil" : "set"
        print("e\(i + 1) src=\(src) labels=\(e.labels.count) segs=[\(segs.joined(separator: " "))]")
    }

    // simulate a layout
    var k = 0.0
    for (_, g) in graphs {
        for (i, n) in allNodes(g).enumerated() where n.type == .externalPort {
            n.position.x = Double(i) * 7; n.position.y = Double(i) * 3
        }
        for n in allNodes(g) {
            for pt in n.ports {
                for e in pt.outgoingEdges {
                    k += 1
                    e.bendPoints.add(KVector(k * 10 + 1, k * 10 + 2))
                    if Int(k) % 3 == 0 { e.setProperty(LayeredOptions.JUNCTION_POINTS, KVectorChain(KVector(k, -k))) }
                    for l in e.labels { l.position.x = k; l.position.y = -k }
                }
            }
        }
    }
    CompoundGraphPostprocessor().process(r, monitor)
    print("== \(header) post")
    print(sortedDump(r))
}

func testCompound() {
    for merge in [false, true] {
        for fixedSide in [false, true] {
            for insideLoops in [false, true] {
                compoundScenario(merge, fixedSide, insideLoops, .RIGHT)
            }
        }
    }
    compoundScenario(true, false, true, .DOWN)
}

testTransformer()
testSelfLoops()
testSwitcher()
testResizer()
testCompound()
