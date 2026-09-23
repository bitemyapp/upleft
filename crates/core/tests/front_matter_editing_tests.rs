//! FrontMatterEditingTests.swift, plus differential cases on hand-built
//! documents.

use std::collections::HashMap;

use upleft_core::editing::front_matter_editing::{FrontMatterEditOperation, FrontMatterEditing, FrontMatterSourceFallback, FrontMatterValue};
use upleft_core::parser::MarkdownParser;
use upleft_core::source_positions::SourceMap;
use upleft_core::swift_text;
use upleft_core::{BlockContent, FrontMatter, FrontMatterField, MDBlock, NSRange, ParsedDocument};

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn set_preserves_quote_and_boundary_whitespace() {
    let source = "---\ntitle:  \"Old\"  \ncount: 2\n---\nBody\n";
    let result = FrontMatterEditing::set(&MarkdownParser::parse(source), "title", FrontMatterValue::Text("New".into()));
    let proposal = result.proposal.expect("a proposal");
    assert_eq!(proposal.applying(source).as_deref(), Some("---\ntitle:  \"New\"  \ncount: 2\n---\nBody\n"));
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn typed_values_round_trip_without_changing_other_fields() {
    let source = "---\ntitle: Demo\n---\n";
    let document = MarkdownParser::parse(source);
    let boolean = FrontMatterEditing::set(&document, "enabled", FrontMatterValue::Boolean(true)).proposal.expect("a proposal");
    let with_bool = boolean.applying(source).unwrap_or_else(|| source.to_owned());
    let list = FrontMatterEditing::add(&MarkdownParser::parse(&with_bool), "tags", FrontMatterValue::List(vec!["one".into(), "two".into()]))
        .proposal
        .expect("a proposal");
    let output = list.applying(&with_bool).expect("applies");
    assert_eq!(output, "---\ntitle: Demo\nenabled: true\ntags: [one, two]\n---\n");
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn add_and_remove_keep_crlf() {
    let source = "---\r\ntitle: Demo\r\n---\r\n";
    let added = FrontMatterEditing::add(&MarkdownParser::parse(source), "draft", FrontMatterValue::Boolean(false)).proposal.expect("a proposal");
    let with_field = added.applying(source).expect("applies");
    assert_eq!(with_field, "---\r\ntitle: Demo\r\ndraft: false\r\n---\r\n");
    let removed = FrontMatterEditing::remove(&MarkdownParser::parse(&with_field), "title").proposal.expect("a proposal");
    assert_eq!(removed.applying(&with_field).as_deref(), Some("---\r\ndraft: false\r\n---\r\n"));
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn complex_yaml_falls_back_to_source() {
    let nested = FrontMatterEditing::set(&MarkdownParser::parse("---\ntags:\n  - one\n---\n"), "tags", FrontMatterValue::List(vec!["two".into()]));
    assert!(nested.proposal.is_none());
    assert_eq!(nested.fallback, Some(FrontMatterSourceFallback::NestedYAML));

    let comment = FrontMatterEditing::set(&MarkdownParser::parse("---\n# note\ntitle: Demo\n---\n"), "title", FrontMatterValue::Text("x".into()));
    assert_eq!(comment.fallback, Some(FrontMatterSourceFallback::CommentsNotSupported));
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn stale_proposal_does_not_apply() {
    let source = "---\ntitle: Demo\n---\n";
    let proposal = FrontMatterEditing::set(&MarkdownParser::parse(source), "title", FrontMatterValue::Text("New".into())).proposal.expect("a proposal");
    assert_eq!(proposal.applying(&swift_text::replacing_occurrences(source, "Demo", "Other")), None);
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn empty_field_can_be_set_without_adding_a_duplicate() {
    let source = "---\ntitle: \n---\n";
    let proposal = FrontMatterEditing::set(&MarkdownParser::parse(source), "title", FrontMatterValue::Text("Demo".into())).proposal.expect("a proposal");
    assert_eq!(proposal.applying(source).as_deref(), Some("---\ntitle: Demo\n---\n"));
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn special_text_uses_safe_yaml_quotes() {
    let source = "---\ntitle: Demo\n---\n";
    let proposal = FrontMatterEditing::set(&MarkdownParser::parse(source), "title", FrontMatterValue::Text("true: \"quoted\" # note".into()))
        .proposal
        .expect("a proposal");
    assert_eq!(proposal.applying(source).as_deref(), Some("---\ntitle: \"true: \\\"quoted\\\" # note\"\n---\n"));
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn list_items_use_type_safe_quotes() {
    let source = "---\ntags: [one, two]\n---\n";
    let proposal = FrontMatterEditing::set(
        &MarkdownParser::parse(source),
        "tags",
        FrontMatterValue::List(vec!["true".into(), "a: b".into(), "plain".into()]),
    )
    .proposal
    .expect("a proposal");
    assert_eq!(proposal.applying(source).as_deref(), Some("---\ntags: [\"true\", \"a: b\", plain]\n---\n"));
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn duplicate_and_invalid_keys_fall_back() {
    let duplicate = FrontMatterEditing::set(&MarkdownParser::parse("---\ntitle: A\ntitle: B\n---\n"), "title", FrontMatterValue::Text("C".into()));
    assert_eq!(duplicate.fallback, Some(FrontMatterSourceFallback::AmbiguousField));
    let invalid = FrontMatterEditing::add(&MarkdownParser::parse("---\ntitle: A\n---\n"), "bad:key", FrontMatterValue::Text("x".into()));
    assert_eq!(invalid.fallback, Some(FrontMatterSourceFallback::UnsupportedValue));
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn malformed_anchored_and_block_scalar_yaml_falls_back() {
    let malformed = FrontMatterEditing::set(&MarkdownParser::parse("---\ntitle: A\n"), "title", FrontMatterValue::Text("B".into()));
    assert_eq!(malformed.fallback, Some(FrontMatterSourceFallback::MalformedFence));
    let anchor = FrontMatterEditing::set(&MarkdownParser::parse("---\ntitle: &base A\n---\n"), "title", FrontMatterValue::Text("B".into()));
    assert_eq!(anchor.fallback, Some(FrontMatterSourceFallback::AnchorsOrAliasesNotSupported));
    let block = FrontMatterEditing::set(&MarkdownParser::parse("---\ndescription: |\n---\n"), "description", FrontMatterValue::Text("B".into()));
    assert_eq!(block.fallback, Some(FrontMatterSourceFallback::BlockScalarNotSupported));
}

#[test]
#[ignore = "needs parser (upleft-markup)"]
fn renders_number_without_point_zero() {
    let source = "---\ncount: 1\n---\n";
    let proposal = FrontMatterEditing::set(&MarkdownParser::parse(source), "count", FrontMatterValue::Number(42.0)).proposal.expect("a proposal");
    assert_eq!(proposal.applying(source).as_deref(), Some("---\ncount: 42\n---\n"));
}

// MARK: - Differential cases (not in Swift)
//
// Hand-built documents (no parser) whose front matter is what Downright's own
// FrontMatterScanner.swift produced for them, run through
// FrontMatterEditing.swift compiled without the parser; the expectations are
// its output, recorded 2026-09-22 with Swift 6.4.

fn r(location: isize, length: isize) -> NSRange {
    NSRange::new(location, length)
}

fn make_doc(text: &str, front_matter: Option<FrontMatter>) -> ParsedDocument {
    let map = SourceMap::new(text);
    let children: Vec<_> = front_matter
        .iter()
        .map(|fm| MDBlock::new(BlockContent::FrontMatter(fm.clone()), fm.range, fm.range).into_ref())
        .collect();
    let root = MDBlock::new(BlockContent::Document, r(0, map.length), r(0, map.length)).with_children(children);
    ParsedDocument::new(
        text.to_owned(),
        map.length,
        root.into_ref(),
        front_matter,
        vec![],
        vec![],
        vec![],
        HashMap::new(),
        HashMap::new(),
        map.line_starts.clone(),
    )
}

fn field(key: &str, value: &str, key_range: NSRange, value_range: NSRange) -> FrontMatterField {
    FrontMatterField::new(key, value, key_range, value_range)
}

/// A `---` / `---` block whose fields are never read (validation fails first).
fn fenced(text: &str) -> Option<FrontMatter> {
    let length = swift_text::utf16_count(text);
    Some(FrontMatter::new(vec![], r(0, length), r(4, length - 8)))
}

type Expected = (&'static str, Option<(NSRange, &'static str, &'static str, &'static str)>, Option<&'static str>, Option<&'static str>);

fn check(source: &str, front_matter: Option<FrontMatter>, operation: FrontMatterEditOperation, expected: Expected) {
    let result = FrontMatterEditing::propose(&make_doc(source, front_matter), &operation);
    let proposal = result.proposal.as_ref().map(|p| (p.range, p.replacement.as_str(), p.summary.as_str(), p.expected.as_str()));
    let applied = result.proposal.as_ref().and_then(|p| p.applying(source));
    assert_eq!(
        (proposal, result.fallback.map(|f| f.raw_value()), applied.as_deref()),
        (expected.1, expected.2, expected.3),
        "{}",
        expected.0
    );
}

fn set(key: &str, value: FrontMatterValue) -> FrontMatterEditOperation {
    FrontMatterEditOperation::Set { key: key.into(), value }
}

fn add(key: &str, value: FrontMatterValue) -> FrontMatterEditOperation {
    FrontMatterEditOperation::Add { key: key.into(), value }
}

fn remove(key: &str) -> FrontMatterEditOperation {
    FrontMatterEditOperation::Remove { key: key.into() }
}

fn text(value: &str) -> FrontMatterValue {
    FrontMatterValue::Text(value.into())
}

#[test]
fn differential_set_add_remove() {
    let a = "---\ntitle:  \"Old\"  \ncount: 2\n---\nBody\n";
    let fm = || {
        Some(FrontMatter::new(
            vec![field("title", "Old", r(4, 5), r(10, 9)), field("count", "2", r(20, 5), r(26, 2))],
            r(0, 33),
            r(4, 25),
        ))
    };
    check(a, fm(), set("title", text("N\"e\\w")), ("set-quoted", Some((r(10, 9), "  \"N\\\"e\\\\w\"  ", "Set title", "  \"Old\"  ")), None, Some("---\ntitle:  \"N\\\"e\\\\w\"  \ncount: 2\n---\nBody\n")));
    check(a, fm(), set("COUNT", FrontMatterValue::Number(-0.5)), ("set-number", Some((r(26, 2), " -0.5", "Set count", " 2")), None, Some("---\ntitle:  \"Old\"  \ncount: -0.5\n---\nBody\n")));
    check(a, fm(), set("count", FrontMatterValue::Number(1e-7)), ("set-number-exp", Some((r(26, 2), " 1e-07", "Set count", " 2")), None, Some("---\ntitle:  \"Old\"  \ncount: 1e-07\n---\nBody\n")));
    check(a, fm(), set("count", FrontMatterValue::Number(-1e19)), ("set-number-big", Some((r(26, 2), " -1e+19", "Set count", " 2")), None, Some("---\ntitle:  \"Old\"  \ncount: -1e+19\n---\nBody\n")));
    check(a, fm(), set("count", FrontMatterValue::Number(12345678.0)), ("set-number-int", Some((r(26, 2), " 12345678", "Set count", " 2")), None, Some("---\ntitle:  \"Old\"  \ncount: 12345678\n---\nBody\n")));
    check(a, fm(), set("count", FrontMatterValue::Number(f64::NAN)), ("set-nan", None, Some("unsupportedValue"), None));
    let items = ["a", "b c", "", " x", "y:", "1e3", "Yes", "-z", "?q", "ok", "\u{1FEF}"];
    check(a, fm(), set("new key", FrontMatterValue::List(items.iter().map(|s| s.to_string()).collect())), ("set-new", Some((r(29, 0), "new key: [a, b c, \"\", \" x\", \"y:\", \"1e3\", \"Yes\", \"-z\", \"?q\", ok, \"\u{1FEF}\"]\n", "Add new key", "")), None, Some("---\ntitle:  \"Old\"  \ncount: 2\nnew key: [a, b c, \"\", \" x\", \"y:\", \"1e3\", \"Yes\", \"-z\", \"?q\", ok, \"\u{1FEF}\"]\n---\nBody\n")));
    check(a, fm(), add("Title", FrontMatterValue::Boolean(true)), ("add-existing", None, Some("ambiguousField"), None));
    check(a, fm(), add("bad:key", FrontMatterValue::Boolean(true)), ("add-bad-key", None, Some("unsupportedValue"), None));
    check(a, fm(), remove("count"), ("remove", Some((r(20, 9), "", "Remove count", "count: 2\n")), None, Some("---\ntitle:  \"Old\"  \n---\nBody\n")));
    check(a, fm(), remove("nope"), ("remove-missing", None, Some("ambiguousField"), None));
}

#[test]
fn differential_line_endings_and_empty_fields() {
    let b = "---\r\ntitle: Demo\r\nTags: [a, b]\r\n---\r\n";
    let fb = || {
        Some(FrontMatter::new(
            vec![field("title", "Demo", r(5, 5), r(11, 5)), field("Tags", "a, b", r(18, 4), r(23, 7))],
            r(0, 37),
            r(5, 27),
        ))
    };
    check(b, fb(), add("draft", FrontMatterValue::Boolean(false)), ("crlf-add", Some((r(32, 0), "draft: false\r\n", "Add draft", "")), None, Some("---\r\ntitle: Demo\r\nTags: [a, b]\r\ndraft: false\r\n---\r\n")));
    check(b, fb(), remove("title"), ("crlf-remove", Some((r(5, 13), "", "Remove title", "title: Demo\r\n")), None, Some("---\r\nTags: [a, b]\r\n---\r\n")));
    check(b, fb(), set("tags", text("plain")), ("crlf-set-list", Some((r(23, 7), " plain", "Set Tags", " [a, b]")), None, Some("---\r\ntitle: Demo\r\nTags: plain\r\n---\r\n")));

    let c = "---\ntitle: \nquote: 'x'\n---\n";
    let fc = || Some(FrontMatter::new(vec![field("quote", "x", r(12, 5), r(18, 4))], r(0, 27), r(4, 19)));
    check(c, fc(), set("TITLE", text("Demo")), ("empty-set", Some((r(10, 1), " Demo", "Set title", " ")), None, Some("---\ntitle: Demo\nquote: 'x'\n---\n")));
    check(c, fc(), set("quote", text("it's")), ("single-quote", Some((r(18, 4), " 'it''s'", "Set quote", " 'x'")), None, Some("---\ntitle: \nquote: 'it''s'\n---\n")));
    check(c, fc(), set("quote", text("")), ("empty-string", Some((r(18, 4), " ''", "Set quote", " 'x'")), None, Some("---\ntitle: \nquote: ''\n---\n")));

    check("---\n---\n", Some(FrontMatter::new(vec![], r(0, 8), r(4, 0))), add("k", text("v")), ("empty-fm-add", Some((r(4, 0), "k: v\n", "Add k", "")), None, Some("---\nk: v\n---\n")));
    let cr = "---\rtitle: A\r---\r";
    check(cr, Some(FrontMatter::new(vec![field("title", "A", r(4, 5), r(10, 2))], r(0, 17), r(4, 9))), add("k", text("v")), ("cr-add", Some((r(13, 0), "k: v\r", "Add k", "")), None, Some("---\rtitle: A\rk: v\r---\r")));
}

#[test]
fn differential_fallbacks() {
    check("Body\n", None, add("k", text("v")), ("missing", None, Some("missingFrontMatter"), None));
    check(" --- \ntitle: A\n", None, add("k", text("v")), ("malformed", None, Some("malformedFence"), None));
    let cases: [(&str, FrontMatterEditOperation, Expected); 7] = [
        ("---\ntags:\n  - one\n---\n", set("tags", FrontMatterValue::List(vec!["two".into()])), ("nested", None, Some("nestedYAML"), None)),
        ("---\n# note\ntitle: Demo\n---\n", set("title", text("x")), ("comment", None, Some("commentsNotSupported"), None)),
        ("---\ntitle: *ref\n---\n", set("title", text("x")), ("anchor", None, Some("anchorsOrAliasesNotSupported"), None)),
        ("---\ndescription: > folded\n---\n", set("description", text("x")), ("block", None, Some("blockScalarNotSupported"), None)),
        ("---\ntitle: A\nTITLE: B\n---\n", set("title", text("x")), ("dup", None, Some("ambiguousField"), None)),
        ("---\njust text\n---\n", set("title", text("x")), ("nokey", None, Some("unsupportedValue"), None)),
        ("---\nba/d: x\n---\n", set("title", text("x")), ("badname", None, Some("unsupportedValue"), None)),
    ];
    for (source, operation, expected) in cases {
        check(source, fenced(source), operation, expected);
    }
}
