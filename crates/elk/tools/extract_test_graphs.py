#!/usr/bin/env python3
"""Extracts the literal ELK graphs (`let graph: [String: Any] = [...]`) from
elk-swift's tests into corpus/elk/elk-swift-tests/<Test>.<func>.json.
Swift integer literals become JSON doubles, as the oracle reads all numbers
as Double."""
import json, os, re, sys

here = os.path.dirname(os.path.abspath(__file__))
root = os.path.normpath(os.path.join(here, "../../.."))
tests = os.path.join(root, "vendor/elk-swift/Tests/ElkSwiftTests")
out = os.path.join(root, "corpus/elk/elk-swift-tests")

def tokenize(s):
    i = 0
    toks = []
    while i < len(s):
        c = s[i]
        if c.isspace():
            i += 1
        elif s.startswith("//", i):
            i = s.index("\n", i) if "\n" in s[i:] else len(s)
        elif c == '"':
            j = i + 1
            buf = ""
            while s[j] != '"':
                if s[j] == "\\":
                    buf += {"n": "\n", "t": "\t", '"': '"', "\\": "\\"}[s[j + 1]]
                    j += 2
                else:
                    buf += s[j]
                    j += 1
            toks.append(("str", buf))
            i = j + 1
        elif c in "[],:":
            toks.append((c, c))
            i += 1
        else:
            m = re.match(r"-?\d+(\.\d+)?([eE][-+]?\d+)?", s[i:])
            if m:
                toks.append(("num", float(m.group(0))))
                i += len(m.group(0))
                continue
            m = re.match(r"true|false", s[i:])
            if m:
                toks.append(("bool", m.group(0) == "true"))
                i += len(m.group(0))
                continue
            m = re.match(r"as\s*\[String\s*:\s*Any\]", s[i:])
            if m:
                i += len(m.group(0))
                continue
            raise ValueError("unexpected at: " + s[i:i + 40])
    return toks

def parse(toks, i):
    kind, val = toks[i]
    if kind in ("str", "num", "bool"):
        return val, i + 1
    assert kind == "["
    i += 1
    if toks[i][0] == ":":  # [:]
        return {}, i + 2
    items = []
    is_dict = False
    while toks[i][0] != "]":
        v, i = parse(toks, i)
        if toks[i][0] == ":":
            is_dict = True
            w, i = parse(toks, i + 1)
            items.append((v, w))
        else:
            items.append(v)
        if toks[i][0] == ",":
            i += 1
    i += 1
    return (dict(items) if is_dict else items), i

count = 0

def save(name, graph):
    global count
    json.dump(graph, open(os.path.join(out, name), "w"), indent=1)
    count += 1

for fname in sorted(os.listdir(tests)):
    if not fname.endswith(".swift"):
        continue
    src = open(os.path.join(tests, fname)).read()
    test = fname[:-6]
    for m in re.finditer(r"func (test\w+)\(\)[^{]*\{|let graph: \[String: Any\] = \[", src):
        if m.group(1):
            func = m.group(1)
            continue
        start = m.end() - 1
        depth = 0
        for j in range(start, len(src)):
            if src[j] == "[":
                depth += 1
            elif src[j] == "]":
                depth -= 1
                if depth == 0:
                    break
        literal = src[start:j + 1]
        # `String(spacing)` with `let spacing = X` earlier in the test.
        for var, value in re.findall(r"let (\w+) = ([0-9.]+)\n", src[:start]):
            literal = literal.replace(f"String({var})", '"' + str(float(value)) + '"')
        if "\\(" in literal:
            continue  # interpolated
        try:
            graph, _ = parse(tokenize(literal), 0)
        except Exception as e:
            print("skip", test, func, e, file=sys.stderr)
            continue
        save(f"{test}.{func}.json", graph)
    # Graph factories: `private static func name() -> [String: Any] { [ ... ] as [String: Any] }`
    for m in re.finditer(r"static func (\w+)\(\) -> \[String: Any\] \{\s*\[", src):
        start = m.end() - 1
        depth = 0
        for j in range(start, len(src)):
            if src[j] == "[":
                depth += 1
            elif src[j] == "]":
                depth -= 1
                if depth == 0:
                    break
        graph, _ = parse(tokenize(src[start:j + 1]), 0)
        save(f"{test}.{m.group(1)}.json", graph)
    # JSON string fixtures: `static let nameJson = """ {...} """`
    for m in re.finditer(r'static let (\w+)Json = """\n(.*?)\n\s*"""', src, re.S):
        graph = json.loads(m.group(2))
        def doubles(v):
            if isinstance(v, dict): return {k: doubles(x) for k, x in v.items()}
            if isinstance(v, list): return [doubles(x) for x in v]
            if isinstance(v, int) and not isinstance(v, bool): return float(v)
            return v
        save(f"{test}.{m.group(1)}.json", doubles(graph))
print(count, "graphs")
