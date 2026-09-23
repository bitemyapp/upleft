//! TableFormatter.swift — table source formatting.
//!
//! Shared by §9.1's `tablePipes` tidy rule and §6.3's realign-on-exit, so the
//! two can never disagree (a disagreement shows up as an edit that
//! flip-flops forever).

use crate::model::{TableAlignment, TableData};
use crate::ns_range::NSRange;
use crate::swift_text::{self, ns::NSStringExt};

pub struct TableFormatter;

/// `TableFormatter.Model`: cell contents of a table, header row first. Read
/// from the *source* rather than the AST so that a table being edited
/// mid-keystroke still formats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub rows: Vec<Vec<String>>,
    pub alignments: Vec<TableAlignment>,
    pub indent: String,
    /// The line terminator that followed each source line, in render order
    /// (header, delimiter, body…). A mixed-ending file keeps the bytes it had
    /// instead of being rebuilt as LF; empty where unknown.
    pub line_terminators: Vec<String>,
}

impl Model {
    /// `Model(rows:alignments:indent:)`, with `lineTerminators` defaulted to `[]`.
    pub fn new(rows: Vec<Vec<String>>, alignments: Vec<TableAlignment>, indent: impl Into<String>) -> Model {
        Model { rows, alignments, indent: indent.into(), line_terminators: Vec::new() }
    }

    pub fn column_count(&self) -> isize {
        let widest = self.rows.iter().map(|row| row.len() as isize).max().unwrap_or(0);
        (self.alignments.len() as isize).max(widest)
    }
}

/// `String(s.prefix { … })` over Characters.
fn prefix_while(s: &str, mut predicate: impl FnMut(&str) -> bool) -> &str {
    let end = swift_text::first_index_where(s, |g| !predicate(g)).unwrap_or(s.len());
    &s[..end]
}

impl TableFormatter {
    /// `model(of:in:)`.
    pub fn model(table: &TableData, text: &[u16]) -> Model {
        // A table inside a blockquote carries its `>` markers before the row
        // range; they must survive a realign or the table silently leaves the
        // quote. Mirrors `TableEditing.indentation`.
        let indent = table
            .rows
            .first()
            .map(|row| {
                let start = text.line_start_before(row.range.location);
                let before = text.substring(NSRange::new(start, row.range.location - text.line_start_before(row.range.location)));
                prefix_while(&before, |g| swift_text::char_is(g, ' ') || swift_text::char_is(g, '\t') || swift_text::char_is(g, '>'))
                    .to_owned()
            })
            .unwrap_or_default();
        let rows = table
            .rows
            .iter()
            .map(|row| row.cells.iter().map(|cell| swift_text::trim_whitespaces(&text.substring(cell.range)).to_owned()).collect())
            .collect();
        Model {
            rows,
            alignments: table.alignments.clone(),
            indent,
            line_terminators: Self::line_terminators(table, text),
        }
    }

    /// One terminator per source line of the table, in order. A line without
    /// a terminator (the last one, in a file with no final newline) yields "".
    fn line_terminators(table: &TableData, text: &[u16]) -> Vec<String> {
        let range = Self::source_range(table, NSRange::new(0, 0));
        let mut terminators: Vec<String> = Vec::new();
        let mut offset = range.location;
        while offset < range.upper_bound() {
            let line_end = text.length().min(text.line_end_after(offset));
            let mut content_end = line_end;
            if content_end > offset && text.character_at(content_end - 1) == 0x0A {
                content_end -= 1;
            }
            if content_end > offset && text.character_at(content_end - 1) == 0x0D {
                content_end -= 1;
            }
            terminators.push(text.substring(NSRange::new(content_end, 0.max(line_end - content_end))));
            offset = line_end;
        }
        terminators
    }

    /// Renders a table back to aligned pipe syntax.
    pub fn render(model: &Model) -> String {
        let columns = model.column_count();
        if !(columns > 0 && !model.rows.is_empty()) {
            return String::new();
        }

        let mut widths: Vec<isize> = vec![3; columns as usize]; // `---` is the minimum
        for row in &model.rows {
            for (index, cell) in row.iter().enumerate() {
                if (index as isize) < columns {
                    widths[index] = widths[index].max(Self::display_width(cell));
                }
            }
        }

        let mut lines: Vec<String> = Vec::with_capacity(model.rows.len() + 1);
        lines.push(Self::render_row(&model.rows[0], &widths, &model.alignments, &model.indent));
        lines.push(Self::render_delimiter(&widths, &model.alignments, &model.indent));
        for row in model.rows.iter().skip(1) {
            lines.push(Self::render_row(row, &widths, &model.alignments, &model.indent));
        }
        let mut out = String::new();
        let count = lines.len();
        for (index, line) in lines.iter().enumerate() {
            out.push_str(line);
            if index < count - 1 {
                let captured = model.line_terminators.get(index).map(String::as_str).unwrap_or("");
                out.push_str(if captured.is_empty() { "\n" } else { captured });
            }
        }
        out
    }

    fn render_row(cells: &[String], widths: &[isize], alignments: &[TableAlignment], indent: &str) -> String {
        let mut out = String::from(indent);
        out.push('|');
        for (index, &width) in widths.iter().enumerate() {
            let cell = cells.get(index).map(String::as_str).unwrap_or("");
            let alignment = alignments.get(index).copied().unwrap_or(TableAlignment::None);
            out.push(' ');
            out.push_str(&Self::pad(cell, width, alignment));
            out.push_str(" |");
        }
        out
    }

