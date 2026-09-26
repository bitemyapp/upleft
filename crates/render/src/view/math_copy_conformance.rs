//! Differential conformance of copying formulas as TeX (`math_copy`), an
//! Upleft extension with no Swift counterpart.
//!
//! For every formula the renderer typesets, the check holds the copy to the
//! dialect: the LaTeX a copy writes is byte for byte what the renderer hands
//! the math engine, the copy parses back through `MarkdownParser` as the
//! same formulas of the same kinds, the HTML flavour carries the same LaTeX,
//! and Copy LaTeX (`formula_at`) returns it bare.
//!
//! What was typeset is read from the renderer's own code:
//! `InlineMathDisplay::ranges` and `InlineMathDisplay::latex` for inline
//! math, `math_fragment::block_latex` for display blocks, both through
//! `MathRenderer::source`; an inline formula's delimiters are the parser's
//! markers. Nothing here reimplements the dialect.
//!
//! `crates/render/tests/math_copy_conformance.rs` runs it over the corpus,
//! generated containers and random documents; the view tests run it through
//! a hosted view and a private pasteboard.

use std::collections::BTreeMap;

use upleft_core::parser::MarkdownParser;
use upleft_core::{BlockContent, BlockRef, InlineKind, NSRange, ParsedDocument};

use crate::clipboard_semantic_html::ClipboardSemanticHTML;
use crate::fragments::inline_math_display::InlineMathDisplay;
use crate::fragments::math_fragment;
use crate::view::math_copy::{self, MathSpan, MathStyle};

/// A typeset formula: its kind and the LaTeX the math engine is handed,
/// delimiters left out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formula {
    pub style: MathStyle,
    pub latex: String,
}

/// A formula the renderer typesets, where it is.
#[derive(Clone, Debug)]
pub struct Typeset {
    pub location: isize,
    pub formula: Formula,
    /// Inside a list, quote, callout or footnote.
    pub in_container: bool,
}

/// Every formula the renderer typesets in `document`, in source order.
pub fn typeset(document: &ParsedDocument) -> Vec<Typeset> {
    let inline_ranges = InlineMathDisplay::ranges(document);
    let mut found = Vec::new();
    fn visit(document: &ParsedDocument, block: &BlockRef, in_container: bool, inline_ranges: &[NSRange], found: &mut Vec<Typeset>) {
        if let BlockContent::MathBlock { latex_range } = block.content {
            let detail = math_fragment::block_latex(document, latex_range);
            let latex = upleft_math::MathRenderer::source(&detail).unwrap_or_default();
            found.push(Typeset {
                location: block.range.location,
                formula: Formula { style: MathStyle::Display, latex },
                in_container,
            });
        }
        for span in &block.inlines {
            span.walk(&mut |inline| {
                if !matches!(inline.kind, InlineKind::InlineMath { .. }) || !inline_ranges.contains(&inline.range) {
                    return;
                }
                let opener = inline.leading_marker_range.map(|range| document.substring(range)).unwrap_or_default();
                let closer = inline.trailing_marker_range.map(|range| document.substring(range)).unwrap_or_default();
                let whole = upleft_math::MathRenderer::source(&InlineMathDisplay::latex(document, inline.range)).unwrap_or_default();
                let latex = whole
                    .strip_prefix(opener.as_str())
                    .and_then(|rest| rest.strip_suffix(closer.as_str()))
                    .map(str::to_owned)
                    .unwrap_or(whole);
                let style = if opener == "$$" || opener == "\\[" { MathStyle::InlineDisplay } else { MathStyle::Inline };
                found.push(Typeset { location: inline.range.location, formula: Formula { style, latex }, in_container });
            });
        }
        let container = matches!(
            block.content,
            BlockContent::BlockQuote
                | BlockContent::Callout { .. }
                | BlockContent::List { .. }
                | BlockContent::ListItem { .. }
                | BlockContent::FootnoteDefinition { .. }
                | BlockContent::Table(_)
        );
        for child in &block.children {
            visit(document, child, in_container || container, inline_ranges, found);
        }
    }
    visit(document, &document.root, false, &inline_ranges, &mut found);
    found.sort_by_key(|typeset| typeset.location);
    // A formula the parser lists twice is drawn once, in one place.
    found.dedup_by_key(|typeset| typeset.location);
    found
}

