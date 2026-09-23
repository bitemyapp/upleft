#!/usr/bin/env python3
"""Structural diff of two JSON files (Swift first, Rust second); a debugging aid for the panel suites."""
import json, sys

def diff(x, y, path, out):
    if type(x) != type(y):
        out.append(f"{path}: type {repr(x)[:90]} != {repr(y)[:90]}"); return
    if isinstance(x, dict):
        for k in list(x) + [k for k in y if k not in x]:
            if k not in x or k not in y:
                out.append(f"{path}/{k}: missing on {'rust' if k in x else 'swift'}"); continue
            diff(x[k], y[k], f"{path}/{k}", out)
    elif isinstance(x, list):
        if len(x) != len(y):
            out.append(f"{path}: length {len(x)} != {len(y)}")
        for i, (a, b) in enumerate(zip(x, y)):
            diff(a, b, f"{path}/{i}", out)
    elif x != y:
        out.append(f"{path}: {repr(x)[:100]} != {repr(y)[:100]}")

out = []
diff(json.load(open(sys.argv[1])), json.load(open(sys.argv[2])), "", out)
limit = int(sys.argv[3]) if len(sys.argv) > 3 else 30
for line in out[:limit]:
    print(line)
print(f"{len(out)} difference(s)")
