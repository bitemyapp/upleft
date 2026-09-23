#!/usr/bin/env python3
"""Builds the machine-extracted part of corpus/mermaid/: every gen-*.mmd and
bms-*.mmd file. Deterministic: the same corpus/generated contents and the same
pinned vendor/ submodule revisions always produce the same files.

Run `python3 scripts/build-corpus.py` first so corpus/generated exists.

corpus/mermaid/ also holds hand-written hand-*.mmd and bad-*.mmd files that
this script never touches: it only ever creates, rewrites, or deletes files
whose name starts with "gen-" or "bms-".

Sources, one function each below:

  gen-<slug>.mmd
      every ```mermaid / ~~~mermaid fenced code block in the Markdown files
      under corpus/generated, and in every *.md file under vendor/downright
      (skipping node_modules/ and .build/). Identical bodies are deduplicated
      (first occurrence, in file-then-block order, wins the slug).

  bms-json-<id>.mmd
      diagrams[].source in
      vendor/beautiful-mermaid-swift/Examples/MermaidPlayground/Resources/test-diagrams.json

  bms-sample-<name>.mmd
      every `public static let <name> = ` multi-line string literal in
      vendor/beautiful-mermaid-swift/Examples/MermaidPlayground/Models/SampleDiagrams.swift

  bms-test-<file>-<n>.mmd
      every string literal (multi-line triple-quoted or single-line) in
      vendor/beautiful-mermaid-swift/Tests/BeautifulMermaidSwiftTests/*.swift
      that is plausibly a diagram (see is_plausible_diagram below), numbered
      in file order, one counter per source file.

  bms-test-downright-<n>.mmd
      the same extraction over every *.swift file under
      vendor/downright/Tests, numbered by a single counter across the whole
      tree in (sorted path, in-file order).

Swift string literals are unescaped per the task's literal rules: a
backslash-backslash escape becomes a backslash, a backslash-quote becomes a
quote, a backslash-n escape becomes a newline, a backslash-t escape becomes a
tab; a multi-line literal's closing triple-quote line indentation is stripped
from every body line; literals containing backslash-paren interpolation are
skipped; raw strings (hash-quote ... quote-hash) are skipped entirely.
"""

import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CORPUS_MERMAID = os.path.join(ROOT, "corpus", "mermaid")
GENERATED = os.path.join(ROOT, "corpus", "generated")
VENDOR = os.path.join(ROOT, "vendor")
BMS = os.path.join(VENDOR, "beautiful-mermaid-swift")
DOWNRIGHT = os.path.join(VENDOR, "downright")

GENERATED_PREFIXES = ("gen-", "bms-")

# Order matters: longer, more specific prefixes must be tried before "graph"
# would (harmlessly) also match "graph" is not a prefix of any other keyword,
# but keep the list exactly as specified.
DIAGRAM_KEYWORDS = [
    "graph",
    "flowchart",
    "sequenceDiagram",
    "classDiagram",
    "erDiagram",
    "stateDiagram",
    "xychart",
]


def sanitize(text):
    text = text.lower()
    text = re.sub(r"[^a-z0-9]+", "-", text)
    text = text.strip("-")
    return text or "x"


def add_unique(results, name, body):
    """Insert body under name, disambiguating with -2, -3, ... on collision."""
    if name not in results:
        results[name] = body
        return name
    stem, ext = os.path.splitext(name)
    n = 2
    while f"{stem}-{n}{ext}" in results:
        n += 1
    unique = f"{stem}-{n}{ext}"
    results[unique] = body
    return unique


