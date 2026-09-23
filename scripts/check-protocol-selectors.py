#!/usr/bin/env python3
"""Checks that every Objective-C method a `define_class!` block declares
inside `unsafe impl <Protocol> for <Class> { … }` is a selector of that
protocol in the objc2 bindings (a misspelt delegate selector panics in
debug builds and is silently never called in release).

  scripts/check-protocol-selectors.py [paths…]   (default: crates/app/src)
"""
import glob, os, re, sys

REGISTRY = os.path.expanduser("~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f")
protocols = {}
for crate in ("objc2-app-kit-0.3.2", "objc2-foundation-0.3.2", "objc2-quartz-core-0.3.2"):
    for path in glob.glob(os.path.join(REGISTRY, crate, "src/generated/*.rs")):
        text = open(path).read()
        for block in re.finditer(r"extern_protocol!\((.*?)\n\);", text, re.S):
            body = block.group(1)
            name = re.search(r"pub unsafe trait (\w+)", body)
            if name:
                protocols.setdefault(name.group(1), set()).update(
                    re.findall(r"#\[unsafe\(method\(([^)]*)\)\)\]", body))

roots = sys.argv[1:] or ["crates/app/src"]
bad = 0
for root in roots:
    for path in glob.glob(os.path.join(root, "**/*.rs"), recursive=True):
        text = open(path).read()
        for block in re.finditer(r"unsafe impl (\w+) for (\w+) \{\n(.*?)\n    \}", text, re.S):
            protocol, cls, body = block.groups()
            if protocol not in protocols or protocol == "NSObjectProtocol":
                continue
            for selector in re.findall(r"#\[unsafe\(method(?:_id)?\(([^)]*)\)\)\]", body):
                if selector not in protocols[protocol]:
                    print(f"{path}: {cls} impl {protocol}: `{selector}` is not a {protocol} selector")
                    bad += 1
print(f"{bad} unknown protocol selector(s)")
sys.exit(1 if bad else 0)
