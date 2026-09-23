//! ZoomMetricsTests.swift: `StructuralZoomTests` and `MetricsTests`.

mod common;

use upleft_core::metrics::Metrics;
use upleft_core::model::InlineKind;
use upleft_core::parser::MarkdownParser;
use upleft_core::structural_zoom::StructuralZoom;
use upleft_core::swift_text::{self, ns::NSStringExt};
use upleft_core::{NSRange, ZoomLevel, ZoomPlan};

// MARK: - StructuralZoomTests

const DOCUMENT: &str = "# Title

Opening sentence. Second sentence of the intro.

## Alpha

Alpha's first sentence. Alpha's second sentence goes on for a while.

```swift
let x = 1
```

### Alpha detail

Detail prose here.

| a | b |
| --- | --- |
| 1 | 2 |

## Beta

Beta prose. More beta prose.

- [ ] a task
- [x] another task

Closing prose sentence. Another closing sentence.

* plain bullet
* another plain bullet";

fn visible_text(plan: &ZoomPlan, text: &str) -> String {
    let ns = swift_text::ns::utf16(text);
    plan.visible_ranges.iter().map(|&range| ns.as_slice().substring(range)).collect()
}

fn assert_well_formed(plan: &ZoomPlan, length: isize) {
    for pair in plan.visible_ranges.windows(2) {
        assert!(pair[0].upper_bound() <= pair[1].location, "visible ranges must be ascending and disjoint");
    }
    for range in plan.visible_ranges.iter().chain(&plan.elided_ranges) {
        assert!(range.location >= 0);
        assert!(range.upper_bound() <= length);
        assert!(range.length > 0);
    }
    // Together they must tile the document exactly once.
    let total: isize = plan.visible_ranges.iter().chain(&plan.elided_ranges).map(|r| r.length).sum();
    assert_eq!(total, length);
}

#[test]
fn level_five_is_the_identity() {
    let doc = MarkdownParser::parse(DOCUMENT);
    let plan = StructuralZoom::plan(&doc, ZoomLevel::Everything);
    assert!(plan.is_identity());
}

#[test]
fn level_one_keeps_only_h1() {
    let doc = MarkdownParser::parse(DOCUMENT);
    let plan = StructuralZoom::plan(&doc, ZoomLevel::H1);
    assert_well_formed(&plan, doc.length);
    let visible = visible_text(&plan, DOCUMENT);
    assert!(swift_text::contains(&visible, "# Title"));
    assert!(!swift_text::contains(&visible, "## Alpha"));
    assert!(!swift_text::contains(&visible, "Opening sentence"));
}

#[test]
fn level_two_adds_h2() {
    let doc = MarkdownParser::parse(DOCUMENT);
    let plan = StructuralZoom::plan(&doc, ZoomLevel::H2);
    assert_well_formed(&plan, doc.length);
    let visible = visible_text(&plan, DOCUMENT);
    assert!(swift_text::contains(&visible, "## Alpha"));
    assert!(swift_text::contains(&visible, "## Beta"));
    assert!(!swift_text::contains(&visible, "### Alpha detail"));
}

#[test]
fn level_three_keeps_all_headings() {
    let doc = MarkdownParser::parse(DOCUMENT);
    let plan = StructuralZoom::plan(&doc, ZoomLevel::Headings);
    assert_well_formed(&plan, doc.length);
    let visible = visible_text(&plan, DOCUMENT);
    for heading in &doc.headings {
        assert!(swift_text::contains(&visible, &heading.title), "missing {}", heading.title);
    }
    assert!(!swift_text::contains(&visible, "Detail prose"));
}

/// §5.2: "every claim's headline plus all the concrete artifacts, and none of
/// the connective padding."
#[test]
fn skeleton_keeps_headings_first_sentences_and_artifacts() {
    let doc = MarkdownParser::parse(DOCUMENT);
    let plan = StructuralZoom::plan(&doc, ZoomLevel::Skeleton);
    assert_well_formed(&plan, doc.length);
    let visible = visible_text(&plan, DOCUMENT);

    for heading in &doc.headings {
        assert!(swift_text::contains(&visible, &heading.title));
    }
    assert!(swift_text::contains(&visible, "let x = 1"));
    assert!(swift_text::contains(&visible, "| 1 | 2 |"));
    assert!(swift_text::contains(&visible, "- [ ] a task"));
    assert!(swift_text::contains(&visible, "Alpha's first sentence."));
    // The connective padding goes: a plain bullet list is not an artifact, and
    // only a section's *first* sentence survives.
    assert!(!swift_text::contains(&visible, "plain bullet"));
    assert!(!swift_text::contains(&visible, "Closing prose sentence"));
}

