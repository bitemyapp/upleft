#!/usr/bin/env python3
"""Writes corpus/elk/polyline/: graphs that route edges as polylines.

beautiful-mermaid always asks for orthogonal routing, but elk-swift's own
tests (ConcurrentLayoutTests.makeGraphCorpus) use POLYLINE, so the port has
PolylineEdgeRouter and the corpus checks it:

* stress-<i>-<direction>-<routing>.json: makeGraphCorpus() verbatim (both
  routings, as in the Swift test);
* the elk-swift test graphs, the hand-written Mermaid graphs and five random
  graphs per family, with every elk.edgeRouting switched to POLYLINE (and set
  on the root if absent).

Usage: crates/elk/tools/gen_polyline_corpus.py   (from the repository root)"""
import glob, json, os

out = "corpus/elk/polyline"
os.makedirs(out, exist_ok=True)
for f in glob.glob(os.path.join(out, "*.json")):
    os.remove(f)

def write(name, graph):
    with open(os.path.join(out, name), "w") as fh:
        json.dump(graph, fh, indent=1)
        fh.write("\n")

for i, direction in enumerate(["RIGHT", "DOWN", "LEFT", "UP"]):
    for routing in ["ORTHOGONAL", "POLYLINE"]:
        write(f"stress-{i}-{direction.lower()}-{routing.lower()}.json", {
            "id": f"root_{i}_{routing}",
            "layoutOptions": {
                "elk.algorithm": "layered",
                "elk.direction": direction,
                "elk.edgeRouting": routing,
                "elk.spacing.nodeNode": "20",
            },
            "children": [{"id": n, "width": 50, "height": 30} for n in "ABCD"],
            "edges": [
                {"id": "e1", "sources": ["A"], "targets": ["B"]},
                {"id": "e2", "sources": ["A"], "targets": ["C"]},
                {"id": "e3", "sources": ["B"], "targets": ["D"]},
                {"id": "e4", "sources": ["C"], "targets": ["D"]},
            ],
        })

def polyline(node):
    options = node.get("layoutOptions")
    if isinstance(options, dict):
        for key in options:
            if key.endswith("edgeRouting"):
                options[key] = "POLYLINE"
    for child in node.get("children") or []:
        polyline(child)

sources = sorted(glob.glob("corpus/elk/elk-swift-tests/*.json"))
sources += sorted(glob.glob("corpus/elk/mermaid/hw-*.json"))
for family in ["flat", "compound", "separate", "fallback", "class", "er"]:
    sources += sorted(glob.glob(f"corpus/elk/random/{family}-*.json"))[::6][:5]
for source in sources:
    graph = json.load(open(source))
    polyline(graph)
    options = graph.setdefault("layoutOptions", {})
    if not any(key.endswith("edgeRouting") for key in options):
        options["elk.edgeRouting"] = "POLYLINE"
    write(os.path.basename(source), graph)
print(len(os.listdir(out)), "graphs in", out)
