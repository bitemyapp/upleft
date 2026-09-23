#!/usr/bin/env python3
"""Builds corpus/generated from the pinned submodules. Deterministic: the same
submodule revisions always produce the same files.

  spec/        every example in cmark-gfm's spec.txt, extensions.txt,
               regression.txt and smart_punct.txt (→ becomes a tab)
  docs/        every Markdown file shipped in the Downright repository
  fixtures/    the multi-line string literals in Downright's Swift test suites
               that contain no interpolation — the documents its own tests
               parse and render
  agent/       drbench's agentDocument(lines:) at several sizes
  math/        every $…$, $$…$$, \\(…\\) and \\[…\\] formula in the documents above,
               as .tex inputs for the math suites (first line: inline|display)
"""

import os
import re
import shutil
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "corpus", "generated")
VENDOR = os.path.join(ROOT, "vendor")


def write(relative, text):
    path = os.path.join(OUT, relative)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="") as handle:
        handle.write(text)


def spec_examples():
    fence = "`" * 32
    for name in ["spec.txt", "extensions.txt", "regression.txt", "smart_punct.txt"]:
        path = os.path.join(VENDOR, "swift-cmark", "test", name)
        lines = open(path, encoding="utf-8").read().split("\n")
        number = 0
        index = 0
        while index < len(lines):
            if lines[index].startswith(fence + " example"):
                number += 1
                body = []
                index += 1
                while not lines[index].startswith("."):
                    body.append(lines[index])
                    index += 1
                markdown = "\n".join(body) + "\n"
                if body == [""]:
                    markdown = ""
                markdown = markdown.replace("→", "\t")
                write(f"spec/{name[:-4]}-{number:04d}.md", markdown)
                while not lines[index].startswith(fence):
                    index += 1
            index += 1


def downright_docs():
    base = os.path.join(VENDOR, "downright")
    for directory, subdirectories, files in os.walk(base):
        subdirectories[:] = sorted(d for d in subdirectories if not d.startswith(".") and d not in ("npm", "node_modules"))
        for name in sorted(files):
            if name.endswith(".md"):
                source = os.path.join(directory, name)
                relative = os.path.relpath(source, base).replace(os.sep, "__")
                shutil.copyfile(source, os.path.join(OUT, "docs", relative))


SWIFT_ESCAPES = {"n": "\n", "t": "\t", "r": "\r", "0": "\0", "\\": "\\", '"': '"', "'": "'"}


def unescape_swift(text):
    """Swift string-literal escapes. Returns None for interpolation."""
    out = []
    index = 0
    while index < len(text):
        character = text[index]
        if character != "\\":
            out.append(character)
            index += 1
            continue
        following = text[index + 1] if index + 1 < len(text) else ""
        if following == "(":
            return None
        if following == "u" and text[index + 2:index + 3] == "{":
            end = text.index("}", index)
            out.append(chr(int(text[index + 3:end], 16)))
            index = end + 1
        elif following == "\n":
            # A backslash at the end of a line joins it to the next.
            index += 2
        elif following in SWIFT_ESCAPES:
            out.append(SWIFT_ESCAPES[following])
            index += 2
        else:
            return None
    return "".join(out)


def multiline_literals(source):
    """Yields the values of `\"\"\"` literals (not raw `#\"\"\"#` ones)."""
    pattern = re.compile(r'(?<!#)"""[ \t]*\n(.*?)\n([ \t]*)"""', re.S)
    for match in pattern.finditer(source):
        body, indent = match.group(1), match.group(2)
        lines = body.split("\n")
        stripped = []
        for line in lines:
            if line.strip() == "":
                stripped.append("")
            elif line.startswith(indent):
                stripped.append(line[len(indent):])
            else:
                break
        else:
            value = unescape_swift("\n".join(stripped))
            if value is not None:
                yield value


def test_fixtures():
    tests = os.path.join(VENDOR, "downright", "Tests")
    seen = set()
    for directory, subdirectories, files in os.walk(tests):
        subdirectories.sort()
        for name in sorted(files):
            if not name.endswith(".swift"):
                continue
            source = open(os.path.join(directory, name), encoding="utf-8").read()
            for number, literal in enumerate(multiline_literals(source), start=1):
                if literal in seen or len(literal.strip()) == 0:
                    continue
                seen.add(literal)
                suite = os.path.basename(directory)
                write(f"fixtures/{suite}__{name[:-6]}-{number:03d}.md", literal)


