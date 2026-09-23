#!/usr/bin/env python3
"""Deterministic random ELK graphs shaped like the ones beautiful-mermaid-swift
builds (src_layout.swift `_buildElkGraph`, `_buildElkGraphNoCrossEdges`,
`_buildFlatElkGraph` + `_applyLayoutConfig`; src_class_layout.swift;
src_er_layout.swift), 1–300 nodes, DAGs and cyclic graphs, with labels,
self-loops and multi-edges. Writes corpus/elk/random/*.json.

Usage: gen_random_graphs.py [count-per-family]"""
import json, os, random, sys

here = os.path.dirname(os.path.abspath(__file__))
out = os.path.normpath(os.path.join(here, "../../../corpus/elk/random"))
os.makedirs(out, exist_ok=True)

DIRS = ["DOWN", "RIGHT", "UP", "LEFT"]
LABEL_OPTS = {"elk.edgeLabels.inline": "true", "elk.edgeLabels.placement": "CENTER"}

def root_options(direction, hierarchy):
    o = {
        "elk.algorithm": "layered", "elk.direction": direction, "elk.spacing.nodeNode": "28",
        "elk.spacing.edgeEdge": "12", "elk.layered.spacing.nodeNodeBetweenLayers": "48",
        "elk.layered.spacing.edgeEdgeBetweenLayers": "12", "elk.layered.spacing.edgeNodeBetweenLayers": "12",
        "elk.padding": "[top=40,left=40,bottom=40,right=40]", "elk.edgeRouting": "ORTHOGONAL",
        "elk.contentAlignment": "H_CENTER V_CENTER", "elk.layered.nodePlacement.bk.fixedAlignment": "BALANCED",
        "elk.layered.considerModelOrder.strategy": "NODES_AND_EDGES", "elk.layered.thoroughness": "3",
        "elk.layered.compaction.postCompaction.strategy": "LEFT_RIGHT_CONSTRAINT_LOCKING",
        "elk.layered.highDegreeNodes.treatment": "true", "elk.layered.highDegreeNodes.threshold": "8",
        "elk.layered.wrapping.strategy": "OFF", "elk.hierarchyHandling": hierarchy,
    }
    # _applyLayoutConfig(LayoutConfig())
    o["elk.spacing.componentComponent"] = "20"
    return o

def sub_options(direction):
    o = {
        "elk.algorithm": "layered", "elk.padding": "[top=44,left=16,bottom=16,right=16]",
        "elk.edgeRouting": "ORTHOGONAL", "elk.contentAlignment": "H_CENTER V_CENTER",
        "elk.spacing.edgeEdge": "12", "elk.layered.spacing.edgeEdgeBetweenLayers": "12",
        "elk.layered.spacing.edgeNodeBetweenLayers": "12", "elk.layered.nodePlacement.bk.fixedAlignment": "BALANCED",
        "elk.layered.spacing.nodeNodeBetweenLayers": "48", "elk.spacing.nodeNode": "28",
    }
    if direction:
        o["elk.direction"] = direction
    return o

def text_size(r):
    # measureMultilineText-like: widths from character counts, 1-3 lines
    lines = r.choice([1, 1, 1, 1, 2, 3])
    w = round(r.uniform(12, 180) + r.random(), 3)
    h = 16.900000000000006 * lines + (0 if lines == 1 else 0.0)
    return w, h

def node(r, i, labelled=True):
    w, h = text_size(r)
    width, height = max(w + 40, 60.0), max(h + 20, 36.0)
    shape = r.random()
    if shape < 0.12:  # diamond
        side = max(width, height) + 24
        width = height = side
    elif shape < 0.18:  # circle
        width = height = float(int((width * width + height * height) ** 0.5 + 0.999)) + 8
    elif shape < 0.22:  # state start/end
        width = height = 28.0
    n = {"id": f"n{i}", "width": float(width), "height": float(height)}
    if labelled:
        n["labels"] = [{"text": f"Node {i}"}]
    return n

def edge_label(r):
    w, h = text_size(r)
    return {"text": "label", "width": w + 8, "height": h + 3.4 + 6, "layoutOptions": dict(LABEL_OPTS)}

def edges_for(r, n, m, cyclic, dense_hub):
    es = []
    order = list(range(n))
    for k in range(m):
        if dense_hub and r.random() < 0.3:
            a, b = 0, r.randrange(n)
        else:
            a, b = r.randrange(n), r.randrange(n)
        if not cyclic and a > b:
            a, b = b, a
        if a == b and r.random() > 0.15:
            continue
        es.append((a, b))
        if r.random() < 0.05:
            es.append((a, b))  # multi-edge
    return es