def is_plausible_diagram(text):
    """True if, after trimming, text starts with a diagram keyword at a word
    boundary and carries something beyond the bare keyword itself. This rules
    out both stray identifiers that merely share the keyword's first letters
    (dictionary keys like "graphWidth"/"graphHeight" in the verification
    exporter tests) and bare category labels ("flowchart", "xychart") that
    show up as Swift identifiers/switch-cases elsewhere, none of which are
    diagram sources."""
    trimmed = text.strip()
    lowered = trimmed.lower()
    for keyword in DIAGRAM_KEYWORDS:
        klen = len(keyword)
        if not lowered.startswith(keyword.lower()) or len(trimmed) <= klen:
            continue
        if not trimmed[klen].isalnum():
            return True
    return False


# --------------------------------------------------------------------------
# 1. gen-<slug>.mmd: fenced ```mermaid blocks in Markdown
# --------------------------------------------------------------------------

FENCE_OPEN_RE = re.compile(r"^( {0,3})(`{3,}|~{3,})[ \t]*(.*)$")


def iter_fenced_mermaid_blocks(text):
    """Yields fenced code block bodies (joined by \\n) whose info string's
    first word is "mermaid" (case-insensitive). Backtick or tilde fences of
    length >= 3; up to 3 spaces of the opening fence's indentation are
    stripped from each body line, CommonMark-style. An unterminated fence
    runs to the end of the document."""
    lines = text.split("\n")
    n = len(lines)
    i = 0
    while i < n:
        m = FENCE_OPEN_RE.match(lines[i])
        if not m:
            i += 1
            continue
        indent, marker, info = m.group(1), m.group(2), m.group(3).strip()
        fence_char = marker[0]
        fence_len = len(marker)
        if fence_char == "`" and "`" in info:
            i += 1
            continue
        first_word = info.split()[0] if info.split() else ""
        is_mermaid = first_word.lower() == "mermaid"
        body_lines = []
        j = i + 1
        closed = False
        while j < n:
            cm = re.match(r"^( {0,3})(`{3,}|~{3,})[ \t]*$", lines[j])
            if cm and cm.group(2)[0] == fence_char and len(cm.group(2)) >= fence_len:
                closed = True
                break
            body_lines.append(lines[j])
            j += 1
        if is_mermaid:
            indent_n = len(indent)
            stripped = []
            for bl in body_lines:
                k = 0
                while k < indent_n and k < len(bl) and bl[k] == " ":
                    k += 1
                stripped.append(bl[k:])
            yield "\n".join(stripped)
        i = (j + 1) if closed else n


def gather_markdown_files():
    files = []
    for directory, subdirs, filenames in os.walk(GENERATED):
        subdirs.sort()
        for name in sorted(filenames):
            if name.endswith(".md"):
                files.append(os.path.join(directory, name))
    for directory, subdirs, filenames in os.walk(DOWNRIGHT):
        subdirs[:] = sorted(
            d for d in subdirs if d not in ("node_modules", ".build") and not d.startswith(".git")
        )
        for name in sorted(filenames):
            if name.endswith(".md"):
                files.append(os.path.join(directory, name))
    return files


def gen_files():
    results = {}
    seen_bodies = set()
    for path in gather_markdown_files():
        stem = sanitize(os.path.splitext(os.path.basename(path))[0])
        text = open(path, encoding="utf-8").read()
        index = 0
        for body in iter_fenced_mermaid_blocks(text):
            index += 1
            if body in seen_bodies:
                continue
            seen_bodies.add(body)
            add_unique(results, f"gen-{stem}-{index}.mmd", body)
    return results


# --------------------------------------------------------------------------
# Swift string literal helpers, shared by items 2-5
# --------------------------------------------------------------------------

SWIFT_ESCAPES = {"n": "\n", "t": "\t", '"': '"', "\\": "\\"}


def swift_unescape(raw):
    """Applies exactly the task's four escapes: \\\\ -> \\, \\" -> ", \\n -> \\n,
    \\t -> tab. Returns None if \\( interpolation is present."""
    if "\\(" in raw:
        return None
    out = []
    i = 0
    n = len(raw)
    while i < n:
        c = raw[i]
        if c == "\\" and i + 1 < n and raw[i + 1] in SWIFT_ESCAPES:
            out.append(SWIFT_ESCAPES[raw[i + 1]])
            i += 2
            continue
        out.append(c)
        i += 1
    return "".join(out)


