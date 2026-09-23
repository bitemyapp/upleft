//! ExtensionTests.swift — each extension is a small, independently testable
//! AST transform (§4.1).
//!
//! The Swift suites go through `MarkdownParser.parse`, so their ports wait for
//! the parser. `direct_scanner_checks` feeds the same inputs straight to the
//! scanners; those expectations were recorded from the Swift scanners
//! themselves (the Swift sources compiled standalone with a probe driver).

use upleft_core::swift_text::ns::{NSStringExt, utf16};
use upleft_core::*;

// MARK: - FrontMatterTests

mod front_matter_tests {
    use super::*;
    use upleft_core::parser::MarkdownParser;

    #[test]

    fn parses_scalars_quotes_and_inline_lists() {
        let doc = MarkdownParser::parse("---\ntitle: Release Plan\nowner: \"Ada Lovelace\"\ntags: [alpha, beta]\ncount: 3\n---\n\n# Body");
        let front = doc.front_matter.as_ref();
        assert!(front.is_some());
        assert_eq!(front.and_then(|f| f.get("title")), Some("Release Plan"));
        assert_eq!(front.and_then(|f| f.get("owner")), Some("Ada Lovelace"));
        assert_eq!(front.and_then(|f| f.get("tags")), Some("alpha, beta"));
        assert_eq!(front.and_then(|f| f.get("count")), Some("3"));
        assert_eq!(
            doc.substring(front.unwrap().range),
            "---\ntitle: Release Plan\nowner: \"Ada Lovelace\"\ntags: [alpha, beta]\ncount: 3\n---\n"
        );
    }

    #[test]

    fn parses_block_sequences() {
        let doc = MarkdownParser::parse("---\ntags:\n  - one\n  - two\nx: y\n---\n\nBody\n");
        assert_eq!(doc.front_matter.as_ref().and_then(|f| f.get("tags")), Some("one, two"));
        assert_eq!(doc.front_matter.as_ref().and_then(|f| f.get("x")), Some("y"));
    }

    /// The reason front matter is stripped before cmark runs: left in place it
    /// parses as a thematic break plus a setext H2.
    #[test]

    fn body_after_front_matter_parses_with_correct_ranges() {
        let text = "---\ntitle: X\n---\n\n# Heading\n\nBody.\n";
        let doc = MarkdownParser::parse(text);
        assert_eq!(doc.headings.len(), 1);
        assert_eq!(doc.headings[0].level, 1);
        assert_eq!(doc.substring(doc.headings[0].range), "# Heading");
        // No thematic break should have been produced by the fences.
        let mut breaks = 0;
        doc.root.walk(&mut |block| {
            if matches!(block.content, BlockContent::ThematicBreak) {
                breaks += 1;
            }
        });
        assert_eq!(breaks, 0);
    }

    #[test]

    fn only_matches_on_the_very_first_line() {
        assert!(MarkdownParser::parse("\n---\ntitle: X\n---\n").front_matter.is_none());
        assert!(MarkdownParser::parse("# H\n\n---\ntitle: X\n---\n").front_matter.is_none());
        assert!(MarkdownParser::parse("---\nno closing fence\n").front_matter.is_none());
    }

    #[test]

    fn ignores_what_it_cannot_parse_rather_than_failing() {
        let doc = MarkdownParser::parse("---\nnested:\n  deep:\n    value: 1\nok: yes\n---\n\nBody\n");
        assert!(doc.front_matter.is_some());
        assert_eq!(doc.front_matter.as_ref().and_then(|f| f.get("ok")), Some("yes"));
    }

    #[test]

    fn parses_block_scalar_values() {
        let doc = MarkdownParser::parse(
            "---\nsummary: |\n  first line\n  second line\nabstract: >\n  folded one\n  folded two\nok: yes\n---\n\nBody",
        );
        let front = doc.front_matter.as_ref();
        assert_eq!(front.and_then(|f| f.get("summary")), Some("first line\nsecond line"));
        assert_eq!(front.and_then(|f| f.get("abstract")), Some("folded one folded two"));
        assert_eq!(front.and_then(|f| f.get("ok")), Some("yes"));
    }

