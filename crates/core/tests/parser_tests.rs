//! ParserTests.swift — `SourcePositionTests` and `ParserTests`.

mod common;

use common::corpus;
use upleft_core::model::{BlockContent, InlineKind, TableAlignment};
use upleft_core::parser::MarkdownParser;
use upleft_core::source_positions::SourceMap;
use upleft_core::swift_text;

// MARK: - SourcePositionTests

/// swift-markdown's column is a UTF-8 byte offset; pinned down rather than
/// assumed.
#[test]
fn columns_are_utf8_bytes_converted_to_utf16() {
    let map = SourceMap::new("héllo *wörld*\n日本語 text\n");
    assert_eq!(map.offset(1, 8), 6);
    assert_eq!(map.offset(1, 1), 0);
    assert_eq!(map.offset(2, 1), 14);
    assert_eq!(map.offset(2, 10), 17);
}

#[test]
fn tabs_count_as_one_byte() {
    let map = SourceMap::new("\tindented\n");
    assert_eq!(map.offset(1, 2), 1);
    assert_eq!(map.offset(1, 10), 9);
}

#[test]
fn line_index_handles_every_terminator() {
    let map = SourceMap::new("a\r\nb\rc\nd");
    assert_eq!(map.line_count(), 4);
    assert_eq!(map.string_of_line(0), "a");
    assert_eq!(map.string_of_line(1), "b");
    assert_eq!(map.string_of_line(2), "c");
    assert_eq!(map.string_of_line(3), "d");
}

#[test]
fn trailing_newline_produces_a_virtual_final_line() {
    let map = SourceMap::new("a\n");
    assert_eq!(map.line_count(), 2);
    assert_eq!(map.content_range_of_line(1).length, 0);
}

// MARK: - ParserTests

/// Every block's range is in bounds and its content range inside it.
#[test]
fn ranges_are_well_formed_across_the_corpus() {
    for (name, text) in corpus::ALL {
        let doc = MarkdownParser::parse(text);
        doc.root.walk(&mut |block| {
            assert!(block.range.location >= 0, "{name}");
            assert!(block.range.upper_bound() <= doc.length, "{name}: {:?}", block.content);
            assert!(block.content_range.location >= block.range.location, "{name}: {:?}", block.content);
            assert!(block.content_range.upper_bound() <= block.range.upper_bound(), "{name}: {:?}", block.content);
            if let Some(marker) = block.marker_range {
                assert!(marker.location >= block.range.location, "{name}: {:?}", block.content);
                assert!(marker.upper_bound() <= block.range.upper_bound(), "{name}: {:?}", block.content);
            }
            if let Some(trailing) = block.trailing_marker_range {
                assert!(trailing.upper_bound() <= block.range.upper_bound(), "{name}: {:?}", block.content);
            }
            for span in &block.inlines {
                span.walk(&mut |inline| {
                    assert!(inline.range.upper_bound() <= doc.length, "{name}");
                    assert!(inline.content_range.location >= inline.range.location, "{name}");
                    assert!(inline.content_range.upper_bound() <= inline.range.upper_bound(), "{name}");
                });
            }
            for pair in block.inlines.windows(2) {
                assert!(pair[0].range.upper_bound() <= pair[1].range.location, "{name}: overlapping inline spans");
            }
            for pair in block.children.windows(2) {
                assert!(pair[0].range.upper_bound() <= pair[1].range.location, "{name}: overlapping child blocks");
            }
        });
    }
}

#[test]
fn substring_round_trips_through_block_ranges() {
    let doc = MarkdownParser::parse(corpus::KITCHEN_SINK);
    let heading = doc.root.children.iter().find(|block| block.heading_level() == Some(1));
    assert!(heading.is_some());
    let heading = heading.unwrap();
    assert_eq!(doc.substring(heading.range), "# Release Plan");
    assert_eq!(doc.substring(heading.marker_range.unwrap()), "# ");
    assert_eq!(doc.substring(heading.content_range), "Release Plan");
}

