//! Tests ported from swift-markdown's own suite (`vendor/swift-markdown/Tests/
//! MarkdownTests`) wherever they exercise parsing, conversion, ranges or the
//! read-only accessors, plus cases for the converter details Downright relies
//! on. Expected dumps are swift-markdown's `debugDescription(options:
//! .printSourceLocations)` text; where a Swift test used the construction or
//! editing API (not ported), the parsed equivalent is checked instead.

use crate::base::raw_markup::{RawMarkupArena, RawMarkupData};
use crate::utility::swift_string::{swift_contains_character, swift_string_eq};
use crate::{
    Checkbox, ColumnAlignment, Document, MarkupData, ParseOptions, SourceLocation, SourceRange,
};

fn dump(source: &str, options: ParseOptions) -> String {
    Document::parse(source, options)
        .root()
        .debug_description(true)
}

fn dump_default(source: &str) -> String {
    dump(source, ParseOptions::EMPTY)
}

// MARK: - CommonMarkConverterTests

/// `testMulitlineLinks`: a link spanning lines does not crash cmark and
/// gets a valid range. (The Swift test also turns on block directives, which
/// `testMultilineLinksWithBlockDirectives` shows leaves the tree unchanged.)
#[test]
fn multiline_links() {
    let text = "This is a link to an article on a different domain [link\nto an article](https://www.host.com/article).";
    let expected = r#"Document @1:1-2:46
└─ Paragraph @1:1-2:46
   ├─ Text @1:1-1:52 "This is a link to an article on a different domain "
   ├─ Link @1:52-2:45 destination: "https://www.host.com/article"
   │  ├─ Text @1:53-1:57 "link"
   │  ├─ SoftBreak
   │  └─ Text @2:1-2:14 "to an article"
   └─ Text @2:45-2:46 ".""#;
    assert_eq!(dump(text, ParseOptions::PARSE_SYMBOL_LINKS), expected);
}

/// `testNestedStructureRanges`.
#[test]
fn nested_structure_ranges() {
    let text = "> Blockquote\n> - List item\n>   - Nested list item\n>     1. Deepest item";
    let expected = r#"Document @1:1-4:22
└─ BlockQuote @1:1-4:22
   ├─ Paragraph @1:3-1:13
   │  └─ Text @1:3-1:13 "Blockquote"
   └─ UnorderedList @2:3-4:22
      └─ ListItem @2:3-4:22
         ├─ Paragraph @2:5-2:14
         │  └─ Text @2:5-2:14 "List item"
         └─ UnorderedList @3:5-4:22
            └─ ListItem @3:5-4:22
               ├─ Paragraph @3:7-3:23
               │  └─ Text @3:7-3:23 "Nested list item"
               └─ OrderedList @4:7-4:22
                  └─ ListItem @4:7-4:22
                     └─ Paragraph @4:10-4:22
                        └─ Text @4:10-4:22 "Deepest item""#;
    assert_eq!(dump_default(text), expected);
}

/// `testTrulyDeepNestingStackUnwind`: extremely deep nesting converts
/// without recursion.
#[test]
fn truly_deep_nesting_stack_unwind() {
    let depth = 15_000;
    let text = "> ".repeat(depth) + "Deep";
    let document = Document::parse(&text, ParseOptions::EMPTY);
    let mut current = document.root();
    let mut actual_depth = 0;
    while let Some(child) = current.child(0) {
        current = child;
        actual_depth += 1;
    }
    assert!(actual_depth > depth);
    assert_eq!(current.data(), MarkupData::Text { string: "Deep" });
    assert!(current.plain_text().is_some());
}

/// `testEmptyDocument`.
#[test]
fn empty_document() {
    assert_eq!(dump_default(""), "Document");
    assert_eq!(Document::parse("", ParseOptions::EMPTY).child_count(), 0);
}

// MARK: - MarkupTreeDumperTests

