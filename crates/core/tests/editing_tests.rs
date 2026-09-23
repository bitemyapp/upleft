//! EditingTests.swift — list editing and smart paste (§6.4).

use upleft_core::swift_text::ns::{string_from_utf16, utf16};
use upleft_core::*;

// MARK: - ListEditingTests

mod list_editing_tests {
    use super::*;
    use upleft_core::list_editing::ListEditing;
    use upleft_core::parser::MarkdownParser;

    fn press_return(text: &str, offset: isize) -> Option<String> {
        let doc = MarkdownParser::parse(text);
        let continuation = ListEditing::continuation(&doc, offset)?;
        // NSMutableString.replaceCharacters(in:with:)
        let mut ns = utf16(text);
        ns.splice(continuation.replace_range.as_usize_range(), utf16(&continuation.insertion));
        Some(string_from_utf16(&ns))
    }

    #[test]
    fn continues_a_bullet_list() {
        // Caret at the end of "- one".
        assert_eq!(press_return("- one\n- two\n", 5).as_deref(), Some("- one\n- \n- two\n"));
    }

    #[test]
    fn continues_an_ordered_list() {
        assert_eq!(press_return("1. one\n", 6).as_deref(), Some("1. one\n2. \n"));
        assert_eq!(press_return("3) three\n", 8).as_deref(), Some("3) three\n4) \n"));
    }

    #[test]
    fn continues_a_task_list() {
        assert_eq!(press_return("- [x] done\n", 10).as_deref(), Some("- [x] done\n- [ ] \n"));
    }

    #[test]
    fn keeps_nested_indentation() {
        assert_eq!(press_return("- top\n  - nested\n", 16).as_deref(), Some("- top\n  - nested\n  - \n"));
    }

    /// Regression: `markerRange` excludes the container indentation, so
    /// dropping the indentation from the front of the *marker text* stripped
    /// into the bullet itself; `*`/`+` items fell through to a `-`.
    #[test]
    fn continues_nested_items_with_their_own_marker_character() {
        assert_eq!(press_return("* top\n  * nested\n", 16).as_deref(), Some("* top\n  * nested\n  * \n"));
        assert_eq!(press_return("+ top\n  + nested\n", 16).as_deref(), Some("+ top\n  + nested\n  + \n"));
        assert_eq!(press_return("- top\n  * nested\n", 16).as_deref(), Some("- top\n  * nested\n  * \n"));
    }

    /// §6.4: "outdent-and-exit on an empty item."
    #[test]
    fn empty_item_at_top_level_exits_the_list() {
        assert_eq!(press_return("- one\n- \n", 8).as_deref(), Some("- one\n\n"));
    }

    #[test]
    fn empty_nested_item_outdents() {
        // `- top` / `  - ` would be a setext H2, not a nested item, so the
        // fixture needs a real nested list above the empty one.
        assert_eq!(press_return("- top\n  - a\n  - \n", 16).as_deref(), Some("- top\n  - a\n- \n"));
    }

    #[test]
    fn returns_nil_outside_a_list() {
        assert!(ListEditing::continuation(&MarkdownParser::parse("Just prose.\n"), 4).is_none());
        assert!(ListEditing::continuation(&MarkdownParser::parse("```\ncode\n```\n"), 6).is_none());
    }

    #[test]
    fn indents_and_outdents_list_lines() {
        let text = "- one\n- two\n";
        let doc = MarkdownParser::parse(text);
        let second = doc.range_of_line(2);
        let indented = applied(&ListEditing::indent(&doc, second, false), text);
        assert_eq!(indented, "- one\n  - two\n");

        let back = applied(&ListEditing::indent(&MarkdownParser::parse(&indented), NSRange::new(6, 7), true), &indented);
        assert_eq!(back, text);
    }

    #[test]
    fn indent_uses_the_marker_width() {
        let text = "1. one\n2. two\n";
        let doc = MarkdownParser::parse(text);
        let indented = applied(&ListEditing::indent(&doc, doc.range_of_line(2), false), text);
        assert_eq!(indented, "1. one\n   2. two\n");
    }

    #[test]
    fn outdent_at_column_zero_is_a_no_op() {
        let text = "- one\n";
        let doc = MarkdownParser::parse(text);
        assert!(ListEditing::indent(&doc, doc.range_of_line(1), true).is_empty());
    }

    #[test]
    fn prose_is_never_treated_as_a_list_item() {
        let text = "Paragraph text.\n";
        let doc = MarkdownParser::parse(text);
        assert!(ListEditing::indent(&doc, doc.range_of_line(1), false).is_empty());
        assert!(ListEditing::indent(&doc, doc.range_of_line(1), true).is_empty());
    }

