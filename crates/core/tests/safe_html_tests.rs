//! SafeHTMLTests.swift — safe presentational HTML.
//!
//! Every Swift test goes through `MarkdownParser.parse`, so the ports wait for
//! the parser. `direct_parser_checks` feeds the same sources straight to
//! `SafeHTMLParser` with the block ranges written out; those expectations were
//! recorded from the Swift `SafeHTMLParser` (SafeHTML.swift compiled
//! standalone with a probe driver). The block ranges are this file's own
//! choice (each run between blank lines, without its trailing newline), not
//! ranges taken from swift-markdown.

use upleft_core::parser::MarkdownParser;
use upleft_core::safe_html::{SafeHTMLAlignment, SafeHTMLDocument, SafeHTMLKind};
use upleft_core::swift_text::ns::utf16;

fn any_kind(annotations: &[upleft_core::safe_html::SafeHTMLAnnotation], test: impl Fn(&SafeHTMLKind) -> bool) -> bool {
    annotations.iter().any(|a| test(&a.kind))
}

#[test]

fn common_readme_html_is_source_addressed_and_safe() {
    let source = r#"<p align="center"><strong>Downright</strong><br><a href="https://example.com">Home</a></p>"#;
    let parsed = MarkdownParser::parse(source);
    let html = parsed.root.children.first().and_then(|b| b.safe_html.clone()).expect("safe HTML");
    assert!(html.is_safe);
    assert!(any_kind(&html.annotations, |k| matches!(k, SafeHTMLKind::Paragraph { align: Some(SafeHTMLAlignment::Center) })));
    assert!(any_kind(&html.annotations, |k| matches!(k, SafeHTMLKind::Strong)));
    assert!(any_kind(&html.annotations, |k| matches!(k, SafeHTMLKind::LineBreak)));
    assert!(any_kind(&html.annotations, |k| matches!(
        k,
        SafeHTMLKind::Link { destination, title: None } if destination == "https://example.com"
    )));
    let length = utf16(source).len() as isize;
    assert!(html.tag_ranges().iter().all(|r| r.location >= 0 && r.upper_bound() <= length));
}

#[test]

fn details_tables_and_local_images_stay_in_the_subset() {
    let source = "<details open><summary>More</summary><table><tr><th align=\"right\">A</th><td>B</td></tr></table></details>\n<img src=\"Docs/demo.png\" alt=\"Demo\">";
    let parsed = MarkdownParser::parse(source);
    let documents: Vec<SafeHTMLDocument> = parsed.root.children.iter().filter_map(|b| b.safe_html.clone()).collect();
    assert!(!documents.is_empty());
    assert!(documents.iter().all(|d| d.is_safe));
    let annotations: Vec<_> = documents.iter().flat_map(|d| d.annotations.clone()).collect();
    assert!(any_kind(&annotations, |k| matches!(k, SafeHTMLKind::Details { open: true })));
    assert!(any_kind(&annotations, |k| matches!(k, SafeHTMLKind::TableCell { header: true, align: Some(SafeHTMLAlignment::Right) })));
    assert!(any_kind(&annotations, |k| matches!(
        k,
        SafeHTMLKind::Image { source, alt } if source == "Docs/demo.png" && alt == "Demo"
    )));
}

/// The parser skips the per-block HTML search when the source holds no `<`
/// at all. The shortcut is document-wide, so a `<` in an unrelated block must
/// leave every other block's annotations exactly as they were.
#[test]

fn the_document_wide_html_shortcut_does_not_change_annotations() {
    let without_angle_brackets = "# Title\n\nA paragraph with **bold**, `code`, and a [link](https://example.com).\n\n- [ ] a task";
    let plain = MarkdownParser::parse(without_angle_brackets);
    assert!(plain.root.children.iter().all(|b| b.safe_html.is_none()));

    // Same document, plus one unrelated block that does contain `<`.
    let with_a_distant_angle_bracket = format!("{without_angle_brackets}\n\n<p align=\"center\"><strong>Footer</strong></p>");
    let mixed = MarkdownParser::parse(&with_a_distant_angle_bracket);
    let annotated: Vec<SafeHTMLDocument> = mixed.root.children.iter().filter_map(|b| b.safe_html.clone()).collect();
    assert_eq!(annotated.len(), 1);
    assert!(annotated[0].is_safe);
    assert!(any_kind(&annotated[0].annotations, |k| matches!(k, SafeHTMLKind::Strong)));
    // The blocks shared with the first document are unchanged.
    let shared = &mixed.root.children[..plain.root.children.len()];
    assert!(shared.iter().all(|b| b.safe_html.is_none()));
}