/// `testDumpEverything`, without the unique identifiers (not ported).
#[test]
fn dump_everything() {
    let everything =
        include_str!("../../../vendor/swift-markdown/Tests/MarkdownTests/Visitors/Everything.md");
    let expected = r#"Document @1:1-45:90
├─ Heading @1:1-1:9 level: 1
│  └─ Text @1:3-1:9 "Header"
├─ Paragraph @3:1-3:65
│  ├─ Emphasis @3:1-3:13
│  │  └─ Text @3:2-3:12 "Emphasized"
│  ├─ Text @3:13-3:14 " "
│  ├─ Strong @3:14-3:24
│  │  └─ Text @3:16-3:22 "strong"
│  ├─ Text @3:24-3:25 " "
│  ├─ InlineCode @3:25-3:38 `inline code`
│  ├─ Text @3:38-3:39 " "
│  ├─ Link @3:39-3:50 destination: "foo"
│  │  └─ Text @3:40-3:44 "link"
│  ├─ Text @3:50-3:51 " "
│  ├─ Image @3:51-3:64 source: "foo"
│  │  └─ Text @3:53-3:58 "image"
│  └─ Text @3:64-3:65 "."
├─ UnorderedList @5:1-9:1
│  ├─ ListItem @5:1-5:7
│  │  └─ Paragraph @5:3-5:7
│  │     └─ Text @5:3-5:7 "this"
│  ├─ ListItem @6:1-6:5
│  │  └─ Paragraph @6:3-6:5
│  │     └─ Text @6:3-6:5 "is"
│  ├─ ListItem @7:1-7:4
│  │  └─ Paragraph @7:3-7:4
│  │     └─ Text @7:3-7:4 "a"
│  └─ ListItem @8:1-9:1
│     └─ Paragraph @8:3-8:7
│        └─ Text @8:3-8:7 "list"
├─ OrderedList @10:1-12:1
│  ├─ ListItem @10:1-10:8
│  │  └─ Paragraph @10:4-10:8
│  │     └─ Text @10:4-10:8 "eggs"
│  └─ ListItem @11:1-12:1
│     └─ Paragraph @11:4-11:8
│        └─ Text @11:4-11:8 "milk"
├─ BlockQuote @13:1-13:13
│  └─ Paragraph @13:3-13:13
│     └─ Text @13:3-13:13 "BlockQuote"
├─ OrderedList @15:1-17:1 startIndex: 2
│  ├─ ListItem @15:1-15:9
│  │  └─ Paragraph @15:4-15:9
│  │     └─ Text @15:4-15:9 "flour"
│  └─ ListItem @16:1-17:1
│     └─ Paragraph @16:4-16:9
│        └─ Text @16:4-16:9 "sugar"
├─ UnorderedList @18:1-20:1
│  ├─ ListItem @18:1-18:37 checkbox: [x]
│  │  └─ Paragraph @18:7-18:37
│  │     └─ Text @18:7-18:37 "Combine flour and baking soda."
│  └─ ListItem @19:1-20:1 checkbox: [ ]
│     └─ Paragraph @19:7-19:30
│        └─ Text @19:7-19:30 "Combine sugar and eggs."
├─ CodeBlock @21:1-25:4 language: swift
│  func foo() {
│      let x = 1
│  }
├─ CodeBlock @27:5-28:1 language: none
│  // Is this real code? Or just fantasy?
├─ Paragraph @29:1-29:31
│  ├─ Text @29:1-29:12 "This is an "
│  ├─ Link @29:12-29:30 destination: "topic://autolink"
│  │  └─ Text @29:13-29:29 "topic://autolink"
│  └─ Text @29:30-29:31 "."
├─ ThematicBreak @31:1-32:1
├─ HTMLBlock @33:1-35:5
│  <a href="foo.png">
│  An HTML Block.
│  </a>
├─ Paragraph @37:1-37:33
│  ├─ Text @37:1-37:14 "This is some "
│  ├─ InlineHTML @37:14-37:17 <p>
│  ├─ Text @37:17-37:28 "inline html"
│  ├─ InlineHTML @37:28-37:32 </p>
│  └─ Text @37:32-37:33 "."
├─ Paragraph @39:1-40:6
│  ├─ Text @39:1-39:7 "line"
│  ├─ LineBreak
│  └─ Text @40:1-40:6 "break"
├─ Paragraph @42:1-43:6
│  ├─ Text @42:1-42:5 "soft"
│  ├─ SoftBreak
│  └─ Text @43:1-43:6 "break"
└─ HTMLBlock @45:1-45:90
   <!-- Copyright (c) 2021 Apple Inc and the Swift Project authors. All Rights Reserved. -->"#;
    assert_eq!(dump_default(everything), expected);

    // The tree without locations (`debugDescription()`).
    let bare = Document::parse(everything, ParseOptions::EMPTY)
        .root()
        .debug_description(false);
    assert!(bare.starts_with("Document\n├─ Heading level: 1\n│  └─ Text \"Header\"\n"));
}

