#!/usr/bin/env python3
"""Builds corpus/math, the LaTeX the math suites run, from the pinned
submodules and the hand-picked list below. Deterministic; the output is
committed (corpus/generated/math holds the formulas found in the Markdown
corpus and is rebuilt by build-corpus.py).

  examples/  every LaTeX block in SwiftMath's EXAMPLES.md and README.md, and
             the string literals of README's Swift snippets
  tests/     every string literal in SwiftMath's test suite and in Downright's
             math tests (valid LaTeX, invalid LaTeX, and plain strings alike)
  hard/      HARD below: every construct Downright can reach, and the errors

Each expression is written twice, as `<name>-inline.tex` and
`<name>-display.tex`: the first line names the style, the rest is the LaTeX.

A handful of inputs trap SwiftMath itself (and so take Downright down); they
cannot be compared and are listed in CRASHES, which is skipped.
"""

import os
import re
import shutil
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "corpus", "math")
SWIFTMATH = os.path.join(ROOT, "vendor", "downright", "Vendor", "SwiftMath")
DOWNRIGHT_TESTS = os.path.join(ROOT, "vendor", "downright", "Tests")

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from importlib import import_module  # noqa: E402

unescape_swift = import_module("build-corpus").unescape_swift

# Inputs on which SwiftMath traps. `\textcolor` after another atom reads the
# first sub-display of its content as a line (`(subDisplay as? MTCTLineDisplay)!`
# and `.atoms[0]`), so anything else there — or nothing — is a crash.
CRASHES = {
    "a\\textcolor{#ff0000}{\\frac{1}{2}}",
    "a\\textcolor{#ff0000}{}",
}