#[test]
fn skeleton_keeps_the_lede_of_a_document_without_headings() {
    let text = "First sentence here. Second sentence. Third one too.\n\nAnother paragraph entirely.\n";
    let doc = MarkdownParser::parse(text);
    let plan = StructuralZoom::plan(&doc, ZoomLevel::Skeleton);
    assert_well_formed(&plan, doc.length);
    assert!(swift_text::contains(&visible_text(&plan, text), "First sentence here."));
}

#[test]
fn section_preview_provides_two_clean_sentences() {
    let doc = MarkdownParser::parse(DOCUMENT);
    let alpha = doc.headings.iter().position(|h| swift_text::str_eq(&h.title, "Alpha"));
    assert!(alpha.is_some());
    assert_eq!(
        alpha.and_then(|index| StructuralZoom::section_preview(&doc, index as isize)).as_deref(),
        Some("Alpha's first sentence. Alpha's second sentence goes on for a while.")
    );

    let math = MarkdownParser::parse("## Math\n\nInline $e^{i\\pi} + 1 = 0$ works. More context.\n");
    assert_eq!(StructuralZoom::section_preview(&math, 0).as_deref(), Some("Inline a formula works. More context."));

    let code = MarkdownParser::parse("## Code\n\n```swift\nlet x = 1\n```\n\n```python\nprint(1)\n```\n");
    assert_eq!(StructuralZoom::section_preview(&code, 0).as_deref(), Some("2 code blocks \u{B7} Swift, Python"));
}

#[test]
fn front_matter_survives_every_level() {
    let text = "---\ntitle: X\n---\n\n# H\n\nbody\n";
    let doc = MarkdownParser::parse(text);
    for level in [ZoomLevel::H1, ZoomLevel::H2, ZoomLevel::Headings, ZoomLevel::Skeleton] {
        let plan = StructuralZoom::plan(&doc, level);
        assert_well_formed(&plan, doc.length);
        assert!(swift_text::contains(&visible_text(&plan, text), "title: X"), "level {}", level.raw_value());
    }
}

#[test]
fn every_corpus_document_plans_cleanly_at_every_level() {
    for (_, text) in common::corpus::ALL {
        let doc = MarkdownParser::parse(text);
        for level in ZoomLevel::ALL_CASES {
            let plan = StructuralZoom::plan(&doc, level);
            if level == ZoomLevel::Everything {
                continue;
            }
            assert_well_formed(&plan, doc.length);
        }
    }
}

// MARK: - MetricsTests

#[test]
fn counts_words_excluding_markers_code_and_front_matter() {
    let text = "---\ntitle: Ignore these words entirely\n---\n\n# One Two\n\nThree **four** five `six` seven.\n\n```swift\nthis code should not be counted at all here\n```";
    let metrics = Metrics::metrics_for(text);
    // "One Two" + "Three four five six seven" = 7 words.
    assert_eq!(metrics.words, 7);
    assert_eq!(metrics.read_minutes, 7.0 / 238.0);
}

#[test]
fn read_time_uses238_words_per_minute() {
    let prose = (0..238).map(|i| format!("word{i}")).collect::<Vec<_>>().join(" ") + "\n";
    let metrics = Metrics::metrics_for(&prose);
    assert_eq!(metrics.words, 238);
    assert!((metrics.read_minutes - 1.0).abs() < 0.0001);
}

/// §9.6 regression: soft and hard line breaks inside a paragraph must count
/// as word separators.
#[test]
fn hard_wrapped_paragraphs_count_every_word() {
    let disposed = Metrics::metrics_for("alpha beta\ngamma delta\n");
    assert_eq!(disposed.words, 4);
    // A two-space hard break is still a single word gap.
    let spaces = Metrics::metrics_for("one two  \nthree four\n");
    assert_eq!(spaces.words, 4);
    let doc = MarkdownParser::parse("one two  \nthree four\n");
    let paragraph = doc.root.children.first();
    assert_eq!(paragraph.map(|p| p.inlines.iter().any(|s| matches!(s.kind, InlineKind::LineBreak))), Some(true));
}

