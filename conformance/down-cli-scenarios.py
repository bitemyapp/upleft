#!/usr/bin/env python3
"""Writes the `down-cli` suite's scenarios, corpus/down-cli/*.json.

The scenario format is documented in
oracle/app/Sources/downright-app-oracle/DownCLIDump.swift. Run from the
repository root: `python3 conformance/down-cli-scenarios.py`. Existing
scenario files are replaced.
"""
import json
import os
import subprocess
import sys

OUT = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "corpus", "down-cli")
os.makedirs(OUT, exist_ok=True)
for name in os.listdir(OUT):
    if name.endswith(".json"):
        os.remove(os.path.join(OUT, name))

scenarios = {}


def add(name, **scenario):
    assert name not in scenarios, name
    scenarios[name] = scenario


def f(path, text=None, **extra):
    item = {"path": path}
    if text is not None:
        item["text"] = text
    item.update(extra)
    return item


def plist_xml(identifier="com.ezzy.downright", version="9.8.7", extensions=None, feed="https://example.invalid/appcast.xml",
              key="abc123", extra="", types_xml=None):
    if extensions is None:
        extensions = [["MD", "markdown", "mdown", "mkd"], ["mdx", "mdc", "qmd", "Rmd"]]
    types = "".join(
        "<dict><key>CFBundleTypeExtensions</key><array>" + "".join(f"<string>{e}</string>" for e in group) + "</array></dict>"
        for group in extensions)
    body = ""
    if identifier is not None:
        body += f"<key>CFBundleIdentifier</key><string>{identifier}</string>"
    if version is not None:
        body += f"<key>CFBundleShortVersionString</key><string>{version}</string>"
    body += f"<key>CFBundleDocumentTypes</key>" + (types_xml if types_xml is not None else f"<array>{types}</array>")
    if feed is not None:
        body += f"<key>SUFeedURL</key><string>{feed}</string>"
    if key is not None:
        body += f"<key>SUPublicEDKey</key><string>{key}</string>"
    body += extra
    return ('<?xml version="1.0" encoding="UTF-8"?>\n<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" '
            '"http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n<plist version="1.0"><dict>' + body + "</dict></plist>\n")


def app_bundle(root, plist=None, executable=True, plugins=(), down_binary=False):
    files = []
    if plist is not None:
        files.append(f(f"{root}/Contents/Info.plist", plist))
    if executable:
        files.append(f(f"{root}/Contents/MacOS/Downright", "#!/bin/sh\nexit 0\n", mode="755"))
    if down_binary:
        files.append(f(f"{root}/Contents/MacOS/down", "#!/bin/sh\nexit 0\n", mode="755"))
    for plugin in plugins:
        files.append(f(f"{root}/Contents/PlugIns/{plugin}", directory=True))
    return files


# ---------------------------------------------------------------- global

add("global-version-long", argv=["--version"])
add("global-version-short", argv=["-v"])
add("global-help-long", argv=["--help"])
add("global-help-short", argv=["-h"])
add("global-help-inside-read", argv=["read", "--help"])
add("global-version-inside-check", argv=["check", "a.md", "-v"])
add("global-help-inside-hook", argv=["hook", "--install", "-h"])
add("global-no-arguments-usage", argv=[])
add("global-unknown-option-open", argv=["--wat"])
add("global-unknown-option-read", argv=["read", "--wat"])
add("global-unknown-option-export", argv=["export", "-x"])
add("global-unknown-option-check", argv=["check", "--jsonx"])
add("global-unknown-option-outline", argv=["outline", "--all"])
add("global-unknown-option-doctor", argv=["doctor", "extra"])
add("global-unknown-option-notify", argv=["notify", "file.md"])
add("global-unknown-option-watch", argv=["watch", "-q"])
add("global-unknown-option-hook", argv=["hook", "--force"])
add("global-missing-value-export-output", argv=["export", "-o"])
add("global-missing-value-export-format", argv=["export", "--format"])
add("global-missing-value-check-target", argv=["check", "--target"])
add("global-missing-value-open-line", argv=["open", "--line"])
add("global-missing-value-doctor-app", argv=["doctor", "--app"])
add("global-missing-value-watch-debounce", argv=["watch", "--debounce"])
add("global-missing-value-hook-scope", argv=["hook", "--scope"])
add("global-invalid-format", argv=["export", "--format", "pdf", "a.md"])
add("global-unknown-target", argv=["check", "--target", "word", "a.md"])
add("global-unknown-scope", argv=["hook", "--install", "--scope", "global"])
add("global-invalid-line-zero", argv=["open", "--line", "0", "a.md"])
add("global-invalid-line-negative", argv=["--line", "-5", "a.md"])
add("global-invalid-line-text", argv=["--line", "twelve", "a.md"])
add("global-invalid-line-overflow", argv=["--line", "99999999999999999999", "a.md"])
add("global-invalid-line-space", argv=["--line", " 3", "a.md"])
add("global-invalid-debounce-text", argv=["watch", "--debounce", "soon"])
add("global-invalid-debounce-negative", argv=["watch", "--debounce", "-1"])
add("global-invalid-debounce-nan", argv=["watch", "--debounce", "nan"])
add("global-invalid-debounce-space", argv=["watch", "--debounce", "5 "])
add("global-combining-dash-is-a-path", argv=["-\u0301"])
add("global-combining-dash-read-path", argv=["read", "-\u0301"])