    #[test]

    fn parses_inline_array_with_commas_inside_quotes() {
        let doc = MarkdownParser::parse("---\ntags: [\"alpha, beta\", \"gamma, delta\"]\ntitle: \"Hello, World\"\n---\n\nBody");
        assert_eq!(doc.front_matter.as_ref().and_then(|f| f.get("tags")), Some("alpha, beta, gamma, delta"));
        assert_eq!(doc.front_matter.as_ref().and_then(|f| f.get("title")), Some("Hello, World"));
    }
}

// MARK: - MathTests

mod math_tests {
    use super::*;
    use upleft_core::parser::MarkdownParser;

    fn inline_math(text: &str) -> Vec<String> {
        let doc = MarkdownParser::parse(text);
        let mut out = Vec::new();
        doc.root.walk(&mut |block| {
            for span in &block.inlines {
                span.walk(&mut |inline| {
                    if let InlineKind::InlineMath { latex_range } = inline.kind {
                        out.push(doc.substring(latex_range));
                    }
                });
            }
        });
        out
    }

    #[test]

    fn matches_inline_and_escaped_delimiters() {
        assert_eq!(inline_math("Math $x^2$ here\n"), vec!["x^2"]);
        assert_eq!(inline_math("Math \\(a+b\\) here\n"), vec!["a+b"]);
        assert_eq!(inline_math("Two $a$ and $b$ here\n"), vec!["a", "b"]);
    }

    /// The cases §4.1 names explicitly. A false positive turns prose into a
    /// broken glyph, so these matter more than the positives.
    #[test]

    fn rejects_shell_and_currency() {
        assert!(inline_math("Run `echo $PATH` now\n").is_empty());
        assert!(inline_math("Run echo $PATH now\n").is_empty());
        assert!(inline_math("It costs $5 and $10\n").is_empty());
        assert!(inline_math("Between $100 and $200 total\n").is_empty());
        assert!(inline_math("Use $(cmd) and $(other) here\n").is_empty());
        assert!(inline_math("A $VAR and $OTHER pair\n").is_empty());
        assert!(inline_math("Empty $$ pair\n").is_empty());
    }

    #[test]

    fn never_matches_inside_code() {
        assert!(inline_math("A `$x$` span\n").is_empty());
        let doc = MarkdownParser::parse("```bash\necho $x$ y\n```\n");
        doc.root.walk(&mut |block| {
            if matches!(block.content, BlockContent::CodeBlock { .. }) {
                assert!(block.inlines.is_empty());
            }
        });
    }

    #[test]

    fn labelled_form_displays_only_its_label() {
        let source = "See [[Design Notes|the notes]] here\n";
        let document = MarkdownParser::parse(source);
        let paragraph = document.root.children.first().expect("a paragraph");
        let span = paragraph.inlines.iter().find(|inline| matches!(inline.kind, InlineKind::Wikilink { .. })).expect("a wikilink span");
        let ns = utf16(source);
        assert_eq!(ns.as_slice().substring(span.content_range), "the notes");
        assert_eq!(ns.as_slice().substring(span.leading_marker_range.expect("a leading marker")), "[[Design Notes|");
    }

    #[test]

    fn whole_paragraph_display_math_becomes_a_block() {
        let doc = MarkdownParser::parse("Intro.\n\n$$\ne^{i\\pi} + 1 = 0\n$$\n\nOutro.\n");
        let mut found: Option<String> = None;
        doc.root.walk(&mut |block| {
            if let BlockContent::MathBlock { latex_range } = block.content {
                found = Some(doc.substring(latex_range));
            }
        });
        assert_eq!(found.as_deref().map(upleft_core::swift_text::trim_whitespaces_and_newlines), Some("e^{i\\pi} + 1 = 0"));
    }

    #[test]

    fn math_fences_become_math_blocks() {
        let doc = MarkdownParser::parse("```math\nx = 1\n```\n");
        let BlockContent::MathBlock { latex_range } = doc.root.children.first().unwrap().content else {
            panic!("expected a math block");
        };
        assert_eq!(doc.substring(latex_range), "x = 1\n");
    }