#[test]

fn unsafe_or_unknown_html_remains_literal_and_inert() {
    for source in [
        r#"<script>alert(1)</script>"#,
        r#"<p onclick="steal()">Text</p>"#,
        r#"<a href="javascript:alert(1)">Run</a>"#,
        r#"<iframe src="https://example.com"></iframe>"#,
    ] {
        let parsed = MarkdownParser::parse(source);
        let html = parsed.root.children.first().and_then(|b| b.safe_html.clone()).expect("safe HTML");
        assert!(!html.is_safe);
        assert!(html.annotations.is_empty());
        assert_eq!(parsed.text, source);
    }
}

#[test]

fn unbalanced_details_without_a_real_document_boundary_stay_literal() {
    for source in ["<details>", "</details>"] {
        let parsed = MarkdownParser::parse(source);
        let html = parsed.root.children.first().and_then(|b| b.safe_html.clone()).expect("safe HTML");
        assert!(!html.is_safe);
        assert!(html.annotations.is_empty());
        assert_eq!(parsed.text, source);
    }
}

/// The cross-block accommodation must answer for parsed tags, not raw
/// substrings: a mention inside a code span or prose must not license hiding
/// a stray literal tag in another block.
#[test]

fn details_mentions_in_code_spans_and_prose_do_not_satisfy_the_cross_block_check() {
    for opener in ["`<details>`", "the <details> element is a container"] {
        let source = format!("{opener}\n\n</details>");
        let parsed = MarkdownParser::parse(&source);
        let documents: Vec<SafeHTMLDocument> = parsed.root.children.iter().filter_map(|b| b.safe_html.clone()).collect();
        let closing = documents.last().expect("a closing document");
        assert!(!closing.is_safe, "a stray closer with only a textual mention before it stays literal");
        assert!(closing.annotations.is_empty());
        assert_eq!(parsed.text, source);
    }

    for closer in ["`</details>`", "write </details> to close"] {
        let source = format!("<details open>\n\n{closer}");
        let parsed = MarkdownParser::parse(&source);
        let documents: Vec<SafeHTMLDocument> = parsed.root.children.iter().filter_map(|b| b.safe_html.clone()).collect();
        let opening = documents.first().expect("an opening document");
        assert!(!opening.is_safe, "an opener whose only partner is textual mention stays literal");
        assert!(opening.annotations.is_empty());
    }
}

/// The genuine README shape — an opening block, a blank line, the body,
/// another blank line, the closing block — keeps its cross-block pairing.
#[test]

fn split_details_across_blank_lines_still_pairs_up() {
    let source = "<details open>\n<summary>More</summary>\n\nBody text across the boundary.\n\n</details>";
    let parsed = MarkdownParser::parse(source);
    let annotations: Vec<_> = parsed.root.children.iter().filter_map(|b| b.safe_html.clone()).flat_map(|d| d.annotations).collect();
    assert!(any_kind(&annotations, |k| matches!(k, SafeHTMLKind::Details { .. })));
    assert!(any_kind(&annotations, |k| matches!(k, SafeHTMLKind::DetailsClosing)));
}

#[test]

fn remote_images_stay_visible_but_inert_inside_safe_parents() {
    let source = r#"<p align="center"><img src="https://tracker.example/pixel.png" alt="remote"></p>"#;
    let parsed = MarkdownParser::parse(source);
    let html = parsed.root.children.first().and_then(|b| b.safe_html.clone()).expect("safe HTML");
    assert!(html.is_safe);
    assert!(any_kind(&html.annotations, |k| matches!(k, SafeHTMLKind::Inert)));
}

