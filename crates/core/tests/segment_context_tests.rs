//! `MarkdownParser::parse_segment` (Upleft extension): a part of a longer
//! document parses as the same text does inside the whole, given what the
//! rest of the document contributes.

use upleft_core::parser::MarkdownParser;
use upleft_core::safe_html::SafeHTMLParser;
use upleft_core::{InlineKind, InlineSpan, SegmentContext};

fn kinds(spans: &[InlineSpan], out: &mut Vec<String>) {
    for span in spans {
        out.push(format!("{:?} {}+{}", span.kind, span.range.location, span.range.length));
        kinds(&span.children, out);
    }
}

fn inline_kinds(document: &upleft_core::ParsedDocument) -> Vec<String> {
    let mut out = Vec::new();
    document.root.walk(&mut |block| kinds(&block.inlines, &mut out));
    out
}

#[test]
fn an_empty_context_parses_as_parse_does() {
    let text = "# Title\n\nSee [docs] and the note[^n].\n\n[docs]: https://example.com\n\n[^n]: A note.\n";
    let plain = MarkdownParser::parse(text);
    let part = MarkdownParser::parse_segment(text, &SegmentContext::default());
    assert_eq!(inline_kinds(&plain), inline_kinds(&part));
    assert_eq!(plain.link_references.len(), part.link_references.len());
    assert_eq!(plain.footnotes.len(), part.footnotes.len());
}

#[test]
fn references_resolve_against_definitions_in_the_rest() {
    let whole_text = "See [docs] and the note[^n].\n\nMore.\n\n[docs]: https://example.com \"Docs\"\n\n[^n]: A note, defined later.\n";
    let whole = MarkdownParser::parse(whole_text);
    let part_text = "See [docs] and the note[^n].\n\n";
    let alone = MarkdownParser::parse(part_text);
    assert!(!inline_kinds(&alone).iter().any(|kind| kind.starts_with("Link") || kind.starts_with("Footnote")));
    let context = SegmentContext {
        references: vec!["[docs]: https://example.com \"Docs\"".into(), "[^n]: A note, defined later.".into()],
        footnotes: vec![("n".into(), "A note, defined later.".into())],
        ..Default::default()
    };
    let part = MarkdownParser::parse_segment(part_text, &context);
    let whole_kinds = inline_kinds(&whole);
    let part_kinds = inline_kinds(&part);
    assert_eq!(whole_kinds[..part_kinds.len()], part_kinds[..]);
    assert!(part_kinds.iter().any(|kind| kind.starts_with("Link")));
    assert!(part_kinds.iter().any(|kind| kind.starts_with("FootnoteReference")));
    // Nothing of the context is in the part: no blocks, no definitions.
    assert_eq!(part.root.children.len(), 1);
    assert!(part.link_references.is_empty() && part.footnotes.is_empty());
    assert_eq!(part.segment_context, context);
}

#[test]
fn details_tags_pair_across_parts() {
    let head = "<details>\n<summary>More</summary>\n\nBody.\n\n";
    let tail = "</details>\n\nAfter.\n";
    let alone = MarkdownParser::parse(head);
    assert!(!alone.root.children[0].safe_html.as_ref().unwrap().is_safe);
    let whole = MarkdownParser::parse(&format!("{head}{tail}"));
    let head_part = MarkdownParser::parse_segment(head, &SegmentContext { details_closed_after: true, ..Default::default() });
    let tail_part = MarkdownParser::parse_segment(tail, &SegmentContext { details_opened_before: true, ..Default::default() });
    assert_eq!(whole.root.children[0].safe_html, head_part.root.children[0].safe_html);
    let closing = whole.root.children.iter().find(|block| block.range.location == head.encode_utf16().count() as isize).unwrap();
    let shifted = tail_part.root.children[0].safe_html.as_ref().unwrap();
    assert!(closing.safe_html.as_ref().unwrap().is_safe && shifted.is_safe);
    let units = |text: &str| text.encode_utf16().collect::<Vec<u16>>();
    assert_eq!(SafeHTMLParser::details_tags(&units(head)), (true, false));
    assert_eq!(SafeHTMLParser::details_tags(&units(tail)), (false, true));
    assert_eq!(SafeHTMLParser::details_tags(&units("```\n<details>\n```\n")), (true, false));
    assert_eq!(SafeHTMLParser::details_tags(&units("> <details>\n")), (false, false));
}

#[test]
fn a_definition_the_rest_repeats_is_not_the_one_kept() {
    let part_text = "Cited[^n] and [x].\n\n[^n]: First.\n\n[x]: /first\n\n";
    let alone = MarkdownParser::parse(part_text);
    assert!(alone.footnotes.contains_key("n") && alone.link_references.contains_key("x"));
    let context = SegmentContext {
        superseded_footnotes: vec!["n".into()],
        superseded_references: vec!["x".into()],
        footnotes: vec![("n".into(), "Second.".into())],
        ..Default::default()
    };
    let part = MarkdownParser::parse_segment(part_text, &context);
    assert!(part.footnotes.is_empty() && part.link_references.is_empty());
    // The part's own definitions still resolve its citations.
    assert_eq!(inline_kinds(&alone), inline_kinds(&part));
}
