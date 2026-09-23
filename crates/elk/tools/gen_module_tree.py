#!/usr/bin/env python3
"""Creates the module skeleton: one Rust module per elk-swift source file,
mirroring the org/eclipse/elk package tree. Existing files are never
overwritten; missing ones get a stub header. Writes tools/modules.tsv mapping
each Swift file to its Rust module and line count."""
import os, re, sys

here = os.path.dirname(os.path.abspath(__file__))
crate = os.path.dirname(here)
swift_root = os.path.join(crate, "../../vendor/elk-swift/Sources/ElkSwift")

def snake(name):
    s = re.sub(r'([A-Z]+)([A-Z][a-z])', r'\1_\2', name)
    s = re.sub(r'([a-z0-9])([A-Z])', r'\1_\2', s)
    return s.lower()

KEYWORDS = {"type", "mod", "use", "impl", "match", "move", "ref", "self", "super", "crate", "loop", "where"}

entries = []
for dirpath, _, files in os.walk(swift_root):
    for f in sorted(files):
        if not f.endswith(".swift"):
            continue
        rel_dir = os.path.relpath(dirpath, swift_root)
        path = os.path.join(dirpath, f)
        base = f[:-6]
        pkg = None
        if rel_dir.startswith("ELK/org/eclipse/elk"):
            pkg = rel_dir[len("ELK/"):]
            prefix = pkg.replace("/", "_") + "_"
            if base.startswith(prefix):
                base = base[len(prefix):]
            rust_dir = os.path.join("src", pkg)
        elif rel_dir == "Bridge":
            rust_dir = "src/bridge"
        else:
            rust_dir = "src"
        mod = snake(base)
        if mod in KEYWORDS:
            mod += "_"
        lines = sum(1 for _ in open(path))
        entries.append((os.path.relpath(path, os.path.join(crate, "../..")), os.path.join(rust_dir, mod + ".rs"), lines))

# write stubs
dirs = {}
for swift, rs, lines in entries:
    full = os.path.join(crate, rs)
    os.makedirs(os.path.dirname(full), exist_ok=True)
    d, m = os.path.split(rs)
    dirs.setdefault(d, set()).add(m[:-3])
    if not os.path.exists(full):
        open(full, "w").write(f"//! Port of `{swift}`.\n//!\n//! Not ported yet.\n")

# every directory between src and a leaf needs a mod.rs listing children
all_dirs = set()
for d in list(dirs):
    parts = d.split("/")
    for i in range(1, len(parts) + 1):
        all_dirs.add("/".join(parts[:i]))
children = {d: set() for d in all_dirs}
for d in all_dirs:
    parent = os.path.dirname(d)
    if parent in children:
        children[parent].add(os.path.basename(d))
for d in sorted(all_dirs):
    if d == "src":
        continue
    modfile = os.path.join(crate, d, "mod.rs")
    extra = {f[:-3] for f in os.listdir(os.path.join(crate, d)) if f.endswith(".rs") and f != "mod.rs"}
    mods = sorted(dirs.get(d, set()) | children[d] | extra)
    body = "".join(f"pub mod {m};\n" for m in mods)
    header = f"//! `{d[4:].replace('/', '.')}`\n\n"
    existing = open(modfile).read() if os.path.exists(modfile) else None
    # Regenerate only the `pub mod` lines; keep anything else a porter added.
    if existing is None:
        open(modfile, "w").write(header + body)
    else:
        kept = [l for l in existing.splitlines(True) if not l.startswith("pub mod ")]
        open(modfile, "w").write("".join(kept).rstrip("\n") + "\n\n" + body if kept else header + body)

with open(os.path.join(here, "modules.tsv"), "w") as out:
    out.write("swift\trust\tlines\n")
    for swift, rs, lines in sorted(entries):
        out.write(f"{swift}\t{rs}\t{lines}\n")
print(len(entries), "modules;", "top-level:", sorted(children.get("src", set())), sorted(dirs.get("src", set())))
