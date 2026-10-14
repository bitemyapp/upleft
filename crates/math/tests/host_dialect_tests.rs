//! `MathRenderer::host_dialect`: LaTeX that models write and SwiftMath
//! lacks, rewritten for hosts that set `HostTypography::math_dialect`.

use objc2_app_kit::NSColor;
use upleft_math::downright::math_renderer::MathRenderer;

#[test]
fn paired_sized_delimiters_become_left_and_right() {
    assert_eq!(MathRenderer::host_dialect(r"\bigl( x \bigr)"), r"\left( x \right)");
    assert_eq!(
        MathRenderer::host_dialect(r"\Bigl\{ \biggl[ a \biggr] \Bigr\}"),
        r"\left\{ \left[ a \right] \right\}"
    );
}

#[test]
fn unbalanced_or_unpaired_sizes_are_dropped() {
    assert_eq!(MathRenderer::host_dialect(r"\bigr) x \bigl("), r") x (");
    assert_eq!(MathRenderer::host_dialect(r"\bigl( x"), r"( x");
    assert_eq!(MathRenderer::host_dialect(r"a \big| b \Bigm| c"), r"a | b | c");
}

#[test]
fn vertical_bars_become_what_swiftmath_reads() {
    assert_eq!(MathRenderer::host_dialect(r"\lvert x \rvert"), r"| x |");
    assert_eq!(MathRenderer::host_dialect(r"\lVert x\rVert_2"), r"\Vert  x\Vert _2");
    assert_eq!(MathRenderer::host_dialect(r"\bigl\lvert v \bigr\rvert"), r"\left| v \right|");
}

#[test]
fn everything_else_is_left_alone() {
    for latex in [r"\frac{a}{b} \\ \{x\} \| y", r"\left( \sum_{i=1}^{n} i \right)", r"\bigcup_i A_i \bigoplus B"] {
        assert_eq!(MathRenderer::host_dialect(latex), latex);
    }
}

#[test]
fn a_formula_swiftmath_refuses_typesets_after_the_rewrite() {
    let color = NSColor::colorWithSRGBRed_green_blue_alpha(0.1, 0.1, 0.1, 1.0);
    let latex = r"T(n) = \sum_{i=1}^{w} \bigl(\lvert \ell_i \rvert + \lvert v_i \rvert\bigr) \le n \cdot \Bigl(1 + \frac{W}{\bar{v}}\Bigr)";
    assert!(MathRenderer::image(latex, true, 18.0, &color, 0.0).is_none(), "SwiftMath lacks these commands");
    assert!(MathRenderer::image(&MathRenderer::host_dialect(latex), true, 18.0, &color, 0.0).is_some());
}
