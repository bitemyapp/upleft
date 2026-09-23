//! TableEditingTests.swift, plus differential cases on hand-built documents.

use std::collections::HashMap;

use upleft_core::editing::table_editing::{TableEditOperation, TableEditProposal, TableEditing, TableSourceFallback};
use upleft_core::parser::MarkdownParser;
use upleft_core::source_positions::SourceMap;
use upleft_core::swift_text;
use upleft_core::table_formatter::TableFormatter;
use upleft_core::{BlockContent, MDBlock, NSRange, ParsedDocument, TableAlignment, TableCell, TableData, TableRow};

fn proposal(source: &str, operation: TableEditOperation) -> TableEditProposal {
    let result = TableEditing::propose(&MarkdownParser::parse(source), 0, &operation);
    result.proposal.expect("a proposal")
}

#[test]
fn cell_edit_is_local_and_escapes_pipes() {
    let source = "| A | B |\n|---|---|\n| one | two |\n";
    let edit = proposal(source, TableEditOperation::SetCell { row: 1, column: 0, text: "x|y".into() });
    let output = edit.applying(source).expect("applies");
    assert_eq!(output, "| A | B |\n|---|---|\n| x\\|y | two |\n");
    assert!(edit.range.length < swift_text::utf16_count(source));
}

#[test]
fn alignment_only_rewrites_delimiter() {
    let source = "  | A | B |\r\n  |---|---|\r\n  | one | two |\r\n";
    let edit = proposal(source, TableEditOperation::SetAlignment { column: 1, alignment: TableAlignment::Right });
    assert_eq!(edit.applying(source).as_deref(), Some("  | A | B |\r\n  |---|--:|\r\n  | one | two |\r\n"));
}

#[test]
fn insert_row_after_delimiter_preserves_indentation() {
    let source = "  | A | B |\n  |---|---|\n  | one | two |\n";
    let edit = proposal(source, TableEditOperation::InsertRow { index: 1, cells: vec!["new".into(), "row".into()] });
    assert_eq!(edit.applying(source).as_deref(), Some("  | A | B |\n  |---|---|\n  | new | row |\n  | one | two |\n"));
}

#[test]
fn insert_row_pads_missing_cells() {
    let source = "| A | B | C |\n|---|---|---|\n| one | two | three |\n";
    let edit = proposal(source, TableEditOperation::InsertRow { index: 1, cells: vec!["new".into()] });
    assert_eq!(edit.applying(source).as_deref(), Some("| A | B | C |\n|---|---|---|\n| new |  |  |\n| one | two | three |\n"));
}

#[test]
fn insert_column_accepts_one_value_per_body_row() {
    let source = "| A |\n|---|\n| one |\n| two |\n| three |\n";
    let edit = proposal(
        source,
        TableEditOperation::InsertColumn { index: 1, header: "B".into(), cells: vec!["1".into(), "2".into(), "3".into()] },
    );
    assert_eq!(edit.applying(source).as_deref(), Some("| A | B |\n|---|---|\n| one | 1 |\n| two | 2 |\n| three | 3 |\n"));
}

#[test]
fn column_operations_keep_escaped_pipes() {
    let source = "| A | B | C |\n|---|---|---|\n| a\\|x | b | c |\n";
    let moved = proposal(source, TableEditOperation::MoveColumn { from: 0, to: 2 });
    let output = moved.applying(source).expect("applies");
    assert!(swift_text::contains(&output, "| b | c | a\\|x |"));
    let deleted = proposal(source, TableEditOperation::DeleteColumn { index: 1 });
    assert_eq!(deleted.applying(source).map(|o| swift_text::contains(&o, "| A | C |")), Some(true));
}

