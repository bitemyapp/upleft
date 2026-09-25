//! Copying formulas as TeX: an Upleft extension with no Swift counterpart,
//! for hosts (docs/EMBEDDING.md, `MarkdownTextView::set_math_copy_as_tex`).
//!
//! A view's text storage is the Markdown source, so every formula's LaTeX is
//! already there at a known source range. Copy maps the selection onto the
//! parsed document when it is written to a pasteboard, and nowhere else:
//! rendering and scrolling do no work for it and nothing is cached.
//!
//! The selection is widened to whole formulas, then cut into prose pieces
//! (copied as they are shown) and formulas, written as `$…$` inline, and as
//! `$$…$$` with its delimiters on their own lines for display math and
//! `math` fences.

use upleft_core::{BlockContent, InlineKind, NSRange, ParsedDocument};

/// How a formula is written back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MathStyle {
    /// `$…$` or `\(…\)` inside prose: `$…$`.
    Inline,
    /// `$$…$$` or `\[…\]` inside prose: `$$…$$`, in place.
    InlineDisplay,
    /// A display block (`$$…$$`, `\[…\]` or a `math` fence): `$$…$$` on
    /// lines of its own.
    Display,
}

/// A formula in the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MathSpan {
    /// The whole formula, delimiters or fences included.
    pub range: NSRange,
    /// The LaTeX alone.
    pub latex_range: NSRange,
    pub style: MathStyle,
}

/// A piece of a copied selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyPiece {
    /// Source copied as it is shown.
    Prose(NSRange),
    /// Text written as is: a formula's TeX, or a line break that puts
    /// display math on lines of its own.
    Text(String),
}

/// Every formula the view typesets, in source order: display blocks, and the
/// inline math of block inlines (what `InlineMathDisplay` substitutes).
pub fn math_spans(document: &ParsedDocument) -> Vec<MathSpan> {
    let mut spans = Vec::new();
    let mut visit = |block: &upleft_core::BlockRef| {
        if let BlockContent::MathBlock { latex_range } = block.content {
            let mut range = block.range;
            // A trailing line break belongs to the prose after the formula.
            while range.length > 0 {
                let last = document.substring(NSRange::new(range.upper_bound() - 1, 1));
                if last == "\n" || last == "\r" {
                    range.length -= 1;
                } else {
                    break;
                }
            }
            if range.length > 0 {
                spans.push(MathSpan { range, latex_range, style: MathStyle::Display });
            }
        }
        for span in &block.inlines {
            span.walk(&mut |inline| {
                if let InlineKind::InlineMath { latex_range } = inline.kind
                    && inline.range.length > 0
                {
                    let opener = inline.leading_marker_range.map(|marker| document.substring(marker)).unwrap_or_default();
                    let style = if opener == "$$" || opener == "\\[" { MathStyle::InlineDisplay } else { MathStyle::Inline };
                    spans.push(MathSpan { range: inline.range, latex_range, style });
                }
            });
        }
    };
    document.root.walk(&mut visit);
    spans.sort_by_key(|span| span.range.location);
    spans
}

/// The formulas `selection` touches, and the selection widened to hold them
/// whole. An empty selection touches nothing.
pub fn widened_selection(spans: &[MathSpan], selection: NSRange) -> (NSRange, Vec<MathSpan>) {
    if selection.length <= 0 {
        return (selection, Vec::new());
    }
    let touched: Vec<MathSpan> = spans
        .iter()
        .filter(|span| span.range.location < selection.upper_bound() && selection.location < span.range.upper_bound())
        .copied()
        .collect();
    let mut widened = selection;
    if let (Some(first), Some(last)) = (touched.first(), touched.last()) {
        let lo = widened.location.min(first.range.location);
        let hi = widened.upper_bound().max(last.range.upper_bound());
        widened = NSRange::new(lo, hi - lo);
    }
    (widened, touched)
}

/// The TeX a formula copies as.
pub fn tex(span: &MathSpan, latex: &str) -> String {
    match span.style {
        MathStyle::Inline => format!("${latex}$"),
        MathStyle::InlineDisplay => format!("$${}$$", latex.trim()),
        MathStyle::Display => format!("$$\n{}\n$$", latex.trim()),
    }
}