// MARK: - BacktickTests

#[test]
fn normal_backticks() {
    let expected = r#"Document @1:1-1:20
└─ Paragraph @1:1-1:20
   ├─ Text @1:1-1:7 "Hello "
   ├─ InlineCode @1:7-1:13 `test`
   └─ Text @1:13-1:20 " String""#;
    assert_eq!(dump_default("Hello `test` String"), expected);
}

#[test]
fn open_backtick() {
    let expected = "Document @1:1-1:2\n└─ Paragraph @1:1-1:2\n   └─ Text @1:1-1:2 \"`\"";
    assert_eq!(dump_default("`"), expected);
}

#[test]
fn open_backticks() {
    let expected = "Document @1:1-1:3\n└─ Paragraph @1:1-1:3\n   └─ Text @1:1-1:3 \"``\"";
    assert_eq!(dump_default("``"), expected);
}

#[test]
fn backtick_in_code_voice_with_double_backtick_delimiters() {
    let expected = r#"Document @1:1-1:23
└─ Paragraph @1:1-1:23
   ├─ Text @1:1-1:5 "Use "
   ├─ InlineCode @1:5-1:12 ```
   └─ Text @1:12-1:23 " to delimit""#;
    assert_eq!(
        dump("Use `` ` `` to delimit", ParseOptions::PARSE_SYMBOL_LINKS),
        expected
    );
}

#[test]
fn backtick_in_code_voice_with_triple_backtick_delimiters() {
    let expected = r#"Document @1:1-1:25
└─ Paragraph @1:1-1:25
   ├─ Text @1:1-1:5 "Use "
   ├─ InlineCode @1:5-1:14 ```
   └─ Text @1:14-1:25 " to delimit""#;
    assert_eq!(
        dump("Use ``` ` ``` to delimit", ParseOptions::PARSE_SYMBOL_LINKS),
        expected
    );
}

#[test]
fn double_backtick_symbol_link_still_works() {
    let expected = r#"Document @1:1-1:26
└─ Paragraph @1:1-1:26
   ├─ Text @1:1-1:5 "See "
   ├─ SymbolLink @1:5-1:14 destination: foo()
   └─ Text @1:14-1:26 " for details""#;
    assert_eq!(
        dump(
            "See ``foo()`` for details",
            ParseOptions::PARSE_SYMBOL_LINKS
        ),
        expected
    );
}

#[test]
fn multiple_backticks_in_code_voice() {
    let expected = r#"Document @1:1-1:23
└─ Paragraph @1:1-1:23
   ├─ Text @1:1-1:5 "Use "
   ├─ InlineCode @1:5-1:15 ````
   └─ Text @1:15-1:23 " in code""#;
    assert_eq!(
        dump("Use ``` `` ``` in code", ParseOptions::PARSE_SYMBOL_LINKS),
        expected
    );
}