#[test]
fn cannot_delete_header_and_rejects_stale_source() {
    let source = "| A | B |\n|---|---|\n| one | two |\n";
    let result = TableEditing::propose(&MarkdownParser::parse(source), 0, &TableEditOperation::DeleteRow { index: 0 });
    assert!(result.proposal.is_none());
    assert_eq!(result.fallback, Some(TableSourceFallback::CannotDeleteHeader));
    let edit = proposal(source, TableEditOperation::SetCell { row: 1, column: 1, text: "new".into() });
    assert_eq!(edit.applying(&swift_text::replacing_occurrences(source, "two", "old")), None);
}

#[test]
fn spaced_delimiter_does_not_duplicate_cell_padding() {
    let source = "| A | B |\n| --- | --- |\n| one | two |\n";
    let edit = proposal(source, TableEditOperation::SetAlignment { column: 1, alignment: TableAlignment::Right });
    assert_eq!(edit.applying(source).as_deref(), Some("| A | B |\n| --- | --: |\n| one | two |\n"));
}

#[test]
fn pipe_less_rows_remain_pipe_less_for_column_moves() {
    let source = "A | B | C\n---|---|---\na | b | c\n";
    let edit = proposal(source, TableEditOperation::MoveColumn { from: 2, to: 0 });
    assert_eq!(edit.applying(source).map(|o| swift_text::contains(&o, " C|A | B ")), Some(true));
}

#[test]
fn reverse_row_move_preserves_crlf_and_final_newline() {
    let source = "| A | B |\r\n|---|---|\r\n| one | two |\r\n| three | four |\r\n";
    let edit = proposal(source, TableEditOperation::MoveRow { from: 2, to: 1 });
    assert_eq!(edit.applying(source).as_deref(), Some("| A | B |\r\n|---|---|\r\n| three | four |\r\n| one | two |\r\n"));
}

/// Regression: inserting a row after a terminator-less last row used to glue
/// the new line onto its content.
#[test]
fn insert_row_after_terminator_less_last_row_does_not_glue() {
    let source = "| A | B |\n|---|---|\n| one | two |";
    let edit = proposal(source, TableEditOperation::InsertRow { index: 2, cells: vec!["x".into(), "y".into()] });
    assert_eq!(edit.applying(source).as_deref(), Some("| A | B |\n|---|---|\n| one | two |\n| x | y |\n"));
}

/// Regression: moving the terminator-less last row into the middle used to
/// glue it onto its new neighbour.
#[test]
fn move_row_with_terminator_less_last_row_does_not_glue() {
    let source = "| A | B |\n|---|---|\n| one | two |\n| three | four |";
    let edit = proposal(source, TableEditOperation::MoveRow { from: 2, to: 1 });
    assert_eq!(edit.applying(source).as_deref(), Some("| A | B |\n|---|---|\n| three | four |\n| one | two |\n"));
}

#[test]
fn blockquote_table_keeps_quote_prefix() {
    let source = "> | A | B |\n> |---|---|\n> | one | two |\n";
    let edit = proposal(source, TableEditOperation::SetCell { row: 1, column: 1, text: "changed".into() });
    assert_eq!(edit.applying(source).map(|o| swift_text::contains(&o, "> | one | changed |")), Some(true));
}

#[test]
fn invalid_structure_operations_return_typed_fallbacks() {
    let source = "| A |\n|---|\n| one |\n";
    let last_column = TableEditing::propose(&MarkdownParser::parse(source), 0, &TableEditOperation::DeleteColumn { index: 0 });
    assert_eq!(last_column.fallback, Some(TableSourceFallback::CannotDeleteLastColumn));
    let too_many = TableEditing::propose(
        &MarkdownParser::parse(source),
        0,
        &TableEditOperation::InsertRow { index: 1, cells: vec!["one".into(), "two".into()] },
    );
    assert_eq!(too_many.fallback, Some(TableSourceFallback::UnsupportedOperation));
    let invalid = TableEditing::propose(
        &MarkdownParser::parse(source),
        0,
        &TableEditOperation::SetCell { row: 99, column: 0, text: "x".into() },
    );
    assert_eq!(invalid.fallback, Some(TableSourceFallback::InvalidRow));
}