/// The widened selection and its pieces. `source` reads the Markdown source;
/// `shown` is the text a prose range copies as today (plain text), which
/// decides where display math needs line breaks of its own.
pub fn copy_pieces(
    spans: &[MathSpan],
    selection: NSRange,
    source: impl Fn(NSRange) -> String,
    shown: impl Fn(NSRange) -> String,
) -> (NSRange, Vec<CopyPiece>) {
    let (widened, touched) = widened_selection(spans, selection);
    let mut pieces = Vec::new();
    // What the pieces so far end with: nothing yet, a line break, or other.
    let mut ends_with_newline: Option<bool> = None;
    let mut after_display = false;
    let mut cursor = widened.location;
    let prose = |pieces: &mut Vec<CopyPiece>, range: NSRange, ends: &mut Option<bool>, after: &mut bool| {
        if range.length <= 0 {
            return;
        }
        let text = shown(range);
        if text.is_empty() {
            return;
        }
        if *after && !text.starts_with('\n') {
            pieces.push(CopyPiece::Text("\n".to_owned()));
        }
        *ends = Some(text.ends_with('\n'));
        *after = false;
        pieces.push(CopyPiece::Prose(range));
    };
    for span in &touched {
        prose(&mut pieces, NSRange::new(cursor, span.range.location - cursor), &mut ends_with_newline, &mut after_display);
        let text = tex(span, &source(span.latex_range));
        if span.style == MathStyle::Display {
            if ends_with_newline == Some(false) {
                pieces.push(CopyPiece::Text("\n".to_owned()));
            }
            after_display = true;
        } else if after_display {
            pieces.push(CopyPiece::Text("\n".to_owned()));
            after_display = false;
        }
        ends_with_newline = Some(false);
        pieces.push(CopyPiece::Text(text));
        cursor = span.range.upper_bound();
    }
    prose(
        &mut pieces,
        NSRange::new(cursor, widened.upper_bound() - cursor),
        &mut ends_with_newline,
        &mut after_display,
    );
    (widened, pieces)
}

/// The plain text of `pieces`, with `shown` for prose.
pub fn plain_text(pieces: &[CopyPiece], shown: impl Fn(NSRange) -> String) -> String {
    let mut out = String::new();
    for piece in pieces {
        match piece {
            CopyPiece::Prose(range) => out.push_str(&shown(*range)),
            CopyPiece::Text(text) => out.push_str(text),
        }
    }
    out
}

/// HTML for the Markdown source `range`, with every formula in it written
/// as its TeX. The clipboard's Markdown-to-HTML projection reads `\` as an
/// escape, which would eat half of every command, so each formula goes
/// through it as a placeholder and comes back as escaped TeX.
pub fn html_with_tex(
    range: NSRange,
    spans: &[MathSpan],
    source: impl Fn(NSRange) -> String,
    render: impl Fn(&str) -> String,
) -> String {
    let mut markdown = String::new();
    let mut formulas: Vec<(String, String)> = Vec::new();
    let mut cursor = range.location;
    for span in spans {
        if span.range.location < cursor || span.range.upper_bound() > range.upper_bound() {
            continue;
        }
        markdown.push_str(&source(NSRange::new(cursor, span.range.location - cursor)));
        let key = format!("\u{E000}{}\u{E001}", formulas.len());
        markdown.push_str(&key);
        formulas.push((key, escape_html(&tex(span, &source(span.latex_range)))));
        cursor = span.range.upper_bound();
    }
    markdown.push_str(&source(NSRange::new(cursor, range.upper_bound() - cursor)));
    let mut html = render(&markdown);
    for (key, formula) in formulas {
        html = html.replacen(&key, &formula, 1);
    }
    html
}

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// The formula at `offset`, if any, and its LaTeX alone (Copy LaTeX). A
/// pointer on the right half of an inline formula's image resolves to the
/// insertion point after it, so a formula ending at `offset` counts too.
pub fn formula_at(document: &ParsedDocument, offset: isize) -> Option<(MathSpan, String)> {
    let spans = math_spans(document);
    let span = spans
        .iter()
        .find(|span| span.range.contains(offset))
        .or_else(|| spans.iter().find(|span| span.range.upper_bound() == offset))
        .copied()?;
    let latex = document.substring(span.latex_range);
    let latex = if span.style == MathStyle::Inline { latex } else { latex.trim().to_owned() };
    Some((span, latex))
}

#[cfg(test)]
mod tests {
    use super::*;
    use upleft_core::parser::MarkdownParser;

    fn utf16(text: &str) -> isize {
        text.encode_utf16().count() as isize
    }

    /// The selection from the first `needle` to the end of the first `end`
    /// after it.
    fn between(text: &str, needle: &str, end: &str) -> NSRange {
        let start = text.find(needle).expect("needle");
        let stop = start + text[start..].find(end).expect("end") + end.len();
        NSRange::new(utf16(&text[..start]), utf16(&text[start..stop]))
    }

    /// Copy with prose as its raw source, so the mapping alone is tested.
    fn copied(text: &str, selection: NSRange) -> String {
        let document = MarkdownParser::parse(text);
        let spans = math_spans(&document);
        let source = |range: NSRange| document.substring(range);
        let (_, pieces) = copy_pieces(&spans, selection, source, source);
        plain_text(&pieces, source)
    }

    #[test]
    fn inline_math_copies_as_dollars() {
        let text = "Energy is $E = mc^2$ here.\n";
        expect_eq(copied(text, between(text, "Energy", "here.")), "Energy is $E = mc^2$ here.");
        expect_eq(copied(text, between(text, "$E", "2$")), "$E = mc^2$");
        let text = "Also \\(a+b\\) works.\n";
        expect_eq(copied(text, between(text, "Also", "works.")), "Also $a+b$ works.");
    }