# ---------------------------------------------------------------- read

add("read-one-file", files=[f("work/notes.md", "# Notes\n\nBody.\n")], argv=["read", "notes.md"])
add("read-several-files", files=[f("work/a.md", "# A\n"), f("work/b.md", "no trailing newline"), f("work/c.md", "crlf\r\n")],
    argv=["read", "a.md", "b.md", "c.md", "b.md"])
add("read-json", files=[f("work/a.md", "# A “quoted” / slash\n\tTab\n"), f("work/sub/é.md", "é\n")],
    argv=["read", "--json", "a.md", "sub/é.md"])
add("read-stdin", argv=["read"], stdin={"text": "from stdin\nno newline at end"})
add("read-stdin-dash-json", argv=["read", "--json", "-"], stdin={"text": "# piped\n"})
add("read-stdin-empty", argv=["read", "-"], stdin={"text": ""})
add("read-stdin-devnull", argv=["read"])
add("read-stdin-twice", argv=["read", "-", "-"], stdin={"text": "once\n"})
add("read-stdin-invalid-utf8", argv=["read"], stdin={"hex": "61e2826220f09f98ff20c08020eda080200a"})
add("read-missing-file", files=[f("work/a.md", "a\n")], argv=["read", "a.md", "missing.md"])
add("read-directory", files=[f("work/dir.md", directory=True)], argv=["read", "dir.md"])
add("read-invalid-utf8-file", files=[f("work/bad.md", hex="23206f6bff0a")], argv=["read", "bad.md"])
add("read-bom-file", files=[f("work/bom.md", hex="efbbbf23206f6b0a")], argv=["read", "--json", "bom.md"])
add("read-unreadable-file", files=[f("work/secret.md", "secret\n", mode="000")], argv=["read", "secret.md"])
add("read-double-dash", files=[f("work/-weird.md", "weird\n"), f("work/--json", "not json\n")],
    argv=["read", "--", "-weird.md", "--json"])
add("read-tilde-path", files=[f("home/notes.md", "home notes\n")], argv=["read", "--json", "~/notes.md"])
add("read-relative-dots", files=[f("work/sub/a.md", "a\n")], argv=["read", "--json", "./sub/../sub/./a.md"])
add("read-symlink", files=[f("work/target.md", "target\n"), f("work/link.md", symlink="target.md")],
    argv=["read", "--json", "link.md"])
add("read-broken-symlink", files=[f("work/dangling.md", symlink="nowhere.md")], argv=["read", "dangling.md"])

# ---------------------------------------------------------------- export

EXPORT_DOC = """# Title <with> "quotes" & more
## Second **bold** and *em* and `code`
####### not a heading
#no space

Paragraph line one
line two with [link](https://example.com/a?b=1&c=2) and [rel](./guide.md#part)

- item one
* item two
+ [x] done item
- [X] done upper
- [ ] open item
1. first
2) second
   3. nested ordered

> quoted *text*
>not a quote

---
***
___

```swift
let a = 1 < 2 && "x"
```

```
unterminated fence
line
"""
add("export-stdout-structure", files=[f("work/doc.md", EXPORT_DOC)], argv=["export", "doc.md"])
add("export-format-uppercase", files=[f("work/doc.md", "# T\n")], argv=["export", "-f", "HTML", "doc.md"])
add("export-stdin-title", argv=["export"], stdin={"text": "# From stdin\n\ntext\n"})
add("export-several-files", files=[f("work/a.md", "# A\n"), f("work/b.md", "- b\n")], argv=["export", "a.md", "b.md"])
add("export-output-file", files=[f("work/doc.md", "# Doc\n\nbody\n")], argv=["export", "-o", "out.html", "doc.md"])
add("export-output-dash", files=[f("work/doc.md", "# Doc\n")], argv=["export", "--output", "-", "doc.md"])
add("export-output-tilde", files=[f("work/doc.md", "# Doc\n")], argv=["export", "-o", "~/exported.html", "doc.md"])
add("export-output-missing-directory", files=[f("work/doc.md", "# Doc\n")], argv=["export", "-o", "nope/out.html", "doc.md"])
add("export-output-is-directory", files=[f("work/doc.md", "# Doc\n"), f("work/outdir", directory=True)],
    argv=["export", "-o", "outdir", "doc.md"])
add("export-output-overwrites", files=[f("work/doc.md", "# New\n"), f("work/out.html", "old contents\n", mode="600")],
    argv=["export", "-o", "out.html", "doc.md"])
add("export-title-escaping", files=[f("work/a \"b\" <c> & d.tar.md", "x\n")], argv=["export", "a \"b\" <c> & d.tar.md"])
add("export-title-hidden-file", files=[f("work/.hidden.md", "x\n"), f("work/.md", "y\n")], argv=["export", ".hidden.md"])
add("export-title-dotfile", files=[f("work/.md", "y\n")], argv=["export", ".md"])
add("export-links-safety", files=[f("work/links.md",
    "[relative](guide.md) [anchor](#part) [web](https://example.com) [mail](mailto:a@example.com)\n"
    "[script](JaVaScRiPt:alert(1)) [data](data:text/html,bad) [editor](vscode://file/tmp/a)\n"
    "[bad](javascript/foo:alert(1)) [spaced]( javascript:x ) [ws](\u2003javascript:x) [colon](:x) [https](HTTPS://X)\n"
    "[tab](java\tscript:alert(1)) [cr](j\rascript:alert(2))\n"
    "[nl](ja\nscript:alert(1))\n"
    "[plus](a+b.c-d:x) [digits](123:x) [unicode](é:x) [math](\U0001D7D8ab:x)\n")], argv=["export", "links.md"])