#[test]
fn table_formatter_with_leading_preamble_does_not_start_at_zero() {
    let source = "# Heading\n\nSome introductory paragraph.\n\n| A | B |\n|---|---|\n| 1 | 2 |\n";
    let doc = MarkdownParser::parse(source);
    let mut found_table: Option<TableData> = None;
    doc.root.walk(&mut |block| {
        if let BlockContent::Table(data) = &block.content {
            found_table = Some(data.clone());
        }
    });
    let table = found_table.expect("a table");
    let range = TableFormatter::source_range(&table, NSRange::new(0, 0));
    let model = TableFormatter::model(&table, &swift_text::ns::utf16(source));
    assert!(range.location > 20);
    assert_eq!(model.line_terminators, ["\n", "\n", "\n"]);
}

// MARK: - Differential cases (not in Swift)
//
// Hand-built table blocks (no parser) run through Downright's own
// TableEditing.swift compiled without the parser; the expectations are its
// output, recorded 2026-09-22 with Swift 6.4.

fn r(location: isize, length: isize) -> NSRange {
    NSRange::new(location, length)
}

fn line(text: &str, index: isize) -> NSRange {
    SourceMap::new(text).content_range_of_line(index)
}

fn lines(text: &str, first: isize, last: isize) -> NSRange {
    let map = SourceMap::new(text);
    let (a, b) = (map.content_range_of_line(first), map.content_range_of_line(last));
    r(a.location, b.upper_bound() - a.location)
}

fn make_doc(text: &str, children: Vec<MDBlock>) -> ParsedDocument {
    let map = SourceMap::new(text);
    let root = MDBlock::new(BlockContent::Document, r(0, map.length), r(0, map.length))
        .with_children(children.into_iter().map(MDBlock::into_ref).collect());
    ParsedDocument::new(
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
    )
}

fn table_doc(text: &str, header: isize, delimiter: isize, body: &[isize], cells: &[usize]) -> ParsedDocument {
    let rows: Vec<TableRow> = std::iter::once(header)
        .chain(body.iter().copied())
        .enumerate()
        .map(|(i, l)| TableRow::new(line(text, l), vec![TableCell::new(r(0, 0), r(0, 0), vec![]); cells[i]], i == 0))
        .collect();
    let data = TableData::new(rows, vec![], line(text, delimiter));
    let last = body.last().copied().unwrap_or(header);
    let range = lines(text, header, last);
    make_doc(text, vec![MDBlock::new(BlockContent::Table(data), range, range)])
}

type Expected = (&'static str, Option<(NSRange, &'static str, &'static str, &'static str)>, Option<&'static str>, Option<&'static str>);

fn check(source: &str, doc: &ParsedDocument, operation: TableEditOperation, expected: Expected) {
    let result = TableEditing::propose(doc, 0, &operation);
    let proposal = result.proposal.as_ref().map(|p| (p.range, p.replacement.as_str(), p.summary.as_str(), p.expected.as_str()));
    let applied = result.proposal.as_ref().and_then(|p| p.applying(source));
    assert_eq!(
        (proposal, result.fallback.map(|f| f.raw_value()), applied.as_deref()),
        (expected.1, expected.2, expected.3),
        "{}",
        expected.0
    );
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
}

