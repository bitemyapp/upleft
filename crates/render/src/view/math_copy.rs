//! Copying formulas as TeX: an Upleft extension with no Swift counterpart,
//! for hosts (docs/EMBEDDING.md, `MarkdownTextView::set_math_copy_as_tex`).
//!
//! A view's text storage is the Markdown source, so every formula's LaTeX is
//! already there at a known source range. Copy maps the selection onto the
//! parsed document when it is written to a pasteboard, and nowhere else:
//! rendering and scrolling do no work for it and nothing is cached.
//!
//! The selection is widened to whole formulas, then cut into prose pieces
//! (copied as they are shown) and formulas. Each formula is written with
//! exactly the LaTeX the renderer typesets (`latex`), in delimiters that
//! parse back through this dialect as the same formula: `$…$` inline and
//! `$$…$$` inline display where they do, else `\(…\)` and `\[…\]` (which
//! the dollar guard rails and prose dollars leave alone), else the source's
//! own; display blocks are `$$…$$` on lines of their own, or a `math`
//! fence where a `$$` paragraph would break. The whole copy is parsed back
//! before it is written. `math_copy_conformance` holds all of this to the
//! dialect.

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
    // The parser can list one paragraph's inlines twice; a formula is
    // written once.
    spans.dedup_by_key(|span| span.range);
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

/// The LaTeX a formula is typeset from, its delimiters left out: exactly
/// what the renderer hands `MathRenderer` (`InlineMathDisplay::latex` for
/// inline math, `math_fragment::block_latex` for display blocks), as
/// `MathRenderer::source` reads it. A blank formula has none (`""`).
pub fn latex(document: &ParsedDocument, span: &MathSpan) -> String {
    match span.style {
        MathStyle::Display => {
            let detail = crate::fragments::math_fragment::block_latex(document, span.latex_range);
            upleft_math::MathRenderer::source(&detail).unwrap_or_default()
        }
        MathStyle::Inline | MathStyle::InlineDisplay => {
            // Inline math is typeset from the whole span, delimiters and all
            // (SwiftMath reads `$` as nothing); the delimiters are the
            // parser's markers around `latex_range`.
            let whole = crate::fragments::inline_math_display::InlineMathDisplay::latex(document, span.range);
            let whole = upleft_math::MathRenderer::source(&whole).unwrap_or_default();
            let opener = document.substring(NSRange::new(span.range.location, span.latex_range.location - span.range.location));
            let closer = document.substring(NSRange::new(
                span.latex_range.upper_bound(),
                span.range.upper_bound() - span.latex_range.upper_bound(),
            ));
            match whole.strip_prefix(opener.as_str()).and_then(|rest| rest.strip_suffix(closer.as_str())) {
                Some(bare) => bare.to_owned(),
                None => whole,
            }
        }
    }
}

/// The ways a formula can be written, in order of preference: KaTeX-style
/// dollars first, then the delimiters that avoid the dialect's guard rails
/// (`\(…\)` and `\[…\]`) and, for display math, a `math` fence.
fn candidates(style: MathStyle, latex: &str) -> Vec<String> {
    match style {
        MathStyle::Inline => vec![format!("${latex}$"), format!("\\({latex}\\)")],
        MathStyle::InlineDisplay => vec![format!("$${latex}$$"), format!("\\[{latex}\\]")],
        MathStyle::Display => {
            let mut longest = 0;
            let mut run = 0;
            for c in latex.chars() {
                run = if c == '`' { run + 1 } else { 0 };
                longest = longest.max(run);
            }
            let fence = "`".repeat((longest + 1).max(3));
            vec![format!("$$\n{latex}\n$$"), format!("{fence}math\n{latex}\n{fence}")]
        }
    }
}

/// How a formula of `style` with `latex` reads back when the dialect finds
/// it as a paragraph of its own: inline display math is then a display
/// block, its LaTeX trimmed as `MathRenderer` reads it.
pub fn as_block(style: MathStyle, latex: &str) -> Option<String> {
    (style == MathStyle::InlineDisplay).then(|| upleft_math::MathRenderer::source(latex).unwrap_or_default())
}