def agent_document(target_lines):
    """drbench's agentDocument(lines:), character for character."""
    out = []
    line_count = 0
    index = 0
    while line_count < target_lines:
        index += 1
        block = (
            f"## Section {index}\n\n"
            f"A paragraph with **bold**, `code`, a [link](https://example.com), and a\n"
            f"path reference `src/module{index}/file.ts:{index}` that resolves.\n\n"
            f"- [ ] first task for section {index}\n"
            f"- [x] second task\n"
            f"- a plain item\n"
        )
        out.append(block)
        line_count += block.count("\n")
        if index % 7 == 0:
            out.append(f"```swift\nlet value{index} = {index}\nfunc compute{index}() -> Int {{ value{index} * 2 }}\n```\n\n")
            line_count += 6
        if index % 11 == 0:
            out.append(f"| column | value |\n|---|--:|\n| a | {index} |\n| b | {index * 2} |\n\n")
            line_count += 6
        if index % 13 == 0:
            out.append("> [!NOTE]\n> A callout, because agents emit these constantly.\n\n")
            line_count += 3
    return "".join(out)


def math_matches(text):
    """MarkdownCore's MathScanner.matches over one run of text: (content, is_display)."""
    out = []
    i, end = 0, len(text)
    at = lambda k: text[k] if 0 <= k < end else None
    while i < end:
        ch = text[i]
        if ch == "\\":
            nxt = at(i + 1)
            if nxt in ("(", "["):
                closer = ")" if nxt == "(" else "]"
                j, found = i + 2, None
                while j + 1 < end:
                    if text[j] == "\\" and text[j + 1] == closer:
                        count, p = 0, j
                        while p >= i + 2 and text[p] == "\\":
                            count += 1
                            p -= 1
                        if count % 2 == 1:
                            found = j
                            break
                    j += 1
                if found is not None and found > i + 2:
                    out.append((text[i + 2:found], nxt == "["))
                    i = found + 2
                    continue
            i += 2
            continue
        if ch == "$" and not (i > 0 and text[i - 1] == "\\") and not (at(i - 1) or "").isdigit():
            display = at(i + 1) == "$"
            width = 2 if display else 1
            j, match = i + width, None
            while j < end:
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == "$":
                    if display and at(j + 1) != "$":
                        j += 1
                        continue
                    if not display and at(j + 1) == "$":
                        break
                    body = text[i + width:j]
                    close_end = j + width
                    if display:
                        plausible = body.strip() != ""
                    else:
                        plausible = (
                            body != "" and "\n" not in body and not body[0].isspace() and not body[-1].isspace()
                            and not all(c.isdigit() or c in ".," for c in body)
                            and not (at(close_end) or "").isdigit()
                        )
                    if plausible:
                        match = (body, display, close_end)
                    break
                j += 1
            if match:
                out.append((match[0], match[1]))
                i = match[2]
                continue
        i += 1
    return out


def math_formulas():
    """The formulas in the Markdown corpus, one .tex per (style, LaTeX): code
    fences and code spans stripped, inline math per line, `$$` blocks per paragraph."""
    seen = set()
    number = 0
    for directory, subdirectories, files in os.walk(OUT):
        subdirectories.sort()
        for name in sorted(files):
            if not name.endswith(".md"):
                continue
            lines, fence = [], None
            for line in open(os.path.join(directory, name), encoding="utf-8").read().split("\n"):
                stripped = line.lstrip()
                if fence is None and (stripped.startswith("```") or stripped.startswith("~~~")):
                    fence = stripped[:3]
                    lines.append("")
                    continue
                if fence is not None:
                    if stripped.startswith(fence):
                        fence = None
                    lines.append("")
                    continue
                lines.append(re.sub(r"(`+)(.+?)\1", "", line))
            found = [m for line in lines for m in math_matches(line)]
            for paragraph in "\n".join(lines).split("\n\n"):
                if "\n" in paragraph.strip():
                    found.extend(m for m in math_matches(paragraph.strip()) if m[1])
            for latex, display in found:
                if (latex, display) in seen:
                    continue
                seen.add((latex, display))
                number += 1
                style = "display" if display else "inline"
                write(f"math/formula-{number:04d}-{style}.tex", f"{style}\n{latex}")


def main():
    if not os.path.exists(os.path.join(VENDOR, "downright", "Package.swift")):
        sys.exit("vendor/downright is missing; run `git submodule update --init`")
    shutil.rmtree(OUT, ignore_errors=True)
    os.makedirs(os.path.join(OUT, "docs"))
    spec_examples()
    downright_docs()
    test_fixtures()
    for lines in (40, 400, 5000):
        write(f"agent/agent-{lines}.md", agent_document(lines))
    math_formulas()
    count = sum(len(files) for _, _, files in os.walk(OUT))
    print(f"corpus/generated: {count} documents")


if __name__ == "__main__":
    main()