/// A backtick followed by a combining mark is a different Swift `Character`,
/// so `literalContent.contains("`")` is false and the span becomes a symbol
/// link (checked against Swift: `"a`\u{301}b".contains("`") == false`).
#[test]
fn symbol_link_contains_uses_characters() {
    let document = Document::parse("``a`\u{301}b``", ParseOptions::PARSE_SYMBOL_LINKS);
    let span = document.child(0).unwrap().child(0).unwrap();
    assert_eq!(
        span.data(),
        MarkupData::SymbolLink {
            destination: Some("a`\u{301}b")
        }
    );
    assert_eq!(span.plain_text().as_deref(), Some("``a`\u{301}b``"));

    assert!(!swift_contains_character("a`\u{301}b", '`'));
    assert!(swift_contains_character("a`b", '`'));
    assert!(!swift_contains_character("\u{0600}`", '`'));
    assert!(!swift_contains_character("`\u{200D}", '`'));
}

// MARK: - SymbolLinkTests

#[test]
fn symbol_link_detection_from_inline_code() {
    let on =
        "Document @1:1-1:10\n└─ Paragraph @1:1-1:10\n   └─ SymbolLink @1:1-1:10 destination: foo()";
    assert_eq!(dump("``foo()``", ParseOptions::PARSE_SYMBOL_LINKS), on);
    let off = "Document @1:1-1:10\n└─ Paragraph @1:1-1:10\n   └─ InlineCode @1:1-1:10 `foo()`";
    assert_eq!(dump_default("``foo()``"), off);
}

#[test]
fn multiline_symbol_link() {
    let expected = r#"Document @1:1-2:17
└─ Paragraph @1:1-2:17
   ├─ Text @1:1-1:11 "Test of a "
   └─ SymbolLink @1:11-2:17 destination: multi line symbolink"#;
    assert_eq!(
        dump(
            "Test of a ``multi\nline symbolink``",
            ParseOptions::PARSE_SYMBOL_LINKS
        ),
        expected
    );
}

// MARK: - SourceLocationTests

/// `testNonAsciiCharacterColumn`: columns count UTF-8 bytes.
#[test]
fn non_ascii_character_column() {
    for text in ["🇺🇳", "叶"] {
        let range = Document::parse(text, ParseOptions::EMPTY).range().unwrap();
        assert_eq!(range.upper_bound.column - 1, text.len() as i64);
    }
}

#[test]
fn source_range_description() {
    let range = SourceRange::new(SourceLocation::new(1, 2), SourceLocation::new(3, 4));
    assert_eq!(range.diagnostic_description(), "1:2-3:4");
    let empty = SourceRange::new(SourceLocation::new(1, 2), SourceLocation::new(1, 2));
    assert_eq!(empty.diagnostic_description(), "1:2");
    assert!(SourceLocation::new(1, 9) < SourceLocation::new(2, 1));
}

// MARK: - TableTests

#[test]
fn table_parse() {
    let source = "|x|y|\n|-|-|\n|1|2|\n|3|4|";
    let expected = r#"Document @1:1-4:6
└─ Table @1:1-4:6 alignments: |-|-|
   ├─ Head @1:1-1:6
   │  ├─ Cell @1:2-1:3
   │  │  └─ Text @1:2-1:3 "x"
   │  └─ Cell @1:4-1:5
   │     └─ Text @1:4-1:5 "y"
   └─ Body @3:1-4:6
      ├─ Row @3:1-3:6
      │  ├─ Cell @3:2-3:3
      │  │  └─ Text @3:2-3:3 "1"
      │  └─ Cell @3:4-3:5
      │     └─ Text @3:4-3:5 "2"
      └─ Row @4:1-4:6
         ├─ Cell @4:2-4:3
         │  └─ Text @4:2-4:3 "3"
         └─ Cell @4:4-4:5
            └─ Text @4:4-4:5 "4""#;
    assert_eq!(dump_default(source), expected);
}

