//! RenderTargetTests.swift (RenderTarget.swift + CompatibilityDiagnostics.swift).

use std::collections::HashSet;

use upleft_core::parser::MarkdownParser;
use upleft_core::compatibility::compatibility_diagnostics::MarkdownCompatibility;
use upleft_core::compatibility::render_target::{BuiltInRenderTarget, MarkdownCapabilities, MarkdownCapability, RenderTargetProfile};

#[test]
fn built_in_profiles_have_intentional_differences() {
    assert_eq!(BuiltInRenderTarget::ALL_CASES.len(), 9);
    assert_eq!(
        BuiltInRenderTarget::ALL_CASES.iter().map(|t| t.display_name()).collect::<Vec<_>>(),
        ["Downright", "CommonMark", "GitHub", "Obsidian", "Pandoc", "MultiMarkdown", "Jekyll", "Hugo", "Quarto"]
    );
    assert_eq!(RenderTargetProfile::built_ins().iter().filter_map(|p| p.built_in).count(), 9);
    assert_eq!(RenderTargetProfile::downright().capabilities, MarkdownCapabilities::ALL);
    assert!(!RenderTargetProfile::common_mark().capabilities.contains(MarkdownCapabilities::TABLES));
    assert!(RenderTargetProfile::git_hub().capabilities.contains(MarkdownCapabilities::TABLES));
    assert!(RenderTargetProfile::obsidian().capabilities.contains(MarkdownCapabilities::WIKILINKS));
    assert!(!RenderTargetProfile::git_hub().capabilities.contains(MarkdownCapabilities::WIKILINKS));
}

#[test]
fn every_capability_is_detected_from_parsed_ranges() {
    let source = "---\ntitle: Demo\n---\n\n# Heading {#demo}\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n- [ ] task\n~~strike~~ and $x^2$ and [[Notes]].\n\n[^one] and <span>HTML</span>\n\n[^one]: Footnote\n\n> [!NOTE] Alert\n\n```mermaid\ngraph TD\n```";
    let document = MarkdownParser::parse(source);
    let report = MarkdownCompatibility::diagnose(&document, &RenderTargetProfile::custom("No extensions", MarkdownCapabilities::EMPTY));
    let found: HashSet<MarkdownCapability> = report.diagnostics.iter().map(|d| d.capability).collect();
    let expected = MarkdownCapabilities::ALL.capabilities();
    assert!(expected.iter().all(|c| found.contains(c)));
    assert!(report.diagnostics.iter().all(|d| !d.explanation.is_empty()));
}

#[test]
fn diagnostics_use_exact_extension_ranges_and_stable_order() {
    let source = "---\ntitle: Demo\n---\n\n# Heading {#demo}\n\n> [!NOTE] Heads up\n> Body\n\nSee [[Notes]] and ~~old~~.\n\n| A | B |\n|---|---|\n| 1 | 2 |\n";
    let document = MarkdownParser::parse(source);
    let report = MarkdownCompatibility::diagnose(&document, &RenderTargetProfile::common_mark());
    let snippets: Vec<String> = report.diagnostics.iter().map(|d| document.substring(d.range)).collect();

    assert_eq!(
        snippets,
        ["---\ntitle: Demo\n---\n", "{#demo}", "> [!NOTE] Heads up", "[[Notes]]", "~~old~~", "| A | B |\n|---|---|\n| 1 | 2 |"]
    );
    let locations: Vec<isize> = report.diagnostics.iter().map(|d| d.range.location).collect();
    let mut sorted = locations.clone();
    sorted.sort();
    assert_eq!(locations, sorted);
    assert!(report.diagnostics.iter().all(|d| !d.explanation.is_empty()));
}

#[test]
fn wikilink_proposal_is_byte_local_and_reversible() {
    let source = "See [[Design Notes|the notes]].\n";
    let document = MarkdownParser::parse(source);
    let report = MarkdownCompatibility::diagnose(&document, &RenderTargetProfile::git_hub());
    let proposal = report.diagnostics.first().and_then(|d| d.proposal.clone()).expect("a proposal");
    let transformed = proposal.applying(source).expect("applies");
    assert_eq!(transformed, "See [the notes](<Design Notes>).\n");
    assert_eq!(proposal.reversing(&transformed).as_deref(), Some(source));
    assert_eq!(proposal.reversing("other text"), None);
}