    #[test]
    fn indent_spans_multiple_lines() {
        let text = "- a\n- b\n- c\n";
        let doc = MarkdownParser::parse(text);
        let edits = ListEditing::indent(&doc, NSRange::new(4, 8), false);
        assert_eq!(applied(&edits, text), "- a\n  - b\n  - c\n");
    }
}

// MARK: - SmartPasteTests

mod smart_paste_tests {
    use upleft_core::smart_paste::SmartPaste;
    use upleft_core::swift_text;

    #[test]
    fn linkifies_a_selection() {
        assert_eq!(SmartPaste::linkified("the docs", "https://example.com").as_deref(), Some("[the docs](https://example.com)"));
        assert_eq!(SmartPaste::linkified("", "https://example.com").as_deref(), Some("<https://example.com>"));
        assert_eq!(SmartPaste::linkified("mail me", "mailto:a@b.com").as_deref(), Some("[mail me](mailto:a@b.com)"));
        assert_eq!(SmartPaste::linkified("x", "www.example.com").as_deref(), Some("[x](https://www.example.com)"));
    }

    #[test]
    fn refuses_non_urls() {
        assert_eq!(SmartPaste::linkified("x", "just some text"), None);
        assert_eq!(SmartPaste::linkified("x", ""), None);
        assert_eq!(SmartPaste::linkified("x", "not-a-url"), None);
    }

    #[test]
    fn escapes_brackets_in_the_label() {
        assert_eq!(SmartPaste::linkified("a [b] c", "https://x.com").as_deref(), Some("[a \\[b\\] c](https://x.com)"));
    }

    #[test]
    fn converts_tab_separated_data_to_a_table() {
        let out = SmartPaste::markdown_table_for_tab_separated("Name\tCount\nAda\t1\nGrace\t22\n");
        assert_eq!(
            out.as_deref(),
            Some("| Name  | Count |\n| ----- | ----- |\n| Ada   | 1     |\n| Grace | 22    |")
        );
    }

    #[test]
    fn escapes_pipes_in_pasted_cells() {
        let out = SmartPaste::markdown_table_for_tab_separated("a|b\tc\n1\t2\n");
        assert_eq!(out.map(|out| swift_text::contains(&out, "a\\|b")), Some(true));
    }

    #[test]
    fn returns_nil_for_text_without_tabs() {
        assert_eq!(SmartPaste::markdown_table_for_tab_separated("just\nsome\nlines\n"), None);
        assert_eq!(SmartPaste::markdown_table_for_tab_separated(""), None);
    }

    #[test]
    fn converts_html_headings_paragraphs_and_emphasis() {
        let markdown =
            SmartPaste::markdown_for_html("<h2>Title</h2><p>Some <strong>bold</strong> and <em>italic</em> text.</p>");
        assert_eq!(markdown, "## Title\n\nSome **bold** and *italic* text.");
    }

    #[test]
    fn converts_html_links_and_images() {
        assert_eq!(SmartPaste::markdown_for_html("<p>See <a href=\"https://x.com\">here</a>.</p>"), "See [here](https://x.com).");
        assert_eq!(SmartPaste::markdown_for_html("<p><img src=\"a.png\" alt=\"Alt\"></p>"), "![Alt](a.png)");
    }

    #[test]
    fn drops_executable_html_destinations_without_dropping_visible_words() {
        assert_eq!(SmartPaste::markdown_for_html("<p><a href=\"javascript:alert(1)\">keep me</a></p>"), "keep me");
        assert_eq!(
            SmartPaste::markdown_for_html("<p>before<img src=\"data:text/html,evil\" alt=\"bad\">after</p>"),
            "beforeafter"
        );
    }

    #[test]
    fn converts_html_lists() {
        assert_eq!(SmartPaste::markdown_for_html("<ul><li>one</li><li>two</li></ul>"), "- one\n- two");
        assert_eq!(SmartPaste::markdown_for_html("<ol><li>one</li><li>two</li></ol>"), "1. one\n2. two");
    }

    #[test]
    fn converts_safari_fragment_without_collapsing_blocks() {
        let html = "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><style>body { color: red }</style></head><body><!--StartFragment-->\n\
<div><h1>Safari selection</h1><p>Intro <strong>bold</strong> and <a href=\"https://example.com\">link</a>.</p>\n\
<ul><li>first<ul><li>nested <code>code</code></li></ul></li><li>second</li></ul>\n\
<table><thead><tr><th>Name</th><th>Count</th></tr></thead><tbody><tr><td>Ada</td><td>1</td></tr><tr><td>Grace</td><td>22</td></tr></tbody></table>\n\
<pre><code>let x = 1</code></pre><script>alert(1)</script><p>After<br>line</p></div>\n\
<!--EndFragment--></body></html>";
        // Safari includes wrapper tags, indentation, and fragment markers in
        // public.html. The visible structure must survive normal Paste.
        // Whitespace-only lines are not allowed to split the list/table into
        // prose, while executable content remains discarded.
        let expected = String::from(
            "# Safari selection\n\n\
Intro **bold** and [link](https://example.com).\n\n\
- first\n  - nested `code`\n- second\n\n\
| Name  | Count |\n| ----- | ----- |\n| Ada   | 1     |\n| Grace | 22    |\n\n\
```\nlet x = 1\n```\n",
        ) + "\nAfter  \nline";
        assert_eq!(SmartPaste::markdown_for_html(html), expected);
    }