def dedent_swift(body, indent):
    """Strips the closing delimiter's indentation from every line of a
    multi-line literal's body. Returns None if a non-blank line is indented
    less than the closing delimiter (a malformed/unexpected literal)."""
    stripped = []
    for line in body.split("\n"):
        if line.strip() == "":
            stripped.append("")
        elif line.startswith(indent):
            stripped.append(line[len(indent):])
        else:
            return None
    return "\n".join(stripped)


TRIPLE_BODY_RE = re.compile(r'"""[ \t]*\n(.*?)\n([ \t]*)"""', re.S)


def find_swift_string_literals(text):
    """Yields (start, end, value_or_None) for every string literal in Swift
    source, in source order: multi-line \"\"\"...\"\"\" and single-line
    "...". Skips // line comments, /* */ block comments, and raw strings
    (#"..."#, ##"..."##, ...) entirely. value is None when the literal
    contains \\( interpolation or is otherwise not decodable."""
    results = []
    i = 0
    n = len(text)
    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            j = text.find("\n", i)
            i = n if j == -1 else j
            continue
        if c == "/" and i + 1 < n and text[i + 1] == "*":
            j = text.find("*/", i + 2)
            i = n if j == -1 else j + 2
            continue
        if c == "#":
            j = i
            while j < n and text[j] == "#":
                j += 1
            if j < n and text[j] == '"':
                hashes = j - i
                closer = '"' + ("#" * hashes)
                end = text.find(closer, j + 1)
                i = n if end == -1 else end + len(closer)
                continue
            i += 1
            continue
        if c == '"':
            if text[i:i + 3] == '"""':
                m = TRIPLE_BODY_RE.match(text, i)
                if m:
                    dedented = dedent_swift(m.group(1), m.group(2))
                    value = swift_unescape(dedented) if dedented is not None else None
                    results.append((m.start(), m.end(), value))
                    i = m.end()
                    continue
                i += 3
                continue
            j = i + 1
            buf = []
            interpolation = False
            closed = False
            while j < n:
                ch = text[j]
                if ch == "\n":
                    break
                if ch == "\\" and j + 1 < n:
                    nxt = text[j + 1]
                    if nxt == "(":
                        interpolation = True
                        buf.append("\\(")
                        j += 2
                        continue
                    if nxt in SWIFT_ESCAPES:
                        buf.append("\\" + nxt)
                        j += 2
                        continue
                    buf.append(ch)
                    j += 1
                    continue
                if ch == '"':
                    closed = True
                    break
                buf.append(ch)
                j += 1
            if closed:
                raw = "".join(buf)
                value = None if interpolation else swift_unescape(raw)
                results.append((i, j + 1, value))
                i = j + 1
                continue
            i = j
            continue
        i += 1
    return results


# --------------------------------------------------------------------------
# 2. bms-json-<id>.mmd
# --------------------------------------------------------------------------

def bms_json_files():
    path = os.path.join(
        BMS, "Examples", "MermaidPlayground", "Resources", "test-diagrams.json"
    )
    data = json.load(open(path, encoding="utf-8"))
    results = {}
    for entry in data["diagrams"]:
        add_unique(results, f"bms-json-{sanitize(entry['id'])}.mmd", entry["source"])
    return results


# --------------------------------------------------------------------------
# 3. bms-sample-<name>.mmd
# --------------------------------------------------------------------------

LET_NAME_RE = re.compile(r"public static let (\w+)\s*=\s*$")


