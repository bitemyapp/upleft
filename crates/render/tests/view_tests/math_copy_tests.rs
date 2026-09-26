//! Copying formulas as TeX from a hosted view (`set_math_copy_as_tex`), an
//! Upleft extension with no Swift counterpart. Every test writes to a
//! private pasteboard made for it, never the general one.

use std::rc::Rc;

use objc2::rc::Retained;
use objc2::{AllocAnyThread, MainThreadMarker, msg_send};
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAttributedStringAppKitDocumentFormats, NSPasteboard, NSPasteboardTypeHTML,
    NSPasteboardTypeRTF, NSPasteboardTypeString, NSTextStorage,
};
use objc2_foundation::{NSArray, NSAttributedString, NSString};
use upleft_core::{DirtySet, NSRange};
use upleft_render::render_contracts::Theme;
use upleft_render::theme::style_sheet::{HostTypography, StyleSheet};
use upleft_render::view::markdown_smart_paste::downright_markdown_type;
use upleft_render::view::markdown_text_view::MarkdownTextView;

use crate::expect;
use crate::support::{parse, text_storage, utf16_len};

pub const TESTS: &[crate::Test] = &[
    ("math_copy_writes_tex_in_every_flavour", math_copy_writes_tex_in_every_flavour),
    ("math_copy_of_one_formula_is_that_formula", math_copy_of_one_formula_is_that_formula),
    ("math_copy_takes_a_partly_selected_formula_whole", math_copy_takes_a_partly_selected_formula_whole),
    ("math_copy_off_copies_as_before", math_copy_off_copies_as_before),
    ("math_latex_at_offset_is_the_bare_formula", math_latex_at_offset_is_the_bare_formula),
    ("math_copy_conforms_through_the_view", math_copy_conforms_through_the_view),
    ("math_backslash_inline_typesets_with_inline_math_content", math_backslash_inline_typesets_with_inline_math_content),
];

/// `\(…\)` and `\[…\]` inline: Downright hands SwiftMath the whole span,
/// which it rejects, so nothing is typeset. A host with
/// `inline_math_content` typesets what lies between the delimiters; `$`
/// math is typeset the same either way.
fn math_backslash_inline_typesets_with_inline_math_content(_mtm: MainThreadMarker) {
    use upleft_render::fragments::inline_math_display::InlineMathDisplay;
    let text = "Inline \\(x^2 + 1\\), then \\[\\sum_k k\\], then $y_1$.\n";
    let document = parse(text);
    let appearance = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).expect("aqua");
    let sheet = |content: Option<bool>| {
        let host = HostTypography { inline_math_content: content, ..HostTypography::default() };
        StyleSheet::for_host(Theme::fallback(), &appearance, true, host)
    };
    let typeset = |content: Option<bool>| InlineMathDisplay::substitutions(&document, &sheet(content), None).len();
    expect!(InlineMathDisplay::ranges(&document).len() == 3);
    // Downright's behaviour: only `$y_1$`.
    expect!(typeset(None) == 1);
    expect!(typeset(Some(true)) == 3);
    let ranges = InlineMathDisplay::ranges(&document);
    expect!(InlineMathDisplay::typeset_source(&document, ranges[0], true) == "x^2 + 1");
    expect!(InlineMathDisplay::typeset_source(&document, ranges[1], true) == "\\sum_k k");
    expect!(InlineMathDisplay::typeset_source(&document, ranges[2], true) == "$y_1$");
    expect!(InlineMathDisplay::typeset_source(&document, ranges[0], false) == "\\(x^2 + 1\\)");
}

const MESSAGE: &str =
    "Mass–energy: $E = mc^2$ holds.\n\n$$\n\\int_0^1 x\\,dx = \\tfrac12\n$$\n\nAnd a fence:\n\n```math\na^2 + b^2 = c^2\n```\n\nDone.\n";

fn hosted(text: &str, mtm: MainThreadMarker) -> (Retained<MarkdownTextView>, Retained<NSTextStorage>) {
    let appearance = NSAppearance::appearanceNamed(unsafe { NSAppearanceNameAqua }).expect("aqua");
    let sheet = Rc::new(StyleSheet::for_host(Theme::fallback(), &appearance, true, HostTypography::default()));
    let storage = text_storage(text);
    let view = MarkdownTextView::new_hosted(&storage, sheet, 480.0, mtm);
    view.update(parse(text), &DirtySet::wholesale(), false);
    (view, storage)
}

/// A pasteboard of our own, released when dropped.
struct PrivatePasteboard(Retained<NSPasteboard>);