#[test]
fn hard_wrap_breaks_become_break_spans() {
    let source = "alpha beta\ngamma delta\n";
    let doc = MarkdownParser::parse(source);
    let paragraph = doc.root.children.first().expect("a paragraph");
    assert!(paragraph.inlines.iter().any(|s| matches!(s.kind, InlineKind::SoftBreak)));
    assert!(paragraph.inlines.iter().any(|s| matches!(s.kind, InlineKind::Text)));
}

#[test]
fn explicit_line_breaks_are_classified_line_break_spans() {
    let doc = MarkdownParser::parse("one  \ntwo\n");
    let paragraph = doc.root.children.first().expect("a paragraph");
    assert!(paragraph.inlines.iter().any(|s| matches!(s.kind, InlineKind::LineBreak)));
}

/// §9.6 regression: the continuation Text of a backslash hard break must be
/// re-anchored to the physical next line.
#[test]
fn backslash_hard_breaks_count_every_word() {
    let disposed = Metrics::metrics_for("one two\\\nthree four\n");
    assert_eq!(disposed.words, 4, "word count was {}", disposed.words);
    let doc = MarkdownParser::parse("one two\\\nthree four\n");
    let paragraph = doc.root.children.first().expect("a paragraph");
    assert!(paragraph.inlines.iter().any(|s| matches!(s.kind, InlineKind::LineBreak)));
    let source = doc.utf16.as_slice();
    let three = paragraph.inlines.iter().find(|s| matches!(s.kind, InlineKind::Text) && source.substring(s.range) == "three four");
    assert!(three.is_some());
}

#[test]
fn empty_text_is_zero() {
    assert_eq!(Metrics::metrics_for("").words, 0);
    assert_eq!(Metrics::metrics_for("").read_minutes, 0.0);
}

/// §9.6: a section must not count its subsections.
#[test]
fn section_metrics_are_parallel_and_exclude_subsections() {
    let text = "# Top\n\nOne two three.\n\n## Sub\n\nFour five six seven eight.\n\n# Second\n\nNine.";
    let doc = MarkdownParser::parse(text);
    let sections = Metrics::section_metrics(&doc);
    assert_eq!(sections.len(), doc.headings.len());
    assert_eq!(sections[0].words, 3);
    assert_eq!(sections[1].words, 5);
    assert_eq!(sections[2].words, 1);
}

#[test]
fn heading_word_counts_match_section_metrics() {
    let doc = MarkdownParser::parse(common::corpus::KITCHEN_SINK);
    let sections = Metrics::section_metrics(&doc);
    assert_eq!(doc.headings.iter().map(|h| h.word_count).collect::<Vec<_>>(), sections.iter().map(|s| s.words).collect::<Vec<_>>());
}

#[test]
fn first_sentence_uses_nl_tokenizer_not_naive_periods() {
    let text = "# H\n\nDr. Smith went to Washington. Then he left.\n";
    let doc = MarkdownParser::parse(text);
    let body = NSRange::new(doc.headings[0].range.upper_bound(), doc.length - doc.headings[0].range.upper_bound());
    let range = Metrics::first_sentence_range(&doc, body);
    assert!(range.is_some());
    assert_eq!(swift_text::trim_whitespaces(&doc.substring(range.unwrap())), "Dr. Smith went to Washington.");
}

#[test]
fn first_sentence_skips_non_prose_blocks() {
    let text = "# H\n\n```swift\nlet x = 1\n```\n\nActual prose here. More.\n";
    let doc = MarkdownParser::parse(text);
    let body = NSRange::new(doc.headings[0].range.upper_bound(), doc.length - doc.headings[0].range.upper_bound());
    let range = Metrics::first_sentence_range(&doc, body);
    assert_eq!(swift_text::trim_whitespaces(&doc.substring(range.unwrap_or(NSRange::new(0, 0)))), "Actual prose here.");
}

#[test]
fn metrics_run_on_every_corpus_document() {
    for (_, text) in common::corpus::ALL {
        let metrics = Metrics::metrics_for(text);
        assert!(metrics.words >= 0);
        assert!(metrics.characters >= metrics.words);
    }
}

// MARK: - Differential cases (not in Swift)
//
// Hand-built documents (no parser) run through Downright's own
// StructuralZoom.swift and Metrics.swift compiled without the parser; the
// expectations are its output, recorded 2026-09-22 with Swift 6.4 (which
// also exercises `SentenceTokenizer` against `NLTokenizer`).