HARD = [
    # Fractions
    ("frac-simple", r"\frac{a}{b}"),
    ("frac-digits", r"\frac12"),
    ("frac-nested", r"\frac{\frac{1}{2}}{\frac{3}{4}}"),
    ("frac-deep", r"\frac{1}{1+\frac{1}{1+\frac{1}{1+\frac{1}{x}}}}"),
    ("frac-over", r"a \over b"),
    ("frac-over-group", r"5 + {x+1 \over y-1} + 8"),
    ("frac-atop", r"{n \atop k}"),
    ("frac-choose", r"{n \choose k}"),
    ("frac-brack", r"{n \brack k}"),
    ("frac-brace", r"{n \brace k}"),
    ("frac-binom", r"\binom{n}{k} = \frac{n!}{k!(n-k)!}"),
    ("frac-binom-script", r"x^{\binom{n}{2}}"),
    ("frac-in-script", r"e^{\frac{1}{2}x} + x_{\frac{a}{b}}"),
    ("frac-tall", r"\frac{x^2+1}{\sqrt{x}} - \frac{\sum_{i=1}^n i}{n^2}"),
    ("frac-scripts", r"\frac{a}{b}^2_3"),
    ("frac-empty", r"\frac{}{}"),
    ("frac-missing", r"\frac"),
    ("frac-dfrac", r"\dfrac{a}{b}"),
    # Radicals
    ("sqrt-simple", r"\sqrt{2}"),
    ("sqrt-bare", r"\sqrt2"),
    ("sqrt-degree", r"\sqrt[3]{x}"),
    ("sqrt-degree-long", r"\sqrt[\alpha\beta\gamma\delta]{x+y}"),
    ("sqrt-degree-empty", r"\sqrt[]{x}"),
    ("sqrt-nested", r"\sqrt{\sqrt{\sqrt{x}}}"),
    ("sqrt-tall", r"\sqrt{\frac{\frac{a}{b}}{\frac{c}{d}}}"),
    ("sqrt-very-tall", r"\sqrt{\frac{\frac{\frac{a}{b}}{\frac{c}{d}}}{\frac{\frac{e}{f}}{\frac{g}{h}}}}"),
    ("sqrt-matrix", r"\sqrt{\begin{matrix} a & b \\ c & d \\ e & f \end{matrix}}"),
    ("sqrt-scripts", r"\sqrt{2}^2 + \sqrt[n]{x}_i"),
    ("sqrt-empty", r"\sqrt{}"),
    ("sqrt-end", r"\sqrt"),
    ("sqrt-degree-open", r"\sqrt[3"),
    ("sqrt-spacing", r"\sqrt{4}4 + 2\sqrt{2}"),
    # Large operators
    ("op-sum", r"\sum_{i=0}^{n} i^2"),
    ("op-sum-bare", r"\sum"),
    ("op-sum-upper", r"\sum^n x"),
    ("op-sum-lower", r"\sum_{k} a_k"),
    ("op-sum-nolimits", r"\sum\nolimits_{i=1}^{n} x_i"),
    ("op-int", r"\int_0^1 f(x)\,dx"),
    ("op-int-limits", r"\int\limits_a^b g(t)\,dt"),
    ("op-int-bare", r"\int f"),
    ("op-int-sub", r"\int_{-\infty}^{\infty} e^{-x^2} dx = \sqrt{\pi}"),
    ("op-oint", r"\oint_C \vec{F}\cdot d\vec{r}"),
    ("op-prod", r"\prod_{k=1}^{\infty} \left(1-\frac{1}{p_k^s}\right)^{-1}"),
    ("op-coprod", r"\coprod_{i} A_i"),
    ("op-big", r"\bigcup_{i=1}^n A_i \cap \bigcap_j B_j \bigoplus_k V_k \bigotimes W \bigvee \bigwedge \bigodot \biguplus \bigsqcup"),
    ("op-lim", r"\lim_{x\to0}\frac{\sin x}{x} = 1"),
    ("op-limsup", r"\limsup_{n\to\infty} a_n \le \liminf_{n\to\infty} b_n"),
    ("op-named", r"\max_{x\in X} f(x) + \min_y g(y) + \sup S + \inf T + \det A + \Pr(E) + \gcd(a,b)"),
    ("op-functions", r"\sin x + \cos y + \tan z + \log_2 n + \ln x + \exp(x) + \arcsin a + \sinh b + \cot c + \sec d + \csc e + \arg z + \ker f + \dim V + \hom + \deg p + \lg x + \arccos + \arctan + \cosh + \tanh + \coth"),
    ("op-functions-scripts", r"\sin^2 x + \cos^2 x = 1"),
    ("op-limits-error", r"x\limits"),
    ("op-nolimits-error", r"\nolimits"),
    ("op-sum-limits", r"\sum\limits_{i} a"),
    ("op-int-italic", r"\int_a^b \int_c^d f\,dy\,dx"),
    # Tables and environments
    ("env-matrix", r"\begin{matrix} a & b \\ c & d \end{matrix}"),
    ("env-pmatrix", r"\begin{pmatrix} 1 & 0 \\ 0 & 1 \end{pmatrix}"),
    ("env-bmatrix", r"\begin{bmatrix} a & b & c \\ d & e & f \end{bmatrix}"),
    ("env-Bmatrix", r"\begin{Bmatrix} x \\ y \end{Bmatrix}"),
    ("env-vmatrix", r"\det \begin{vmatrix} a & b \\ c & d \end{vmatrix} = ad - bc"),
    ("env-Vmatrix", r"\begin{Vmatrix} x & y \end{Vmatrix}"),
    ("env-matrix-ragged", r"\begin{pmatrix} a & b & c \\ d \\ e & f \end{pmatrix}"),
    ("env-matrix-tall", r"\begin{bmatrix} \frac{a}{b} & \sum_i^n x \\ \int_0^1 & \sqrt{\frac{1}{2}} \end{bmatrix}"),
    ("env-matrix-empty", r"\begin{matrix}\end{matrix}"),
    ("env-matrix-nested", r"\begin{pmatrix} \begin{matrix} a & b \\ c & d \end{matrix} & 0 \\ 0 & 1 \end{pmatrix}"),
    ("env-cases", r"f(x) = \begin{cases} x & x \geq 0 \\ -x & x < 0 \end{cases}"),
    ("env-cases-columns", r"\begin{cases} a \end{cases}"),
    ("env-aligned", r"\begin{aligned} a &= b + c \\ d &= e \end{aligned}"),
    ("env-eqalign", r"\begin{eqalign} x &= 1 \\ y &= 2 \end{eqalign}"),
    ("env-split", r"\begin{split} a &= b \\ &= c \end{split}"),
    ("env-aligned-columns", r"\begin{aligned} a & b & c \end{aligned}"),
    ("env-gather", r"\begin{gather} a = b \\ c = d \end{gather}"),
    ("env-displaylines", r"\begin{displaylines} x \\ y + z \end{displaylines}"),
    ("env-gather-columns", r"\begin{gather} a & b \end{gather}"),
    ("env-eqnarray", r"\begin{eqnarray} x & = & y \\ z & \le & w \end{eqnarray}"),
    ("env-eqnarray-columns", r"\begin{eqnarray} x & y \end{eqnarray}"),
    ("env-default-row", r"a \\ b"),
    ("env-default-cols", r"a & b \\ c & d"),
    ("env-default-cr", r"x \cr y"),
    ("env-unknown", r"\begin{array} a \end{array}"),
    ("env-mismatch", r"\begin{matrix} a \end{pmatrix}"),
    ("env-missing-end", r"\begin{matrix} a & b"),
    ("env-missing-begin", r"a \end{matrix}"),
    ("env-missing-brace", r"\begin matrix"),
    ("env-unclosed-name", r"\begin{matrix"),
    ("env-spaces-name", r"\begin{ matrix } a \end{matrix}"),
    # Accents
    ("accent-hat", r"\hat{x}"),
    ("accent-widehat", r"\widehat{xyz}"),
    ("accent-tilde", r"\tilde{a} + \widetilde{abc}"),
    ("accent-bar", r"\bar{x} + \bar{XY}"),
    ("accent-vec", r"\vec{v} + \vec{AB}"),
    ("accent-dots", r"\dot{x} + \ddot{y}"),
    ("accent-others", r"\acute{e}\grave{a}\breve{u}\check{c}"),
    ("accent-script", r"\hat{x}^2 + \vec{v}_i + \bar{x}_i^2"),
    ("accent-nested", r"\hat{\hat{x}}"),
    ("accent-unicode", r"á é í ó ú ý à è ì ò ù â ê î ô û ä ë ï ö ü ÿ ã ñ õ"),
    ("accent-unicode-upper", r"Á É Í Ó Ú Ý À È Ì Ò Ù Â Ê Î Ô Û Ä Ë Ï Ö Ü Ã Ñ Õ"),
    ("accent-unicode-special", r"ç ø å æ œ ß Ç Ø Å Æ Œ \aa \ae \o \oe \ss \cc \CC \O \AE \OE \AA \angstrom"),
    ("accent-decomposed", "é + ñ"),
    ("accent-imath", r"\hat \imath + \vec \jmath"),
    ("accent-empty", r"\hat{}"),
    ("accent-fraction", r"\hat{\frac{a}{b}}"),
    # Over/under lines, braces (unsupported)
    ("overline", r"\overline{x+y}"),
    ("overline-nested", r"\overline{\overline{x}}"),
    ("overline-script", r"\overline{z}^2"),
    ("underline", r"\underline{abc}"),
    ("underline-script", r"\underline{x}_1"),
    ("underline-fraction", r"\underline{\frac{a}{b}}"),
    ("overbrace", r"\overbrace{a+b}^{n}"),
    ("underbrace", r"\underbrace{x+y}_{2}"),
    # Delimiters
    ("left-paren", r"\left( \frac{a}{b} \right)"),
    ("left-bracket", r"\left[ x \right]"),
    ("left-brace", r"\left\{ x \in X \right\}"),
    ("left-lbrace", r"\left\lbrace x \right\rbrace"),
    ("left-bars", r"\left| x \right| + \left\| v \right\| + \left\vert y \right\vert + \left\Vert z \right\Vert"),
    ("left-angle", r"\left\langle \psi \right\rangle + \left< a \right>"),
    ("left-floor", r"\left\lfloor \frac{n}{2} \right\rfloor + \left\lceil \frac{n}{2} \right\rceil"),
    ("left-group", r"\left\lgroup x \right\rgroup"),
    ("left-arrows", r"\left\uparrow x \right\downarrow + \left\Uparrow y \right\Downarrow + \left\updownarrow z \right\Updownarrow"),
    ("left-slash", r"\left/ x \right\backslash + \left\\ y \right/"),
    ("left-dot", r"\left. \frac{df}{dx} \right|_{x=0}"),
    ("left-dot-both", r"\left. x \right."),
    ("left-tall", r"\left( \frac{\frac{a}{b}}{\frac{c}{d}} \right)"),
    ("left-very-tall", r"\left( \begin{matrix} a \\ b \\ c \\ d \\ e \\ f \end{matrix} \right)"),
    ("left-sum", r"\left( \sum_{i=1}^{n} a_i \right)^2"),
    ("left-nested", r"\left( a + \left[ b + \left\{ c \right\} \right] \right)"),
    ("left-script", r"\left( x \right)^2_i"),
    ("left-missing-right", r"\left( x"),
    ("left-missing-left", r"x \right)"),
    ("left-missing-delim", r"\left"),
    ("left-invalid-delim", r"\left x \right)"),
    ("left-invalid-command", r"\left\alpha x \right)"),
    ("left-spaces", r"\left  (  x  \right  )"),
    # Font styles
    ("font-mathbb", r"\mathbb{R} \mathbb{C} \mathbb{N} \mathbb{Z} \mathbb{Q} \mathbb{H} \mathbb{P} \mathbb{ABXY} \mathbb{abz} \mathbb{0189}"),
    ("font-mathcal", r"\mathcal{ABCDEFGHIJKLMNOPQRSTUVWXYZ}"),
    ("font-mathcal-lower", r"\mathcal{abcdefgoz} \mathcal{\alpha 12}"),
    ("font-mathrm", r"\mathrm{d}x + \mathrm{sin}(x) + \mathrm{ABC123}"),
    ("font-mathbf", r"\mathbf{v} + \mathbf{\alpha\Gamma} + \mathbf{0123} + \mathbf{AZaz}"),
    ("font-mathit", r"\mathit{abc} + \mathit{h} + \mathit{\Omega}"),
    ("font-mathtt", r"\mathtt{code} + \mathtt{XYZ09}"),
    ("font-mathsf", r"\mathsf{ABC} + \mathsf{xyz} + \mathsf{789}"),
    ("font-mathfrak", r"\mathfrak{g} + \mathfrak{CHIRZ} + \mathfrak{ABab}"),
    ("font-bm", r"\bm{x} + \mathbfit{\alpha} + \bm{A1}"),
    ("font-text", r"\text{if } x > 0 \text{ and } y"),
    ("font-textstyles", r"\textrm{abc} \textbf{x} \textit{y} \texttt{z} \textsf{w}"),
    ("font-old", r"{\cal L} + {\bf x} + {\rm d} + {\frak g} + {\mit v}"),
    ("font-scope", r"\mathbf x y \mathrm{AB} C"),
    ("font-greek", r"\mathbf{\epsilon\vartheta\phi\varrho\varpi\varsigma}"),
    ("font-mathnormal", r"\mathnormal{abc}"),
    # Spacing
    ("space-commands", r"a\,b\>c\;d\!e\quad f\qquad g\ h"),
    ("space-colon", r"a\:b"),
    ("space-tilde", r"a~b"),
    ("space-leading", r"\quad x"),
    ("space-only", r"\,\;\quad"),
    ("space-negative", r"\!\!\!x"),
    # Greek and symbols
    ("greek-lower", r"\alpha\beta\gamma\delta\epsilon\varepsilon\zeta\eta\theta\vartheta\iota\kappa\lambda\mu\nu\xi\omicron\pi\varpi\rho\varrho\sigma\varsigma\tau\upsilon\phi\varphi\chi\psi\omega"),
    ("greek-upper", r"\Gamma\Delta\Theta\Lambda\Xi\Pi\Sigma\Upsilon\Phi\Psi\Omega"),
    ("symbols-misc", r"\infty \partial \nabla \forall \exists \emptyset \hbar \ell \Re \Im \aleph \wp \mho \top \bot \angle \triangle \prime \degree \neg \lnot \lbar"),
    ("symbols-dots", r"a_1, \ldots, a_n \cdots \vdots \ddots"),
    ("symbols-arrows", r"\leftarrow \rightarrow \leftrightarrow \Leftarrow \Rightarrow \Leftrightarrow \longleftarrow \longrightarrow \Longleftrightarrow \uparrow \downarrow \mapsto \nearrow \searrow \nwarrow \swarrow \gets \to \iff"),
    ("symbols-relations", r"a \leq b \geq c \neq d \approx e \equiv f \sim g \simeq h \cong i \propto j \ll k \gg l \prec m \succ n \models o \perp p \mid q \parallel r \asymp s \doteq t"),
    ("symbols-sets", r"A \subset B \supset C \subseteq D \supseteq E \sqsubset F \sqsupset G \sqsubseteq H \sqsupseteq I \in J \notin K \ni L"),
    ("symbols-binary", r"a \pm b \mp c \times d \div e \cdot f \circ g \bullet h \ast i \star j \oplus k \ominus l \otimes m \oslash n \odot o \cup p \cap q \setminus r \wedge s \vee t \dagger u \ddagger v \amalg w \wr x \uplus y \sqcap z \sqcup"),
    ("symbols-aliases", r"a \ne b \le c \ge d \land e \lor f \lnot g"),
    ("symbols-escapes", r"\{ x \} \$ \% \# \_ \& \backslash \| \vert"),
    ("symbols-punct", r"f\colon A \to B, a:b; x \cdotp y"),
    ("symbols-square", r"\square + x"),
    ("symbols-latex-chars", r"|x| + [a,b) + (a) + x! + n? + a/b + a*b + @ + ` + \" + \upquote"),
    # Scripts
    ("script-super", r"x^2"),
    ("script-sub", r"x_i"),
    ("script-both", r"x_i^2 + x^2_i"),
    ("script-nested", r"x^{2^{2^2}} + x_{i_{j_k}}"),
    ("script-pre", r"{}^{14}_{6}C"),
    ("script-empty-base", r"^2 + _3"),
    ("script-double", r"x^2^3 + x_1_2"),
    ("script-prime", r"f'(x) + f''(x) + x'"),
    ("script-euler", r"e^{i\pi}+1=0"),
    ("script-long", r"x^{a+b+c+d} + y_{i,j,k,l}"),
    ("script-empty", r"x^{} + y_{}"),
    ("script-open", r"x^"),
    ("script-italic", r"f^2 + V_1 + P^a_b + T^2"),
    ("script-number", r"10^{-3} + 2^{10}"),
    # Colours
    ("color", r"\color{#ff0000}{x+y} = z"),
    ("color-textcolor", r"\textcolor{#00aa00}{abc} + d"),
    ("color-textcolor-after", r"a + \textcolor{#0000ff}{b}"),
    ("color-colorbox", r"\colorbox{#ffff00}{x^2}"),
    ("color-colorbox-frac", r"\colorbox{#cccccc}{\frac{a}{b}}"),
    ("color-named", r"\color{red}{x}"),
    ("color-short", r"\color{#12}{x} + \color{#abcxyz}{y}"),
    ("color-nested", r"\color{#ff0000}{a + \color{#0000ff}{b} + c}"),
    ("color-missing", r"\color x"),
    ("color-unclosed", r"\color{#fff"),
    # Style switches
    ("style-display", r"\displaystyle \sum_{i}^{n} \frac{a}{b}"),
    ("style-text", r"\textstyle \sum_{i}^{n} \frac{a}{b}"),
    ("style-script", r"\scriptstyle x + \frac{a}{b}"),
    ("style-scriptscript", r"\scriptscriptstyle y^2"),
    ("style-mixed", r"a \scriptstyle b \displaystyle c"),
    # Downright's rewrite and trimming
    ("mathop", r"\mathop{\mathrm{read}}(source) \longrightarrow \mathop{\mathrm{parse}}(tree)"),
    ("mathop-nested", r"\mathop{a{b}c}_x"),
    ("mathop-unclosed", r"\mathop{x"),
    ("mathop-bare", r"\mathop x"),
    ("whitespace", "  \n\tx + y \n  "),
    ("whitespace-only", " \t\n "),
    ("empty", ""),
    # Errors
    ("error-open-brace", r"\frac{"),
    ("error-close-brace", r"x}"),
    ("error-lone-open", r"{"),
    ("error-command", r"\notacommand"),
    ("error-command-digits", r"\123"),
    ("error-backslash-end", "x\\"),
    ("error-internal", r"\left(\right"),
    # Characters SwiftMath ignores or passes through
    ("unicode-greek", "αβγ + x"),
    ("unicode-relation", "x ≤ y"),
    ("unicode-emoji", "x + 😀"),
    ("unicode-cyrillic", r"\text{Привет} + й"),
    ("unicode-cjk", "中文 x"),
    ("control-characters", "x\u0007y"),
    ("crlf", "a\r\nb"),
    # Numbers and operators
    ("numbers", r"3.14159 + 1,000 + .5 + 2."),
    ("operators-unary", r"-x + (-y) = +z - -w"),
    ("operators-trailing", r"x + y -"),
    ("relations", r"a = b < c > d"),
    # Long formulas
    ("long-maxwell", r"\nabla \times \vec{\mathbf{B}} - \frac{1}{c}\frac{\partial\vec{\mathbf{E}}}{\partial t} = \frac{4\pi}{c}\vec{\mathbf{j}}"),
    ("long-basel", r"\sum_{n=1}^{\infty} \frac{1}{n^2} = \frac{\pi^2}{6}"),
    ("long-gauss", r"\frac{1}{\sigma\sqrt{2\pi}} \exp\left( -\frac{(x-\mu)^2}{2\sigma^2} \right)"),
    ("long-taylor", r"f(x) = \sum_{n=0}^\infty \frac{f^{(n)}(a)}{n!}(x-a)^n"),
    ("long-fourier", r"\hat{f}(\xi) = \int_{-\infty}^{\infty} f(x)\, e^{-2\pi i x \xi}\, dx"),
    ("long-bayes", r"P(A \mid B) = \frac{P(B \mid A)\,P(A)}{P(B)}"),
    ("long-stokes", r"\oint_{\partial \Sigma} \mathbf{F}\cdot d\mathbf{r} = \iint_\Sigma (\nabla\times\mathbf{F})\cdot d\mathbf{S}"),
    ("long-schrodinger", r"i\hbar\frac{\partial}{\partial t}\Psi(\mathbf{r},t) = \left[-\frac{\hbar^2}{2m}\nabla^2 + V(\mathbf{r},t)\right]\Psi(\mathbf{r},t)"),
    ("long-quadratic", r"x = \frac{-b \pm \sqrt{b^2-4ac}}{2a}"),
    ("long-determinant", r"\det(A) = \sum_{\sigma \in S_n} \operatorname{sgn}(\sigma) \prod_{i=1}^{n} a_{i,\sigma_i}"),
    ("long-limit", r"\lim_{n \to \infty} \left(1 + \frac{1}{n}\right)^n = e"),
    ("long-cauchy", r"\left( \sum_{k=1}^n a_k b_k \right)^2 \leq \left( \sum_{k=1}^n a_k^2 \right) \left( \sum_{k=1}^n b_k^2 \right)"),
]