def bms_sample_files():
    path = os.path.join(
        BMS, "Examples", "MermaidPlayground", "Models", "SampleDiagrams.swift"
    )
    text = open(path, encoding="utf-8").read()
    results = {}
    for start, end, value in find_swift_string_literals(text):
        if value is None:
            continue
        if text[start:start + 3] != '"""':
            continue
        m = LET_NAME_RE.search(text[:start])
        if not m:
            continue
        add_unique(results, f"bms-sample-{sanitize(m.group(1))}.mmd", value)
    return results


# --------------------------------------------------------------------------
# 4. bms-test-<file>-<n>.mmd
# --------------------------------------------------------------------------

def bms_test_files():
    directory = os.path.join(BMS, "Tests", "BeautifulMermaidSwiftTests")
    results = {}
    for name in sorted(os.listdir(directory)):
        if not name.endswith(".swift"):
            continue
        path = os.path.join(directory, name)
        text = open(path, encoding="utf-8").read()
        stem = sanitize(os.path.splitext(name)[0])
        index = 0
        for start, end, value in find_swift_string_literals(text):
            if value is None or not is_plausible_diagram(value):
                continue
            index += 1
            add_unique(results, f"bms-test-{stem}-{index}.mmd", value)
    return results


# --------------------------------------------------------------------------
# 5. bms-test-downright-<n>.mmd
# --------------------------------------------------------------------------

def bms_test_downright_files():
    tests_dir = os.path.join(DOWNRIGHT, "Tests")
    paths = []
    for directory, subdirs, filenames in os.walk(tests_dir):
        subdirs.sort()
        for name in sorted(filenames):
            if name.endswith(".swift"):
                paths.append(os.path.join(directory, name))
    results = {}
    index = 0
    for path in paths:
        text = open(path, encoding="utf-8").read()
        for start, end, value in find_swift_string_literals(text):
            if value is None or not is_plausible_diagram(value):
                continue
            index += 1
            add_unique(results, f"bms-test-downright-{index}.mmd", value)
    return results


# --------------------------------------------------------------------------
# main
# --------------------------------------------------------------------------

def normalize(body):
    """Diagram text verbatim, ending with exactly one trailing newline."""
    return body.rstrip("\n") + "\n"


def main():
    if not os.path.isdir(GENERATED):
        sys.exit("corpus/generated is missing; run `python3 scripts/build-corpus.py` first")
    if not os.path.isfile(os.path.join(DOWNRIGHT, "Package.swift")):
        sys.exit("vendor/downright is missing; run `git submodule update --init`")
    if not os.path.isdir(os.path.join(BMS, "Sources")):
        sys.exit("vendor/beautiful-mermaid-swift is missing; run `git submodule update --init`")

    os.makedirs(CORPUS_MERMAID, exist_ok=True)

    all_results = {}
    counts = {}
    for label, builder in (
        ("gen-", gen_files),
        ("bms-json-", bms_json_files),
        ("bms-sample-", bms_sample_files),
        ("bms-test-downright-", bms_test_downright_files),
        ("bms-test-", bms_test_files),
    ):
        produced = builder()
        counts[label] = len(produced)
        for name, body in produced.items():
            if name in all_results:
                sys.exit(f"internal error: duplicate output filename {name}")
            all_results[name] = body

    existing = {
        name
        for name in os.listdir(CORPUS_MERMAID)
        if name.startswith(GENERATED_PREFIXES) and name.endswith(".mmd")
    }

    for name, body in all_results.items():
        text = normalize(body)
        text.encode("utf-8")  # fail loudly on anything not valid Unicode
        with open(os.path.join(CORPUS_MERMAID, name), "w", encoding="utf-8", newline="\n") as handle:
            handle.write(text)

    stale = existing - set(all_results)
    for name in stale:
        os.remove(os.path.join(CORPUS_MERMAID, name))

    for label in ("gen-", "bms-json-", "bms-sample-", "bms-test-downright-", "bms-test-"):
        print(f"{label}: {counts[label]} files")
    print(f"total generated: {len(all_results)} files ({len(stale)} stale files removed)")


if __name__ == "__main__":
    main()