mod differential {
    use std::collections::HashMap;

    use upleft_core::metrics::Metrics;
    use upleft_core::source_positions::SourceMap;
    use upleft_core::structural_zoom::StructuralZoom;
    use upleft_core::swift_text::ns::{NSStringExt, utf16};
    use upleft_core::{
        BlockContent, Checkbox, FrontMatter, HeadingNode, InlineKind, InlineSpan, ListMarkerStyle, MDBlock, NSRange, ParsedDocument,
        PathToken, TableData, ZoomLevel,
    };

    fn r(location: isize, length: isize) -> NSRange {
        NSRange::new(location, length)
    }

    fn find(text: &str, needle: &str, from: isize) -> NSRange {
        let units = utf16(text);
        units.as_slice().range_of_literal(&utf16(needle), r(from, units.len() as isize - from))
    }

    fn line(text: &str, index: isize) -> NSRange {
        SourceMap::new(text).content_range_of_line(index)
    }

    fn lines(text: &str, first: isize, last: isize) -> NSRange {
        let map = SourceMap::new(text);
        let (a, b) = (map.content_range_of_line(first), map.content_range_of_line(last));
        r(a.location, b.upper_bound() - a.location)
    }

    fn block(content: BlockContent, range: NSRange) -> MDBlock {
        MDBlock::new(content, range, range)
    }

    fn span(kind: InlineKind, range: NSRange) -> InlineSpan {
        InlineSpan::new(kind, range, range)
    }

    fn make_doc(text: &str, children: Vec<MDBlock>, front_matter: Option<FrontMatter>, headings: Vec<HeadingNode>) -> ParsedDocument {
        let map = SourceMap::new(text);
        let root = MDBlock::new(BlockContent::Document, r(0, map.length), r(0, map.length))
            .with_children(children.into_iter().map(MDBlock::into_ref).collect());
        ParsedDocument::new(
            text.to_owned(),
            map.length,
            root.into_ref(),
            front_matter,
            headings,
            vec![],
            vec![],
            HashMap::new(),
            HashMap::new(),
            map.line_starts.clone(),
        )
    }

    const T: &str = "---\nt: 1\n---\n\nLede sentence one. Lede two.\n\n# Title\n\nOpening sentence. Second sentence of the intro.\n\n## Alpha\n\n```swift\nlet x = 1\n```\n\nAlpha's first sentence. Alpha's second.\n\n### Detail\n\n| a | b |\n|---|---|\n\n## Beta\n\n- [ ] a task\n\n* plain bullet\n\n## Code\n\n```swift\na\n```\n\n```SWIFT\nb\n```\n\n```objective-c\nc\n```\n\n```c++\nd\n```\n\n```python\ne\n```\n\n$$\nx\n$$\n\n```mermaid\ng\n```\n\n- [x] done\n\n## Empty\n";

    fn zoom_document() -> ParsedDocument {
        let t = T;
        let front_matter = FrontMatter::new(vec![], lines(t, 0, 2), line(t, 1));
        let para = |i: isize| block(BlockContent::Paragraph, line(t, i)).with_inlines(vec![span(InlineKind::Text, line(t, i))]);
        let mut headings: Vec<HeadingNode> = Vec::new();
        let mut hd = |i: isize, level: isize, title: &str| {
            let range = line(t, i);
            headings.push(HeadingNode::new(level, title, range, find(t, title, range.location), range));
            block(BlockContent::Heading { level }, range)
        };
        let code = |a: isize, b: isize, language: &str| {
            block(BlockContent::CodeBlock { language: Some(language.into()), is_fenced: true, content_range: line(t, a + 1) }, lines(t, a, b))
        };
        let task = |i: isize, checked: bool| {
            let item = block(
                BlockContent::ListItem { ordinal: None, checkbox: Some(Checkbox::new(checked, r(line(t, i).location + 3, 1))) },
                line(t, i),
            );
            block(BlockContent::List { ordered: false, start: 1, tight: true, marker: ListMarkerStyle::Dash }, line(t, i))
                .with_children(vec![item.into_ref()])
        };
        let b1 = hd(6, 1, "Title");
        let b2 = hd(10, 2, "Alpha");
        let b3 = hd(18, 3, "Detail");
        let b4 = hd(23, 2, "Beta");
        let b5 = hd(29, 2, "Code");
        let b6 = hd(62, 2, "Empty");
        let plain = block(BlockContent::List { ordered: false, start: 1, tight: true, marker: ListMarkerStyle::Asterisk }, line(t, 27))
            .with_children(vec![
                block(BlockContent::ListItem { ordinal: None, checkbox: None }, line(t, 27)).with_children(vec![para(27).into_ref()]).into_ref(),
            ]);
        let children = vec![
            block(BlockContent::FrontMatter(front_matter.clone()), front_matter.range),
            para(4),
            b1,
            para(8),
            b2,
            code(12, 14, "swift"),
            para(16),
            b3,
            block(BlockContent::Table(TableData::new(vec![], vec![], line(t, 21))), lines(t, 20, 21)),
            b4,
            task(25, false),
            plain,
            b5,
            code(31, 33, "swift"),
            code(35, 37, "SWIFT"),
            code(39, 41, "objective-c"),
            code(43, 45, "c++"),
            code(47, 49, "python"),
            block(BlockContent::MathBlock { latex_range: line(t, 52) }, lines(t, 51, 53)),
            block(BlockContent::Mermaid { source_range: line(t, 56) }, lines(t, 55, 57)),
            task(59, true),
            b6,
        ];
        make_doc(t, children, Some(front_matter), headings)
    }