    #[test]

    fn matrix_double_backslash_does_not_trigger_escaped_closer() {
        let text = "Formula \\( \\begin{pmatrix} 1 \\\\ ) 2 \\end{pmatrix} \\) works\n";
        let matches = inline_math(text);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0], " \\begin{pmatrix} 1 \\\\ ) 2 \\end{pmatrix} ");
    }
}

// MARK: - CalloutTests

mod callout_tests {
    use super::*;
    use upleft_core::parser::MarkdownParser;

    #[test]

    fn recognises_kind_and_title() {
        let doc = MarkdownParser::parse("> [!WARNING] Be careful\n> The body.\n");
        let BlockContent::Callout { kind, title } = &doc.root.children.first().unwrap().content else {
            panic!("expected a callout, got {:?}", doc.root.children.first().unwrap().content);
        };
        assert_eq!(*kind, CalloutKind::Warning);
        assert_eq!(title.as_deref(), Some("Be careful"));
        assert_eq!(doc.substring(doc.root.children[0].marker_range.unwrap()), "> [!WARNING] Be careful");
    }

    #[test]

    fn handles_multiple_spaces_and_tabs_after_quote_marker() {
        let multiple_spaces = MarkdownParser::parse(">  [!NOTE] Spaced\n> Body\n");
        let BlockContent::Callout { kind: kind1, title: title1 } = &multiple_spaces.root.children.first().unwrap().content else {
            panic!("expected callout for multiple spaces");
        };
        assert_eq!(*kind1, CalloutKind::Note);
        assert_eq!(title1.as_deref(), Some("Spaced"));

        let tabbed = MarkdownParser::parse(">\t[!TIP] Tabbed\n> Body\n");
        let BlockContent::Callout { kind: kind2, title: title2 } = &tabbed.root.children.first().unwrap().content else {
            panic!("expected callout for tabbed marker");
        };
        assert_eq!(*kind2, CalloutKind::Tip);
        assert_eq!(title2.as_deref(), Some("Tabbed"));
    }

    #[test]

    fn is_case_insensitive_and_title_is_optional() {
        for token in ["[!note]", "[!Note]", "[!NOTE]"] {
            let doc = MarkdownParser::parse(&format!("> {token}\n> body\n"));
            let BlockContent::Callout { kind, title } = &doc.root.children.first().unwrap().content else {
                panic!("expected a callout for {token}");
            };
            assert_eq!(*kind, CalloutKind::Note);
            assert_eq!(*title, None);
        }
    }

    #[test]

    fn marker_text_is_lifted_out_of_the_body() {
        let doc = MarkdownParser::parse("> [!TIP] Hint\n> Body text.\n");
        let callout = &doc.root.children[0];
        assert!(!upleft_core::swift_text::contains(&doc.substring(callout.content_range), "[!TIP]"));
        assert!(upleft_core::swift_text::contains(&doc.substring(callout.content_range), "Body text."));
    }

    #[test]

    fn plain_quotes_stay_quotes() {
        let doc = MarkdownParser::parse("> just a quote\n");
        assert!(matches!(doc.root.children.first().unwrap().content, BlockContent::BlockQuote), "expected a blockquote");
        let unknown = MarkdownParser::parse("> [!NOTAKIND] x\n");
        assert!(
            matches!(unknown.root.children.first().unwrap().content, BlockContent::BlockQuote),
            "an unknown callout kind must stay a blockquote"
        );
    }
}

// MARK: - WikilinkTests

mod wikilink_tests {
    use super::*;
    use upleft_core::parser::MarkdownParser;

    fn wikilinks(text: &str) -> Vec<(String, Option<String>)> {
        let doc = MarkdownParser::parse(text);
        let mut out = Vec::new();
        doc.root.walk(&mut |block| {
            for span in &block.inlines {
                span.walk(&mut |inline| {
                    if let InlineKind::Wikilink { target, label } = &inline.kind {
                        out.push((target.clone(), label.clone()));
                    }
                });
            }
        });
        out
    }

