//! Editing/TableEditing.swift — source-preserving table edits.
//!
//! Each operation becomes one proposal over the table's source lines, guarded
//! by `expected` so an old proposal never changes a newer buffer.

use crate::contracts::TextEdit;
use crate::model::{BlockContent, MDBlock, ParsedDocument, TableAlignment, TableData};
use crate::ns_range::{NSRange, ns_intersection_range};
use crate::swift_text::{
    self,
    ns::{NSStringExt, string_from_utf16, utf16},
};

#[derive(Clone, Debug, PartialEq)]
pub enum TableEditOperation {
    SetCell { row: isize, column: isize, text: String },
    SetAlignment { column: isize, alignment: TableAlignment },
    InsertRow { index: isize, cells: Vec<String> },
    DeleteRow { index: isize },
    MoveRow { from: isize, to: isize },
    InsertColumn { index: isize, header: String, cells: Vec<String> },
    DeleteColumn { index: isize },
    MoveColumn { from: isize, to: isize },
}

impl TableEditOperation {
    pub fn align(column: isize, alignment: TableAlignment) -> TableEditOperation {
        TableEditOperation::SetAlignment { column, alignment }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TableSourceFallback {
    TableNotFound,
    InvalidSourceRange,
    SourceChanged,
    InvalidRow,
    InvalidColumn,
    CannotDeleteHeader,
    MalformedTable,
    UnsupportedOperation,
    CannotDeleteLastColumn,
}

impl TableSourceFallback {
    pub fn raw_value(&self) -> &'static str {
        match self {
            TableSourceFallback::TableNotFound => "tableNotFound",
            TableSourceFallback::InvalidSourceRange => "invalidSourceRange",
            TableSourceFallback::SourceChanged => "sourceChanged",
            TableSourceFallback::InvalidRow => "invalidRow",
            TableSourceFallback::InvalidColumn => "invalidColumn",
            TableSourceFallback::CannotDeleteHeader => "cannotDeleteHeader",
            TableSourceFallback::MalformedTable => "malformedTable",
            TableSourceFallback::UnsupportedOperation => "unsupportedOperation",
            TableSourceFallback::CannotDeleteLastColumn => "cannotDeleteLastColumn",
        }
    }
}

impl std::fmt::Display for TableSourceFallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.raw_value())
    }
}

impl std::error::Error for TableSourceFallback {}

#[derive(Clone, Debug, PartialEq)]
pub struct TableEditProposal {
    pub range: NSRange,
    pub replacement: String,
    pub summary: String,
    pub expected: String,
}

impl TableEditProposal {
    pub fn new(range: NSRange, replacement: impl Into<String>, summary: impl Into<String>, expected: impl Into<String>) -> Self {
        TableEditProposal { range, replacement: replacement.into(), summary: summary.into(), expected: expected.into() }
    }

    pub fn edit(&self) -> TextEdit {
        TextEdit::new(self.range, self.replacement.clone(), self.summary.clone(), None)
    }

    pub fn applying(&self, source: &str) -> Option<String> {
        let mut ns = utf16(source);
        if !(self.range.location >= 0
            && self.range.upper_bound() <= ns.as_slice().length()
            && swift_text::str_eq(&ns.as_slice().substring(self.range), &self.expected))
        {
            return None;
        }
        ns.splice(self.range.as_usize_range(), self.replacement.encode_utf16());
        Some(string_from_utf16(&ns))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableEditResult {
    pub proposal: Option<TableEditProposal>,
    pub fallback: Option<TableSourceFallback>,
}

impl TableEditResult {
    pub fn new(proposal: Option<TableEditProposal>, fallback: Option<TableSourceFallback>) -> TableEditResult {
        TableEditResult { proposal, fallback }
    }

    fn fail(reason: TableSourceFallback) -> TableEditResult {
        TableEditResult::new(None, Some(reason))
    }
}

enum ColumnMode<'a> {
    Insert { header: &'a str, cells: &'a [String] },
    Delete,
    Move { to: isize },
}

struct LineRecord {
    content_range: NSRange,
    full_range: NSRange,
    text: String,
    /// `text as NSString`.
    text_utf16: Vec<u16>,
    terminator: String,
}

impl LineRecord {
    fn raw_with_terminator(&self) -> String {
        format!("{}{}", self.text, self.terminator)
    }
}

pub struct TableEditing;

impl TableEditing {
    /// `propose(_:tableIndex:operation:)` (Swift defaults `tableIndex` to 0).
    pub fn propose(document: &ParsedDocument, table_index: isize, operation: &TableEditOperation) -> TableEditResult {
        let tables = Self::table_blocks(document);
        if !(table_index >= 0 && (table_index as usize) < tables.len()) {
            return TableEditResult::fail(TableSourceFallback::TableNotFound);
        }
        let (block, data) = tables[table_index as usize];
        Self::propose_block(document, block, data, operation)
    }