#[test]
#[ignore = "needs parser (upleft-markup) and compatibility (CompatibilityDiagnostics.swift, RenderTarget.swift)"]
fn github_profile_continues_to_describe_raw_html_as_target_compatibility() {
    let parsed = MarkdownParser::parse("<strong>Text</strong>");
    let _ = parsed;
    // The compatibility module is a placeholder in this worktree. Once it is
    // ported, this is the Swift test (API names to match that port):
    //
    // let github = MarkdownCompatibility::diagnose(&parsed, &RenderTarget::GitHub);
    // let no_html = MarkdownCompatibility::diagnose(
    //     &parsed,
    //     &RenderTarget::Custom { name: "No raw HTML".into(), capabilities: MarkdownCapabilities::empty() },
    // );
    // assert!(!github.diagnostics.iter().any(|d| d.capability == MarkdownCapability::RawHTML));
    // assert!(no_html.diagnostics.iter().any(|d| d.capability == MarkdownCapability::RawHTML));
    unimplemented!("needs the compatibility port");
}

#[test]

fn downright_readme_corpus_classifies_safe_and_remote_risk_html() {
    // `#filePath` → Tests/MarkdownCoreTests → the Downright repository root.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vendor/downright/README.md");
    let readme = std::fs::read_to_string(path).expect("Downright's README.md");
    let parsed = MarkdownParser::parse(&readme);
    let mut documents: Vec<SafeHTMLDocument> = Vec::new();
    parsed.root.walk(&mut |block| {
        if let Some(html) = &block.safe_html {
            documents.push(html.clone());
        }
    });
    assert!(!documents.is_empty());
    assert!(documents.iter().any(|html| any_kind(&html.annotations, |k| matches!(k, SafeHTMLKind::Inert))));
    assert_eq!(parsed.text, readme);
}

// MARK: - Direct parser checks (not in the Swift suite)

mod direct_parser_checks {
    use upleft_core::NSRange;
    use upleft_core::safe_html::{SafeHTMLAlignment, SafeHTMLAnnotation, SafeHTMLKind, SafeHTMLParser};
    use upleft_core::swift_text::ns::utf16;

    fn r(location: isize, length: isize) -> NSRange {
        NSRange::new(location, length)
    }

    fn a(kind: SafeHTMLKind, range: NSRange, content: NSRange, tags: &[NSRange]) -> SafeHTMLAnnotation {
        SafeHTMLAnnotation::new(kind, range, content, tags.to_vec())
    }

    /// `(is_safe, annotations)` for `range` of `source`, through the NSString
    /// entry point the Markdown parser uses.
    fn parse(source: &str, range: NSRange) -> Option<(bool, Vec<SafeHTMLAnnotation>)> {
        let units = utf16(source);
        SafeHTMLParser::parse_ns(&units, Some(range)).map(|d| (d.is_safe, d.annotations))
    }

    #[test]
    fn common_readme_html() {
        let source = r#"<p align="center"><strong>Downright</strong><br><a href="https://example.com">Home</a></p>"#;
        let document = SafeHTMLParser::parse(source, None).unwrap();
        assert_eq!(document.range, r(0, 90));
        assert!(document.is_safe);
        assert_eq!(
            document.annotations,
            vec![
                a(SafeHTMLKind::Paragraph { align: Some(SafeHTMLAlignment::Center) }, r(0, 90), r(18, 68), &[r(0, 18), r(86, 4)]),
                a(SafeHTMLKind::Strong, r(18, 26), r(26, 9), &[r(18, 8), r(35, 9)]),
                a(SafeHTMLKind::LineBreak, r(44, 4), r(48, 0), &[r(44, 4)]),
                a(
                    SafeHTMLKind::Link { destination: "https://example.com".into(), title: None },
                    r(48, 38),
                    r(78, 4),
                    &[r(48, 30), r(82, 4)]
                ),
            ]
        );
        assert_eq!(document.tag_ranges(), vec![r(0, 18), r(18, 8), r(35, 9), r(44, 4), r(48, 30), r(82, 4), r(86, 4)]);
        // A range cutting through the element is malformed.
        assert_eq!(parse(source, r(30, 60)), Some((false, vec![])));
    }

