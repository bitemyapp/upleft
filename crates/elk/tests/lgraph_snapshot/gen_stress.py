#!/usr/bin/env python3
"""Stress graphs for the group C processors: explicit ports with fixed sides
(north/south ports, inverted ports, port self loops), hierarchical ports,
BK alignment variants, merged hyperedges, dense fan-in/fan-out."""
import json, os, random, sys

out = sys.argv[1]
count = int(sys.argv[2]) if len(sys.argv) > 2 else 40
os.makedirs(out, exist_ok=True)

SIDES = ["NORTH", "SOUTH", "EAST", "WEST"]
CONSTRAINTS = ["FIXED_SIDE", "FIXED_ORDER", "FIXED_POS", "FIXED_RATIO", "FREE"]
ALIGN = ["BALANCED", "NONE", "LEFTUP", "LEFTDOWN", "RIGHTUP", "RIGHTDOWN"]
DIRS = ["RIGHT", "DOWN", "LEFT", "UP"]


def base_opts(r, hierarchy=None):
    o = {
        "elk.algorithm": "layered",
        "elk.direction": r.choice(DIRS),
        "elk.spacing.nodeNode": str(r.choice([10, 20, 28, 40])),
        "elk.spacing.edgeEdge": str(r.choice([5, 10, 12])),
        "elk.layered.spacing.nodeNodeBetweenLayers": str(r.choice([20, 48, 60])),
        "elk.layered.spacing.edgeEdgeBetweenLayers": str(r.choice([5, 10, 12])),
        "elk.layered.spacing.edgeNodeBetweenLayers": str(r.choice([5, 10, 12])),
        "elk.edgeRouting": "ORTHOGONAL",
        "elk.layered.nodePlacement.bk.fixedAlignment": r.choice(ALIGN),
    }
    if r.random() < 0.35:
        o["elk.layered.nodePlacement.bk.edgeStraightening"] = "IMPROVE_STRAIGHTNESS"
    if r.random() < 0.3:
        o["elk.layered.unnecessaryBendpoints"] = "true"
    if r.random() < 0.3:
        o["elk.layered.mergeEdges"] = "true"
    if r.random() < 0.2:
        o["elk.layered.feedbackEdges"] = "true"
    if r.random() < 0.2:
        o["elk.padding"] = "[top=10,left=12,bottom=14,right=16]"
    if hierarchy:
        o["elk.hierarchyHandling"] = hierarchy
    return o


def make_node(r, nid, n_ports, constraint):
    w = float(r.choice([30, 40, 60, 80, 100.5, 120.25]))
    h = float(r.choice([20, 30, 36, 50.75, 70]))
    node = {"id": nid, "width": w, "height": h, "ports": [], "layoutOptions": {}}
    if constraint != "FREE":
        node["layoutOptions"]["elk.portConstraints"] = constraint
    for k in range(n_ports):
        side = r.choice(SIDES)
        p = {"id": f"{nid}_p{k}", "width": float(r.choice([0, 4, 6, 8])), "height": float(r.choice([0, 4, 6, 8])), "layoutOptions": {"elk.port.side": side}}
        if side in ("NORTH", "SOUTH"):
            p["x"] = round(r.uniform(0, w), 2)
            p["y"] = 0.0 if side == "NORTH" else h
        else:
            p["x"] = 0.0 if side == "WEST" else w
            p["y"] = round(r.uniform(0, h), 2)
        if r.random() < 0.3:
            p["layoutOptions"]["elk.port.index"] = str(k)
        node["ports"].append(p)
    return node


def flat_ports(r, i):
    n = r.randint(3, 14)
    constraint = r.choice(CONSTRAINTS)
    nodes = [make_node(r, f"n{j}", r.randint(1, 5), constraint if r.random() < 0.8 else r.choice(CONSTRAINTS)) for j in range(n)]
    ports = [p["id"] for nd in nodes for p in nd["ports"]]
    edges = []
    m = r.randint(n, 3 * n)
    for k in range(m):
        if r.random() < 0.75 and ports:
            a, b = r.choice(ports), r.choice(ports)
        else:
            a, b = f"n{r.randrange(n)}", f"n{r.randrange(n)}"
        e = {"id": f"e{k}", "sources": [a], "targets": [b]}
        if r.random() < 0.2:
            e["labels"] = [{"text": "l", "width": 20.0, "height": 12.0, "layoutOptions": {"elk.edgeLabels.placement": r.choice(["CENTER", "HEAD", "TAIL"])}}]
        edges.append(e)
    return {"id": "root", "layoutOptions": base_opts(r), "children": nodes, "edges": edges}


