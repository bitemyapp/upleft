#!/usr/bin/env python3
"""The app is always "Upleft" (owner's decision, see AGENTS.md). This script
applies that identity mechanically so the Swift reference and the Rust port
carry exactly the same visible strings.

  scripts/rebrand.py copy     build target/rebranded/downright: a copy of
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
# The directory is named `downright` so SwiftPM gives the path package the
# identity `downright`, which the oracles reference by name.
COPY = os.path.join(ROOT, "target", "rebranded", "downright")

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
RUST_LITERAL = re.compile(r"'(?:[^'\\\n]|\\.)'" + r'|b?r(#*)"[\s\S]*?"\1' + r'|"(?:[^"\\]|\\.)*"')


def rebrand_rust_literals(source):
    def replace(match):
        text = match.group(0)
        return text if text.startswith("'") else rebrand_text(text)
    return RUST_LITERAL.sub(replace, source)


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


def transform_for(relative, name):
    """The rewrite for one file of the copy, or None to copy it verbatim."""
    if name == "Package.swift":
        return None
    if name.endswith(".swift"):
        return rebrand_literals
    if name.endswith(".plist") and relative.startswith("Config"):
        return rebrand_text
    if relative in ("Resources/Welcome.md",):
        return rebrand_text
    if relative.startswith("Scripts") and name.endswith(".sh"):
        return rebrand_script_identity
    return None


SKIPPED_DIRECTORIES = {".git", ".swiftpm", "node_modules"}


def copy():
    """Syncs the rebranded copy incrementally: a file is written only when its
    rebranded bytes differ from what is already there, so modification times —
    and therefore SwiftPM's incremental builds — survive repeated runs."""
    if not os.path.exists(os.path.join(SOURCE, "Package.swift")):
        sys.exit("vendor/downright is missing; run `git submodule update --init`")
    wanted = set()
    written = 0
    rebranded = 0
    for directory, subdirectories, files in os.walk(SOURCE):
        subdirectories[:] = [d for d in subdirectories if d not in SKIPPED_DIRECTORIES and not d.startswith(".build")]
        for name in files:
            source_path = os.path.join(directory, name)
            relative = os.path.relpath(source_path, SOURCE)
            destination = os.path.join(COPY, relative)
            wanted.add(relative)
            if os.path.islink(source_path):
                link = os.readlink(source_path)
                if not (os.path.islink(destination) and os.readlink(destination) == link):
                    os.makedirs(os.path.dirname(destination), exist_ok=True)
                    if os.path.lexists(destination):
                        os.remove(destination)
                    os.symlink(link, destination)
                    written += 1
                continue
            with open(source_path, "rb") as handle:
                data = handle.read()
            transform = transform_for(relative, name)
            if transform is not None:
                text = data.decode("utf-8")
                changed = transform(text)
                if changed != text:
                    rebranded += 1
                data = changed.encode("utf-8")
            try:
                with open(destination, "rb") as handle:
                    if handle.read() == data:
                        continue
            except FileNotFoundError:
                pass
            os.makedirs(os.path.dirname(destination), exist_ok=True)
            with open(destination, "wb") as handle:
                handle.write(data)
            shutil.copymode(source_path, destination)
            written += 1
    # Remove files that no longer exist upstream (build products live in
    # .build* directories, which are left alone).
    for directory, subdirectories, files in os.walk(COPY):
        subdirectories[:] = [d for d in subdirectories if not d.startswith(".build")]
        for name in files:
            relative = os.path.relpath(os.path.join(directory, name), COPY)
            if relative not in wanted:
                os.remove(os.path.join(directory, name))
    print(f"target/rebranded/downright: {rebranded} files rebranded, {written} written")


def check():
    remaining = []
    roots = [os.path.join(ROOT, "crates"), os.path.join(ROOT, "oracle", "Sources"),
             os.path.join(ROOT, "oracle", "app", "Sources", "downright-app-oracle")]
    for directory, subdirectories, files in (entry for root in roots for entry in os.walk(root)):
        subdirectories[:] = [d for d in subdirectories if d not in ("target", ".build")]
        for name in files:
            if not name.endswith((".rs", ".swift")):
                continue
            path = os.path.join(directory, name)
            with open(path, encoding="utf-8") as handle:
                text = handle.read()
            if f"{os.sep}tests{os.sep}" in path:
                # Test fixtures are content, not the app's identity; several
                # pin offsets and slugs that depend on the word's spelling.
                continue
            pattern = RUST_LITERAL if name.endswith(".rs") else LITERAL
            for match in pattern.finditer(text):
                literal = match.group(0)
                if literal.startswith("'"):
                    continue
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
