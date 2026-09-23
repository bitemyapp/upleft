#!/usr/bin/env python3
"""Writes corpus/render-state/*.json: the `render-state` scenarios.

Each scenario renders one document through the real text view after driving
it into a state with the renderer's public surface (see
oracle/Sources/downright-oracle/RenderScenario.swift and
crates/conformance/src/dump/render_state.rs). The set covers what the plain
`render` suite (top of the document, initial state) cannot: every theme and
appearance, Read mode, scroll positions below the fold, narrow and wide
measures, render configuration, search hits, change marks, speech highlight,
selection, source focus, folding, structural zoom, collapsed code, streamed
appends (the Omperor use case) and in-place edits, and motion with Reduce
Motion off.

Heading slugs and code-block offsets come from `upleft-oracle parse`, whose
output is conformance-checked against Downright's parser. Every range is UTF-16
and refers to the final text. Requires `just corpus` and a release
upleft-oracle. Deterministic.
"""

import json
import os
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "corpus", "render-state")
ORACLE = os.path.join(ROOT, "target", "release", "upleft-oracle")

THEMES = ["Paper Light", "Nord", "Solarized Light", "High Contrast", "System", "Warm Dark"]


def u16(text):
    return len(text.encode("utf-16-le")) // 2


def u16_offset(text, index):
    return u16(text[:index])


def read(relative):
    with open(os.path.join(ROOT, relative), encoding="utf-8") as handle:
        text = handle.read()
    return text[1:] if text.startswith("﻿") else text


def parse(relative):
    with tempfile.NamedTemporaryFile(suffix=".json", delete=False) as handle:
        output = handle.name
    subprocess.run([ORACLE, "parse", os.path.join(ROOT, relative), output], check=True)
    with open(output, encoding="utf-8") as handle:
        dump = json.load(handle)
    os.remove(output)
    return dump


def blocks(node, kind):
    found = []
    if node["content"]["kind"] == kind:
        found.append(node)
    for child in node["children"]:
        found.extend(blocks(child, kind))
    return found


def occurrences(text, needle, limit=None):
    hits = []
    start = 0
    while True:
        index = text.find(needle, start)
        if index < 0:
            break
        hits.append([u16_offset(text, index), u16(needle)])
        start = index + len(needle)
        if limit and len(hits) >= limit:
            break
    return hits


SCENARIOS = {}


def scenario(name, document, **fields):
    body = {"document": os.path.relpath(os.path.join(ROOT, document), OUT)}
    body.update(fields)
    SCENARIOS[name] = body