/// §6.1a: `markerRange` is exactly the marker plus its trailing space.
#[test]
fn marker_ranges_cover_the_leading_block_marker() {
    let text = "## Heading\n\n> quoted\n\n- bullet\n\n1. numbered\n\n- [ ] task\n";
    let doc = MarkdownParser::parse(text);
    let mut markers: Vec<String> = Vec::new();
    doc.root.walk(&mut |block| {
        let Some(marker) = block.marker_range else { return };
        match block.content {
            BlockContent::Heading { .. } | BlockContent::BlockQuote | BlockContent::ListItem { .. } => markers.push(doc.substring(marker)),
            _ => {}
        }
    });
    assert_eq!(markers, vec!["## ", "> ", "- ", "1. ", "- [ ] "]);
}

#[test]
fn setext_headings_carry_their_underline_as_trailing_marker() {
    let doc = MarkdownParser::parse("Title\n=====\n\nBody.\n");
    let heading = &doc.root.children[0];
    assert_eq!(heading.heading_level(), Some(1));
    assert!(heading.marker_range.is_none());
    assert_eq!(doc.substring(heading.trailing_marker_range.unwrap()), "=====");
    assert_eq!(doc.substring(heading.content_range), "Title");
}

#[test]
fn closing_hashes_become_a_trailing_marker() {
    let doc = MarkdownParser::parse("## Heading ##\n");
    let heading = &doc.root.children[0];
    assert_eq!(doc.substring(heading.range), "## Heading ##");
    assert_eq!(doc.substring(heading.marker_range.unwrap()), "## ");
    assert_eq!(doc.substring(heading.trailing_marker_range.unwrap()), " ##");
}

#[test]
fn fenced_code_splits_into_fence_content_fence() {
    let doc = MarkdownParser::parse("```swift\nlet x = 1\n```\n");
    let block = &doc.root.children[0];
    let BlockContent::CodeBlock { language, is_fenced, content_range } = &block.content else {
        panic!("expected a code block, got {:?}", block.content);
    };
    assert_eq!(language.as_deref(), Some("swift"));
    assert!(is_fenced);
    assert_eq!(doc.substring(*content_range), "let x = 1\n");
    assert_eq!(doc.substring(block.marker_range.unwrap()), "```swift\n");
    assert_eq!(doc.substring(block.trailing_marker_range.unwrap()), "```");
}

#[test]
fn closing_fence_may_carry_trailing_whitespace() {
    let doc = MarkdownParser::parse("```swift\nlet x = 1\n``` \n");
    let block = &doc.root.children[0];
    let BlockContent::CodeBlock { is_fenced, content_range, .. } = &block.content else {
        panic!("expected a code block, got {:?}", block.content);
    };
    assert!(is_fenced);
    assert_eq!(doc.substring(*content_range), "let x = 1\n");
    assert_eq!(doc.substring(block.trailing_marker_range.unwrap()), "``` ");
}

#[test]
fn short_fence_line_inside_longer_unclosed_fence_is_content() {
    let doc = MarkdownParser::parse("````\ncode\n```\nmore\n");
    let block = &doc.root.children[0];
    let BlockContent::CodeBlock { is_fenced, content_range, .. } = &block.content else {
        panic!("expected a code block, got {:?}", block.content);
    };
    assert!(is_fenced);
    assert_eq!(doc.substring(*content_range), "code\n```\nmore");
    assert!(block.trailing_marker_range.is_none());
}

#[test]
fn indented_code_is_not_fenced() {
    let doc = MarkdownParser::parse("    indented\n    more\n");
    let BlockContent::CodeBlock { is_fenced, .. } = &doc.root.children[0].content else {
        panic!("expected a code block");
    };
    assert!(!is_fenced);
}