add("export-images-safety", files=[f("work/images.md",
    "![relative](images/photo.png)\n"
    "![web](https://tracker.example/pixel.png)\n"
    "![data](data:image/svg+xml,bad)\n"
    "![file](file:///private/etc/passwd)\n"
    "![protocol](//tracker.example/pixel.png)\n"
    "![absolute](/private/etc/passwd)\n"
    "![traversal](../private/photo.png)\n"
    "![encoded](%2e%2e/private/photo.png)\n"
    "![backslash](..\\\\x.png) ![lead](\\\\x.png)\n"
    "![query](a.png?x=../y) ![frag](a.png#../y) ![bad-percent](%zz.png) ![bad-utf8](%ff.png) ![nul](%00.png)\n"
    "![tabbed](http\t://tracking.example/x.png) ![empty-alt](x.png) ![alt with *stars*](y.png)\n")],
    argv=["export", "images.md"])
add("export-inline-markup", files=[f("work/inline.md",
    "`a` ``b`` ```c``` `unterminated\n"
    "**bold** ***both*** *em* **a*b** * spaced * \\*escaped\\*\n"
    "[label with **bold**](x.md) [](empty.md) [nested [brackets]](y.md) ![img](a.png)[link](b.md)\n"
    "&amp; &lt; <b>raw</b> \"quote\"\n")], argv=["export", "inline.md"])
add("export-unicode-edges", files=[f("work/u.md",
    "#\u0301 not heading\n"
    "# \u0301combining after space\n"
    "```\u0301not fence\n"
    ">\u0301 not quote\n"
    "- item\n"
    "-\u0301 not item\n"
    "&\u0301 amp < \u0301\n"
    "\u0663. arabic digit item\n"
    "[l](a.md)\u0301 after\n"
    "line\u2028separator\u2029para\u0085next\x0bvt\x0cff\n"
    "---\u0301\n"
    "\ufeffbom inside\n")], argv=["export", "u.md"])
add("export-crlf-and-cr", files=[f("work/crlf.md", "# A\r\n\r\npara\r\ncontinued\r\n- x\r\n\ry\rz\n")], argv=["export", "crlf.md"])
add("export-empty-document", files=[f("work/empty.md", "")], argv=["export", "empty.md"])
add("export-whitespace-lines", files=[f("work/w.md", "a\n \t \nb\n\u3000\nc\n\u200b\nd\n")], argv=["export", "w.md"])
add("export-lists-switching", files=[f("work/l.md", "- a\n1. b\n- c\n\n2. d\ntext\n- e\n")], argv=["export", "l.md"])
add("export-fence-language", files=[f("work/f.md", "```  c++ <x> \nint a;\n```\n``` \n```\n````\nfour\n````\n")],
    argv=["export", "f.md"])
add("export-image-fragment-crash", files=[f("work/crash.md", "![x](#)\n")], argv=["export", "crash.md"])
add("export-missing-input", argv=["export", "nothing.md"])
add("export-invalid-utf8-input", files=[f("work/bad.md", hex="ff")], argv=["export", "bad.md"])

# ---------------------------------------------------------------- check

CHECK_DOC = """---
title: Checked
tags: [a, b]
---

# Checked

Plain paragraph with [present](present.md) and [missing](missing.md#frag).
![alt](images/missing.png) ![](present.md)
[abs](/definitely/not/here.md) [home](~/home-note.md) [up](../outside.md) [web](https://example.com)
[bad](javascript:alert(1)) [ref][nothing]

| a | b |
|---|---|
| 1 | 2 |

- [ ] task
~~strike~~ $x^2$

[^1]: footnote
"""
add("check-clean", files=[f("work/clean.md", "# Clean\n\nNothing to see.\n")], argv=["check", "clean.md"])
add("check-findings-text", files=[f("work/doc.md", CHECK_DOC), f("work/present.md", "p\n"), f("home/home-note.md", "h\n"),
                                  f("outside.md", "o\n")], argv=["check", "doc.md"])
add("check-findings-json", files=[f("work/doc.md", CHECK_DOC), f("work/present.md", "p\n")], argv=["check", "--json", "doc.md"])
for target in ["commonmark", "GitHub", "git-hub", "obsidian", "PANDOC", "multi-markdown", "jekyll", "hugo", "quarto", "downright"]:
    add(f"check-target-{target.lower()}", files=[f("work/doc.md", CHECK_DOC)], argv=["check", "--target", target, "doc.md"])