#[test]
fn table_parse_cell_spans() {
    let source = "| one | two | three |\n| --- | --- | ----- |\n| big      || small |\n| ^        || small |";
    let expected = r#"Document @1:1-4:22
└─ Table @1:1-4:22 alignments: |-|-|-|
   ├─ Head @1:1-1:22
   │  ├─ Cell @1:2-1:7
   │  │  └─ Text @1:3-1:6 "one"
   │  ├─ Cell @1:8-1:13
   │  │  └─ Text @1:9-1:12 "two"
   │  └─ Cell @1:14-1:21
   │     └─ Text @1:15-1:20 "three"
   └─ Body @3:1-4:22
      ├─ Row @3:1-3:22
      │  ├─ Cell @3:2-3:12 colspan: 2 rowspan: 2
      │  │  └─ Text @3:3-3:6 "big"
      │  ├─ Cell @3:13-3:14 colspan: 0
      │  └─ Cell @3:14-3:21
      │     └─ Text @3:15-3:20 "small"
      └─ Row @4:1-4:22
         ├─ Cell @4:2-4:12 colspan: 2 rowspan: 0
         ├─ Cell @4:13-4:14 colspan: 0
         └─ Cell @4:14-4:21
            └─ Text @4:15-4:20 "small""#;
    assert_eq!(dump_default(source), expected);
}

/// A header-only table (the parsed half of `testSetBody`): the body exists,
/// is empty and has no range.
#[test]
fn table_without_body_rows() {
    let document = Document::parse("|x|y|z|\n|-|-|-|", ParseOptions::EMPTY);
    let table = document.child(0).unwrap();
    assert_eq!(table.child_count(), 2);
    let head = table.table_head().unwrap();
    let body = table.table_body().unwrap();
    assert_eq!(head.data(), MarkupData::TableHead);
    assert_eq!(head.child_count(), 3);
    assert_eq!(body.data(), MarkupData::TableBody);
    assert!(body.is_empty());
    assert_eq!(body.range(), None);
    assert_eq!(table.max_column_count(), Some(3));
}

#[test]
fn table_alignments_and_cells() {
    let source = "| a | b | c | d |\n|:--|:-:|--:|---|\n| 1 | 2 |\n";
    let document = Document::parse(source, ParseOptions::DISABLE_SMART_OPTS);
    let table = document.child(0).unwrap();
    let MarkupData::Table { column_alignments } = table.data() else {
        panic!("not a table")
    };
    assert_eq!(
        column_alignments,
        &[
            Some(ColumnAlignment::Left),
            Some(ColumnAlignment::Center),
            Some(ColumnAlignment::Right),
            None
        ]
    );
    // cmark pads a short row with empty cells.
    let row = table.table_body().unwrap().child(0).unwrap();
    assert_eq!(row.child_count(), 4);
    for cell in row.children() {
        assert_eq!(
            cell.data(),
            MarkupData::TableCell {
                colspan: 1,
                rowspan: 1
            }
        );
    }
    // The body range runs from its first row to the end of the table.
    assert_eq!(
        table.table_body().unwrap().range(),
        Some(SourceRange::new(
            SourceLocation::new(3, 1),
            table.range().unwrap().upper_bound
        ))
    );
}

