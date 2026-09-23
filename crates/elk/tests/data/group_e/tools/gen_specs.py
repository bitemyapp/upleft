# Generates the group E golden-test specs (../*.json); see golden_main.swift.
import json, os, copy
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")

K = {
 "dir": "org.eclipse.elk.direction",
 "constraints": "org.eclipse.elk.nodeSize.constraints",
 "options": "org.eclipse.elk.nodeSize.options",
 "minsize": "org.eclipse.elk.nodeSize.minimum",
 "fixedGraphSize": "org.eclipse.elk.nodeSize.fixedGraphSize",
 "nlp": "org.eclipse.elk.nodeLabels.placement",
 "nlpad": "org.eclipse.elk.nodeLabels.padding",
 "plp": "org.eclipse.elk.portLabels.placement",
 "group": "org.eclipse.elk.portLabels.treatAsGroup",
 "pc": "org.eclipse.elk.portConstraints",
 "pad": "org.eclipse.elk.portAlignment.default",
 "paN": "org.eclipse.elk.portAlignment.north",
 "paS": "org.eclipse.elk.portAlignment.south",
 "paE": "org.eclipse.elk.portAlignment.east",
 "paW": "org.eclipse.elk.portAlignment.west",
 "pbo": "org.eclipse.elk.port.borderOffset",
 "surround": "org.eclipse.elk.spacing.portsSurrounding",
 "portPort": "org.eclipse.elk.spacing.portPort",
 "labelNode": "org.eclipse.elk.spacing.labelNode",
 "labelLabel": "org.eclipse.elk.spacing.labelLabel",
 "lph": "org.eclipse.elk.spacing.labelPortHorizontal",
 "lpv": "org.eclipse.elk.spacing.labelPortVertical",
 "edgeLabel": "org.eclipse.elk.spacing.edgeLabel",
 "elp": "org.eclipse.elk.edgeLabels.placement",
 "inline": "org.eclipse.elk.edgeLabels.inline",
 "thickness": "org.eclipse.elk.edge.thickness",
 "sideSel": "org.eclipse.elk.layered.edgeLabels.sideSelection",
 "topdown": "org.eclipse.elk.topdownLayout",
 "insideSelfLoops": "org.eclipse.elk.insideSelfLoops.activate",
 "ratio": "portRatioOrPosition",
}
def P(k, t, v): return [K.get(k, k), t, v]

# SizeConstraint bits: PORTS 1, PORT_LABELS 2, NODE_LABELS 4, MINIMUM_SIZE 8
# SizeOptions: DEFAULT_MINIMUM_SIZE 1, MIN_ACCOUNTS_PADDING 2, COMPUTE_PADDING 4, OUTSIDE_OVERHANG 8, PORTS_OVERHANG 16, UNIFORM_PORT_SPACING 32, SPACE_EFF 64, FORCE_TABULAR 128, ASYMMETRICAL 256
# NodeLabelPlacement: H_LEFT 1, H_CENTER 2, H_RIGHT 4, V_TOP 8, V_CENTER 16, V_BOTTOM 32, INSIDE 64, OUTSIDE 128
# PortLabelPlacement: OUTSIDE 1, INSIDE 2, NEXT_TO_PORT 4, ALWAYS_SAME_SIDE 8, ALWAYS_OTHER_SAME_SIDE 16, SPACE_EFFICIENT 32

def lab(text, w=0, h=0, props=None, pos=None):
    o = {"text": text, "size": [w, h]}
    if props: o["props"] = props
    if pos: o["pos"] = pos
    return o

def port(pid, side, w=0, h=0, labels=None, props=None, pos=None):
    o = {"id": pid, "side": side, "size": [w, h]}
    if labels: o["labels"] = labels
    if props: o["props"] = props
    if pos: o["pos"] = pos
    return o

def node(nid, w, h, props=None, labels=None, ports=None, pos=None, type=None, margin=None):
    o = {"id": nid, "size": [w, h]}
    if props: o["props"] = props
    if labels: o["labels"] = labels
    if ports: o["ports"] = ports
    if pos: o["pos"] = pos
    if type: o["type"] = type
    if margin: o["margin"] = margin
    return o

def write(name, spec):
    with open(os.path.join(OUT, name + ".json"), "w") as f:
        json.dump(spec, f, indent=1)

SIZE_RUN = ["LabelAndNodeSizeProcessor", "Dump:sized", "InnermostNodeMarginCalculator", "Dump:margins"]