/// The formulas `text` typesets once parsed.
pub fn formulas(text: &str) -> Vec<Formula> {
    typeset(&MarkdownParser::parse(text)).into_iter().map(|typeset| typeset.formula).collect()
}

/// The LaTeX of `written`, one formula of `style` as a copy writes it, less
/// its delimiters; `None` when it is not written that way.
pub fn strip(style: MathStyle, written: &str) -> Option<String> {
    let between = |open: &str, close: &str| {
        (written.len() >= open.len() + close.len())
            .then(|| written.strip_prefix(open)?.strip_suffix(close).map(str::to_owned))
            .flatten()
    };
    match style {
        MathStyle::Inline if !written.starts_with("$$") => between("$", "$").or_else(|| between("\\(", "\\)")),
        MathStyle::Inline => None,
        MathStyle::InlineDisplay => between("$$", "$$").or_else(|| between("\\[", "\\]")),
        MathStyle::Display => {
            if let Some(latex) = between("$$\n", "\n$$") {
                return Some(latex);
            }
            let fence: String = written.chars().take_while(|&c| c == '`').collect();
            if fence.len() < 3 {
                return None;
            }
            between(&format!("{fence}math\n"), &format!("\n{fence}"))
        }
    }
}

/// Decoded text of an HTML flavour: tags dropped, entities decoded.
pub fn html_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => text.push(c),
            _ => {}
        }
    }
    decode_entities(&text)
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|&end| end <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            _ if name.starts_with("#x") || name.starts_with("#X") => {
                u32::from_str_radix(&name[2..], 16).ok().and_then(char::from_u32)
            }
            _ if name.starts_with('#') => name[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Counts per property and category, and the first failures of each.
#[derive(Default)]
pub struct Report {
    /// (property, category) → (passed, failed).
    pub counts: BTreeMap<(String, String), (usize, usize)>,
    pub failures: Vec<Failure>,
}

#[derive(Clone, Debug)]
pub struct Failure {
    pub property: String,
    pub category: String,
    pub input: String,
    pub detail: String,
}

/// Failures kept for each property and category.
const KEPT: usize = 3;

impl Report {
    pub fn record(&mut self, property: &str, category: &str, ok: bool, input: &str, detail: impl FnOnce() -> String) {
        let entry = self.counts.entry((property.to_owned(), category.to_owned())).or_default();
        if ok {
            entry.0 += 1;
            return;
        }
        entry.1 += 1;
        if entry.1 <= KEPT {
            self.failures.push(Failure {
                property: property.to_owned(),
                category: category.to_owned(),
                input: input.to_owned(),
                detail: detail(),
            });
        }
    }

    /// Failures, residuals (`residual:…`, known and reported) left out.
    pub fn failed(&self) -> usize {
        self.counts.iter().filter(|((property, _), _)| !property.starts_with("residual:")).map(|(_, (_, failed))| failed).sum()
    }

    pub fn merge(&mut self, other: Report) {
        for (key, (passed, failed)) in other.counts {
            let entry = self.counts.entry(key).or_default();
            entry.0 += passed;
            entry.1 += failed;
        }
        self.failures.extend(other.failures);
    }

    /// Counts per property, then per property and category.
    pub fn summary(&self) -> String {
        let mut by_property: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        for ((property, _), (passed, failed)) in &self.counts {
            let entry = by_property.entry(property).or_default();
            entry.0 += passed;
            entry.1 += failed;
        }
        let mut out = String::new();
        for (property, (passed, failed)) in &by_property {
            out.push_str(&format!("{property:<20} {passed:>8} pass {failed:>6} fail\n"));
        }
        out.push('\n');
        for ((property, category), (passed, failed)) in &self.counts {
            out.push_str(&format!("{property:<20} {category:<24} {passed:>8} pass {failed:>6} fail\n"));
        }
        out
    }
}

/// Copy `selection` the pure way: prose as its source, so the formula
/// mapping alone is under test (the view tests copy through the view).
pub fn copy_source(document: &ParsedDocument, spans: &[MathSpan], selection: NSRange) -> String {
    let source = |range: NSRange| document.substring(range);
    let (_, pieces) = math_copy::copy_pieces(spans, selection, |span| math_copy::latex(document, span), source, source);
    math_copy::plain_text(&pieces, source)
}

fn utf16(text: &str) -> isize {
    text.encode_utf16().count() as isize
}

/// Every property for `text`, filed under `category`.
pub fn check_document(text: &str, category: &str, report: &mut Report) {
    let document = MarkdownParser::parse(text);
    let spans = math_copy::math_spans(&document);
    let typeset = typeset(&document);

    // Coverage: the copy knows exactly the formulas the renderer typesets.
    let covered = spans.len() == typeset.len()
        && spans.iter().zip(&typeset).all(|(span, typeset)| {
            span.range.location == typeset.location && span.style == typeset.formula.style
        });
    let disjoint = spans.windows(2).all(|pair| pair[0].range.upper_bound() <= pair[1].range.location);
    report.record("disjoint", category, disjoint, text, || format!("copy spans {spans:?}"));
    report.record("coverage", category, covered, text, || {
        format!("copy spans {:?}\ntypeset {:?}", spans, typeset.iter().map(|t| (t.location, &t.formula)).collect::<Vec<_>>())
    });
    if !covered {
        return;
    }

    let source = |range: NSRange| document.substring(range);
    let latex = |span: &MathSpan| math_copy::latex(&document, span);
    for (span, typeset) in spans.iter().zip(&typeset) {
        let expected = &typeset.formula;
        let copied = copy_source(&document, &spans, span.range);

        // 1. What the copy writes is what was typeset.
        let bare = strip(span.style, &copied);
        report.record("typeset", category, bare.as_deref() == Some(expected.latex.as_str()), text, || {
            format!("formula {:?}\ncopied {copied:?}\nstripped {bare:?}", expected)
        });

        // 2. It parses back as the same formula. A paragraph that is only
        // `$$…$$` or `\[…\]` is a display block in this dialect, so inline
        // display math copied alone can only come back as one.
        let alone = alone(expected);
        let back = formulas(&copied);
        // The parser takes Markdown inline syntax inside a formula (`*b*`,
        // a code span) for prose in some places and math in others; such a
        // formula alone may not come back, whatever its delimiters.
        let inline_markup = expected.latex.contains(['*', '`']);
        let property = if inline_markup { "residual:inline-markup" } else { "round-trip" };
        report.record(property, category, back.as_slice() == std::slice::from_ref(&alone), text, || {
            format!("formula {:?}\ncopied {copied:?}\nparsed back {back:?}", expected)
        });

        // A selection inside the formula copies it whole.
        if span.range.length > 2 {
            let inner = copy_source(&document, &spans, NSRange::new(span.range.location + 1, span.range.length - 2));
            report.record("partial-selection", category, inner == copied, text, || {
                format!("whole {copied:?}\ninner {inner:?}")
            });
        }

        // 3. The HTML flavour carries the same LaTeX.
        let html = math_copy::html_with_tex(span.range, &spans, source, latex, ClipboardSemanticHTML::render);
        let html_back = formulas(html_text(&html).trim());
        let property = if inline_markup { "residual:inline-markup" } else { "html" };
        report.record(property, category, html_back.as_slice() == std::slice::from_ref(&alone), text, || {
            format!("formula {:?}\nhtml {html:?}\nparsed back {html_back:?}", expected)
        });

        // 4. Copy LaTeX returns it bare, from inside the formula and from
        // its trailing edge.
        let mut offsets = vec![span.range.location, span.range.location + span.range.length / 2];
        if !spans.iter().any(|other| other.range.contains(span.range.upper_bound())) {
            offsets.push(span.range.upper_bound());
        }
        for offset in offsets {
            let at = math_copy::formula_at(&document, offset).map(|(_, latex)| latex);
            report.record("copy-latex", category, at.as_deref() == Some(expected.latex.as_str()), text, || {
                format!("formula {:?}\noffset {offset}\ncopy latex {at:?}", expected)
            });
        }
    }

    if typeset.is_empty() {
        return;
    }
    let whole = NSRange::new(0, utf16(text));
    let expected: Vec<Formula> = typeset.iter().map(|typeset| typeset.formula.clone()).collect();

    // The whole document parses back as the same formulas, in order. With
    // prose copied as its source, container markers stay in the prose, so
    // only documents whose formulas are all outside containers compare here;
    // the view tests copy containers as they are shown.
    if typeset.iter().all(|typeset| !typeset.in_container) {
        let copied = copy_source(&document, &spans, whole);
        let back = formulas(&copied);
        report.record("round-trip-document", category, back == expected, text, || {
            format!("expected {expected:?}\ncopied {copied:?}\nparsed back {back:?}")
        });
    }

    // The whole document's HTML holds each formula's LaTeX, in order. The
    // HTML projection may read a line the parser takes for prose as a
    // fence's info string, so attributes count.
    let html = decode_entities(&math_copy::html_with_tex(whole, &spans, source, latex, ClipboardSemanticHTML::render));
    let mut cursor = 0;
    let mut missing = None;
    for formula in &expected {
        let written = written_forms(formula);
        let next = written.iter().filter_map(|form| html[cursor..].find(form.as_str()).map(|at| (at, form.len()))).min();
        match next {
            Some((at, len)) => cursor += at + len,
            None => {
                missing = Some(formula.clone());
                break;
            }
        }
    }
    // The HTML projection (`ClipboardSemanticHTML`, as Downright's) reads
    // some lines the parser takes for prose as fences, and a formula in one
    // is lost with its line.
    let property = if text.contains("```") || text.contains("~~~") { "residual:html-fences" } else { "html-document" };
    report.record(property, category, missing.is_none(), text, || format!("missing {missing:?}\nhtml {html:?}"));
}

/// `formula` as it reads back when copied alone: inline display math is a
/// display block on its own (its LaTeX trimmed, as `MathRenderer` reads it).
pub fn alone(formula: &Formula) -> Formula {
    match math_copy::as_block(formula.style, &formula.latex) {
        Some(latex) => Formula { style: MathStyle::Display, latex },
        None => formula.clone(),
    }
}

/// Every way a copy may write `formula`.
fn written_forms(formula: &Formula) -> Vec<String> {
    let latex = &formula.latex;
    match formula.style {
        MathStyle::Inline => vec![format!("${latex}$"), format!("\\({latex}\\)")],
        MathStyle::InlineDisplay => vec![format!("$${latex}$$"), format!("\\[{latex}\\]")],
        MathStyle::Display => {
            let mut forms = vec![format!("$$\n{latex}\n$$")];
            for n in 3..8 {
                let fence = "`".repeat(n);
                forms.push(format!("{fence}math\n{latex}\n{fence}"));
            }
            forms
        }
    }
}

/// Whether `text` fails `property`.
pub fn fails(text: &str, property: &str) -> bool {
    let mut report = Report::default();
    check_document(text, "minimize", &mut report);
    report.counts.iter().any(|((failed, _), (_, count))| failed == property && *count > 0)
}

/// A smaller input that still fails `property`: lines, then characters,
/// dropped while it keeps failing.
pub fn minimize(text: &str, property: &str) -> String {
    let mut current = text.to_owned();
    if !fails(&current, property) {
        return current;
    }
    loop {
        let mut shrunk = false;
        let lines: Vec<&str> = current.split_inclusive('\n').collect();
        for skip in 0..lines.len() {
            let candidate: String = lines.iter().enumerate().filter(|(i, _)| *i != skip).map(|(_, line)| *line).collect();
            if fails(&candidate, property) {
                current = candidate;
                shrunk = true;
                break;
            }
        }
        if shrunk {
            continue;
        }
        let chars: Vec<char> = current.chars().collect();
        for skip in 0..chars.len() {
            let candidate: String = chars.iter().enumerate().filter(|(i, _)| *i != skip).map(|(_, c)| *c).collect();
            if fails(&candidate, property) {
                current = candidate;
                shrunk = true;
                break;
            }
        }
        if !shrunk {
            return current;
        }
    }
}