#[test]
fn differential_simple_table() {
    let a = "| A | B |\n|---|---|\n| one | two |\n";
    let d = table_doc(a, 0, 1, &[2], &[2, 2]);
    use TableEditOperation as Op;
    check(a, &d, Op::SetCell { row: 1, column: 0, text: "x|y\\z".into() }, ("cell", Some((r(20, 13), "| x\\|y\\\\z | two |", "Edit table cell", "| one | two |")), None, Some("| A | B |\n|---|---|\n| x\\|y\\\\z | two |\n")));
    check(a, &d, Op::SetCell { row: 1, column: 5, text: "x".into() }, ("cell-col", None, Some("invalidColumn"), None));
    check(a, &d, Op::SetCell { row: -1, column: 0, text: "x".into() }, ("cell-row", None, Some("invalidRow"), None));
    check(a, &d, Op::align(0, TableAlignment::Center), ("align-center", Some((r(10, 9), "|:-:|---|", "Set table alignment", "|---|---|")), None, Some("| A | B |\n|:-:|---|\n| one | two |\n")));
    check(a, &d, Op::align(1, TableAlignment::Left), ("align-left", Some((r(10, 9), "|---|:--|", "Set table alignment", "|---|---|")), None, Some("| A | B |\n|---|:--|\n| one | two |\n")));
    check(a, &d, Op::align(1, TableAlignment::None), ("align-none", Some((r(10, 9), "|---|---|", "Set table alignment", "|---|---|")), None, Some("| A | B |\n|---|---|\n| one | two |\n")));
    check(a, &d, Op::DeleteRow { index: 0 }, ("del-row-0", None, Some("cannotDeleteHeader"), None));
    check(a, &d, Op::DeleteRow { index: 1 }, ("del-row-1", Some((r(20, 14), "", "Delete table row", "| one | two |\n")), None, Some("| A | B |\n|---|---|\n")));
    check(a, &d, Op::DeleteRow { index: 2 }, ("del-row-2", None, Some("invalidRow"), None));
    check(a, &d, Op::InsertRow { index: 0, cells: vec![] }, ("ins-row-0", None, Some("invalidRow"), None));
    check(a, &d, Op::InsertRow { index: 2, cells: strings(&["p|q"]) }, ("ins-row-2", Some((r(0, 34), "| A | B |\n|---|---|\n| one | two |\n| p\\|q |  |\n", "Insert table row", "| A | B |\n|---|---|\n| one | two |\n")), None, Some("| A | B |\n|---|---|\n| one | two |\n| p\\|q |  |\n")));
    check(a, &d, Op::InsertColumn { index: 0, header: "H".into(), cells: strings(&["1"]) }, ("ins-col-0", Some((r(0, 34), "| H | A | B |\n|---|---|---|\n| 1 | one | two |\n", "Insert table column", "| A | B |\n|---|---|\n| one | two |\n")), None, Some("| H | A | B |\n|---|---|---|\n| 1 | one | two |\n")));
    check(a, &d, Op::InsertColumn { index: 2, header: "H".into(), cells: vec![] }, ("ins-col-2", Some((r(0, 34), "| A | B | H |\n|---|---|---|\n| one | two |  |\n", "Insert table column", "| A | B |\n|---|---|\n| one | two |\n")), None, Some("| A | B | H |\n|---|---|---|\n| one | two |  |\n")));
    check(a, &d, Op::InsertColumn { index: 1, header: "H".into(), cells: strings(&["1", "2"]) }, ("ins-col-many", None, Some("unsupportedOperation"), None));
    check(a, &d, Op::DeleteColumn { index: 0 }, ("del-col", Some((r(0, 34), "| B |\n|---|\n| two |\n", "Delete table column", "| A | B |\n|---|---|\n| one | two |\n")), None, Some("| B |\n|---|\n| two |\n")));
    check(a, &d, Op::MoveColumn { from: 1, to: 0 }, ("move-col", Some((r(0, 34), "| B | A |\n|---|---|\n| two | one |\n", "Move table column", "| A | B |\n|---|---|\n| one | two |\n")), None, Some("| B | A |\n|---|---|\n| two | one |\n")));
    check(a, &d, Op::MoveColumn { from: 2, to: 0 }, ("move-col-bad", None, Some("invalidColumn"), None));
    check(a, &d, Op::MoveRow { from: 1, to: 2 }, ("move-row-bad", None, Some("invalidRow"), None));
}