# 1. Mermaid-like: fixed sizes, zero-size labels without placement, free ports.
write("mermaid_flat", {
 "graph": {"props": [P("dir", "Direction", "RIGHT"), P("portPort", "Double", 10.0)]},
 "nodes": [
  node("A", 87.5, 36, labels=[lab("Start")], ports=[port("pa1", "EAST"), port("pa2", "EAST"), port("pa3", "EAST")]),
  node("B", 60, 36, labels=[lab("Middle")], ports=[port("pb1", "WEST"), port("pb2", "EAST")]),
  node("C", 104.25, 44, labels=[lab("End one")], ports=[port("pc1", "WEST"), port("pc2", "WEST")]),
  node("D", 60, 36.5, labels=[lab("End two")], ports=[port("pd1", "WEST", 4, 4), port("pd2", "NORTH", 2, 3), port("pd3", "SOUTH")],
       props=[P("pc", "PortConstraints", "FIXED_ORDER")]),
 ],
 "edges": [{"id": "e1", "source": "pa1", "target": "pb1"}, {"id": "e2", "source": "pa2", "target": "pc1"},
           {"id": "e3", "source": "pa3", "target": "pd1"}, {"id": "e4", "source": "pb2", "target": "pc2"}],
 "layers": [["A"], ["B"], ["C", "D"]],
 "run": SIZE_RUN,
})

# 2. Sized nodes: free constraints, inside node labels, outside port labels.
write("sized_nodes", {
 "graph": {"props": [P("dir", "Direction", "RIGHT"), P("labelNode", "Double", 5.0), P("portPort", "Double", 10.0),
                     P("labelLabel", "Double", 3.0), P("lph", "Double", 2.0), P("lpv", "Double", 1.5),
                     P("nlpad", "ElkPadding", [5, 6, 7, 8])]},
 "nodes": [
  node("N1", 20, 20, props=[P("constraints", "SizeConstraint", 15), P("options", "SizeOptions", 1), P("nlp", "NodeLabelPlacement", 82),
                             P("plp", "PortLabelPlacement", 1)],
       labels=[lab("N1a", 40, 12), lab("N1b", 25, 10)],
       ports=[port("n1n1", "NORTH", 6, 6, [lab("n1n1l", 18, 8)]), port("n1n2", "NORTH", 6, 6, [lab("n1n2l", 22, 8)]),
              port("n1e1", "EAST", 6, 6, [lab("n1e1l", 15, 9)]), port("n1s1", "SOUTH", 6, 6, [lab("n1s1l", 11, 7)]),
              port("n1w1", "WEST", 6, 6, [lab("n1w1l", 13, 7)]), port("n1w2", "WEST", 6, 6), port("n1w3", "WEST", 6, 6, [lab("n1w3l", 9, 11)])]),
  node("N2", 30, 10, props=[P("constraints", "SizeConstraint", 4), P("options", "SizeOptions", 5), P("nlp", "NodeLabelPlacement", 73)],
       labels=[lab("N2a", 33.25, 12.5)]),
  node("N3", 10, 10, props=[P("constraints", "SizeConstraint", 12), P("options", "SizeOptions", 6), P("nlp", "NodeLabelPlacement", 100),
                             P("minsize", "KVector", [60, 40])],
       labels=[lab("N3a", 21, 13)]),
  node("N4", 50, 30, props=[P("constraints", "SizeConstraint", 3), P("plp", "PortLabelPlacement", 5)],
       ports=[port("n4w1", "WEST", 5, 5, [lab("n4w1l", 12, 20)]), port("n4w2", "WEST", 5, 5, [lab("n4w2l", 12, 6)]),
              port("n4e1", "EAST", 5, 5, [lab("n4e1l", 17, 6)]), port("n4e2", "EAST", 5, 5, [lab("n4e2l", 30, 16)])]),
  node("N5", 40, 40, props=[P("constraints", "SizeConstraint", 15), P("options", "SizeOptions", 1), P("nlp", "NodeLabelPlacement", 82),
                             P("topdown", "Bool", True), P("insideSelfLoops", "Bool", True)],
       labels=[lab("N5a", 10, 10)]),
 ],
 "edges": [{"id": "e1", "source": "n1e1", "target": "n4w1"}],
 "layers": [["N1", "N2"], ["N3", "N4", "N5"]],
 "run": SIZE_RUN + ["InsidePadding:N1:RIGHT", "InsidePadding:N2:DOWN"],
})