add("check-target-json", files=[f("work/doc.md", CHECK_DOC)], argv=["check", "--json", "--target", "commonMark", "doc.md"])
add("check-stdin", argv=["check", "-"], stdin={"text": "# S\n\n[missing](missing.md) [bad](javascript:x)\n"})
add("check-stdin-json-target", argv=["check", "--json", "--target", "hugo"], stdin={"text": "- [ ] t\n\n$$x$$\n"})
add("check-double-dash", files=[f("work/-notes.md", "# N\n")], argv=["check", "--", "-notes.md"])
add("check-line-numbers-cr-crlf", files=[f("work/cr.md", "l1\r[m](missing.md)\r\n\r\nl4\r\n[n](nope.md)\n")],
    argv=["check", "cr.md"])
add("check-missing-file", argv=["check", "missing.md"])
add("check-invalid-utf8", files=[f("work/bad.md", hex="5b615d286d2e6d6429ff")], argv=["check", "bad.md"])
FOLDER_FILES = [
    f("work/docs/keep.md", "# Keep\n\n[missing](does-not-exist.md)\n"),
    f("work/docs/Upper.MD", "# Upper\n"),
    f("work/docs/b.markdown", "[x](gone.md)\n"),
    f("work/docs/c.qmd", "# Q\n"),
    f("work/docs/notes.txt", "[x](gone.md)\n"),
    f("work/docs/.hidden.md", "[x](gone.md)\n"),
    f("work/docs/sub/deep/d.mdx", "[x](../../keep.md)\n"),
    f("work/docs/.git/ignored.md", "[x](gone.md)\n"),
    f("work/docs/.build/ignored.md", "[x](gone.md)\n"),
    f("work/docs/node_modules/pkg/ignored.md", "[x](gone.md)\n"),
    f("work/docs/sub/DerivedData/ignored.md", "[x](gone.md)\n"),
    f("work/docs/.swiftpm/ignored.md", "[x](gone.md)\n"),
    f("work/docs/.git.md", "[x](gone.md)\n"),
    f("work/docs/linked", symlink="../elsewhere"),
    f("work/elsewhere/through-link.md", "[x](gone.md)\n"),
    f("work/docs/link.md", symlink="keep.md"),
    f("work/docs/é-nfc.md", "# NFC\n"),
    f("work/docs/e\u0301-nfd.md", "# NFD\n"),
]
add("check-folder-json", files=FOLDER_FILES, argv=["check", "--json", "docs"])
add("check-folder-text", files=FOLDER_FILES, argv=["check", "docs", "."])
add("check-folder-and-files-sorted", files=FOLDER_FILES + [f("work/z.md", "[x](gone.md)\n"), f("work/a.md", "[x](gone.md)\n")],
    argv=["check", "z.md", "docs/sub", "a.md"])
add("check-folder-tilde", files=[f("home/notes/n.md", "[x](gone.md)\n")], argv=["check", "~/notes"])
add("check-folder-broken-symlink", files=[f("work/docs/ok.md", "# ok\n"), f("work/docs/broken.md", symlink="missing-target.md")],
    argv=["check", "docs"])
add("check-folder-unreadable-subdirectory", files=[f("work/docs/ok.md", "# ok\n"), f("work/docs/locked/x.md", "[x](gone.md)\n"),
                                                    f("work/docs/locked", directory=True, mode="000")], argv=["check", "docs"])
add("check-folder-too-many-files",
    files=[f(f"work/many/n{index:04d}.md", "# n\n") for index in range(1001)], argv=["check", "many"])
add("check-folder-exactly-limit",
    files=[f(f"work/many/n{index:04d}.md", "") for index in range(1000)], argv=["check", "many"])
add("check-oversize-file", files=[f("work/big.md", repeat="a", count=10 * 1024 * 1024 + 1)], argv=["check", "big.md"])
add("check-exact-limit-file", files=[f("work/big.md", repeat="a", count=10 * 1024 * 1024 - 1, suffix="\n")],
    argv=["check", "--json", "big.md"])
add("check-oversize-stdin", argv=["check"], stdin={"repeat": "a", "count": 10 * 1024 * 1024 + 1})
add("check-oversize-through-symlink", files=[f("work/big.txt", repeat="a", count=10 * 1024 * 1024 + 1),
                                             f("work/small.md", symlink="big.txt")], argv=["check", "--json", "small.md"])
add("check-directory-md-path", files=[f("work/folder.md/inner.md", "[x](gone.md)\n")], argv=["check", "folder.md"])

# ---------------------------------------------------------------- outline

OUTLINE_DOC = """---
title: Front
---
# First
Text
## Child *em* `code`
### Grand [link](x.md)
## Child
Setext
======
####### seven
# Émoji 🌊 heading
"""
add("outline-text", files=[f("work/o.md", OUTLINE_DOC)], argv=["outline", "o.md"])
add("outline-json", files=[f("work/o.md", OUTLINE_DOC)], argv=["outline", "--json", "o.md"], stdoutFormat="json-lines")
add("outline-several-json", files=[f("work/o.md", OUTLINE_DOC), f("work/empty.md", "no headings\n")],
    argv=["outline", "--json", "o.md", "empty.md"], stdoutFormat="json-lines")
add("outline-stdin", argv=["outline"], stdin={"text": "Line 1\r# Heading 1\rLine 3\r## Heading 2\r"})
add("outline-crlf", files=[f("work/crlf.md", "Line 1\r\n# Heading 1\r\nLine 3\r\n## Heading 2\r\n")], argv=["outline", "crlf.md"])
add("outline-double-dash", files=[f("work/-draft.md", "# Draft\n")], argv=["outline", "--", "-draft.md"])
add("outline-no-headings", files=[f("work/p.md", "plain\n")], argv=["outline", "p.md"])
add("outline-missing", argv=["outline", "missing.md"])