    #[test]
    fn details_tables_and_local_images() {
        let source = "<details open><summary>More</summary><table><tr><th align=\"right\">A</th><td>B</td></tr></table></details>\n<img src=\"Docs/demo.png\" alt=\"Demo\">";
        let document = SafeHTMLParser::parse(source, None).unwrap();
        assert!(document.is_safe);
        assert_eq!(
            document.annotations,
            vec![
                a(SafeHTMLKind::Details { open: true }, r(0, 105), r(14, 81), &[r(0, 14), r(95, 10)]),
                a(SafeHTMLKind::Summary, r(14, 23), r(23, 4), &[r(14, 9), r(27, 10)]),
                a(SafeHTMLKind::Table, r(37, 58), r(44, 43), &[r(37, 7), r(87, 8)]),
                a(SafeHTMLKind::TableRow, r(44, 43), r(48, 34), &[r(44, 4), r(82, 5)]),
                a(
                    SafeHTMLKind::TableCell { header: true, align: Some(SafeHTMLAlignment::Right) },
                    r(48, 24),
                    r(66, 1),
                    &[r(48, 18), r(67, 5)]
                ),
                a(SafeHTMLKind::TableCell { header: false, align: None }, r(72, 10), r(76, 1), &[r(72, 4), r(77, 5)]),
                a(SafeHTMLKind::Image { source: "Docs/demo.png".into(), alt: "Demo".into() }, r(106, 36), r(142, 0), &[r(106, 36)]),
            ]
        );
        // A range starting inside `<details>` closes it across the boundary:
        // the opener is the first `<details` at a block line start before it.
        assert_eq!(
            parse(source, r(35, 71)),
            Some((
                true,
                vec![
                    a(SafeHTMLKind::Table, r(37, 58), r(44, 43), &[r(37, 7), r(87, 8)]),
                    a(SafeHTMLKind::TableRow, r(44, 43), r(48, 34), &[r(44, 4), r(82, 5)]),
                    a(
                        SafeHTMLKind::TableCell { header: true, align: Some(SafeHTMLAlignment::Right) },
                        r(48, 24),
                        r(66, 1),
                        &[r(48, 18), r(67, 5)]
                    ),
                    a(SafeHTMLKind::TableCell { header: false, align: None }, r(72, 10), r(76, 1), &[r(72, 4), r(77, 5)]),
                    a(SafeHTMLKind::DetailsClosing, r(95, 10), r(95, 0), &[r(95, 10)]),
                ]
            ))
        );
    }

    #[test]
    fn unsafe_or_unknown_html() {
        for source in [
            r#"<script>alert(1)</script>"#,
            r#"<p onclick="steal()">Text</p>"#,
            r#"<a href="javascript:alert(1)">Run</a>"#,
            r#"<iframe src="https://example.com"></iframe>"#,
            "<details>",
            "</details>",
        ] {
            let document = SafeHTMLParser::parse(source, None).unwrap();
            assert!(!document.is_safe, "{source}");
            assert!(document.annotations.is_empty(), "{source}");
        }
    }

    #[test]
    fn details_mentions_do_not_satisfy_the_cross_block_check() {
        // `<details>` in a code span or prose, then a closing block.
        assert_eq!(parse("`<details>`\n\n</details>", r(13, 10)), Some((false, vec![])));
        assert_eq!(parse("the <details> element is a container\n\n</details>", r(38, 10)), Some((false, vec![])));
        assert_eq!(
            parse("the <details> element is a container\n\n</details>", r(0, 36)),
            Some((true, vec![a(SafeHTMLKind::Details { open: false }, r(4, 9), r(13, 0), &[r(4, 9)]),]))
        );
        // An opening block whose only partner is a textual mention.
        assert_eq!(parse("<details open>\n\n`</details>`", r(0, 14)), Some((false, vec![])));
        assert_eq!(parse("<details open>\n\nwrite </details> to close", r(0, 14)), Some((false, vec![])));
    }

