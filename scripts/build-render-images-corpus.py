#!/usr/bin/env python3
"""Writes corpus/render-images: Markdown documents (`.markdown`, so only the
`render-images` suite reads them) and the local images they reference.

The main corpus has no image files, so without this every ImageFragment in the
render suite draws a placeholder. These documents exercise the loaded path:
decode and downsample, the viewport height cap, scaling into the reading
column, the transparent-artwork matte, captions, and the blocked and missing
states. Images are generated deterministically (no dependencies beyond zlib);
the JPEG is converted with `sips`.
"""

import os
import struct
import subprocess
import zlib

ROOT = os.path.join(os.path.dirname(__file__), "..", "corpus", "render-images")
IMG = os.path.join(ROOT, "img")


def png(path, width, height, pixel, alpha):
    """RGBA or RGB PNG where `pixel(x, y)` returns the channels."""
    channels = 4 if alpha else 3
    raw = bytearray()
    for y in range(height):
        raw.append(0)
        for x in range(width):
            raw.extend(pixel(x, y)[:channels])

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", width, height, 8, 6 if alpha else 2, 0, 0, 0)
    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b"")
    with open(path, "wb") as handle:
        handle.write(data)


def gradient(width, height):
    def pixel(x, y):
        return (int(255 * x / max(1, width - 1)), int(255 * y / max(1, height - 1)), 160, 255)
    return pixel


def stripes(width, height, period):
    def pixel(x, y):
        band = ((x + y) // period) % 3
        return [(214, 92, 64, 255), (64, 140, 214, 255), (240, 200, 80, 255)][band]
    return pixel


def line_art(width, height):
    """Black ink on transparency: a ring and a diagonal."""
    cx, cy, r = width / 2, height / 2, min(width, height) * 0.38

    def pixel(x, y):
        d = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5
        ink = abs(d - r) < 3 or abs((x / width) - (y / height)) < 0.012
        return (0, 0, 0, 255) if ink else (0, 0, 0, 0)
    return pixel


def main():
    os.makedirs(IMG, exist_ok=True)
    png(os.path.join(IMG, "opaque.png"), 480, 270, gradient(480, 270), alpha=False)
    png(os.path.join(IMG, "wide.png"), 2400, 600, stripes(2400, 600, 60), alpha=False)
    png(os.path.join(IMG, "tall.png"), 320, 2600, stripes(320, 2600, 40), alpha=True)
    png(os.path.join(IMG, "alpha.png"), 400, 300, line_art(400, 300), alpha=True)
    png(os.path.join(IMG, "tiny.png"), 24, 24, gradient(24, 24), alpha=True)
    png(os.path.join(IMG, "photo-source.png"), 640, 427, gradient(640, 427), alpha=False)
    subprocess.run(
        ["sips", "-s", "format", "jpeg", "-s", "formatOptions", "80",
         os.path.join(IMG, "photo-source.png"), "--out", os.path.join(IMG, "photo.jpg")],
        check=True, stdout=subprocess.DEVNULL,
    )
    os.remove(os.path.join(IMG, "photo-source.png"))

    documents = {
        "opaque.markdown": "# Opaque image\n\nA paragraph before the picture.\n\n![A generated gradient](img/opaque.png)\n\nA paragraph after it.\n",
        "alpha.markdown": "# Line art\n\nTransparent artwork gets a plate in a dark theme.\n\n![](img/alpha.png)\n\nText under the art.\n",
        "wide.markdown": "# Wide image\n\nWider than the reading column, so it scales down to fit.\n\n![Stripes](img/wide.png)\n\nAfter.\n",
        "tall.markdown": "# Tall image\n\nTaller than the viewport cap.\n\n![A tall strip](img/tall.png)\n\nAfter.\n",
        "photo.markdown": "# JPEG\n\n![A photo](img/photo.jpg)\n\nThe caption above is the alt text.\n",
        "mixed.markdown": (
            "# Mixed\n\n- A list item with a small image below.\n\n  ![tiny](img/tiny.png)\n\n"
            "> Quoted before an image.\n\n![Gradient again](img/opaque.png)\n\n---\n\nThe end.\n"
        ),
        "failures.markdown": (
            "# Images that do not render\n\n![Missing](img/does-not-exist.png)\n\n"
            "![Outside the folder](../secret.png)\n\n![Absolute](/tmp/upleft-no-such-image.png)\n\n"
            "![Remote](https://example.com/picture.png)\n\nText after the failures.\n"
        ),
    }
    for name, text in documents.items():
        with open(os.path.join(ROOT, name), "w", encoding="utf-8") as handle:
            handle.write(text)
    print(f"corpus/render-images: {len(documents)} documents")


if __name__ == "__main__":
    main()