# 3. Outside and mixed node label placements.
def outside_nodes():
    labels = [lab("Xa", 50, 10), lab("Xb", 30, 10, [P("nlp", "NodeLabelPlacement", 161)]),
              lab("Xc", 20, 14, [P("nlp", "NodeLabelPlacement", 148)]), lab("Xd", 10, 8, [P("nlp", "NodeLabelPlacement", 145)]),
              lab("Xe", 12, 6, [P("nlp", "NodeLabelPlacement", 73)]), lab("Xf", 14, 8, [P("nlp", "NodeLabelPlacement", 82)]),
              lab("Xg", 16, 9, [P("nlp", "NodeLabelPlacement", 98)]), lab("Xh", 9, 5, [P("nlp", "NodeLabelPlacement", 82)]),
              lab("Xi", 7, 7, [P("nlp", "NodeLabelPlacement", 132)]), lab("Xj", 8, 3, [P("nlp", "NodeLabelPlacement", 196)])]
    def ren(prefix):
        ls = copy.deepcopy(labels)
        for l in ls: l["text"] = prefix + l["text"][1:]
        return ls
    return [
     node("O1", 20, 20, props=[P("constraints", "SizeConstraint", 4), P("options", "SizeOptions", 1), P("nlp", "NodeLabelPlacement", 138)], labels=ren("O")),
     node("O2", 20, 20, props=[P("constraints", "SizeConstraint", 4), P("options", "SizeOptions", 265), P("nlp", "NodeLabelPlacement", 138)], labels=ren("P")),
     node("O3", 20, 20, props=[P("constraints", "SizeConstraint", 12), P("options", "SizeOptions", 133), P("nlp", "NodeLabelPlacement", 138),
                               P("minsize", "KVector", [100, 10])], labels=ren("Q")),
     node("O4", 80, 70, props=[P("constraints", "SizeConstraint", 0), P("options", "SizeOptions", 4), P("nlp", "NodeLabelPlacement", 138)], labels=ren("R")),
     node("O5", 20, 20, props=[P("constraints", "SizeConstraint", 12), P("options", "SizeOptions", 3), P("nlp", "NodeLabelPlacement", 138),
                               P("minsize", "KVector", [0, 90])], labels=ren("S")),
    ]
for d in ["RIGHT", "DOWN"]:
    write("outside_labels_" + d.lower(), {
     "graph": {"props": [P("dir", "Direction", d), P("labelNode", "Double", 4.0), P("labelLabel", "Double", 2.0)]},
     "nodes": outside_nodes(),
     "layers": [["O1", "O2"], ["O3", "O4", "O5"]],
     "run": SIZE_RUN + ["InsidePadding:O3:RIGHT", "InsidePadding:O3:UP", "InsidePadding:O4:LEFT"],
    })

# 4. Inside port labels (simple and constrained).
def ports_all(prefix, n=3, s=2, e=2, w=1, wlab=18, size=(6, 6)):
    ps = []
    for i in range(n): ps.append(port(f"{prefix}n{i}", "NORTH", size[0], size[1], [lab(f"{prefix}n{i}l", wlab + 3 * i, 7)]))
    for i in range(e): ps.append(port(f"{prefix}e{i}", "EAST", size[0], size[1], [lab(f"{prefix}e{i}l", 14 + i, 8)]))
    for i in range(s): ps.append(port(f"{prefix}s{i}", "SOUTH", size[0], size[1], [lab(f"{prefix}s{i}l", wlab + 5 * i, 9)]))
    for i in range(w): ps.append(port(f"{prefix}w{i}", "WEST", size[0], size[1], [lab(f"{prefix}w{i}l", 10 + i, 6)]))
    return ps
write("port_labels_inside", {
 "graph": {"props": [P("dir", "Direction", "RIGHT"), P("lph", "Double", 2.0), P("lpv", "Double", 3.0), P("portPort", "Double", 4.0),
                     P("surround", "ElkMargin", [1, 2, 3, 4])]},
 "nodes": [
  node("I1", 30, 30, props=[P("constraints", "SizeConstraint", 7), P("plp", "PortLabelPlacement", 2)], ports=ports_all("i1")),
  node("I2", 60, 40, props=[P("constraints", "SizeConstraint", 1), P("plp", "PortLabelPlacement", 2)], ports=ports_all("i2", n=4, s=3, wlab=30)),
  node("I3", 50, 50, props=[P("constraints", "SizeConstraint", 15), P("plp", "PortLabelPlacement", 6), P("compoundNode", "Bool", True)],
       ports=[port("i3n0", "NORTH", 4, 4, [lab("i3n0l", 20, 5)], [P("pbo", "Double", 3.0), P("insideConnections", "Bool", True)]),
              port("i3n1", "NORTH", 4, 4, [lab("i3n1l", 8, 5)], [P("pbo", "Double", -2.0)]),
              port("i3e0", "EAST", 4, 4, [lab("i3e0l", 9, 12), lab("i3e0m", 7, 4)], [P("pbo", "Double", -1.5)]),
              port("i3s0", "SOUTH", 4, 4, [lab("i3s0l", 11, 6)]),
              port("i3w0", "WEST", 4, 4, [lab("i3w0l", 6, 3)], [P("insideConnections", "Bool", True)])]),
  node("I4", 40, 40, props=[P("constraints", "SizeConstraint", 3), P("plp", "PortLabelPlacement", 6), P("group", "Bool", False)],
       ports=[port("i4e0", "EAST", 4, 4, [lab("i4e0l", 9, 12), lab("i4e0m", 7, 4)]), port("i4w0", "WEST", 4, 4, [lab("i4w0l", 5, 12), lab("i4w0m", 4, 6)])]),
  node("I5", 40, 40, props=[P("constraints", "SizeConstraint", 1), P("plp", "PortLabelPlacement", 2)], ports=ports_all("i5", n=1, s=1, e=0, w=0)),
 ],
 "edges": [{"id": "e1", "source": "i1e0", "target": "i2w0"}],
 "layers": [["I1"], ["I2", "I3", "I4", "I5"]],
 "run": SIZE_RUN,
})

