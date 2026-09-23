#!/usr/bin/env python3
"""Collects Mermaid sources for the ELK corpus into one JSON list:
beautiful-mermaid's playground diagrams, its SampleDiagrams and test strings,
the Mermaid fences in Downright's docs/tests and corpus/generated, and the
hand-written set. Prints {name, source} objects."""
import json, os, re, subprocess, sys

here = os.path.dirname(os.path.abspath(__file__))
root = os.path.normpath(os.path.join(here, "../../../.."))
bm = os.path.join(root, "vendor/beautiful-mermaid-swift")
out = []
seen = set()

def add(name, source):
    source = source.strip("\n")
    if not source.strip() or source in seen:
        return
    first = source.strip().splitlines()[0].strip().lower()
    if not re.match(r"(graph|flowchart|statediagram|classdiagram|erdiagram)", first):
        return
    seen.add(source)
    out.append({"name": re.sub(r"[^A-Za-z0-9_.-]+", "-", name), "source": source})

d = json.load(open(os.path.join(bm, "Examples/MermaidPlayground/Resources/test-diagrams.json")))
for x in d["diagrams"]:
    add("playground-" + x["id"], x["source"])

def swift_strings(path, prefix):
    src = open(path, encoding="utf-8").read()
    for i, m in enumerate(re.finditer(r'"""\n(.*?)\n(\s*)"""', src, re.S)):
        if "\\(" in m.group(1):
            continue
        indent = m.group(2)
        lines = [l[len(indent):] if l.startswith(indent) else l.lstrip() for l in m.group(1).split("\n")]
        name = f"{prefix}-{i:03d}"
        before = src[:m.start()]
        n = re.findall(r"(?:let|var|func)\s+(\w+)", before)
        if n:
            name = f"{prefix}-{n[-1]}-{i:03d}"
        add(name, "\n".join(lines))

swift_strings(os.path.join(bm, "Examples/MermaidPlayground/Models/SampleDiagrams.swift"), "sample")
for f in sorted(os.listdir(os.path.join(bm, "Tests/BeautifulMermaidSwiftTests"))):
    if f.endswith(".swift"):
        swift_strings(os.path.join(bm, "Tests/BeautifulMermaidSwiftTests", f), "bmtest-" + f[:-6])

def fences(base, prefix):
    for directory, dirs, files in os.walk(base):
        dirs[:] = sorted(x for x in dirs if not x.startswith("."))
        for f in sorted(files):
            if not f.endswith((".md", ".swift")):
                continue
            src = open(os.path.join(directory, f), encoding="utf-8", errors="replace").read()
            for i, m in enumerate(re.finditer(r"```mermaid\n(.*?)```", src, re.S)):
                if "\\(" in m.group(1):
                    continue
                add(f"{prefix}-{f}-{i}", m.group(1))

fences(os.path.join(root, "vendor/downright"), "downright")
fences(os.path.join(root, "corpus/generated"), "generated")

for x in json.loads(subprocess.check_output([sys.executable, os.path.join(here, "handwritten.py")])):
    add(x["name"], x["source"])
print(json.dumps(out, indent=1))