# ---------------------------------------------------------------- open (error paths only)

add("open-missing-file", argv=["missing.md"])
add("open-missing-with-options", files=[f("work/a.md", "a\n")], argv=["open", "-n", "-b", "-w", "-e", "--line", "+3", "--review", "a.md", "gone.md"])
add("open-double-dash-missing", argv=["--", "-x.md", "--line"])
add("open-reveal-missing", argv=["open", "--reveal", "~/nope.md"])
add("open-reveal-relative-missing", files=[f("work/a.md", "a\n")], argv=["--reveal", "a.md", "./sub/../b.md"])
add("open-reveal-stdin", argv=["--reveal", "-"])
add("open-reveal-no-paths", argv=["--reveal"])
add("open-stdin-not-available", argv=["open", "-"], stdin={"text": ""})
add("open-stdin-then-missing", argv=["open", "-", "missing.md"], stdin={"text": "# piped document\n"}, tempFiles=True)
add("open-tilde-missing", argv=["~/missing.md"])

# ---------------------------------------------------------------- notify

NOTIFY_FILES = [f("work/notes.md", "# notes\n"), f("work/main.swift", "print(1)\n"), f("work/nb.ipynb", "{}\n"),
                f("work/other.markdown", "o\n"), f("home/home.md", "h\n"), f("work/é.md", "nfc\n")]


def notify(name, payload, dry_run=True, extra_argv=(), files=NOTIFY_FILES, raw=None):
    stdin = {"text": raw} if raw is not None else ({"text": json.dumps(payload)} if payload is not None else None)
    argv = ["notify"] + (["--dry-run"] if dry_run else []) + list(extra_argv)
    scenario = {"files": files, "argv": argv}
    if stdin is not None:
        scenario["stdin"] = stdin
    add(name, **scenario)


notify("notify-write-payload", {"hook_event_name": "PostToolUse", "tool_name": "Write",
                                "tool_input": {"file_path": "$SANDBOX/work/notes.md", "content": "hello"}})
notify("notify-response-path", {"tool_response": {"filePath": "$SANDBOX/work/other.markdown"}})
notify("notify-dedupes", {"tool_input": {"file_path": "$SANDBOX/work/notes.md"}, "tool_response": {"filePath": "$SANDBOX/work/notes.md"},
                          "path": "$SANDBOX/work/notes.md"})
notify("notify-dedupes-canonically", {"tool_input": {"file_path": "$SANDBOX/work/é.md"},
                                      "tool_response": {"filePath": "$SANDBOX/work/e\u0301.md"}})
notify("notify-all-keys-order", {"path": "$SANDBOX/work/other.markdown", "notebook_path": "$SANDBOX/home/home.md",
                                 "tool_input": {"notebookPath": "$SANDBOX/work/notes.md", "filePath": "$SANDBOX/work/other.markdown",
                                                "notebook_path": "$SANDBOX/work/nb.ipynb", "file_path": "$SANDBOX/work/main.swift"}})
notify("notify-notebook-not-markdown", {"tool_input": {"notebook_path": "$SANDBOX/work/nb.ipynb"}})
notify("notify-missing-markdown", {"tool_input": {"file_path": "$SANDBOX/work/deleted.md"}})
notify("notify-tilde-and-relative", {"tool_input": {"file_path": "~/home.md", "filePath": "notes.md", "path": "./sub/../other.markdown"}})
notify("notify-dotted-absolute", {"tool_input": {"file_path": "$SANDBOX/work/./sub/../notes.md"}})
notify("notify-focus-dry-run", {"tool_input": {"file_path": "$SANDBOX/work/notes.md"}}, extra_argv=["--focus"])
notify("notify-wrong-types", {"tool_input": {"file_path": 123, "filePath": None, "path": ["$SANDBOX/work/notes.md"],
                                             "notebook_path": {"x": 1}, "notebookPath": ""}})
notify("notify-tool-input-not-object", {"tool_input": "$SANDBOX/work/notes.md", "tool_response": ["x"]})
for index, raw in enumerate(["", "not json at all", "[]", "null", "{}", "{\"tool_input\":null}", "\"$SANDBOX/work/notes.md\"",
                             "{\"tool_input\":{\"file_path\":\"$SANDBOX/work/notes.md\"}} trailing"]):
    notify(f"notify-malformed-{index}", None, raw=raw)
notify("notify-no-stdin", None)
notify("notify-no-targets-without-dry-run", {"tool_input": {"file_path": "$SANDBOX/work/main.swift"}}, dry_run=False)
notify("notify-malformed-without-dry-run", None, dry_run=False, raw="{oops")
add("notify-utf16-payload", files=NOTIFY_FILES, argv=["notify", "--dry-run"],
    stdin={"hex": "fffe" + json.dumps({"path": "notes.md"}).encode("utf-16-le").hex()})
add("notify-extended-extension", files=[f("work/deep/Notes.MDX", "x\n"), f("work/q.Qmd", "q\n")], argv=["notify", "--dry-run"],
    stdin={"text": json.dumps({"tool_input": {"file_path": "$SANDBOX/work/deep/Notes.MDX", "filePath": "$SANDBOX/work/q.Qmd"}})})