    #[test]

    fn rejects_wikilinks_across_lone_cr() {
        let text = "[[Target\rLabel]]";
        assert!(wikilinks(text).is_empty());
    }

    #[test]

    fn matches_both_forms() {
        let plain = wikilinks("See [[Design Notes]] here\n");
        assert_eq!(plain.len(), 1);
        assert_eq!(plain[0].0, "Design Notes");
        assert_eq!(plain[0].1, None);

        let labelled = wikilinks("See [[Design Notes|the notes]] here\n");
        assert_eq!(labelled.len(), 1);
        assert_eq!(labelled[0].0, "Design Notes");
        assert_eq!(labelled[0].1.as_deref(), Some("the notes"));
    }

    #[test]

    fn padded_targets_and_labels_trim_but_keep_geometry() {
        let padded = wikilinks("See [[ Design Notes | the notes ]] here\n");
        assert_eq!(padded.len(), 1);
        assert_eq!(padded[0].0, "Design Notes");
        assert_eq!(padded[0].1.as_deref(), Some("the notes"));

        let doc = MarkdownParser::parse("[[  A  ]]\n");
        let mut span: Option<(Option<NSRange>, Option<NSRange>, Option<NSRange>)> = None;
        doc.root.walk(&mut |block| {
            for s in &block.inlines {
                s.walk(&mut |inline| {
                    if matches!(inline.kind, InlineKind::Wikilink { .. }) {
                        span = Some((inline.leading_marker_range, Some(inline.content_range), inline.trailing_marker_range));
                    }
                });
            }
        });
        // markers and content must cover the written padding, not the trimmed
        // target: `[[` is a 2-char marker, the body `  A  ` is content.
        let (leading, content, trailing) = span.unwrap();
        assert_eq!(leading, Some(NSRange::new(0, 2)));
        assert_eq!(content, Some(NSRange::new(2, 5)));
        assert_eq!(trailing, Some(NSRange::new(7, 2)));
    }

    #[test]

    fn never_matches_inside_code() {
        assert!(wikilinks("A `[[Name]]` span\n").is_empty());
    }

    #[test]

    fn ignores_malformed_brackets() {
        assert!(wikilinks("A [[unclosed here\n").is_empty());
        assert!(wikilinks("A [[]] here\n").is_empty());
    }
}

// MARK: - PathTokenTests

mod path_token_tests {
    use upleft_core::parser::MarkdownParser;

    fn paths(text: &str) -> Vec<String> {
        MarkdownParser::parse(text).path_tokens.iter().map(|t| t.token.raw_path.clone()).collect()
    }

    #[test]

    fn finds_paths_with_line_numbers() {
        let doc = MarkdownParser::parse("Edit src/auth/session.ts:42 next.\n");
        assert_eq!(doc.path_tokens.len(), 1);
        assert_eq!(doc.path_tokens[0].token.raw_path, "src/auth/session.ts");
        assert_eq!(doc.path_tokens[0].token.line, Some(42));
        // The whole token is underlined and opened, `:42` included.
        assert_eq!(doc.substring(doc.path_tokens[0].range), "src/auth/session.ts:42");
    }

    #[test]

    fn finds_relative_and_extension_only_forms() {
        assert_eq!(paths("See ./x/y.md for details.\n"), vec!["./x/y.md"]);
        assert_eq!(paths("Open Package.swift now.\n"), vec!["Package.swift"]);
        assert_eq!(paths("Check src/foo.ts here.\n"), vec!["src/foo.ts"]);
        assert_eq!(paths("Look at ../parent/file.py today.\n"), vec!["../parent/file.py"]);
    }

    #[test]

    fn code_spans_relax_the_shape_rules() {
        let doc = MarkdownParser::parse("Run `docs/plans` for it.\n");
        assert_eq!(doc.path_tokens.len(), 1);
        assert!(doc.path_tokens[0].from_code_span);
        assert_eq!(doc.path_tokens[0].token.raw_path, "docs/plans");
    }