# 5. Outside port labels (simple and constrained).
write("port_labels_outside", {
 "graph": {"props": [P("dir", "Direction", "RIGHT"), P("lph", "Double", 1.0), P("lpv", "Double", 2.0), P("portPort", "Double", 5.0)]},
 "nodes": [
  node("P1", 30, 30, props=[P("constraints", "SizeConstraint", 3), P("plp", "PortLabelPlacement", 1)],
       ports=ports_all("p1", n=3, s=2, e=2, w=2) + [port("p1e9", "EAST", 6, 6, [lab("p1e9a", 10, 5), lab("p1e9b", 12, 5)])]),
  node("P2", 80, 30, props=[P("constraints", "SizeConstraint", 1), P("plp", "PortLabelPlacement", 33)], ports=ports_all("p2", n=4, s=3, e=1, w=1, wlab=25)),
  node("P3", 30, 30, props=[P("constraints", "SizeConstraint", 3), P("plp", "PortLabelPlacement", 17)], ports=ports_all("p3", n=2, s=0, e=2, w=0)),
  node("P4", 30, 30, props=[P("constraints", "SizeConstraint", 3), P("plp", "PortLabelPlacement", 9), P("options", "SizeOptions", 33)],
       ports=ports_all("p4", n=2, s=2, e=2, w=2)),
  node("P5", 30, 30, props=[P("constraints", "SizeConstraint", 3), P("plp", "PortLabelPlacement", 37), P("options", "SizeOptions", 32)],
       ports=ports_all("p5", n=2, s=2, e=2, w=3)),
  node("P6", 90, 30, props=[P("constraints", "SizeConstraint", 0), P("plp", "PortLabelPlacement", 1)], ports=ports_all("p6", n=3, s=4, e=1, w=1, wlab=35)),
 ],
 "edges": [{"id": "e1", "source": "p1e0", "target": "p2w0"}, {"id": "e2", "source": "p1n0", "target": "p5w0"}],
 "layers": [["P1"], ["P2", "P3", "P4", "P5", "P6"]],
 "run": SIZE_RUN,
})

# 6. Fixed ratio / fixed pos / alignments.
write("fixed_ports", {
 "graph": {"props": [P("dir", "Direction", "RIGHT"), P("portPort", "Double", 7.0), P("surround", "ElkMargin", [3, 4, 5, 6])]},
 "nodes": [
  node("F1", 20, 20, props=[P("constraints", "SizeConstraint", 3), P("plp", "PortLabelPlacement", 2), P("pc", "PortConstraints", "FIXED_RATIO")],
       ports=[port("f1n0", "NORTH", 6, 4, [lab("f1n0l", 9, 5)], [P("ratio", "Double", 0.25)]), port("f1n1", "NORTH", 6, 4, [lab("f1n1l", 9, 5)], [P("ratio", "Double", 0.75)]),
              port("f1e0", "EAST", 4, 6, [lab("f1e0l", 9, 5)], [P("ratio", "Double", 0.3)]), port("f1e1", "EAST", 4, 6, [], [P("ratio", "Double", 0.6)]),
              port("f1s0", "SOUTH", 6, 4, [], [P("ratio", "Double", 0.5)]), port("f1w0", "WEST", 4, 6, [lab("f1w0l", 3, 3)], [P("ratio", "Double", 0.1)])]),
  node("F2", 20, 20, props=[P("constraints", "SizeConstraint", 9), P("pc", "PortConstraints", "FIXED_POS"), P("minsize", "KVector", [15, 25])],
       ports=[port("f2n0", "NORTH", 6, 4, pos=[30, -4]), port("f2e0", "EAST", 4, 6, pos=[20, 40]), port("f2s0", "SOUTH", 6, 4, pos=[3, 20]),
              port("f2w0", "WEST", 4, 6, pos=[-4, 5], props=[P("pbo", "Double", 2.0)])]),
  node("F3", 30, 30, props=[P("constraints", "SizeConstraint", 0), P("paN", "PortAlignment", "BEGIN"), P("paS", "PortAlignment", "END"),
                             P("paE", "PortAlignment", "CENTER"), P("paW", "PortAlignment", "JUSTIFIED")],
       ports=[port(f"f3{s[0].lower()}{i}", s, 5, 5) for s in ["NORTH", "EAST", "SOUTH", "WEST"] for i in range(3)]),
  node("F4", 30, 30, props=[P("constraints", "SizeConstraint", 0), P("options", "SizeOptions", 16), P("paN", "PortAlignment", "BEGIN"),
                             P("paS", "PortAlignment", "END"), P("paE", "PortAlignment", "CENTER"), P("paW", "PortAlignment", "JUSTIFIED")],
       ports=[port(f"f4{s[0].lower()}{i}", s, 5, 5) for s in ["NORTH", "EAST", "SOUTH", "WEST"] for i in range(4)]),
  node("F5", 100, 100, props=[P("constraints", "SizeConstraint", 0), P("paN", "PortAlignment", "JUSTIFIED"), P("paS", "PortAlignment", "DISTRIBUTED"),
                               P("paE", "PortAlignment", "JUSTIFIED"), P("paW", "PortAlignment", "END")],
       ports=[port("f5n0", "NORTH", 5, 5), port("f5s0", "SOUTH", 5, 5), port("f5e0", "EAST", 5, 5), port("f5w0", "WEST", 5, 5),
              port("f5w1", "WEST", 5, 5), port("f5e1", "EAST", 5, 5)]),
  node("F6", 8, 8, props=[P("constraints", "SizeConstraint", 0), P("pad", "PortAlignment", "BEGIN")],
       ports=[port("f6n0", "NORTH", 12, 5), port("f6e0", "EAST", 5, 12), port("f6e1", "EAST", 5, 12), port("f6s0", "SOUTH", 5, 5), port("f6s1", "SOUTH", 5, 5)]),
  node("F7", 50, 50, props=[P("constraints", "SizeConstraint", 1), P("pc", "PortConstraints", "FIXED_SIDE"), P("fixedGraphSize", "Bool", True)],
       ports=[port("f7n0", "NORTH", 5, 5), port("f7w0", "WEST", 5, 5)]),
 ],
 "layers": [["F1", "F2"], ["F3", "F4", "F5", "F6", "F7"]],
 "run": SIZE_RUN + ["MarginCalcNode:F1", "Dump:margincalc"],
})