impl PrivatePasteboard {
    fn new() -> PrivatePasteboard {
        PrivatePasteboard(NSPasteboard::pasteboardWithUniqueName())
    }
}

impl Drop for PrivatePasteboard {
    fn drop(&mut self) {
        // SAFETY: `releaseGlobally` takes no arguments and returns nothing.
        let _: () = unsafe { msg_send![&*self.0, releaseGlobally] };
    }
}

struct Flavours {
    plain: String,
    rtf: String,
    html: String,
    markdown: String,
}

fn range_of(text: &str, needle: &str, end: &str) -> NSRange {
    let start = text.find(needle).expect("needle");
    let stop = start + text[start..].find(end).expect("end") + end.len();
    NSRange::new(utf16_len(&text[..start]), utf16_len(&text[start..stop]))
}

/// Selects `range` and writes it the way Copy and a drag do.
fn copy(view: &MarkdownTextView, range: NSRange) -> Flavours {
    view.set_source_selected_ranges(&[range]);
    let pasteboard = PrivatePasteboard::new();
    let types = unsafe { NSArray::from_slice(&[NSPasteboardTypeString, NSPasteboardTypeRTF, NSPasteboardTypeHTML]) };
    let wrote: bool = unsafe { msg_send![view, writeSelectionToPasteboard: &*pasteboard.0, types: &*types] };
    expect!(wrote);
    let string = |kind: &NSString| pasteboard.0.stringForType(kind).map(|s| s.to_string()).unwrap_or_default();
    let rtf = pasteboard
        .0
        .dataForType(unsafe { NSPasteboardTypeRTF })
        .and_then(|data| unsafe {
            NSAttributedString::initWithRTF_documentAttributes(NSAttributedString::alloc(), &data, None)
        })
        .map(|attributed| attributed.string().to_string())
        .unwrap_or_default();
    Flavours {
        plain: string(unsafe { NSPasteboardTypeString }),
        rtf,
        html: string(unsafe { NSPasteboardTypeHTML }),
        markdown: string(&downright_markdown_type()),
    }
}

fn math_copy_writes_tex_in_every_flavour(mtm: MainThreadMarker) {
    let (view, _storage) = hosted(MESSAGE, mtm);
    view.set_math_copy_as_tex(true);
    let copied = copy(&view, NSRange::new(0, utf16_len(MESSAGE)));
    // Plain text: prose as shown, each formula as TeX, in place.
    expect!(copied.plain.contains("Mass–energy: $E = mc^2$ holds."));
    expect!(copied.plain.contains("$$\n\\int_0^1 x\\,dx = \\tfrac12\n$$"));
    expect!(copied.plain.contains("$$\na^2 + b^2 = c^2\n$$"));
    expect!(copied.plain.contains("Done."));
    expect!(!copied.plain.contains('\u{FFFC}'));
    expect!(!copied.plain.contains("```"));
    // RTF carries the same text.
    expect!(copied.rtf == copied.plain);
    // HTML and the private Markdown flavour come from the source: the TeX
    // travels as text.
    expect!(copied.html.contains("<p>Mass–energy: $E = mc^2$ holds.</p>"));
    expect!(copied.html.contains("<p>$$\n\\int_0^1 x\\,dx = \\tfrac12\n$$</p>"));
    expect!(copied.html.contains("<p>$$\na^2 + b^2 = c^2\n$$</p>"));
    expect!(copied.markdown == MESSAGE.trim_end_matches('\n') || copied.markdown == MESSAGE);
}

fn math_copy_of_one_formula_is_that_formula(mtm: MainThreadMarker) {
    let (view, _storage) = hosted(MESSAGE, mtm);
    view.set_math_copy_as_tex(true);
    let inline = copy(&view, range_of(MESSAGE, "$E", "2$"));
    expect!(inline.plain == "$E = mc^2$");
    expect!(inline.rtf == "$E = mc^2$");
    expect!(inline.markdown == "$E = mc^2$");
    expect!(inline.html == "<p>$E = mc^2$</p>");
    let display = copy(&view, range_of(MESSAGE, "$$", "12\n$$"));
    expect!(display.plain == "$$\n\\int_0^1 x\\,dx = \\tfrac12\n$$");
    let fence = copy(&view, range_of(MESSAGE, "```math", "c^2\n```"));
    expect!(fence.plain == "$$\na^2 + b^2 = c^2\n$$");
    expect!(fence.markdown.contains("```math"));
}