    /// `propose(_:table:operation:)`.
    pub fn propose_table(document: &ParsedDocument, table: &TableData, operation: &TableEditOperation) -> TableEditResult {
        let tables = Self::table_blocks(document);
        let Some(&(block, data)) = tables.iter().find(|(_, data)| Self::same_table(data, table)) else {
            return TableEditResult::fail(TableSourceFallback::TableNotFound);
        };
        Self::propose_block(document, block, data, operation)
    }

    pub fn proposal(document: &ParsedDocument, table_index: isize, operation: &TableEditOperation) -> Option<TableEditProposal> {
        Self::propose(document, table_index, operation).proposal
    }

    fn propose_block(document: &ParsedDocument, block: &MDBlock, data: &TableData, operation: &TableEditOperation) -> TableEditResult {
        if !(matches!(block.content, BlockContent::Table(_)) && !data.rows.is_empty()) {
            return TableEditResult::fail(TableSourceFallback::MalformedTable);
        }
        let source = document.utf16.as_slice();
        let first_row = &data.rows[0];
        let last_row = &data.rows[data.rows.len() - 1];
        let start = source.line_start_before(first_row.range.location).min(source.line_start_before(data.delimiter_range.location));
        let end = source.line_end_after(last_row.range.upper_bound()).max(source.line_end_after(data.delimiter_range.upper_bound()));
        if !(start >= 0 && end <= source.length() && start < end) {
            return TableEditResult::fail(TableSourceFallback::InvalidSourceRange);
        }
        let table_range = NSRange::new(start, end - start);
        let expected = source.substring(table_range);
        let records = Self::line_records(source, table_range);
        let Some(delimiter) =
            records.iter().position(|record| ns_intersection_range(record.content_range, data.delimiter_range).length > 0)
        else {
            return TableEditResult::fail(TableSourceFallback::MalformedTable);
        };
        let row_indices: Vec<usize> = data
            .rows
            .iter()
            .filter_map(|row| records.iter().position(|record| ns_intersection_range(record.content_range, row.range).length > 0))
            .collect();
        if row_indices.len() != data.rows.len() {
            return TableEditResult::fail(TableSourceFallback::SourceChanged);
        }
        let row_count = row_indices.len() as isize;
        let column_count = data.column_count();

        match operation {
            TableEditOperation::SetCell { row, column, text } => {
                if !(*row >= 0 && *row < row_count) {
                    return TableEditResult::fail(TableSourceFallback::InvalidRow);
                }
                let line = &records[row_indices[*row as usize]];
                let Some(segment) = element_at(&Self::cell_segments(&line.text_utf16), *column) else {
                    return TableEditResult::fail(TableSourceFallback::InvalidColumn);
                };
                let replacement = Self::replace_segment(&line.text_utf16, segment, text);
                let range = line.content_range;
                Self::make(document, range, replacement, "Edit table cell", source.substring(range))
            }
            TableEditOperation::SetAlignment { column, alignment } => {
                let line = &records[delimiter];
                let segments = Self::cell_segments(&line.text_utf16);
                let Some(segment) = element_at(&segments, *column) else {
                    return TableEditResult::fail(TableSourceFallback::InvalidColumn);
                };
                let token = Self::delimiter_token(*alignment, &line.text_utf16.as_slice().substring(segment));
                let replacement = Self::replace_segment(&line.text_utf16, segment, &token);
                Self::make(document, line.content_range, replacement, "Set table alignment", source.substring(line.content_range))
            }
            TableEditOperation::InsertRow { index, cells } => {
                if !(*index >= 1 && *index <= data.rows.len() as isize) {
                    return TableEditResult::fail(TableSourceFallback::InvalidRow);
                }
                if !(cells.len() as isize <= column_count) {
                    return TableEditResult::fail(TableSourceFallback::UnsupportedOperation);
                }
                let insertion_offset =
                    if *index < row_count { (delimiter + 1).max(row_indices[*index as usize]) } else { records.len() };
                let indent = Self::indentation(&records[row_indices[0]].text);
                let mut padded_cells: Vec<String> = cells.clone();
                padded_cells.extend(std::iter::repeat_n(String::new(), (column_count - cells.len() as isize) as usize));
                let line = Self::render_row(&padded_cells, indent);
                Self::make_whole_inserting(document, table_range, &records, insertion_offset, &[line], "Insert table row", expected)
            }
            TableEditOperation::DeleteRow { index } => {
                if !(*index > 0 && *index < row_count) {
                    return TableEditResult::fail(if *index == 0 {
                        TableSourceFallback::CannotDeleteHeader
                    } else {
                        TableSourceFallback::InvalidRow
                    });
                }
                let full_range = records[row_indices[*index as usize]].full_range;
                Self::make(document, full_range, String::new(), "Delete table row", source.substring(full_range))
            }
            TableEditOperation::MoveRow { from, to } => {
                if !(*from > 0 && *to > 0 && *from < row_count && *to < row_count) {
                    return TableEditResult::fail(TableSourceFallback::InvalidRow);
                }
                let ending = records.first().map_or_else(|| "\n".to_owned(), |record| record.terminator.clone());
                // `rawWithTerminator` has no terminator for the last record of a
                // file that does not end in a newline; a move can place it in the
                // middle, gluing it onto its new neighbour — so give every record
                // a terminator first.
                let mut output: Vec<String> = records
                    .iter()
                    .map(|record| {
                        let raw = record.raw_with_terminator();
                        if Self::has_terminator(&raw) { raw } else { raw + &ending }
                    })
                    .collect();
                let moved = output.remove(row_indices[*from as usize]);
                output.insert(row_indices[*to as usize], moved);
                Self::make(document, table_range, output.concat(), "Move table row", expected)
            }
            TableEditOperation::InsertColumn { index, header, cells } => {
                let count = column_count;
                if !(*index >= 0 && *index <= count) {
                    return TableEditResult::fail(TableSourceFallback::InvalidColumn);
                }
                if !(cells.len() as isize <= 0.max(data.rows.len() as isize - 1)) {
                    return TableEditResult::fail(TableSourceFallback::UnsupportedOperation);
                }
                Self::column_mutation(
                    document,
                    table_range,
                    &records,
                    delimiter,
                    &row_indices,
                    *index,
                    ColumnMode::Insert { header, cells },
                    "Insert table column",
                    expected,
                )
            }
            TableEditOperation::DeleteColumn { index } => {
                if !(*index >= 0 && *index < column_count) {
                    return TableEditResult::fail(TableSourceFallback::InvalidColumn);
                }
                if !(column_count > 1) {
                    return TableEditResult::fail(TableSourceFallback::CannotDeleteLastColumn);
                }
                Self::column_mutation(
                    document,
                    table_range,
                    &records,
                    delimiter,
                    &row_indices,
                    *index,
                    ColumnMode::Delete,
                    "Delete table column",
                    expected,
                )
            }
            TableEditOperation::MoveColumn { from, to } => {
                if !(*from >= 0 && *to >= 0 && *from < column_count && *to < column_count) {
                    return TableEditResult::fail(TableSourceFallback::InvalidColumn);
                }
                Self::column_mutation(
                    document,
                    table_range,
                    &records,
                    delimiter,
                    &row_indices,
                    *from,
                    ColumnMode::Move { to: *to },
                    "Move table column",
                    expected,
                )
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn column_mutation(
        document: &ParsedDocument,
        table_range: NSRange,
        records: &[LineRecord],
        delimiter: usize,
        row_indices: &[usize],
        column: isize,
        mode: ColumnMode<'_>,
        summary: &str,
        expected: String,
    ) -> TableEditResult {
        let mut output: Vec<String> = records.iter().map(LineRecord::raw_with_terminator).collect();
        for (row, &record_index) in row_indices.iter().enumerate() {
            let record = &records[record_index];
            let record_source = record.text_utf16.as_slice();
            let mut parts: Vec<String> =
                Self::cell_segments(record_source).into_iter().map(|segment| record_source.substring(segment)).collect();
            match &mode {
                ColumnMode::Insert { header, cells } => {
                    let value: &str = if row == 0 {
                        header
                    } else if row - 1 < cells.len() {
                        &cells[row - 1]
                    } else {
                        ""
                    };
                    let cell = Self::preserved_cell(value, parts.first().map_or("", String::as_str));
                    let at = (column as usize).min(parts.len());
                    parts.insert(at, cell);
                }
                ColumnMode::Delete => {
                    if !(column < parts.len() as isize) {
                        return TableEditResult::fail(TableSourceFallback::InvalidColumn);
                    }
                    parts.remove(column as usize);
                }
                ColumnMode::Move { to } => {
                    if !(column < parts.len() as isize && *to < parts.len() as isize) {
                        return TableEditResult::fail(TableSourceFallback::InvalidColumn);
                    }
                    let moved = parts.remove(column as usize);
                    parts.insert(*to as usize, moved);
                }
            }
            output[record_index] = Self::render_raw_row(&parts, &record.text, record_source) + &record.terminator;
        }
        let delimiter_record = &records[delimiter];
        let delimiter_source = delimiter_record.text_utf16.as_slice();
        let mut delimiter_values: Vec<String> =
            Self::cell_segments(delimiter_source).into_iter().map(|segment| delimiter_source.substring(segment)).collect();
        match &mode {
            ColumnMode::Insert { .. } => {
                let at = (column as usize).min(delimiter_values.len());
                delimiter_values.insert(at, "---".to_owned());
            }
            ColumnMode::Delete => {
                if !(column < delimiter_values.len() as isize) {
                    return TableEditResult::fail(TableSourceFallback::InvalidColumn);
                }
                delimiter_values.remove(column as usize);
            }
            ColumnMode::Move { to } => {
                if !(column < delimiter_values.len() as isize && *to < delimiter_values.len() as isize) {
                    return TableEditResult::fail(TableSourceFallback::InvalidColumn);
                }
                let moved = delimiter_values.remove(column as usize);
                delimiter_values.insert(*to as usize, moved);
            }
        }
        output[delimiter] =
            Self::render_raw_row(&delimiter_values, &delimiter_record.text, delimiter_source) + &delimiter_record.terminator;
        Self::make(document, table_range, output.concat(), summary, expected)
    }

    /// `makeWhole(_:range:records:replacementAt:inserted:summary:expected:)`.
    fn make_whole_inserting(
        document: &ParsedDocument,
        range: NSRange,
        records: &[LineRecord],
        replacement_at: usize,
        inserted: &[String],
        summary: &str,
        expected: String,
    ) -> TableEditResult {
        let ending = records.first().map_or_else(|| "\n".to_owned(), |record| record.terminator.clone());
        let mut output: Vec<String> = records.iter().map(LineRecord::raw_with_terminator).collect();
        // The last record of a file that does not end in a newline has an
        // empty terminator; inserting after it would glue the new line onto
        // its content, so give the preceding record a terminator first.
        if replacement_at > 0 && replacement_at <= output.len() && !Self::has_terminator(&output[replacement_at - 1]) {
            output[replacement_at - 1].push_str(&ending);
        }
        let lines: Vec<String> = inserted.iter().map(|line| format!("{line}{ending}")).collect();
        output.splice(replacement_at..replacement_at, lines);
        Self::make(document, range, output.concat(), summary, expected)
    }

    /// True when the last *code unit* of the line is a line terminator. A
    /// Character-wise `hasSuffix("\n")` is false for CR LF.
    fn has_terminator(line: &str) -> bool {
        matches!(line.chars().next_back(), Some('\n' | '\r'))
    }

    fn make(document: &ParsedDocument, range: NSRange, replacement: String, summary: &str, expected: String) -> TableEditResult {
        let source = document.utf16.as_slice();
        if !(range.location >= 0 && range.upper_bound() <= source.length() && swift_text::str_eq(&source.substring(range), &expected)) {
            return TableEditResult::fail(TableSourceFallback::SourceChanged);
        }
        TableEditResult::new(Some(TableEditProposal::new(range, replacement, summary, expected)), None)
    }

    fn line_records(source: &[u16], range: NSRange) -> Vec<LineRecord> {
        let mut records: Vec<LineRecord> = Vec::new();
        let mut offset = range.location;
        while offset < range.upper_bound() {
            let full_end = source.length().min(source.line_end_after(offset));
            let mut content_end = full_end;
            if content_end > offset && source.character_at(content_end - 1) == 0x0A {
                content_end -= 1;
            }
            if content_end > offset && source.character_at(content_end - 1) == 0x0D {
                content_end -= 1;
            }
            let bounded_content_end = content_end.min(range.upper_bound());
            let full = NSRange::new(offset, full_end.min(range.upper_bound()) - offset);
            let content = NSRange::new(offset, 0.max(bounded_content_end - offset));
            let text = source.substring(content);
            let terminator =
                source.substring(NSRange::new(content.upper_bound(), 0.max(full.upper_bound() - content.upper_bound())));
            let text_utf16 = utf16(&text);
            records.push(LineRecord { content_range: content, full_range: full, text, text_utf16, terminator });
            offset = full.upper_bound();
        }
        records
    }

    fn table_blocks(document: &ParsedDocument) -> Vec<(&MDBlock, &TableData)> {
        let mut found: Vec<(&MDBlock, &TableData)> = Vec::new();
        document.root.walk(&mut |block| {
            if let BlockContent::Table(data) = &block.content {
                found.push((block, data));
            }
        });
        found
    }

    fn same_table(lhs: &TableData, rhs: &TableData) -> bool {
        lhs.delimiter_range == rhs.delimiter_range && lhs.rows.iter().map(|row| row.range).eq(rhs.rows.iter().map(|row| row.range))
    }

    /// Cell ranges of one line (UTF-16), split at unescaped pipes.
    fn cell_segments(ns: &[u16]) -> Vec<NSRange> {
        let mut pipes: Vec<isize> = Vec::new();
        for index in 0..ns.length() {
            if ns.character_at(index) != 124 {
                continue;
            }
            let mut slashes = 0isize;
            let mut prior = index - 1;
            while prior >= 0 && ns.character_at(prior) == 92 {
                slashes += 1;
                prior -= 1;
            }
            if slashes % 2 == 0 {
                pipes.push(index);
            }
        }
        if pipes.is_empty() {
            return Vec::new();
        }
        let mut segments: Vec<NSRange> = Vec::new();
        if pipes[0] > 0 {
            let prefix = ns.substring(NSRange::new(0, pipes[0]));
            if !swift_text::all_satisfy(&prefix, |g| swift_text::char_is(g, ' ') || swift_text::char_is(g, '\t') || swift_text::char_is(g, '>'))
            {
                segments.push(NSRange::new(0, pipes[0]));
            }
        }
        for pair in pipes.windows(2) {
            let (left, right) = (pair[0], pair[1]);
            segments.push(NSRange::new(left + 1, 0.max(right - left - 1)));
        }
        let last = *pipes.last().unwrap();
        if last < ns.length() - 1 {
            segments.push(NSRange::new(last + 1, ns.length() - last - 1));
        }
        segments
    }

    fn replace_segment(raw: &[u16], segment: NSRange, content: &str) -> String {
        let old = raw.substring(segment);
        let (left, right) = whitespace_edges(&old);
        let replacement = format!("{left}{}{right}", Self::escape_cell(content));
        let mut output = raw.to_vec();
        output.splice(segment.as_usize_range(), replacement.encode_utf16());
        string_from_utf16(&output)
    }

    fn preserved_cell(value: &str, old: &str) -> String {
        let (left, right) = whitespace_edges(old);
        format!("{left}{}{right}", Self::escape_cell(value))
    }

    fn escape_cell(value: &str) -> String {
        swift_text::replacing_occurrences(&swift_text::replacing_occurrences(value, "\\", "\\\\"), "|", "\\|")
    }

    /// `template` and `source` are the same line (`String` and `NSString`).
    fn render_raw_row(parts: &[String], template: &str, source: &[u16]) -> String {
        let Some(first_pipe) = (0..source.length()).find(|&i| source.character_at(i) == 124) else {
            return template.to_owned();
        };
        let prefix = source.substring(NSRange::new(0, first_pipe));
        let has_leading_pipe =
            swift_text::all_satisfy(&prefix, |g| swift_text::char_is(g, ' ') || swift_text::char_is(g, '\t') || swift_text::char_is(g, '>'));
        // `template.lastIndex { !$0.isWhitespace }` and `template[$0] == "|"`.
        let has_trailing_pipe =
            swift_text::graphemes(template).rev().find(|g| !swift_text::is_whitespace(g)).is_some_and(|g| swift_text::char_is(g, '|'));
        let mut out = String::new();
        if has_leading_pipe {
            out.push_str(&prefix);
            out.push('|');
        }
        out.push_str(&parts.join("|"));
        if has_trailing_pipe {
            out.push('|');
        }
        out
    }

    /// A single inserted row has no column-width context, so alignment is not
    /// padded here — the exit-time §6.3 realign applies it to the whole table.
    fn render_row(cells: &[String], indent: &str) -> String {
        let mut out = format!("{indent}|");
        for cell in cells {
            out.push(' ');
            out.push_str(&Self::escape_cell(cell));
            out.push_str(" |");
        }
        out
    }

    fn delimiter_token(alignment: TableAlignment, old: &str) -> String {
        let trimmed = swift_text::trim_whitespaces(old);
        let dashes = swift_text::graphemes(trimmed).filter(|g| swift_text::char_is(g, '-')).count() as isize;
        let width = 3.max(dashes);
        let core = "-".repeat(width as usize);
        match alignment {
            TableAlignment::None => core,
            TableAlignment::Left => format!(":{}", swift_text::drop_first(&core, 1)),
            TableAlignment::Right => format!("{}:", swift_text::drop_last(&core, 1)),
            TableAlignment::Center => format!(":{}:", swift_text::drop_first(&core, 2)),
        }
    }

    fn indentation(line: &str) -> &str {
        let end = swift_text::first_index_where(line, |g| {
            !(swift_text::char_is(g, ' ') || swift_text::char_is(g, '\t') || swift_text::char_is(g, '>'))
        })
        .unwrap_or(line.len());
        &line[..end]
    }
}

/// `old.prefix { $0.isWhitespace }` and
/// `old.reversed().prefix { $0.isWhitespace }.reversed()`. An all-whitespace
/// cell yields itself for both, as in Swift.
fn whitespace_edges(old: &str) -> (&str, &str) {
    let left_end = swift_text::first_index_where(old, |g| !swift_text::is_whitespace(g)).unwrap_or(old.len());
    let mut right_start = old.len();
    for g in swift_text::graphemes(old).rev() {
        if !swift_text::is_whitespace(g) {
            break;
        }
        right_start -= g.len();
    }
    (&old[..left_end], &old[right_start..])
}

/// `Array.element(at:)`.
fn element_at<T: Copy>(items: &[T], index: isize) -> Option<T> {
    if index >= 0 && (index as usize) < items.len() { Some(items[index as usize]) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_segments_split_at_unescaped_pipes() {
        let segments = |s: &str| TableEditing::cell_segments(&utf16(s));
        assert_eq!(segments("| a | b\\|c |"), vec![NSRange::new(1, 3), NSRange::new(5, 6)]);
        assert_eq!(segments("> | a |"), vec![NSRange::new(3, 3)]);
        assert_eq!(segments("a | b"), vec![NSRange::new(0, 2), NSRange::new(3, 2)]);
        assert_eq!(segments("a \\\\| b"), vec![NSRange::new(0, 4), NSRange::new(5, 2)]);
        assert!(segments("no pipes").is_empty());
    }

    #[test]
    fn whitespace_edges_duplicate_an_all_blank_cell() {
        assert_eq!(whitespace_edges("  x "), ("  ", " "));
        assert_eq!(whitespace_edges("   "), ("   ", "   "));
        assert_eq!(whitespace_edges(""), ("", ""));
    }

    #[test]
    fn delimiter_tokens() {
        assert_eq!(TableEditing::delimiter_token(TableAlignment::Right, " --- "), "--:");
        assert_eq!(TableEditing::delimiter_token(TableAlignment::Center, ":-----:"), ":---:");
        assert_eq!(TableEditing::delimiter_token(TableAlignment::Left, "-"), ":--");
        assert_eq!(TableEditing::delimiter_token(TableAlignment::None, ":--:"), "---");
    }

    #[test]
    fn escape_cell_escapes_backslashes_then_pipes() {
        assert_eq!(TableEditing::escape_cell("a|b\\c"), "a\\|b\\\\c");
    }
}