    #[test]
    fn differential_plans_at_every_level() {
        let doc = zoom_document();
        let expected: Vec<(isize, Vec<NSRange>, Vec<NSRange>)> = vec![
            (1, vec![r(0, 13), r(44, 8)], vec![r(13, 31), r(52, 339)]),
            (2, vec![r(0, 13), r(44, 8), r(102, 9), r(210, 8), r(249, 8)], vec![r(13, 31), r(52, 50), r(111, 99), r(218, 31), r(257, 134)]),
            (3, vec![r(0, 13), r(44, 8), r(102, 9), r(177, 11), r(210, 8), r(249, 8)], vec![r(13, 31), r(52, 50), r(111, 66), r(188, 22), r(218, 31), r(257, 134)]),
            (4, vec![r(0, 13), r(14, 29), r(44, 8), r(53, 48), r(102, 9), r(112, 23), r(136, 40), r(177, 11), r(189, 20), r(210, 8), r(219, 13), r(233, 15), r(249, 8), r(258, 15), r(274, 15), r(290, 21), r(312, 13), r(326, 16), r(343, 8), r(352, 17), r(370, 11)], vec![r(13, 1), r(43, 1), r(52, 1), r(101, 1), r(111, 1), r(135, 1), r(176, 1), r(188, 1), r(209, 1), r(218, 1), r(232, 1), r(248, 1), r(257, 1), r(273, 1), r(289, 1), r(311, 1), r(325, 1), r(342, 1), r(351, 1), r(369, 1), r(381, 10)]),
            (5, vec![], vec![]),
        ];
        for (raw, visible, elided) in expected {
            let level = ZoomLevel::from_raw_value(raw).unwrap();
            let plan = StructuralZoom::plan(&doc, level);
            assert_eq!((plan.visible_ranges, plan.elided_ranges), (visible, elided), "level {raw}");
        }
    }

    #[test]
    fn differential_section_previews() {
        let doc = zoom_document();
        let expected: Vec<(isize, Option<&str>)> = vec![
            (-1, None),
            (0, Some("Opening sentence. Second sentence of the intro.")),
            (1, Some("Alpha's first sentence. Alpha's second.")),
            (2, Some("1 table")),
            (3, Some("* plain bullet")),
            (4, Some("5 code blocks \u{B7} Swift, Objective-C, C++ \u{B7} 1 math block \u{B7} 1 diagram \u{B7} 1 task list")),
            (5, None),
            (6, None),
        ];
        for (index, preview) in expected {
            assert_eq!(StructuralZoom::section_preview(&doc, index).as_deref(), preview, "heading {index}");
        }
    }

    fn single_section(text: &str, inlines: Vec<InlineSpan>) -> ParsedDocument {
        let paragraph = line(text, 2);
        make_doc(
            text,
            vec![block(BlockContent::Heading { level: 1 }, line(text, 0)), block(BlockContent::Paragraph, paragraph).with_inlines(inlines)],
            None,
            vec![HeadingNode::new(1, "L", line(text, 0), r(2, 1), line(text, 0))],
        )
    }