def write_expression(directory, name, latex):
    if latex in CRASHES:
        return
    for style in ("inline", "display"):
        path = os.path.join(OUT, directory, f"{name}-{style}.tex")
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8", newline="") as handle:
            handle.write(style + "\n" + latex)


def latex_blocks(markdown):
    """```LaTeX fenced blocks (any capitalisation), verbatim."""
    return re.findall(r"^```latex[ \t]*\n(.*?)\n```", markdown, re.S | re.M | re.I)


STRING = re.compile(r'(?<![#\w])"((?:[^"\\\n]|\\.)*)"')


def string_literals(source):
    """Single-line Swift string literals, unescaped; interpolated ones are skipped."""
    for match in STRING.finditer(source):
        value = unescape_swift(match.group(1))
        if value is not None:
            yield value


def examples():
    seen = set()
    number = 0
    for name in ["EXAMPLES.md", "README.md"]:
        text = open(os.path.join(SWIFTMATH, name), encoding="utf-8").read()
        found = latex_blocks(text)
        for block in re.findall(r"^```swift[ \t]*\n(.*?)\n```", text, re.S | re.M):
            found.extend(string_literals(block))
        for latex in found:
            if latex in seen:
                continue
            seen.add(latex)
            number += 1
            write_expression("examples", f"{name[:-3].lower()}-{number:03d}", latex)


