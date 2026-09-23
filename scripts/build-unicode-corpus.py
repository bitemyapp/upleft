#!/usr/bin/env python3
"""Writes corpus/unicode/strings.txt, the input of the `unicode` conformance
suite: adversarial strings for grapheme segmentation, Character properties,
case mapping, trimming and canonical-equivalence comparisons.

Format: one string per line, `S` followed by its scalars in hex (`S` alone is
the empty string); `#` lines are comments. The first PAIRS strings are also
compared with each other (==, <, hasPrefix, hasSuffix, contains) by the suite.

`N <scalar> : <NFD> : <NFC>` lines list every scalar whose canonical
decomposition or composition is not itself (Python's unicodedata, which is on
the same decomposition data as Swift 6.4's stdlib and `unicode-normalization`
0.1.25 for every decomposable scalar); the suite checks Swift's `==` and `<`
across each triple, which is how Downright observes normalization.

`C <scalars> / <scalars>` lines are extra comparison pairs (==, <, and both
ways), random strings over scalars that stress the stdlib's comparison
shortcuts: combining marks, composition exclusions, singletons, Hangul jamo.

Deterministic: a fixed LCG, not Python's `random`. The output is committed.
"""

import os
import unicodedata

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "corpus", "unicode", "strings.txt")
PAIRS = 256


def cps(text):
    return [ord(c) for c in text]


strings = []
seen = set()


def add(scalars):
    key = tuple(scalars)
    if key in seen:
        return
    seen.add(key)
    strings.append(list(scalars))


# --- Pair set: canonical-equivalence and prefix/contains material -----------

pair_texts = [
    "", "a", "b", "A", "K", "k", "\u212A", "`", "\u1FEF", ";", "\u037E", "\u00E9", "e\u0301", "e", "e\u0301\u0323",
    "e\u0323\u0301", "\u1EB9\u0301", "\u1EC7", "e\u0302\u0323", "e\u0323\u0302", "a\u0301\u0301", "\u00C5", "A\u030A", "\u212B",
    "\u2126", "\u03A9", "\u1100\u1161", "\uAC00", "\u1100\u1161\u11A8", "\uAC01", "\uAC00\u11A8", "\r\n", "\r", "\n",
    "\n\r", "a\r\nb", "a\nb", "a\rb", "<", "<\u0338", "\u226E", "=\u0338", "\u2260", "*", "*\u0301", "*\u200D", "!\u0301",
    "\u0915\u094D\u0937", "\u0915\u094D", "\u0915", "\u0937", "\U0001F468\u200D\U0001F469\u200D\U0001F467", "\U0001F468",
    "\U0001F1FA\U0001F1F8", "\U0001F1FA", "\U0001F1FA\U0001F1F8\U0001F1EC", "\u0600a", "\u0600", "a\u0600", "\u00DF", "SS",
    "ss", "\u1E9E", "\u0130", "i\u0307", "I", "\u0131", "\u03A3", "\u03C3", "\u03C2", "\u01C5", "\u01C4", "\u01C6", "\uFB01",
    "fi", "A\u030A\u0301", "\u01FA", "\u00C5\u0301", "abc", "abd", "ab", "b\u0301c", "\u00E9t\u00E9", "e\u0301te\u0301",
    "caf\u00E9", "cafe\u0301", "cafe", "caf", "\u00E9\u0301", "e\u0301\u0301", "e\u0327\u0301", "\u0229\u0301",
    "\u1E1D", "\U0001D15E", "\U0001D157\U0001D165", "\u0F73", "\u0F71\u0F72", "\u2000", "\u2002", "\u3000", " ", "\t",
    "\u00A0", "\u200B", "\u2028", "\u0085", "x\u0308\u0301", "x\u0301\u0308", "\u0344", "\u0308\u0301", "\u0301",
    "\u0301a", "a\u0301b", "\u00E1b", "\u0958", "\u0915\u093C", "\uF900", "\u8C48", "\U0002F800", "\u4E3D", "\u00BD", "1/2",
    "\u2460", "1", "\u0661", "\u4E00", "\u00AA", "\u02B0", "\U00016D63", "\U00016D67", "\U00016D40\U00016D63",
    "\U00016D40\U00016D67\U00016D67", "\u1B44\u1B13", "\u0CCD\u0C95", "\u17D2\u1780", "\u1039\u1000", "\u0E33", "\u0EB3",
    "\u0E01\u0E33", "a\u0E33", "\U0001F3FB", "\U0001F44D\U0001F3FB", "\U0001F44D", "\u2764\uFE0F", "\u2764",
    "\U000E0067", "\U0001F3F4\U000E0067\U000E0062\U000E0065\U000E006E\U000E0067\U000E007F", "\U0001F3F4",
]
for text in pair_texts:
    add(cps(text))

# NFC/NFD forms and combining-mark permutations of some decomposable scalars.
decomposable = [c for c in range(0xC0, 0x250) if unicodedata.decomposition(chr(c)) and not unicodedata.decomposition(chr(c)).startswith("<")]
for c in decomposable[:40]:
    add(cps(unicodedata.normalize("NFD", chr(c))))
    add([c])