fn math_copy_takes_a_partly_selected_formula_whole(mtm: MainThreadMarker) {
    let (view, _storage) = hosted(MESSAGE, mtm);
    view.set_math_copy_as_tex(true);
    // From inside the inline formula to the end of its sentence.
    let copied = copy(&view, range_of(MESSAGE, "mc^2", "holds."));
    expect!(copied.plain == "$E = mc^2$ holds.");
    expect!(copied.markdown == "$E = mc^2$ holds.");
    // From the prose before to inside the display formula.
    let copied = copy(&view, range_of(MESSAGE, "holds.", "\\int"));
    expect!(copied.plain.starts_with("holds."));
    expect!(copied.plain.ends_with("$$\n\\int_0^1 x\\,dx = \\tfrac12\n$$"));
    // Prose alone copies as it always has.
    let prose = range_of(MESSAGE, "And", "fence:");
    let copied = copy(&view, prose);
    expect!(copied.plain == view.exportable_attributed_string(prose).string().to_string());
    expect!(copied.plain == "And a fence:");
}

fn math_copy_off_copies_as_before(mtm: MainThreadMarker) {
    let (view, _storage) = hosted(MESSAGE, mtm);
    expect!(!view.math_copy_as_tex());
    let range = range_of(MESSAGE, "Mass", "holds.");
    let copied = copy(&view, range);
    // The typeset image's attachment character, as Downright copies it.
    expect!(copied.plain == view.exportable_attributed_string(range).string().to_string());
    expect!(!copied.plain.contains("$E"));
}

fn math_latex_at_offset_is_the_bare_formula(mtm: MainThreadMarker) {
    let (view, _storage) = hosted(MESSAGE, mtm);
    let inside = |needle: &str| utf16_len(&MESSAGE[..MESSAGE.find(needle).expect("needle")]);
    expect!(view.math_latex_at_source_offset(inside("mc^2")).as_deref() == Some("E = mc^2"));
    expect!(view.math_latex_at_source_offset(inside("\\tfrac")).as_deref() == Some("\\int_0^1 x\\,dx = \\tfrac12"));
    expect!(view.math_latex_at_source_offset(inside("b^2")).as_deref() == Some("a^2 + b^2 = c^2"));
    expect!(view.math_latex_at_source_offset(inside("Done")).is_none());
}