#[test]
fn quoted_fenced_code_keeps_its_markers() {
    let doc = MarkdownParser::parse("> ```swift\n> let x = 1\n> ```\n");
    let quote = &doc.root.children[0];
    let block = &quote.children[0];
    let BlockContent::CodeBlock { language, is_fenced, content_range } = &block.content else {
        panic!("expected a code block inside the quote, got {:?}", block.content);
    };
    assert_eq!(language.as_deref(), Some("swift"));
    assert!(is_fenced);
    assert_eq!(doc.substring(block.marker_range.unwrap()), "```swift\n");
    assert_eq!(doc.substring(block.trailing_marker_range.unwrap()), "> ```");
    assert_eq!(doc.substring(*content_range), "> let x = 1\n");
}

#[test]
fn deeply_nested_quoted_fence_still_resolves() {
    let doc = MarkdownParser::parse("> > ```\n> > code\n> > ```\n");
    let outer = &doc.root.children[0];
    let inner = &outer.children[0];
    let block = &inner.children[0];
    let BlockContent::CodeBlock { is_fenced, .. } = &block.content else {
        panic!("expected a code block, got {:?}", block.content);
    };
    assert!(is_fenced);
    assert_eq!(doc.substring(block.trailing_marker_range.unwrap()), "> > ```");
}

#[test]
fn tasks_are_collected_with_single_character_mark_ranges() {
    let doc = MarkdownParser::parse("## Work\n\n- [ ] alpha\n- [x] beta\n  - [ ] nested\n");
    assert_eq!(doc.tasks.len(), 3);
    assert_eq!(doc.tasks.iter().map(|t| t.is_checked).collect::<Vec<_>>(), vec![false, true, false]);
    for task in &doc.tasks {
        assert_eq!(task.mark_range.length, 1);
        assert!(["x", " "].contains(&swift_text::lowercased(&doc.substring(task.mark_range)).as_str()));
    }
    assert_eq!(doc.tasks.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), vec!["alpha", "beta", "nested"]);
    assert!(doc.tasks.iter().all(|t| t.heading_index == Some(0)));
    assert_eq!(doc.tasks[2].indent_level, 1);
}

#[test]
fn outline_links_parents_and_sections() {
    let doc = MarkdownParser::parse("# A\n\ntext\n\n## B\n\nmore\n\n### C\n\ndeep\n\n## D\n\nend\n");
    assert_eq!(doc.headings.iter().map(|h| h.title.as_str()).collect::<Vec<_>>(), vec!["A", "B", "C", "D"]);
    assert_eq!(doc.headings.iter().map(|h| h.level).collect::<Vec<_>>(), vec![1, 2, 3, 2]);
    assert_eq!(doc.headings.iter().map(|h| h.parent_index).collect::<Vec<_>>(), vec![None, Some(0), Some(1), Some(0)]);
    assert_eq!(doc.headings[0].child_indices, vec![1, 3]);
    assert_eq!(doc.substring(doc.headings[1].section_range), "## B\n\nmore\n\n### C\n\ndeep\n\n");
    assert_eq!(doc.headings.iter().map(|h| h.slug.as_str()).collect::<Vec<_>>(), vec!["a", "b", "c", "d"]);
}

#[test]
fn duplicate_headings_get_distinct_slugs() {
    let doc = MarkdownParser::parse("# Setup\n\n## Setup\n\n## Setup\n");
    assert_eq!(doc.headings.iter().map(|h| h.slug.as_str()).collect::<Vec<_>>(), vec!["setup", "setup-1", "setup-2"]);
}

#[test]
fn link_reference_definitions_are_recovered() {
    let doc = MarkdownParser::parse("[ref]: https://example.com \"Title\"\n\nSee [ref].\n");
    assert_eq!(doc.link_references.get("ref").map(|r| r.destination.as_str()), Some("https://example.com"));
    assert_eq!(doc.link_references.get("ref").and_then(|r| r.title.as_deref()), Some("Title"));
}