def chunked_appends(text, pieces):
    """Splits `text` at line boundaries into about `pieces` chunks: the first
    is the initial text, the rest are appends, as a streamed reply arrives."""
    lines = text.splitlines(keepends=True)
    size = max(1, len(lines) // pieces)
    chunks = ["".join(lines[i:i + size]) for i in range(0, len(lines), size)]
    initial, rest = chunks[0], chunks[1:]
    edits = []
    length = u16(initial)
    for chunk in rest:
        edits.append({"location": length, "length": 0, "text": chunk})
        length += u16(chunk)
    return initial, edits


def token_appends(text, count):
    """A stream that arrives mid-line: `count` appends cut at fixed character
    counts, so fences, tables and math open before they close."""
    step = max(1, len(text) // count)
    cuts = list(range(step, len(text), step))
    initial = text[:cuts[0]] if cuts else text
    edits = []
    previous = len(initial)
    length = u16(initial)
    for cut in cuts[1:] + [len(text)]:
        piece = text[previous:cut]
        if piece:
            edits.append({"location": length, "length": 0, "text": piece})
            length += u16(piece)
        previous = cut
    return initial, edits


def main():
    if not os.path.exists(ORACLE):
        sys.exit("target/release/upleft-oracle is missing; run `cargo build --release -p upleft-conformance`")
    sample = "corpus/generated/docs/Docs__sample.md"
    readme = "corpus/generated/docs/README.md"
    welcome = "corpus/generated/docs/Resources__Welcome.md"
    matrix = "corpus/generated/docs/Docs__FEATURE-MATRIX.md"
    math = "corpus/generated/docs/Vendor__SwiftMath__EXAMPLES.md"
    agent400 = "corpus/generated/agent/agent-400.md"
    agent5000 = "corpus/generated/agent/agent-5000.md"
    images = "corpus/render-images/mixed.markdown"
    for required in (sample, readme, welcome, matrix, math, agent400, agent5000, images):
        if not os.path.exists(os.path.join(ROOT, required)):
            sys.exit(f"{required} is missing; run `just corpus` (and scripts/build-render-images-corpus.py)")

    # Themes and appearances, including the three the render suites never draw.
    for theme in THEMES:
        slug = theme.lower().replace(" ", "-")
        scenario(f"theme-{slug}-light", sample, theme=theme, dark=False)
        scenario(f"theme-{slug}-dark", sample, theme=theme, dark=True)

    # Read mode (the render suites use Live and Source).
    for name, document in (("sample", sample), ("readme", readme), ("matrix", matrix), ("math", math), ("images", images)):
        scenario(f"read-{name}", document, mode="read")
    scenario("read-sample-dark", sample, mode="read", dark=True, theme="Warm Dark")

    # Below the fold: the render suites only ever see the top of a document.
    for name, document in (("sample", sample), ("readme", readme), ("agent400", agent400), ("agent5000", agent5000), ("matrix", matrix)):
        text = read(document)
        length = u16(text)
        scenario(f"scroll-middle-{name}", document, scroll={"offset": length // 2, "position": "top"})
        scenario(f"scroll-end-{name}", document, scroll={"offset": length, "position": "top"})
    scenario("scroll-center-readme-dark", readme, dark=True, scroll={"offset": u16(read(readme)) // 3, "position": "center"})

    # Measures: narrow and wide windows.
    for name, document in (("sample", sample), ("readme", readme), ("matrix", matrix)):
        scenario(f"narrow-{name}", document, width=560, height=900)
        scenario(f"wide-{name}", document, width=1600, height=1000)

    # Render configuration.
    for name, document in (("sample", sample), ("readme", readme)):
        scenario(f"invisibles-{name}", document, configuration={"showInvisibles": True})
        scenario(f"no-reflow-{name}", document, configuration={"reflowHardWrappedParagraphs": False})
        scenario(f"typographic-{name}", document, configuration={"typographicSubstitution": True})
    scenario("invisibles-source-sample", sample, mode="source", configuration={"showInvisibles": True})
    scenario("reveal-never-sample", sample, configuration={"revealPolicy": "never"})
    scenario("collapse-threshold-3-sample", sample, configuration={"codeCollapseThreshold": 3})
    scenario("collapse-threshold-3-read-agent400", agent400, mode="read", configuration={"codeCollapseThreshold": 3})

    # Collapsed and expanded code blocks by offset.
    for name, document in (("sample", sample), ("agent400", agent400)):
        codes = blocks(parse(document)["root"], "codeBlock")
        if codes:
            first = codes[0]["range"][0]
            scenario(f"collapse-code-{name}", document, collapseCode=[{"offset": first, "collapsed": True}])
            scenario(f"expand-code-read-{name}", document, mode="read",
                     configuration={"codeCollapseThreshold": 1},
                     collapseCode=[{"offset": first, "collapsed": False}])

    # Search hits, the current hit, speech highlight, selection.
    for name, document, needle in (("sample", sample, "the"), ("readme", readme, "Markdown"), ("agent400", agent400, "task")):
        text = read(document)
        hits = occurrences(text, needle, limit=200)
        scenario(f"search-{name}", document, searchHits=hits, currentSearchHit=hits[min(2, len(hits) - 1)])
        scenario(f"search-{name}-dark", document, dark=True, searchHits=hits, currentSearchHit=hits[0])
    for name, document in (("sample", sample), ("readme", readme)):
        text = read(document)
        index = text.find(" ", len(text) // 5)
        scenario(f"speech-{name}", document, speechHighlight=[u16_offset(text, index + 1), 6])
        scenario(f"selection-{name}", document, selection=[[u16_offset(text, len(text) // 6), 120]])
    text = read(sample)
    scenario("selection-multiple-sample", sample,
             selection=[[u16_offset(text, 10), 20], [u16_offset(text, len(text) // 3), 40]])

    # Change marks: inserted, modified with word ranges, and a deletion ghost.
    for name, document in (("sample", sample), ("readme", readme), ("agent400", agent400)):
        text = read(document)
        para = text.find("\n\n", len(text) // 4) + 2
        end = text.find("\n", para)
        first_space = text.find(" ", para)
        second = text.find("\n\n", end) + 2
        second_end = text.find("\n", second)
        marks = [
            {"kind": "inserted", "range": [u16_offset(text, para), u16(text[para:end])], "words": []},
            {"kind": "modified", "range": [u16_offset(text, second), u16(text[second:second_end])],
             "words": [[u16_offset(text, second), max(1, u16(text[second:text.find(" ", second)]))]]},
            {"kind": "deleted", "range": [u16_offset(text, first_space), 0], "words": [],
             "deletedText": "a sentence an external edit removed"},
        ]
        scenario(f"changes-{name}", document, changeMarks=marks)
        marks_visited = [dict(mark, visited=True) for mark in marks]
        scenario(f"changes-visited-{name}-dark", document, dark=True, changeMarks=marks_visited)

    # Source focus: a range shown as flat monospaced source in Live mode.
    for name, document in (("sample", sample), ("readme", readme)):
        codes = blocks(parse(document)["root"], "paragraph")
        target = codes[min(2, len(codes) - 1)]["range"]
        scenario(f"focus-source-{name}", document, focusSource=target)

    # Folding and structural zoom.
    for name, document in (("sample", sample), ("readme", readme), ("agent400", agent400)):
        headings = parse(document)["headings"]
        if len(headings) > 1:
            scenario(f"fold-{name}", document, foldedHeadings=[headings[1]["slug"]])
        for level, label in ((1, "h1"), (2, "h2"), (3, "headings"), (4, "skeleton")):
            scenario(f"zoom-{label}-{name}", document, zoom=level)

    # Streams: appends at line boundaries, and token-sized appends that leave
    # fences, tables, math and diagrams open mid-stream (the Omperor case).
    for name, document, pieces in (("sample", sample, 12), ("readme", readme, 10), ("agent400", agent400, 8),
                                   ("math", math, 8), ("matrix", matrix, 6), ("welcome", welcome, 8)):
        initial, edits = chunked_appends(read(document), pieces)
        scenario(f"stream-lines-{name}", document, initialText=initial, edits=edits)
    for name, document, count in (("sample", sample, 60), ("readme", readme, 40), ("math", math, 40)):
        initial, edits = token_appends(read(document), count)
        scenario(f"stream-tokens-{name}", document, initialText=initial, edits=edits)
    initial, edits = token_appends(read(sample), 30)
    scenario("stream-tokens-sample-source", sample, mode="source", initialText=initial, edits=edits)
    scenario("stream-tokens-sample-dark", sample, dark=True, theme="Warm Dark", initialText=initial, edits=edits)
    initial, edits = chunked_appends(read(agent5000), 20)
    scenario("stream-lines-agent5000-end", agent5000, initialText=initial, edits=edits,
             scroll={"offset": u16(read(agent5000)), "position": "top"})

    # In-place edits: typing, deleting a block, retitling a heading, breaking
    # and restoring a fence. Ranges are computed step by step on the text as
    # it stands before each edit.
    text = read(sample)
    edits = []
    working = text
    def replace(start, length, replacement):
        nonlocal_edits = {"location": u16_offset(working, start), "length": u16(working[start:start + length]), "text": replacement}
        edits.append(nonlocal_edits)
        return working[:start] + replacement + working[start + length:]
    anchor = working.find("\n\n", len(working) // 3) + 2
    working = replace(anchor, 0, "Freshly typed words land here. ")
    heading = working.find("\n## ")
    if heading >= 0:
        line_end = working.find("\n", heading + 1)
        working = replace(heading + 4, line_end - (heading + 4), "A retitled section")
    fence = working.find("```")
    if fence >= 0:
        working = replace(fence, 3, "``")
        working = replace(fence, 2, "```")
    block_start = working.find("\n\n", len(working) // 2) + 2
    block_end = working.find("\n\n", block_start)
    if block_end > block_start:
        working = replace(block_start, block_end - block_start + 2, "")
    scenario("edits-sample", sample, edits=edits)
    scenario("edits-sample-source", sample, mode="source", edits=edits)

    # Motion with Reduce Motion off (the harness otherwise forces it on).
    scenario("motion-sample", sample, reduceMotion=False)
    scenario("motion-sample-dark", sample, reduceMotion=False, dark=True)
    scenario("motion-zoom-readme", readme, reduceMotion=False, zoom=3)

    os.makedirs(OUT, exist_ok=True)
    for name in os.listdir(OUT):
        if name.endswith(".json") and name[:-5] not in SCENARIOS:
            os.remove(os.path.join(OUT, name))
    for name, body in sorted(SCENARIOS.items()):
        with open(os.path.join(OUT, f"{name}.json"), "w", encoding="utf-8") as handle:
            json.dump(body, handle, indent=1, ensure_ascii=False)
            handle.write("\n")
    print(f"corpus/render-state: {len(SCENARIOS)} scenarios")


if __name__ == "__main__":
    main()