    #[test]
    fn display_math_copies_on_lines_of_its_own() {
        let text = "Before.\n\n$$\n\\int_0^1 x\\,dx\n$$\n\nAfter.\n";
        expect_eq(copied(text, between(text, "$$", "dx\n$$")), "$$\n\\int_0^1 x\\,dx\n$$");
        expect_eq(
            copied(text, between(text, "Before", "After.")),
            "Before.\n\n$$\n\\int_0^1 x\\,dx\n$$\n\nAfter.",
        );
        let text = "\\[x^2\\]\n";
        expect_eq(copied(text, NSRange::new(0, utf16(text))), "$$\nx^2\n$$\n");
    }

    #[test]
    fn math_fence_copies_as_display_math() {
        let text = "See:\n\n```math\na^2 + b^2 = c^2\n```\n\nDone.\n";
        expect_eq(copied(text, between(text, "```math", "c^2\n```")), "$$\na^2 + b^2 = c^2\n$$");
        expect_eq(copied(text, between(text, "See", "Done.")), "See:\n\n$$\na^2 + b^2 = c^2\n$$\n\nDone.");
    }

    #[test]
    fn several_formulas_are_each_replaced() {
        let text = "Let $a$ and $b$ be reals.\n\n$$a+b$$\n\nSo $c$.\n";
        expect_eq(copied(text, NSRange::new(0, utf16(text))), "Let $a$ and $b$ be reals.\n\n$$\na+b\n$$\n\nSo $c$.\n");
        let document = MarkdownParser::parse(text);
        expect_eq(math_spans(&document).len(), 4);
    }

    #[test]
    fn a_selection_inside_a_formula_takes_it_whole() {
        let text = "Energy is $E = mc^2$ here.\n";
        // Starts inside the formula.
        expect_eq(copied(text, between(text, "mc", "here")), "$E = mc^2$ here");
        // Ends inside the formula.
        expect_eq(copied(text, between(text, "is", "E =")), "is $E = mc^2$");
        // Wholly inside it.
        expect_eq(copied(text, between(text, "=", "=")), "$E = mc^2$");
        let text = "Intro.\n\n```math\nx = 1\ny = 2\n```\n";
        expect_eq(copied(text, between(text, "Intro", "x =")), "Intro.\n\n$$\nx = 1\ny = 2\n$$");
        let (widened, _) = widened_selection(&math_spans(&MarkdownParser::parse(text)), between(text, "y", "y"));
        expect_eq(widened, between(text, "```math", "2\n```"));
    }

    #[test]
    fn a_selection_without_math_is_unchanged() {
        let text = "Plain *text* with $x$ later.\n";
        let selection = between(text, "Plain", "with");
        expect_eq(copied(text, selection), "Plain *text* with");
        let spans = math_spans(&MarkdownParser::parse(text));
        let (widened, touched) = widened_selection(&spans, selection);
        expect_eq(widened, selection);
        expect_eq(touched.len(), 0);
        // An empty selection copies nothing and touches nothing.
        expect_eq(widened_selection(&spans, NSRange::new(19, 0)).1.len(), 0);
    }

    #[test]
    fn copy_latex_is_the_bare_formula() {
        let text = "Energy is $E = mc^2$.\n\n```math\n a^2 \n```\n";
        let document = MarkdownParser::parse(text);
        expect_eq(formula_at(&document, 12).map(|found| found.1), Some("E = mc^2".to_owned()));
        let fence = utf16(&text[..text.find("a^2").unwrap()]);
        expect_eq(formula_at(&document, fence).map(|found| found.1), Some("a^2".to_owned()));
        expect_eq(formula_at(&document, 2), None);
        // The insertion point just after the inline formula.
        let after = utf16(&text[..text.find("$.").unwrap() + 1]);
        expect_eq(formula_at(&document, after).map(|found| found.1), Some("E = mc^2".to_owned()));
    }

    #[test]
    fn html_keeps_the_tex_whole() {
        let text = "Let $a<\\beta$ hold.\n\n$$\n\\int_0^1 x\\,dx\n$$\n\n```math\na^2\n```\n";
        let document = MarkdownParser::parse(text);
        let spans = math_spans(&document);
        let html = html_with_tex(NSRange::new(0, utf16(text)), &spans, |range| document.substring(range), |markdown| {
            crate::clipboard_semantic_html::ClipboardSemanticHTML::render(markdown)
        });
        expect_eq(
            html,
            "<p>Let $a&lt;\\beta$ hold.</p>\n<p>$$\n\\int_0^1 x\\,dx\n$$</p>\n<p>$$\na^2\n$$</p>",
        );
    }

    fn expect_eq<T: PartialEq<U> + std::fmt::Debug, U: std::fmt::Debug>(actual: T, expected: U) {
        assert!(actual == expected, "\n  actual: {actual:?}\nexpected: {expected:?}");
    }
}