def tests():
    seen = set()
    sources = []
    tests_dir = os.path.join(SWIFTMATH, "Tests", "SwiftMathTests")
    for name in sorted(os.listdir(tests_dir)):
        if name.endswith(".swift"):
            sources.append((name[:-6], os.path.join(tests_dir, name)))
    for name in ["GeometryProbeTests.swift"]:
        sources.append(("Downright" + name[:-6], os.path.join(DOWNRIGHT_TESTS, "MarkdownRenderTests", name)))
    for stem, path in sources:
        text = open(path, encoding="utf-8").read()
        literals = list(string_literals(text))
        # Raw strings: #"…"#
        literals.extend(re.findall(r'#"(.*?)"#', text))
        number = 0
        for latex in literals:
            if latex in seen:
                continue
            seen.add(latex)
            number += 1
            write_expression("tests", f"{stem}-{number:03d}", latex)


def main():
    if not os.path.isdir(SWIFTMATH):
        sys.exit("vendor/downright is missing; run `git submodule update --init`")
    shutil.rmtree(OUT, ignore_errors=True)
    examples()
    tests()
    for name, latex in HARD:
        write_expression("hard", name, latex)
    count = sum(len(files) for _, _, files in os.walk(OUT))
    print(f"corpus/math: {count} expressions")


if __name__ == "__main__":
    main()