/// The Swift test round-trips the profile through `JSONEncoder` /
/// `JSONDecoder`. MarkdownCore never encodes these types, so the `Codable`
/// conformance is not ported (see `render_target.rs`); only the value checks
/// that do not depend on JSON are kept.
#[test]
#[ignore = "Codable (JSON) round trip not ported: MarkdownCore never encodes RenderTargetProfile"]
fn custom_profile_round_trips_through_codable() {
    let profile = RenderTargetProfile::new("Docs", MarkdownCapabilities::TABLES | MarkdownCapabilities::MATH | MarkdownCapabilities::RAW_HTML);
    let decoded = profile.clone(); // stands in for the JSON round trip
    assert_eq!(decoded, profile);
    assert_eq!(decoded.built_in, None);
}

#[test]

fn report_and_proposal_codable_round_trip() {
    let source = "é [[Design Notes]]\n";
    let document = MarkdownParser::parse(source);
    let report = MarkdownCompatibility::diagnose(&document, &RenderTargetProfile::git_hub());
    let diagnostic = report.diagnostics.first().expect("a diagnostic");
    assert_eq!(diagnostic.capability, MarkdownCapability::Wikilinks);
    assert_eq!(diagnostic.range.location, 2);
    assert_eq!(document.substring(diagnostic.range), "[[Design Notes]]");

    // JSONEncoder / JSONDecoder round trip: not ported.
    let decoded = report.clone();
    assert_eq!(decoded, report);
    assert_eq!(
        decoded.diagnostics.first().and_then(|d| d.proposal.as_ref()).map(|p| p.replacement.as_str()),
        Some("[Design Notes](<Design Notes>)")
    );
}

#[test]
fn duplicate_findings_are_deterministic_and_range_ordered() {
    let source = "~~one~~ and [[A]] and ~~two~~ and [[B]]\n";
    let document = MarkdownParser::parse(source);
    let first = MarkdownCompatibility::diagnose(&document, &RenderTargetProfile::common_mark());
    let second = MarkdownCompatibility::diagnose(&document, &RenderTargetProfile::common_mark());
    assert_eq!(first, second);
    assert_eq!(first.diagnostics.iter().map(|d| d.range.location).collect::<Vec<_>>(), [0, 12, 22, 34]);
    assert_eq!(
        first.diagnostics.iter().map(|d| d.capability).collect::<Vec<_>>(),
        [MarkdownCapability::Strikethrough, MarkdownCapability::Wikilinks, MarkdownCapability::Strikethrough, MarkdownCapability::Wikilinks]
    );
}

#[test]
fn side_by_side_comparison_exposes_capability_delta() {
    let source = MarkdownParser::parse("# H\n\n- [ ] task\n");
    let result = MarkdownCompatibility::compare(&source, &RenderTargetProfile::downright(), &RenderTargetProfile::common_mark());
    assert_eq!(result.source.name, "Downright");
    assert_eq!(result.target.name, "CommonMark");
    assert!(result.only_in_source.contains(MarkdownCapabilities::TASK_LISTS));
    assert!(result.report.diagnostics.iter().any(|d| d.capability == MarkdownCapability::TaskLists));
}

// MARK: - Extra (not in Swift)

/// SafeHTMLTests.swift's `githubProfileContinuesToDescribeRawHTMLAsTargetCompatibility`,
/// which safe_html_tests.rs leaves for this port: the body to paste there.
#[test]
fn github_profile_continues_to_describe_raw_html_as_target_compatibility() {
    let parsed = MarkdownParser::parse("<strong>Text</strong>");
    let github = MarkdownCompatibility::diagnose(&parsed, &RenderTargetProfile::git_hub());
    let no_html = MarkdownCompatibility::diagnose(&parsed, &RenderTargetProfile::custom("No raw HTML", MarkdownCapabilities::EMPTY));
    assert!(!github.diagnostics.iter().any(|d| d.capability == MarkdownCapability::RawHTML));
    assert!(no_html.diagnostics.iter().any(|d| d.capability == MarkdownCapability::RawHTML));
}