/// `RawMarkup.table` pads alignments to the widest row, but measures the
/// body through `RawMarkup.children`, which yields `child(at: 0)` for every
/// index: every row counts as the first row's width. cmark never produces
/// ragged rows, so only a hand-built tree shows it; `Table.maxColumnCount`
/// walks the real children.
#[test]
fn table_alignment_padding_measures_first_row() {
    let mut arena = RawMarkupArena::default();
    let cell = |arena: &mut RawMarkupArena| {
        arena.create(
            RawMarkupData::TableCell {
                colspan: 1,
                rowspan: 1,
            },
            None,
            &[],
        )
    };
    let head_cell = cell(&mut arena);
    let header = arena.table_head(None, &[head_cell]);
    let narrow_cell = cell(&mut arena);
    let narrow = arena.table_row(None, &[narrow_cell]);
    let wide_cells = [cell(&mut arena), cell(&mut arena), cell(&mut arena)];
    let wide = arena.table_row(None, &wide_cells);
    let body = arena.table_body(None, &[narrow, wide]);
    let table = arena.table(&[Some(ColumnAlignment::Left)], None, header, body);
    let root = arena.create(RawMarkupData::Document, None, &[table]);
    let document = Document { arena, root };
    let table = document.child(0).unwrap();
    let MarkupData::Table { column_alignments } = table.data() else {
        panic!("not a table")
    };
    assert_eq!(column_alignments, &[Some(ColumnAlignment::Left)]);
    assert_eq!(table.max_column_count(), Some(3));
}

// MARK: - InlineAttributesTests

#[test]
fn parse_inline_attributes() {
    let expected = r#"Document @1:1-1:37
└─ Paragraph @1:1-1:37
   └─ InlineAttributes @1:1-1:37 attributes: `rainbow: 'extreme'`
      └─ Text @1:3-1:16 "Hello, world!""#;
    assert_eq!(
        dump_default("^[Hello, world!](rainbow: 'extreme')"),
        expected
    );
}

// MARK: - LineBreakTests

#[test]
fn parse_line_break() {
    for source in [
        "Paragraph.  \nStill the same paragraph.",
        "Paragraph.\\\nStill the same paragraph.",
    ] {
        let document = Document::parse(source, ParseOptions::EMPTY);
        let paragraph = document.child(0).unwrap();
        assert_eq!(paragraph.child(1).unwrap().data(), MarkupData::LineBreak);
        assert_eq!(paragraph.child(1).unwrap().range(), None);
    }
    let document = Document::parse("Paragraph.\nSame line text.", ParseOptions::EMPTY);
    assert_eq!(
        document.child(0).unwrap().child(1).unwrap().data(),
        MarkupData::SoftBreak
    );
}

// MARK: - LinkTests, ImageTests

/// `testTitleLink`.
#[test]
fn title_link() {
    let markdown =
        "[Example](example.com \"The example title\")\n[Example2](example2.com)\n[Example3]()";
    let document = Document::parse(markdown, ParseOptions::EMPTY);
    assert_eq!(document.child_count(), 1);
    let paragraph = document.child(0).unwrap();
    assert_eq!(paragraph.child_count(), 5);
    assert_eq!(paragraph.child(1).unwrap().data(), MarkupData::SoftBreak);
    assert_eq!(paragraph.child(3).unwrap().data(), MarkupData::SoftBreak);

    let with_title = paragraph.child(0).unwrap();
    assert_eq!(
        with_title.child(0).unwrap().data(),
        MarkupData::Text { string: "Example" }
    );
    assert_eq!(
        with_title.data(),
        MarkupData::Link {
            destination: Some("example.com"),
            title: Some("The example title")
        }
    );
    let without_title = paragraph.child(2).unwrap();
    assert_eq!(
        without_title.data(),
        MarkupData::Link {
            destination: Some("example2.com"),
            title: None
        }
    );
    let without_destination = paragraph.child(4).unwrap();
    assert_eq!(
        without_destination.data(),
        MarkupData::Link {
            destination: None,
            title: None
        }
    );
    assert_eq!(without_destination.is_autolink(), Some(false));
}