    #[test]
    fn converts_html_code_and_tables() {
        assert_eq!(SmartPaste::markdown_for_html("<p>Use <code>x = 1</code> here.</p>"), "Use `x = 1` here.");
        let table = SmartPaste::markdown_for_html("<table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>");
        assert_eq!(table, "| A   | B   |\n| --- | --- |\n| 1   | 2   |");
    }

    #[test]
    fn decodes_entities_and_drops_scripts() {
        assert_eq!(SmartPaste::markdown_for_html("<p>a &amp; b &lt;c&gt;</p>"), "a & b <c>");
        assert_eq!(SmartPaste::markdown_for_html("<script>evil()</script><p>safe</p>"), "safe");
        assert_eq!(SmartPaste::markdown_for_html("<style>p{}</style><p>safe</p>"), "safe");
    }

    #[test]
    fn unknown_tags_degrade_to_their_text_content() {
        assert_eq!(SmartPaste::markdown_for_html("<p>a <mark>highlighted</mark> word</p>"), "a highlighted word");
    }

    #[test]
    fn match_style_html_keeps_block_and_list_line_breaks() {
        let html = String::from("<h2>Title</h2><p>First <strong>paragraph</strong>.</p>") + "<ul><li>one</li><li>two</li></ul><p>Last</p>";
        assert_eq!(SmartPaste::plain_text_for_html(&html), "Title\n\nFirst paragraph.\none\ntwo\n\nLast");
    }

    #[test]
    fn match_style_html_collapses_inline_whitespace_only() {
        assert_eq!(
            SmartPaste::plain_text_for_html("<p>one <span style=\"color:red\">two</span>\t three</p>"),
            "one two three"
        );
    }
}

// MARK: - Probed Swift semantics (recorded from Swift 6.4 running the
// original SmartPaste.swift / TableFormatter.swift / BlockMarker)

mod probed_semantics {
    use upleft_core::restructure::BlockMarker;
    use upleft_core::smart_paste::SmartPaste;

    #[test]
    fn native_components_split_tabs_by_character() {
        // A native Swift String splits "\t\u{301}" at the tab (Character-wise);
        // NSString's composed-sequence search would not.
        assert_eq!(
            SmartPaste::markdown_table_for_tab_separated("é \t\t\u{301}").as_deref(),
            Some("| é   |     | \u{301}   |\n| --- | --- | --- |")
        );
        assert_eq!(SmartPaste::markdown_table_for_tab_separated("\t\u{301}").as_deref(), Some("|     | \u{301}   |\n| --- | --- |"));
    }

    #[test]
    fn crlf_between_blocks_collapses() {
        assert_eq!(SmartPaste::markdown_for_html("<p>a</p>\r\n<p>b</p>"), "a\n\nb");
    }

    #[test]
    fn prepended_attribute_names_do_not_match() {
        // U+0600 prepends to the `s` of `src=`, so Swift's Character-wise
        // search finds no attribute and the image is dropped.
        assert_eq!(SmartPaste::markdown_for_html("<p><img \u{600}src=\"x.png\"></p>"), "");
        assert_eq!(SmartPaste::markdown_for_html("<p><img ſrc=\"x.png\" alt=\"a\"></p>"), "![a](x.png)");
    }

    #[test]
    fn url_components_accept_what_foundation_accepts() {
        assert_eq!(SmartPaste::linkified("x", "https://ſ.com").as_deref(), Some("[x](https://ſ.com)"));
        assert_eq!(SmartPaste::linkified("x", "http://"), None);
        assert_eq!(SmartPaste::linkified("x", "mailto:%FF"), None);
        assert_eq!(SmartPaste::linkified("x", "https://x.com/(a)").as_deref(), Some("[x](<https://x.com/(a)>)"));
    }

    #[test]
    fn block_marker_strip() {
        assert_eq!(BlockMarker::strip("> > ## - [x] task"), "task");
        assert_eq!(BlockMarker::strip("####### seven"), "####### seven");
        assert_eq!(BlockMarker::strip("\u{661}. arabic-indic"), "arabic-indic");
        assert_eq!(BlockMarker::strip("-\u{301} x"), "-\u{301} x");
        // The task-marker strip runs even without a bullet in front.
        assert_eq!(BlockMarker::strip("[ ] bare"), "bare");
    }
}