# 7. End labels.
def end_label_spec(direction):
    TAIL = lambda t, w, h, extra=None: lab(t, w, h, [P("elp", "EdgeLabelPlacement", "TAIL")] + (extra or []))
    HEAD = lambda t, w, h, extra=None: lab(t, w, h, [P("elp", "EdgeLabelPlacement", "HEAD")] + (extra or []))
    ABOVE = [P("labelSide", "LabelSide", "ABOVE")]
    BELOW = [P("labelSide", "LabelSide", "BELOW")]
    return {
     "graph": {"props": [P("dir", "Direction", direction), P("edgeLabel", "Double", 2.0), P("labelLabel", "Double", 1.0)]},
     "nodes": [
      node("E1", 40, 60, margin=[1, 2, 3, 4], ports=[port("pe1", "EAST", 4, 4, pos=[40, 10]), port("pe2", "EAST", 4, 4, pos=[40, 30]),
                                               port("pe3", "NORTH", 4, 4, pos=[10, -4]), port("pe4", "SOUTH", 4, 4, pos=[20, 60])]),
      node("E2", 50, 50, ports=[port("pw1", "WEST", 4, 4, pos=[-4, 20]), port("pw2", "WEST", 4, 4, pos=[-4, 40]), port("pn2", "NORTH", 4, 4, pos=[25, -4])]),
      node("E3", 30, 30, ports=[port("pw3", "WEST", 4, 4, pos=[-4, 10]), port("ps3", "SOUTH", 4, 4, pos=[10, 30])]),
     ],
     "edges": [
      {"id": "ea", "source": "pe1", "target": "pw1", "props": [P("thickness", "Double", 3.0)],
       "labels": [TAIL("ta1", 20, 8), TAIL("ta2", 14, 6), HEAD("ha1", 12, 7, BELOW), lab("ca1", 9, 9)]},
      {"id": "eb", "source": "pe1", "target": "pw3", "labels": [TAIL("tb1", 25, 9, ABOVE), HEAD("hb1", 10, 5)]},
      {"id": "ec", "source": "pe2", "target": "pw1", "props": [P("thickness", "Double", 1.5)], "labels": [TAIL("tc1", 11, 4), HEAD("hc1", 16, 8, ABOVE)]},
      {"id": "ed", "source": "pe3", "target": "pn2", "labels": [TAIL("td1", 8, 8, ABOVE), HEAD("hd1", 9, 9, ABOVE)]},
      {"id": "ee", "source": "pe4", "target": "ps3", "labels": [TAIL("te1", 7, 3, BELOW), HEAD("he1", 6, 6)]},
      {"id": "ef", "source": "pe2", "target": "pw2", "labels": [lab("cf1", 5, 5)]},
      {"id": "eg", "source": "pe1", "target": "pw2", "labels": [TAIL("tg1", 3, 3), TAIL("tg2", 4, 2)]},
     ],
     "layers": [["E1"], ["E2", "E3"]],
     "run": ["EndLabelPreprocessor", "Dump:pre", "EndLabelSorter", "Dump:sorted", "SetPos:E1:10:20", "SetPos:E2:200:30", "SetPos:E3:210:130",
             "EndLabelPostprocessor", "Dump:post"],
    }
write("end_labels_right", end_label_spec("RIGHT"))
write("end_labels_down", end_label_spec("DOWN"))

