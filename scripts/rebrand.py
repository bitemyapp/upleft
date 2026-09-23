#!/usr/bin/env python3
"""The app is always "Upleft" (owner's decision, see AGENTS.md). This script
applies that identity mechanically so the Swift reference and the Rust port
carry exactly the same visible strings.

  scripts/rebrand.py copy     build target/downright-upleft: a copy of
                              vendor/downright with the rules below applied
                              (vendor/ itself is never touched)
  scripts/rebrand.py check    list string literals in crates/ that still say
                              "Downright" or com.ezzy (exit 1 if any)

The rules, applied only inside string literals, Info.plists and the bundled
Markdown resources — never to type, module, file or symbol names:

  https://github.com/ezzy1630/Downright…     → https://github.com/bitemyapp/upleft…
  https://ezzy1630.github.io/Downright/…     → https://bitemyapp.github.io/upleft/…
  com.ezzy.downright                         → com.bitemyapp.upleft
  Downright                                  → Upleft

Never touched: `Package.swift` (its literals are target and product names),
and in the bundling scripts everything but the APP_NAME and BUNDLE_ID lines.

Left alone on purpose: environment variables (DOWNRIGHT_DEBUG_*), defaults
keys ("downright.theme.selected" …) and other lowercase identifiers, which
no reader sees and which only ever live inside Upleft's own domain.
"""

import os
import re
import shutil
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SOURCE = os.path.join(ROOT, "vendor", "downright")
COPY = os.path.join(ROOT, "target", "downright-upleft")

RULES = [
    (re.compile(r"https://github\.com/ezzy1630/Downright"), "https://github.com/bitemyapp/upleft"),
    (re.compile(r"https://ezzy1630\.github\.io/Downright/"), "https://bitemyapp.github.io/upleft/"),
    (re.compile(r"com\.ezzy\.downright"), "com.bitemyapp.upleft"),
    # Only the standalone word. `DownrightApp`, `DownrightQL.appex`,
    # `Downright_MarkdownRender.bundle`, `@_cdecl("DownrightSpotlight…")` are
    # module, product, resource-bundle and symbol names that the build and the
    # C importer depend on, so they keep their spelling.
    (re.compile(r"\bDownright\b"), "Upleft"),
]

# Swift and Rust string literals, single-line and multi-line. Interpolations
# (`\(…)` in Swift, `{…}` in Rust format strings) stay inside the literal and
# are rewritten with it, which is harmless: no rule matches an identifier.
LITERAL = re.compile(r'"""[\s\S]*?"""|"(?:[^"\\\n]|\\.)*"')


def rebrand_text(text):
    for pattern, replacement in RULES:
        text = pattern.sub(replacement, text)
    return text


def rebrand_literals(source):
    return LITERAL.sub(lambda match: rebrand_text(match.group(0)), source)


def rebrand_script_identity(source):
    """Only the APP_NAME= and BUNDLE_ID= assignments of a bundling script."""
    return "\n".join(
        rebrand_text(line) if line.startswith(("APP_NAME=", "BUNDLE_ID=")) else line
        for line in source.split("\n")
    )


def copy():
    if not os.path.exists(os.path.join(SOURCE, "Package.swift")):
        sys.exit("vendor/downright is missing; run `git submodule update --init`")
    ignore = shutil.ignore_patterns(".build*", ".git", ".swiftpm", "node_modules")
    if os.path.exists(COPY):
        shutil.rmtree(COPY)
    shutil.copytree(SOURCE, COPY, ignore=ignore, symlinks=True)
    changed = 0
    for directory, _, files in os.walk(COPY):
        for name in files:
            path = os.path.join(directory, name)
            relative = os.path.relpath(path, COPY)
            if name == "Package.swift":
                continue
            if name.endswith(".swift"):
                transform = rebrand_literals
            elif name.endswith(".plist") and relative.startswith("Config"):
                transform = rebrand_text
            elif relative in ("Resources/Welcome.md",):
                transform = rebrand_text
            elif relative.startswith("Scripts") and name.endswith(".sh"):
                transform = rebrand_script_identity
            else:
                continue
            with open(path, encoding="utf-8") as handle:
                before = handle.read()
            after = transform(before)
            if after != before:
                with open(path, "w", encoding="utf-8") as handle:
                    handle.write(after)
                changed += 1
    print(f"target/downright-upleft: {changed} files rebranded")


def check():
    remaining = []
    for directory, subdirectories, files in os.walk(os.path.join(ROOT, "crates")):
        subdirectories[:] = [d for d in subdirectories if d not in ("target", ".build")]
        for name in files:
            if not name.endswith((".rs", ".swift")):
                continue
            path = os.path.join(directory, name)
            with open(path, encoding="utf-8") as handle:
                text = handle.read()
            for match in LITERAL.finditer(text):
                literal = match.group(0)
                if re.search(r"\bDownright\b", literal) or "com.ezzy" in literal:
                    line = text.count("\n", 0, match.start()) + 1
                    remaining.append(f"{os.path.relpath(path, ROOT)}:{line}: {literal[:100]}")
    for entry in remaining:
        print(entry)
    print(f"{len(remaining)} literal(s) still carry Downright's identity")
    sys.exit(1 if remaining else 0)


if __name__ == "__main__":
    command = sys.argv[1] if len(sys.argv) > 1 else ""
    if command == "copy":
        copy()
    elif command == "check":
        check()
    else:
        sys.exit(__doc__)