/// Whether two formulas read the same: the same kind and LaTeX, or inline
/// display math and the display block it becomes alone in its paragraph.
pub fn same_formula(a: (MathStyle, &str), b: (MathStyle, &str)) -> bool {
    a == b
        || (a.0 == MathStyle::Display && as_block(b.0, b.1).is_some_and(|block| block == a.1))
        || (b.0 == MathStyle::Display && as_block(a.0, a.1).is_some_and(|block| block == b.1))
}

/// Whether `written`, between the text `before` and `after` it, parses back
/// as the same formula (`same_formula`), the formulas already written in
/// `before` (`earlier`) still parse as they did, and no prose before it
/// became a formula.
pub fn parses_back(
    written: &str,
    style: MathStyle,
    latex: &str,
    before: &str,
    after: &str,
    earlier: &[(NSRange, MathStyle, String)],
) -> bool {
    let text = format!("{before}{written}{after}");
    let found = formulas_in(&text);
    let at = NSRange::new(before.encode_utf16().count() as isize, written.encode_utf16().count() as isize);
    let has = |range: NSRange, style: MathStyle, latex: &str| {
        found.iter().any(|(r, s, l)| *r == range && same_formula((*s, l), (style, latex)))
    };
    // No prose before it turned into a formula either.
    let before_count = found.iter().filter(|(range, ..)| range.upper_bound() <= at.location).count();
    before_count == earlier.len()
        && earlier.iter().all(|(range, style, latex)| has(*range, *style, latex))
        && has(at, style, latex)
}

/// The formulas in `text`: where, which kind, and their `latex`.
pub fn formulas_in(text: &str) -> Vec<(NSRange, MathStyle, String)> {
    let document = upleft_core::parser::MarkdownParser::parse(text);
    math_spans(&document).iter().map(|span| (span.range, span.style, self::latex(&document, span))).collect()
}

/// The paragraph so far before a formula, and the rest of it after, as
/// context for `parses_back`: at most this many characters of each.
const CONTEXT: usize = 256;

fn paragraph_before(text: &str) -> &str {
    let start = text.rfind("\n\n").map_or(0, |at| at + 2);
    let tail = &text[start..];
    match tail.char_indices().rev().nth(CONTEXT) {
        Some((at, c)) => &tail[at + c.len_utf8()..],
        None => tail,
    }
}

fn paragraph_after(text: &str) -> &str {
    let head = &text[..text.find("\n\n").unwrap_or(text.len())];
    match head.char_indices().nth(CONTEXT) {
        Some((at, _)) => &head[..at],
        None => head,
    }
}

/// The TeX a formula copies as, given the text written before it and the
/// text that follows: the first of `candidates` that parses back as the
/// same formula there, so a copy pasted into this dialect is the same math.
/// `latex` is the formula's `latex`. Without `dollars`, inline math keeps
/// to `\(…\)` and `\[…\]`, which prose dollars cannot pair with.
///
/// `original` is the formula as the source writes it, which parses the same
/// wherever the copy reads like the source: the last resort for inline
/// math.
/// `written` lists the formulas already in `before`: where, which kind,
/// and their LaTeX.
pub fn tex(
    span: &MathSpan,
    latex: &str,
    original: &str,
    before: &str,
    after: &str,
    written: &[(NSRange, MathStyle, String)],
    dollars: bool,
) -> String {
    let full_before = before;
    let mut candidates = candidates(span.style, latex);
    // Display math is written on lines of its own, a blank line around it.
    let (before, after) = if span.style == MathStyle::Display { ("", "") } else { (paragraph_before(before), paragraph_after(after)) };
    // A dollar written against a dollar of the prose makes `$$`.
    if span.style != MathStyle::Display && (!dollars || before.ends_with('$') || after.starts_with('$')) {
        candidates.remove(0);
    }
    if span.style != MathStyle::Display && !candidates.iter().any(|candidate| candidate == original) {
        candidates.push(original.to_owned());
    }
    // The formulas written earlier in the paragraph, where they are in it.
    let start = (full_before.len() - before.len()) as isize;
    let start = full_before[..start as usize].encode_utf16().count() as isize;
    let earlier: Vec<(NSRange, MathStyle, String)> = if span.style == MathStyle::Display {
        Vec::new()
    } else {
        written
            .iter()
            .filter(|(range, ..)| range.location >= start)
            .map(|(range, style, latex)| (NSRange::new(range.location - start, range.length), *style, latex.clone()))
            .collect()
    };
    candidates
        .iter()
        .find(|written| parses_back(written, span.style, latex, before, after, &earlier))
        .unwrap_or_else(|| candidates.last().expect("a candidate"))
        .clone()
}