# 7b. NodeMarginCalculator with edge head/tail labels on port edges.
write("margin_calc_end_labels", {
 "graph": {"props": [P("labelNode", "Double", 3.0)]},
 "nodes": [
  node("M1", 40, 40, pos=[100, 50], props=[P("plp", "PortLabelPlacement", 1)], labels=[lab("m1l", 10, 10, pos=[-5, -12])],
       ports=[port("m1e", "EAST", 4, 4, [lab("m1el", 6, 5, pos=[5, 5])], pos=[40, 10]), port("m1w", "WEST", 4, 4, [lab("m1wl", 3, 3)], pos=[-4, 10]),
              port("m1n", "NORTH", 4, 4, pos=[10, -4]), port("m1s", "SOUTH", 4, 4, [lab("m1sl", 8, 2)], pos=[10, 40])]),
  node("M2", 10, 10, pos=[300, 50], ports=[port("m2w", "WEST", 2, 2, pos=[-2, 4])]),
 ],
 "edges": [
  {"id": "x1", "source": "m1e", "target": "m2w", "labels": [lab("xt", 9, 4, [P("elp", "EdgeLabelPlacement", "TAIL")]), lab("xh", 5, 5, [P("elp", "EdgeLabelPlacement", "HEAD")])]},
  {"id": "x2", "source": "m2w", "target": "m1w", "labels": [lab("yh", 7, 6, [P("elp", "EdgeLabelPlacement", "HEAD")])]},
  {"id": "x3", "source": "m2w", "target": "m1n", "labels": [lab("zh", 4, 9, [P("elp", "EdgeLabelPlacement", "HEAD")])]},
  {"id": "x4", "source": "m1s", "target": "m2w", "labels": [lab("wt", 6, 7, [P("elp", "EdgeLabelPlacement", "TAIL")])]},
 ],
 "layers": [["M1"], ["M2"]],
 "run": ["MarginCalcNode:M1", "MarginCalcNode:M2", "Dump:margins", "InnermostNodeMarginCalculator", "Dump:innermost"],
})

# 8. Label side selection.
def label_side_spec(mode):
    return {
     "graph": {"props": [P("sideSel", "EdgeLabelSideSelection", mode), P("edgeLabel", "Double", 2.0)]},
     "nodes": [
      node("S1", 30, 60, ports=[port("s1a", "EAST", pos=[30, 10]), port("s1b", "EAST", pos=[30, 20]), port("s1c", "EAST", pos=[30, 30]),
                                port("s1d", "EAST", pos=[30, 40]), port("s1n", "NORTH", pos=[10, 0]), port("s1m", "NORTH", pos=[20, 0])]),
      node("S0", 30, 30, ports=[port("s0a", "EAST"), port("s0b", "EAST")]),
      node("L1", 30, 20, type="LABEL", props=[P("origin", "EdgeRef", "o1"), P("representedLabels", "LabelRefs", ["r1"])],
           ports=[port("l1in", "WEST"), port("l1out", "EAST")]),
      node("LE1", 0, 0, type="LONG_EDGE", props=[P("longEdgeSource", "PortRef", "s1b"), P("longEdgeTarget", "PortRef", "s3b")],
           ports=[port("le1in", "WEST"), port("le1out", "EAST")]),
      node("L2", 40, 25, type="LABEL", props=[P("origin", "EdgeRef", "o2"), P("representedLabels", "LabelRefs", ["r2"]),
                                             P("longEdgeSource", "PortRef", "s1c"), P("longEdgeTarget", "PortRef", "s3c")],
           ports=[port("l2in", "WEST"), port("l2out", "EAST")]),
      node("L3", 20, 16, type="LABEL", props=[P("origin", "EdgeRef", "o3"), P("representedLabels", "LabelRefs", ["r3"]),
                                             P("longEdgeSource", "PortRef", "s1c"), P("longEdgeTarget", "PortRef", "s3c")],
           ports=[port("l3in", "WEST"), port("l3out", "EAST")]),
      node("N2", 20, 20, ports=[port("n2w", "WEST")]),
      node("L4", 22, 18, type="LABEL", props=[P("origin", "EdgeRef", "o1"), P("representedLabels", "LabelRefs", ["r4"])],
           ports=[port("l4in", "WEST"), port("l4out", "EAST")]),
      node("S3", 30, 60, ports=[port("s3a", "WEST"), port("s3b", "WEST"), port("s3c", "WEST"), port("s3d", "WEST"), port("s3e", "WEST"),
                                port("s3n", "NORTH"), port("s3s", "SOUTH")]),
      node("L5", 18, 18, type="LABEL", props=[P("origin", "EdgeRef", "o2")], ports=[port("l5in", "WEST"), port("l5out", "EAST")]),
     ],
     "edges": [
      {"id": "o1", "source": "s1a", "target": "l1in", "props": [P("thickness", "Double", 3.0)], "labels": [lab("r1", 30, 10), lab("r4", 5, 5)]},
      {"id": "o1b", "source": "l1out", "target": "s3a", "labels": [lab("t1", 5, 5, [P("elp", "EdgeLabelPlacement", "HEAD")])]},
      {"id": "o4", "source": "s1b", "target": "le1in", "labels": [lab("t2", 5, 5, [P("elp", "EdgeLabelPlacement", "TAIL")])]},
      {"id": "o4b", "source": "le1out", "target": "s3b"},
      {"id": "o2", "source": "s1c", "target": "l2in", "props": [P("thickness", "Double", 2.0), P("reversed", "Bool", True)],
       "labels": [lab("r2", 40, 10, [P("inline", "Bool", True)])]},
      {"id": "o2b", "source": "l2out", "target": "s3c", "props": [P("reversed", "Bool", True)]},
      {"id": "o3", "source": "s1d", "target": "l3in", "labels": [lab("r3", 20, 6), lab("t3", 4, 4, [P("elp", "EdgeLabelPlacement", "TAIL")])]},
      {"id": "o3b", "source": "l3out", "target": "s3d", "props": [P("reversed", "Bool", True)]},
      {"id": "o5", "source": "s0a", "target": "n2w", "labels": [lab("t5", 3, 3, [P("elp", "EdgeLabelPlacement", "TAIL")])]},
      {"id": "o6", "source": "s0b", "target": "l4in", "props": [P("reversed", "Bool", True)]},
      {"id": "o6b", "source": "l4out", "target": "s3e"},
      {"id": "o7", "source": "s1n", "target": "l5in", "labels": [lab("t7", 4, 4, [P("elp", "EdgeLabelPlacement", "TAIL")])]},
      {"id": "o7b", "source": "l5out", "target": "s3n", "labels": [lab("t8", 4, 4, [P("elp", "EdgeLabelPlacement", "HEAD")])]},
      {"id": "o8", "source": "s1m", "target": "s3s", "labels": [lab("t9", 4, 4, [P("elp", "EdgeLabelPlacement", "TAIL")])]},
     ],
     "layers": [["S1", "S0"], ["L1", "LE1", "L2", "L3", "N2", "L4"], ["L5"], ["S3"]],
     "run": ["LabelSideSelector", "Dump:sides"],
    }