    /// §8.4 is a trust instrument: underlining `and/or` would train the user
    /// to ignore the signal, so prose needs a real path shape.
    #[test]

    fn rejects_prose_that_merely_contains_a_slash() {
        assert!(paths("Use and/or as needed.\n").is_empty());
        assert!(paths("A read/write lock here.\n").is_empty());
        assert!(paths("They said he/him plainly.\n").is_empty());
    }

    #[test]

    fn rejects_urls() {
        assert!(paths("Visit https://example.com/x.md today.\n").is_empty());
        assert!(paths("Mail mailto:a@b.com now.\n").is_empty());
        assert!(paths("See www.example.com/a.md here.\n").is_empty());
    }

    #[test]

    fn trims_sentence_punctuation() {
        assert_eq!(paths("Look at src/foo.ts.\n"), vec!["src/foo.ts"]);
        assert_eq!(paths("Files: src/a.ts, src/b.ts.\n"), vec!["src/a.ts", "src/b.ts"]);
    }

    #[test]

    fn never_touches_the_filesystem() {
        // A path that certainly does not exist still produces a token; the app
        // resolves, not the parser.
        assert_eq!(paths("See imaginary/nowhere/at/all.ts here.\n"), vec!["imaginary/nowhere/at/all.ts"]);
    }

    /// Regression: a non-ASCII character such as an emoji in a code span used
    /// to force-unwrap `UnicodeScalar` on a UTF-16 surrogate half and trap.
    #[test]

    fn emoji_in_code_span_does_not_crash() {
        assert!(paths("Run `config.🚀` next.\n").is_empty());
        // A real extension after the emoji still resolves.
        assert_eq!(paths("Edit `src/cache.🚀.ts`.\n"), vec!["src/cache.🚀.ts"]);
    }

    /// Regression: a one-character path before a `:digits` suffix (`3:16`,
    /// `9:30`) used to reach `isURL` with a one-unit range and trap.
    #[test]

    fn single_character_clock_and_ratio_tokens_do_not_crash() {
        assert!(paths("John 3:16 says so.\n").is_empty());
        assert!(paths("Meet at 9:30 sharp.\n").is_empty());
        assert!(paths("A ratio of 1:2 here.\n").is_empty());
        assert!(paths("Run `a:1` for it.\n").is_empty());
        assert!(paths("Verse 2:5 and chapter 3:16 agree.\n").is_empty());
        // A real path keeps its suffix behaviour after the fix.
        let doc = MarkdownParser::parse("Edit src/auth/session.ts:42 next.\n");
        assert_eq!(doc.path_tokens.first().and_then(|t| t.token.line), Some(42));
    }
}

// MARK: - FenceLanguageTests

mod fence_language_tests {
    use super::*;
    use upleft_core::extensions::fence_language::FenceLanguage;
    use upleft_core::parser::MarkdownParser;

    #[test]

    fn mermaid_becomes_a_diagram() {
        let doc = MarkdownParser::parse("```mermaid\ngraph TD;\nA-->B;\n```\n");
        let BlockContent::Mermaid { source_range } = doc.root.children.first().unwrap().content else {
            panic!("expected a mermaid block");
        };
        assert_eq!(doc.substring(source_range), "graph TD;\nA-->B;\n");
    }

    #[test]

    fn diff_keeps_its_language_and_stays_code() {
        let doc = MarkdownParser::parse("```diff\n- a\n+ b\n```\n");
        let BlockContent::CodeBlock { language, .. } = &doc.root.children.first().unwrap().content else {
            panic!("expected a code block");
        };
        assert_eq!(language.as_deref(), Some("diff"));
    }

    #[test]
    fn guesses_only_when_confident() {
        assert_eq!(FenceLanguage::guess("#!/usr/bin/env python\nprint(1)\n").as_deref(), Some("python"));
        assert_eq!(FenceLanguage::guess("func f() -> Int { 1 }\nguard x else { }\n").as_deref(), Some("swift"));
        assert_eq!(FenceLanguage::guess("def f(self):\n    import os\n").as_deref(), Some("python"));
        assert_eq!(FenceLanguage::guess("const x = 1;\nfunction f() {}\n").as_deref(), Some("javascript"));
        assert_eq!(FenceLanguage::guess("<div>\n</div>\n").as_deref(), Some("html"));
        assert_eq!(FenceLanguage::guess("$ ls -la\n$ cd /tmp\n").as_deref(), Some("bash"));
        assert_eq!(FenceLanguage::guess("just some prose here\nnothing to see\n"), None);
        assert_eq!(FenceLanguage::guess(""), None);
    }
}