    fn render_delimiter(widths: &[isize], alignments: &[TableAlignment], indent: &str) -> String {
        let mut out = String::from(indent);
        out.push('|');
        for (index, &width) in widths.iter().enumerate() {
            let alignment = alignments.get(index).copied().unwrap_or(TableAlignment::None);
            match alignment {
                TableAlignment::None => {
                    out.push(' ');
                    out.push_str(&repeating("-", width));
                    out.push_str(" |");
                }
                TableAlignment::Left => {
                    out.push_str(" :");
                    out.push_str(&repeating("-", width - 1));
                    out.push_str(" |");
                }
                TableAlignment::Right => {
                    out.push(' ');
                    out.push_str(&repeating("-", width - 1));
                    out.push_str(": |");
                }
                TableAlignment::Center => {
                    out.push_str(" :");
                    out.push_str(&repeating("-", width - 2));
                    out.push_str(": |");
                }
            }
        }
        out
    }

    fn pad(cell: &str, width: isize, alignment: TableAlignment) -> String {
        let slack = width - Self::display_width(cell);
        if slack <= 0 {
            return cell.to_owned();
        }
        match alignment {
            TableAlignment::Right => repeating(" ", slack) + cell,
            TableAlignment::Center => {
                let left = slack / 2;
                repeating(" ", left) + cell + &repeating(" ", slack - left)
            }
            TableAlignment::None | TableAlignment::Left => cell.to_owned() + &repeating(" ", slack),
        }
    }

    /// Character count, not UTF-16 length: a table aligned by code units looks
    /// wrong the moment a cell contains an emoji or an accented letter.
    fn display_width(cell: &str) -> isize {
        swift_text::count(cell) as isize
    }

    /// Source range covering the whole table including its delimiter row.
    pub fn source_range(table: &TableData, fallback: NSRange) -> NSRange {
        let Some(first) = table.rows.first().map(|row| row.range) else {
            return if table.delimiter_range.length > 0 { table.delimiter_range } else { fallback };
        };
        let mut range = first;
        for row in table.rows.iter().skip(1) {
            range = range.union(row.range);
        }
        if table.delimiter_range.length > 0 {
            range = range.union(table.delimiter_range);
        }
        range
    }
}

/// `String(repeating:count:)`, which traps on a negative count.
fn repeating(s: &str, count: isize) -> String {
    assert!(count >= 0, "Negative count not allowed");
    s.repeat(count as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{TableCell, TableRow};
    use crate::swift_text::ns::utf16;

    fn model(rows: &[&[&str]], alignments: &[TableAlignment]) -> Model {
        Model::new(rows.iter().map(|r| r.iter().map(|c| c.to_string()).collect()).collect(), alignments.to_vec(), "")
    }

    #[test]
    fn renders_aligned_columns() {
        let m = model(
            &[&["Name", "Count", "Notes"], &["a", "1", "x"], &["bbbb", "22", "yy"]],
            &[TableAlignment::Left, TableAlignment::Right, TableAlignment::Center],
        );
        assert_eq!(
            TableFormatter::render(&m),
            "| Name | Count | Notes |\n| :--- | ----: | :---: |\n| a    |     1 |   x   |\n| bbbb |    22 |  yy   |"
        );
    }

    #[test]
    fn widths_count_characters_not_utf16() {
        // A decomposed "é" is one Character of two scalars, a flag one
        // Character of four UTF-16 units, and CR LF one Character.
        let m = model(&[&["e\u{301}e\u{301}e\u{301}e\u{301}", "🇺🇸🇺🇸🇺🇸🇺🇸"], &["abcd", "a\r\nb"]], &[]);
        assert_eq!(
            TableFormatter::render(&m),
            "| e\u{301}e\u{301}e\u{301}e\u{301} | 🇺🇸🇺🇸🇺🇸🇺🇸 |\n| ---- | ---- |\n| abcd | a\r\nb  |"
        );
    }

    #[test]
    fn empty_model_renders_nothing() {
        assert_eq!(TableFormatter::render(&model(&[], &[TableAlignment::None])), "");
        assert_eq!(TableFormatter::render(&model(&[&[]], &[])), "");
    }

    #[test]
    fn keeps_captured_terminators_and_quote_indent() {
        let text = "> | a | b |\r\n> |---|---|\n> | 1 | 2 |";
        let ns = utf16(text);
        let cell = |loc: isize, len: isize| TableCell::new(NSRange::new(loc, len), NSRange::new(loc, len), Vec::new());
        let table = TableData::new(
            vec![
                TableRow::new(NSRange::new(2, 9), vec![cell(3, 3), cell(7, 3)], true),
                TableRow::new(NSRange::new(27, 9), vec![cell(28, 3), cell(32, 3)], false),
            ],
            vec![TableAlignment::None, TableAlignment::None],
            NSRange::new(15, 9),
        );
        let m = TableFormatter::model(&table, &ns);
        assert_eq!(m.indent, "> ");
        assert_eq!(m.rows, vec![vec!["a".to_string(), "b".to_string()], vec!["1".to_string(), "2".to_string()]]);
        assert_eq!(m.line_terminators, vec!["\r\n".to_string(), "\n".to_string(), String::new()]);
        assert_eq!(TableFormatter::render(&m), "> | a   | b   |\r\n> | --- | --- |\n> | 1   | 2   |");
        assert_eq!(TableFormatter::source_range(&table, NSRange::new(0, 0)), NSRange::new(2, 34));
    }
}