for m in ["ALWAYS_UP", "ALWAYS_DOWN", "DIRECTION_UP", "DIRECTION_DOWN", "SMART_UP", "SMART_DOWN"]:
    write("label_sides_" + m.lower(), label_side_spec(m))

# 9. Wrongly typed stored values: decides which Swift getProperty overload the
# `?? default` reads resolve to (as? cast vs property default).
write("wrong_types", {
 "graph": {"props": [P("labelNode", "Int", 3), P("dir", "String", "DOWN")]},
 "nodes": [
  node("W1", 5, 5, props=[P("constraints", "SizeConstraint", 8), P("options", "String", "DEFAULT_MINIMUM_SIZE")]),
  node("W2", 5, 5, props=[P("constraints", "SizeConstraint", 8)]),
  node("W3", 40, 40, pos=[0, 0], props=[P("plp", "String", "OUTSIDE"), P("pc", "String", "FIXED_POS"), P("constraints", "SizeConstraint", 1)],
       ports=[port("w3e", "EAST", 4, 4, [lab("w3el", 6, 5)], pos=[40, 10]), port("w3w", "WEST", 4, 4, pos=[-4, 10])]),
  node("W4", 10, 10, pos=[100, 0], ports=[port("w4w", "WEST", 2, 2, pos=[-2, 4])]),
 ],
 "edges": [
  {"id": "x1", "source": "w3e", "target": "w4w", "labels": [lab("xt", 9, 4, [P("elp", "EdgeLabelPlacement", "TAIL")])]},
  {"id": "x2", "source": "w4w", "target": "w3w", "labels": [lab("yh", 7, 6, [P("elp", "EdgeLabelPlacement", "HEAD")])]},
 ],
 "layers": [["W1", "W2"], ["W3", "W4"]],
 "run": ["LabelAndNodeSizeProcessor", "Dump:sized", "MarginCalcNode:W3", "Dump:margin"],
})

# 10. Fixed graph size, fixed port label placement with positioned port labels.
write("fixed_graph_size", {
 "graph": {"props": [P("fixedGraphSize", "Bool", True), P("portPort", "Double", 3.0)]},
 "nodes": [
  node("G1", 50, 10, props=[P("constraints", "SizeConstraint", 3)],
       ports=[port("g1n0", "NORTH", 4, 4, [lab("g1n0l", 10, 6, pos=[-8, -7])], [P("pbo", "Double", -3.0)]),
              port("g1n1", "NORTH", 4, 4, [lab("g1n1l", 6, 6, pos=[2, 1])]),
              port("g1e0", "EAST", 4, 4, [lab("g1e0l", 7, 5, pos=[-9, 2]), lab("g1e0m", 3, 3, pos=[5, -4])], [P("pbo", "Double", 1.0)]),
              port("g1s0", "SOUTH", 4, 4, [lab("g1s0l", 5, 8, pos=[1, -6])]),
              port("g1w0", "WEST", 4, 4, [lab("g1w0l", 8, 2, pos=[3, 1])], [P("pbo", "Double", -2.5)])]),
  node("G2", 10, 50, props=[P("constraints", "SizeConstraint", 3), P("options", "SizeOptions", 256)],
       ports=[port("g2e0", "EAST", 4, 4, [lab("g2e0l", 7, 5, pos=[-9, 2])], [P("pbo", "Double", -4.0)]),
              port("g2w0", "WEST", 4, 4, [lab("g2w0l", 8, 2, pos=[3, 1])]),
              port("g2s0", "SOUTH", 4, 4, [lab("g2s0l", 2, 9, pos=[0, -8])])]),
 ],
 "layers": [["G1", "G2"]],
 "run": SIZE_RUN,
})