#[test]
fn differential_crlf_indented_table_without_final_newline() {
    let b = "  | A | B |\r\n  |---|---|\r\n  | one | two |\r\n  | three | four |";
    let d = table_doc(b, 0, 1, &[2, 3], &[2, 2, 2]);
    use TableEditOperation as Op;
    check(b, &d, Op::MoveRow { from: 2, to: 1 }, ("crlf-move", Some((r(0, 61), "  | A | B |\r\n  |---|---|\r\n  | three | four |\r\n  | one | two |\r\n", "Move table row", "  | A | B |\r\n  |---|---|\r\n  | one | two |\r\n  | three | four |")), None, Some("  | A | B |\r\n  |---|---|\r\n  | three | four |\r\n  | one | two |\r\n")));
    check(b, &d, Op::InsertRow { index: 3, cells: strings(&["x", "y"]) }, ("crlf-ins", Some((r(0, 61), "  | A | B |\r\n  |---|---|\r\n  | one | two |\r\n  | three | four |\r\n  | x | y |\r\n", "Insert table row", "  | A | B |\r\n  |---|---|\r\n  | one | two |\r\n  | three | four |")), None, Some("  | A | B |\r\n  |---|---|\r\n  | one | two |\r\n  | three | four |\r\n  | x | y |\r\n")));
    check(b, &d, Op::InsertRow { index: 1, cells: strings(&["x"]) }, ("crlf-ins-mid", Some((r(0, 61), "  | A | B |\r\n  |---|---|\r\n  | x |  |\r\n  | one | two |\r\n  | three | four |", "Insert table row", "  | A | B |\r\n  |---|---|\r\n  | one | two |\r\n  | three | four |")), None, Some("  | A | B |\r\n  |---|---|\r\n  | x |  |\r\n  | one | two |\r\n  | three | four |")));
    check(b, &d, Op::InsertColumn { index: 1, header: "M".into(), cells: strings(&["m1"]) }, ("crlf-ins-col", Some((r(0, 61), "  | A | M | B |\r\n  |---|---|---|\r\n  | one | m1 | two |\r\n  | three |  | four |", "Insert table column", "  | A | B |\r\n  |---|---|\r\n  | one | two |\r\n  | three | four |")), None, Some("  | A | M | B |\r\n  |---|---|---|\r\n  | one | m1 | two |\r\n  | three |  | four |")));
}

#[test]
fn differential_blockquote_table_with_blank_and_escaped_cells() {
    let c = "> | A |   |\n> |:-:|---|\n> |   | \\| |\n";
    let d = table_doc(c, 0, 1, &[2], &[2, 2]);
    use TableEditOperation as Op;
    check(c, &d, Op::SetCell { row: 1, column: 0, text: "v".into() }, ("quote-cell", Some((r(24, 12), "> |   v   | \\| |", "Edit table cell", "> |   | \\| |")), None, Some("> | A |   |\n> |:-:|---|\n> |   v   | \\| |\n")));
    check(c, &d, Op::SetCell { row: 0, column: 1, text: "w".into() }, ("quote-cell2", Some((r(0, 11), "> | A |   w   |", "Edit table cell", "> | A |   |")), None, Some("> | A |   w   |\n> |:-:|---|\n> |   | \\| |\n")));
    check(c, &d, Op::MoveColumn { from: 0, to: 1 }, ("quote-move", Some((r(0, 37), "> |   | A |\n> |---|:-:|\n> | \\| |   |\n", "Move table column", "> | A |   |\n> |:-:|---|\n> |   | \\| |\n")), None, Some("> |   | A |\n> |---|:-:|\n> | \\| |   |\n")));
    check(c, &d, Op::align(0, TableAlignment::Right), ("quote-align", Some((r(12, 11), "> |--:|---|", "Set table alignment", "> |:-:|---|")), None, Some("> | A |   |\n> |--:|---|\n> |   | \\| |\n")));
}