    #[test]
    fn differential_preview_truncation_and_inline_kinds() {
        let long = format!("# L\n\n{}{}\n", "A fairly long sentence that keeps going and going. ".repeat(3), "word ".repeat(60));
        let doc = single_section(&long, vec![span(InlineKind::Text, line(&long, 2))]);
        assert_eq!(StructuralZoom::section_preview(&doc, 0).as_deref(), Some("A fairly long sentence that keeps going and going. A fairly long sentence that keeps going and going."));

        let huge = format!("# L\n\n{}end.\n", "word ".repeat(80));
        let doc = single_section(&huge, vec![span(InlineKind::Text, line(&huge, 2))]);
        assert_eq!(StructuralZoom::section_preview(&doc, 0).as_deref(), Some("word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word word wo\u{2026}"));

        let mixed = "# M\n\nx\n";
        let mp = line(mixed, 2);
        let doc = single_section(
            mixed,
            vec![
                span(InlineKind::InlineMath { latex_range: mp }, mp),
                span(InlineKind::SoftBreak, mp),
                span(InlineKind::Wikilink { target: "T".into(), label: None }, mp),
                span(InlineKind::Wikilink { target: "T".into(), label: Some("Lbl".into()) }, mp),
                span(InlineKind::LineBreak, mp),
                span(InlineKind::Image { source: "s".into(), alt: "".into() }, mp),
                span(InlineKind::Image { source: "s".into(), alt: "Alt".into() }, mp),
                span(InlineKind::FootnoteReference { identifier: "f".into() }, mp),
                span(InlineKind::InlineHTML, mp),
                span(InlineKind::Autolink { destination: "https://x".into() }, mp),
                InlineSpan::new(InlineKind::InlineCode, mp, r(mp.location, 1)),
                span(InlineKind::Emphasis, mp).with_children(vec![span(InlineKind::Text, mp)]),
                span(InlineKind::PathToken(PathToken::new("p", None, None)), mp),
            ],
        );
        assert_eq!(StructuralZoom::section_preview(&doc, 0).as_deref(), Some("a formula TLbl imageAlthttps://xxxx"));
    }

    #[test]
    fn differential_skeleton_without_headings() {
        let text = "First sentence here. Second.\n\nAnother.\n";
        let doc = make_doc(
            text,
            vec![
                block(BlockContent::Paragraph, line(text, 0)).with_inlines(vec![span(InlineKind::Text, line(text, 0))]),
                block(BlockContent::Paragraph, line(text, 2)),
            ],
            None,
            vec![],
        );
        let plan = StructuralZoom::plan(&doc, ZoomLevel::Skeleton);
        assert_eq!((plan.visible_ranges, plan.elided_ranges), (vec![r(0, 29)], vec![r(29, 10)]));
    }

    /// `Metrics.wordCount` and `Metrics.metrics(of:)` (metrics.rs).
    #[test]
    fn differential_metrics_of_prose() {
        let expected: Vec<(&str, isize, isize, isize, isize, u64)> = vec![
            ("", 0, 0, 0, 0, 0),
            ("one two three", 3, 3, 13, 1, 4578419933784878106),
            ("don't stop\u{2014}now\u{2019}s 42x co-op", 5, 5, 26, 1, 4581712481411611158),
            ("Dr. Smith went to Washington. Then he left.", 8, 8, 43, 2, 4585005029038344209),
            ("e\u{301}t\u{E9} \u{65E5}\u{672C}\u{8A9E}\u{306E}\u{30C6}\u{30AD}\u{30B9}\u{30C8}\u{3002}\u{4E8C}\u{6587}\u{76EE}\u{3002}", 4, 4, 17, 2, 4580501429410973713),
            ("Emoji \u{1F389} in a run!\r\nNext line? Yes.", 7, 7, 33, 3, 4584134585412886046),
            ("\u{661}\u{662} \u{663} x\u{2019}y", 3, 3, 8, 1, 4578419933784878106),
            ("a\u{200B}b c\u{A0}d", 4, 4, 7, 1, 4580501429410973713),
        ];
        for (prose, word_count, words, characters, sentences, read_minutes) in expected {
            assert_eq!(Metrics::word_count(prose), word_count, "{prose:?}");
            let metrics = Metrics::metrics_of(prose);
            assert_eq!(
                (metrics.words, metrics.characters, metrics.sentences, metrics.read_minutes.to_bits()),
                (words, characters, sentences, read_minutes),
                "{prose:?}"
            );
        }
    }
}