def hierarchical(r, i):
    hier = r.choice(["INCLUDE_CHILDREN", "SEPARATE_CHILDREN"])
    constraint = r.choice(CONSTRAINTS)
    comp = {"id": "c0", "layoutOptions": base_opts(r), "children": [], "ports": [], "edges": []}
    del comp["layoutOptions"]["elk.direction"]
    if constraint != "FREE":
        comp["layoutOptions"]["elk.portConstraints"] = constraint
    inner = r.randint(2, 7)
    for j in range(inner):
        comp["children"].append({"id": f"c0_n{j}", "width": float(r.choice([30, 50, 70])), "height": float(r.choice([20, 30, 40]))})
    W, H = 300.0, 200.0
    for k in range(r.randint(2, 6)):
        side = r.choice(SIDES)
        p = {"id": f"c0_p{k}", "width": 6.0, "height": 6.0, "layoutOptions": {"elk.port.side": side}}
        if side in ("NORTH", "SOUTH"):
            p["x"], p["y"] = round(r.uniform(0, W), 1), (0.0 if side == "NORTH" else H)
        else:
            p["x"], p["y"] = (0.0 if side == "WEST" else W), round(r.uniform(0, H), 1)
        comp["ports"].append(p)
        # inner edges between port and inner nodes
        tgt = f"c0_n{r.randrange(inner)}"
        if r.random() < 0.5:
            comp["edges"].append({"id": f"ci{k}", "sources": [p["id"]], "targets": [tgt]})
        else:
            comp["edges"].append({"id": f"ci{k}", "sources": [tgt], "targets": [p["id"]]})
        if r.random() < 0.4:
            comp["edges"].append({"id": f"cj{k}", "sources": [p["id"]], "targets": [f"c0_n{r.randrange(inner)}"]})
    for j in range(inner):
        if r.random() < 0.6:
            comp["edges"].append({"id": f"cn{j}", "sources": [f"c0_n{j}"], "targets": [f"c0_n{r.randrange(inner)}"]})
    outer = [{"id": f"o{j}", "width": 40.0, "height": 30.0} for j in range(r.randint(1, 4))]
    edges = []
    for k, p in enumerate(comp["ports"]):
        o = r.choice(outer)["id"]
        if r.random() < 0.5:
            edges.append({"id": f"oe{k}", "sources": [o], "targets": [p["id"]]})
        else:
            edges.append({"id": f"oe{k}", "sources": [p["id"]], "targets": [o]})
    return {"id": "root", "layoutOptions": base_opts(r, hier), "children": [comp] + outer, "edges": edges}


def dense(r, i):
    n = r.randint(6, 30)
    nodes = [{"id": f"n{j}", "width": float(r.choice([20, 30, 40])), "height": float(r.choice([10, 12, 20, 30]))} for j in range(n)]
    edges = []
    for k in range(r.randint(2 * n, 5 * n)):
        a = r.randrange(n)
        b = r.randrange(n)
        if r.random() < 0.4:
            a = r.randrange(3)
        if r.random() < 0.4:
            b = n - 1 - r.randrange(3)
        edges.append({"id": f"e{k}", "sources": [f"n{a}"], "targets": [f"n{b}"]})
    return {"id": "root", "layoutOptions": base_opts(r), "children": nodes, "edges": edges}


for i in range(count):
    r = random.Random(1000 + i)
    for fam, f in (("flatports", flat_ports), ("hier", hierarchical), ("dense", dense)):
        g = f(r, i)
        with open(os.path.join(out, f"{fam}-{i:03d}.json"), "w") as fh:
            json.dump(g, fh, indent=1)
print("wrote", 3 * count)