#[test]
fn autolinks() {
    let document = Document::parse(
        "<http://example.com> [http://example.com](http://example.com) [x](http://example.com) [caf\u{e9}](cafe\u{301})",
        ParseOptions::EMPTY,
    );
    let paragraph = document.child(0).unwrap();
    let links: Vec<_> = paragraph
        .children()
        .filter(|child| matches!(child.data(), MarkupData::Link { .. }))
        .collect();
    let autolink: Vec<_> = links
        .iter()
        .map(|link| link.is_autolink().unwrap())
        .collect();
    // Swift compares the text and destination with `==`, which is canonical
    // equivalence, so the precomposed and decomposed spellings match.
    assert_eq!(autolink, [true, true, false, true]);
    assert_eq!(paragraph.is_autolink(), None);
    assert!(swift_string_eq("caf\u{e9}", "cafe\u{301}"));
    assert!(!swift_string_eq("cafe", "caf\u{e9}"));
}

/// `testImageTitle` (parsed half).
#[test]
fn image_title() {
    let document = Document::parse("![Alt](test.png \"title\")", ParseOptions::EMPTY);
    let image = document.root().child_through([0, 0]).unwrap();
    assert_eq!(
        image.data(),
        MarkupData::Image {
            source: Some("test.png"),
            title: Some("title")
        }
    );
    assert_eq!(image.plain_text().as_deref(), Some("Alt"));
}

// MARK: - HierarchyTests

/// `testRoot`.
#[test]
fn root() {
    let document = Document::parse("*OK*", ParseOptions::EMPTY);
    let leaf = document.root().child_through([0, 0, 0]).unwrap();
    assert_eq!(leaf.data(), MarkupData::Text { string: "OK" });
    assert_eq!(leaf.root(), document.root());
    assert_eq!(document.root().root(), document.root());
    assert_eq!(document.root().parent(), None);
    assert_eq!(leaf.parent().unwrap().data(), MarkupData::Emphasis);
}

#[test]
fn children_and_indices() {
    let document = Document::parse("a\n\n- b\n- c\n\n> d\n", ParseOptions::EMPTY);
    let kinds: Vec<_> = document
        .children()
        .map(|child| child.data().type_name())
        .collect();
    assert_eq!(kinds, ["Paragraph", "UnorderedList", "BlockQuote"]);
    for (index, child) in document.children().enumerate() {
        assert_eq!(child.index_in_parent(), index);
        assert_eq!(child.parent(), Some(document.root()));
    }
    let reversed: Vec<_> = document
        .children()
        .rev()
        .map(|child| child.index_in_parent())
        .collect();
    assert_eq!(reversed, [2, 1, 0]);
    assert_eq!(document.children().len(), 3);
    assert_eq!(document.child(3), None);
    assert_eq!(document.root().index_in_parent(), 0);
}

// MARK: - SourceURLTests

/// `testParseString_SmartOpts`.
#[test]
fn smart_opts() {
    let text = "The iPod (2001--2022) changed the way people listened and interacted with music---it'll forever be in our hearts!";
    let enabled = Document::parse(text, ParseOptions::EMPTY)
        .root()
        .debug_description(false);
    assert_eq!(
        enabled,
        "Document\n└─ Paragraph\n   └─ Text \"The iPod (2001–2022) changed the way people listened and interacted with music—it’ll forever be in our hearts!\""
    );
    let disabled = Document::parse(text, ParseOptions::DISABLE_SMART_OPTS)
        .root()
        .debug_description(false);
    assert_eq!(
        disabled,
        format!("Document\n└─ Paragraph\n   └─ Text \"{text}\"")
    );
}

// MARK: - PlainTextConvertibleMarkupTests (on parsed elements)

#[test]
fn plain_text() {
    let document = Document::parse(
        "This is a *paragraph* with **strong**, ~~struck~~, `code`, <br />, [a link](u), ![an *image*](i) and ^[attrs](x: 1).\nsoft  \nhard",
        ParseOptions::DISABLE_SMART_OPTS,
    );
    let paragraph = document.child(0).unwrap();
    assert_eq!(
        paragraph.plain_text().as_deref(),
        Some(
            "This is a paragraph with strong, ~struck~, `code`, <br />, a link, an image and attrs. soft\nhard"
        )
    );
    let texts: Vec<_> = paragraph
        .children()
        .map(|child| child.plain_text().unwrap())
        .collect();
    assert_eq!(texts[1], "paragraph");
    assert_eq!(texts[5], "~struck~");
    assert_eq!(document.root().plain_text(), None);

    let heading = Document::parse("# A `b` *c*", ParseOptions::EMPTY);
    assert_eq!(
        heading.child(0).unwrap().plain_text().as_deref(),
        Some("A `b` c")
    );
    let list = Document::parse("- x", ParseOptions::EMPTY);
    assert_eq!(list.child(0).unwrap().plain_text(), None);
    assert_eq!(list.child(0).unwrap().child(0).unwrap().plain_text(), None);
}