def flat(r, n, m, direction, cyclic, hub, fallback=False):
    children = [node(r, i, labelled=not fallback) for i in range(n)]
    edges = []
    for idx, (a, b) in enumerate(edges_for(r, n, m, cyclic, hub)):
        e = {"id": f"e{idx}", "sources": [f"n{a}"], "targets": [f"n{b}"]}
        if r.random() < 0.3:
            e["labels"] = [edge_label(r)]
        edges.append(e)
    if fallback:
        o = root_options(direction, "INCLUDE_CHILDREN")
        del o["elk.hierarchyHandling"]
        del o["elk.layered.wrapping.strategy"]
        o["elk.randomSeed"] = "1"
    else:
        o = root_options(direction, "INCLUDE_CHILDREN")
    return {"id": "root", "layoutOptions": o, "children": children, "edges": edges}

def subgraph_tree(r, n, ns):
    """Node owners (deepest subgraph or -1) and subgraph parents (-1 = root)."""
    parent = [r.randrange(-1, s) if s else -1 for s in range(ns)]
    owner = [r.randrange(-1, ns) if ns else -1 for _ in range(n)]
    return owner, parent

def ancestors(s, parent):
    out = []
    while s >= 0:
        out.append(s)
        s = parent[s]
    return out

def compound(r, n, m, ns, direction, cyclic):
    """_buildElkGraphNoCrossEdges: INCLUDE_CHILDREN, internal edges in the
    deepest common subgraph only when both ends share it, the rest at root."""
    owner, parent = subgraph_tree(r, n, ns)
    nodes = [node(r, i) for i in range(n)]
    subs = [{"id": f"sg{s}", "layoutOptions": sub_options(None), "children": [], "labels": [{"text": f"Group {s}"}]} for s in range(ns)]
    sub_edges = {s: [] for s in range(ns)}
    root_level, cross = [], []
    for idx, (a, b) in enumerate(edges_for(r, n, m, cyclic, False)):
        e = {"id": f"e{idx}", "sources": [f"n{a}"], "targets": [f"n{b}"]}
        if r.random() < 0.3:
            e["labels"] = [edge_label(r)]
        sa, sb = owner[a], owner[b]
        if sa >= 0 and sa == sb:
            sub_edges[sa].append(e)
        elif sa < 0 and sb < 0:
            root_level.append(e)
        else:
            cross.append(e)
    def build(s):
        d = subs[s]
        d["children"] = [nodes[i] for i in range(n) if owner[i] == s] + [build(c) for c in range(ns) if parent[c] == s]
        if sub_edges[s]:
            d["edges"] = sub_edges[s]
        return d
    children = [nodes[i] for i in range(n) if owner[i] < 0] + [build(s) for s in range(ns) if parent[s] < 0]
    return {"id": "root", "layoutOptions": root_options(direction, "INCLUDE_CHILDREN"), "children": children, "edges": root_level + cross}

def separate(r, n, m, ns, direction, cyclic):
    """_buildElkGraph with subgraphs (some with a direction override): root
    hierarchyHandling "SEPARATE", hierarchical ports for cross edges."""
    owner, parent = subgraph_tree(r, n, ns)
    nodes = [node(r, i) for i in range(n)]
    subs = [{"id": f"sg{s}", "layoutOptions": sub_options(r.choice(DIRS) if r.random() < 0.6 else None), "children": [], "labels": [{"text": f"Group {s}"}]} for s in range(ns)]
    sub_edges = {s: [] for s in range(ns)}
    ports = {s: [] for s in range(ns)}
    root_edges = []
    cross_list = []
    for idx, (a, b) in enumerate(edges_for(r, n, m, cyclic, False)):
        label = edge_label(r) if r.random() < 0.3 else None
        sa, sb = owner[a], owner[b]
        e = {"id": f"e{idx}", "sources": [f"n{a}"], "targets": [f"n{b}"]}
        if label:
            e["labels"] = [label]
        if sa >= 0 and sa == sb:
            sub_edges[sa].append(e)
        elif sa < 0 and sb < 0:
            root_edges.append(e)
        else:
            cross_list.append((idx, a, b, sa, sb, label))
    for idx, a, b, sa, sb, label in cross_list:
        if sa >= 0:
            pid = f"sg{sa}_out_{idx}"
            ie = {"id": f"e{idx}_out", "sources": [f"n{a}"], "targets": [pid]}
            if label:
                ie["labels"] = [label]
            ports[sa].append(({"id": pid}, ie))
        if sb >= 0:
            pid = f"sg{sb}_in_{idx}"
            ports[sb].append(({"id": pid}, {"id": f"e{idx}_in", "sources": [pid], "targets": [f"n{b}"]}))
        src = f"sg{sa}_out_{idx}" if sa >= 0 else f"n{a}"
        tgt = f"sg{sb}_in_{idx}" if sb >= 0 else f"n{b}"
        re_ = {"id": f"e{idx}", "sources": [src], "targets": [tgt]}
        if sa < 0 and label:
            re_["labels"] = [label]
        root_edges.append(re_)
    def build(s):
        d = subs[s]
        d["children"] = [nodes[i] for i in range(n) if owner[i] == s] + [build(c) for c in range(ns) if parent[c] == s]
        if ports[s]:
            d["ports"] = [p for p, _ in ports[s]]
        es = sub_edges[s] + [e for _, e in ports[s]]
        if es:
            d["edges"] = es
        return d
    children = [nodes[i] for i in range(n) if owner[i] < 0] + [build(s) for s in range(ns) if parent[s] < 0]
    return {"id": "root", "layoutOptions": root_options(direction, "SEPARATE"), "children": children, "edges": root_edges}