# ---------------------------------------------------------------- watch (error paths only)

add("watch-missing-parent", argv=["watch", "/nonexistent-upleft-root/dir/x.md"])
add("watch-missing-relative", argv=["watch", "missing/dir/x.md"])
add("watch-missing-tilde", argv=["watch", "--focus", "~/nope/x.md"])
add("watch-second-root-missing", files=[f("work/a.md", "a\n")], argv=["watch", "a.md", ".", "gone/x/y.md"])
add("watch-debounce-infinite", argv=["watch", "--debounce", "inf", "/nonexistent-upleft-root/x/y"])
add("watch-debounce-negative-zero", argv=["watch", "--debounce", "-0", "--", "/nonexistent-upleft-root/x/--y.md"])
add("watch-debounce-hex", argv=["watch", "--debounce", "0x1p3", "/nonexistent-upleft-root/x/y"])

# ---------------------------------------------------------------- hook

ODD_SETTINGS = """{
  "permissions" : { "allow" : [ "Bash(ls:*)", "Read(~/docs/**)" ], "deny":[] },
  "env": {"GREETING": "h\\u00e9llo \\ud83c\\udf0a", "PATH_HINT": "/usr/local/bin", "EMPTY": ""},
  "numbers": [1, 1.0, 1.5, -0, 0.1, 1e2, 12345678901234567890, 3.14159265358979323846, -9223372036854775808, 2.5e-8],
  "nested": {"z": null, "a": true, "B": false, "é": "composed", "e\\u0301": "decomposed", "_": {"deep": [[]]}},
  "model": "opus",
  "hooks": {"PreToolUse": [{"matcher": "Read", "hooks": [{"type": "command", "command": "echo pre", "timeout": 30}]}],
            "PostToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "echo mine"}]}]}
}
"""
LEGACY_BARE = json.dumps({"hooks": {"PostToolUse": [{"matcher": "Write|Edit|MultiEdit|NotebookEdit",
                                                     "hooks": [{"type": "command", "command": "$SANDBOX/bin/down notify"}]}]}})
LEGACY_QUOTED = json.dumps({"hooks": {"PostToolUse": [{"matcher": "Write|Edit|MultiEdit|NotebookEdit", "extra": 1,
                                                       "hooks": [{"type": "command", "command": "echo keep"},
                                                                 {"type": "command", "command": "\"$SANDBOX/bin/down\" notify"}]}]}})
INSTALLED = json.dumps({"model": "sonnet", "hooks": {"PostToolUse": [
    {"matcher": "Write|Edit|MultiEdit|NotebookEdit", "hooks": [{"type": "command", "command": "'$SANDBOX/bin/down' notify"}]}]}}, indent=4)
FOREIGN_SAME_COMMAND = json.dumps({"hooks": {"PostToolUse": [
    {"matcher": "Bash", "hooks": [{"type": "command", "command": "'$SANDBOX/bin/down' notify"}]}]}})

add("hook-print-default", argv=["hook"])
add("hook-print-explicit", argv=["hook", "--install", "--print"])
add("hook-print-relative-argv0", cwd=".", argv0="bin/down", argv=["hook", "--print"])
add("hook-print-dotted-argv0", cwd="work", argv0="../bin/./down", argv=["hook"])
add("hook-print-bare-argv0", cwd="work", argv0="down", argv=["hook"])
add("hook-print-hostile-argv0", argv0="/opt/My Tools/o'brien/\"$(touch /tmp/pwned)\"/down", argv=["hook"])
add("hook-install-project-new", argv=["hook", "--install"])
add("hook-install-user-new", argv=["hook", "--install", "--scope", "user"])
add("hook-install-user-uppercase-scope", argv=["hook", "--scope", "USER", "--install"])
add("hook-install-project-subdirectory-cwd", cwd="work/nested/deeper", files=[f("work/nested/deeper", directory=True)],
    argv=["hook", "--install", "--scope", "project"])
add("hook-install-odd-settings", files=[f("work/.claude/settings.json", ODD_SETTINGS, mode="600")], argv=["hook", "--install"])
add("hook-install-user-odd-settings", files=[f("home/.claude/settings.json", ODD_SETTINGS)], argv=["hook", "--install", "--scope", "user"])
add("hook-install-already-installed", files=[f("work/.claude/settings.json", INSTALLED)], argv=["hook", "--install"])
add("hook-install-migrates-bare-legacy", files=[f("work/.claude/settings.json", LEGACY_BARE)], argv=["hook", "--install"])
add("hook-install-migrates-quoted-legacy", files=[f("work/.claude/settings.json", LEGACY_QUOTED)], argv=["hook", "--install"])
add("hook-install-foreign-matcher-same-command", files=[f("work/.claude/settings.json", FOREIGN_SAME_COMMAND)], argv=["hook", "--install"])
add("hook-install-different-executable", files=[f("work/.claude/settings.json", INSTALLED)], argv0="/opt/homebrew/bin/down",
    argv=["hook", "--install"])
add("hook-install-post-tool-use-not-all-objects", files=[f("work/.claude/settings.json",
    json.dumps({"hooks": {"PostToolUse": [{"matcher": "Bash", "hooks": []}, "stray string", 7]}}))], argv=["hook", "--install"])