/// How `assemble` writes inline math.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Delimiters {
    /// KaTeX-style dollars where they parse back.
    Dollars,
    /// `\(…\)` and `\[…\]`, which prose dollars cannot pair with.
    Backslashes,
    /// As the source writes it, which parses as the source does.
    Source,
}

/// Prose and formulas in selection order, the prose already read.
enum Part {
    Prose(NSRange, String),
    Formula(MathSpan),
}

/// The pieces for `parts`, the text they make, and each formula's TeX.
fn assemble(parts: &[Part], formula: &impl Fn(&MathSpan) -> (String, String), delimiters: Delimiters) -> (Vec<CopyPiece>, String, Vec<String>) {
    let mut pieces = Vec::new();
    let mut written = Vec::new();
    // Each formula written: where in `out`, which kind, its LaTeX.
    let mut placed: Vec<(NSRange, MathStyle, String)> = Vec::new();
    // The text written so far.
    let mut out = String::new();
    let mut after_display = false;
    for (index, part) in parts.iter().enumerate() {
        match part {
            Part::Prose(range, text) => {
                if after_display && !text.trim().is_empty() {
                    let leading = text.chars().take_while(|c| c.is_whitespace()).filter(|&c| c == '\n').count();
                    let gap = "\n".repeat(2usize.saturating_sub(leading));
                    out.push_str(&gap);
                    if !gap.is_empty() {
                        pieces.push(CopyPiece::Text(gap));
                    }
                }
                if !text.trim().is_empty() {
                    after_display = false;
                }
                out.push_str(text);
                pieces.push(CopyPiece::Prose(*range));
            }
            Part::Formula(span) => {
                let display = span.style == MathStyle::Display;
                if (display || after_display) && !out.trim().is_empty() {
                    let trailing = out.chars().rev().take_while(|c| c.is_whitespace()).filter(|&c| c == '\n').count();
                    let gap = "\n".repeat(2usize.saturating_sub(trailing));
                    out.push_str(&gap);
                    if !gap.is_empty() {
                        pieces.push(CopyPiece::Text(gap));
                    }
                }
                after_display = display;
                // A formula next checks that this one still parses.
                let after = match parts.get(index + 1) {
                    Some(Part::Prose(_, text)) => text.as_str(),
                    _ => "",
                };
                let (latex, original) = formula(span);
                let text = if delimiters == Delimiters::Source && span.style != MathStyle::Display {
                    original
                } else {
                    tex(span, &latex, &original, &out, after, &placed, delimiters == Delimiters::Dollars)
                };
                let at = out.encode_utf16().count() as isize;
                placed.push((NSRange::new(at, text.encode_utf16().count() as isize), span.style, latex));
                out.push_str(&text);
                written.push(text.clone());
                pieces.push(CopyPiece::Text(text));
            }
        }
    }
    (pieces, out, written)
}

/// `assemble` with dollars where the whole text parses back as the same
/// formulas; else with `\(…\)` and `\[…\]` for inline math, or inline
/// math as the source writes it, whichever does.
fn assemble_checked(parts: &[Part], formula: &impl Fn(&MathSpan) -> (String, String)) -> (Vec<CopyPiece>, Vec<String>) {
    let expected: Vec<(MathStyle, String)> = parts
        .iter()
        .filter_map(|part| match part {
            Part::Formula(span) => Some((span.style, formula(span).0)),
            Part::Prose(..) => None,
        })
        .collect();
    let reads_back = |out: &str| {
        let found = formulas_in(out);
        found.len() == expected.len()
            && found.iter().zip(&expected).all(|((_, s, l), (style, latex))| same_formula((*s, l), (*style, latex)))
    };
    let (pieces, out, written) = assemble(parts, formula, Delimiters::Dollars);
    if expected.is_empty() || reads_back(&out) || expected.iter().all(|(style, _)| *style == MathStyle::Display) {
        return (pieces, written);
    }
    for delimiters in [Delimiters::Backslashes, Delimiters::Source] {
        let (other_pieces, other_out, other_written) = assemble(parts, formula, delimiters);
        if reads_back(&other_out) {
            return (other_pieces, other_written);
        }
    }
    (pieces, written)
}

