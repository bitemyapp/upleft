//! DocumentHealthTests.swift, plus differential cases on hand-built documents.

use std::collections::HashMap;

use upleft_core::health::document_health::{
    DocumentHealth, DocumentHealthDiagnostic, DocumentHealthOptions, DocumentHealthResolver,
};
use upleft_core::source_positions::SourceMap;
use upleft_core::swift_text::ns::{NSStringExt, utf16};
use upleft_core::{
    BlockContent, FrontMatter, HeadingNode, InlineKind, InlineSpan, MDBlock, NSRange, ParsedDocument, TableAlignment, TableCell,
    TableData, TableRow,
};

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn reports_stable_ids_and_utf16_ranges() {
    let text = "# Title\n\n#### Café\n";
    let first = DocumentHealth::analyze(text);
    let second = DocumentHealth::analyze(text);
    assert_eq!(first.iter().map(|d| &d.id).collect::<Vec<_>>(), second.iter().map(|d| &d.id).collect::<Vec<_>>());
    let skipped = first.iter().find(|d| d.id == "heading.skipped-level");
    assert_eq!(skipped.map(|d| d.range), Some(NSRange::new(9, 9)));
    assert_eq!(skipped.and_then(|d| d.fix.as_ref()).map(|f| f.replacement.as_str()), Some("## "));
    assert_eq!(skipped.and_then(|d| d.fix.as_ref()).map(|f| f.range.location), Some(9));
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn ignores_code_and_front_matter() {
    let text = "---\ntitle: the the\n---\n\n# Title\n\n```\n# Fake\n[bad](javascript:alert(1))\n```\n";
    let findings = DocumentHealth::analyze(text);
    assert!(!findings.iter().any(|d| d.range.location < 25 && d.id == "prose.repeated-word"));
    assert!(!findings.iter().any(|d| d.id == "url.unsafe-scheme"));
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn finds_references_footnotes_and_images() {
    let text = "[x][missing]\n\n[^a]: one\n[^a]: two\n\n![](missing.png)\n";
    let findings = DocumentHealth::analyze_with(text, DocumentHealthOptions::DEFAULT, Some(&DocumentHealthResolver::new(|_| false)));
    assert!(findings.iter().any(|d| d.id == "reference.undefined"));
    assert!(findings.iter().any(|d| d.id == "footnote.duplicate"));
    assert!(findings.iter().any(|d| d.id == "image.missing-alt"));
    assert!(findings.iter().any(|d| d.id == "asset.missing"));
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn does_not_flag_external_links_as_missing_assets() {
    let findings = DocumentHealth::analyze_with(
        "[site](https://example.com)\n",
        DocumentHealthOptions::DEFAULT,
        Some(&DocumentHealthResolver::new(|_| false)),
    );
    assert!(!findings.iter().any(|d| d.id == "link.missing"));
    assert!(!findings.iter().any(|d| d.id == "url.unsafe-scheme"));
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn catches_unclosed_fence_and_ignores_its_contents() {
    let text = "# Title\n\n```swift\nlet x = 1\n\n![diagram](/tmp/diagram.png)\n";
    let findings = DocumentHealth::analyze(text);
    assert!(findings.iter().any(|d| d.id == "fence.unclosed"));
    assert!(!findings.iter().any(|d| d.id == "asset.absolute-path"));
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn catches_absolute_path_outside_code() {
    let findings = DocumentHealth::analyze("![diagram](/assets/diagram.png)\n");
    assert!(findings.iter().any(|d| d.id == "asset.absolute-path"));
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn catches_unsafe_url_and_duplicate_anchor() {
    let findings = DocumentHealth::analyze("# Same\n\n# Same\n\n[run](javascript:alert(1))\n");
    assert!(findings.iter().any(|d| d.id == "heading.duplicate-anchor"));
    assert!(findings.iter().any(|d| d.id == "url.unsafe-scheme"));
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn resolver_only_checks_local_targets() {
    let findings = DocumentHealth::analyze_with(
        "[missing](docs/missing.md)\n",
        DocumentHealthOptions::DEFAULT,
        Some(&DocumentHealthResolver::new(|_| false)),
    );
    assert!(findings.iter().any(|d| d.id == "link.missing"));
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn reports_dense_paragraph_and_invalid_table_row() {
    let text = format!("| A | B |\n| --- | --- |\n| only |\n\n{}\n", "word ".repeat(130));
    let findings = DocumentHealth::analyze(&text);
    assert!(findings.iter().any(|d| d.id == "table.invalid-row"));
    assert!(findings.iter().any(|d| d.id == "paragraph.dense"));
}

// MARK: - Differential cases (not in Swift)
//
// Hand-built documents (no parser), run through Downright's own
// DocumentHealth.swift compiled without the parser; the expectations below are
// its output, recorded 2026-09-22 with Swift 6.4.

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

fn link(destination: &str, range: NSRange) -> InlineSpan {
    span(InlineKind::Link { destination: destination.into(), title: None }, range)
}

fn image(source: &str, alt: &str, range: NSRange) -> InlineSpan {
    span(InlineKind::Image { source: source.into(), alt: alt.into() }, range)
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
        Vec::new(),
        Vec::new(),
        HashMap::new(),
        HashMap::new(),
        map.line_starts.clone(),
    )
}

fn heading(level: isize, title: &str, range: NSRange, content_range: NSRange, words: isize) -> HeadingNode {
    let mut node = HeadingNode::new(level, title, range, content_range, range);
    node.word_count = words;
    node
}

type Expected = (&'static str, &'static str, &'static str, NSRange, &'static str, &'static str, Option<(NSRange, &'static str, &'static str)>);

fn check(findings: &[DocumentHealthDiagnostic], expected: &[Expected]) {
    let actual: Vec<(String, String, String, NSRange, String, String, Option<(NSRange, String, String)>)> = findings
        .iter()
        .map(|d| {
            (
                d.id.clone(),
                d.severity.raw_value().to_owned(),
                d.category.raw_value().to_owned(),
                d.range,
                d.message.clone(),
                d.explanation.clone(),
                d.fix.as_ref().map(|f| (f.range, f.replacement.clone(), f.summary.clone())),
            )
        })
        .collect();
    let expected: Vec<_> = expected
        .iter()
        .map(|e| {
            (
                e.0.to_owned(),
                e.1.to_owned(),
                e.2.to_owned(),
                e.3,
                e.4.to_owned(),
                e.5.to_owned(),
                e.6.map(|f| (f.0, f.1.to_owned(), f.2.to_owned())),
            )
        })
        .collect();
    assert_eq!(actual, expected);
}

const LINK_MISSING: (&str, &str, &str) = ("link.missing", "Local link target was not found", "Check the relative path or add the referenced file to the document's project.");
const UNUSED: &str = "Remove definitions that have no references to keep the document easy to maintain.";

#[test]
fn differential_front_matter_delimiters() {
    check(
        &DocumentHealth::analyze_document(&make_doc("t---\ntitle: x\n", vec![], None, vec![])),
        &[(
            "frontmatter.malformed-delimiter",
            "warning",
            "structure",
            r(0, 4),
            "Front-matter opener has a stray character before `---`",
            "A valid YAML front-matter block opens with `---` on the very first line. `t---` is not a valid opener, so the metadata below renders as body prose instead of a metadata card.",
            None,
        )],
    );
    check(
        &DocumentHealth::analyze_document(&make_doc("---\ntitle: x\nbody\n", vec![], None, vec![])),
        &[(
            "frontmatter.unclosed",
            "warning",
            "structure",
            r(0, 4),
            "Front-matter block is never closed",
            "A YAML front-matter block needs a closing `---` line; without it the metadata below renders as ordinary prose.",
            None,
        )],
    );
    check(
        &DocumentHealth::analyze_document(&make_doc("\u{301}---\n", vec![], None, vec![])),
        &[(
            "frontmatter.malformed-delimiter",
            "warning",
            "structure",
            r(0, 4),
            "Front-matter opener has a stray character before `---`",
            "A valid YAML front-matter block opens with `---` on the very first line. `\u{301}---` is not a valid opener, so the metadata below renders as body prose instead of a metadata card.",
            None,
        )],
    );
}

#[test]
fn differential_references_and_definitions() {
    let text = "[x][missing] and [Used][] ![i][img] [K][]\n\n[used]: /abs/path\n[unused]: javascript:alert(1)\n[\u{212A}]: http:x\n[f]: ftp://host/x\n[^a]: one\n[^A]: two\n  [^b]: three\n[g]:\t<docs/missing.md#frag>\n";
    let paragraph = block(BlockContent::Paragraph, line(text, 0));
    let doc = make_doc(text, vec![paragraph], None, vec![]);
    let resolver = DocumentHealthResolver::new(|_| false);
    check(
        &DocumentHealth::analyze_document_with(&doc, DocumentHealthOptions::DEFAULT, Some(&resolver)),
        &[
            (
                "reference.undefined",
                "error",
                "references",
                r(0, 12),
                "Reference \u{2018}missing\u{2019} is undefined",
                "Add a matching [missing]: destination definition, or use an inline link.",
                None,
            ),
            (LINK_MISSING.0, "warning", "links", r(43, 17), LINK_MISSING.1, LINK_MISSING.2, None),
            (LINK_MISSING.0, "warning", "links", r(61, 29), LINK_MISSING.1, LINK_MISSING.2, None),
            ("reference.unused", "info", "references", r(61, 29), "Reference \u{2018}unused\u{2019} is unused", UNUSED, None),
            (LINK_MISSING.0, "warning", "links", r(91, 11), LINK_MISSING.1, LINK_MISSING.2, None),
            (LINK_MISSING.0, "warning", "links", r(103, 17), LINK_MISSING.1, LINK_MISSING.2, None),
            ("reference.unused", "info", "references", r(103, 17), "Reference \u{2018}f\u{2019} is unused", UNUSED, None),
            (
                "footnote.duplicate",
                "error",
                "references",
                r(131, 9),
                "Footnote \u{2018}a\u{2019} is defined more than once",
                "Footnote references resolve to one definition; keep a single definition for each identifier.",
                None,
            ),
            (LINK_MISSING.0, "warning", "links", r(155, 27), LINK_MISSING.1, LINK_MISSING.2, None),
            ("reference.unused", "info", "references", r(155, 27), "Reference \u{2018}g\u{2019} is unused", UNUSED, None),
        ],
    );
}

#[test]
fn differential_heading_structure() {
    let text = "intro\n\n# A\n\n#### B\n\n# A\n\n##  \n\n#   Caf\u{E9}-x_y!\n";
    let headings = vec![
        heading(1, "A", line(text, 2), find(text, "A", 0), 0),
        heading(4, "B", line(text, 4), find(text, "B", 0), 501),
        heading(1, "A", line(text, 6), find(text, "A", 20), 0),
        heading(2, "", line(text, 8), r(line(text, 8).upper_bound(), 0), 0),
        heading(1, "Caf\u{E9}-x_y!", line(text, 10), find(text, "Caf", 0), 0),
    ];
    let children = vec![block(BlockContent::Paragraph, line(text, 0)), block(BlockContent::HtmlBlock, r(5, 1))];
    check(
        &DocumentHealth::analyze_document(&make_doc(text, children, None, headings)),
        &[
            (
                "document.content-before-first-heading",
                "info",
                "structure",
                r(0, 5),
                "Content appears before the first heading",
                "Put introductory content under a heading, or keep only a short document preamble.",
                None,
            ),
            (
                "heading.skipped-level",
                "warning",
                "structure",
                r(12, 6),
                "Heading level skips from H1 to H4",
                "Heading levels should increase one level at a time so the outline remains navigable.",
                Some((r(12, 5), "## ", "Lower heading level")),
            ),
            (
                "section.long",
                "info",
                "structure",
                r(12, 6),
                "Section is unusually long",
                "Large sections are harder to scan; consider splitting this section into focused subsections.",
                None,
            ),
            (
                "heading.duplicate-anchor",
                "warning",
                "structure",
                r(20, 3),
                "Heading creates a duplicate anchor",
                "Two headings with the same anchor make links and table-of-contents entries ambiguous.",
                None,
            ),
            (
                "heading.multiple-h1",
                "warning",
                "structure",
                r(20, 3),
                "Document has more than one H1 heading",
                "Use one document title (H1), then use H2 and deeper headings for sections.",
                None,
            ),
            (
                "heading.empty",
                "warning",
                "accessibility",
                r(25, 4),
                "Heading has no text",
                "Empty headings create an unlabeled outline entry and an unusable anchor.",
                None,
            ),
            (
                "heading.multiple-h1",
                "warning",
                "structure",
                r(31, 13),
                "Document has more than one H1 heading",
                "Use one document title (H1), then use H2 and deeper headings for sections.",
                None,
            ),
        ],
    );
}

#[test]
fn differential_fences_tables_and_prose() {
    let text = "---\n```\n---\n```swift\ncode\n~~~\n  ~~~~\n````\n    ```\n\u{1FEF}``\nThe the cat. Word word `x x` end! Last  last\n| a | b |\n|---|---|\n| 1 |\n";
    let front_matter = FrontMatter::new(vec![], lines(text, 0, 2), line(text, 1));
    let paragraph_range = line(text, 10);
    let code = find(text, "`x x`", 0);
    let paragraph = block(BlockContent::Paragraph, paragraph_range).with_inlines(vec![
        span(InlineKind::Text, r(paragraph_range.location, 10)),
        InlineSpan::new(InlineKind::InlineCode, code, r(code.location + 1, 3)),
    ]);
    let cell = || TableCell::new(r(0, 0), r(0, 0), vec![]);
    let table = TableData::new(
        vec![TableRow::new(line(text, 11), vec![cell(), cell()], true), TableRow::new(line(text, 13), vec![cell()], false)],
        vec![TableAlignment::None, TableAlignment::None],
        line(text, 12),
    );
    let children = vec![
        block(BlockContent::FrontMatter(front_matter.clone()), front_matter.range),
        paragraph,
        block(BlockContent::Table(table), lines(text, 11, 13)),
    ];
    let doc = make_doc(text, children, Some(front_matter), vec![]);
    let options = DocumentHealthOptions::new(0, 2, 3);
    assert_eq!(options.max_section_words, 1);
    let repeated = ("prose.repeated-word", "Repeated adjacent word", "Remove the accidental duplicate unless the repetition is intentional.");
    let long = ("sentence.long", "Sentence is unusually long", "Shorter sentences are easier to understand and translate.");
    check(
        &DocumentHealth::analyze_document_with(&doc, options, None),
        &[
            ("fence.unclosed", "error", "syntax", r(50, 3), "Code fence is not closed", "Add a closing fence with the same marker character.", None),
            (repeated.0, "warning", "prose", r(54, 7), repeated.1, repeated.2, None),
            (long.0, "info", "prose", r(54, 12), long.1, long.2, None),
            (
                "paragraph.dense",
                "info",
                "prose",
                r(54, 44),
                "Paragraph is dense (10 words)",
                "Break long paragraphs into smaller units so readers can scan the document.",
                None,
            ),
            (long.0, "info", "prose", r(66, 21), long.1, long.2, None),
            (repeated.0, "warning", "prose", r(67, 9), repeated.1, repeated.2, None),
            (repeated.0, "warning", "prose", r(88, 10), repeated.1, repeated.2, None),
            (
                "table.invalid-row",
                "warning",
                "syntax",
                r(119, 5),
                "Table row has a different number of cells",
                "Keep each table row aligned with the header's column count.",
                None,
            ),
        ],
    );
}

#[test]
fn differential_links_and_images() {
    let text = "See links.\n";
    let paragraph = block(BlockContent::Paragraph, line(text, 0)).with_inlines(vec![
        image("/img.png", " ", r(0, 1)),
        link("javascript:x", r(1, 1)).with_children(vec![image("<missing.png>", "a", r(1, 1))]),
        link("HTTP:x", r(2, 1)),
        span(InlineKind::Autolink { destination: "https://ok".into() }, r(3, 1)),
        link(" docs/a.md#frag ", r(4, 1)),
        link("//cdn/x", r(5, 1)),
        link("mailto:a@b", r(0, 3)),
        link("://x", r(6, 1)),
        link("'#top'", r(7, 1)),
        link("docs/gone.md", r(8, 1)),
        link("C:\\x", r(9, 1)),
        link("/", r(10, 1)),
    ]);
    let doc = make_doc(text, vec![paragraph], None, vec![]);
    let resolver = DocumentHealthResolver::new(|path| path == "docs/a.md");
    let absolute = ("Local path is absolute", "Use a project-relative path so the document works on another machine.");
    let asset_missing = ("asset.missing", "Local image asset was not found", LINK_MISSING.2);
    let malformed = ("url.malformed", "URL has a malformed scheme", "Use a valid absolute URL such as https://example.com or a relative path.");
    let unsafe_explanation = "Only web and mail links are accepted by the health pass; unsafe schemes can execute code or expose local data.";
    check(
        &DocumentHealth::analyze_document_with(&doc, DocumentHealthOptions::DEFAULT, Some(&resolver)),
        &[
            ("asset.absolute-path", "warning", "media", r(0, 1), absolute.0, absolute.1, None),
            (asset_missing.0, "warning", "media", r(0, 1), asset_missing.1, asset_missing.2, None),
            (
                "image.missing-alt",
                "error",
                "accessibility",
                r(0, 1),
                "Image is missing alternative text",
                "Describe the image's purpose so readers using assistive technology are not left without context.",
                None,
            ),
            (asset_missing.0, "warning", "media", r(1, 1), asset_missing.1, asset_missing.2, None),
            ("url.unsafe-scheme", "error", "links", r(1, 1), "URL uses unsafe scheme \u{2018}javascript:\u{2019}", unsafe_explanation, None),
            (malformed.0, "error", "links", r(2, 1), malformed.1, malformed.2, None),
            (malformed.0, "error", "links", r(6, 1), malformed.1, malformed.2, None),
            (LINK_MISSING.0, "warning", "links", r(8, 1), LINK_MISSING.1, LINK_MISSING.2, None),
            ("url.unsafe-scheme", "error", "links", r(9, 1), "URL uses unsafe scheme \u{2018}c:\u{2019}", unsafe_explanation, None),
            ("link.absolute-path", "warning", "links", r(10, 1), absolute.0, absolute.1, None),
            (LINK_MISSING.0, "warning", "links", r(10, 1), LINK_MISSING.1, LINK_MISSING.2, None),
        ],
    );
}

#[test]
fn options_clamp_to_one() {
    let options = DocumentHealthOptions::new(-5, 0, 7);
    assert_eq!((options.max_section_words, options.max_sentence_words, options.max_paragraph_words), (1, 1, 7));
    assert_eq!(DocumentHealthOptions::default(), DocumentHealthOptions::new(500, 35, 120));
}
