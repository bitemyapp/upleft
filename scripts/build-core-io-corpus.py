#!/usr/bin/env python3
"""Writes corpus/core-io: byte-level and Unicode edge cases for the `core-io`
conformance suite (DocumentIO decoding, BOMs, encodings, line endings, and
text the scanners and Swift String semantics find hard).

The files are `.txt` so only the core suites (which accept any bytes) read
them; the parse/decorate/render suites take `.md` and would reject non-UTF-8
input. Deterministic; the outputs are committed.
"""

import os
import shutil

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "corpus", "core-io")

SAMPLE = (
    "---\ntitle: Café notes\ntags: [a, b]\n---\n\n"
    "# Überschrift — 日本\n\n"
    "Prose with $x^2$ and [[Wiki|label]] and src/a/b.ts:12.\n\n"
    "> [!warning] Careful\n> body\n\n"
    "```swift\nfunc f() -> Int { 1 }\n```\n\n"
    "- [ ] task \U0001F600\n- [x] done\n\n"
    "[ref]: https://example.com \"Title\"\n[^1]: A footnote.\n"
)

ADVERSARIAL = "\n".join([
    "# Heading with combining á and K Kelvin",
    "## ΣΣ final sigma ΟΣ",
    "## emoji \U0001F468‍\U0001F469‍\U0001F467 and flags \U0001F1FA\U0001F1F8\U0001F1EC",
    "### dup",
    "### dup",
    "### dup",
    "",
    "Math $á$ and $ spaced $ and $x$$ and $5 and $10 and \\(inline\\) and \\[display\\].",
    "Math with crlf $a\r\nb$ and money $100$ and $1,000.5$ and $x$1.",
    "Wikilinks [[a|́b]] and [[ Target ]] and [[x|]] and [[K]] and [[a[b]] and [[multi",
    "line]].",
    "Paths ./a/b.md, ../up/x.swift:10:4, ~/h/f.txt, /abs/p.rs:0, and/or, www.x.com/a, http://x.y/z.md,",
    "  a:b c:3:16 pkg/mod.go config.\U0001F680 x.TS X.Md file.rs:99999999999999999999999 f.md:",
    "`code/span.ts` `and/or` `plain` `x.json:3:4` `a` `` `double` ``",
    "Colon keý: value",
    "> [!NOTE]+ Folded title",
    "> [!unknown] not a callout",
    ">>  [!Tip]-",
    "> > [!caution] nested",
    ">",
    "Line with zero width​space and nbsp here and ideographic　space.",
    "<details open>",
    "<summary>More</summary>",
    "",
    "Body.",
    "",
    "</details>",
    "",
    "<p align=\"center\"><img src=\"logo.png\" alt=\"Logo\"></p>",
    "<a href=\"javascript:x\">bad</a> <b onclick=\"x\">bad</b> <br> <br/>",
    "<table><tr><th align=\"left\">h</th><td>d</td></tr></table>",
    "",
    "~~~ Python extra",
    "def f(self):",
    "    pass",
    "~~~",
    "",
    "````\n```\nnot closed by shorter\n````",
    "",
    "```mermaid\ngraph TD; A-->B\n```",
    "",
    "```\n$ ls\n% pwd\n```",
    "",
    "```\n{\n  \"a\": 1\n}\n```",
    "",
    "```\n<div>hi</div>\n```",
    "",
    "```\n#!/usr/bin/env node\nconsole.log(1)\n```",
    "",
    "   ```\n   indented fence\n   ```",
    "",
    "[Labél]: /one",
    "[LABÉL]: /two 'single'",
    "[KK]: <angled> (paren title)",
    "[kk]: /kk",
    "[^ń]: Footnote with combining identifier",
    "    continued",
    "[^]: empty",
    "[ \\] ]: /escaped",
    "    [indented]: /nope",
    "",
    "Text[^ń] and [^missing] ref.",
    " Line separator start",
    "Tabs\tinside\tand trailing   ",
    "Final line without newline é",
])


def write(name, data):
    with open(os.path.join(OUT, name), "wb") as handle:
        handle.write(data)


def main():
    shutil.rmtree(OUT, ignore_errors=True)
    os.makedirs(OUT)
    text = SAMPLE
    write("utf8.txt", text.encode("utf-8"))
    write("utf8-bom.txt", b"\xef\xbb\xbf" + text.encode("utf-8"))
    write("utf8-double-bom.txt", b"\xef\xbb\xbf\xef\xbb\xbf" + text.encode("utf-8"))
    write("utf16le-bom.txt", b"\xff\xfe" + text.encode("utf-16-le"))
    write("utf16be-bom.txt", b"\xfe\xff" + text.encode("utf-16-be"))
    write("utf16le-nobom.txt", text.encode("utf-16-le"))
    write("utf16be-nobom.txt", text.encode("utf-16-be"))
    write("utf16le-double-bom.txt", b"\xff\xfe\xff\xfe" + text.encode("utf-16-le"))
    write("utf16le-odd.txt", b"\xff\xfe" + text.encode("utf-16-le") + b"\x41")
    write("utf16le-lone-surrogate.txt", b"\xff\xfe" + "ab".encode("utf-16-le") + b"\x00\xd8" + "cd\n".encode("utf-16-le"))
    write("utf16be-reversed-surrogates.txt", b"\xfe\xff" + b"\xdc\x00\xd8\x00" + "x\n".encode("utf-16-be"))
    write("utf32le-bom.txt", b"\xff\xfe\x00\x00" + text.encode("utf-32-le"))
    write("utf32be-bom.txt", b"\x00\x00\xfe\xff" + text.encode("utf-32-be"))
    write("utf32le-nobom.txt", text.encode("utf-32-le"))
    write("utf32be-nobom.txt", text.encode("utf-32-be"))
    write("utf32le-truncated.txt", b"\xff\xfe\x00\x00" + text.encode("utf-32-le")[:-2])
    write("utf32le-invalid-scalar.txt", b"\xff\xfe\x00\x00" + "ab".encode("utf-32-le") + b"\x00\x00\x11\x00" + "\n".encode("utf-32-le"))
    write("latin1.txt", "café über naïve\nsecond line ÿ\n".encode("latin-1"))
    write("invalid-utf8-tail.txt", "# ok\n\nText ".encode("utf-8") + b"\xe2\x82")
    write("nul-bytes.txt", b"a\x00b\x00c\x00\n\x00")
    write("mostly-nul.txt", b"\x00\x00\x00a\x00\x00\x00b")
    write("crlf.txt", text.replace("\n", "\r\n").encode("utf-8"))
    write("crlf-utf16le.txt", b"\xff\xfe" + text.replace("\n", "\r\n").encode("utf-16-le"))
    write("cr.txt", text.replace("\n", "\r").encode("utf-8"))
    write("mixed.txt", b"line one\nline two\r\nline three\rline four\n")
    write("crlf-no-trailing.txt", b"# T\r\n\r\nbody")
    write("trailing-cr.txt", b"a\nb\r")
    write("empty.txt", b"")
    write("one-byte.txt", b"x")
    write("only-newlines.txt", b"\n\n\r\n\r")
    write("adversarial.txt", ADVERSARIAL.encode("utf-8"))
    write("adversarial-crlf.txt", ADVERSARIAL.replace("\r\n", "\n").replace("\n", "\r\n").encode("utf-8"))
    write("adversarial-utf16be.txt", b"\xfe\xff" + ADVERSARIAL.encode("utf-16-be"))
    print(f"corpus/core-io: {len(os.listdir(OUT))} files")


if __name__ == "__main__":
    main()