// MARK: - Direct scanner checks (not in the Swift suite)
//
// The Swift tests' inputs fed straight to each scanner. Expectations were
// recorded from the Swift scanners (SafeHTML.swift and Extensions/*.swift
// compiled standalone against a copy of SourcePositions.swift's SourceMap);
// they are not derived by reading the code.

mod direct_scanner_checks {
    use super::*;
    use upleft_core::extensions::callout_scanner::CalloutScanner;
    use upleft_core::extensions::front_matter_scanner::FrontMatterScanner;
    use upleft_core::extensions::math_scanner::MathScanner;
    use upleft_core::extensions::path_token_scanner::PathTokenScanner;
    use upleft_core::extensions::wikilink_scanner::WikilinkScanner;
    use upleft_core::source_positions::SourceMap;

    fn r(location: isize, length: isize) -> NSRange {
        NSRange::new(location, length)
    }

    fn fields(text: &str) -> Option<(NSRange, NSRange, Vec<(String, String, NSRange, NSRange)>)> {
        let front = FrontMatterScanner::scan(&SourceMap::new(text))?;
        Some((
            front.range,
            front.body_range,
            front.fields.iter().map(|f| (f.key.clone(), f.value.clone(), f.key_range, f.value_range)).collect(),
        ))
    }

    fn field(key: &str, value: &str, key_range: NSRange, value_range: NSRange) -> (String, String, NSRange, NSRange) {
        (key.to_owned(), value.to_owned(), key_range, value_range)
    }

    #[test]
    fn front_matter_scanner_on_the_swift_inputs() {
        assert_eq!(
            fields("---\ntitle: Release Plan\nowner: \"Ada Lovelace\"\ntags: [alpha, beta]\ncount: 3\n---\n\n# Body"),
            Some((
                r(0, 79),
                r(4, 71),
                vec![
                    field("title", "Release Plan", r(4, 5), r(10, 13)),
                    field("owner", "Ada Lovelace", r(24, 5), r(30, 15)),
                    field("tags", "alpha, beta", r(46, 4), r(51, 14)),
                    field("count", "3", r(66, 5), r(72, 2)),
                ]
            ))
        );
        assert_eq!(
            fields("---\ntags:\n  - one\n  - two\nx: y\n---\n\nBody\n"),
            Some((r(0, 35), r(4, 27), vec![field("tags", "one, two", r(4, 4), r(9, 16)), field("x", "y", r(26, 1), r(28, 2))]))
        );
        assert_eq!(
            fields("---\ntitle: X\n---\n\n# Heading\n\nBody.\n"),
            Some((r(0, 17), r(4, 9), vec![field("title", "X", r(4, 5), r(10, 2))]))
        );
        assert_eq!(fields("\n---\ntitle: X\n---\n"), None);
        assert_eq!(fields("# H\n\n---\ntitle: X\n---\n"), None);
        assert_eq!(fields("---\nno closing fence\n"), None);
        assert_eq!(
            fields("---\nnested:\n  deep:\n    value: 1\nok: yes\n---\n\nBody\n"),
            Some((r(0, 45), r(4, 37), vec![field("ok", "yes", r(33, 2), r(36, 4))]))
        );
        assert_eq!(
            fields("---\nsummary: |\n  first line\n  second line\nabstract: >\n  folded one\n  folded two\nok: yes\n---\n\nBody"),
            Some((
                r(0, 92),
                r(4, 84),
                vec![
                    field("summary", "first line\nsecond line", r(4, 7), r(12, 29)),
                    field("abstract", "folded one folded two", r(42, 8), r(51, 28)),
                    field("ok", "yes", r(80, 2), r(83, 4)),
                ]
            ))
        );
        assert_eq!(
            fields("---\ntags: [\"alpha, beta\", \"gamma, delta\"]\ntitle: \"Hello, World\"\n---\n\nBody"),
            Some((
                r(0, 68),
                r(4, 60),
                vec![field("tags", "alpha, beta, gamma, delta", r(4, 4), r(9, 32)), field("title", "Hello, World", r(42, 5), r(48, 15))]
            ))
        );
    }