add("hook-install-hooks-is-array", files=[f("work/.claude/settings.json", json.dumps({"hooks": ["x"], "keep": 1}))],
    argv=["hook", "--install"])
add("hook-install-empty-post-tool-use", files=[f("work/.claude/settings.json", json.dumps({"hooks": {"PostToolUse": []}}))],
    argv=["hook", "--install"])
add("hook-install-nested-canonical-duplicate-keys", files=[f("work/.claude/settings.json",
    "{\"permissions\": {\"\\u00e9\": 1, \"e\\u0301\": 2, \"K\": 3, \"\\u212a\": 4}}")], argv=["hook", "--install"])
add("hook-install-duplicate-json-keys", files=[f("work/.claude/settings.json", "{\"a\": 1, \"a\": 2, \"hooks\": {}, \"hooks\": {\"x\": []}}")],
    argv=["hook", "--install"])
add("hook-install-infinite-number", files=[f("work/.claude/settings.json", "{\"big\": 1e400}")], argv=["hook", "--install"])
add("hook-install-bom-settings", files=[f("work/.claude/settings.json", hex="efbbbf" + b'{"a":1}'.hex())], argv=["hook", "--install"])
add("hook-install-utf16-settings", files=[f("work/.claude/settings.json", hex="fffe" + '{"a":"\u00e9"}'.encode("utf-16-le").hex())],
    argv=["hook", "--install"])
add("hook-install-array-settings", files=[f("work/.claude/settings.json", "[]")], argv=["hook", "--install"])
add("hook-install-string-settings", files=[f("work/.claude/settings.json", "\"just a string\"")], argv=["hook", "--install"])
add("hook-install-malformed", files=[f("work/.claude/settings.json", "{ user-owned and damaged")], argv=["hook", "--install"])
add("hook-install-empty-file", files=[f("work/.claude/settings.json", "")], argv=["hook", "--install"])
add("hook-install-oversize", files=[f("work/.claude/settings.json", repeat=" ", count=1_048_577)], argv=["hook", "--install"])
add("hook-install-exact-limit", files=[f("work/.claude/settings.json", repeat=" ", count=1_048_576 - 2, prefix="{", suffix="}")],
    argv=["hook", "--install"])
add("hook-install-settings-is-directory", files=[f("work/.claude/settings.json", directory=True)], argv=["hook", "--install"])
add("hook-install-settings-is-symlink", files=[f("work/real.json", "{}"), f("work/.claude/settings.json", symlink="../real.json")],
    argv=["hook", "--install"])
add("hook-install-unreadable-settings", files=[f("work/.claude/settings.json", "{\"permissions\":{\"allow\":[]}}", mode="000")],
    argv=["hook", "--install"])
add("hook-install-claude-is-file", files=[f("work/.claude", "not a directory\n")], argv=["hook", "--install"])
add("hook-install-readonly-directory", files=[f("work/.claude/settings.json", "{}"), f("work/.claude", directory=True, mode="555")],
    argv=["hook", "--install"])
add("hook-install-readonly-parent", files=[f("work/ro", directory=True, mode="555")], cwd="work/ro", argv=["hook", "--install"])
add("hook-install-unsearchable-directory", files=[f("work/.claude/settings.json", "{}"), f("work/.claude", directory=True, mode="000")],
    argv=["hook", "--install"])
add("hook-uninstall-installed", files=[f("work/.claude/settings.json", INSTALLED)], argv=["hook", "--uninstall"])
add("hook-uninstall-keeps-others", files=[f("work/.claude/settings.json", json.dumps({"permissions": {"allow": []}, "hooks": {
    "PreToolUse": [{"matcher": "Read", "hooks": []}],
    "PostToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "'$SANDBOX/bin/down' notify"}]},
                    {"matcher": "Write|Edit|MultiEdit|NotebookEdit", "hooks": [
                        {"type": "command", "command": "'$SANDBOX/bin/down' notify"}, {"type": "command", "command": "echo also"},
                        {"type": "other"}]},
                    {"matcher": "Write|Edit|MultiEdit|NotebookEdit", "timeout": 5}]}}))], argv=["hook", "--uninstall"])
add("hook-uninstall-legacy", files=[f("work/.claude/settings.json", LEGACY_QUOTED)], argv=["hook", "--uninstall"])
add("hook-uninstall-not-installed", files=[f("work/.claude/settings.json", FOREIGN_SAME_COMMAND)], argv=["hook", "--uninstall"])
add("hook-uninstall-no-file", argv=["hook", "--uninstall", "--scope", "user"])
add("hook-uninstall-prunes-everything", files=[f("home/.claude/settings.json", INSTALLED)], argv=["hook", "--uninstall", "--scope", "user"])
add("hook-uninstall-malformed", files=[f("work/.claude/settings.json", "{")], argv=["hook", "--uninstall"])
add("hook-uninstall-hooks-not-object", files=[f("work/.claude/settings.json", json.dumps({"hooks": 5}))], argv=["hook", "--uninstall"])

# ---------------------------------------------------------------- doctor