# 11. External port dummies (labels placed by LabelAndNodeSizeProcessor).
def ext_spec(plp, group):
    def dummy(did, side, extra_labels=True):
        ls = [lab(f"{did}l1", 12, 5)] + ([lab(f"{did}l2", 8, 7)] if extra_labels else [])
        return node(did, 10, 10, type="EXTERNAL_PORT", props=[P("extPort.side", "PortSide", side), P("labelLabel", "Double", 2.0),
                                                             P("lph", "Double", 3.0), P("lpv", "Double", 4.0)],
                    ports=[port(f"{did}p", {"NORTH": "SOUTH", "SOUTH": "NORTH", "EAST": "WEST", "WEST": "EAST"}[side], 2, 2, ls, pos=[4, 4])])
    return {
     "graph": {"props": [P("graphProperties", "GraphProperties", ["EXTERNAL_PORTS"]), P("plp", "PortLabelPlacement", plp), P("group", "Bool", group)]},
     "nodes": [dummy("XN", "NORTH"), dummy("XS", "SOUTH"), dummy("XE", "EAST"), dummy("XW", "WEST", False), dummy("XE2", "EAST"),
               node("Y", 20, 20, ports=[port("yw", "WEST", 2, 2), port("ye", "EAST", 2, 2)])],
     "edges": [{"id": "x1", "source": "XEp", "target": "yw"}],
     "layers": [["XN", "XS", "XE", "XW", "XE2"], ["Y"]],
     "run": ["LabelAndNodeSizeProcessor", "Dump:sized"],
    }
write("external_ports_inside", ext_spec(6, True))
write("external_ports_inside_nogroup", ext_spec(6, False))
write("external_ports_outside", ext_spec(5, False))
write("external_ports_outside_group", ext_spec(1, True))

# 12. End labels gathered through a port dummy.
TAILL = lambda t, w, h: lab(t, w, h, [P("elp", "EdgeLabelPlacement", "TAIL")])
HEADL = lambda t, w, h: lab(t, w, h, [P("elp", "EdgeLabelPlacement", "HEAD")])
write("end_labels_port_dummy", {
 "graph": {"props": [P("edgeLabel", "Double", 3.0), P("labelLabel", "Double", 2.0)]},
 "nodes": [
  node("H1", 40, 40, ports=[port("h1e", "EAST", 4, 4, pos=[40, 10], props=[P("portDummy", "NodeRef", "PD")]), port("h1w", "WEST", 4, 4, pos=[-4, 10])]),
  node("PD", 1, 1, ports=[port("pdp", "EAST", props=[P("origin", "PortRef", "h1e")]), port("pdq", "EAST", props=[P("origin", "PortRef", "h1w")]),
                          port("pdr", "WEST", props=[P("origin", "PortRef", "h1e")])]),
  node("H2", 20, 20, ports=[port("h2w", "WEST", 2, 2, pos=[-2, 5])]),
 ],
 "edges": [
  {"id": "q1", "source": "pdp", "target": "h2w", "props": [P("thickness", "Double", 4.0)], "labels": [TAILL("q1t", 10, 4), HEADL("q1h", 6, 6)]},
  {"id": "q2", "source": "h1e", "target": "h2w", "labels": [TAILL("q2t", 7, 3)]},
  {"id": "q3", "source": "h2w", "target": "pdr", "props": [P("thickness", "Double", 0.5)], "labels": [HEADL("q3h", 5, 9)]},
  {"id": "q4", "source": "pdq", "target": "h2w", "labels": [TAILL("q4t", 4, 4)]},
 ],
 "layers": [["H1"], ["H2"]],
 "run": ["EndLabelPreprocessor", "Dump:pre", "EndLabelSorter", "SetPos:H1:5:6", "EndLabelPostprocessor", "Dump:post"],
})

# 13. The strip overlap remover on its own, all four directions.
RECTS = "0,0,10,5;4,2,8,6;11,1,5,5;30,0,4,4;2,-3,20,2;13,7,3,9;30,1,1,1"
write("overlap_remover", {
 "run": [f"Remover:{d}:2:3:{s}:{RECTS}" for d, s in [("UP", 100), ("DOWN", -7.5), ("LEFT", 40), ("RIGHT", 0.25)]]
        + ["Remover:DOWN:0:0:0:5,5,1,1", "Remover:UP:1:1:0:0,0,4,4;0,0,4,4;0,0,4,4"],
})