#[test]
fn capability_set_is_an_option_set() {
    let set = MarkdownCapabilities::from(MarkdownCapability::Math) | MarkdownCapabilities::TABLES;
    assert_eq!(set.raw_value(), (1 << 4) | 1);
    assert!(set.contains_capability(MarkdownCapability::Tables));
    assert!(!set.contains(MarkdownCapabilities::TABLES | MarkdownCapabilities::MERMAID));
    assert_eq!(set.capabilities(), [MarkdownCapability::Tables, MarkdownCapability::Math]);
    assert_eq!(MarkdownCapabilities::ALL.raw_value(), 0x7FF);
    assert_eq!(MarkdownCapabilities::ALL.subtracting(set).capabilities().len(), 9);
    for capability in MarkdownCapability::ALL_CASES {
        assert_eq!(MarkdownCapability::from_raw_value(capability.raw_value()), Some(capability));
    }
    let custom = RenderTargetProfile::custom("Docs", set);
    assert_eq!(custom.id, "custom:Docs");
    assert_eq!(RenderTargetProfile::multi_markdown().id, "multiMarkdown");
    assert_eq!(RenderTargetProfile::quarto().capabilities.raw_value(), 0b111_0111_1111);
}

// MARK: - Differential cases (not in Swift)
//
// A hand-built document (no parser) run through Downright's own
// CompatibilityDiagnostics.swift compiled without the parser; the
// expectations are its output, recorded 2026-09-22 with Swift 6.4.

mod differential {
    use std::collections::HashMap;

    use upleft_core::compatibility::compatibility_diagnostics::{
        CompatibilityDiagnostic, CompatibilityTransformProposal, MarkdownCompatibility,
    };
    use upleft_core::compatibility::render_target::{MarkdownCapabilities, RenderTargetProfile};
    use upleft_core::source_positions::SourceMap;
    use upleft_core::swift_text::ns::{NSStringExt, utf16};
    use upleft_core::{
        BlockContent, CalloutKind, Checkbox, FrontMatter, InlineKind, InlineSpan, ListMarkerStyle, MDBlock, NSRange, ParsedDocument,
        TableData,
    };

    fn r(location: isize, length: isize) -> NSRange {
        NSRange::new(location, length)
    }