assert len(strings) <= PAIRS, len(strings)
while len(strings) < PAIRS:
    # Pad the pair set with two-scalar mixes of what is already there.
    base = strings[len(strings) % 97]
    other = strings[(len(strings) * 7) % 131]
    add((base + other)[:6] or [0x61])
    if len(strings) < PAIRS:
        add((other + base)[:5] or [0x62])
pair_count = len(strings)
assert pair_count == PAIRS

# --- Everything else: one representative scalar per grapheme class, in
# combinations, plus canonical-equivalence and case material. ---------------

pool = [
    0x61, 0x20, 0x0D, 0x0A, 0x09, 0x0B, 0x7F, 0x85, 0xAD, 0x300, 0x301, 0x323, 0x34F, 0x200C, 0x200D, 0xFE0F, 0x1F3FB,
    0xE0020, 0x903, 0x93F, 0x600, 0x6DD, 0x110BD, 0x1100, 0x1160, 0x11A8, 0xAC00, 0xAC01, 0x1F1E6, 0x1F1E7, 0x1F600, 0x2764,
    0xA9, 0x915, 0x94D, 0x937, 0x9CD, 0x995, 0x1039, 0x1000, 0x17D2, 0x1780, 0xD4D, 0xD15, 0x16D40, 0x16D63, 0x16D67, 0x16D6A,
    0x2028, 0x2029, 0x3042, 0x4E00, 0xE33, 0xEB3, 0x1B44, 0xBCD, 0xB95, 0xA8F1, 0x11F42, 0xD3B, 0x1F9D1, 0x1F4BB, 0x303,
    0x93C, 0xA3C, 0xCBC, 0x2640, 0x212A, 0x1FEF, 0x37E, 0xDF, 0x130, 0x3A3, 0x1C5, 0xFB01, 0x149, 0x1E9E, 0x3000, 0xA0,
    0x200B, 0x0660, 0x2160, 0x24B6, 0x1D400, 0x10400, 0x10428, 0xFF21, 0xFF41,
]

seed = 0x2545F4914F6CDD1D


def next_random():
    global seed
    seed = (seed * 6364136223846793005 + 1442695040888963407) & 0xFFFFFFFFFFFFFFFF
    return seed >> 33


for _ in range(3000):
    length = next_random() % 7 + 1
    add([pool[next_random() % len(pool)] for _ in range(length)])

# Every pool scalar after and before every other one (pairs exercise each
# break rule directly).
for a in pool:
    for b in pool[::3]:
        add([a, b])

compare_pool = [
    0x61, 0x62, 0x65, 0x4B, 0x3B, 0x60, 0x301, 0x302, 0x308, 0x323, 0x327, 0x344, 0x93C, 0xF71, 0xF72, 0xF73, 0xE1, 0xE9,
    0x1EB9, 0x1EC7, 0x1100, 0x1161, 0x11A8, 0xAC00, 0xAC01, 0x915, 0x958, 0x212A, 0x37E, 0x1FEF, 0x2B0, 0x1F600, 0x2126,
    0x3A9, 0xC5, 0x212B, 0x1D15E, 0x1D157, 0x1D165, 0x105C9, 0x105D2, 0x307, 0xF900, 0x8C48, 0x2F800, 0x4E3D,
]
compare_pairs = []
for _ in range(20000):
    a = [compare_pool[next_random() % len(compare_pool)] for _ in range(next_random() % 5 + 1)]
    b = [compare_pool[next_random() % len(compare_pool)] for _ in range(next_random() % 5 + 1)]
    if next_random() % 3 == 0:
        # A shared prefix, so the comparison diverges mid-string.
        b = a[: next_random() % (len(a) + 1)] + b
    compare_pairs.append((a, b))

lines = [
    "# corpus/unicode/strings.txt: generated by scripts/build-unicode-corpus.py; do not edit.",
    f"# The first {PAIRS} strings form the pair set (==, <, hasPrefix, hasSuffix, contains).",
]
for scalars in strings:
    lines.append("S" + "".join(" %X" % s for s in scalars))
for a, b in compare_pairs:
    lines.append("C%s /%s" % ("".join(" %X" % s for s in a), "".join(" %X" % s for s in b)))
for value in range(0x110000):
    if 0xD800 <= value <= 0xDFFF:
        continue
    char = chr(value)
    nfd = unicodedata.normalize("NFD", char)
    nfc = unicodedata.normalize("NFC", char)
    if nfd != char or nfc != char:
        lines.append(
            "N %X :%s :%s" % (value, "".join(" %X" % ord(c) for c in nfd), "".join(" %X" % ord(c) for c in nfc))
        )
os.makedirs(os.path.dirname(OUT), exist_ok=True)
with open(OUT, "w", encoding="ascii") as handle:
    handle.write("\n".join(lines) + "\n")
print(f"corpus/unicode/strings.txt: {len(strings)} strings, {PAIRS} in the pair set")