def class_like(r, n, m, cyclic):
    children = [{"id": f"C{i}", "width": float(max(120.0, round(r.uniform(60, 300), 3))), "height": 32.0 + r.choice([8.0, 28.0, 48.0, 68.0]) + r.choice([8.0, 28.0, 48.0])} for i in range(n)]
    edges = []
    for idx, (a, b) in enumerate(edges_for(r, n, m, cyclic, False)):
        e = {"id": f"e{idx}", "sources": [f"C{a}"], "targets": [f"C{b}"]}
        if r.random() < 0.4:
            w, h = text_size(r)
            e["labels"] = [{"text": "rel", "width": w + 8, "height": h + 6}]
        edges.append(e)
    o = {"elk.algorithm": "layered", "elk.direction": "DOWN", "elk.spacing.nodeNode": "40.0",
         "elk.layered.spacing.nodeNodeBetweenLayers": "60.0", "elk.padding": "[top=40.0,left=40.0,bottom=40.0,right=40.0]",
         "elk.edgeRouting": "ORTHOGONAL", "elk.edgeLabels.placement": "CENTER", "elk.layered.edgeLabels.sideSelection": "ALWAYS_DOWN"}
    return {"id": "root", "layoutOptions": o, "children": children, "edges": edges}

def er_like(r, n, m, cyclic):
    children = [{"id": f"E{i}", "width": float(max(140.0, round(r.uniform(80, 320), 3))), "height": 34.0 + 22.0 * r.randrange(0, 8)} for i in range(n)]
    edges = []
    for idx, (a, b) in enumerate(edges_for(r, n, m, cyclic, False)):
        e = {"id": f"e{idx}", "sources": [f"E{a}"], "targets": [f"E{b}"]}
        if r.random() < 0.8:
            w, h = text_size(r)
            e["labels"] = [{"text": "rel", "width": w + 8, "height": h + 6}]
        edges.append(e)
    o = {"elk.algorithm": "layered", "elk.direction": "RIGHT", "elk.spacing.nodeNode": "70.0",
         "elk.layered.spacing.nodeNodeBetweenLayers": "90.0", "elk.padding": "[top=40.0,left=40.0,bottom=40.0,right=40.0]",
         "elk.edgeRouting": "ORTHOGONAL", "elk.edgeLabels.placement": "CENTER"}
    return {"id": "root", "layoutOptions": o, "children": children, "edges": edges}

def size_for(r, k, count):
    # spread 1..300 with more small graphs
    if k < count // 2:
        return r.randint(1, 30)
    if k < count * 5 // 6:
        return r.randint(30, 120)
    return r.randint(120, 300)

count = int(sys.argv[1]) if len(sys.argv) > 1 else 30
written = 0
for family in ["flat", "compound", "separate", "fallback", "class", "er"]:
    for k in range(count):
        r = random.Random(f"{family}-{k}")
        n = size_for(r, k, count)
        cyclic = k % 2 == 1
        density = r.choice([0.8, 1.0, 1.3, 1.6, 2.2])
        m = int(n * density) + r.randint(0, 3)
        direction = DIRS[k % 4]
        if family == "flat":
            g = flat(r, n, m, direction, cyclic, hub=(k % 5 == 0))
        elif family == "fallback":
            g = flat(r, n, m, direction, cyclic, hub=False, fallback=True)
        elif family == "compound":
            g = compound(r, n, m, max(1, min(n // 4, r.randint(1, 8))), direction, cyclic)
        elif family == "separate":
            g = separate(r, n, m, max(1, min(n // 4, r.randint(1, 6))), direction, cyclic)
        elif family == "class":
            g = class_like(r, n, m, cyclic)
        else:
            g = er_like(r, n, m, cyclic)
        name = f"{family}-{k:03d}-n{n}-{'cyclic' if cyclic else 'dag'}.json"
        json.dump(g, open(os.path.join(out, name), "w"), separators=(",", ":"))
        written += 1
print(written, "graphs in", out)