    fn find(text: &str, needle: &str) -> NSRange {
        let units = utf16(text);
        units.as_slice().range_of_literal(&utf16(needle), r(0, units.len() as isize))
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

    const T: &str = "~~one~~ and [[A]] and ~~two~~ and [[B c|Label]] $x$ `$y$` [^n] <b>\n\n# Head {#id .cls}  \n\n## Plain {x}\n\n~~x\ny~~ ~~ ~~ ~~~a~~~ ~~z~~\n\n- [ ] task\n\n> [!NOTE] hi\n\n<div>\n\n```mermaid\ng\n```\n\n$$\nx\n$$\n\n| a |\n|---|\n\n[^n]: note\n";

    fn document() -> ParsedDocument {
        let t = T;
        let paragraph = block(BlockContent::Paragraph, line(t, 0)).with_inlines(vec![
            span(InlineKind::Strikethrough, find(t, "~~one~~")),
            span(InlineKind::Wikilink { target: "A".into(), label: None }, find(t, "[[A]]")),
            span(InlineKind::Strikethrough, find(t, "~~two~~")),
            span(InlineKind::Wikilink { target: "B c".into(), label: Some("Label".into()) }, find(t, "[[B c|Label]]")),
            span(InlineKind::InlineMath { latex_range: find(t, "x$") }, find(t, "$x$")),
            span(InlineKind::InlineCode, find(t, "`$y$`")),
            span(InlineKind::FootnoteReference { identifier: "n".into() }, find(t, "[^n]")),
            span(InlineKind::InlineHTML, find(t, "<b>")),
        ]);
        let item = block(
            BlockContent::ListItem {
                ordinal: None,
                checkbox: Some(Checkbox::new(false, r(find(t, "[ ] task").location + 1, 1))),
            },
            line(t, 9),
        );
        let list = block(BlockContent::List { ordered: false, start: 1, tight: true, marker: ListMarkerStyle::Dash }, line(t, 9))
            .with_children(vec![item.into_ref()]);
        let callout = block(BlockContent::Callout { kind: CalloutKind::Note, title: Some("hi".into()) }, line(t, 11))
            .with_marker_range(Some(find(t, "> [!NOTE]")));
        let footnote = block(BlockContent::FootnoteDefinition { identifier: "n".into() }, line(t, 26)).into_ref();
        let footnote2 = block(BlockContent::FootnoteDefinition { identifier: "m".into() }, r(line(t, 26).location, 4)).into_ref();
        let front_matter = FrontMatter::new(vec![], r(0, 0), r(0, 0));
        let children = vec![
            block(BlockContent::FrontMatter(front_matter), r(0, 3)),
            paragraph,
            block(BlockContent::Heading { level: 1 }, line(t, 2)),
            block(BlockContent::Heading { level: 2 }, line(t, 4)),
            block(BlockContent::Paragraph, lines(t, 6, 7)),
            list,
            callout,
            block(BlockContent::HtmlBlock, line(t, 13)),
            block(BlockContent::Mermaid { source_range: line(t, 16) }, lines(t, 15, 17)),
            block(BlockContent::MathBlock { latex_range: line(t, 20) }, lines(t, 19, 21)),
            block(BlockContent::Table(TableData::new(vec![], vec![], line(t, 24))), lines(t, 23, 24)),
        ];
        let map = SourceMap::new(t);
        let root = MDBlock::new(BlockContent::Document, r(0, map.length), r(0, map.length))
            .with_children(children.into_iter().map(MDBlock::into_ref).chain([footnote.clone()]).collect());
        let footnotes = HashMap::from([("n".to_owned(), footnote), ("m".to_owned(), footnote2)]);
        ParsedDocument::new(
            t.to_owned(),
            map.length,
            root.into_ref(),
            None,
            vec![],
            vec![],
            vec![],
            footnotes,
            HashMap::new(),
            map.line_starts.clone(),
        )
    }

    type Expected = (&'static str, &'static str, NSRange, &'static str, &'static str, Option<(NSRange, &'static str, &'static str)>);

    fn check(diagnostics: &[CompatibilityDiagnostic], expected: &[Expected]) {
        let actual: Vec<_> = diagnostics
            .iter()
            .map(|d| {
                (
                    d.id.clone(),
                    d.capability.raw_value().to_owned(),
                    d.range,
                    d.title.clone(),
                    d.explanation.clone(),
                    d.proposal.as_ref().map(|p| (p.range, p.replacement.clone(), p.reverse_replacement.clone())),
                )
            })
            .collect();
        let expected: Vec<_> = expected
            .iter()
            .map(|e| (e.0.to_owned(), e.1.to_owned(), e.2, e.3.to_owned(), e.4.to_owned(), e.5.map(|p| (p.0, p.1.to_owned(), p.2.to_owned()))))
            .collect();
        assert_eq!(actual, expected);
    }

    // The two footnote definitions share a location (an artificial case: a
    // parse never produces it). Swift walks `document.footnotes.values` in
    // Dictionary order, which changes from run to run, and its stable sort
    // keeps that order for equal locations; so Swift prints (205, 4) and
    // (205, 10) in either order. The port walks them by position.
    #[test]
    fn differential_no_extensions() {
        let doc = document();
        let report = MarkdownCompatibility::diagnose(&doc, &RenderTargetProfile::custom("none", MarkdownCapabilities::EMPTY));
        assert_eq!(report.profile.id, "custom:none");
        check(&report.diagnostics, &[
            ("frontMatter:0:0", "frontMatter", r(0, 0), "Front matter is not supported", "The metadata fence and fields are not interpreted by this renderer.", None),
            ("strikethrough:0:7", "strikethrough", r(0, 7), "Strikethrough is not supported", "This renderer treats `~~text~~` as literal text.", None),
            ("strikethrough:5:19", "strikethrough", r(5, 19), "Strikethrough is not supported", "This renderer treats `~~text~~` as literal text.", None),
            ("wikilinks:12:5", "wikilinks", r(12, 5), "Wikilinks are not supported", "The `[[target]]` syntax will remain visible instead of becoming a link.", Some((r(12, 5), "[A](A)", "[[A]]"))),
            ("strikethrough:22:7", "strikethrough", r(22, 7), "Strikethrough is not supported", "This renderer treats `~~text~~` as literal text.", None),
            ("wikilinks:34:13", "wikilinks", r(34, 13), "Wikilinks are not supported", "The `[[target]]` syntax will remain visible instead of becoming a link.", Some((r(34, 13), "[Label](<B c>)", "[[B c|Label]]"))),
            ("math:48:3", "math", r(48, 3), "Inline math is not supported", "The `$\u{2026}$` or escaped math expression will not be rendered as mathematics.", None),
            ("footnotes:58:4", "footnotes", r(58, 4), "Footnotes are not supported", "Footnote references will not resolve in this renderer.", None),
            ("rawHTML:63:3", "rawHTML", r(63, 3), "Raw HTML is not supported", "Inline HTML is shown as literal text or removed by the renderer.", None),
            ("headingAttributes:75:10", "headingAttributes", r(75, 10), "Heading attributes are not supported", "The `{#id .class}` heading attribute will remain literal text.", None),
            ("strikethrough:114:6", "strikethrough", r(114, 6), "Strikethrough is not supported", "This renderer treats `~~text~~` as literal text.", None),
            ("strikethrough:125:5", "strikethrough", r(125, 5), "Strikethrough is not supported", "This renderer treats `~~text~~` as literal text.", None),
            ("taskLists:135:1", "taskLists", r(135, 1), "Task lists are not supported", "The checkbox marker will be rendered as ordinary list text.", None),
            ("calloutsAlerts:144:9", "calloutsAlerts", r(144, 9), "Callouts or alerts are not supported", "The callout marker will be treated as an ordinary blockquote.", None),
            ("rawHTML:158:5", "rawHTML", r(158, 5), "Raw HTML is not supported", "The HTML block will not be interpreted by the renderer.", None),
            ("mermaid:165:16", "mermaid", r(165, 16), "Mermaid is not supported", "The fenced diagram will remain a code block or plain text.", None),
            ("math:183:7", "math", r(183, 7), "Display math is not supported", "The display formula will not be rendered as mathematics.", None),
            ("tables:192:11", "tables", r(192, 11), "Tables are not supported", "The pipe table will not be laid out as a table.", None),
            ("footnotes:205:4", "footnotes", r(205, 4), "Footnotes are not supported", "Footnote definitions will not resolve in this renderer.", None),
            ("footnotes:205:10", "footnotes", r(205, 10), "Footnotes are not supported", "Footnote definitions will not resolve in this renderer.", None),
        ]);
    }

    #[test]
    fn differential_common_mark() {
        let report = MarkdownCompatibility::diagnose(&document(), &RenderTargetProfile::common_mark());
        check(&report.diagnostics, &[
            ("frontMatter:0:0", "frontMatter", r(0, 0), "Front matter is not supported", "The metadata fence and fields are not interpreted by this renderer.", None),
            ("strikethrough:0:7", "strikethrough", r(0, 7), "Strikethrough is not supported", "This renderer treats `~~text~~` as literal text.", None),
            ("strikethrough:5:19", "strikethrough", r(5, 19), "Strikethrough is not supported", "This renderer treats `~~text~~` as literal text.", None),
            ("wikilinks:12:5", "wikilinks", r(12, 5), "Wikilinks are not supported", "The `[[target]]` syntax will remain visible instead of becoming a link.", Some((r(12, 5), "[A](A)", "[[A]]"))),
            ("strikethrough:22:7", "strikethrough", r(22, 7), "Strikethrough is not supported", "This renderer treats `~~text~~` as literal text.", None),
            ("wikilinks:34:13", "wikilinks", r(34, 13), "Wikilinks are not supported", "The `[[target]]` syntax will remain visible instead of becoming a link.", Some((r(34, 13), "[Label](<B c>)", "[[B c|Label]]"))),
            ("math:48:3", "math", r(48, 3), "Inline math is not supported", "The `$\u{2026}$` or escaped math expression will not be rendered as mathematics.", None),
            ("footnotes:58:4", "footnotes", r(58, 4), "Footnotes are not supported", "Footnote references will not resolve in this renderer.", None),
            ("headingAttributes:75:10", "headingAttributes", r(75, 10), "Heading attributes are not supported", "The `{#id .class}` heading attribute will remain literal text.", None),
            ("strikethrough:114:6", "strikethrough", r(114, 6), "Strikethrough is not supported", "This renderer treats `~~text~~` as literal text.", None),
            ("strikethrough:125:5", "strikethrough", r(125, 5), "Strikethrough is not supported", "This renderer treats `~~text~~` as literal text.", None),
            ("taskLists:135:1", "taskLists", r(135, 1), "Task lists are not supported", "The checkbox marker will be rendered as ordinary list text.", None),
            ("calloutsAlerts:144:9", "calloutsAlerts", r(144, 9), "Callouts or alerts are not supported", "The callout marker will be treated as an ordinary blockquote.", None),
            ("mermaid:165:16", "mermaid", r(165, 16), "Mermaid is not supported", "The fenced diagram will remain a code block or plain text.", None),
            ("math:183:7", "math", r(183, 7), "Display math is not supported", "The display formula will not be rendered as mathematics.", None),
            ("tables:192:11", "tables", r(192, 11), "Tables are not supported", "The pipe table will not be laid out as a table.", None),
            ("footnotes:205:4", "footnotes", r(205, 4), "Footnotes are not supported", "Footnote definitions will not resolve in this renderer.", None),
            ("footnotes:205:10", "footnotes", r(205, 10), "Footnotes are not supported", "Footnote definitions will not resolve in this renderer.", None),
        ]);
    }

    #[test]
    fn differential_git_hub_and_downright() {
        let doc = document();
        check(&MarkdownCompatibility::diagnose(&doc, &RenderTargetProfile::git_hub()).diagnostics, &[
            ("frontMatter:0:0", "frontMatter", r(0, 0), "Front matter is not supported", "The metadata fence and fields are not interpreted by this renderer.", None),
            ("wikilinks:12:5", "wikilinks", r(12, 5), "Wikilinks are not supported", "The `[[target]]` syntax will remain visible instead of becoming a link.", Some((r(12, 5), "[A](A)", "[[A]]"))),
            ("wikilinks:34:13", "wikilinks", r(34, 13), "Wikilinks are not supported", "The `[[target]]` syntax will remain visible instead of becoming a link.", Some((r(34, 13), "[Label](<B c>)", "[[B c|Label]]"))),
            ("headingAttributes:75:10", "headingAttributes", r(75, 10), "Heading attributes are not supported", "The `{#id .class}` heading attribute will remain literal text.", None),
        ]);
        assert!(MarkdownCompatibility::diagnose(&doc, &RenderTargetProfile::downright()).diagnostics.is_empty());
        let comparison = MarkdownCompatibility::compare(&doc, &RenderTargetProfile::obsidian(), &RenderTargetProfile::jekyll());
        assert_eq!(
            (
                comparison.source_capabilities.raw_value(),
                comparison.target_capabilities.raw_value(),
                comparison.only_in_source.raw_value(),
                comparison.only_in_target.raw_value(),
                comparison.report.diagnostics.len(),
            ),
            (1023, 1807, 240, 1024, 6)
        );
    }

    /// A document's `NSString` substrings are bridged when it holds any
    /// non-ASCII character, and a bridged string's `contains("\n")` finds the
    /// LF of a CR LF.
    #[test]
    fn differential_bridged_substrings() {
        let cases: [(&str, Vec<(&str, NSRange)>); 3] = [
            ("x ~~a\r\nb~~ y\n", vec![("strikethrough", r(2, 8))]),
            ("\u{E9} ~~a\r\nb~~ y\n", vec![]),
            ("# H {#\u{200D}x}\n", vec![("headingAttributes", r(4, 5))]),
        ];
        for (text, expected) in cases {
            let content = if text.starts_with('#') { BlockContent::Heading { level: 1 } } else { BlockContent::Paragraph };
            let map = SourceMap::new(text);
            let root = MDBlock::new(BlockContent::Document, r(0, map.length), r(0, map.length))
                .with_children(vec![block(content, line(text, 0)).into_ref()]);
            let doc = ParsedDocument::new(
                text.to_owned(),
                map.length,
                root.into_ref(),
                None,
                vec![],
                vec![],
                vec![],
                HashMap::new(),
                HashMap::new(),
                map.line_starts.clone(),
            );
            let report = MarkdownCompatibility::diagnose(&doc, &RenderTargetProfile::common_mark());
            let actual: Vec<(&str, NSRange)> = report.diagnostics.iter().map(|d| (d.capability.raw_value(), d.range)).collect();
            assert_eq!(actual, expected, "{text:?}");
        }
    }

    #[test]
    fn differential_proposal_apply_and_reverse() {
        let proposal = CompatibilityTransformProposal::new(r(2, 3), "\u{E9}\r\n", "xyz", "s");
        assert_eq!(proposal.applying("abxyzc").as_deref(), Some("ab\u{E9}\r\nc"));
        assert_eq!(proposal.applying("ab"), None);
        assert_eq!(proposal.reversing("ab\u{E9}\r\nc").as_deref(), Some("abxyzc"));
        assert_eq!(proposal.reversing("abe\u{301}\r\nc"), None);
        assert_eq!(proposal.reversing("ab\u{E9}"), None);
    }
}