#[test]
fn differential_pipeless_table() {
    let t = "A | B | C\n---|:---|---\na | b | c\n";
    let d = table_doc(t, 0, 1, &[2], &[3, 3]);
    use TableEditOperation as Op;
    check(t, &d, Op::MoveColumn { from: 2, to: 0 }, ("pipeless-move", Some((r(0, 33), " C|A | B \n---|---|:---\n c|a | b \n", "Move table column", "A | B | C\n---|:---|---\na | b | c\n")), None, Some(" C|A | B \n---|---|:---\n c|a | b \n")));
    check(t, &d, Op::DeleteColumn { index: 0 }, ("pipeless-del", Some((r(0, 33), " B | C\n:---|---\n b | c\n", "Delete table column", "A | B | C\n---|:---|---\na | b | c\n")), None, Some(" B | C\n:---|---\n b | c\n")));
    check(t, &d, Op::InsertColumn { index: 3, header: "D".into(), cells: strings(&["d"]) }, ("pipeless-ins", Some((r(0, 33), "A | B | C|D \n---|:---|---|---\na | b | c|d \n", "Insert table column", "A | B | C\n---|:---|---\na | b | c\n")), None, Some("A | B | C|D \n---|:---|---|---\na | b | c|d \n")));
    check(t, &d, Op::align(1, TableAlignment::Center), ("pipeless-align", Some((r(10, 12), "---|:-:|---", "Set table alignment", "---|:---|---")), None, Some("A | B | C\n---|:-:|---\na | b | c\n")));
}

#[test]
fn differential_fallbacks_and_row_moves() {
    let e = "| A | B |\n|---|---|\n| one | two |\n| x | y |\n";
    let d = table_doc(e, 0, 1, &[2, 3], &[2, 2, 2]);
    use TableEditOperation as Op;
    check(e, &d, Op::MoveRow { from: 1, to: 2 }, ("stale-rows", Some((r(0, 44), "| A | B |\n|---|---|\n| x | y |\n| one | two |\n", "Move table row", "| A | B |\n|---|---|\n| one | two |\n| x | y |\n")), None, Some("| A | B |\n|---|---|\n| x | y |\n| one | two |\n")));
    check(e, &d, Op::InsertRow { index: 3, cells: strings(&["z"]) }, ("ins-row-end", Some((r(0, 44), "| A | B |\n|---|---|\n| one | two |\n| x | y |\n| z |  |\n", "Insert table row", "| A | B |\n|---|---|\n| one | two |\n| x | y |\n")), None, Some("| A | B |\n|---|---|\n| one | two |\n| x | y |\n| z |  |\n")));
    check(e, &make_doc(e, vec![]), Op::DeleteRow { index: 1 }, ("table-missing", None, Some("tableNotFound"), None));
    let header_only = TableData::new(vec![TableRow::new(line(e, 0), vec![], true)], vec![], r(100, 3));
    let wrong_delimiter = make_doc(e, vec![MDBlock::new(BlockContent::Table(header_only), line(e, 0), line(e, 0))]);
    check(e, &wrong_delimiter, Op::DeleteRow { index: 1 }, ("no-delimiter", None, Some("invalidSourceRange"), None));
    let empty_rows = make_doc(e, vec![MDBlock::new(BlockContent::Table(TableData::new(vec![], vec![], line(e, 1))), line(e, 1), line(e, 1))]);
    check(e, &empty_rows, Op::DeleteRow { index: 1 }, ("empty-rows", None, Some("malformedTable"), None));
    // `propose_table` finds the same table by its ranges.
    let BlockContent::Table(data) = &d.root.children[0].content else { unreachable!() };
    assert_eq!(TableEditing::propose_table(&d, data, &Op::MoveRow { from: 1, to: 2 }), TableEditing::propose(&d, 0, &Op::MoveRow { from: 1, to: 2 }));
    assert_eq!(TableEditing::propose(&d, 1, &Op::DeleteRow { index: 1 }).fallback, Some(TableSourceFallback::TableNotFound));
    let edit = TableEditing::proposal(&d, 0, &Op::DeleteRow { index: 1 }).expect("a proposal").edit();
    assert_eq!((edit.range, edit.replacement.as_str(), edit.summary.as_str(), edit.rule), (r(20, 14), "", "Delete table row", None));
}