/// The differential check (`math_copy_conformance`) through a hosted view
/// and a private pasteboard: prose as the view shows it, containers and all.
fn math_copy_conforms_through_the_view(mtm: MainThreadMarker) {
    use upleft_render::view::math_copy::same_formula;
    use upleft_render::view::math_copy_conformance::{Report, alone, formulas, html_text, strip, typeset};

    let latexes = ["E = mc^2", " x ", "\\$5", "\\text{a $ b}", "a \\\\ b", "α + β", "5 + x", "100", "x$y", "a\n+ b"];
    let mut documents = Vec::new();
    for latex in latexes {
        let inline = [format!("${latex}$"), format!("\\({latex}\\)"), format!("$${latex}$$"), format!("\\[{latex}\\]")];
        let blocks = [format!("$$\n{latex}\n$$"), format!("```math\n{latex}\n```"), format!("\\[\n{latex}\n\\]")];
        for form in &inline {
            let line = format!("Some text {form} more.");
            documents.push(("paragraph", format!("Before.\n\n{line}\n\nAfter.\n")));
            documents.push(("list-tight", format!("- a\n- {line}\n- c\n")));
            documents.push(("list-loose", format!("- a\n\n- {line}\n\n- c\n")));
            documents.push(("quote", format!("> {line}\n")));
            documents.push(("quote-in-list", format!("- item\n\n  > {line}\n")));
            documents.push(("footnote", format!("Text[^1].\n\n[^1]: {line}\n")));
            documents.push(("callout", format!("> [!NOTE]\n> {line}\n")));
            documents.push(("heading", format!("# Title {form}\n\nBody.\n")));
            documents.push(("table-cell", format!("| a | b |\n|---|---|\n| {form} | x |\n")));
            documents.push(("neighbours", format!("({form}), **5**{form} and {form}**5**.\n")));
            documents.push(("crlf", format!("Before.\r\n\r\n{line}\r\n")));
        }
        for form in &blocks {
            let quoted = form.lines().map(|line| format!("> {line}")).collect::<Vec<_>>().join("\n");
            let listed = form.lines().map(|line| format!("  {line}")).collect::<Vec<_>>().join("\n");
            documents.push(("paragraph", format!("Before.\n\n{form}\n\nAfter.\n")));
            documents.push(("quote", format!("{quoted}\n")));
            documents.push(("list-loose", format!("- a\n\n{listed}\n\n- c\n")));
            documents.push(("crlf", format!("Before.\r\n\r\n{}\r\n\r\nAfter.\r\n", form.replace('\n', "\r\n"))));
        }
    }
    documents.push(("stress", MESSAGE.to_owned()));

    let mut report = Report::default();
    for (category, text) in &documents {
        let document = parse(text);
        let expected = typeset(&document);
        let (view, _storage) = hosted(text, mtm);
        view.set_math_copy_as_tex(true);
        // The whole document parses back as the same formulas.
        let copied = copy(&view, NSRange::new(0, utf16_len(text)));
        let back = formulas(&copied.plain);
        let same = back.len() == expected.len()
            && back.iter().zip(&expected).all(|(a, b)| same_formula((a.style, &a.latex), (b.formula.style, &b.formula.latex)));
        // Without a formula the copy writes no TeX (Downright's copy). Prose
        // is copied as shown, markers left out, so a `$` of the prose can
        // pair with another once `**` between them is gone.
        let spans = upleft_render::view::math_copy::math_spans(&document);
        let mut prose = String::new();
        let mut cursor = 0;
        for span in &spans {
            prose.push_str(&document.substring(NSRange::new(cursor, span.range.location - cursor)));
            cursor = span.range.upper_bound();
        }
        prose.push_str(&document.substring(NSRange::new(cursor, document.length - cursor)));
        let property = if prose.contains('$') { "residual:prose-dollars" } else { "view-round-trip-document" };
        if !expected.is_empty() {
            report.record(property, category, same, text, || format!("copied {:?}\nparsed back {back:?}", copied.plain));
        }
        // Selections across containers, their ends inside prose and inside
        // formulas: they parse back as the formulas they touch.
        if !expected.is_empty() {
            let length = utf16_len(text);
            let first = spans[0].range;
            let mut selections = vec![NSRange::new(2.min(length - 1), (length - 4).max(1))];
            if first.length > 2 {
                selections.push(NSRange::new(first.location + 1, length - first.location - 2));
            }
            for selection in selections {
                let (_, touched) = upleft_render::view::math_copy::widened_selection(&spans, selection);
                let wanted: Vec<_> = expected
                    .iter()
                    .filter(|typeset| touched.iter().any(|span| span.range.location == typeset.location))
                    .collect();
                let copied = copy(&view, selection);
                let back = formulas(&copied.plain);
                let same = back.len() == wanted.len()
                    && back.iter().zip(&wanted).all(|(a, b)| same_formula((a.style, &a.latex), (b.formula.style, &b.formula.latex)));
                report.record(property.replace("document", "selection").as_str(), category, same, text, || {
                    format!("selection {selection:?}\ncopied {:?}\nparsed back {back:?}", copied.plain)
                });
            }
        }
        // AppKit's RTF writes paragraph breaks as LF.
        let rtf_same = copied.rtf == copied.plain.replace("\r\n", "\n");
        report.record("view-rtf", category, rtf_same, text, || format!("rtf {:?}\nplain {:?}", copied.rtf, copied.plain));
        // Each formula alone.
        for (span, typeset) in spans.iter().zip(&expected) {
            let copied = copy(&view, span.range);
            let bare = strip(span.style, &copied.plain);
            // Read as the renderer reads it: hosted views typeset `\\( x \\)` from
            // its content, whose outer whitespace `MathRenderer::source` trims.
            let read = |latex: &str| upleft_math::MathRenderer::source(latex).unwrap_or_default();
            let same = bare.as_deref().is_some_and(|bare| read(bare) == read(&typeset.formula.latex));
            report.record("view-typeset", category, same, text, || {
                format!("formula {:?}\ncopied {:?}", typeset.formula, copied.plain)
            });
            let alone = alone(&typeset.formula);
            let back = formulas(&copied.plain);
            report.record("view-round-trip", category, back == [alone.clone()], text, || format!("copied {:?}\nparsed back {back:?}", copied.plain));
            // The HTML comes from the lossless Markdown range, which may
            // keep markers around the formula (`**`), so inline display math
            // need not stand alone there.
            let html_back = formulas(html_text(&copied.html).trim());
            let html_same = matches!(html_back.as_slice(), [found] if same_formula((found.style, &found.latex), (alone.style, &alone.latex)));
            report.record("view-html", category, html_same, text, || format!("html {:?}\nparsed back {html_back:?}", copied.html));
            let at = view.math_latex_at_source_offset(span.range.location);
            report.record("view-copy-latex", category, at.as_deref() == Some(typeset.formula.latex.as_str()), text, || format!("{at:?}"));
        }
    }
    println!("{}", report.summary());
    for failure in &report.failures {
        println!("FAIL {} / {}\n{:?}\n{}\n", failure.property, failure.category, failure.input, failure.detail);
    }
    expect!(report.failed() == 0);
}