// MARK: - Converter details Downright depends on

#[test]
fn block_properties() {
    let source = "## Two\n\n3. x\n4. y\n\n- [x] done\n- [ ] todo\n- plain\n\n```rust  \ncode\n```\n\n    indented\n\n<div>\nhi\n</div>\n\n***\n";
    let document = Document::parse(source, ParseOptions::DISABLE_SMART_OPTS);
    let blocks: Vec<_> = document.children().collect();
    assert_eq!(blocks[0].data(), MarkupData::Heading { level: 2 });
    assert_eq!(blocks[1].data(), MarkupData::OrderedList { start_index: 3 });
    let items: Vec<_> = blocks[2].children().map(|item| item.data()).collect();
    assert_eq!(
        items,
        [
            MarkupData::ListItem {
                checkbox: Some(Checkbox::Checked)
            },
            MarkupData::ListItem {
                checkbox: Some(Checkbox::Unchecked)
            },
            MarkupData::ListItem { checkbox: None },
        ]
    );
    // cmark trims the fence info string.
    assert_eq!(
        blocks[3].data(),
        MarkupData::CodeBlock {
            code: "code\n",
            language: Some("rust")
        }
    );
    assert_eq!(
        blocks[4].data(),
        MarkupData::CodeBlock {
            code: "indented\n",
            language: None
        }
    );
    assert_eq!(
        blocks[5].data(),
        MarkupData::HtmlBlock {
            raw_html: "<div>\nhi\n</div>\n"
        }
    );
    assert_eq!(blocks[6].data(), MarkupData::ThematicBreak);
}

/// Code spans' ranges include their backticks; soft and hard breaks have no
/// range; columns are UTF-8 bytes.
#[test]
fn inline_ranges() {
    let document = Document::parse("é ``x`` y\nz", ParseOptions::DISABLE_SMART_OPTS);
    let paragraph = document.child(0).unwrap();
    let ranges: Vec<_> = paragraph
        .children()
        .map(|child| child.range().map(|range| range.diagnostic_description()))
        .collect();
    assert_eq!(
        ranges,
        [
            Some("1:1-1:4".to_owned()),
            Some("1:4-1:9".to_owned()),
            Some("1:9-1:11".to_owned()),
            None,
            Some("2:1-2:2".to_owned())
        ]
    );
    assert_eq!(
        paragraph.child(1).unwrap().data(),
        MarkupData::InlineCode { code: "x" }
    );
}

/// NUL becomes U+FFFD inside cmark; CRLF and lone CR end lines.
#[test]
fn nul_and_line_endings() {
    let document = Document::parse("a\0b\r\nc\rd", ParseOptions::DISABLE_SMART_OPTS);
    let paragraph = document.child(0).unwrap();
    let parts: Vec<_> = paragraph.children().map(|child| child.data()).collect();
    assert_eq!(
        parts,
        [
            MarkupData::Text {
                string: "a\u{FFFD}b"
            },
            MarkupData::SoftBreak,
            MarkupData::Text { string: "c" },
            MarkupData::SoftBreak,
            MarkupData::Text { string: "d" },
        ]
    );
    assert_eq!(
        paragraph.range().unwrap().diagnostic_description(),
        "1:1-3:2"
    );
}

#[test]
fn documents_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Document>();
}