    #[test]
    fn split_details_pairs_across_blank_lines() {
        let source = "<details open>\n<summary>More</summary>\n\nBody text across the boundary.\n\n</details>";
        assert_eq!(
            parse(source, r(0, 38)),
            Some((
                true,
                vec![
                    a(SafeHTMLKind::Details { open: true }, r(0, 14), r(14, 0), &[r(0, 14)]),
                    a(SafeHTMLKind::Summary, r(15, 23), r(24, 4), &[r(15, 9), r(28, 10)]),
                ]
            ))
        );
        assert_eq!(parse(source, r(40, 30)), None);
        assert_eq!(parse(source, r(72, 10)), Some((true, vec![a(SafeHTMLKind::DetailsClosing, r(72, 10), r(72, 0), &[r(72, 10)])])));
    }

    #[test]
    fn remote_images_are_inert() {
        let source = r#"<p align="center"><img src="https://tracker.example/pixel.png" alt="remote"></p>"#;
        let document = SafeHTMLParser::parse(source, None).unwrap();
        assert!(document.is_safe);
        assert_eq!(
            document.annotations,
            vec![
                a(SafeHTMLKind::Paragraph { align: Some(SafeHTMLAlignment::Center) }, r(0, 80), r(18, 58), &[r(0, 18), r(76, 4)]),
                a(SafeHTMLKind::Inert, r(18, 58), r(76, 0), &[r(18, 58)]),
            ]
        );
    }

    #[test]
    fn distant_angle_bracket_block() {
        let source = "# Title\n\nA paragraph with **bold**, `code`, and a [link](https://example.com).\n\n- [ ] a task\n\n<p align=\"center\"><strong>Footer</strong></p>";
        for block in [r(0, 7), r(9, 69), r(80, 12)] {
            assert_eq!(parse(source, block), None);
        }
        assert_eq!(
            parse(source, r(94, 45)),
            Some((
                true,
                vec![
                    a(SafeHTMLKind::Paragraph { align: Some(SafeHTMLAlignment::Center) }, r(94, 45), r(112, 23), &[r(94, 18), r(135, 4)]),
                    a(SafeHTMLKind::Strong, r(112, 23), r(120, 6), &[r(112, 8), r(126, 9)]),
                ]
            ))
        );
    }

    /// Swift string semantics the port has to keep (recorded from Swift).
    #[test]
    fn swift_string_semantics_in_tags() {
        // `<br/ >`: `body.dropLast()` drops the trailing space, not the `/`,
        // so the name is `br/` and the tag is rejected.
        assert_eq!(parse("<br/ >", r(0, 6)), Some((false, vec![])));
        assert_eq!(parse("<br />", r(0, 6)), Some((true, vec![a(SafeHTMLKind::LineBreak, r(0, 6), r(6, 0), &[r(0, 6)])])));
        // A combining mark after `:` hides it from Character-wise `contains`
        // and `hasPrefix`, so this `javascript:` URL passes `safeURL`.
        assert_eq!(
            parse("<a href=\"javascript:\u{301}x\">y</a>", r(0, 29)),
            Some((
                true,
                vec![a(
                    SafeHTMLKind::Link { destination: "javascript:\u{301}x".into(), title: None },
                    r(0, 29),
                    r(24, 1),
                    &[r(0, 24), r(25, 4)]
                )]
            ))
        );
        // Tag names and attribute values compare case-insensitively.
        assert_eq!(
            parse("<P ALIGN=\"CENTER\">x</P>", r(0, 23)),
            Some((
                true,
                vec![a(SafeHTMLKind::Paragraph { align: Some(SafeHTMLAlignment::Center) }, r(0, 23), r(18, 1), &[r(0, 18), r(19, 4)])]
            ))
        );
        // A `<` inside text is malformed; a mismatched close pops the element.
        assert_eq!(parse("<p>a<b</p>", r(0, 10)), Some((false, vec![])));
        assert_eq!(
            parse("<details>\n\n<p></details>", r(11, 13)),
            Some((true, vec![a(SafeHTMLKind::DetailsClosing, r(14, 10), r(14, 0), &[r(14, 10)])]))
        );
    }
}