    fn callout(text: &str) -> Option<(CalloutKind, Option<String>, NSRange)> {
        let map = SourceMap::new(text);
        CalloutScanner::scan(&map, r(0, 0)).map(|m| (m.kind, m.title, m.marker_range))
    }

    #[test]
    fn callout_scanner_on_the_swift_inputs() {
        assert_eq!(callout("> [!WARNING] Be careful\n> The body.\n"), Some((CalloutKind::Warning, Some("Be careful".into()), r(0, 23))));
        assert_eq!(callout(">  [!NOTE] Spaced\n> Body\n"), Some((CalloutKind::Note, Some("Spaced".into()), r(0, 17))));
        assert_eq!(callout(">\t[!TIP] Tabbed\n> Body\n"), Some((CalloutKind::Tip, Some("Tabbed".into()), r(0, 15))));
        for token in ["[!note]", "[!Note]", "[!NOTE]"] {
            assert_eq!(callout(&format!("> {token}\n> body\n")), Some((CalloutKind::Note, None, r(0, 9))));
        }
        assert_eq!(callout("> [!TIP] Hint\n> Body text.\n"), Some((CalloutKind::Tip, Some("Hint".into()), r(0, 13))));
        assert_eq!(callout("> just a quote\n"), None);
        assert_eq!(callout("> [!NOTAKIND] x\n"), None);
        // The second line of a callout is not itself a callout.
        let map = SourceMap::new("> [!WARNING] Be careful\n> The body.\n");
        assert_eq!(CalloutScanner::scan(&map, r(24, 0)), None);
    }

    fn wikilinks(text: &str) -> Vec<(NSRange, NSRange, String, Option<String>)> {
        let ns = utf16(text);
        WikilinkScanner::matches(&ns, r(0, ns.len() as isize)).into_iter().map(|m| (m.range, m.target_range, m.target, m.label)).collect()
    }

    #[test]
    fn wikilink_scanner_on_the_swift_inputs() {
        assert_eq!(wikilinks("[[Target\rLabel]]"), vec![]);
        assert_eq!(wikilinks("See [[Design Notes]] here\n"), vec![(r(4, 16), r(6, 12), "Design Notes".into(), None)]);
        assert_eq!(
            wikilinks("See [[Design Notes|the notes]] here\n"),
            vec![(r(4, 26), r(6, 12), "Design Notes".into(), Some("the notes".into()))]
        );
        assert_eq!(
            wikilinks("See [[ Design Notes | the notes ]] here\n"),
            vec![(r(4, 30), r(6, 14), "Design Notes".into(), Some("the notes".into()))]
        );
        assert_eq!(wikilinks("[[  A  ]]\n"), vec![(r(0, 9), r(2, 5), "A".into(), None)]);
        assert_eq!(wikilinks("A [[unclosed here\n"), vec![]);
        assert_eq!(wikilinks("A [[]] here\n"), vec![]);
    }

    fn prose_paths(text: &str) -> Vec<(NSRange, String, Option<isize>, Option<isize>)> {
        let ns = utf16(text);
        PathTokenScanner::matches(&ns, r(0, ns.len() as isize))
            .into_iter()
            .map(|m| (m.range, m.token.raw_path, m.token.line, m.token.column))
            .collect()
    }

    fn code_span_path(text: &str) -> Option<(NSRange, String)> {
        let ns = utf16(text);
        PathTokenScanner::code_span_match(&ns, r(0, ns.len() as isize)).map(|m| (m.range, m.token.raw_path))
    }