/// The widened selection and its pieces. `latex` gives a formula's LaTeX
/// (`latex`), `source` reads the Markdown source, and `shown` is the text a
/// prose range copies as today (plain text), which decides where display
/// math needs line breaks of its own.
pub fn copy_pieces(
    spans: &[MathSpan],
    selection: NSRange,
    latex: impl Fn(&MathSpan) -> String,
    source: impl Fn(NSRange) -> String,
    shown: impl Fn(NSRange) -> String,
) -> (NSRange, Vec<CopyPiece>) {
    let (widened, touched) = widened_selection(spans, selection);
    let mut parts = Vec::new();
    let mut cursor = widened.location;
    let push_prose = |parts: &mut Vec<Part>, range: NSRange| {
        if range.length > 0 {
            let text = shown(range);
            if !text.is_empty() {
                parts.push(Part::Prose(range, text));
            }
        }
    };
    for span in &touched {
        push_prose(&mut parts, NSRange::new(cursor, span.range.location - cursor));
        parts.push(Part::Formula(*span));
        cursor = span.range.upper_bound();
    }
    push_prose(&mut parts, NSRange::new(cursor, widened.upper_bound() - cursor));
    let (pieces, _) = assemble_checked(&parts, &|span: &MathSpan| (latex(span), source(span.range)));
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
    latex: impl Fn(&MathSpan) -> String,
    render: impl Fn(&str) -> String,
) -> String {
    // The formulas wholly inside `range`, written as a copy of the source
    // would write them.
    let mut parts = Vec::new();
    let mut inside = Vec::new();
    let mut cursor = range.location;
    for span in spans {
        if span.range.location < cursor || span.range.upper_bound() > range.upper_bound() {
            continue;
        }
        if span.range.location > cursor {
            let prose = NSRange::new(cursor, span.range.location - cursor);
            parts.push(Part::Prose(prose, source(prose)));
        }
        parts.push(Part::Formula(*span));
        inside.push(*span);
        cursor = span.range.upper_bound();
    }
    if range.upper_bound() > cursor {
        let prose = NSRange::new(cursor, range.upper_bound() - cursor);
        parts.push(Part::Prose(prose, source(prose)));
    }
    let (_, written) = assemble_checked(&parts, &|span: &MathSpan| (latex(span), source(span.range)));
    let mut markdown = String::new();
    let mut formulas: Vec<(String, String)> = Vec::new();
    let mut cursor = range.location;
    for (span, tex) in inside.iter().zip(written) {
        markdown.push_str(&source(NSRange::new(cursor, span.range.location - cursor)));
        let key = format!("\u{E000}{}\u{E001}", formulas.len());
        markdown.push_str(&key);
        formulas.push((key, escape_html(&tex)));
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

/// The formula at `offset`, if any, and its LaTeX alone (Copy LaTeX): the
/// formula's `latex`, what is typeset. A pointer on the right half of an
/// inline formula's image resolves to the insertion point after it, so a
/// formula ending at `offset` counts too.
pub fn formula_at(document: &ParsedDocument, offset: isize) -> Option<(MathSpan, String)> {
    let spans = math_spans(document);
    let span = spans
        .iter()
        .find(|span| span.range.contains(offset))
        .or_else(|| spans.iter().find(|span| span.range.upper_bound() == offset))
        .copied()?;
    let latex = latex(document, &span);
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
        let (_, pieces) = copy_pieces(&spans, selection, |span| latex(&document, span), source, source);
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
        let html = html_with_tex(
            NSRange::new(0, utf16(text)),
            &spans,
            |range| document.substring(range),
            |span| latex(&document, span),
            crate::clipboard_semantic_html::ClipboardSemanticHTML::render,
        );
        expect_eq(
            html,
            "<p>Let $a&lt;\\beta$ hold.</p>\n<p>$$\n\\int_0^1 x\\,dx\n$$</p>\n<p>$$\na^2\n$$</p>",
        );
    }

    /// The whole of `text`, copied.
    fn copied_all(text: &str) -> String {
        copied(text, NSRange::new(0, utf16(text)))
    }

    // Regressions from the differential check (`math_copy_conformance`),
    // one per mismatch it found.

    #[test]
    fn inline_display_math_keeps_its_padding() {
        // Typeset from `$$ x $$` less its delimiters: ` x `, not `x`.
        let text = "Some $$ x $$ more.\n";
        expect_eq(copied_all(text), "Some $$ x $$ more.\n");
        let document = MarkdownParser::parse(text);
        expect_eq(formula_at(&document, 7).map(|found| found.1), Some(" x ".to_owned()));
    }

    #[test]
    fn delimiters_the_guard_rails_reject_are_kept() {
        // `$ x $` and `$100$` are not math in this dialect.
        expect_eq(copied_all("Some \\( x \\) more.\n"), "Some \\( x \\) more.\n");
        expect_eq(copied_all("Pay \\(100\\) now.\n"), "Pay \\(100\\) now.\n");
        // A digit after the closer.
        expect_eq(copied_all("\\(x\\)5\n"), "\\(x\\)5\n");
        // `$$` inside the formula.
        expect_eq(copied_all("Some \\[x $$ y\\] more.\n"), "Some \\[x $$ y\\] more.\n");
        // KaTeX dollars wherever they do parse back.
        expect_eq(copied_all("Also \\(a+b\\) and \\[c\\] work.\n"), "Also $a+b$ and $$c$$ work.\n");
    }

    #[test]
    fn display_math_that_dollars_would_break_is_a_fence() {
        // A blank line, or a line that starts a block, ends a `$$` paragraph.
        expect_eq(copied_all("```math\na\n\nb\n```\n"), "```math\na\n\nb\n```\n");
        expect_eq(copied_all("```math\n- x\n```\n"), "```math\n- x\n```\n");
        // A blank formula, and backticks inside one.
        expect_eq(copied_all("```math\n\n```\n"), "```math\n\n```\n");
        expect_eq(copied_all("~~~math\n```\n~~~\n"), "````math\n```\n````\n");
        // Quote markers inside a `$$` paragraph are part of what is typeset.
        expect_eq(copied_all("> $$\n> a\n> $$\n"), "> \n\n```math\n> a\n>\n```\n");
    }

    #[test]
    fn display_math_is_kept_apart_from_prose() {
        // A fence can follow a line of prose; `$$` cannot.
        expect_eq(copied_all("Text\n```math\nx\n```\nMore\n"), "Text\n\n$$\nx\n$$\n\nMore\n");
        // Whitespace-only prose after a display block is no paragraph.
        let text = "```math\na\n```\n\n $$x$$\n\\(b\\)";
        let back = crate::view::math_copy_conformance::formulas(&copied_all(text));
        expect_eq(back, crate::view::math_copy_conformance::formulas(text));
    }

    #[test]
    fn prose_dollars_do_not_pair_with_the_copy() {
        // `$a$` is prose here only because `$` follows it.
        expect_eq(copied_all("$a$$*$\n"), "$a$$*$\n");
        expect_eq(copied_all("echo $PATH\\(x^2\\)\n"), "echo $PATH\\(x^2\\)\n");
        let text = "\\[foo\\]~~~[$\\([x]\\)|\\[^é\\]~~~$5$$\n";
        let back = crate::view::math_copy_conformance::formulas(&copied_all(text));
        expect_eq(back, crate::view::math_copy_conformance::formulas(text));
    }

    #[test]
    fn a_formula_the_parser_lists_twice_is_copied_once() {
        let text = "```\n```\n\\\ng`2`$^$";
        let spans = math_spans(&MarkdownParser::parse(text));
        expect_eq(spans.len(), 1);
        expect_eq(copied_all(text).matches("$^$").count(), 1);
    }

    #[test]
    fn lone_inline_display_math_copies_as_a_block() {
        // `\[ \]` alone is a display block; `$$ $$` is no formula at all.
        let text = "Some \\[ \\] more.\n";
        let document = MarkdownParser::parse(text);
        let spans = math_spans(&document);
        let source = |range: NSRange| document.substring(range);
        let (_, pieces) = copy_pieces(&spans, spans[0].range, |span| latex(&document, span), source, source);
        expect_eq(plain_text(&pieces, source), "\\[ \\]");
    }

    fn expect_eq<T: PartialEq<U> + std::fmt::Debug, U: std::fmt::Debug>(actual: T, expected: U) {
        assert!(actual == expected, "\n  actual: {actual:?}\nexpected: {expected:?}");
    }
}