#[test]
fn footnote_definitions_and_references_survive() {
    let doc = MarkdownParser::parse("Body text.[^1]\n\n[^1]: The note.\n");
    assert!(doc.footnotes.contains_key("1"));
    let mut referenced = false;
    doc.root.walk(&mut |block| {
        for span in &block.inlines {
            span.walk(&mut |inline| {
                if let InlineKind::FootnoteReference { identifier } = &inline.kind
                    && identifier == "1"
                {
                    referenced = true;
                }
            });
        }
    });
    assert!(referenced);
}

/// Regression: a shorter fence run with an info string inside an outer fence
/// is content, not a closer.
#[test]
fn fenced_example_code_with_info_strings_hides_footnote_looking_lines() {
    let doc = MarkdownParser::parse("````markdown\n```ruby\n[^inner]: not a definition\n```\n````\n");
    assert!(!doc.footnotes.contains_key("inner"));
    assert_eq!(doc.root.children.len(), 1, "the whole construct is one code block");
}

/// Regression: a synthesized footnote block must not overlap its sibling.
#[test]
fn synthesized_footnote_block_does_not_overlap_its_sibling() {
    let doc = MarkdownParser::parse("[^a]: first\n    continued\n");
    assert!(doc.footnotes.contains_key("a"));
    let children = &doc.root.children;
    for index in 0..children.len() {
        if index + 1 < children.len() {
            assert!(children[index].range.upper_bound() <= children[index + 1].range.location);
        }
    }
}

#[test]
fn tables_carry_rows_cells_and_alignments() {
    let doc = MarkdownParser::parse("| a | b | c |\n|:--|:-:|--:|\n| 1 | 2 | 3 |\n");
    let BlockContent::Table(table) = &doc.root.children[0].content else { panic!("expected a table") };
    assert_eq!(table.alignments, vec![TableAlignment::Left, TableAlignment::Center, TableAlignment::Right]);
    assert_eq!(table.rows.len(), 2);
    assert!(table.header_row().is_some());
    assert_eq!(table.column_count(), 3);
    assert_eq!(doc.substring(table.delimiter_range), "|:--|:-:|--:|");
    assert_eq!(doc.substring(table.rows[0].cells[1].content_range), "b");
}

#[test]
fn block_quote_depth_is_tracked() {
    let doc = MarkdownParser::parse("> outer\n>\n> > inner\n");
    let mut depths: Vec<isize> = Vec::new();
    doc.root.walk(&mut |block| {
        if matches!(block.content, BlockContent::Paragraph) {
            depths.push(block.quote_depth);
        }
    });
    assert!(depths.contains(&1));
    assert!(depths.contains(&2));
}

#[test]
fn empty_document_is_safe() {
    let doc = MarkdownParser::parse("");
    assert_eq!(doc.length, 0);
    assert!(doc.headings.is_empty());
    assert!(doc.root.children.is_empty());
}

#[test]
fn smart_punctuation_is_never_applied() {
    let text = "He said \"hello\" -- really.\n";
    let doc = MarkdownParser::parse(text);
    assert_eq!(doc.text, text);
    let paragraph = &doc.root.children[0];
    assert!(swift_text::contains(&doc.substring(paragraph.content_range), "\"hello\""));
    assert!(swift_text::contains(&doc.substring(paragraph.content_range), "--"));
}

#[test]
fn inline_markers_are_exactly_the_delimiters() {
    let doc = MarkdownParser::parse("a **bold** and _em_ and ~~x~~ and `c` here\n");
    let mut pairs: Vec<String> = Vec::new();
    for span in &doc.root.children[0].inlines {
        span.walk(&mut |inline| {
            if !inline.kind.reveals_markers() {
                return;
            }
            for marker in inline.marker_ranges() {
                pairs.push(doc.substring(marker));
            }
        });
    }
    assert_eq!(pairs, vec!["**", "**", "_", "_", "~~", "~~", "`", "`"]);
}