    #[test]
    fn path_token_scanner_on_the_swift_inputs() {
        assert_eq!(prose_paths("Edit src/auth/session.ts:42 next.\n"), vec![(r(5, 22), "src/auth/session.ts".into(), Some(42), None)]);
        assert_eq!(prose_paths("See ./x/y.md for details.\n"), vec![(r(4, 8), "./x/y.md".into(), None, None)]);
        assert_eq!(prose_paths("Open Package.swift now.\n"), vec![(r(5, 13), "Package.swift".into(), None, None)]);
        assert_eq!(prose_paths("Check src/foo.ts here.\n"), vec![(r(6, 10), "src/foo.ts".into(), None, None)]);
        assert_eq!(prose_paths("Look at ../parent/file.py today.\n"), vec![(r(8, 17), "../parent/file.py".into(), None, None)]);
        for prose in [
            "Use and/or as needed.\n",
            "A read/write lock here.\n",
            "They said he/him plainly.\n",
            "Visit https://example.com/x.md today.\n",
            "Mail mailto:a@b.com now.\n",
            "See www.example.com/a.md here.\n",
            "John 3:16 says so.\n",
            "Meet at 9:30 sharp.\n",
            "A ratio of 1:2 here.\n",
            "Verse 2:5 and chapter 3:16 agree.\n",
        ] {
            assert_eq!(prose_paths(prose), vec![], "{prose:?}");
        }
        assert_eq!(prose_paths("Look at src/foo.ts.\n"), vec![(r(8, 10), "src/foo.ts".into(), None, None)]);
        assert_eq!(
            prose_paths("Files: src/a.ts, src/b.ts.\n"),
            vec![(r(7, 8), "src/a.ts".into(), None, None), (r(17, 8), "src/b.ts".into(), None, None)]
        );
        assert_eq!(
            prose_paths("See imaginary/nowhere/at/all.ts here.\n"),
            vec![(r(4, 27), "imaginary/nowhere/at/all.ts".into(), None, None)]
        );

        // Code span contents.
        assert_eq!(code_span_path("docs/plans"), Some((r(0, 10), "docs/plans".into())));
        assert_eq!(code_span_path("config.🚀"), None);
        assert_eq!(code_span_path("src/cache.🚀.ts"), Some((r(0, 15), "src/cache.🚀.ts".into())));
        assert_eq!(code_span_path("a:1"), None);
    }

    #[test]
    fn path_token_location_suffixes() {
        // Swift reads the *last* segment as the line and the one before it as
        // the column; `parseDigits` overflow and the `> 0` guards leave the
        // suffix on the path, whose extension (`ts:0`) is then unknown.
        let token = |text: &str| {
            let ns = utf16(text);
            PathTokenScanner::code_span_match(&ns, r(0, ns.len() as isize)).map(|m| (m.token.raw_path, m.token.line, m.token.column))
        };
        assert_eq!(token("x.ts:42:8"), Some(("x.ts".into(), Some(8), Some(42))));
        assert_eq!(token("x.ts:9223372036854775807"), Some(("x.ts".into(), Some(isize::MAX), None)));
        assert_eq!(token("x.ts:9223372036854775808"), None);
        assert_eq!(token("x.ts:0"), None);
        assert_eq!(token("x.ts:0:5"), None);
    }

    /// MathScanner (ported earlier, not by this group) against Swift: a
    /// `String` from `NSString.substring(with:)` of a non-ASCII document is
    /// NSString-backed, and its `contains("\n")` is Foundation's search, which
    /// finds the LF of a CR LF. Recorded from the Swift MathScanner.
    #[test]
    fn math_body_with_crlf_in_a_non_ascii_document() {
        let text = utf16("é $a\r\nb$ z");
        assert_eq!(MathScanner::matches(&text, r(0, text.len() as isize)), vec![]);
        // In an all-ASCII document the substring is native, and Character-wise
        // `contains` does not see the LF inside CR LF: the math matches.
        let ascii = utf16("e $a\r\nb$ z");
        assert_eq!(MathScanner::matches(&ascii, r(0, ascii.len() as isize)).len(), 1);
    }
}