APP = "work/apps/Downright.app"
add("doctor-no-app", argv=["doctor"])
add("doctor-no-app-json", argv=["doctor", "--json"])
add("doctor-app-missing-path", argv=["doctor", "--app", "/nonexistent-upleft-root/Downright.app"])
add("doctor-app-empty-directory", files=[f(APP, directory=True)], argv=["doctor", "--app", "apps/Downright.app"])
add("doctor-app-full", files=app_bundle(APP, plist_xml(), plugins=["DownrightQL.appex"], down_binary=True)
    + [f("home/.local/bin/down", symlink="$SANDBOX/work/apps/Downright.app/Contents/MacOS/down"),
       f("home/.local/bin/md", symlink="../../elsewhere/md")],
    argv=["doctor", "--app", "$SANDBOX/work/apps/Downright.app"])
add("doctor-app-full-json", files=app_bundle(APP, plist_xml(), plugins=["DownrightQL.appex", "DownrightThumb.appex"], down_binary=True)
    + [f("home/.local/bin/md", symlink="../../work/apps/Downright.app/Contents/MacOS/down")],
    argv=["doctor", "--json", "--app", "apps/Downright.app"])
add("doctor-app-binary-plist", files=app_bundle(APP, None) + [f(f"{APP}/Contents/Info.plist", hex=subprocess.run(
    ["plutil", "-convert", "binary1", "-o", "-", "-"], input=plist_xml(version="1.0 β").encode(), capture_output=True, check=True).stdout.hex())],
    argv=["doctor", "--app", "apps/Downright.app"])
add("doctor-app-wrong-identifier", files=app_bundle(APP, plist_xml(identifier="com.example.other")), argv=["doctor", "--app", "apps/Downright.app"])
add("doctor-app-missing-identifier", files=app_bundle(APP, plist_xml(identifier=None, version=None)), argv=["doctor", "--json", "--app", "apps/Downright.app"])
add("doctor-app-no-executable", files=app_bundle(APP, plist_xml(), executable=False), argv=["doctor", "--app", "apps/Downright.app"])
add("doctor-app-executable-not-executable", files=app_bundle(APP, plist_xml(), executable=False)
    + [f(f"{APP}/Contents/MacOS/Downright", "x", mode="644")], argv=["doctor", "--app", "apps/Downright.app"])
add("doctor-app-placeholder-key", files=app_bundle(APP, plist_xml(key="REPLACE_PLACEHOLDER_KEY")), argv=["doctor", "--app", "apps/Downright.app"])
add("doctor-app-no-feed", files=app_bundle(APP, plist_xml(feed=None, key=None)), argv=["doctor", "--app", "apps/Downright.app"])
add("doctor-app-empty-feed", files=app_bundle(APP, plist_xml(feed="", key="k")), argv=["doctor", "--app", "apps/Downright.app"])
add("doctor-app-missing-extensions", files=app_bundle(APP, plist_xml(extensions=[["md", "markdown"], ["QMD"]])),
    argv=["doctor", "--app", "apps/Downright.app"])
add("doctor-app-extension-not-string", files=app_bundle(APP, plist_xml(types_xml=
    "<array><dict><key>CFBundleTypeExtensions</key><array><string>md</string><integer>1</integer></array></dict>"
    "<dict><key>CFBundleTypeExtensions</key><array><string>markdown</string></array></dict></array>")),
    argv=["doctor", "--app", "apps/Downright.app"])
add("doctor-app-document-type-not-dictionary", files=app_bundle(APP, plist_xml(types_xml=
    "<array><dict><key>CFBundleTypeExtensions</key><array><string>md</string></array></dict><string>stray</string></array>")),
    argv=["doctor", "--app", "apps/Downright.app"])
add("doctor-app-malformed-plist", files=app_bundle(APP, "<plist><dict><key>broken"), argv=["doctor", "--app", "apps/Downright.app"])
add("doctor-app-plist-not-dictionary", files=app_bundle(APP, '<?xml version="1.0"?><plist version="1.0"><array/></plist>'),
    argv=["doctor", "--app", "apps/Downright.app"])
add("doctor-app-translocated", files=app_bundle("work/private/var/AppTranslocation/X/d/Downright.app", plist_xml()),
    argv=["doctor", "--app", "private/var/AppTranslocation/X/d/Downright.app"])
add("doctor-home-applications", files=app_bundle("home/Applications/Downright.app", plist_xml()), argv=["doctor"])
add("doctor-app-tilde", files=app_bundle("home/Apps/Downright.app", plist_xml()), argv=["doctor", "--app", "~/Apps/Downright.app"])
add("doctor-app-symlinked-into-home", files=app_bundle(APP, plist_xml()) + [f("home/Applications/Linked.app", symlink="$SANDBOX/work/apps/Downright.app")],
    argv=["doctor", "--app", "~/Applications/Linked.app"])
add("doctor-app-is-a-file", files=[f("work/Downright.app", "not a bundle\n")], argv=["doctor", "--json", "--app", "Downright.app"])
add("doctor-argv0-inside-bundle", files=app_bundle("work/Bundle.app", plist_xml(version="2.0")),
    argv0="$SANDBOX/work/Bundle.app/Contents/MacOS/down", argv=["doctor"])

for name, scenario in scenarios.items():
    with open(os.path.join(OUT, name + ".json"), "w", encoding="utf-8") as handle:
        json.dump(scenario, handle, ensure_ascii=False, indent=1)
        handle.write("\n")
print(len(scenarios), "scenarios")
