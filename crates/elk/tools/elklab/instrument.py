#!/usr/bin/env python3
"""Instruments a copy of elk-swift's sources for debugging the port.

* ELKLAB_TRACE=1 prints the layered graph after every processor, in the same
  format as upleft-elk's UPLEFT_ELK_TRACE=1.
* NetworkSimplex.treeEdges iterates in insertion order and
  HyperEdgeCycleDetector breaks ties by taking the first candidate
  (upleft-elk's choices); ELKLAB_HASHSET=1 restores Swift's hash order and
  random tie-breaking.

Usage: instrument.py <copy of Sources/ElkSwift>"""
import os, sys

root = sys.argv[1]
layered = os.path.join(root, "ELK/org/eclipse/elk/alg/layered")

def edit(path, pairs, append=""):
    s = open(path).read()
    for old, new in pairs:
        if old not in s:
            sys.exit(f"instrument.py: pattern not found in {path}:\n{old}")
        s = s.replace(old, new, 1)
    open(path, "w").write(s + append)

trace = '''
package func _labShort(_ name: String) -> String {
    var n = name
    if let r = n.range(of: "_", options: .backwards) { n = String(n[r.upperBound...]) }
    return n
}

package func _labDouble(_ d: Double) -> String { "\\(d)" }

package func _labTrace(_ g: LGraph, _ what: String) {
    guard ProcessInfo.processInfo.environment["ELKLAB_TRACE"] != nil else { return }
    var s = "== \\(_labShort(what))\\n"
    for n in g.layerlessNodes {
        s += " ll \\(n.getDesignation() ?? "") \\(_labDouble(n.position.x)),\\(_labDouble(n.position.y)) \\(_labDouble(n.size.x)),\\(_labDouble(n.size.y)) ports:\\(n.ports.count)\\n"
    }
    var layerIdx = 0
    for l in g.layers {
        s += " L\\(layerIdx):"
        for n in l.nodes {
            s += " \\(n.type.rawValue.prefix(2))\\(n.labels.first?.text ?? "")@\\(_labDouble(n.position.x)),\\(_labDouble(n.position.y))"
        }
        s += "\\n"
        layerIdx += 1
    }
    FileHandle.standardError.write(s.data(using: .utf8)!)
}
'''

edit(os.path.join(layered, "org_eclipse_elk_alg_layered_ElkLayered.swift"), [
    ("package class ElkLayered {", trace + "\npackage class ElkLayered {"),
    ('''                    if !processor.isHierarchyAware {
                        // Regular processor: execute immediately
                        if let sub = monitor.subTask(1) { processor.process(graph, sub) }
''', '''                    if !processor.isHierarchyAware {
                        // Regular processor: execute immediately
                        if let sub = monitor.subTask(1) { processor.process(graph, sub) }
                        _labTrace(graph, processor.name)
'''),
    ('''                    } else if isRoot(graph) {
                        // Hierarchy-aware processor on root: execute it
                        if let sub = monitor.subTask(1) { processor.process(graph, sub) }
''', '''                    } else if isRoot(graph) {
                        // Hierarchy-aware processor on root: execute it
                        if let sub = monitor.subTask(1) { processor.process(graph, sub) }
                        _labTrace(graph, processor.name)
'''),
    ('''            if let sub = monitor.subTask(monitorProgress) { processor.process(lgraph, sub) }''',
     '''            if let sub = monitor.subTask(monitorProgress) { processor.process(lgraph, sub) }
            _labTrace(lgraph, processor.name)'''),
])

edit(os.path.join(root, "ELK/org/eclipse/elk/core/alg/org_eclipse_elk_core_alg_ILayoutProcessor.swift"), [
    ('''    package let isHierarchyAware: Bool
''', '''    package let isHierarchyAware: Bool
    package var name: String = ""
'''),
    ('''        AnyGraphProcessor(isHierarchyAware: isHierarchyAware) { graph, monitor in
            guard let lgraph = graph as? LGraph else { return }
            processor.process(lgraph, monitor)
        }''', '''        var p = AnyGraphProcessor(isHierarchyAware: isHierarchyAware) { graph, monitor in
            guard let lgraph = graph as? LGraph else { return }
            processor.process(lgraph, monitor)
        }
        p.name = String(describing: type(of: processor))
        return p'''),
])

ns = os.path.join(root, "ELK/org/eclipse/elk/alg/common/networksimplex/org_eclipse_elk_alg_common_networksimplex_NetworkSimplex.swift")
edit(ns, [
    ("package var treeEdges: Set<NEdge>?", "package var treeEdges: _LabOrderedSet?"),
    ("treeEdges = Set<NEdge>()", "treeEdges = _LabOrderedSet()"),
], append='''
let _labHashOrder = ProcessInfo.processInfo.environment["ELKLAB_HASHSET"] != nil
package final class _LabOrderedSet: Sequence {
    var items: [NEdge] = []
    var members = Set<ObjectIdentifier>()
    var hashed = Set<NEdge>()
    init() {}
    func insert(_ e: NEdge) { hashed.insert(e); if members.insert(ObjectIdentifier(e)).inserted { items.append(e) } }
    func remove(_ e: NEdge) { hashed.remove(e); if members.remove(ObjectIdentifier(e)) != nil { items.removeAll { $0 === e } } }
    package func makeIterator() -> IndexingIterator<[NEdge]> { _labHashOrder ? Array(hashed).makeIterator() : items.makeIterator() }
}
''')
# HyperEdgeCycleDetector ties: elk-swift passes no random generator, so
# nextRandomInt falls back to Int.random (system RNG). upleft-elk takes the
# first candidate; ELKLAB_HASHSET=1 restores Swift's randomness.
hecd = os.path.join(root, "ELK/org/eclipse/elk/alg/layered/p5edges/orthogonal/org_eclipse_elk_alg_layered_p5edges_orthogonal_HyperEdgeCycleDetector.swift")
edit(hecd, [
    ("        return Int.random(in: 0..<bound)", "        return ProcessInfo.processInfo.environment[\"ELKLAB_HASHSET\"] != nil ? Int.random(in: 0..<bound) : 0"),
])
print("instrumented", root)
